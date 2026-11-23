//! 출발 방향과 실제 공유 노드 연결에 따른 도로 생성 범위 선택.
use super::super::{
    map::LatLon,
    road_blocks::{BlockId, RoadBlockModel},
};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

/// 경로 통행 규칙이 아닌 물리적 연결을 따라 출발 방향 앞쪽의 완전한 블록만 선택한다.
pub fn select_forward_roads(
    model: &RoadBlockModel,
    anchor: &BlockId,
    distance_m: f32,
    origin: LatLon,
    reversed: bool,
    incomplete: &BTreeSet<u64>,
) -> Result<RoadBlockModel, String> {
    use crate::world::{map::project, road_geom};
    if !distance_m.is_finite()
        || !(100.0..=10000.0).contains(&distance_m)
        || !origin.lat.is_finite()
        || !origin.lon.is_finite()
        || origin.lat.abs() > 85.
        || origin.lon.abs() > 180.
    {
        return Err("invalid forward road selection range or origin".into());
    }
    let seed = model
        .blocks
        .iter()
        .find(|b| &b.id == anchor)
        .ok_or("selected road does not exist")?;
    let line: Vec<_> = seed.nodes.iter().map(|n| project(origin, n.pos)).collect();
    let (center, heading) =
        road_geom::point_at_distance(&line, road_geom::polyline_length(&line) / 2.)
            .ok_or("selected road has no centerline")?;
    let heading = heading * if reversed { -1. } else { 1. };
    let first = seed.nodes.first().ok_or("selected road has no nodes")?.id;
    let last = seed.nodes.last().unwrap().id;
    let (rear, exit) = if reversed {
        (last, first)
    } else {
        (first, last)
    };
    if incomplete.contains(&exit) {
        return Err(
            "출발 방향의 연결이 자료 경계에서 끊겨 있습니다. 더 넓은 지역을 열어 주세요".into(),
        );
    }
    let mut adjacent: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, b) in model.blocks.iter().enumerate() {
        for n in [b.nodes.first(), b.nodes.last()].into_iter().flatten() {
            adjacent.entry(n.id).or_default().push(i);
        }
    }
    let mut ids = BTreeSet::from([anchor.clone()]);
    let mut distance = BTreeMap::from([(exit, seed.length_m / 2.)]);
    let mut queue = BinaryHeap::from([std::cmp::Reverse(((seed.length_m / 2.).to_bits(), exit))]);
    let rear_margin = 8.0_f32.max(seed.attributes.width_m);
    while let Some(std::cmp::Reverse((bits, node))) = queue.pop() {
        let traveled = f64::from_bits(bits);
        if traveled > distance[&node] || traveled >= distance_m as f64 {
            continue;
        }
        for &index in adjacent.get(&node).into_iter().flatten() {
            let b = &model.blocks[index];
            if &b.id == anchor || b.nodes.len() < 2 || !b.length_m.is_finite() || b.length_m <= 0. {
                continue;
            }
            let a = b.nodes[0].id;
            let z = b.nodes.last().unwrap().id;
            let next = if a == node { z } else { a };
            if next == rear && rear != exit {
                continue;
            }
            if b.nodes.iter().any(|n| {
                incomplete.contains(&n.id)
                    || (project(origin, n.pos) - center).dot(heading) < -rear_margin
            }) {
                continue;
            }
            let total = traveled + b.length_m;
            if total > distance_m as f64 + 0.01 {
                continue;
            }
            ids.insert(b.id.clone());
            if distance.get(&next).is_none_or(|&old| total < old) {
                distance.insert(next, total);
                queue.push(std::cmp::Reverse((total.to_bits(), next)));
            }
        }
    }
    let mut selected = model.clone();
    selected.blocks.retain(|b| ids.contains(&b.id));
    selected.blocks.sort_by(|a, b| a.id.cmp(&b.id));
    selected
        .connections
        .retain(|c| ids.contains(&c.from.block_id) && ids.contains(&c.to.block_id));
    Ok(selected)
}

/// 범위 밖에서 끝나는 고립 끝점과 명시적인 추출 연결 불완전 정보를 합친다.
/// 범위 안의 정상적인 막다른길을 추출 오류로 추측하지 않는다.
pub fn incomplete_nodes(
    model: &RoadBlockModel,
    bounds: Option<[f64; 4]>,
    known: &BTreeSet<u64>,
) -> Result<BTreeSet<u64>, String> {
    let mut ids = known.clone();
    let Some([south, west, north, east]) = bounds else {
        return Ok(ids);
    };
    if ![south, west, north, east].iter().all(|v| v.is_finite())
        || south >= north
        || west >= east
        || south < -85.
        || north > 85.
        || west < -180.
        || east > 180.
    {
        return Err("invalid source bounds".into());
    }
    let mut adjacent: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for block in &model.blocks {
        for pair in block.nodes.windows(2) {
            adjacent.entry(pair[0].id).or_default().insert(pair[1].id);
            adjacent.entry(pair[1].id).or_default().insert(pair[0].id);
        }
    }
    for node in model.blocks.iter().flat_map(|b| &b.nodes) {
        if adjacent.get(&node.id).is_some_and(|n| n.len() == 1)
            && (node.pos.lat < south
                || node.pos.lat > north
                || node.pos.lon < west
                || node.pos.lon > east)
        {
            ids.insert(node.id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        osm_graph::{fixtures, parse_graph_full},
        road_blocks::build_road_blocks,
        route_plan::RoadGraphConfig,
    };

    /// 북쪽·남쪽 연결, 남쪽 곁가지, 연결 없는 평행 도로와 예산을 넘는 가지를 만든다.
    fn fixture() -> RoadBlockModel {
        let json = fixtures::overpass_json(&[
            (
                100,
                "service",
                &[("name", "seed")],
                vec![(1, 0., 40.), (2, 0., 0.)],
            ),
            (
                200,
                "service",
                &[("name", "south")],
                vec![(2, 0., 0.), (3, 0., -60.)],
            ),
            (
                300,
                "service",
                &[("name", "north")],
                vec![(4, 0., 100.), (1, 0., 40.)],
            ),
            (400, "service", &[], vec![(10, 20., 0.), (11, 20., -80.)]),
            (500, "service", &[], vec![(3, 0., -60.), (7, 80., -60.)]),
            (600, "service", &[], vec![(3, 0., -60.), (8, 0., -700.)]),
        ]);
        let source = parse_graph_full(&json).unwrap();
        build_road_blocks(
            &source.ways,
            &source.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap()
    }

    /// 원본 way ID로 분할된 블록을 찾는다.
    fn anchor(model: &RoadBlockModel) -> BlockId {
        model
            .blocks
            .iter()
            .find(|b| b.source_edges.iter().any(|e| e.way_id == 100))
            .unwrap()
            .id
            .clone()
    }

    /// 선택 결과가 포함한 원본 way 목록을 정렬해 돌려준다.
    fn ways(model: &RoadBlockModel) -> BTreeSet<u64> {
        model
            .blocks
            .iter()
            .flat_map(|b| b.source_edges.iter().map(|e| e.way_id))
            .collect()
    }

    /// 남쪽 선택은 북쪽·고립 도로를 제외하고 연결된 남쪽 도로와 곁가지만 생성한다.
    #[test]
    fn south_selection_does_not_include_north_or_disconnected_roads() {
        let model = fixture();
        let id = anchor(&model);
        let origin = model.blocks.iter().find(|b| b.id == id).unwrap().nodes[0].pos;
        let south =
            select_forward_roads(&model, &id, 500., origin, false, &BTreeSet::new()).unwrap();
        assert_eq!(ways(&south), BTreeSet::from([100, 200, 500]));
        let north =
            select_forward_roads(&model, &id, 500., origin, true, &BTreeSet::new()).unwrap();
        assert_eq!(ways(&north), BTreeSet::from([100, 300]));
    }

    /// 자료 경계에서 불완전하다고 확인된 가지를 파란 생성 도로에 포함하지 않는다.
    #[test]
    fn incomplete_branch_is_not_generated_but_real_dead_end_is_kept() {
        let model = fixture();
        let id = anchor(&model);
        let origin = model.blocks.iter().find(|b| b.id == id).unwrap().nodes[0].pos;
        let selected =
            select_forward_roads(&model, &id, 500., origin, false, &BTreeSet::from([7])).unwrap();
        assert_eq!(ways(&selected), BTreeSet::from([100, 200]));
        assert!(
            select_forward_roads(&model, &id, 500., origin, false, &BTreeSet::from([2])).is_err()
        );
    }

    /// 지역 밖의 끊긴 끝점만 경계로 분류하고 안쪽 막다른길은 남긴다.
    #[test]
    fn source_boundary_does_not_turn_real_dead_ends_into_missing_roads() {
        let model = fixture();
        let id = anchor(&model);
        let at = model.blocks.iter().find(|b| b.id == id).unwrap().nodes[0].pos;
        let bounds = Some([
            at.lat - 0.002,
            at.lon - 0.002,
            at.lat + 0.002,
            at.lon + 0.002,
        ]);
        let ids = incomplete_nodes(&model, bounds, &BTreeSet::from([7])).unwrap();
        assert!(ids.contains(&8) && ids.contains(&7));
        assert!(!ids.contains(&4) && !ids.contains(&11));
        assert!(incomplete_nodes(&model, Some([1., 1., 0., 0.]), &BTreeSet::new()).is_err());
    }

    /// 실제 엑스포 내부 남쪽 도로를 고르면 역·북쪽 도로와 고립 조각을 생성하지 않는다.
    #[test]
    fn expo_internal_south_direction_excludes_station_side() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/junction-regressions/expo-region.osm.json");
        let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
        let model = build_road_blocks(
            &snapshot.ways,
            &snapshot.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let root = model
            .blocks
            .iter()
            .find(|b| {
                b.source_edges.iter().any(|e| e.way_id == 532526349)
                    && (b.nodes[0].pos.lat - b.nodes.last().unwrap().pos.lat).abs() > 0.00005
            })
            .unwrap();
        let origin = root.nodes[0].pos;
        let reversed = root.nodes[0].pos.lat < root.nodes.last().unwrap().pos.lat;
        let selected =
            select_forward_roads(&model, &root.id, 500., origin, reversed, &BTreeSet::new())
                .unwrap();
        let square = super::super::select_road_area(&model, &root.id, 500., origin).unwrap();
        assert!(
            selected.blocks.len() < square.blocks.len(),
            "direction must narrow the previous square selection"
        );
        assert!(!ways(&selected).contains(&167055747));
        assert!(ways(&selected).contains(&532526349));
    }

    /// 일방통행·접근 제한·입력 순서로 물리 연결 선택이 달라지지 않는다.
    #[test]
    fn forward_selection_is_deterministic_without_navigation_rules() {
        let mut model = fixture();
        let id = anchor(&model);
        let origin = model.blocks.iter().find(|b| b.id == id).unwrap().nodes[0].pos;
        let expected = ways(
            &select_forward_roads(&model, &id, 500., origin, false, &BTreeSet::new()).unwrap(),
        );
        model.blocks.reverse();
        model.connections.clear();
        for block in &mut model.blocks {
            block.directions.clear();
        }
        assert_eq!(
            ways(
                &select_forward_roads(&model, &id, 500., origin, false, &BTreeSet::new()).unwrap()
            ),
            expected
        );
    }
}
