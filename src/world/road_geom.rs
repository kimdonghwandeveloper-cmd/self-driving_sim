//! 굽은 도로 형상 생성(Phase 4-1). 순수 기하 계산만 하며 Bevy 엔티티를 만들지 않는다.
//!
//! 중심선 폴리라인의 꺾이는 점마다 원호(필렛)를 넣어 매끄러운 중심선을 만들고, 그 중심선을 양옆으로
//! 평행 이동해 노면·표시선·연석 경계를 만든다. 형상 전체가 겹치지 않는지는 연석 바깥선으로 닫은 다각형이
//! 단순 다각형인지로 판정한다.
//!
//! 규칙(PLAN 4-1):
//! - 최소 허용 반경 `R_min = max(차량 최소 회전 반경, 폭/2 + 여유)`. 반경을 줄여도 이 값 아래로 내리지 않는다.
//! - 필렛 i의 접선 길이 `t_i = R_i·tan(θ_i/2)`. 한 구간을 양 끝 필렛이 나눠 쓰므로 `t_i + t_(i+1) ≤ 구간 길이`.
//!   `R_min`으로도 이 조건을 못 지키면 거부한다.
//! - 원호 하나의 안쪽 반경이 양수인 것만으로는 전체가 겹치지 않는다고 보지 않는다. 실제 경계 다각형으로 검사한다.
//!
//! 좌표는 지면 (x, z) m이며, 진행 방향 `f`의 오른쪽은 `(-f.z, f.x)`다([`crate::vehicle::Motion`]과 같은 규칙).
//! 반경 여유·최대 반경·원호 분할 각도는 모두 **임시값**이다.

use bevy::prelude::*;

use crate::vehicle::GroundRect;

/// 필렛 최소 반경에서 도로 반폭에 더하는 여유(m). 연석 두께(0.3 m)보다 커야 연석 바깥선 원호가 뒤집히지 않는다.
pub const RADIUS_MARGIN_M: f32 = 0.5;
/// 필렛 반경 상한(m). 공간이 넉넉해도 원래 중심선에서 너무 멀어지지 않게 한다. 임시값.
pub const MAX_FILLET_RADIUS_M: f32 = 30.0;
/// 기본 원호 분할 각도(deg). 원호를 이 각도 이하의 조각으로 나눈다.
pub const ARC_STEP_DEG: f32 = 3.0;
/// 이 거리(m) 안에서 거의 일직선인 중간 점은 지운다(같은 직선을 잘게 나눈 점 정리).
pub const COLLINEAR_TOLERANCE_M: f32 = 0.01;
/// 이보다 급한 꺾임(deg)은 되돌아가는 길로 보고 거부한다.
const MAX_TURN_DEG: f32 = 170.0;
/// 이보다 작은 꺾임(rad)은 필렛 없이 점을 그대로 둔다.
const NO_FILLET_RAD: f32 = 1.0e-4;
/// 연석 직사각형 이음부에 더 겹쳐 두는 길이(m).
const CURB_JOINT_OVERLAP_M: f32 = 0.02;
/// 이보다 가까운 연속 점(m)은 하나로 합친다(원호 끝과 다음 원호 시작이 겹칠 때).
const MERGE_EPS_M: f32 = 1.0e-4;

/// 도로 형상 생성 조건.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadSpec {
    /// 노면 폭(m).
    pub width_m: f32,
    /// 연석 두께(m).
    pub curb_thickness_m: f32,
    /// 차량 최소 회전 반경(m, 후륜축 기준).
    pub vehicle_min_radius_m: f32,
    /// 원호 분할 각도(deg, 0 초과 45 이하).
    pub arc_step_deg: f32,
}

impl RoadSpec {
    /// 최소 허용 필렛 반경 `max(차량 최소 회전 반경, 폭/2 + 여유)`(m)를 구한다.
    pub fn min_fillet_radius_m(&self) -> f32 {
        self.vehicle_min_radius_m
            .max(self.width_m / 2.0 + RADIUS_MARGIN_M)
    }

    /// 값이 유한한 양수인지, 분할 각도가 범위 안인지, 여유가 연석 두께보다 큰지 검사한다.
    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("width", self.width_m),
            ("curb thickness", self.curb_thickness_m),
            ("vehicle minimum radius", self.vehicle_min_radius_m),
            ("arc step", self.arc_step_deg),
        ] {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!(
                    "road {name} must be a positive number, got {value}"
                ));
            }
        }
        if self.arc_step_deg > 45.0 {
            return Err(format!(
                "arc step {} deg must be at most 45 deg",
                self.arc_step_deg
            ));
        }
        if self.curb_thickness_m >= RADIUS_MARGIN_M {
            return Err("curb thickness must be smaller than the radius margin".to_owned());
        }
        Ok(())
    }
}

/// 필렛을 넣은 도로 중심선.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadGeometry {
    /// 원호를 조각낸 매끄러운 중심선(진행 순서).
    pub centerline: Vec<Vec2>,
    /// 입력 중심선의 중간 점마다 고른 필렛 반경(m). 필렛을 넣지 않은 거의 직선인 점은 0.
    pub radii: Vec<f32>,
}

/// 진행 방향 `forward`의 오른쪽 단위 벡터를 구한다.
pub fn right_of(forward: Vec2) -> Vec2 {
    Vec2::new(-forward.y, forward.x)
}

/// 방향 벡터 `direction`(x, z)을 바라보는 heading(rad)을 구한다. [`crate::vehicle::Motion`]과 같은 규칙이다.
pub fn heading_of(direction: Vec2) -> f32 {
    (-direction.x).atan2(-direction.y)
}

/// 폴리라인 길이(m)를 구한다.
pub fn polyline_length(points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

/// 폴리라인에서 처음부터 길이 `distance`(m)인 점과 그곳의 진행 방향을 구한다. 길이를 넘으면 `None`.
pub fn point_at_distance(points: &[Vec2], distance: f32) -> Option<(Vec2, Vec2)> {
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length > 0.0 && walked + length >= distance {
            let direction = (pair[1] - pair[0]) / length;
            return Some((pair[0] + direction * (distance - walked), direction));
        }
        walked += length;
    }
    None
}

/// 같은 직선을 잘게 나눈 점처럼, 앞뒤 남긴 점을 잇는 선분에서 [`COLLINEAR_TOLERANCE_M`] 안에 있는 중간 점을 지운다.
///
/// 지운 점들이 모두 새 선분에서 허용 거리 안에 있을 때만 지우므로 형상은 1 cm 안에서 유지된다.
pub fn drop_collinear(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut kept = vec![points[0]];
    let mut anchor = 0;
    for next in 2..points.len() {
        let (a, b) = (points[anchor], points[next]);
        let fits = points[anchor + 1..next]
            .iter()
            .all(|p| distance_to_segment(*p, a, b) <= COLLINEAR_TOLERANCE_M);
        if !fits {
            anchor = next - 1;
            kept.push(points[anchor]);
        }
    }
    kept.push(*points.last().unwrap());
    kept
}

/// 점 `p`에서 선분 `a-b`까지의 거리(m)를 구한다.
fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// 정리된 중심선(연속 중복점 없음, 점 2개 이상)의 꺾이는 점마다 필렛을 넣어 매끄러운 중심선을 만든다.
///
/// 거의 일직선인 중간 점은 먼저 지운다. 각 필렛은 `R_min` 이상, [`MAX_FILLET_RADIUS_M`] 이하이며,
/// 한 구간을 양 끝 필렛이 나눠 쓰는 길이가 구간 길이를 넘지 않는다. 지킬 수 없으면 오류다.
pub fn build_geometry(points: &[Vec2], spec: &RoadSpec) -> Result<RoadGeometry, String> {
    spec.validate()?;
    let points = drop_collinear(points);
    if points.len() < 2 {
        return Err("centerline has fewer than 2 points".to_owned());
    }
    let n = points.len();
    let lengths: Vec<f32> = points.windows(2).map(|p| p[0].distance(p[1])).collect();
    let dirs: Vec<Vec2> = points
        .windows(2)
        .map(|p| (p[1] - p[0]).normalize())
        .collect();
    // 중간 점 i(1..n-1)의 꺾임 각(rad, 크기).
    let turns: Vec<f32> = (1..n - 1)
        .map(|i| dirs[i - 1].angle_to(dirs[i]).abs())
        .collect();
    for (k, turn) in turns.iter().enumerate() {
        if turn.to_degrees() >= MAX_TURN_DEG {
            return Err(format!(
                "centerline turns back {:.0} deg at {:?}",
                turn.to_degrees(),
                points[k + 1]
            ));
        }
    }
    let r_min = spec.min_fillet_radius_m();
    let tangent = |turn: f32, radius: f32| {
        if turn < NO_FILLET_RAD {
            0.0
        } else {
            radius * (turn / 2.0).tan()
        }
    };
    let t_min: Vec<f32> = turns.iter().map(|&turn| tangent(turn, r_min)).collect();
    // 구간 j(점 j→j+1)를 쓰는 필렛: 시작 점 j(중간 점이면)와 끝 점 j+1(중간 점이면).
    let t_at = |t: &[f32], vertex: usize| {
        if vertex == 0 || vertex == n - 1 {
            0.0
        } else {
            t[vertex - 1]
        }
    };
    let interior = |vertex: usize| vertex != 0 && vertex != n - 1;
    let mut spare = Vec::with_capacity(n - 1);
    for j in 0..n - 1 {
        let used = t_at(&t_min, j) + t_at(&t_min, j + 1);
        if used > lengths[j] + 1.0e-4 {
            return Err(format!(
                "segment {:?}-{:?} is {:.2} m but bends at its ends need {:.2} m at the minimum radius {:.2} m \
                 (vehicle turning radius or road width {:.1} m): the bend is too sharp for this short segment",
                points[j],
                points[j + 1],
                lengths[j],
                used,
                r_min,
                spec.width_m
            ));
        }
        let users = usize::from(interior(j)) + usize::from(interior(j + 1));
        spare.push(if users == 0 {
            0.0
        } else {
            (lengths[j] - used) / users as f32
        });
    }
    let radii: Vec<f32> = (1..n - 1)
        .map(|i| {
            let turn = turns[i - 1];
            if turn < NO_FILLET_RAD {
                return 0.0;
            }
            let t_max = t_min[i - 1] + spare[i - 1].min(spare[i]);
            (t_max / (turn / 2.0).tan()).clamp(r_min, MAX_FILLET_RADIUS_M.max(r_min))
        })
        .collect();

    let mut centerline = vec![points[0]];
    let step = spec.arc_step_deg.to_radians();
    for i in 1..n - 1 {
        let radius = radii[i - 1];
        if radius == 0.0 {
            push_point(&mut centerline, points[i]);
            continue;
        }
        let (d_in, d_out) = (dirs[i - 1], dirs[i]);
        let t = tangent(turns[i - 1], radius);
        let (start, end) = (points[i] - d_in * t, points[i] + d_out * t);
        let side = if d_out.dot(right_of(d_in)) > 0.0 {
            right_of(d_in)
        } else {
            -right_of(d_in)
        };
        let center = start + side * radius;
        let from = start - center;
        let sweep = from.angle_to(end - center);
        let pieces = (sweep.abs() / step).ceil().max(1.0) as usize;
        for k in 0..=pieces {
            let point = center + Vec2::from_angle(sweep * k as f32 / pieces as f32).rotate(from);
            push_point(&mut centerline, point);
        }
    }
    push_point(&mut centerline, points[n - 1]);
    Ok(RoadGeometry { centerline, radii })
}

/// 앞 점과 [`MERGE_EPS_M`]보다 멀 때만 점을 붙인다.
fn push_point(points: &mut Vec<Vec2>, point: Vec2) {
    if points
        .last()
        .is_none_or(|last| last.distance(point) > MERGE_EPS_M)
    {
        points.push(point);
    }
}

/// 중심선을 오른쪽으로 `lateral`(m, 음수면 왼쪽)만큼 평행 이동한 선을 만든다. 꺾이는 점은 마이터로 잇는다.
pub fn offset_line(center: &[Vec2], lateral: f32) -> Vec<Vec2> {
    let n = center.len();
    (0..n)
        .map(|i| {
            let d_in = if i > 0 {
                (center[i] - center[i - 1]).normalize()
            } else {
                (center[1] - center[0]).normalize()
            };
            let d_out = if i + 1 < n {
                (center[i + 1] - center[i]).normalize()
            } else {
                d_in
            };
            let sum = right_of(d_in) + right_of(d_out);
            let normal = if sum.length() > 1.0e-6 {
                sum.normalize()
            } else {
                right_of(d_in)
            };
            center[i] + normal * (lateral / normal.dot(right_of(d_in)))
        })
        .collect()
}

/// 연석 바깥선(중심선에서 `half_width_m`)을 왼쪽은 앞으로, 오른쪽은 거꾸로 이어 닫은 다각형이 단순 다각형인지 검사한다.
///
/// 이웃한 변(끝점 공유)끼리만 만날 수 있다. 그 밖의 두 변이 닿거나 교차하면 노면·연석이 겹치는 것으로 보고
/// 위치와 함께 오류를 반환한다. 후보는 변의 x 범위로 먼저 걸러낸다(broad phase).
pub fn check_simple_boundary(center: &[Vec2], half_width_m: f32) -> Result<(), String> {
    let left = offset_line(center, -half_width_m);
    let right = offset_line(center, half_width_m);
    let polygon: Vec<Vec2> = left.iter().chain(right.iter().rev()).copied().collect();
    let m = polygon.len();
    let edges: Vec<(Vec2, Vec2)> = (0..m).map(|k| (polygon[k], polygon[(k + 1) % m])).collect();
    let mut order: Vec<usize> = (0..m).collect();
    let min_x = |k: usize| edges[k].0.x.min(edges[k].1.x);
    let max_x = |k: usize| edges[k].0.x.max(edges[k].1.x);
    order.sort_by(|&a, &b| min_x(a).total_cmp(&min_x(b)));
    for (rank, &a) in order.iter().enumerate() {
        for &b in &order[rank + 1..] {
            if min_x(b) > max_x(a) {
                break;
            }
            let gap = a.abs_diff(b);
            if gap <= 1 || gap == m - 1 {
                continue;
            }
            if segments_touch(edges[a], edges[b]) {
                let at = (edges[a].0 + edges[a].1) / 2.0;
                return Err(format!(
                    "road or curb overlaps itself near ({:.1}, {:.1}) (S-bend, loop or road coming back too close)",
                    at.x, at.y
                ));
            }
        }
    }
    Ok(())
}

/// 두 선분이 닿거나 교차하는지 판정한다(끝점 접촉·같은 직선 위 겹침 포함). 계산은 f64로 한다.
fn segments_touch((p1, p2): (Vec2, Vec2), (q1, q2): (Vec2, Vec2)) -> bool {
    let to64 = |v: Vec2| (f64::from(v.x), f64::from(v.y));
    let (p1, p2, q1, q2) = (to64(p1), to64(p2), to64(q1), to64(q2));
    let orient = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        let value = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if value.abs() < 1.0e-9 {
            0.0
        } else {
            value.signum()
        }
    };
    let on_segment = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        c.0 >= a.0.min(b.0) - 1.0e-9
            && c.0 <= a.0.max(b.0) + 1.0e-9
            && c.1 >= a.1.min(b.1) - 1.0e-9
            && c.1 <= a.1.max(b.1) + 1.0e-9
    };
    let (o1, o2) = (orient(p1, p2, q1), orient(p1, p2, q2));
    let (o3, o4) = (orient(q1, q2, p1), orient(q1, q2, p2));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    (o1 == 0.0 && on_segment(p1, p2, q1))
        || (o2 == 0.0 && on_segment(p1, p2, q2))
        || (o3 == 0.0 && on_segment(q1, q2, p1))
        || (o4 == 0.0 && on_segment(q1, q2, p2))
}

/// 중심선 조각마다 양옆 연석 직사각형을 만든다. 직사각형 중심은 중심선에서 `폭/2 + 두께/2`만큼 떨어진다.
///
/// 이음부의 꺾임 Δθ마다 **바깥쪽** 연석만 `(폭/2 + 두께)·tan(Δθ/2) + 0.02 m` 늘려 쐐기 틈을 덮는다.
/// 안쪽 연석은 이음부에서 서로 겹치므로 늘리지 않으며, 그래서 노면 안으로 들어오지 않는다.
pub fn curb_rects(center: &[Vec2], width_m: f32, curb_m: f32) -> Vec<GroundRect> {
    let n = center.len();
    let reach = width_m / 2.0 + curb_m;
    // 이음부의 부호 있는 꺾임(rad). 양수면 오른쪽(right_of 쪽)으로 꺾여 왼쪽(-1)이 바깥이다.
    let joint_turn = |i: usize| {
        if i == 0 || i == n - 1 {
            0.0
        } else {
            (center[i] - center[i - 1]).angle_to(center[i + 1] - center[i])
        }
    };
    let mut rects = Vec::with_capacity(2 * (n - 1));
    for i in 0..n - 1 {
        let (a, b) = (center[i], center[i + 1]);
        let length = a.distance(b);
        let forward = (b - a) / length;
        // 쪽 `side`가 이음부 꺾임 `turn`의 바깥이면 늘릴 길이, 아니면 0.
        let extend = |turn: f32, side: f32| {
            if turn * side < 0.0 {
                reach * (turn.abs() / 2.0).tan() + CURB_JOINT_OVERLAP_M
            } else {
                0.0
            }
        };
        for side in [-1.0, 1.0] {
            let (back, front) = (extend(joint_turn(i), side), extend(joint_turn(i + 1), side));
            let middle = (a + b) / 2.0
                + right_of(forward) * side * (width_m / 2.0 + curb_m / 2.0)
                + forward * (front - back) / 2.0;
            rects.push(GroundRect {
                center: middle,
                half_size: Vec2::new(curb_m / 2.0, (length + back + front) / 2.0),
                heading_rad: heading_of(forward),
            });
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험용 형상 조건: 폭 4 m, 연석 0.3 m, 차량 최소 반경 4.85 m, 분할 3°.
    fn spec(width_m: f32) -> RoadSpec {
        RoadSpec {
            width_m,
            curb_thickness_m: 0.3,
            vehicle_min_radius_m: 4.85,
            arc_step_deg: ARC_STEP_DEG,
        }
    }

    /// 점 `p`가 직사각형 안(경계에서 `tolerance` m 여유 포함)인지 확인한다.
    fn rect_contains(rect: &GroundRect, p: Vec2, tolerance: f32) -> bool {
        let (sin, cos) = rect.heading_rad.sin_cos();
        let (right, forward) = (Vec2::new(cos, -sin), Vec2::new(-sin, -cos));
        let local = p - rect.center;
        local.dot(right).abs() <= rect.half_size.x + tolerance
            && local.dot(forward).abs() <= rect.half_size.y + tolerance
    }

    /// 90° 꺾인 직각 중심선에 필렛이 들어가고, 반경이 최소값 이상이며, 원호 조각이 분할 각도 이하인지 확인한다.
    #[test]
    fn right_angle_gets_fillet_within_limits() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(40.0, -40.0)];
        let geometry = build_geometry(&line, &spec(4.0)).unwrap();
        assert_eq!(geometry.radii.len(), 1);
        let radius = geometry.radii[0];
        assert!(radius >= spec(4.0).min_fillet_radius_m() && radius <= MAX_FILLET_RADIUS_M);
        for window in geometry.centerline.windows(3) {
            let turn = (window[1] - window[0])
                .angle_to(window[2] - window[1])
                .abs();
            assert!(
                turn.to_degrees() <= ARC_STEP_DEG + 0.01,
                "{}",
                turn.to_degrees()
            );
        }
        assert!(geometry.centerline.iter().all(|p| p.is_finite()));
        // 꼭짓점(0, -40)은 원호가 깎아 지나가므로 중심선 위에 없다.
        assert!(
            geometry
                .centerline
                .iter()
                .all(|p| p.distance(line[1]) > 1.0)
        );
    }

    /// 넓은 도로에서는 R_min이 폭/2 + 여유로 커지고, 짧은 구간 양 끝 꺾임이 그 반경으로 들어가지 않으면 거부하는지 확인한다.
    #[test]
    fn wide_road_short_segment_keeps_both_conditions() {
        let wide = spec(12.0);
        assert_eq!(wide.min_fillet_radius_m(), 6.5);
        // 45° 꺾임 두 번, 사이 구간 4 m: 필요 접선 길이 2 × 6.5 × tan(22.5°) ≈ 5.39 m > 4 m.
        let zigzag = [
            Vec2::ZERO,
            Vec2::new(0.0, -30.0),
            Vec2::new(2.83, -32.83),
            Vec2::new(2.83, -62.83),
        ];
        let error = build_geometry(&zigzag, &wide).unwrap_err();
        assert!(error.contains("too sharp"), "{error}");
        // 같은 꺾임도 폭 4 m(R_min 4.85 m, 필요 약 4.02 m > 4 m)에서는 아직 부족하다.
        assert!(build_geometry(&zigzag, &spec(4.0)).is_err());
        // 사이 구간이 6 m면 폭 4 m는 들어가고, 고른 반경은 모두 R_min 이상이다.
        let longer = [
            Vec2::ZERO,
            Vec2::new(0.0, -30.0),
            Vec2::new(4.243, -34.243),
            Vec2::new(4.243, -64.243),
        ];
        let geometry = build_geometry(&longer, &spec(4.0)).unwrap();
        assert!(
            geometry.radii.iter().all(|&r| r >= 4.85 - 1e-4),
            "{:?}",
            geometry.radii
        );
    }

    /// 거의 되돌아가는 꺾임과 잘못된 조건을 거부하는지 확인한다.
    #[test]
    fn reversal_and_invalid_spec_are_rejected() {
        let back = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(1.0, 0.0)];
        assert!(
            build_geometry(&back, &spec(4.0))
                .unwrap_err()
                .contains("turns back")
        );
        let line = [Vec2::ZERO, Vec2::new(0.0, -40.0)];
        for bad in [
            RoadSpec {
                width_m: 0.0,
                ..spec(4.0)
            },
            RoadSpec {
                arc_step_deg: 60.0,
                ..spec(4.0)
            },
            RoadSpec {
                curb_thickness_m: 0.6,
                ..spec(4.0)
            },
            RoadSpec {
                vehicle_min_radius_m: f32::NAN,
                ..spec(4.0)
            },
        ] {
            assert!(build_geometry(&line, &bad).is_err(), "{bad:?}");
        }
    }

    /// 같은 직선을 잘게 나눈 점은 지우고, 1 cm보다 벗어난 점은 남기는지 확인한다.
    #[test]
    fn collinear_points_are_dropped() {
        let line: Vec<Vec2> = (0..=40).map(|i| Vec2::new(0.0, -(i as f32))).collect();
        assert_eq!(
            drop_collinear(&line),
            vec![Vec2::ZERO, Vec2::new(0.0, -40.0)]
        );
        let bent = [Vec2::ZERO, Vec2::new(0.02, -20.0), Vec2::new(0.0, -40.0)];
        assert_eq!(drop_collinear(&bent).len(), 3);
    }

    /// 마이터 평행선이 직선 구간에서 정확히 폭/2 떨어지고, 오른쪽이 진행 방향 오른쪽인지 확인한다.
    #[test]
    fn offset_line_uses_vehicle_right() {
        // -Z(heading 0)로 진행하면 오른쪽은 +X다.
        let line = [Vec2::ZERO, Vec2::new(0.0, -10.0), Vec2::new(0.0, -20.0)];
        let right = offset_line(&line, 2.0);
        assert!(right.iter().all(|p| (p.x - 2.0).abs() < 1e-5), "{right:?}");
        let left = offset_line(&line, -2.0);
        assert!(left.iter().all(|p| (p.x + 2.0).abs() < 1e-5));
    }

    /// 단순 다각형 검사: 정상 굽은 길은 통과하고, 처음 직선 위로 되돌아오는 길은 거부하는지 확인한다.
    #[test]
    fn boundary_check_distinguishes_overlap() {
        let turn = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(40.0, -40.0)];
        let geometry = build_geometry(&turn, &spec(4.0)).unwrap();
        assert!(check_simple_boundary(&geometry.centerline, 2.3).is_ok());
        // 사각형으로 돌아 처음 직선을 다시 지나가는 길.
        let looped = [
            Vec2::ZERO,
            Vec2::new(0.0, -40.0),
            Vec2::new(30.0, -40.0),
            Vec2::new(30.0, -20.0),
            Vec2::new(-10.0, -20.0),
        ];
        let geometry = build_geometry(&looped, &spec(4.0)).unwrap();
        let error = check_simple_boundary(&geometry.centerline, 2.3).unwrap_err();
        assert!(error.contains("overlaps"), "{error}");
    }

    /// 연석 직사각형이 굽은 길 양옆 연석 띠를 빈틈 없이 덮는지, 노면 안쪽으로 들어오는 깊이가 분할 각도에 맞는
    /// 상한(기본 3°에서 2 cm, 10°에서 8 cm) 이하인지 확인한다. 안쪽 이웃 연석이 이음부 뒤로 겹치는 깊이는
    /// 대략 `(폭/2)·tan(Δθ)·sin(Δθ)`이다.
    #[test]
    fn curbs_cover_band_without_intruding() {
        let turn = [Vec2::ZERO, Vec2::new(0.0, -40.0), Vec2::new(40.0, -40.0)];
        for step in [1.0, 3.0, 10.0] {
            let road = RoadSpec {
                arc_step_deg: step,
                ..spec(4.0)
            };
            let geometry = build_geometry(&turn, &road).unwrap();
            let rects = curb_rects(&geometry.centerline, 4.0, 0.3);
            let dense = densify(&geometry.centerline, 0.05);
            for lateral in [-2.15, 2.15, -2.29, 2.29, -2.01, 2.01] {
                for p in offset_line(&dense, lateral) {
                    assert!(
                        rects.iter().any(|r| rect_contains(r, p, 1e-4)),
                        "gap at {p} (lateral {lateral}, step {step})"
                    );
                }
            }
            let allowed = if step > 3.0 { 0.08 } else { 0.02 };
            for lateral in [-(2.0 - allowed), 2.0 - allowed] {
                for p in offset_line(&dense, lateral) {
                    assert!(
                        !rects.iter().any(|r| rect_contains(r, p, 0.0)),
                        "curb intrudes at {p} (step {step})"
                    );
                }
            }
        }
    }

    /// 폴리라인 각 구간을 `spacing` m 이하 조각으로 나눈다(시험용).
    fn densify(points: &[Vec2], spacing: f32) -> Vec<Vec2> {
        let mut out = vec![points[0]];
        for pair in points.windows(2) {
            let pieces = (pair[0].distance(pair[1]) / spacing).ceil().max(1.0) as usize;
            for k in 1..=pieces {
                out.push(pair[0].lerp(pair[1], k as f32 / pieces as f32));
            }
        }
        out
    }

    /// 길이 위치의 점과 방향을 구하고, 길이를 넘으면 `None`인지 확인한다.
    #[test]
    fn point_at_distance_walks_polyline() {
        let line = [Vec2::ZERO, Vec2::new(0.0, -10.0), Vec2::new(10.0, -10.0)];
        let (p, d) = point_at_distance(&line, 15.0).unwrap();
        assert!((p - Vec2::new(5.0, -10.0)).length() < 1e-5 && d == Vec2::X);
        assert_eq!(polyline_length(&line), 20.0);
        assert!(point_at_distance(&line, 21.0).is_none());
    }
}
