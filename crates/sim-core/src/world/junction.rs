//! 교차로 면 형상(Phase 4-2). 순수 기하 계산만 한다.
//!
//! 교차로 중심에서 뻗은 팔(진행 방향 단위 벡터와 반폭)을 각도순으로 정렬하고, 이웃한 두 팔 사이마다
//! 모서리를 만든다.
//! - 두 팔 사이 각 φ가 180°보다 작으면: 두 팔의 마주 보는 가장자리 선을 반경 r의 원호(모서리 원호)로 잇는다.
//!   원호 접점까지가 그 팔을 잘라 낼 최소 거리다.
//! - φ가 180° 이상이면(곧은 길 옆에 붙은 T자의 반대쪽 등): 두 가장자리를 교차로 중심 둘레의 원호(반경 = 반폭)나
//!   직선으로 잇는다.
//!
//! 각 팔의 잘라 낼 거리는 PLAN 기준 `다른 팔의 최대 반폭 + 모서리 반경`과 두 모서리 원호 접점 거리 중 큰 값이다.
//! 교차로 면은 중심에서 본 별 모양(star-shaped) 다각형이어야 하며, 부채꼴 삼각형으로 그린다.
//! 좌표는 지면 (x, z) m이고, 각도는 `atan2(z, x)`가 커지는 방향(수학적 반시계)으로 잰다.

use std::f32::consts::{PI, TAU};

use glam::Vec2;

/// 교차로 팔 하나.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmInput {
    /// 교차로 중심에서 팔 쪽으로 향하는 단위 벡터.
    pub direction: Vec2,
    /// 팔 도로의 반폭(m).
    pub half_width_m: f32,
}

/// 교차로 면 생성 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionShape {
    /// 입력 순서대로 각 팔을 중심에서 잘라 낼 거리(m). 팔 도로는 이 거리에서 시작한다.
    pub cut_m: Vec<f32>,
    /// 교차로 면 경계(반시계 순서, 닫힌 다각형의 꼭짓점). 노면 가장자리(반폭) 기준이다.
    pub boundary: Vec<Vec2>,
    /// 이웃한 팔 사이 모서리 경계선(반시계 진행). 연석을 이 선 바깥(진행 방향 왼쪽)에 둔다.
    pub corners: Vec<Vec<Vec2>>,
}

/// 벡터를 +90°(반시계) 돌린다.
fn ccw(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// 2차원 외적 `a × b`.
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// 중심 `center`의 교차로 면을 만든다.
///
/// `corner_radius_m`은 모서리 원호 반경, `arc_step_deg`는 원호 분할 각도, `min_gap_deg`는 이웃한 팔 사이 최소 각이다.
/// 팔이 3개 미만이거나, 이웃한 팔 사이 각이 `min_gap_deg`보다 작거나, 면이 별 모양이 아니면 오류다.
pub fn junction_shape(
    center: Vec2,
    arms: &[ArmInput],
    corner_radius_m: f32,
    arc_step_deg: f32,
    min_gap_deg: f32,
) -> Result<JunctionShape, String> {
    if arms.len() < 3 {
        return Err(format!("junction at {center} has fewer than 3 arms"));
    }
    if !(corner_radius_m.is_finite() && corner_radius_m > 0.0) {
        return Err(format!(
            "corner radius must be positive, got {corner_radius_m}"
        ));
    }
    let angle = |v: Vec2| v.y.atan2(v.x).rem_euclid(TAU);
    let mut order: Vec<usize> = (0..arms.len()).collect();
    order.sort_by(|&a, &b| angle(arms[a].direction).total_cmp(&angle(arms[b].direction)));
    let count = order.len();
    // PLAN 기준값: 다른 팔의 최대 반폭 + 모서리 반경.
    let mut cut: Vec<f32> = (0..arms.len())
        .map(|i| {
            let other = arms
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, arm)| arm.half_width_m)
                .fold(0.0f32, f32::max);
            other + corner_radius_m
        })
        .collect();
    let step = arc_step_deg.to_radians();
    // 모서리별 (앞 팔 위의 시작 거리, 원호/잇는 선, 뒤 팔 위의 끝 거리).
    let mut corner_paths = Vec::with_capacity(count);
    for k in 0..count {
        let (i, j) = (order[k], order[(k + 1) % count]);
        let (a, b) = (arms[i], arms[j]);
        let gap = (angle(b.direction) - angle(a.direction)).rem_euclid(TAU);
        if gap.to_degrees() < min_gap_deg {
            return Err(format!(
                "junction at ({:.1}, {:.1}): arms are only {:.0} deg apart (minimum {min_gap_deg} deg)",
                center.x,
                center.y,
                gap.to_degrees()
            ));
        }
        // 앞 팔의 반시계쪽 가장자리, 뒤 팔의 시계쪽 가장자리.
        let edge_a = ccw(a.direction) * a.half_width_m;
        let edge_b = -ccw(b.direction) * b.half_width_m;
        let path = if gap < PI - 1.0e-3 {
            // 두 가장자리 선의 교점 X = center + edge_a + a·s_a = center + edge_b + b·s_b.
            let rhs = edge_b - edge_a;
            let denom = cross(a.direction, b.direction);
            let s_a = cross(rhs, b.direction) / denom;
            let s_b = cross(rhs, a.direction) / denom;
            let x = center + edge_a + a.direction * s_a;
            let t = corner_radius_m / (gap / 2.0).tan();
            let (t_a, t_b) = (x + a.direction * t, x + b.direction * t);
            let bisector = (a.direction + b.direction).normalize();
            let arc_center = x + bisector * (corner_radius_m / (gap / 2.0).sin());
            let from = t_a - arc_center;
            let sweep = from.angle_to(t_b - arc_center);
            let pieces = (sweep.abs() / step).ceil().max(1.0) as usize;
            let arc: Vec<Vec2> = (0..=pieces)
                .map(|n| {
                    arc_center + Vec2::from_angle(sweep * n as f32 / pieces as f32).rotate(from)
                })
                .collect();
            cut[i] = cut[i].max(s_a + t);
            cut[j] = cut[j].max(s_b + t);
            arc
        } else {
            // 180° 이상: 중심 둘레(반폭 원호) 또는 직선으로 잇는다.
            let (p, q) = (center + edge_a, center + edge_b);
            if (a.half_width_m - b.half_width_m).abs() < 1.0e-4 {
                let from = edge_a;
                let sweep = (angle(edge_b) - angle(edge_a)).rem_euclid(TAU);
                let pieces = (sweep / step).ceil().max(1.0) as usize;
                (0..=pieces)
                    .map(|n| {
                        center + Vec2::from_angle(sweep * n as f32 / pieces as f32).rotate(from)
                    })
                    .collect()
            } else {
                vec![p, q]
            }
        };
        corner_paths.push((i, j, path));
    }
    let mut boundary = Vec::new();
    let mut corners = Vec::with_capacity(count);
    for (i, j, path) in corner_paths {
        let (a, b) = (arms[i], arms[j]);
        let start = center + a.direction * cut[i] + ccw(a.direction) * a.half_width_m;
        let end = center + b.direction * cut[j] - ccw(b.direction) * b.half_width_m;
        let mut line = vec![start];
        for point in path.into_iter().chain(std::iter::once(end)) {
            if line
                .last()
                .is_none_or(|last: &Vec2| last.distance(point) > 1.0e-4)
            {
                line.push(point);
            }
        }
        boundary.extend(line.iter().copied());
        corners.push(line);
    }
    // 별 모양 검사: 중심과 이웃한 두 경계점이 이루는 삼각형이 모두 반시계여야 한다.
    let m = boundary.len();
    for k in 0..m {
        let (p, q) = (boundary[k], boundary[(k + 1) % m]);
        if p.distance(q) <= 1.0e-4 {
            continue;
        }
        if cross(p - center, q - center) < -1.0e-4 {
            return Err(format!(
                "junction at ({:.1}, {:.1}) is not star-shaped near ({:.1}, {:.1})",
                center.x, center.y, p.x, p.y
            ));
        }
    }
    Ok(JunctionShape {
        cut_m: cut,
        boundary,
        corners,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 각도(deg, 반시계)의 단위 벡터와 반폭 2 m인 팔을 만든다.
    fn arm(deg: f32) -> ArmInput {
        ArmInput {
            direction: Vec2::from_angle(deg.to_radians()),
            half_width_m: 2.0,
        }
    }

    /// 십자 교차로: 모든 팔을 반폭 + 모서리 반경(2 + 3 = 5 m)에서 자르고, 모서리 원호 4개가 반경 3 m인지 확인한다.
    #[test]
    fn four_way_cut_and_corners() {
        let arms = [arm(0.0), arm(90.0), arm(180.0), arm(270.0)];
        let shape = junction_shape(Vec2::ZERO, &arms, 3.0, 3.0, 25.0).unwrap();
        for cut in &shape.cut_m {
            assert!((cut - 5.0).abs() < 1e-4, "{:?}", shape.cut_m);
        }
        assert_eq!(shape.corners.len(), 4);
        // 첫 모서리(0°~90° 사이) 원호 중심은 (5, 5), 원호 위 점은 중심에서 3 m.
        let corner = &shape.corners[0];
        for p in &corner[1..corner.len() - 1] {
            assert!((p.distance(Vec2::new(5.0, 5.0)) - 3.0).abs() < 1e-3, "{p}");
        }
    }

    /// T자 교차로: 직선 쪽 모서리(180°)는 중심 둘레 반폭 원호(직선과 같음)로 잇고, 곁가지 쪽은 원호로 잇는지 확인한다.
    #[test]
    fn three_way_straight_side_and_reflex() {
        let arms = [arm(0.0), arm(180.0), arm(270.0)];
        let shape = junction_shape(Vec2::ZERO, &arms, 3.0, 3.0, 25.0).unwrap();
        assert_eq!(shape.corners.len(), 3);
        // 0°→180° 모서리: 반시계쪽 가장자리 y = +2 직선 위.
        let straight = shape
            .corners
            .iter()
            .find(|c| c.iter().all(|p| (p.y - 2.0).abs() < 1e-3))
            .expect("straight side");
        assert!(straight.len() >= 2);
        // 곁가지(270°)는 5 m에서 자른다.
        assert!((shape.cut_m[2] - 5.0).abs() < 1e-4);
        // 200° 넘게 벌어진 Y자도 별 모양으로 만든다.
        let wide = [arm(0.0), arm(100.0), arm(160.0)];
        assert!(junction_shape(Vec2::ZERO, &wide, 3.0, 3.0, 25.0).is_ok());
    }

    /// 예각 교차로는 원호 접점까지 더 길게 자르고, 25°보다 좁은 팔 사이는 거부하는지 확인한다.
    #[test]
    fn acute_arms_cut_longer_and_narrow_rejected() {
        let acute = [arm(0.0), arm(40.0), arm(180.0)];
        let shape = junction_shape(Vec2::ZERO, &acute, 3.0, 3.0, 25.0).unwrap();
        assert!(
            shape.cut_m[0] > 5.0 && shape.cut_m[1] > 5.0,
            "{:?}",
            shape.cut_m
        );
        let narrow = [arm(0.0), arm(20.0), arm(180.0)];
        let error = junction_shape(Vec2::ZERO, &narrow, 3.0, 3.0, 25.0).unwrap_err();
        assert!(error.contains("apart"), "{error}");
        assert!(junction_shape(Vec2::ZERO, &[arm(0.0), arm(180.0)], 3.0, 3.0, 25.0).is_err());
    }

    /// 경계 점이 모두 유한하고, 중심에서 본 부채꼴 삼각형이 모두 반시계인지 확인한다.
    #[test]
    fn boundary_is_star_shaped() {
        for arms in [
            vec![arm(0.0), arm(90.0), arm(180.0), arm(270.0)],
            vec![arm(10.0), arm(130.0), arm(250.0)],
            vec![arm(0.0), arm(100.0), arm(160.0)],
        ] {
            let shape = junction_shape(Vec2::new(3.0, -4.0), &arms, 3.0, 3.0, 25.0).unwrap();
            let c = Vec2::new(3.0, -4.0);
            let b = &shape.boundary;
            assert!(b.iter().all(|p| p.is_finite()));
            for k in 0..b.len() {
                let (p, q) = (b[k], b[(k + 1) % b.len()]);
                assert!(cross(p - c, q - c) >= -1e-4);
            }
        }
    }
}
