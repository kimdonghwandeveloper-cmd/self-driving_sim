//! 검증된 Lanelet2 경계선을 시뮬레이터의 지역 좌표 도로 면과 중심선으로 변환한다.
//!
//! 이 단계는 `CourseDef`나 Godot 노드를 만들지 않는다. 각 lanelet을 진행 방향의 좌우 경계가
//! 같은 개수로 대응되는 [`Strip`]과 중심선으로 바꾸며, 다음 단계가 이 결과를 연결·조립한다.

use std::collections::BTreeMap;

use glam::Vec2;

use super::course::{Strip, StripKind};
use super::lanelet2::{LaneletDirection, LaneletMap, LaneletRelation, LaneletWay};
use super::map::{LatLon, project};

/// 서로 다른 점으로 인정할 최소 거리(m).
const MIN_POINT_SPACING_M: f32 = 0.05;
/// 도로 면 대응점을 만들 때 허용하는 최대 간격(m).
const MAX_SAMPLE_SPACING_M: f32 = 5.0;
/// 단일 lanelet 도로 폭의 허용 범위(m).
const WIDTH_RANGE_M: std::ops::RangeInclusive<f32> = 0.5..=30.0;
/// 차로 하나로 보기 위한 평균 폭의 임시 허용 범위(m).
const MEAN_WIDTH_RANGE_M: std::ops::RangeInclusive<f32> = 2.0..=7.0;
/// 지역 원점에서 형상이 떨어질 수 있는 최대 거리(m).
const MAX_ORIGIN_DISTANCE_M: f32 = 2_000.0;
/// 비정상적으로 큰 입력이 과도한 도로 면 표본을 만들지 못하게 하는 상한.
const MAX_SAMPLES_PER_LANELET: usize = 4_096;
/// 기하 비교에서 부동소수점 오차를 흡수하는 거리(m).
const GEOMETRY_EPSILON_M: f32 = 1.0e-3;

/// lanelet 하나를 지역 좌표의 도로 면과 중심선으로 바꾼 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneletGeometry {
    /// 원본 Lanelet2 relation ID.
    pub id: i64,
    /// 진행 방향을 기준으로 좌우 경계를 대응시킨 도로 면.
    pub surface: Strip,
    /// 원본 중심선 또는 좌우 경계의 중점으로 생성한 중심선.
    pub centerline: Vec<Vec2>,
    /// 표본별 좌우 경계 거리의 평균(m).
    pub mean_width_m: f32,
    /// 원본 relation의 통행 방향.
    pub direction: LaneletDirection,
    /// 공유 경계 시작점으로 이어지는 이전 lanelet relation ID.
    pub predecessors: Vec<i64>,
    /// 공유 경계 끝점에서 이어지는 다음 lanelet relation ID.
    pub successors: Vec<i64>,
}

/// 한 Lanelet2 파일을 공통 지역 원점으로 투영한 결정적 geometry 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneletGeometryMap {
    /// 지역 좌표 `(0, 0)`에 해당하는 WGS84 좌표.
    pub origin: LatLon,
    /// relation ID 순으로 정렬된 lanelet geometry.
    pub lanelets: BTreeMap<i64, LaneletGeometry>,
    /// 파서 경고와 geometry 단계에서 생성한 경고.
    pub warnings: Vec<String>,
}

/// lanelet 진행 방향에서 좌우 경계의 시작·끝 node ID 쌍.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LaneletEndpoints {
    start: (i64, i64),
    end: (i64, i64),
}

/// 파서가 검증한 Lanelet2 지도를 지정한 원점의 지역 좌표 도로 면과 중심선으로 변환한다.
pub fn build_lanelet_geometry(
    map: &LaneletMap,
    origin: LatLon,
) -> Result<LaneletGeometryMap, String> {
    if !origin.lat.is_finite()
        || !origin.lon.is_finite()
        || origin.lat.abs() > 90.0
        || origin.lon.abs() > 180.0
    {
        return Err(format!("invalid Lanelet2 geometry origin {origin:?}"));
    }
    let mut warnings = map.warnings.clone();
    let mut lanelets = BTreeMap::new();
    let mut endpoints = BTreeMap::new();
    for relation in map.lanelets.values() {
        let (geometry, relation_endpoints) = build_one(map, relation, origin, &mut warnings)?;
        lanelets.insert(relation.id, geometry);
        endpoints.insert(relation.id, relation_endpoints);
    }
    connect_lanelets(&mut lanelets, &endpoints);
    Ok(LaneletGeometryMap {
        origin,
        lanelets,
        warnings,
    })
}

/// 실제 lanelet이 참조하는 모든 way의 경계 상자 중심을 편의용 투영 원점으로 제안한다.
pub fn suggest_lanelet_origin(map: &LaneletMap) -> Result<LatLon, String> {
    let mut min_lat = f64::INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut min_lon = f64::INFINITY;
    let mut max_lon = f64::NEG_INFINITY;
    for relation in map.lanelets.values() {
        for way_id in [
            Some(relation.left),
            Some(relation.right),
            relation.centerline,
        ]
        .into_iter()
        .flatten()
        {
            let way = &map.ways[&way_id];
            for node_id in &way.nodes {
                let node = &map.nodes[node_id];
                min_lat = min_lat.min(node.lat);
                max_lat = max_lat.max(node.lat);
                min_lon = min_lon.min(node.lon);
                max_lon = max_lon.max(node.lon);
            }
        }
    }
    if !min_lat.is_finite() {
        return Err("Lanelet2 geometry has no referenced coordinates".to_owned());
    }
    Ok(LatLon {
        lat: (min_lat + max_lat) * 0.5,
        lon: (min_lon + max_lon) * 0.5,
    })
}

/// relation 하나의 경계 방향을 맞추고 도로 면·중심선·평균 폭을 만든다.
fn build_one(
    map: &LaneletMap,
    relation: &LaneletRelation,
    origin: LatLon,
    warnings: &mut Vec<String>,
) -> Result<(LaneletGeometry, LaneletEndpoints), String> {
    let left = project_way(map, &map.ways[&relation.left], origin)?;
    let mut right = project_way(map, &map.ways[&relation.right], origin)?;
    let right_reversed = orient_like(&left, &mut right);

    let sample_parameters = sample_parameters(&left, &right)?;
    let left_samples = resample(&left, &sample_parameters);
    let right_samples = resample(&right, &sample_parameters);
    let mut widths = Vec::with_capacity(sample_parameters.len());
    for (index, (left_point, right_point)) in left_samples.iter().zip(&right_samples).enumerate() {
        let width = left_point.distance(*right_point);
        if !WIDTH_RANGE_M.contains(&width) {
            return Err(format!(
                "lanelet {} width at sample {index} is {width:.3} m (allowed {:.1}..={:.1} m)",
                relation.id,
                WIDTH_RANGE_M.start(),
                WIDTH_RANGE_M.end()
            ));
        }
        widths.push(width);
    }
    validate_left_right(relation.id, &left_samples, &right_samples)?;
    validate_simple_surface(relation.id, &left_samples, &right_samples)?;

    let generated_center: Vec<Vec2> = left_samples
        .iter()
        .zip(&right_samples)
        .map(|(left_point, right_point)| (*left_point + *right_point) * 0.5)
        .collect();
    let centerline = if let Some(centerline_id) = relation.centerline {
        let mut centerline = project_way(map, &map.ways[&centerline_id], origin)?;
        orient_like(&generated_center, &mut centerline);
        validate_centerline(relation.id, &centerline, &left_samples, &right_samples)?;
        centerline
    } else {
        warnings.push(format!(
            "lanelet {} has no centerline; generated from boundary midpoints",
            relation.id
        ));
        generated_center
    };
    let mean_width_m: f32 = widths
        .windows(2)
        .zip(sample_parameters.windows(2))
        .map(|(width_pair, parameter_pair)| {
            (width_pair[0] + width_pair[1]) * 0.5 * (parameter_pair[1] - parameter_pair[0])
        })
        .sum();
    if !MEAN_WIDTH_RANGE_M.contains(&mean_width_m) {
        return Err(format!(
            "lanelet {} mean width is {mean_width_m:.3} m (allowed {:.1}..={:.1} m)",
            relation.id,
            MEAN_WIDTH_RANGE_M.start(),
            MEAN_WIDTH_RANGE_M.end()
        ));
    }
    let left_way = &map.ways[&relation.left];
    let right_way = &map.ways[&relation.right];
    let right_start = if right_reversed {
        right_way.nodes[right_way.nodes.len() - 1]
    } else {
        right_way.nodes[0]
    };
    let right_end = if right_reversed {
        right_way.nodes[0]
    } else {
        right_way.nodes[right_way.nodes.len() - 1]
    };
    Ok((
        LaneletGeometry {
            id: relation.id,
            surface: Strip {
                kind: StripKind::Road,
                left: left_samples,
                right: right_samples,
            },
            centerline,
            mean_width_m,
            direction: relation.direction,
            predecessors: Vec::new(),
            successors: Vec::new(),
        },
        LaneletEndpoints {
            start: (left_way.nodes[0], right_start),
            end: (left_way.nodes[left_way.nodes.len() - 1], right_end),
        },
    ))
}

/// 공유 경계 node 쌍을 색인해 모든 lanelet의 이전·다음 연결을 결정적으로 채운다.
fn connect_lanelets(
    lanelets: &mut BTreeMap<i64, LaneletGeometry>,
    endpoints: &BTreeMap<i64, LaneletEndpoints>,
) {
    let mut starts: BTreeMap<(i64, i64), Vec<i64>> = BTreeMap::new();
    for (&id, endpoint) in endpoints {
        starts.entry(endpoint.start).or_default().push(id);
    }
    let mut edges = Vec::new();
    for (&id, endpoint) in endpoints {
        if let Some(successors) = starts.get(&endpoint.end) {
            edges.extend(
                successors
                    .iter()
                    .copied()
                    .filter(|successor| *successor != id)
                    .map(|successor| (id, successor)),
            );
        }
    }
    for (predecessor, successor) in edges {
        lanelets
            .get_mut(&predecessor)
            .expect("endpoint IDs come from lanelets")
            .successors
            .push(successor);
        lanelets
            .get_mut(&successor)
            .expect("endpoint IDs come from lanelets")
            .predecessors
            .push(predecessor);
    }
}

/// way의 위경도 node를 지역 좌표로 투영하고 퇴화·범위 초과를 거부한다.
fn project_way(map: &LaneletMap, way: &LaneletWay, origin: LatLon) -> Result<Vec<Vec2>, String> {
    let mut points = Vec::with_capacity(way.nodes.len());
    for node_id in &way.nodes {
        let node = &map.nodes[node_id];
        let point = project(
            origin,
            LatLon {
                lat: node.lat,
                lon: node.lon,
            },
        );
        if !point.is_finite() || point.length() > MAX_ORIGIN_DISTANCE_M {
            return Err(format!(
                "way {} node {} is outside the {MAX_ORIGIN_DISTANCE_M:.0} m geometry region",
                way.id, node.id
            ));
        }
        if points
            .last()
            .is_some_and(|previous: &Vec2| previous.distance(point) < MIN_POINT_SPACING_M)
        {
            return Err(format!(
                "way {} has points closer than {MIN_POINT_SPACING_M:.2} m",
                way.id
            ));
        }
        points.push(point);
    }
    if polyline_length(&points) < MIN_POINT_SPACING_M {
        return Err(format!("way {} has zero-length geometry", way.id));
    }
    Ok(points)
}

/// 기준 선과 후보 선의 양 끝 거리 합이 작아지는 방향으로 후보를 뒤집는다.
fn orient_like(reference: &[Vec2], candidate: &mut [Vec2]) -> bool {
    let same = reference[0].distance(candidate[0])
        + reference[reference.len() - 1].distance(candidate[candidate.len() - 1]);
    let reversed = reference[0].distance(candidate[candidate.len() - 1])
        + reference[reference.len() - 1].distance(candidate[0]);
    if reversed + GEOMETRY_EPSILON_M < same {
        candidate.reverse();
        true
    } else {
        false
    }
}

/// 양쪽 경계의 모든 꼭짓점과 최대 간격 표본을 포함하는 정규화 길이 위치를 만든다.
fn sample_parameters(left: &[Vec2], right: &[Vec2]) -> Result<Vec<f32>, String> {
    let left_length = polyline_length(left);
    let right_length = polyline_length(right);
    let max_length = left_length.max(right_length);
    let interval_count = (max_length / MAX_SAMPLE_SPACING_M).ceil() as usize;
    let mut parameters = Vec::with_capacity(left.len() + right.len() + interval_count + 1);
    parameters.extend(normalized_vertices(left, left_length));
    parameters.extend(normalized_vertices(right, right_length));
    for index in 0..=interval_count {
        parameters.push(index as f32 / interval_count.max(1) as f32);
    }
    parameters.sort_by(f32::total_cmp);
    let parameter_tolerance = (MIN_POINT_SPACING_M / max_length).min(0.25);
    parameters.dedup_by(|a, b| (*a - *b).abs() <= parameter_tolerance);
    if parameters.len() > MAX_SAMPLES_PER_LANELET {
        return Err(format!(
            "lanelet requires {} surface samples (limit {MAX_SAMPLES_PER_LANELET})",
            parameters.len()
        ));
    }
    Ok(parameters)
}

/// 폴리라인 꼭짓점의 누적 길이를 0..=1 비율로 바꾼다.
fn normalized_vertices(points: &[Vec2], total: f32) -> Vec<f32> {
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(0.0);
    let mut length = 0.0;
    for pair in points.windows(2) {
        length += pair[0].distance(pair[1]);
        parameters.push(length / total);
    }
    parameters
}

/// 폴리라인을 주어진 누적 길이 비율 위치에서 다시 표본화한다.
fn resample(points: &[Vec2], parameters: &[f32]) -> Vec<Vec2> {
    let lengths: Vec<f32> = points
        .windows(2)
        .scan(0.0, |total, pair| {
            *total += pair[0].distance(pair[1]);
            Some(*total)
        })
        .collect();
    let total = lengths[lengths.len() - 1];
    let mut result = Vec::with_capacity(parameters.len());
    let mut segment = 0usize;
    for parameter in parameters {
        let target = total * parameter;
        while segment < lengths.len() - 1 && target > lengths[segment] {
            segment += 1;
        }
        let before = if segment == 0 {
            0.0
        } else {
            lengths[segment - 1]
        };
        let segment_length = lengths[segment] - before;
        let ratio = ((target - before) / segment_length).clamp(0.0, 1.0);
        result.push(points[segment].lerp(points[segment + 1], ratio));
    }
    result
}

/// 폴리라인 전체 길이를 구한다.
fn polyline_length(points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

/// 진행 방향에서 right가 실제로 left의 오른쪽에 있고 경계가 붙지 않는지 검사한다.
fn validate_left_right(id: i64, left: &[Vec2], right: &[Vec2]) -> Result<(), String> {
    for index in 0..left.len() - 1 {
        let forward = (left[index + 1] + right[index + 1]) - (left[index] + right[index]);
        let across = right[index] - left[index];
        let cross = forward.perp_dot(across);
        if cross <= GEOMETRY_EPSILON_M {
            return Err(format!(
                "lanelet {id} left/right boundaries are crossed or reversed at sample {index}"
            ));
        }
    }
    Ok(())
}

/// 좌우 경계를 이어 만든 닫힌 도로 면이 자기 교차하지 않는지 검사한다.
fn validate_simple_surface(id: i64, left: &[Vec2], right: &[Vec2]) -> Result<(), String> {
    let polygon: Vec<Vec2> = left
        .iter()
        .copied()
        .chain(right.iter().rev().copied())
        .collect();
    let edge_count = polygon.len();
    for first in 0..edge_count {
        let first_next = (first + 1) % edge_count;
        for second in first + 1..edge_count {
            let second_next = (second + 1) % edge_count;
            if first == second
                || first_next == second
                || second_next == first
                || (first == 0 && second_next == 0)
            {
                continue;
            }
            if segments_intersect(
                polygon[first],
                polygon[first_next],
                polygon[second],
                polygon[second_next],
            ) {
                return Err(format!("lanelet {id} road surface self-intersects"));
            }
        }
    }
    Ok(())
}

/// 두 닫힌 선분이 끝점 접촉을 포함해 교차하는지 검사한다.
fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let orient = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    if o1.abs() <= GEOMETRY_EPSILON_M && point_on_segment(c, a, b)
        || o2.abs() <= GEOMETRY_EPSILON_M && point_on_segment(d, a, b)
        || o3.abs() <= GEOMETRY_EPSILON_M && point_on_segment(a, c, d)
        || o4.abs() <= GEOMETRY_EPSILON_M && point_on_segment(b, c, d)
    {
        return true;
    }
    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0)
}

/// 점이 주어진 선분의 축 정렬 경계 안에 있는지 검사한다.
fn point_on_segment(point: Vec2, a: Vec2, b: Vec2) -> bool {
    point.x >= a.x.min(b.x) - GEOMETRY_EPSILON_M
        && point.x <= a.x.max(b.x) + GEOMETRY_EPSILON_M
        && point.y >= a.y.min(b.y) - GEOMETRY_EPSILON_M
        && point.y <= a.y.max(b.y) + GEOMETRY_EPSILON_M
}

/// 명시적 중심선이 도로 진행 방향을 따르고 각 점이 대응 경계 폭 안에 있는지 검사한다.
fn validate_centerline(
    id: i64,
    centerline: &[Vec2],
    left: &[Vec2],
    right: &[Vec2],
) -> Result<(), String> {
    let polygon: Vec<Vec2> = left
        .iter()
        .copied()
        .chain(right.iter().rev().copied())
        .collect();
    let mut index = 0usize;
    for pair in centerline.windows(2) {
        let distance = pair[0].distance(pair[1]);
        let steps = (distance / MAX_SAMPLE_SPACING_M).ceil() as usize;
        for step in 0..steps {
            let point = pair[0].lerp(pair[1], step as f32 / steps.max(1) as f32);
            if !point_in_polygon_or_boundary(point, &polygon) {
                return Err(format!(
                    "lanelet {id} centerline sample {index} lies outside its boundaries"
                ));
            }
            index += 1;
        }
    }
    if !point_in_polygon_or_boundary(*centerline.last().expect("centerline has points"), &polygon) {
        return Err(format!(
            "lanelet {id} centerline sample {index} lies outside its boundaries"
        ));
    }
    Ok(())
}

/// 점이 다각형 내부 또는 경계 위에 있는지 홀짝 교차 규칙으로 검사한다.
fn point_in_polygon_or_boundary(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        if (b - a).perp_dot(point - a).abs() <= GEOMETRY_EPSILON_M && point_on_segment(point, a, b)
        {
            return true;
        }
        if (a.y > point.y) != (b.y > point.y) {
            let crossing_x = (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::lanelet2::{LaneletNode, LaneletRelation, LaneletWay, parse_lanelet2};

    /// 저장소의 직선 Lanelet2 fixture를 파싱한다.
    fn straight_map() -> LaneletMap {
        parse_lanelet2(include_bytes!(
            "../../../../testdata/lanelet2/synthetic-straight.osm"
        ))
        .unwrap()
    }

    /// 시험 지도에 제안 원점을 적용해 geometry를 만든다.
    fn build_auto(map: &LaneletMap) -> Result<LaneletGeometryMap, String> {
        build_lanelet_geometry(map, suggest_lanelet_origin(map)?)
    }

    /// 정상 직선 lanelet을 도로 면과 명시적 중심선으로 변환한다.
    #[test]
    fn builds_straight_surface_and_centerline() {
        let result = build_auto(&straight_map()).unwrap();
        let lanelet = &result.lanelets[&301];
        assert_eq!(lanelet.surface.kind, StripKind::Road);
        assert_eq!(lanelet.surface.left.len(), lanelet.surface.right.len());
        assert_eq!(lanelet.centerline.len(), 3);
        assert!((3.5..=4.0).contains(&lanelet.mean_width_m));
        assert_eq!(lanelet.direction, LaneletDirection::OneWay);
        assert!(lanelet.predecessors.is_empty());
        assert!(lanelet.successors.is_empty());
        assert!(result.warnings.is_empty());
    }

    /// 호출자가 지정한 원점이 결과에 보존되고 모든 좌표 투영에 사용된다.
    #[test]
    fn uses_explicit_origin() {
        let map = straight_map();
        let origin = LatLon {
            lat: map.nodes[&1].lat,
            lon: map.nodes[&1].lon,
        };
        let result = build_lanelet_geometry(&map, origin).unwrap();
        assert_eq!(result.origin, origin);
        assert!(result.lanelets[&301].surface.left[0].length() < GEOMETRY_EPSILON_M);
    }

    /// 공유 경계 끝점 쌍으로 직렬·갈림·합류 연결을 ID 순서로 생성한다.
    #[test]
    fn connects_serial_fork_and_merge_lanelets() {
        let mut map = straight_map();
        for (id, lat, lon) in [
            (10, 34.0004, 127.0),
            (11, 34.0004, 127.00004),
            (12, 34.0006, 127.0),
            (13, 34.0006, 127.00004),
        ] {
            map.nodes.insert(id, LaneletNode { id, lat, lon });
        }
        for (id, nodes) in [
            (204, vec![3, 10]),
            (205, vec![6, 11]),
            (206, vec![10, 12]),
            (207, vec![11, 13]),
        ] {
            map.ways.insert(id, LaneletWay { id, nodes });
        }
        for id in [302, 303] {
            map.lanelets.insert(
                id,
                LaneletRelation {
                    id,
                    left: 204,
                    right: 205,
                    centerline: None,
                    direction: LaneletDirection::OneWay,
                },
            );
        }
        map.lanelets.insert(
            304,
            LaneletRelation {
                id: 304,
                left: 206,
                right: 207,
                centerline: None,
                direction: LaneletDirection::OneWay,
            },
        );

        let geometry = build_auto(&map).unwrap();
        assert_eq!(geometry.lanelets[&301].successors, [302, 303]);
        assert_eq!(geometry.lanelets[&302].predecessors, [301]);
        assert_eq!(geometry.lanelets[&302].successors, [304]);
        assert_eq!(geometry.lanelets[&303].predecessors, [301]);
        assert_eq!(geometry.lanelets[&303].successors, [304]);
        assert_eq!(geometry.lanelets[&304].predecessors, [302, 303]);
    }

    /// 지점별 폭은 유효해도 평균이 2~7 m 밖인 비현실적 차선을 거부한다.
    #[test]
    fn rejects_implausible_mean_width() {
        for (width_lon, center_lon) in [(0.000008, 0.000004), (0.00027, 0.000135)] {
            let mut map = straight_map();
            for id in [4, 5, 6] {
                map.nodes.get_mut(&id).unwrap().lon = 127.0 + width_lon;
            }
            for id in [7, 8, 9] {
                map.nodes.get_mut(&id).unwrap().lon = 127.0 + center_lon;
            }
            assert!(build_auto(&map).unwrap_err().contains("mean width"));
        }
    }

    /// 중심선이 없으면 대응시킨 좌우 경계 중점으로 결정적으로 생성한다.
    #[test]
    fn generates_missing_centerline() {
        let mut map = straight_map();
        map.lanelets.get_mut(&301).unwrap().centerline = None;
        let first = build_auto(&map).unwrap();
        let second = build_auto(&map).unwrap();
        assert_eq!(first, second);
        assert!(first.lanelets[&301].centerline.len() >= 6);
        assert!(first.warnings[0].contains("generated from boundary midpoints"));
    }

    /// 반대로 저장된 오른쪽 경계는 진행 방향에 맞게 뒤집는다.
    #[test]
    fn reverses_opposite_boundary_direction() {
        let mut map = straight_map();
        map.ways.get_mut(&202).unwrap().nodes.reverse();
        let geometry = build_auto(&map).unwrap();
        let lanelet = &geometry.lanelets[&301];
        assert!(lanelet.surface.right[0].y > lanelet.surface.right.last().unwrap().y);
    }

    /// 경계 재표본화 뒤에도 원본의 굽은 꼭짓점은 도로 면에 남는다.
    #[test]
    fn resampling_preserves_boundary_vertices() {
        let mut map = straight_map();
        map.nodes.get_mut(&2).unwrap().lon += 0.00001;
        map.nodes.get_mut(&5).unwrap().lon += 0.00001;
        map.lanelets.get_mut(&301).unwrap().centerline = None;
        let geometry = build_auto(&map).unwrap();
        let expected = project(
            geometry.origin,
            LatLon {
                lat: map.nodes[&2].lat,
                lon: map.nodes[&2].lon,
            },
        );
        assert!(
            geometry.lanelets[&301]
                .surface
                .left
                .iter()
                .any(|point| point.distance(expected) < GEOMETRY_EPSILON_M)
        );
    }

    /// 명시적 중심선이 도로 면 밖에 있으면 잘못된 lanelet geometry로 거부한다.
    #[test]
    fn rejects_centerline_outside_surface() {
        let mut map = straight_map();
        map.nodes.insert(
            101,
            LaneletNode {
                id: 101,
                lat: 34.0,
                lon: 127.0002,
            },
        );
        map.nodes.insert(
            102,
            LaneletNode {
                id: 102,
                lat: 34.0002,
                lon: 127.0002,
            },
        );
        map.ways.get_mut(&203).unwrap().nodes = vec![101, 102];
        assert!(build_auto(&map).unwrap_err().contains("centerline sample"));
    }

    /// 지나치게 좁거나 좌우 역할이 바뀐 경계는 도로 면으로 만들지 않는다.
    #[test]
    fn rejects_bad_width_and_swapped_roles() {
        let mut narrow = straight_map();
        narrow.lanelets.get_mut(&301).unwrap().right = 201;
        assert!(build_auto(&narrow).unwrap_err().contains("width"));

        let mut swapped = straight_map();
        let relation = swapped.lanelets.get_mut(&301).unwrap();
        std::mem::swap(&mut relation.left, &mut relation.right);
        assert!(
            build_auto(&swapped)
                .unwrap_err()
                .contains("crossed or reversed")
        );
    }

    /// 교차하는 경계로 생긴 자기 교차 도로 면을 거부한다.
    #[test]
    fn rejects_self_intersecting_surface() {
        let mut map = straight_map();
        map.ways.get_mut(&202).unwrap().nodes.swap(0, 1);
        assert!(build_auto(&map).is_err());
    }
}
