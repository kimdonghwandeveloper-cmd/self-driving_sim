//! 노드 순서로 지정한 경로의 교차로 포함 코스 생성(Phase 4-2).
//!
//! 경로는 OSM 노드 ID 순서다(경로 탐색은 PLAN 4-3). 경로 중간의 연결 차수 3 이상 노드를 교차로로 보고,
//! 교차로마다 [`junction::junction_shape`]로 교차로 면과 모서리 원호를 만든다.
//! - 경로 조각: 교차로 사이(또는 경로 끝과 교차로 사이)의 경로를 각 교차로의 잘라 낼 거리만큼 잘라 도로 조각으로 만든다.
//! - 곁가지: 경로에 포함되지 않은 팔은 잘라 낸 곳부터 [`STUB_LENGTH_M`]만 만들고 끝을 벽으로 막는다.
//! - 연석: 도로 조각 양옆과 교차로 모서리에만 둔다. 팔이 열린 쪽에는 두지 않는다.
//!
//! 1차 판에서 거부하는 경우: 시작 노드가 교차로, 노드 재방문, 이웃한 팔 사이 각 [`MIN_ARM_GAP_DEG`] 미만,
//! 교차로 근처에서 팔이 굽음, 교차로끼리 너무 가까움, 일방통행(`oneway`) 팔(왕복 분리 도로 미지원),
//! 서로 다른 조각·교차로 면의 겹침, 같은 노드 ID에 다른 좌표. 형상 임시값은 모두 **실측이 아니다**.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;

use super::course::{Area, Collider, ColliderKind, CourseDef, Strip};
use super::junction::{self, ArmInput};
use super::map::{self, CURB_THICKNESS_M, LatLon, project};
use super::osm_graph::{OsmGraphWay, node_degrees};
use super::road_geom::{self, RoadSpec};

/// 교차로 모서리 원호 반경(m). 임시값.
pub const CORNER_RADIUS_M: f32 = 3.0;
/// 경로에 포함되지 않은 교차로 팔(곁가지)의 길이(m). 임시값.
pub const STUB_LENGTH_M: f32 = 10.0;
/// 이웃한 팔 사이 최소 각(deg). 이보다 좁으면 거부한다.
pub const MIN_ARM_GAP_DEG: f32 = 25.0;
/// 교차로에서 잘라 낼 거리까지 팔이 곧은 직선에서 벗어나도 되는 거리(m).
const ARM_STRAIGHT_TOLERANCE_M: f32 = 0.05;
/// 잘라 낸 뒤 도로 조각·곁가지에 남아야 하는 최소 길이(m).
const MIN_PIECE_LENGTH_M: f32 = 1.0;
/// 같은 노드 ID의 좌표가 이보다 다르면(deg) 자료 모순으로 거부한다.
const SAME_NODE_TOLERANCE_DEG: f64 = 1.0e-7;
/// 교차로를 건너는 안내선의 표본 수.
const GUIDE_SAMPLES: usize = 16;

/// 경로 코스 생성 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteCourse {
    /// 코스 정의.
    pub course: CourseDef,
    /// 경로를 따라가는 안내선(도로 조각 중심선 + 교차로를 건너는 곡선). 시험용 자동 주행과 이후 목표 표시에 쓴다.
    pub guide: Vec<Vec2>,
    /// 경로가 지나는 교차로 노드 ID(경로 순서).
    pub junctions: Vec<u64>,
}

/// 허용한 way로 만든 노드 연결 정보.
struct Graph<'a> {
    /// 노드 ID별 위경도.
    pos: BTreeMap<u64, LatLon>,
    /// 노드 ID별 이웃 (이웃 노드 ID, way). 이웃 하나에 way 하나만 둔다.
    adjacent: BTreeMap<u64, Vec<(u64, &'a OsmGraphWay)>>,
    /// 노드 연결 차수.
    degree: BTreeMap<u64, u32>,
}

impl<'a> Graph<'a> {
    /// 허용한 `highway` 종류의 way로 연결 정보를 만든다. 같은 노드 ID에 다른 좌표가 있으면 오류다.
    fn new(ways: &'a [OsmGraphWay], allowed: &BTreeSet<&str>) -> Result<Self, String> {
        let mut pos: BTreeMap<u64, LatLon> = BTreeMap::new();
        let mut adjacent: BTreeMap<u64, Vec<(u64, &OsmGraphWay)>> = BTreeMap::new();
        for way in ways
            .iter()
            .filter(|way| way.highway().is_some_and(|kind| allowed.contains(kind)))
        {
            for node in &way.nodes {
                if let Some(known) = pos.insert(node.id, node.pos)
                    && ((known.lat - node.pos.lat).abs() > SAME_NODE_TOLERANCE_DEG
                        || (known.lon - node.pos.lon).abs() > SAME_NODE_TOLERANCE_DEG)
                {
                    return Err(format!("node {} has conflicting coordinates", node.id));
                }
            }
            for pair in way.nodes.windows(2) {
                for (from, to) in [(pair[0].id, pair[1].id), (pair[1].id, pair[0].id)] {
                    let list = adjacent.entry(from).or_default();
                    if !list.iter().any(|(id, _)| *id == to) {
                        list.push((to, way));
                    }
                }
            }
        }
        Ok(Self {
            pos,
            adjacent,
            degree: node_degrees(ways, allowed),
        })
    }

    /// 노드가 교차로(차수 3 이상)인지 판정한다.
    fn is_junction(&self, id: u64) -> bool {
        self.degree.get(&id).is_some_and(|&d| d >= 3)
    }

    /// `from`에서 `to`로 가는 way를 찾는다. 이웃이 아니면 `None`.
    fn way_between(&self, from: u64, to: u64) -> Option<&'a OsmGraphWay> {
        self.adjacent
            .get(&from)?
            .iter()
            .find(|(id, _)| *id == to)
            .map(|(_, way)| *way)
    }

    /// 교차로 `junction`에서 이웃 `first`를 지나 같은 way를 따라 `length_m` 이상 또는 다음 교차로·way 끝까지 걷는다.
    ///
    /// 반환값은 교차로부터의 노드 ID 목록이다.
    fn walk(&self, junction: u64, first: u64, length_m: f32, origin: LatLon) -> Vec<u64> {
        let Some(way) = self.way_between(junction, first) else {
            return vec![junction];
        };
        let ids: Vec<u64> = way.nodes.iter().map(|n| n.id).collect();
        let (Some(at), Some(next)) = (
            ids.iter().position(|&id| id == junction),
            ids.iter().position(|&id| id == first),
        ) else {
            return vec![junction];
        };
        let forward = next > at;
        let mut walked = vec![junction];
        let mut length = 0.0;
        let mut index = at;
        loop {
            let following = if forward {
                index + 1
            } else {
                match index.checked_sub(1) {
                    Some(value) => value,
                    None => break,
                }
            };
            let Some(&id) = ids.get(following) else {
                break;
            };
            length += project(origin, self.pos[walked.last().unwrap()])
                .distance(project(origin, self.pos[&id]));
            walked.push(id);
            index = following;
            if self.is_junction(id) || length >= length_m {
                break;
            }
        }
        walked
    }
}

/// `oneway` 태그가 일방통행을 뜻하는지 판정한다.
fn is_oneway(way: &OsmGraphWay) -> bool {
    matches!(
        way.tags.get("oneway").map(String::as_str),
        Some("yes" | "1" | "-1" | "true" | "reversible")
    )
}

/// 폴리라인 앞에서 `start_m`, 뒤에서 `end_m`만큼 잘라 낸다. 남는 길이가 [`MIN_PIECE_LENGTH_M`]보다 짧으면 오류다.
fn trim(points: &[Vec2], start_m: f32, end_m: f32) -> Result<Vec<Vec2>, String> {
    let total = road_geom::polyline_length(points);
    if total - start_m - end_m < MIN_PIECE_LENGTH_M {
        return Err(format!(
            "road between intersections is {total:.1} m but {:.1} m is cut for the intersections: intersections are too close",
            start_m + end_m
        ));
    }
    let (first, _) = road_geom::point_at_distance(points, start_m).unwrap();
    let (last, _) = road_geom::point_at_distance(points, total - end_m).unwrap();
    let mut out = vec![first];
    let mut walked = 0.0;
    for pair in points.windows(2) {
        walked += pair[0].distance(pair[1]);
        if walked > start_m && walked < total - end_m {
            out.push(pair[1]);
        }
    }
    out.push(last);
    Ok(out)
}

/// 교차로를 건너는 안내선 점을 만든다(`from`은 제외하고 다음 조각 첫 점까지 포함).
///
/// 들어오는 방향은 `before → from`, 나가는 방향은 `next` 첫 구간이다. 두 방향의 직선이 교차로 중심 `center`에서
/// 만난다고 보고, 짧은 쪽 잘라 낸 거리를 접선 길이로 하는 원호(필렛)로 잇는다. 베지어처럼 가운데가 좁게 꺾이지 않아
/// 회전 반경이 접선 길이 / tan(θ/2)로 정해진다(90°에서 잘라 낸 거리와 같다). 거의 직진이면 직선으로 잇는다.
fn crossing_guide(from: Vec2, before: Vec2, center: Vec2, next: &[Vec2]) -> Vec<Vec2> {
    let to = next[0];
    let entry = (from - before).normalize();
    let exit = (next[1] - to).normalize();
    let turn = entry.perp_dot(exit).atan2(entry.dot(exit));
    let mut out = Vec::new();
    if turn.abs() < 1.0e-3 {
        out.extend((1..=GUIDE_SAMPLES).map(|n| from.lerp(to, n as f32 / GUIDE_SAMPLES as f32)));
        return out;
    }
    let tangent = from.distance(center).min(to.distance(center));
    let radius = tangent / (turn.abs() / 2.0).tan();
    let (start, end) = (center - entry * tangent, center + exit * tangent);
    // 원호 중심은 들어오는 방향에서 회전 쪽으로 반경만큼 떨어진 곳이다.
    let normal = Vec2::new(-entry.y, entry.x) * turn.signum();
    let pivot = start + normal * radius;
    if start.distance(from) > 1.0e-3 {
        out.push(start);
    }
    for n in 1..=GUIDE_SAMPLES {
        let angle = turn * n as f32 / GUIDE_SAMPLES as f32;
        out.push(pivot + Vec2::from_angle(angle).rotate(start - pivot));
    }
    if end.distance(to) > 1.0e-3 {
        out.push(to);
    }
    out
}

/// 교차로 하나의 생성 결과.
struct JunctionBuild {
    /// 이웃 노드 ID별 잘라 낼 거리(m).
    cut: BTreeMap<u64, f32>,
    /// 교차로 면.
    area: Area,
    /// 모서리 연석.
    curbs: Vec<Collider>,
    /// 곁가지 도로 조각.
    stubs: Vec<map::RoadPiece>,
}

/// 경로 노드 ID 순서와 저장한 그래프 자료로 교차로를 포함한 코스를 만든다.
///
/// 지역 좌표 원점은 경로 첫 노드다. `width_m`은 모든 도로의 훈련용 임시 폭이다.
pub fn build_route_course(
    name: &str,
    ways: &[OsmGraphWay],
    allowed: &BTreeSet<&str>,
    route: &[u64],
    width_m: f32,
    notices: Vec<String>,
) -> Result<RouteCourse, String> {
    map::check_width(width_m)?;
    let graph = Graph::new(ways, allowed)?;
    if route.len() < 2 {
        return Err("route needs at least 2 nodes".to_owned());
    }
    let mut seen = BTreeSet::new();
    for &id in route {
        if !graph.pos.contains_key(&id) {
            return Err(format!("route node {id} is not on an allowed road"));
        }
        if !seen.insert(id) {
            return Err(format!("route visits node {id} twice"));
        }
    }
    for pair in route.windows(2) {
        if graph.way_between(pair[0], pair[1]).is_none() {
            return Err(format!(
                "route nodes {} and {} are not connected by one road segment",
                pair[0], pair[1]
            ));
        }
    }
    if graph.is_junction(route[0]) {
        return Err(format!(
            "route starts at intersection node {}; start on a road segment",
            route[0]
        ));
    }
    let origin = graph.pos[&route[0]];
    let at = |id: u64| project(origin, graph.pos[&id]);
    let spec = RoadSpec {
        width_m,
        curb_thickness_m: CURB_THICKNESS_M,
        vehicle_min_radius_m: map::vehicle_min_radius_m(),
        arc_step_deg: road_geom::ARC_STEP_DEG,
    };

    // 교차로마다 면·모서리·곁가지를 만든다.
    let junction_indices: Vec<usize> = (1..route.len())
        .filter(|&i| graph.is_junction(route[i]))
        .collect();
    let mut builds: BTreeMap<usize, JunctionBuild> = BTreeMap::new();
    for &i in &junction_indices {
        let id = route[i];
        let route_neighbors: Vec<u64> = [i.checked_sub(1), Some(i + 1)]
            .into_iter()
            .flatten()
            .filter_map(|k| route.get(k).copied())
            .collect();
        builds.insert(
            i,
            build_junction(&graph, id, &route_neighbors, origin, &spec)?,
        );
    }

    // 경로 조각: 시작·교차로·끝으로 나눈다.
    let mut breaks = vec![0];
    breaks.extend(junction_indices.iter().copied());
    if *breaks.last().unwrap() != route.len() - 1 {
        breaks.push(route.len() - 1);
    }
    let mut strips: Vec<Strip> = Vec::new();
    let mut colliders: Vec<Collider> = Vec::new();
    let mut areas: Vec<Area> = Vec::new();
    // 겹침 검사용 (외곽 다각형, 붙어 있는 교차로 경로 위치).
    let mut outlines: Vec<(Vec<Vec2>, BTreeSet<usize>)> = Vec::new();
    let mut guide: Vec<Vec2> = Vec::new();
    let mut first_piece: Option<Vec<Vec2>> = None;
    for pair in breaks.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let points: Vec<Vec2> = route[a..=b].iter().map(|&id| at(id)).collect();
        let start_cut = builds.get(&a).map_or(0.0, |j| j.cut[&route[a + 1]]);
        let end_cut = builds.get(&b).map_or(0.0, |j| j.cut[&route[b - 1]]);
        let trimmed = trim(&points, start_cut, end_cut)?;
        let walls = [!builds.contains_key(&a), !builds.contains_key(&b)];
        let piece = map::road_piece(&trimmed, &spec, walls)?;
        // 교차로를 건너는 안내선: 앞 조각 끝과 이 조각 시작에 접하는 원호.
        if builds.contains_key(&a)
            && let [.., before, from] = guide[..]
        {
            let crossing = crossing_guide(from, before, at(route[a]), &piece.centerline);
            guide.extend(crossing);
            guide.extend(piece.centerline[1..].iter().copied());
        } else {
            guide.extend(piece.centerline.iter().copied());
        }
        if first_piece.is_none() {
            first_piece = Some(piece.centerline.clone());
        }
        let attached: BTreeSet<usize> = [a, b]
            .into_iter()
            .filter(|k| builds.contains_key(k))
            .collect();
        outlines.push((piece.outline, attached));
        strips.extend(piece.strips);
        colliders.extend(piece.colliders);
    }
    let mut junction_ids = Vec::new();
    for (i, build) in builds {
        junction_ids.push(route[i]);
        let attached: BTreeSet<usize> = [i].into_iter().collect();
        outlines.push((build.area.boundary.clone(), attached.clone()));
        for stub in build.stubs {
            outlines.push((stub.outline, attached.clone()));
            strips.extend(stub.strips);
            colliders.extend(stub.colliders);
        }
        areas.push(build.area);
        colliders.extend(build.curbs);
    }
    check_outline_overlaps(&outlines)?;
    let start = map::start_pose(&first_piece.unwrap())?;
    let course = map::finish_course(name, notices, start, strips, areas, colliders)?;
    Ok(RouteCourse {
        course,
        guide,
        junctions: junction_ids,
    })
}

/// 교차로 노드 `id`의 면·모서리 연석·곁가지를 만든다. `route_neighbors`는 경로가 지나는 이웃 노드다.
fn build_junction(
    graph: &Graph,
    id: u64,
    route_neighbors: &[u64],
    origin: LatLon,
    spec: &RoadSpec,
) -> Result<JunctionBuild, String> {
    let center = project(origin, graph.pos[&id]);
    let neighbors = &graph.adjacent[&id];
    let half = spec.width_m / 2.0;
    let mut arms = Vec::with_capacity(neighbors.len());
    let mut polylines = Vec::with_capacity(neighbors.len());
    for &(neighbor, way) in neighbors {
        if is_oneway(way) {
            return Err(format!(
                "intersection {id}: way {} is one-way; divided or one-way roads are not supported yet",
                way.id
            ));
        }
        let walked = graph.walk(id, neighbor, 80.0, origin);
        let points: Vec<Vec2> = walked
            .iter()
            .map(|&n| project(origin, graph.pos[&n]))
            .collect();
        arms.push(ArmInput {
            direction: (points[1] - points[0]).normalize(),
            half_width_m: half,
        });
        polylines.push(points);
    }
    let shape = junction::junction_shape(
        center,
        &arms,
        CORNER_RADIUS_M,
        spec.arc_step_deg,
        MIN_ARM_GAP_DEG,
    )?;
    let mut cut = BTreeMap::new();
    let mut stubs = Vec::new();
    for (k, &(neighbor, _)) in neighbors.iter().enumerate() {
        let (points, arm, cut_m) = (&polylines[k], arms[k], shape.cut_m[k]);
        // 잘라 낼 거리까지는 팔이 곧아야 도로 조각이 교차로 면 입구와 정확히 맞는다.
        let check = cut_m + 0.5;
        let length = road_geom::polyline_length(points);
        if length < cut_m + MIN_PIECE_LENGTH_M {
            return Err(format!(
                "intersection {id}: road toward node {neighbor} is {length:.1} m, shorter than the {cut_m:.1} m cut (intersections too close or dead end)"
            ));
        }
        let mut sample = 0.0;
        while sample <= check.min(length) {
            let (p, _) = road_geom::point_at_distance(points, sample).unwrap();
            let off = (p - center).perp_dot(arm.direction).abs();
            if off > ARM_STRAIGHT_TOLERANCE_M {
                return Err(format!(
                    "intersection {id}: road toward node {neighbor} bends within {check:.1} m of the intersection"
                ));
            }
            sample += 0.25;
        }
        cut.insert(neighbor, cut_m);
        if !route_neighbors.contains(&neighbor) {
            let end = (cut_m + STUB_LENGTH_M).min(length);
            let stub_points = trim(points, cut_m, length - end)?;
            stubs.push(map::road_piece(&stub_points, spec, [false, true])?);
        }
    }
    let curbs = shape
        .corners
        .iter()
        .flat_map(|corner| road_geom::side_curb_rects(corner, 0.0, spec.curb_thickness_m, -1.0))
        .map(|rect| Collider {
            rect,
            height_m: 0.25,
            kind: ColliderKind::Curb,
        })
        .collect();
    Ok(JunctionBuild {
        cut,
        area: Area {
            center,
            boundary: shape.boundary,
        },
        curbs,
        stubs,
    })
}

/// 서로 다른 교차로에 붙은 조각·교차로 면의 외곽 다각형끼리 닿거나 교차하면 거부한다.
///
/// 같은 교차로에 붙은 것끼리는 그 교차로 입구에서 맞닿도록 만들었으므로 검사하지 않는다.
fn check_outline_overlaps(outlines: &[(Vec<Vec2>, BTreeSet<usize>)]) -> Result<(), String> {
    let bounds = |polygon: &[Vec2]| {
        polygon.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(low, high), p| (low.min(*p), high.max(*p)),
        )
    };
    for (index, (a, attached_a)) in outlines.iter().enumerate() {
        let (low_a, high_a) = bounds(a);
        for (b, attached_b) in &outlines[index + 1..] {
            if !attached_a.is_disjoint(attached_b) {
                continue;
            }
            let (low_b, high_b) = bounds(b);
            if low_a.cmpgt(high_b).any() || low_b.cmpgt(high_a).any() {
                continue;
            }
            for k in 0..a.len() {
                let edge_a = (a[k], a[(k + 1) % a.len()]);
                for m in 0..b.len() {
                    let edge_b = (b[m], b[(m + 1) % b.len()]);
                    if road_geom::segments_touch(edge_a, edge_b) {
                        let at = (edge_a.0 + edge_a.1) / 2.0;
                        return Err(format!(
                            "roads overlap near ({:.1}, {:.1}) without a shared intersection node",
                            at.x, at.y
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::{fixtures, parse_overpass_graph};
    use crate::world::test_drive::follow_path;

    /// 시험용 허용 `highway` 종류.
    fn allowed() -> BTreeSet<&'static str> {
        [
            "service",
            "pedestrian",
            "residential",
            "unclassified",
            "tertiary",
        ]
        .into_iter()
        .collect()
    }

    /// 합성 자료를 그래프로 읽는다.
    fn parse(ways: Vec<fixtures::FixtureWay>) -> Vec<OsmGraphWay> {
        parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap()
    }

    /// 합성 3·4갈래 교차로에서 직진·회전 경로가 만들어지고, 교차로 면 1개·곁가지가 생기며,
    /// 경로 추종 조향기가 교차로를 지나 충돌 없이 끝 부근까지 가는지 확인한다(PLAN 4-2 합성 완료 기준).
    #[test]
    fn synthetic_junctions_are_drivable() {
        for (name, ways, route, stubs) in [
            ("three-turn", fixtures::three_way(), vec![1, 10, 3], 1),
            ("three-turn-east", fixtures::three_way(), vec![2, 10, 3], 1),
            ("three-straight", fixtures::three_way(), vec![1, 10, 2], 1),
            ("four-turn", fixtures::four_way(), vec![1, 10, 4], 2),
            ("four-turn-other", fixtures::four_way(), vec![1, 10, 3], 2),
            ("four-straight", fixtures::four_way(), vec![1, 10, 2], 2),
        ] {
            let ways = parse(ways);
            let built = build_route_course(name, &ways, &allowed(), &route, 4.0, Vec::new())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(built.junctions, vec![10], "{name}");
            assert_eq!(built.course.areas.len(), 1, "{name}");
            // 경로 조각 2개 + 곁가지마다 노면·표시선 띠 3개.
            assert_eq!(built.course.strips.len(), 3 * (2 + stubs), "{name}");
            let result = follow_path(&built.course, &built.guide, 3.0);
            assert_eq!(result.impacts, 0, "{name}: {result:?}");
            assert!(result.remaining_m < 12.5, "{name}: {result:?}");
            assert!(result.max_offset_m < 1.2, "{name}: {result:?}");
        }
    }

    /// 교차로 안내선이 90° 회전에서 반경 = 잘라 낸 거리인 원호이고, 직진에서는 직선인지 확인한다.
    #[test]
    fn crossing_guide_is_tangent_arc() {
        let center = Vec2::new(50.0, 0.0);
        let points = crossing_guide(
            Vec2::new(45.0, 0.0),
            Vec2::new(40.0, 0.0),
            center,
            &[Vec2::new(50.0, 5.0), Vec2::new(50.0, 10.0)],
        );
        let pivot = Vec2::new(45.0, 5.0);
        for p in &points {
            assert!((p.distance(pivot) - 5.0).abs() < 1e-3, "{p}");
        }
        assert!(points.last().unwrap().distance(Vec2::new(50.0, 5.0)) < 1e-3);
        let straight = crossing_guide(
            Vec2::new(45.0, 0.0),
            Vec2::new(40.0, 0.0),
            center,
            &[Vec2::new(55.0, 0.0), Vec2::new(60.0, 0.0)],
        );
        assert!(straight.iter().all(|p| p.y.abs() < 1e-5));
    }

    /// 좌표만 겹치는 십자는 교차로가 아니므로 교차로 없는 직선 코스가 되는지 확인한다.
    #[test]
    fn crossing_without_shared_node_is_not_a_junction() {
        let ways = parse(fixtures::crossing_without_shared_node());
        let built =
            build_route_course("cross", &ways, &allowed(), &[1, 10, 2], 4.0, Vec::new()).unwrap();
        assert!(built.junctions.is_empty() && built.course.areas.is_empty());
    }

    /// 거부 사례: 좁은 팔 사이 각, 가까운 교차로, 일방통행 팔, 교차로에서 시작, 끊긴 경로, 노드 재방문,
    /// 같은 노드 ID에 다른 좌표(PLAN 4-2 합성 거부 기준).
    #[test]
    fn junction_rejections() {
        let cases: Vec<(&str, Vec<fixtures::FixtureWay>, Vec<u64>, &str)> = vec![
            ("narrow", fixtures::narrow_arms(), vec![1, 10, 3], "apart"),
            (
                "close",
                fixtures::close_junctions(),
                vec![1, 10, 11, 2],
                "too close",
            ),
            ("oneway", fixtures::oneway_arm(), vec![1, 10, 2], "one-way"),
            (
                "start",
                fixtures::three_way(),
                vec![10, 2],
                "starts at intersection",
            ),
            ("gap", fixtures::three_way(), vec![1, 2], "not connected"),
            ("revisit", fixtures::three_way(), vec![1, 10, 1], "twice"),
        ];
        for (name, ways, route, expected) in cases {
            let ways = parse(ways);
            let error =
                build_route_course(name, &ways, &allowed(), &route, 4.0, Vec::new()).unwrap_err();
            assert!(error.contains(expected), "{name}: {error}");
        }
        let mut ways = parse(fixtures::three_way());
        ways[1].nodes[0].pos.lat += 1.0e-5;
        let error = build_route_course("conflict", &ways, &allowed(), &[1, 10, 3], 4.0, Vec::new())
            .unwrap_err();
        assert!(error.contains("conflicting"), "{error}");
    }

    /// 교차로 면 경계가 노면 띠 입구 점과 맞닿고(틈 없음), 모서리 연석이 면 안으로 들어오지 않는지 확인한다.
    #[test]
    fn junction_meets_road_pieces() {
        let ways = parse(fixtures::four_way());
        let built =
            build_route_course("four", &ways, &allowed(), &[1, 10, 4], 4.0, Vec::new()).unwrap();
        let area = &built.course.areas[0];
        // 각 노면 띠의 교차로 쪽 끝 두 점이 모두 교차로 면 경계 꼭짓점과 일치한다.
        let roads: Vec<&Strip> = built
            .course
            .strips
            .iter()
            .filter(|s| s.kind == crate::world::course::StripKind::Road)
            .collect();
        for strip in roads {
            let ends = [
                (strip.left[0], strip.right[0]),
                (*strip.left.last().unwrap(), *strip.right.last().unwrap()),
            ];
            let near = ends
                .iter()
                .find(|(l, _)| l.distance(area.center) < 10.0)
                .expect("road end at junction");
            for p in [near.0, near.1] {
                assert!(
                    area.boundary.iter().any(|q| q.distance(p) < 1e-3),
                    "{p} not on junction boundary"
                );
            }
        }
    }

    /// 실제 스냅샷: 굽은 way 529495041을 따라 4갈래 교차로(5143358021)를 직진하고 3갈래 교차로(5143358070)에서
    /// way 532526349로 꺾어 3갈래 교차로(5239572002)에서 끝나는 경로를 만들고, 경로 추종 조향기가 충돌 없이
    /// 끝 부근까지 가는지 확인한다(PLAN 4-2 실제 스냅샷 완료 기준).
    #[test]
    fn real_route_through_junctions_is_drivable() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/maps/yeosu-expo/graph-2026-11-06.osm.json");
        let ways = parse_overpass_graph(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ids = |id: u64| -> Vec<u64> {
            ways.iter()
                .find(|w| w.id == id)
                .unwrap()
                .nodes
                .iter()
                .map(|n| n.id)
                .collect()
        };
        // 첫 노드 13388325412는 일방통행 tertiary way 1459775353과 만나는 교차로라 두 번째 노드부터 시작한다.
        let mut route = ids(529_495_041)[1..].to_vec();
        let tail = ids(532_526_349);
        let turn = tail.iter().position(|&n| n == 5_143_358_070).unwrap();
        let end = tail.iter().position(|&n| n == 5_239_572_002).unwrap();
        assert_eq!(*route.last().unwrap(), 5_143_358_070);
        route.extend_from_slice(&tail[turn + 1..=end]);
        let built = build_route_course("real", &ways, &allowed(), &route, 4.0, Vec::new()).unwrap();
        for junction in [5_143_358_021, 5_143_358_070, 5_239_572_002] {
            assert!(built.junctions.contains(&junction), "{:?}", built.junctions);
        }
        let result = follow_path(&built.course, &built.guide, 4.0);
        assert_eq!(result.impacts, 0, "{result:?}");
        assert!(result.remaining_m < 12.5, "{result:?}");
        assert!(result.max_offset_m < 1.2, "{result:?}");
    }
}
