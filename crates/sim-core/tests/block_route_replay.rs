//! 별도 프로세스와 이동한 자료 묶음에서 CWD와 무관한 블록 경로 재생을 검사한다.
use sim_core::world::{
    block_route_file::*,
    osm_graph::parse_overpass_graph,
    road_blocks::{BlockTraversal, build_road_blocks},
    road_network::NetworkSettings,
    route_plan::RoadGraphConfig,
};
use std::path::Path;
/// 부모가 지정한 파일을 임의 CWD에서 재생하고 검증된 결과의 식별값을 출력한다.
#[test]
fn replay_child() {
    let Some(path) = std::env::var_os("SIM_BLOCK_REPLAY_FILE") else {
        return;
    };
    let path = Path::new(&path);
    let file = parse_block_route_file(&std::fs::read_to_string(path).unwrap()).unwrap();
    let result = load_block_route_file(path).unwrap();
    println!("REPLAY={}:{}", file.result_sha256, result.guide.len());
}
/// 다른 CWD와 무관한 파일·이동된 폴더에서도 동일 저장 경로를 오프라인 재생한다.
#[test]
fn replay_is_independent_of_cwd_and_bundle_location() {
    let root = std::env::temp_dir().join(format!(
        "sim-block-cwd-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bundle = root.join("bundle");
    let snapshots = bundle.join("snapshots");
    let routes = bundle.join("routes");
    for dir in [
        &snapshots,
        &routes,
        &root.join("cwd-a"),
        &root.join("cwd-b"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let text = include_str!("../../../testdata/road-network/expo-service.osm.json");
    std::fs::write(snapshots.join("source.osm.json"), text).unwrap();
    let ways = parse_overpass_graph(text).unwrap();
    let graph = RoadGraphConfig::default();
    let model = build_road_blocks(&ways, &[], &graph).unwrap();
    let selected = [BlockTraversal {
        block_id: model.blocks[0].id.clone(),
        direction: model.blocks[0].directions[0],
    }];
    let (file, result) = prepare_block_route_file(
        &routes,
        &BlockRouteRequest {
            name: "expo saved",
            snapshot: "../snapshots/source.osm.json",
            origin: ways[0].nodes[0].pos,
            graph: &graph,
            network: NetworkSettings::default(),
            selected: &selected,
        },
    )
    .unwrap();
    let path = routes.join("saved.toml");
    save_block_route_file(&path, &file).unwrap();
    for folder in ["cwd-a", "cwd-b"] {
        std::fs::write(
            root.join(folder).join("source.osm.json"),
            "unrelated invalid source",
        )
        .unwrap();
        std::fs::write(
            root.join(folder).join("places.osm.json"),
            "unrelated invalid places",
        )
        .unwrap();
    }
    for moved in [false, true] {
        if moved {
            std::fs::rename(&bundle, root.join("relocated")).unwrap();
        }
        let path = if moved {
            root.join("relocated/routes/saved.toml")
        } else {
            path.clone()
        };
        for folder in ["cwd-a", "cwd-b"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "replay_child", "--nocapture"])
                .env("SIM_BLOCK_REPLAY_FILE", &path)
                .current_dir(root.join(folder))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
                "REPLAY={}:{}",
                file.result_sha256,
                result.guide.len()
            )));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
