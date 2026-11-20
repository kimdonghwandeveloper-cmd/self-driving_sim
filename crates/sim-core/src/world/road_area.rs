//! 출발·도착·방향 선택 없는 도로 영역 생성과 자유 주행 코스.
use super::{
    course::CourseDef,
    map::LatLon,
    road_blocks::{BlockId, RoadBlockModel},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 자료와 생성 규칙을 고정해 경로 탐색 없이 재생하는 자유 주행 파일.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadAreaFile {
    pub format: String,
    pub network_version: String,
    pub block_version: String,
    pub snapshot: super::block_route_file::LockedFile,
    pub dependencies: Vec<super::block_route_file::LockedFile>,
    pub blocks: super::block_route_file::BlockBuildSettings,
    pub origin: [f64; 2],
    pub anchor: BlockId,
    pub span_m: f32,
    pub result_sha256: String,
}
/// 자유 주행 저장 형식.
pub const ROAD_AREA_FORMAT: &str = "road-area-v1";

/// 원본·부가 자료와 생성 결과를 묶어 주행 직전 변경 여부를 확인한다.
pub fn prepare_road_area(
    base: &Path,
    snapshot: &str,
    config: &super::route_plan::RoadGraphConfig,
    anchor: &BlockId,
    span_m: f32,
    origin: LatLon,
) -> Result<(RoadAreaFile, CourseDef), String> {
    let lock = |path: &Path| -> Result<super::block_route_file::LockedFile, String> {
        let relative = crate::picker::model::relative_path(base, path)?;
        Ok(super::block_route_file::LockedFile {
            path: relative,
            sha256: super::road_snapshot::sha256_hex(
                &std::fs::read(path).map_err(|e| e.to_string())?,
            ),
        })
    };
    let path = base.join(snapshot);
    let source = super::road_snapshot::load_road_snapshot(&path)?;
    let mut dependencies = Vec::new();
    if source.connectivity.is_some() {
        for name in ["meta.toml", super::road_snapshot::CONNECTIVITY_FILE] {
            dependencies.push(lock(&path.with_file_name(name))?);
        }
    }
    let places = super::places::snapshot_places_path(&path);
    if places.exists() {
        dependencies.push(lock(&places)?);
    }
    let mut file = RoadAreaFile {
        format: ROAD_AREA_FORMAT.into(),
        network_version: super::road_network::NETWORK_GEOMETRY_VERSION.into(),
        block_version: super::road_blocks::BLOCK_MODEL_VERSION.into(),
        snapshot: lock(&path)?,
        dependencies,
        blocks: super::block_route_file::BlockBuildSettings {
            allowed_highways: config.allowed_highways.clone(),
            lane_width_m: config.lane_width_m,
            default_width_m: config.default_width_m.clone(),
        },
        origin: [origin.lat, origin.lon],
        anchor: anchor.clone(),
        span_m,
        result_sha256: String::new(),
    };
    let course = rebuild(&file, base)?;
    file.result_sha256 = super::road_snapshot::sha256_hex(format!("{course:?}").as_bytes());
    Ok((file, course))
}

/// 저장한 파일만 읽고 원본 또는 결과가 달라졌으면 새 경로로 대체하지 않고 거부한다.
pub fn replay_road_area(file: &RoadAreaFile, base: &Path) -> Result<CourseDef, String> {
    let course = rebuild(file, base)?;
    if super::road_snapshot::sha256_hex(format!("{course:?}").as_bytes()) != file.result_sha256 {
        return Err("road area result SHA-256 mismatch".into());
    }
    Ok(course)
}

/// 고정 자료로 도로·건물을 재생한다. 작업 디렉터리나 전국 캐시를 탐색하지 않는다.
fn rebuild(file: &RoadAreaFile, base: &Path) -> Result<CourseDef, String> {
    if file.format != ROAD_AREA_FORMAT
        || file.network_version != super::road_network::NETWORK_GEOMETRY_VERSION
        || file.block_version != super::road_blocks::BLOCK_MODEL_VERSION
    {
        return Err("unsupported road area version".into());
    }
    for source in std::iter::once(&file.snapshot).chain(file.dependencies.iter()) {
        if Path::new(&source.path).is_absolute() {
            return Err("road area inputs must be relative".into());
        }
        let bytes = std::fs::read(base.join(&source.path)).map_err(|e| e.to_string())?;
        if super::road_snapshot::sha256_hex(&bytes) != source.sha256 {
            return Err(format!("road area input SHA-256 mismatch: {}", source.path));
        }
    }
    let path = base.join(&file.snapshot.path);
    let snapshot = super::road_snapshot::load_road_snapshot(&path)?;
    let connectivity_locked = file.dependencies.iter().any(|d| {
        base.join(&d.path) == path.with_file_name(super::road_snapshot::CONNECTIVITY_FILE)
    });
    if snapshot.connectivity.is_some() != connectivity_locked {
        return Err("road area connectivity dependencies changed".into());
    }
    let expected = [
        path.with_file_name("meta.toml"),
        path.with_file_name(super::road_snapshot::CONNECTIVITY_FILE),
        super::places::snapshot_places_path(&path),
    ];
    let mut unique = std::collections::BTreeSet::new();
    for dependency in &file.dependencies {
        let dependency_path = base.join(&dependency.path);
        if !expected.contains(&dependency_path) || !unique.insert(dependency_path) {
            return Err("unexpected road area dependency".into());
        }
    }
    if connectivity_locked && !unique.contains(&path.with_file_name("meta.toml")) {
        return Err("missing road area metadata lock".into());
    }
    let config = super::route_plan::RoadGraphConfig {
        allowed_highways: file.blocks.allowed_highways.clone(),
        lane_width_m: file.blocks.lane_width_m,
        default_width_m: file.blocks.default_width_m.clone(),
        ..Default::default()
    };
    let model =
        super::road_blocks::build_road_blocks(&snapshot.ways, &snapshot.restrictions, &config)?;
    let origin = LatLon {
        lat: file.origin[0],
        lon: file.origin[1],
    };
    let mut course = build_road_area(&model, &file.anchor, file.span_m, origin)?;
    let places_path = super::places::snapshot_places_path(&path);
    if file
        .dependencies
        .iter()
        .any(|p| base.join(&p.path) == places_path)
    {
        let selected = select_road_area(&model, &file.anchor, file.span_m, origin)?;
        let lines: Vec<_> = selected
            .blocks
            .iter()
            .map(|b| {
                b.nodes
                    .iter()
                    .map(|n| super::map::project(origin, n.pos))
                    .collect()
            })
            .collect();
        let (buildings, estimated) = super::building::load_buildings_file(
            &places_path,
            origin,
            &lines,
            &course.surfaces,
            &course.strips,
            &course.areas,
        )?;
        course.buildings = buildings;
        if estimated {
            course
                .notices
                .push(super::building::ESTIMATED_HEIGHT_NOTICE.into());
        }
    }
    course.validate().map_err(|e| e.to_string())?;
    Ok(course)
}

/// 이미 있는 파일은 덮어쓰지 않고 검증한 자유 주행 파일을 새로 저장한다.
pub fn save_road_area(path: &Path, file: &RoadAreaFile) -> Result<(), String> {
    use std::io::Write;
    replay_road_area(file, path.parent().ok_or("missing parent")?)?;
    let text = toml::to_string_pretty(file).map_err(|e| e.to_string())?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = path.with_file_name(format!(
        ".road-area-{}-{}.partial",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let written = output
        .write_all(text.as_bytes())
        .and_then(|()| output.sync_all());
    drop(output);
    let published = written.and_then(|()| std::fs::hard_link(&temporary, path));
    let _ = std::fs::remove_file(&temporary);
    published.map_err(|e| e.to_string())
}

/// 선택한 도로를 중심으로 주변 도로 전체를 생성한다.
pub fn build_road_area(
    model: &RoadBlockModel,
    anchor: &BlockId,
    span_m: f32,
    origin: LatLon,
) -> Result<CourseDef, String> {
    use super::{
        map, road_geom,
        road_network::{NetworkSettings, build_road_network},
    };
    use crate::vehicle::{Motion, VEHICLE_FOOTPRINT, collision};
    let selected = select_road_area(model, anchor, span_m, origin)?;
    let network = build_road_network(&selected, origin, NetworkSettings::default())
        .map_err(|e| format!("road area geometry: {e:?}"))?;
    let shape = network
        .blocks
        .iter()
        .find(|b| &b.block_id == anchor)
        .ok_or("selected road has no surface")?;
    let obstacles: Vec<_> = network.colliders.iter().map(|c| c.rect).collect();
    let width = selected
        .blocks
        .iter()
        .find(|b| &b.id == anchor)
        .unwrap()
        .attributes
        .width_m;
    if width < VEHICLE_FOOTPRINT.width_m + 0.4 {
        return Err("선택한 도로가 차량을 배치하기에 너무 좁습니다".into());
    }
    let mut start = None;
    // 잘린 원본 중심선의 직선 부분에서 차체 전체가 들어가는 자세를 결정적으로 찾는다.
    for pair in shape.centerline.windows(2) {
        if pair[0].distance(pair[1]) < VEHICLE_FOOTPRINT.length_m + 1.0 {
            continue;
        }
        let direction = (pair[1] - pair[0]).normalize();
        let center = pair[0].lerp(pair[1], 0.5);
        let rear = center - direction * VEHICLE_FOOTPRINT.rear_axle_to_center_m;
        let pose = Motion {
            x_m: rear.x,
            z_m: rear.y,
            heading_rad: road_geom::heading_of(direction),
            ..Motion::default()
        };
        if !collision::in_contact(&VEHICLE_FOOTPRINT, &pose, &obstacles) {
            start = Some(pose);
            break;
        }
    }
    let start = start
        .ok_or("선택한 도로에 차량을 안전하게 놓을 공간이 없습니다. 인접한 긴 도로를 선택하세요")?;
    let course = map::finish_course(
        "선택한 도로 영역 · 자유 주행",
        vec![
            map::OSM_ATTRIBUTION.into(),
            "자유 주행 · 목표 지점 없음 · 영역 끝 도로는 이어지지 않을 수 있습니다".into(),
        ],
        start,
        network
            .blocks
            .iter()
            .flat_map(|b| b.strips.clone())
            .collect(),
        network
            .blocks
            .iter()
            .flat_map(|b| b.areas.clone())
            .chain(network.joints.iter().flat_map(|j| j.areas.clone()))
            .collect(),
        network.colliders,
        (24100., 24100.),
    )?;
    course.validate().map_err(|e| e.to_string())?;
    Ok(course)
}

/// 선택 도로 가운데를 중심으로 사각 영역과 교차하는 블록을 원형 그대로 보존한다.
/// 도로 끝을 임의로 자르지 않아 선택 범위 바깥으로 완전한 블록이 이어질 수 있다.
pub fn select_road_area(
    model: &RoadBlockModel,
    anchor: &BlockId,
    span_m: f32,
    origin: LatLon,
) -> Result<RoadBlockModel, String> {
    use super::map::project;
    if !span_m.is_finite() || !(100.0..=10000.0).contains(&span_m) {
        return Err("생성 범위는 100 m~10 km입니다".into());
    }
    let b = model
        .blocks
        .iter()
        .find(|b| &b.id == anchor)
        .ok_or("선택한 도로를 찾을 수 없습니다")?;
    let line: Vec<_> = b.nodes.iter().map(|n| project(origin, n.pos)).collect();
    if line.len() < 2 {
        return Err("선택한 도로의 좌표가 부족합니다".into());
    }
    let center =
        super::road_geom::point_at_distance(&line, super::road_geom::polyline_length(&line) / 2.0)
            .ok_or("선택한 도로의 길이가 부족합니다")?
            .0;
    let half = span_m / 2.;
    let mut selected = model.clone();
    selected.blocks.retain(|b| {
        &b.id == anchor
            || b.nodes.windows(2).any(|p| {
                let a = project(origin, p[0].pos) - center;
                let z = project(origin, p[1].pos) - center;
                let d = z - a;
                let (mut low, mut high) = (0.0_f32, 1.0_f32);
                for axis in 0..2 {
                    if d[axis].abs() < 1e-6 {
                        if a[axis].abs() > half {
                            return false;
                        }
                    } else {
                        let u = (-half - a[axis]) / d[axis];
                        let v = (half - a[axis]) / d[axis];
                        low = low.max(u.min(v));
                        high = high.min(u.max(v));
                    }
                }
                low <= high
            })
    });
    selected.blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: std::collections::BTreeSet<_> = selected.blocks.iter().map(|b| b.id.clone()).collect();
    selected
        .connections
        .retain(|c| ids.contains(&c.from.block_id) && ids.contains(&c.to.block_id));
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        road_blocks::build_road_blocks, road_snapshot::load_road_snapshot,
        route_plan::RoadGraphConfig,
    };

    /// 실제 엑스포 표본으로 경로 역할 없이 도로망을 구성한다.
    fn fixture() -> RoadBlockModel {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/block-real/expo-internal.osm.json");
        let snapshot = load_road_snapshot(&path).unwrap();
        build_road_blocks(
            &snapshot.ways,
            &snapshot.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap()
    }

    /// 도로 하나만 고르면 목표와 안내 경로 없이 실제 노면·연석과 안전한 시작 자세를 만든다.
    #[test]
    fn one_road_generates_free_drive_without_endpoints() {
        let model = fixture();
        let anchor = &model.blocks[0];
        let course = build_road_area(&model, &anchor.id, 500., anchor.nodes[0].pos).unwrap();
        assert!(course.goal.is_none());
        assert!(!course.strips.is_empty());
        assert!(!course.colliders.is_empty());
        course.validate().unwrap();
        assert!(!crate::vehicle::collision::in_contact(
            &crate::vehicle::VEHICLE_FOOTPRINT,
            &course.start,
            &course.collision_rects()
        ));
    }

    /// 일방통행과 경로 연결 없이 입력 순서에 상관없이 같은 영역을 생성한다.
    #[test]
    fn area_is_deterministic_and_does_not_require_route_connections() {
        let mut model = fixture();
        let id = model.blocks[0].id.clone();
        let origin = model.blocks[0].nodes[0].pos;
        let a = build_road_area(&model, &id, 500., origin).unwrap();
        model.connections.clear();
        for block in &mut model.blocks {
            block.directions.clear();
        }
        model.blocks.reverse();
        assert_eq!(a, build_road_area(&model, &id, 500., origin).unwrap());
    }

    /// 잘못된 영역과 존재하지 않는 도로는 명확하게 거부한다.
    #[test]
    fn rejects_invalid_selection_and_extent() {
        let model = fixture();
        let b = &model.blocks[0];
        for span in [f32::NAN, 0., 10001.] {
            assert!(build_road_area(&model, &b.id, span, b.nodes[0].pos).is_err());
        }
        assert!(build_road_area(&model, &BlockId("missing".into()), 500., b.nodes[0].pos).is_err());
    }

    /// 저장·공통 로더 재생이 동일하며 원본 변경·덮어쓰기·규칙 변경을 거부한다.
    #[test]
    fn saved_area_replays_and_rejects_modified_inputs() {
        let dir = std::env::temp_dir().join(format!(
            "road-area-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/block-real/expo-internal.osm.json");
        std::fs::copy(fixture, dir.join("roads.json")).unwrap();
        let places = super::super::places::snapshot_places_path(&dir.join("roads.json"));
        std::fs::write(&places, "{\"elements\":[]}").unwrap();
        let model = fixture_model_for_save(&dir);
        let anchor = &model.blocks[0];
        let (file, course) = prepare_road_area(
            &dir,
            "roads.json",
            &RoadGraphConfig::default(),
            &anchor.id,
            500.,
            anchor.nodes[0].pos,
        )
        .unwrap();
        let target = dir.join("area.toml");
        save_road_area(&target, &file).unwrap();
        assert_eq!(course, crate::world::map::load_map_course(&target).unwrap());
        assert!(save_road_area(&target, &file).is_err());
        let mut changed = file.clone();
        changed.network_version = "unknown".into();
        assert!(replay_road_area(&changed, &dir).is_err());
        std::fs::write(&places, "{\"elements\":[]} ").unwrap();
        assert!(
            replay_road_area(&file, &dir)
                .unwrap_err()
                .contains("SHA-256")
        );
        std::fs::write(&places, "{\"elements\":[]}").unwrap();
        std::fs::write(dir.join("roads.json"), "{}").unwrap();
        assert!(
            replay_road_area(&file, &dir)
                .unwrap_err()
                .contains("SHA-256")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 임시 저장 시험의 원본 도로를 같은 설정으로 구성한다.
    fn fixture_model_for_save(dir: &Path) -> RoadBlockModel {
        let source = load_road_snapshot(&dir.join("roads.json")).unwrap();
        build_road_blocks(
            &source.ways,
            &source.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap()
    }

    /// 사각형을 가로지르는 도로는 양 끝이 밖이어도 포함하고 먼 도로는 제외한다.
    #[test]
    fn area_includes_crossing_roads_but_excludes_distant_roads() {
        let mut model = fixture();
        let anchor = model.blocks[0].clone();
        let origin = anchor.nodes[0].pos;
        let mut distant = anchor.clone();
        distant.id = BlockId("distant".into());
        for node in &mut distant.nodes {
            node.pos.lat += 0.05;
            node.id += 100000;
        }
        model.blocks.push(distant);
        let mut crossing = anchor.clone();
        crossing.id = BlockId("crossing".into());
        let line: Vec<_> = anchor
            .nodes
            .iter()
            .map(|n| super::super::map::project(origin, n.pos))
            .collect();
        let center = super::super::road_geom::point_at_distance(
            &line,
            super::super::road_geom::polyline_length(&line) / 2.,
        )
        .unwrap()
        .0;
        crossing.nodes = vec![-200., 200.]
            .into_iter()
            .enumerate()
            .map(|(i, x)| crate::world::osm_graph::OsmNode {
                id: 900000 + i as u64,
                pos: LatLon {
                    lat: origin.lat - (center.y as f64 / 6371008.8).to_degrees(),
                    lon: origin.lon
                        + ((center.x as f64 + x) / (6371008.8 * origin.lat.to_radians().cos()))
                            .to_degrees(),
                },
            })
            .collect();
        model.blocks.push(crossing);
        let selected = select_road_area(&model, &anchor.id, 100., origin).unwrap();
        assert!(!selected.blocks.iter().any(|b| b.id.0 == "distant"));
        assert!(selected.blocks.iter().any(|b| b.id == anchor.id));
        assert!(selected.blocks.iter().any(|b| b.id.0 == "crossing"));
    }

    /// 다수 블록이 포함된 실제 엑스포·강남 영역의 형상을 경로 탐색 없이 생성한다.
    #[test]
    fn real_region_areas_generate_without_a_route() {
        for name in ["expo-region", "gangnam-region"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../testdata/junction-regressions/{name}.osm.json"
            ));
            let snapshot = load_road_snapshot(&path).unwrap();
            let model = build_road_blocks(
                &snapshot.ways,
                &snapshot.restrictions,
                &RoadGraphConfig::default(),
            )
            .unwrap();
            let anchor = model
                .blocks
                .iter()
                .filter(|b| b.attributes.width_m >= 4.)
                .max_by(|a, b| a.length_m.total_cmp(&b.length_m))
                .unwrap();
            for span in [500., 1000., 3000., 10000.] {
                let course = build_road_area(&model, &anchor.id, span, anchor.nodes[0].pos)
                    .unwrap_or_else(|e| panic!("{name} {span}: {e}"));
                assert!(course.goal.is_none());
            }
        }
    }
}
