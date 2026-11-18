//! 고정 스냅샷과 확정 블록 경로의 저장·오프라인 재생. 기존 route-v1은 별도 로더로 유지한다.
//!
//! TOML은 자료를 내장하지 않는다. 상대 경로가 가리키는 원본 세대 폴더도 함께 보존해야 한다.
//! 재생은 저장 통과 순서를 사용하며 새 최단 경로를 찾지 않는다. 결과·자료·규칙 판본 불일치는 오류다.
//! 건물 파일은 저장에 명시한 것만 읽고, 저장 후 추가된 동반 자료를 자동으로 끼워 넣지 않는다.
//! v1은 고정 OSM 스냅샷 입력용이다. 지역 보정의 별도 참조와 Lanelet2 입력은 기존 전용 형식을 유지한다.
//! 신규 UI/저장 버튼 연결은 후속 단계다. hard-link를 지원하지 않는 파일 시스템에서는 저장이 오류로 끝난다.
use super::{
    block_route::BlockRouteCourse, map::LatLon, road_blocks::BlockTraversal,
    road_network::NetworkSettings, route_plan::RoadGraphConfig,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};
/// 블록 경로 파일 판본.
pub const BLOCK_ROUTE_FORMAT: &str = "block-route-v1";
/// 상대 경로와 원문 바이트 해시로 고정한 입력 파일.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedFile {
    pub path: String,
    pub sha256: String,
}
/// 블록 분할·폭 산정에 사용하는 설정만 저장한다. 옛 점 스냅 옵션은 새 경로에 사용하지 않는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockBuildSettings {
    pub allowed_highways: Vec<String>,
    pub lane_width_m: f32,
    pub default_width_m: BTreeMap<String, f32>,
}
/// 원본과 생성 정책·실제 통과 순서·결과 해시를 고정한 저장 파일.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRouteFile {
    pub format: String,
    pub name: String,
    pub block_model_version: String,
    pub network_version: String,
    pub guide_version: String,
    pub origin: [f64; 2],
    pub max_route_length_m: f64,
    pub blocks: BlockBuildSettings,
    pub network: NetworkSettings,
    pub snapshot: LockedFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LockedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connectivity: Option<LockedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub places: Option<LockedFile>,
    pub selected: Vec<BlockTraversal>,
    pub traversals: Vec<BlockTraversal>,
    pub length_m: f64,
    pub result_sha256: String,
}
/// 새 저장 파일을 만들 때 필요한 명시 입력. 실행 폴더나 전역 설정을 추측하지 않는다.
pub struct BlockRouteRequest<'a> {
    pub name: &'a str,
    pub snapshot: &'a str,
    pub origin: LatLon,
    pub graph: &'a RoadGraphConfig,
    pub network: NetworkSettings,
    pub selected: &'a [BlockTraversal],
}
use super::{
    block_route::{self, BlockRoutePlan},
    building, places,
    road_blocks::{self, BLOCK_MODEL_VERSION},
    road_network::NETWORK_GEOMETRY_VERSION,
    road_snapshot::{self, RoadSnapshot},
};

/// 자료를 읽어 새 경로를 탐색하고 재생에 필요한 참조와 결과를 함께 준비한다.
pub fn prepare_block_route_file(
    base_dir: &Path,
    request: &BlockRouteRequest<'_>,
) -> Result<(BlockRouteFile, BlockRouteCourse), String> {
    request.graph.validate()?;
    relative_path(request.snapshot)?;
    let path = request.snapshot.replace('\\', "/");
    let snapshot_path = base_dir.join(&path);
    let locked_snapshot = lock_file(base_dir, &path)?;
    let snapshot = road_snapshot::load_road_snapshot(&snapshot_path)?;
    let model =
        road_blocks::build_road_blocks(&snapshot.ways, &snapshot.restrictions, request.graph)?;
    let mut result = block_route::build_block_route_course(
        request.name,
        &model,
        request.origin,
        request.network,
        request.selected,
        request.graph.max_route_length_m,
    )
    .map_err(|e| format!("block route: {e:?}"))?;
    check_connectivity(&snapshot, request.graph, &model, &result.plan)?;
    let dependency = |name: &str| {
        lock_file(
            base_dir,
            Path::new(&path)
                .with_file_name(name)
                .to_string_lossy()
                .as_ref(),
        )
    };
    let extracted = snapshot.connectivity.is_some();
    let places_path = places::snapshot_places_path(Path::new(&path));
    let places = if base_dir
        .join(&places_path)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        Some(lock_file(base_dir, &places_path.to_string_lossy())?)
    } else {
        None
    };
    let mut file = BlockRouteFile {
        format: BLOCK_ROUTE_FORMAT.into(),
        name: request.name.into(),
        block_model_version: BLOCK_MODEL_VERSION.into(),
        network_version: NETWORK_GEOMETRY_VERSION.into(),
        guide_version: block_route::BLOCK_GUIDE_VERSION.into(),
        origin: [request.origin.lat, request.origin.lon],
        max_route_length_m: request.graph.max_route_length_m,
        blocks: BlockBuildSettings {
            allowed_highways: request.graph.allowed_highways.clone(),
            lane_width_m: request.graph.lane_width_m,
            default_width_m: request.graph.default_width_m.clone(),
        },
        network: request.network,
        snapshot: locked_snapshot,
        metadata: if extracted {
            Some(dependency("meta.toml")?)
        } else {
            None
        },
        connectivity: if extracted {
            Some(dependency(road_snapshot::CONNECTIVITY_FILE)?)
        } else {
            None
        },
        places,
        selected: result.plan.selected.clone(),
        traversals: result.plan.traversals.clone(),
        length_m: result.plan.length_m,
        result_sha256: "0".repeat(64),
    };
    attach_buildings(&file, base_dir, &mut result)?;
    file.result_sha256 = result_fingerprint(&result);
    file.validate()?;
    // 준비 도중 원본이나 결속 자료가 바뀌었어도 잘못된 판본을 저장 대상으로 내보내지 않는다.
    let verified = replay_block_route_file(&file, base_dir)?;
    Ok((file, verified))
}
/// 저장 파일의 문법과 지원 판본·설정·참조 형식을 검사한다.
pub fn parse_block_route_file(text: &str) -> Result<BlockRouteFile, String> {
    let file: BlockRouteFile =
        toml::from_str(text).map_err(|e| format!("invalid block route file: {e}"))?;
    file.validate()?;
    Ok(file)
}
/// 저장 파일을 결정적인 TOML 바이트 순서로 직렬화한다.
pub fn block_route_file_toml(file: &BlockRouteFile) -> Result<String, String> {
    file.validate()?;
    toml::to_string(file).map_err(|e| format!("cannot serialize block route: {e}"))
}
/// 저장된 실제 통과 순서로만 재생하며 새 최단 경로를 찾지 않는다.
pub fn replay_block_route_file(
    file: &BlockRouteFile,
    base_dir: &Path,
) -> Result<BlockRouteCourse, String> {
    file.validate()?;
    verify_files(file, base_dir)?;
    let snapshot = road_snapshot::load_road_snapshot(&base_dir.join(&file.snapshot.path))?;
    if snapshot.connectivity.is_some() != file.connectivity.is_some() {
        return Err("snapshot metadata/connectivity references do not match source format".into());
    }
    let config = file.config();
    let model = road_blocks::build_road_blocks(&snapshot.ways, &snapshot.restrictions, &config)?;
    let plan = BlockRoutePlan {
        selected: file.selected.clone(),
        traversals: file.traversals.clone(),
        length_m: file.length_m,
    };
    check_connectivity(&snapshot, &config, &model, &plan)?;
    let mut result = block_route::build_resolved_block_route_course(
        &file.name,
        &model,
        LatLon {
            lat: file.origin[0],
            lon: file.origin[1],
        },
        file.network,
        plan,
        file.max_route_length_m,
    )
    .map_err(|e| format!("cannot replay block route: {e:?}"))?;
    attach_buildings(file, base_dir, &mut result)?;
    if result_fingerprint(&result) != file.result_sha256 {
        return Err(
            "block route result SHA-256 mismatch; regenerate explicitly with these inputs".into(),
        );
    }
    verify_files(file, base_dir)?;
    Ok(result)
}
/// 파일 기준 상대 경로를 해석해 오프라인으로 코스를 재생한다.
pub fn load_block_route_file(path: &Path) -> Result<BlockRouteCourse, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    replay_block_route_file(
        &parse_block_route_file(&text)?,
        path.parent().unwrap_or(Path::new(".")),
    )
}
/// 재생 검증을 통과한 파일만 새 이름으로 원자적으로 게시한다. 기존 파일은 덮어쓰지 않는다.
/// 같은 폴더의 임시 파일을 쓴 뒤 hard-link를 만들어 대상 이름을 독점 확보한다.
pub fn save_block_route_file(path: &Path, file: &BlockRouteFile) -> Result<(), String> {
    replay_block_route_file(file, path.parent().unwrap_or(Path::new(".")))?;
    let text = block_route_file_toml(file)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_file_name(format!(
        ".block-route-{}-{}.partial",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("cannot create {}: {e}", temporary.display()))?;
    let written = output
        .write_all(text.as_bytes())
        .and_then(|()| output.sync_all());
    drop(output);
    let published = written.and_then(|()| std::fs::hard_link(&temporary, path));
    let _ = std::fs::remove_file(&temporary);
    published.map_err(|e| format!("cannot save new block route {}: {e}", path.display()))
}
impl BlockRouteFile {
    /// 새 블록 구성에 실제 사용하는 폭/종류 설정으로 기존 설정 객체를 만든다.
    fn config(&self) -> RoadGraphConfig {
        RoadGraphConfig {
            allowed_highways: self.blocks.allowed_highways.clone(),
            lane_width_m: self.blocks.lane_width_m,
            default_width_m: self.blocks.default_width_m.clone(),
            max_route_length_m: self.max_route_length_m,
            ..Default::default()
        }
    }
    /// 알려진 판본·방향·해시·상대 경로 및 유한한 설정만 받아들인다.
    fn validate(&self) -> Result<(), String> {
        if self.format != BLOCK_ROUTE_FORMAT
            || self.block_model_version != BLOCK_MODEL_VERSION
            || self.network_version != NETWORK_GEOMETRY_VERSION
            || self.guide_version != block_route::BLOCK_GUIDE_VERSION
        {
            return Err("unsupported block route format or generation version".into());
        }
        if self.name.trim().is_empty()
            || !(self.origin[0].is_finite()
                && self.origin[1].is_finite()
                && (-85.0..=85.0).contains(&self.origin[0])
                && (-180.0..=180.0).contains(&self.origin[1]))
        {
            return Err("invalid block route name or origin".into());
        }
        self.config().validate()?;
        self.network.validate()?;
        if !valid_sha(&self.result_sha256)
            || self.selected.is_empty()
            || self.traversals.is_empty()
            || !(self.length_m.is_finite()
                && self.length_m > 0.0
                && self.length_m <= self.max_route_length_m)
        {
            return Err("invalid block route result hash, length or selections".into());
        }
        for t in self.selected.iter().chain(&self.traversals) {
            if !t
                .block_id
                .0
                .strip_prefix(&format!("{BLOCK_MODEL_VERSION}:"))
                .is_some_and(valid_sha)
            {
                return Err("invalid stored block ID".into());
            }
        }
        for file in std::iter::once(&self.snapshot)
            .chain(self.metadata.iter())
            .chain(self.connectivity.iter())
            .chain(self.places.iter())
        {
            relative_path(&file.path)?;
            if !valid_sha(&file.sha256) {
                return Err("invalid input SHA-256".into());
            }
        }
        if self.metadata.is_some() != self.connectivity.is_some() {
            return Err("metadata and connectivity must be recorded together".into());
        }
        for (reference, expected) in [
            (
                &self.metadata,
                Path::new(&self.snapshot.path).with_file_name("meta.toml"),
            ),
            (
                &self.connectivity,
                Path::new(&self.snapshot.path).with_file_name(road_snapshot::CONNECTIVITY_FILE),
            ),
            (
                &self.places,
                places::snapshot_places_path(Path::new(&self.snapshot.path)),
            ),
        ] {
            if reference.as_ref().is_some_and(|r| {
                r.path.replace('\\', "/") != expected.to_string_lossy().replace('\\', "/")
            }) {
                return Err("input dependency is not paired with the selected snapshot".into());
            }
        }
        Ok(())
    }
}
/// SHA-256 표기는 정확히 소문자 16진수 64자리여야 한다.
fn valid_sha(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// 배포 위치와 CWD에 독립적인 상대 경로만 허용한다. 상위 폴더 참조는 명시된 경로로 허용한다.
fn relative_path(text: &str) -> Result<(), String> {
    if text.is_empty()
        || text.contains('\0')
        || Path::new(text)
            .components()
            .any(|p| matches!(p, Component::RootDir | Component::Prefix(_)))
    {
        Err("input file path must be relative to the saved route".into())
    } else {
        Ok(())
    }
}
/// 명시한 파일 바이트의 해시를 기록한다.
fn lock_file(base: &Path, path: &str) -> Result<LockedFile, String> {
    relative_path(path)?;
    let path = path.replace('\\', "/");
    let bytes = std::fs::read(base.join(&path)).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(LockedFile {
        path,
        sha256: road_snapshot::sha256_hex(&bytes),
    })
}
/// 저장 당시 기록한 자료만 확인하며 신규 장소 파일이나 전역 캐시는 탐색하지 않는다.
fn verify_files(file: &BlockRouteFile, base: &Path) -> Result<(), String> {
    for reference in std::iter::once(&file.snapshot)
        .chain(file.metadata.iter())
        .chain(file.connectivity.iter())
        .chain(file.places.iter())
    {
        if lock_file(base, &reference.path)?.sha256 != reference.sha256 {
            return Err(format!("input SHA-256 mismatch: {}", reference.path));
        }
    }
    Ok(())
}
/// 선택 경로와 인접 교차로 팔의 추출 경계 연결 정보를 기존 규칙으로 검사한다.
fn check_connectivity(
    snapshot: &RoadSnapshot,
    config: &RoadGraphConfig,
    model: &super::road_blocks::RoadBlockModel,
    plan: &BlockRoutePlan,
) -> Result<(), String> {
    if let Some(c) = &snapshot.connectivity {
        let ids: Vec<_> = plan
            .traversals
            .iter()
            .flat_map(|t| {
                model
                    .blocks
                    .iter()
                    .find(|b| b.id == t.block_id)
                    .into_iter()
                    .flat_map(|b| b.nodes.iter().map(|n| n.id))
            })
            .collect();
        road_snapshot::check_route_connectivity(c, &snapshot.ways, &config.allowed_highways, &ids)?;
    }
    Ok(())
}
/// 원본 스냅샷과 일대일로 대응하며 저장에 기록한 건물 파일만 코스에 붙인다.
fn attach_buildings(
    file: &BlockRouteFile,
    base: &Path,
    result: &mut BlockRouteCourse,
) -> Result<(), String> {
    if let Some(places) = &file.places {
        let lines: Vec<_> = result
            .network
            .blocks
            .iter()
            .map(|b| b.centerline.clone())
            .collect();
        let (buildings, estimated) = building::load_buildings_file(
            &base.join(&places.path),
            LatLon {
                lat: file.origin[0],
                lon: file.origin[1],
            },
            &lines,
            &result.course.surfaces,
            &result.course.strips,
            &result.course.areas,
        )?;
        result.course.buildings = buildings;
        if estimated {
            result
                .course
                .notices
                .push(building::ESTIMATED_HEIGHT_NOTICE.into());
        }
        result.course.validate().map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// 좌표를 IEEE-754 비트 정수로 기록해 소수점 출력 규칙에 지문이 의존하지 않게 한다.
fn point(p: glam::Vec2) -> [u32; 2] {
    p.to_array().map(f32::to_bits)
}
/// 시작/목표의 물리 상태까지 지문에 포함한다.
fn motion(m: crate::vehicle::Motion) -> [u32; 7] {
    [
        m.x_m,
        m.z_m,
        m.heading_rad,
        m.accel_mps2,
        m.steer_angle_rad,
        m.lateral_mps,
        m.slip_yaw_rate_rad_s,
    ]
    .map(f32::to_bits)
}
/// 회전된 충돌 상자의 위치와 크기를 고정 순서로 기록한다.
fn rect(r: crate::vehicle::GroundRect) -> [u32; 5] {
    [
        r.center.x,
        r.center.y,
        r.half_size.x,
        r.half_size.y,
        r.heading_rad,
    ]
    .map(f32::to_bits)
}
/// 도로 띠의 종류와 좌우 경계 순서를 지문용 값으로 바꾼다.
fn strip(s: &super::course::Strip) -> serde_json::Value {
    serde_json::json!([
        match s.kind {
            super::course::StripKind::Road => 0,
            super::course::StripKind::EdgeLine => 1,
        },
        s.left.iter().copied().map(point).collect::<Vec<_>>(),
        s.right.iter().copied().map(point).collect::<Vec<_>>()
    ])
}
/// 디버그 출력이나 파일 경로 없이 확정 경로·형상·안내선·차체 자세·건물을 정규 직렬화해 결속한다.
fn result_fingerprint(result: &BlockRouteCourse) -> String {
    use super::course::{ColliderKind, MarkingKind};
    let c = &result.course;
    let value = serde_json::json!({
        "fingerprint_version":1,
        "plan":[result.plan.selected,result.plan.traversals,result.plan.length_m.to_bits()],
        "guide":result.guide.iter().copied().map(point).collect::<Vec<_>>(),
        "network_blocks":result.network.blocks.iter().map(|b|serde_json::json!([b.block_id,b.centerline.iter().copied().map(point).collect::<Vec<_>>(),strip(&b.strip)])).collect::<Vec<_>>(),
        "network_joints":result.network.joints.iter().map(|j|serde_json::json!([j.node_id,j.openings.iter().map(|p|p.map(point)).collect::<Vec<_>>()])).collect::<Vec<_>>(),
        "name":c.name,"notices":c.notices,"start":motion(c.start),"goal":c.goal.map(motion),
        "surfaces":c.surfaces.iter().copied().map(rect).collect::<Vec<_>>(),
        "strips":c.strips.iter().map(strip).collect::<Vec<_>>(),
        "areas":c.areas.iter().map(|a|serde_json::json!([point(a.center),a.boundary.iter().copied().map(point).collect::<Vec<_>>()])).collect::<Vec<_>>(),
        "colliders":c.colliders.iter().map(|b|serde_json::json!([rect(b.rect),b.height_m.to_bits(),match b.kind{ColliderKind::Curb=>0,ColliderKind::LowCurb=>1,ColliderKind::Wall=>2,ColliderKind::Barrier=>3,ColliderKind::Obstacle=>4}])).collect::<Vec<_>>(),
        "markings":c.markings.iter().map(|m|serde_json::json!([rect(m.rect),match m.kind{MarkingKind::EdgeLine=>0,MarkingKind::StopLine=>1}])).collect::<Vec<_>>(),
        "buildings":c.buildings.iter().map(|b|serde_json::json!([b.id,b.name,b.kind,b.height_m.to_bits(),b.is_estimated,b.footprint.iter().copied().map(point).collect::<Vec<_>>()])).collect::<Vec<_>>()
    });
    road_snapshot::sha256_hex(
        &serde_json::to_vec(&value).expect("integer-only canonical result serialization"),
    )
}
#[cfg(test)]
#[path = "block_route_file_tests.rs"]
mod tests;
