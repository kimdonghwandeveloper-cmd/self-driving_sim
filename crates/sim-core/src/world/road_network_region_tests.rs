//! 고정 전체 지역 위에서 원본 도로 순서를 지정한 저장·재생·차량 목표 도달 회귀 시험.
use super::*;
use crate::world::{
    block_route_file,
    road_blocks::{BlockDirection, BlockTraversal, build_road_blocks},
    road_snapshot,
    route_plan::RoadGraphConfig,
};

/// 원본 way의 인접 노드 쌍을 모두 포함하는 블록 통과 순서를 고정한다. 실패 후 다른 도로를 고르지 않는다.
fn source_way_route(
    model: &RoadBlockModel,
    snapshot: &road_snapshot::RoadSnapshot,
    way: u64,
    reversed: bool,
) -> Vec<BlockTraversal> {
    let mut nodes = snapshot
        .ways
        .iter()
        .find(|w| w.id == way)
        .unwrap()
        .nodes
        .clone();
    if reversed {
        nodes.reverse();
    }
    let mut selected = Vec::new();
    for pair in nodes.windows(2) {
        let ids = [pair[0].id, pair[1].id];
        let (block, edge) = model
            .blocks
            .iter()
            .find_map(|b| {
                b.source_edges
                    .iter()
                    .find(|e| e.way_id == way && (e.nodes == ids || e.nodes == [ids[1], ids[0]]))
                    .map(|e| (b, e))
            })
            .unwrap();
        let t = BlockTraversal {
            block_id: block.id.clone(),
            direction: if edge.nodes == ids {
                BlockDirection::Forward
            } else {
                BlockDirection::Backward
            },
        };
        if selected.last() != Some(&t) {
            selected.push(t);
        }
    }
    selected
}
/// 원본 중심선·회전 제한을 유지한 전체 지역에서 고정 경로의 저장·재생·3/8 m/s 목표 도달을 검증한다.
#[test]
fn complete_regions_save_replay_and_drive_fixed_source_roads() {
    for (name, ways) in [
        ("expo", vec![(1286142779, false)]),
        ("gangnam", vec![(129731721, true), (504227065, false)]),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../testdata/junction-regressions/{name}-region.osm.json"
        ));
        let snapshot = road_snapshot::load_road_snapshot(&path).unwrap();
        let config = RoadGraphConfig::default();
        let model = build_road_blocks(&snapshot.ways, &snapshot.restrictions, &config).unwrap();
        let selected: Vec<_> = ways
            .into_iter()
            .flat_map(|(id, rev)| source_way_route(&model, &snapshot, id, rev))
            .collect();
        let origin = if name == "expo" {
            LatLon {
                lat: 34.7526,
                lon: 127.7471,
            }
        } else {
            LatLon {
                lat: 37.4979,
                lon: 127.0276,
            }
        };
        let expected =
            crate::world::block_route::plan_block_route(&model, &selected, 10000.).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "sim-full-region-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let relative = crate::picker::model::relative_path(&dir, &path).unwrap();
        let (file, prepared) = block_route_file::prepare_block_route_file(
            &dir,
            &block_route_file::BlockRouteRequest {
                name: "full-region-regression",
                snapshot: &relative,
                origin,
                graph: &config,
                network: NetworkSettings::default(),
                selected: &selected,
            },
        )
        .expect(name);
        assert_eq!(prepared.plan, expected);
        assert_eq!(prepared.network.blocks.len(), model.blocks.len());
        let saved = dir.join("route.toml");
        block_route_file::save_block_route_file(&saved, &file).unwrap();
        let replay = block_route_file::load_block_route_file(&saved).unwrap();
        assert_eq!(prepared, replay);
        for speed in [3., 8.] {
            let drive =
                crate::world::test_drive::follow_path_to_goal(&replay.course, &replay.guide, speed);
            eprintln!(
                "FULL REGION DRIVE {name}: {:.1} m, {speed} m/s, {drive:?}",
                replay.plan.length_m
            );
            assert!(drive.goal_reached, "{name}: {drive:?}");
            assert_eq!(drive.impacts, 0, "{name}: {drive:?}");
        }
        for path in [saved, dir] {
            if path.is_file() {
                std::fs::remove_file(path).unwrap();
            } else {
                std::fs::remove_dir(path).unwrap();
            }
        }
    }
}
