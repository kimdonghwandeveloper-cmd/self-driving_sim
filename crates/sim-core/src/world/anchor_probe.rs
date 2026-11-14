//! 지역 보정 접속점(anchor) 후보 조사(PLAN 4-11a, 조사표 2절).
//!
//! 내장 OSM 지역 자료에서 확인 좌표 주변 도로 way를 찾아 보정 파일에 적을 OSM node ID, way ID와 인접 노드 쌍,
//! 좌표 거리를 알려 준다. 허용 도로끼리 노드 ID로 이어진 묶음 번호를 함께 보여 주어, 두 점이 보정 전에 서로
//! 끊겨 있는지 확인할 수 있게 한다. 자료를 바꾸지 않고, 좌표가 가깝다는 이유로 연결을 추정하지 않는다.

use std::collections::{BTreeMap, BTreeSet};

use super::map::{LatLon, project};
use super::osm_graph::{OsmGraphWay, node_degrees};

/// 확인 좌표 주변의 way 하나에 대한 접속 후보.
#[derive(Debug, Clone, PartialEq)]
pub struct WayCandidate {
    /// OSM way ID.
    pub way_id: u64,
    /// `highway` 값. 없으면 `None`.
    pub highway: Option<String>,
    /// 경로 탐색 허용 도로인지.
    pub allowed: bool,
    /// `oneway` 태그 값. 없으면 `None`.
    pub oneway: Option<String>,
    /// `name` 태그 값. 없으면 `None`.
    pub name: Option<String>,
    /// 확인 좌표에서 way까지 가장 가까운 거리(m).
    pub distance_m: f64,
    /// 가장 가까운 선분의 앞 노드 번호(way 안 순서).
    pub segment_index: usize,
    /// 가장 가까운 선분의 인접 노드 쌍(`between_nodes`에 쓰는 순서).
    pub between_nodes: [u64; 2],
    /// 선분 위 가장 가까운 점의 앞 노드에서의 비율(0..=1).
    pub fraction: f64,
    /// way 안에서 확인 좌표에 가장 가까운 노드 ID.
    pub nearest_node: u64,
    /// 가장 가까운 노드까지 거리(m).
    pub nearest_node_m: f64,
    /// 가장 가까운 노드의 허용 도로 차수(1이면 막힌 끝). 허용 도로가 아니면 0.
    pub nearest_node_degree: u32,
    /// 허용 도로가 노드 ID로 이어진 묶음 번호(1부터). 허용 도로가 아니면 `None`.
    pub group: Option<usize>,
}

/// 한 확인 좌표의 조사 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct PointProbe {
    /// 확인 좌표.
    pub point: LatLon,
    /// 반경 안의 way 후보(가까운 순, 같으면 way ID 순).
    pub candidates: Vec<WayCandidate>,
}

/// 허용 도로 묶음 하나의 크기.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupSize {
    /// 묶음 번호(1부터).
    pub group: usize,
    /// 묶음에 든 way 수.
    pub ways: usize,
}

/// 여러 확인 좌표의 조사 결과와 허용 도로 묶음 정보.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeReport {
    /// 확인 좌표별 결과(입력 순서).
    pub points: Vec<PointProbe>,
    /// 지역 안 허용 도로 묶음 크기(묶음 번호 순).
    pub groups: Vec<GroupSize>,
}

/// 확인 좌표마다 `radius_m` 안의 way를 찾아 접속 후보를 만든다.
///
/// 거리는 각 확인 좌표를 기준으로 한 지역 평면 근사로 잰다. 묶음 번호는 허용 도로를 노드 ID로만 이어 정하며,
/// 가장 작은 way ID 순서로 번호를 매겨 같은 입력이면 같은 번호가 나온다.
pub fn probe(
    ways: &[OsmGraphWay],
    allowed: &BTreeSet<&str>,
    points: &[LatLon],
    radius_m: f64,
) -> Result<ProbeReport, String> {
    if !(radius_m.is_finite() && radius_m > 0.0) {
        return Err(format!(
            "radius must be a positive number of metres, got {radius_m}"
        ));
    }
    let is_allowed = |way: &OsmGraphWay| way.highway().is_some_and(|kind| allowed.contains(kind));
    let degrees = node_degrees(ways, allowed);
    let (way_groups, groups) = group_ways(ways, &is_allowed);
    let points = points
        .iter()
        .map(|&point| {
            let mut candidates: Vec<WayCandidate> = ways
                .iter()
                .filter(|way| way.nodes.len() >= 2)
                .map(|way| {
                    let allowed = is_allowed(way);
                    candidate(
                        way,
                        point,
                        allowed,
                        &degrees,
                        way_groups.get(&way.id).copied(),
                    )
                })
                .filter(|candidate| candidate.distance_m <= radius_m)
                .collect();
            candidates.sort_by(|left, right| {
                left.distance_m
                    .total_cmp(&right.distance_m)
                    .then(left.way_id.cmp(&right.way_id))
            });
            PointProbe { point, candidates }
        })
        .collect();
    Ok(ProbeReport { points, groups })
}

/// way 하나에서 확인 좌표에 가장 가까운 선분·노드를 찾는다.
fn candidate(
    way: &OsmGraphWay,
    point: LatLon,
    allowed: bool,
    degrees: &BTreeMap<u64, u32>,
    group: Option<usize>,
) -> WayCandidate {
    let local: Vec<(f64, f64)> = way
        .nodes
        .iter()
        .map(|node| {
            let v = project(point, node.pos);
            (f64::from(v.x), f64::from(v.y))
        })
        .collect();
    let mut best = (f64::INFINITY, 0, 0.0);
    for (index, pair) in local.windows(2).enumerate() {
        let (distance, fraction) = point_segment(pair[0], pair[1]);
        if distance < best.0 {
            best = (distance, index, fraction);
        }
    }
    let (node_index, node_m) = local.iter().map(|&(x, z)| x.hypot(z)).enumerate().fold(
        (0, f64::INFINITY),
        |best, (index, distance)| {
            if distance < best.1 {
                (index, distance)
            } else {
                best
            }
        },
    );
    let nearest_node = way.nodes[node_index].id;
    let tag = |key: &str| way.tags.get(key).cloned();
    WayCandidate {
        way_id: way.id,
        highway: tag("highway"),
        allowed,
        oneway: tag("oneway"),
        name: tag("name"),
        distance_m: best.0,
        segment_index: best.1,
        between_nodes: [way.nodes[best.1].id, way.nodes[best.1 + 1].id],
        fraction: best.2,
        nearest_node,
        nearest_node_m: node_m,
        nearest_node_degree: if allowed {
            degrees.get(&nearest_node).copied().unwrap_or(0)
        } else {
            0
        },
        group,
    }
}

/// 원점에서 선분 `a`–`b`까지의 거리(m)와 가장 가까운 점의 비율(0..=1)을 구한다.
fn point_segment(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dz) = (b.0 - a.0, b.1 - a.1);
    let length2 = dx * dx + dz * dz;
    let fraction = if length2 > 0.0 {
        (-(a.0 * dx + a.1 * dz) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, z) = (a.0 + dx * fraction, a.1 + dz * fraction);
    (x.hypot(z), fraction)
}

/// 허용 도로를 공유 노드 ID로 이어 묶고, 묶음마다 가장 작은 way ID 순으로 1부터 번호를 매긴다.
fn group_ways(
    ways: &[OsmGraphWay],
    is_allowed: &dyn Fn(&OsmGraphWay) -> bool,
) -> (BTreeMap<u64, usize>, Vec<GroupSize>) {
    let allowed: Vec<&OsmGraphWay> = ways.iter().filter(|way| is_allowed(way)).collect();
    let mut parent: Vec<usize> = (0..allowed.len()).collect();
    let mut owner: BTreeMap<u64, usize> = BTreeMap::new();
    for (index, way) in allowed.iter().enumerate() {
        for node in &way.nodes {
            match owner.get(&node.id) {
                Some(&other) => union(&mut parent, index, other),
                None => {
                    owner.insert(node.id, index);
                }
            }
        }
    }
    // 묶음 대표 → 묶음 안 가장 작은 way ID와 way 수.
    let mut roots: BTreeMap<usize, (u64, usize)> = BTreeMap::new();
    for (index, way) in allowed.iter().enumerate() {
        let entry = roots
            .entry(find(&mut parent, index))
            .or_insert((u64::MAX, 0));
        entry.0 = entry.0.min(way.id);
        entry.1 += 1;
    }
    let mut ordered: Vec<(u64, usize, usize)> = roots
        .into_iter()
        .map(|(root, (min_id, count))| (min_id, root, count))
        .collect();
    ordered.sort_unstable();
    let numbers: BTreeMap<usize, usize> = ordered
        .iter()
        .enumerate()
        .map(|(number, &(_, root, _))| (root, number + 1))
        .collect();
    let way_groups = allowed
        .iter()
        .enumerate()
        .map(|(index, way)| (way.id, numbers[&find(&mut parent, index)]))
        .collect();
    let groups = ordered
        .iter()
        .enumerate()
        .map(|(number, &(_, _, ways))| GroupSize {
            group: number + 1,
            ways,
        })
        .collect();
    (way_groups, groups)
}

/// 합치기-찾기의 대표를 찾고 경로를 줄인다.
fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

/// 두 묶음을 합친다. 작은 번호를 대표로 둔다.
fn union(parent: &mut [usize], left: usize, right: usize) {
    let (left, right) = (find(parent, left), find(parent, right));
    if left != right {
        parent[left.max(right)] = left.min(right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::fixtures::{FixtureWay, offset, overpass_json};
    use crate::world::osm_graph::parse_overpass_graph;

    /// 합성 지역: 서쪽 도로 두 개(10, 11)가 노드 2에서 이어지고, 동쪽 도로(20)는 떨어져 있으며,
    /// 보행로(30)는 허용 도로가 아니다. 실제 도로가 아니다.
    fn ways() -> Vec<OsmGraphWay> {
        let fixture: Vec<FixtureWay> = vec![
            (
                10,
                "service",
                &[("name", "서쪽길")],
                vec![(1, 0.0, 0.0), (2, 0.0, 40.0)],
            ),
            (11, "service", &[], vec![(2, 0.0, 40.0), (3, 0.0, 80.0)]),
            (
                20,
                "service",
                &[("oneway", "yes")],
                vec![(4, 30.0, 0.0), (5, 30.0, 40.0), (6, 30.0, 80.0)],
            ),
            (30, "footway", &[], vec![(7, 15.0, 0.0), (8, 15.0, 80.0)]),
        ];
        parse_overpass_graph(&overpass_json(&fixture)).unwrap()
    }

    /// 허용 도로 종류(시험용).
    fn allowed() -> BTreeSet<&'static str> {
        BTreeSet::from(["service"])
    }

    /// 선분 중간에 가까운 점: 인접 노드 쌍·비율·거리, 가까운 순 정렬, 반경 밖 제외, 허용 여부를 확인한다.
    #[test]
    fn finds_segment_and_nearest_node() {
        let report = probe(&ways(), &allowed(), &[offset(2.0, 10.0)], 20.0).unwrap();
        let candidates = &report.points[0].candidates;
        let ids: Vec<u64> = candidates.iter().map(|c| c.way_id).collect();
        assert_eq!(ids, vec![10, 30], "way 20은 28 m라 반경 밖");
        let west = &candidates[0];
        assert!((west.distance_m - 2.0).abs() < 0.01, "{west:?}");
        assert_eq!(west.between_nodes, [1, 2]);
        assert_eq!(west.segment_index, 0);
        assert!((west.fraction - 0.25).abs() < 0.01, "{west:?}");
        assert_eq!(west.nearest_node, 1);
        assert_eq!(west.nearest_node_degree, 1, "노드 1은 막힌 끝");
        assert_eq!(west.name.as_deref(), Some("서쪽길"));
        assert!(west.allowed);
        let foot = &candidates[1];
        assert!(!foot.allowed);
        assert_eq!(foot.group, None);
        assert_eq!(foot.nearest_node_degree, 0);
    }

    /// 묶음 번호: 노드 ID를 공유한 서쪽 두 도로는 같은 묶음, 떨어진 동쪽 도로는 다른 묶음이다.
    #[test]
    fn groups_follow_shared_node_ids() {
        let report = probe(
            &ways(),
            &allowed(),
            &[offset(1.0, 60.0), offset(29.0, 60.0)],
            5.0,
        )
        .unwrap();
        let west = &report.points[0].candidates[0];
        let east = &report.points[1].candidates[0];
        assert_eq!((west.way_id, west.group), (11, Some(1)));
        assert_eq!((east.way_id, east.group), (20, Some(2)));
        assert_eq!(east.oneway.as_deref(), Some("yes"));
        assert_eq!(east.between_nodes, [5, 6]);
        assert_eq!(east.nearest_node_degree, 2, "노드 5는 way 가운데");
        assert_eq!(
            report.groups,
            vec![
                GroupSize { group: 1, ways: 2 },
                GroupSize { group: 2, ways: 1 }
            ]
        );
    }

    /// 같은 입력은 순서를 바꿔도 같은 묶음 번호를 내고, 잘못된 반경은 거부한다.
    #[test]
    fn deterministic_and_rejects_bad_radius() {
        let mut reversed = ways();
        reversed.reverse();
        let point = [offset(1.0, 60.0)];
        let a = probe(&ways(), &allowed(), &point, 50.0).unwrap();
        let b = probe(&reversed, &allowed(), &point, 50.0).unwrap();
        assert_eq!(a, b);
        assert!(probe(&ways(), &allowed(), &point, 0.0).is_err());
        assert!(probe(&ways(), &allowed(), &point, f64::NAN).is_err());
    }
}
