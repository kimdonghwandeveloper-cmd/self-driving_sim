//! 독립 프로세스에서 CWD를 바꿔도 특정 스냅샷에 연결된 건물만 재생되는지 검사한다.
use sim_core::world::{map, places};
use std::path::Path;

/// 부모가 지정한 절대 경로를 다른 CWD에서 열고 비교 가능한 건물 결과를 출력한다.
#[test]
fn replay_child() {
    let Some(route) = std::env::var_os("SIM_BUILDING_REPLAY_ROUTE") else {
        return;
    };
    let course = map::load_map_course(Path::new(&route)).unwrap();
    assert_eq!(course.buildings.len(), 1);
    assert_eq!(course.buildings[0].id, "way/900001");
    println!("BUILDINGS={:?}", course.buildings);
}

/// 같은 경로의 두 CWD 재생·무관한 파일 추가·선택 파일 손상과 누락을 검사한다.
#[test]
fn saved_route_replay_is_independent_of_cwd_and_other_sidecars() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = std::env::temp_dir().join(format!("sim-building-replay-{}", std::process::id()));
    for dir in ["routes", "snapshots", "cwd-a", "cwd-b"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let snapshot = root.join("snapshots/selected.osm.json");
    std::fs::copy(
        repo.join("testdata/yeosu-expo/graph-2026-11-06.osm.json"),
        &snapshot,
    )
    .unwrap();
    let text = std::fs::read_to_string(repo.join("testdata/yeosu-expo/route-junctions.toml"))
        .unwrap()
        .replace(
            "graph-2026-11-06.osm.json",
            "../snapshots/selected.osm.json",
        );
    let route = root.join("routes/saved.toml");
    std::fs::write(&route, text).unwrap();
    let building = r#"{"elements":[{"type":"way","id":900001,"tags":{"building":"yes","height":"12"},"geometry":[{"lat":34.7523,"lon":127.7480},{"lat":34.7523,"lon":127.7481},{"lat":34.7524,"lon":127.7481},{"lat":34.7524,"lon":127.7480},{"lat":34.7523,"lon":127.7480}]}]}"#;
    let sidecar = places::snapshot_places_path(&snapshot);
    std::fs::write(&sidecar, building).unwrap();
    let expected = map::load_map_course(&route).unwrap();
    assert_eq!(expected.buildings.len(), 1);
    for folder in ["routes", "snapshots", "cwd-a", "cwd-b"] {
        std::fs::write(
            root.join(folder).join("000.places.osm.json"),
            building.replace("900001", "999999"),
        )
        .unwrap();
        std::fs::write(
            root.join(folder).join("places.osm.json"),
            "invalid unrelated data",
        )
        .unwrap();
    }
    for cwd in ["cwd-a", "cwd-b"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "replay_child", "--nocapture"])
            .env("SIM_BUILDING_REPLAY_ROUTE", &route)
            .current_dir(root.join(cwd))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("BUILDINGS={:?}", expected.buildings))
        );
    }
    std::fs::write(&sidecar, "invalid selected data").unwrap();
    assert!(map::load_map_course(&route).is_err());
    std::fs::remove_file(&sidecar).unwrap();
    assert!(map::load_map_course(&route).unwrap().buildings.is_empty());
}
