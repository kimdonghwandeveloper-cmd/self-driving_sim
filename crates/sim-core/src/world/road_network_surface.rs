//! 같은 OSM 접속점에 속하는 노면의 외곽과 내부 연석 절단. 좌표가 겹친 다른 도로는 합치지 않는다.
use super::{BlockSurface, NetworkJoint};
use crate::world::{course::Collider, road_blocks::RoadBlock};
use glam::{DVec2, Vec2};
use std::collections::{BTreeMap, BTreeSet};

/// 원래 교차로와 블록을 구분해 좌표만 겹친 도로의 연석은 유지한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Owner {
    Block(usize),
    Joint(u64),
}

/// 원점을 포함하는 노면들의 정확한 방사 외곽을 꼭짓점·변 교차점의 방위에서 계산한다.
/// 볼록 껍질로 빈 공간을 채우지 않으며 별 모양 성분에 구멍을 만들지 않는다.
pub(super) fn union_boundary(polygons: &[Vec<Vec2>]) -> Result<Vec<Vec2>, String> {
    if polygons.is_empty()
        || polygons
            .iter()
            .any(|p| p.len() < 3 || p.iter().any(|v| !v.is_finite()))
    {
        return Err("invalid source junction footprint".into());
    }
    for polygon in polygons {
        let values: Vec<_> = (0..polygon.len())
            .map(|i| {
                polygon[i]
                    .as_dvec2()
                    .perp_dot(polygon[(i + 1) % polygon.len()].as_dvec2())
            })
            .collect();
        let area: f64 = values.iter().sum();
        if area.abs() < 1e-9 || values.iter().any(|v| v * area.signum() < -1e-7) {
            return Err("source junction footprint is not radial about its original node".into());
        }
    }
    let mut edges: Vec<_> = polygons
        .iter()
        .flat_map(|p| (0..p.len()).map(|i| (p[i].as_dvec2(), p[(i + 1) % p.len()].as_dvec2())))
        .collect();
    edges.sort_by(|(a, b), (c, d)| {
        a.x.total_cmp(&c.x)
            .then_with(|| a.y.total_cmp(&c.y))
            .then_with(|| b.x.total_cmp(&d.x))
            .then_with(|| b.y.total_cmp(&d.y))
    });
    let mut angles = Vec::new();
    for &(a, _) in &edges {
        if a.length_squared() > 1e-14 {
            angles.push(a.y.atan2(a.x).rem_euclid(std::f64::consts::TAU));
        }
    }
    for (i, &(a, b)) in edges.iter().enumerate() {
        for &(c, d) in &edges[i + 1..] {
            if let Some((t, _)) = crossing(a, b, c, d) {
                let p = a + (b - a) * t;
                if p.length_squared() > 1e-14 {
                    angles.push(p.y.atan2(p.x).rem_euclid(std::f64::consts::TAU));
                }
            }
        }
    }
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
    let mut boundary = Vec::new();
    for angle in angles {
        let direction = DVec2::new(angle.cos(), angle.sin());
        let mut radius = 0.0_f64;
        for &(a, b) in &edges {
            let edge = b - a;
            let den = direction.perp_dot(edge);
            if den.abs() < 1e-12 {
                continue;
            }
            let r = a.perp_dot(edge) / den;
            let t = a.perp_dot(direction) / den;
            if r >= 0. && (-1e-9..=1. + 1e-9).contains(&t) {
                radius = radius.max(r);
            }
        }
        if radius <= 0. {
            return Err("junction footprint union is not connected around its source node".into());
        }
        let p = (direction * radius).as_vec2();
        if boundary
            .last()
            .is_none_or(|last: &Vec2| last.distance(p) > 1e-5)
        {
            boundary.push(p);
        }
    }
    if boundary.len() > 1 && boundary[0].distance(*boundary.last().unwrap()) < 1e-5 {
        boundary.pop();
    }
    Ok(boundary)
}

/// 선분 교차 매개변수를 f64로 구한다. 일직선 겹침은 끝점 방위 또는 포함 검사에서 다룬다.
fn crossing(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<(f64, f64)> {
    let ab = b - a;
    let cd = d - c;
    let den = ab.perp_dot(cd);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (c - a).perp_dot(cd) / den;
    let u = (c - a).perp_dot(ab) / den;
    ((-1e-9..=1. + 1e-9).contains(&t) && (-1e-9..=1. + 1e-9).contains(&u))
        .then_some((t.clamp(0., 1.), u.clamp(0., 1.)))
}

/// 연결 관계와 외곽 검사에 사용할 원본 노면을 보관한다.
struct Surface {
    owner: Owner,
    polygon: Vec<Vec2>,
    low: Vec2,
    high: Vec2,
}
impl Surface {
    /// 노면의 최소 사각형을 미리 계산해 멀리 있는 변을 검사하지 않는다.
    fn new(owner: Owner, polygon: Vec<Vec2>) -> Self {
        let low = polygon
            .iter()
            .copied()
            .fold(Vec2::splat(f32::INFINITY), Vec2::min);
        let high = polygon
            .iter()
            .copied()
            .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
        Self {
            owner,
            polygon,
            low,
            high,
        }
    }
    /// 선분과 노면 경계 상자가 닿을 수 있을 때만 실제 교차를 검사한다.
    fn meets(&self, a: Vec2, b: Vec2) -> bool {
        a.min(b).cmple(self.high).all() && a.max(b).cmpge(self.low).all()
    }
}
/// 짝수/홀수 교차 규칙으로 단순 노면 내부를 판정한다. 큰 지도 좌표에서도 차분은 f64로 계산한다.
fn inside(point: DVec2, polygon: &[Vec2]) -> bool {
    let mut result = false;
    for i in 0..polygon.len() {
        let a = polygon[i].as_dvec2();
        let b = polygon[(i + 1) % polygon.len()].as_dvec2();
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            result = !result;
        }
    }
    result
}
/// 공통 원본 노드 또는 블록을 가진 노면 안에 덮인 연석 조각만 제거한다.
/// 노면은 합치거나 이동하지 않고 충돌 상자만 실제 노면 외곽에 맞춰 분할한다.
pub(super) fn exterior_curbs(
    raw: Vec<(Collider, Owner)>,
    blocks: &[BlockSurface],
    joints: &[NetworkJoint],
    source: &[&RoadBlock],
) -> Vec<Collider> {
    let mut surfaces = Vec::new();
    let mut by_node: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let mut by_block: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, block) in blocks.iter().enumerate() {
        let polygons = block
            .strips
            .iter()
            .map(|s| s.left.iter().chain(s.right.iter().rev()).copied().collect())
            .chain(block.areas.iter().map(|a| a.boundary.clone()));
        for poly in polygons {
            let id = surfaces.len();
            surfaces.push(Surface::new(Owner::Block(index), poly));
            for node in [
                source[index].nodes[0].id,
                source[index].nodes.last().unwrap().id,
            ] {
                by_node.entry(node).or_default().push(id);
            }
            by_block.entry(index).or_default().push(id);
        }
    }
    for joint in joints {
        let id = surfaces.len();
        surfaces.push(Surface::new(
            Owner::Joint(joint.node_id),
            joint.boundary.clone(),
        ));
        by_node.entry(joint.node_id).or_default().push(id);
        for (i, b) in source.iter().enumerate() {
            if b.nodes[0].id == joint.node_id || b.nodes.last().unwrap().id == joint.node_id {
                by_block.entry(i).or_default().push(id);
            }
        }
    }
    let owners: BTreeMap<_, _> = surfaces
        .iter()
        .enumerate()
        .map(|(i, s)| (s.owner, i))
        .collect();
    let mut neighbors = vec![BTreeSet::new(); surfaces.len()];
    for group in by_node.values().chain(by_block.values()) {
        for &i in group {
            for &j in group {
                neighbors[i].insert(j);
            }
        }
    }
    let mut output = Vec::new();
    for (collider, owner) in raw {
        let (sin, cos) = collider.rect.heading_rad.sin_cos();
        let direction = Vec2::new(-sin, -cos);
        let half = direction * collider.rect.half_size.y;
        let (a, b) = (collider.rect.center - half, collider.rect.center + half);
        let candidates: Vec<_> = neighbors[owners[&owner]]
            .iter()
            .copied()
            .filter(|&i| surfaces[i].meets(a, b))
            .collect();
        if candidates.is_empty() {
            output.push(collider);
            continue;
        }
        let mut stops = vec![0., 1.];
        for &i in &candidates {
            let poly = &surfaces[i].polygon;
            for k in 0..poly.len() {
                if let Some((t, _)) = crossing(
                    a.as_dvec2(),
                    b.as_dvec2(),
                    poly[k].as_dvec2(),
                    poly[(k + 1) % poly.len()].as_dvec2(),
                ) {
                    stops.push(t);
                }
            }
        }
        stops.sort_by(f64::total_cmp);
        stops.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        let mut kept = Vec::<(f64, f64)>::new();
        for pair in stops.windows(2) {
            let mid = a.as_dvec2() + (b - a).as_dvec2() * (pair[0] + pair[1]) / 2.;
            if candidates
                .iter()
                .any(|&i| inside(mid, &surfaces[i].polygon))
            {
                continue;
            }
            if let Some(last) = kept
                .last_mut()
                .filter(|last| (last.1 - pair[0]).abs() < 1e-8)
            {
                last.1 = pair[1];
            } else {
                kept.push((pair[0], pair[1]));
            }
        }
        for (start, end) in kept {
            let length = a.distance(b) * (end - start) as f32;
            if length < 1e-4 {
                continue;
            }
            let mut piece = collider;
            piece.rect.center = a + (b - a) * ((start + end) / 2.) as f32;
            piece.rect.half_size.y = length / 2.;
            output.push(piece);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 십자형 합집합은 76 m²이며 볼록 껍질처럼 네 모서리의 빈 공간을 채우면 안 된다.
    #[test]
    fn radial_union_preserves_concave_gaps_and_order() {
        let rectangle = [
            Vec2::new(-10., -1.),
            Vec2::new(10., -1.),
            Vec2::new(10., 1.),
            Vec2::new(-10., 1.),
        ];
        for angle in 0..90 {
            let rotation = Vec2::from_angle((angle as f32).to_radians());
            let a: Vec<_> = rectangle.iter().map(|&v| rotation.rotate(v)).collect();
            let b: Vec<_> = a.iter().map(|&v| Vec2::new(-v.y, v.x)).collect();
            let boundary = union_boundary(&[a.clone(), b.clone()]).unwrap();
            let area = (0..boundary.len())
                .map(|i| {
                    boundary[i]
                        .as_dvec2()
                        .perp_dot(boundary[(i + 1) % boundary.len()].as_dvec2())
                        / 2.
                })
                .sum::<f64>();
            assert!((area - 76.).abs() < 0.001, "{area}");
            assert!(!inside(
                rotation.rotate(Vec2::new(5., 5.)).as_dvec2(),
                &boundary
            ));
            let reordered = union_boundary(&[b, a]).unwrap();
            assert_eq!(boundary, reordered);
        }
    }
    /// 빈 입력과 비유한·퇴화된 다각형으로 임의의 교차로 면을 만들지 않는다.
    #[test]
    fn radial_union_rejects_invalid_footprints() {
        assert!(union_boundary(&[]).is_err());
        assert!(union_boundary(&[vec![Vec2::ZERO, Vec2::X, Vec2::new(2., 0.)]]).is_err());
        assert!(union_boundary(&[vec![Vec2::ZERO, Vec2::X, Vec2::NAN]]).is_err());
    }
}
