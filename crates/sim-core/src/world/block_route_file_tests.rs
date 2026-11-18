//! 블록 경로 파일의 자료 잠금·정확 재생·호환성 기대 동작.
use super::*;
use crate::world::{
    block_route::build_block_route_course,
    osm_graph::{fixtures, parse_overpass_graph},
    road_blocks::{BlockDirection, build_road_blocks},
    road_snapshot::sha256_hex,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
/// 각 시험이 독립된 임시 자료와 선택을 갖게 한다.
struct Case {
    root: PathBuf,
    selected: Vec<BlockTraversal>,
}
impl Case {
    /// 서로 다른 이름의 두 도로 블록과 선택 방향을 실제 파서로 준비한다.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sim-block-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let text = fixtures::overpass_json(&[
            (
                10,
                "service",
                &[("name", "start"), ("width", "6")],
                vec![(1, -60.0, 0.0), (2, 0.0, 0.0)],
            ),
            (
                20,
                "service",
                &[("name", "goal"), ("width", "6")],
                vec![(2, 0.0, 0.0), (3, 60.0, 0.0)],
            ),
        ]);
        std::fs::write(root.join("graph.osm.json"), &text).unwrap();
        let m = build_road_blocks(
            &parse_overpass_graph(&text).unwrap(),
            &[],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let selected = [10, 20]
            .iter()
            .map(|id| {
                let b = m
                    .blocks
                    .iter()
                    .find(|b| b.source_edges[0].way_id == *id)
                    .unwrap();
                BlockTraversal {
                    block_id: b.id.clone(),
                    direction: BlockDirection::Forward,
                }
            })
            .collect();
        Self { root, selected }
    }
    /// 명시 입력으로 저장 파일과 코스를 준비한다.
    fn prepare(&self) -> (BlockRouteFile, BlockRouteCourse) {
        prepare_block_route_file(
            &self.root,
            &BlockRouteRequest {
                name: "saved demo",
                snapshot: "graph.osm.json",
                origin: fixtures::ORIGIN,
                graph: &RoadGraphConfig::default(),
                network: NetworkSettings::default(),
                selected: &self.selected,
            },
        )
        .unwrap()
    }
}
impl Drop for Case {
    /// 이 시험이 만든 고유 임시 폴더만 정리한다.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
/// 상대 경로와 설정·선택·확정 순서·결과 지문이 저장 후 동일하게 재생된다.
#[test]
fn saves_and_replays_exact_course_and_guide() {
    let c = Case::new();
    let (file, expected) = c.prepare();
    assert_eq!(file.format, BLOCK_ROUTE_FORMAT);
    assert_eq!(file.selected, c.selected);
    assert_eq!(file.traversals, expected.plan.traversals);
    assert_eq!(file.result_sha256.len(), 64);
    let path = c.root.join("route.toml");
    save_block_route_file(&path, &file).unwrap();
    assert_eq!(expected, load_block_route_file(&path).unwrap());
}
/// TOML 왕복과 반복 직렬화에서 설정·블록 ID·방향·원점이 유지된다.
#[test]
fn toml_roundtrip_is_deterministic() {
    let c = Case::new();
    let (f, _) = c.prepare();
    let text = block_route_file_toml(&f).unwrap();
    assert_eq!(f, parse_block_route_file(&text).unwrap());
    assert_eq!(text, block_route_file_toml(&f).unwrap());
}
/// 내용이 같은 도로라도 원문 바이트가 바뀌면 새 판본으로 취급한다.
#[test]
fn modified_snapshot_is_rejected() {
    let c = Case::new();
    let (f, _) = c.prepare();
    let p = c.root.join("graph.osm.json");
    let mut bytes = std::fs::read(&p).unwrap();
    bytes.push(b' ');
    std::fs::write(&p, bytes).unwrap();
    let e = replay_block_route_file(&f, &c.root).unwrap_err();
    assert!(e.contains("SHA-256"), "{e}");
}
/// 기록된 파일이 없어졌을 때 전국 캐시나 다른 파일을 대신 쓰지 않는다.
#[test]
fn missing_snapshot_is_rejected() {
    let c = Case::new();
    let (f, _) = c.prepare();
    std::fs::remove_file(c.root.join("graph.osm.json")).unwrap();
    assert!(
        replay_block_route_file(&f, &c.root)
            .unwrap_err()
            .contains("graph.osm.json")
    );
}
/// 형식·블록·도로망·차량 안내선 판본 중 하나라도 다르면 거부한다.
#[test]
fn unknown_versions_are_rejected() {
    let c = Case::new();
    let (f, _) = c.prepare();
    for k in 0..4 {
        let mut bad = f.clone();
        match k {
            0 => bad.format = "block-route-v99".into(),
            1 => bad.block_model_version = "future".into(),
            2 => bad.network_version = "future".into(),
            _ => bad.guide_version = "future".into(),
        };
        assert!(replay_block_route_file(&bad, &c.root).is_err());
    }
}
/// 확정 순서를 바꿨을 때 새 최단 경로를 찾아 저장된 순서를 덮지 않는다.
#[test]
fn changed_traversal_is_rejected_without_replanning() {
    let c = Case::new();
    let (mut f, _) = c.prepare();
    f.traversals.reverse();
    assert!(replay_block_route_file(&f, &c.root).is_err());
}
/// 같은 블록 ID라도 잘못된 방향·없는 ID·기록 길이는 재생할 수 없다.
#[test]
fn wrong_direction_identity_and_length_are_rejected() {
    let c = Case::new();
    let (f, _) = c.prepare();
    for k in 0..3 {
        let mut bad = f.clone();
        match k {
            0 => bad.traversals[0].direction = BlockDirection::Backward,
            1 => bad.traversals[0].block_id.0 = "missing".into(),
            _ => bad.length_m += 1.0,
        };
        assert!(replay_block_route_file(&bad, &c.root).is_err());
    }
}
/// 결과가 달라지는 설정·이름·원점 수정은 원래 결과 해시와 불일치한다.
#[test]
fn generation_changes_fail_result_fingerprint() {
    let c = Case::new();
    let (f, _) = c.prepare();
    for k in 0..3 {
        let mut bad = f.clone();
        match k {
            0 => bad.network.curb_thickness_m = 0.2,
            1 => bad.name = "edited".into(),
            _ => bad.origin[1] += 0.00001,
        };
        assert!(replay_block_route_file(&bad, &c.root).is_err());
    }
}
/// 필수 칸 누락·모르는 키·잘못된 해시·절대 파일 경로는 거부한다.
#[test]
fn malformed_format_is_rejected() {
    let c = Case::new();
    let (f, _) = c.prepare();
    let text = block_route_file_toml(&f).unwrap();
    assert!(parse_block_route_file(&(text + "\n[unexpected]\nx=1\n")).is_err());
    let mut bad = f.clone();
    bad.snapshot.sha256 = "wrong".into();
    assert!(block_route_file_toml(&bad).is_err());
    let mut bad = f;
    bad.snapshot.path = c.root.join("graph.osm.json").to_string_lossy().into_owned();
    assert!(block_route_file_toml(&bad).is_err());
}
/// 같은 이름을 다시 저장해 기존 파일을 덮거나 실패한 경로의 불완전 파일을 남기지 않는다.
#[test]
fn save_preserves_existing_file_and_rejects_invalid_data() {
    let c = Case::new();
    let (f, _) = c.prepare();
    let p = c.root.join("saved.toml");
    std::fs::write(&p, "existing").unwrap();
    assert!(save_block_route_file(&p, &f).is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "existing");
    let mut bad = f;
    bad.result_sha256 = "0".repeat(64);
    let p = c.root.join("bad.toml");
    assert!(save_block_route_file(&p, &bad).is_err());
    assert!(!p.exists());
}
/// 새 형식은 기존 공통 코스 로더에서도 열리고 route-v1·Lanelet2 로더도 계속 작동한다.
#[test]
fn map_loader_dispatch_and_existing_formats_are_preserved() {
    let c = Case::new();
    let (f, expected) = c.prepare();
    let p = c.root.join("route.toml");
    save_block_route_file(&p, &f).unwrap();
    assert_eq!(
        crate::world::map::load_map_course(&p).unwrap(),
        expected.course
    );
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in [
        "testdata/yeosu-expo/route-junctions.toml",
        "assets/maps/lanelet2-demo/course.toml",
    ] {
        crate::world::map::load_map_course(&repo.join(path)).unwrap();
    }
}
/// 저장 당시 없던 장소 파일을 나중에 추가해도 옛 저장 결과에 섞이지 않는다.
#[test]
fn unrecorded_places_are_ignored() {
    let c = Case::new();
    let (f, expected) = c.prepare();
    std::fs::write(c.root.join("graph.places.osm.json"), "invalid new data").unwrap();
    assert_eq!(expected, replay_block_route_file(&f, &c.root).unwrap());
}
/// 단순 데이터 배열 비교로 계획 입력을 되만드는 기존 로직과의 기본 계약을 확인한다.
#[test]
fn recorded_hash_matches_explicit_source() {
    let c = Case::new();
    let (f, _) = c.prepare();
    assert_eq!(
        f.snapshot.sha256,
        sha256_hex(&std::fs::read(c.root.join("graph.osm.json")).unwrap())
    );
    let text = std::fs::read_to_string(c.root.join("graph.osm.json")).unwrap();
    let m = build_road_blocks(
        &parse_overpass_graph(&text).unwrap(),
        &[],
        &RoadGraphConfig::default(),
    )
    .unwrap();
    assert_eq!(
        c.prepare().1,
        build_block_route_course(
            "saved demo",
            &m,
            fixtures::ORIGIN,
            NetworkSettings::default(),
            &c.selected,
            10_000.0
        )
        .unwrap()
    );
}

/// 도로 밖의 작은 건물을 실측 주장 없이 합성 자료로 준비한다.
fn building_text(height: f32) -> String {
    let geometry: Vec<_> = [
        (25.0, 20.0),
        (35.0, 20.0),
        (35.0, 30.0),
        (25.0, 30.0),
        (25.0, 20.0),
    ]
    .into_iter()
    .map(|(e, n)| {
        let p = fixtures::offset(e, n);
        serde_json::json!({"lat":p.lat,"lon":p.lon})
    })
    .collect();
    serde_json::json!({"elements":[{"type":"way","id":9001,"tags":{"building":"yes","height":height.to_string()},"geometry":geometry}]}).to_string()
}
/// 기록한 건물의 변경/누락은 조용히 건물 없는 코스로 재생하지 않고 자료 불일치로 거부한다.
#[test]
fn recorded_buildings_are_locked_and_required() {
    let c = Case::new();
    let p = c.root.join("graph.places.osm.json");
    std::fs::write(&p, building_text(12.0)).unwrap();
    let (f, expected) = c.prepare();
    assert!(f.places.is_some());
    assert_eq!(expected.course.buildings.len(), 1);
    assert_eq!(expected, replay_block_route_file(&f, &c.root).unwrap());
    std::fs::write(&p, building_text(18.0)).unwrap();
    assert!(
        replay_block_route_file(&f, &c.root)
            .unwrap_err()
            .contains("SHA-256")
    );
    std::fs::remove_file(&p).unwrap();
    assert!(replay_block_route_file(&f, &c.root).is_err());
}
/// 건물만 바뀐 새 자료 폴더를 만들면 결과 해시는 달라지고 옛 폴더의 경로는 계속 재생된다.
#[test]
fn building_only_revision_keeps_both_generations_replayable() {
    let c = Case::new();
    std::fs::write(c.root.join("graph.places.osm.json"), building_text(12.0)).unwrap();
    let (old, old_course) = c.prepare();
    save_block_route_file(&c.root.join("old.toml"), &old).unwrap();
    let next = c.root.join("next");
    std::fs::create_dir(&next).unwrap();
    std::fs::copy(c.root.join("graph.osm.json"), next.join("graph.osm.json")).unwrap();
    std::fs::write(next.join("graph.places.osm.json"), building_text(18.0)).unwrap();
    let (new, new_course) = prepare_block_route_file(
        &next,
        &BlockRouteRequest {
            name: "saved demo",
            snapshot: "graph.osm.json",
            origin: fixtures::ORIGIN,
            graph: &RoadGraphConfig::default(),
            network: NetworkSettings::default(),
            selected: &c.selected,
        },
    )
    .unwrap();
    assert_eq!(old.snapshot.sha256, new.snapshot.sha256);
    assert_eq!(old.traversals, new.traversals);
    assert_ne!(old.places, new.places);
    assert_ne!(old.result_sha256, new.result_sha256);
    save_block_route_file(&next.join("new.toml"), &new).unwrap();
    assert_eq!(
        old_course,
        load_block_route_file(&c.root.join("old.toml")).unwrap()
    );
    assert_eq!(
        new_course,
        load_block_route_file(&next.join("new.toml")).unwrap()
    );
}
/// Geofabrik 로더가 읽는 메타·연결 자료를 작은 합성 스냅샷에 결속한다.
fn extracted_case(c: &Case, incomplete: bool) {
    use crate::world::road_snapshot::*;
    let path = c.root.join("graph.osm.json");
    let mut graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    graph["generator"] = serde_json::json!("self-driving-sim geofabrik-extract v1");
    let body = serde_json::to_vec(&graph).unwrap();
    std::fs::write(&path, &body).unwrap();
    let allowed = normalized_highways(&RoadGraphConfig::default().allowed_highways);
    let connection = Connectivity {
        format: CONNECTIVITY_FORMAT.into(),
        rule_version: EXTRACT_RULE_VERSION,
        graph_sha256: sha256_hex(&body),
        allowed_highways: allowed.clone(),
        checked_nodes: 3,
        incomplete: if incomplete {
            vec![IncompleteNode {
                node: 2,
                national_degree: 3,
                local_degree: 2,
                missing: vec![[999, 999]],
            }]
        } else {
            vec![]
        },
    };
    let bytes = connection.to_json();
    std::fs::write(c.root.join(CONNECTIVITY_FILE), &bytes).unwrap();
    let meta = SnapshotMetaV2 {
        meta_format: META_FORMAT_V2.into(),
        source: "geofabrik".into(),
        bbox: crate::world::map_fetch::Bbox {
            south: 34.748,
            west: 127.748,
            north: 34.752,
            east: 127.752,
        },
        created_at: "2026-11-18T00:00:00Z".into(),
        graph_file: "graph.osm.json".into(),
        graph_sha256: sha256_hex(&body),
        graph_bytes: body.len() as u64,
        geofabrik: GeofabrikSource {
            pbf_file_name: "synthetic.osm.pbf".into(),
            pbf_url: None,
            pbf_sha256: "0".repeat(64),
            pbf_bytes: 0,
            integrity: "unverified".into(),
            replication_timestamp: None,
            replication_sequence: None,
            data_time: "unknown".into(),
            extract_rule_version: EXTRACT_RULE_VERSION,
            margin_m: 150.0,
            allowed_highways: allowed,
        },
        connectivity: ConnectivityRecord {
            file: CONNECTIVITY_FILE.into(),
            format: CONNECTIVITY_FORMAT.into(),
            sha256: sha256_hex(bytes.as_bytes()),
            checked_nodes: 3,
            incomplete_nodes: connection.incomplete.len() as u64,
        },
    };
    std::fs::write(c.root.join("meta.toml"), toml::to_string(&meta).unwrap()).unwrap();
}
/// 메타·연결 자료를 함께 기록하고 각 파일이 바뀌거나 빠지면 거부한다.
#[test]
fn extract_dependencies_are_locked() {
    let c = Case::new();
    extracted_case(&c, false);
    let (f, expected) = c.prepare();
    assert!(f.metadata.is_some());
    assert!(f.connectivity.is_some());
    assert_eq!(expected, replay_block_route_file(&f, &c.root).unwrap());
    for name in ["meta.toml", "connectivity.json"] {
        let p = c.root.join(name);
        let original = std::fs::read(&p).unwrap();
        let mut changed = original.clone();
        changed.push(b' ');
        std::fs::write(&p, &changed).unwrap();
        assert!(
            replay_block_route_file(&f, &c.root)
                .unwrap_err()
                .contains("SHA-256")
        );
        std::fs::remove_file(&p).unwrap();
        assert!(replay_block_route_file(&f, &c.root).is_err());
        std::fs::write(p, original).unwrap();
    }
}
/// 경로 위에 지역 밖 연결이 빠진 노드가 있으면 불완전한 스냅샷으로 새 파일을 만들지 않는다.
#[test]
fn incomplete_extract_is_rejected() {
    let c = Case::new();
    extracted_case(&c, true);
    let error = prepare_block_route_file(
        &c.root,
        &BlockRouteRequest {
            name: "bad",
            snapshot: "graph.osm.json",
            origin: fixtures::ORIGIN,
            graph: &RoadGraphConfig::default(),
            network: NetworkSettings::default(),
            selected: &c.selected,
        },
    )
    .unwrap_err();
    assert!(error.contains("outside the extract"), "{error}");
}
/// 저장된 유효한 우회 통과 순서를 재생할 때 지금의 최단 경로로 단축하지 않는다.
#[test]
fn replay_uses_recorded_walk_without_shortest_path_search() {
    let mut c = Case::new();
    let text = fixtures::overpass_json(&[
        (
            10,
            "service",
            &[("name", "start"), ("width", "6")],
            vec![(1, -60.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("name", "up"), ("width", "6")],
            vec![(2, 0.0, 0.0), (3, 0.0, 60.0)],
        ),
        (
            21,
            "service",
            &[("name", "across"), ("width", "6")],
            vec![(3, 0.0, 60.0), (4, 100.0, 60.0)],
        ),
        (
            22,
            "service",
            &[("name", "down"), ("width", "6")],
            vec![(4, 100.0, 60.0), (5, 100.0, 0.0)],
        ),
        (
            30,
            "service",
            &[("name", "shortcut"), ("width", "6")],
            vec![(2, 0.0, 0.0), (5, 100.0, 0.0)],
        ),
        (
            40,
            "service",
            &[("name", "goal"), ("width", "6")],
            vec![(5, 100.0, 0.0), (6, 160.0, 0.0)],
        ),
    ]);
    std::fs::write(c.root.join("graph.osm.json"), &text).unwrap();
    let m = build_road_blocks(
        &parse_overpass_graph(&text).unwrap(),
        &[],
        &RoadGraphConfig::default(),
    )
    .unwrap();
    c.selected = [10, 20, 40]
        .iter()
        .map(|id| {
            let b = m
                .blocks
                .iter()
                .find(|b| b.source_edges[0].way_id == *id)
                .unwrap();
            BlockTraversal {
                block_id: b.id.clone(),
                direction: BlockDirection::Forward,
            }
        })
        .collect();
    let (mut f, _) = c.prepare();
    f.selected.remove(1);
    let plan = block_route::BlockRoutePlan {
        selected: f.selected.clone(),
        traversals: f.traversals.clone(),
        length_m: f.length_m,
    };
    let expected = block_route::build_resolved_block_route_course(
        &f.name,
        &m,
        fixtures::ORIGIN,
        NetworkSettings::default(),
        plan,
        10_000.0,
    )
    .unwrap();
    f.result_sha256 = result_fingerprint(&expected);
    assert!(
        block_route::plan_block_route(&m, &f.selected, 10_000.0)
            .unwrap()
            .length_m
            + 100.0
            < f.length_m
    );
    assert_eq!(expected, replay_block_route_file(&f, &c.root).unwrap());
}

/// 소수 길이의 여러 블록을 지나는 경로도 합산 묶음 차이로 저장/재생이 거부되지 않아야 한다.
#[test]
fn fractional_block_lengths_roundtrip() {
    let mut c = Case::new();
    for seed in 0..16 {
        let seed = seed as f64;
        let lengths = [
            23.123456789 + seed * 0.01,
            33.23456789 + seed * 0.013,
            43.3456789 + seed * 0.017,
            53.45678912 + seed * 0.019,
        ];
        let mut x = [0.0; 5];
        for i in 0..4 {
            x[i + 1] = x[i] + lengths[i];
        }
        let text = fixtures::overpass_json(&[
            (
                10,
                "service",
                &[("name", "a"), ("width", "6")],
                vec![(1, x[0], 0.0), (2, x[1], 0.0)],
            ),
            (
                20,
                "service",
                &[("name", "b"), ("width", "6")],
                vec![(2, x[1], 0.0), (3, x[2], 0.0)],
            ),
            (
                30,
                "service",
                &[("name", "c"), ("width", "6")],
                vec![(3, x[2], 0.0), (4, x[3], 0.0)],
            ),
            (
                40,
                "service",
                &[("name", "d"), ("width", "6")],
                vec![(4, x[3], 0.0), (5, x[4], 0.0)],
            ),
        ]);
        std::fs::write(c.root.join("graph.osm.json"), &text).unwrap();
        let m = build_road_blocks(
            &parse_overpass_graph(&text).unwrap(),
            &[],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        c.selected = [10, 40]
            .iter()
            .map(|id| {
                let b = m
                    .blocks
                    .iter()
                    .find(|b| b.source_edges[0].way_id == *id)
                    .unwrap();
                BlockTraversal {
                    block_id: b.id.clone(),
                    direction: BlockDirection::Forward,
                }
            })
            .collect();
        let (f, expected) = c.prepare();
        assert_eq!(expected, replay_block_route_file(&f, &c.root).unwrap());
    }
}
