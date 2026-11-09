//! 지역 도로 스냅샷 읽기와 경계 연결 정보(Phase 4-5G).
//!
//! 스냅샷 본문(`graph.osm.json`)은 두 가지다.
//!
//! - **Overpass 응답**(4-4, `meta.toml` 기존 형식 = v1): 연결 정보가 없다. 지금 동작을 유지하되
//!   "연결 검증 정보 없음"으로 본다.
//! - **Geofabrik 추출**(`generator`가 [`EXTRACT_GENERATOR_PREFIX`]로 시작): 같은 세대 폴더의 `meta.toml`
//!   ([`META_FORMAT_V2`])과 `connectivity.json`([`CONNECTIVITY_FORMAT`])이 있어야 하고, 본문·연결 정보 해시와
//!   규칙 버전이 맞아야 한다. 하나라도 맞지 않으면 거부한다.
//!
//! 연결 정보는 추출 때 스냅샷의 **모든 노드**를 전국 자료와 이웃 (way ID, 이웃 노드 ID) 집합으로 대조한 결과다.
//! 목록에 있는 노드는 영역 밖 도로가 붙어 연결이 불완전하다. 경로 계획과 저장 경로 재생은 모두
//! [`check_route_connectivity`]를 거쳐, 코스 형상에 쓰는 노드([`route::shape_nodes`])가 불완전하면 거부한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::map_fetch::{self, Bbox, SnapshotMeta};
use super::osm_graph::{EXTRACT_GENERATOR_PREFIX, OsmGraphWay, parse_graph_body};
use super::route;

/// Geofabrik 세대 `meta.toml`의 형식 이름.
pub const META_FORMAT_V2: &str = "snapshot-meta-v2";
/// 연결 정보 파일의 형식 이름.
pub const CONNECTIVITY_FORMAT: &str = "connectivity-v1";
/// 연결 정보 파일 이름.
pub const CONNECTIVITY_FILE: &str = "connectivity.json";
/// 이 프로그램이 아는 추출 규칙 버전(제안서 §3.1·§3.2). 규칙이 바뀌면 올린다.
pub const EXTRACT_RULE_VERSION: u32 = 1;

/// 연결이 전국 자료와 다른 노드 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncompleteNode {
    /// OSM 노드 ID.
    pub node: u64,
    /// 전국 자료(허용 도로 기준)의 (way, 이웃) 연결 수.
    pub national_degree: u32,
    /// 지역 스냅샷의 (way, 이웃) 연결 수.
    pub local_degree: u32,
    /// 지역 스냅샷에 없는 연결 `[way ID, 이웃 노드 ID]`. 오름차순.
    pub missing: Vec<[u64; 2]>,
}

/// `connectivity.json` 내용.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connectivity {
    /// 형식 이름([`CONNECTIVITY_FORMAT`]).
    pub format: String,
    /// 추출 규칙 버전.
    pub rule_version: u32,
    /// 대조한 본문(`graph.osm.json`)의 SHA-256(소문자 16진).
    pub graph_sha256: String,
    /// 대조에 쓴 허용 도로 종류(오름차순, 중복 없음).
    pub allowed_highways: Vec<String>,
    /// 대조한 노드 수(= 스냅샷의 서로 다른 노드 수).
    pub checked_nodes: u64,
    /// 연결이 불완전한 노드. 노드 ID 오름차순.
    pub incomplete: Vec<IncompleteNode>,
}

impl Connectivity {
    /// 정해진 직렬화(공백 없는 한 줄 JSON + 줄바꿈)로 바꾼다. 같은 내용이면 같은 바이트다.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string(self).expect("connectivity serializes");
        text.push('\n');
        text
    }

    /// 불완전 노드 ID 집합.
    pub fn incomplete_ids(&self) -> BTreeSet<u64> {
        self.incomplete.iter().map(|node| node.node).collect()
    }
}

/// Geofabrik 원본 기록(`meta.toml`의 `[geofabrik]` 절).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeofabrikSource {
    /// 원본 PBF 파일 이름(로컬 절대 경로는 쓰지 않는다).
    pub pbf_file_name: String,
    /// 받은 주소. 사용자가 준 로컬 파일이면 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbf_url: Option<String>,
    /// 원본 PBF의 SHA-256. 버전 식별용이며 정상 다운로드를 증명하지 않는다.
    pub pbf_sha256: String,
    /// 원본 PBF 크기(바이트).
    pub pbf_bytes: u64,
    /// 제공된 MD5와 대조했는지: `verified` | `unverified`.
    pub integrity: String,
    /// PBF 머리부의 복제 시각(UTC RFC 3339). 없으면 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replication_timestamp: Option<String>,
    /// PBF 머리부의 복제 순번. 없으면 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replication_sequence: Option<i64>,
    /// 자료 시각의 출처: `header`(머리부) | `unknown`.
    pub data_time: String,
    /// 추출 규칙 버전.
    pub extract_rule_version: u32,
    /// 사용자 영역에 더한 여유 폭(m).
    pub margin_m: f64,
    /// 추출에 쓴 허용 도로 종류(오름차순).
    pub allowed_highways: Vec<String>,
}

/// 연결 정보 파일 기록(`meta.toml`의 `[connectivity]` 절).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityRecord {
    /// 파일 이름([`CONNECTIVITY_FILE`]).
    pub file: String,
    /// 형식 이름.
    pub format: String,
    /// 파일의 SHA-256.
    pub sha256: String,
    /// 대조한 노드 수.
    pub checked_nodes: u64,
    /// 불완전 노드 수.
    pub incomplete_nodes: u64,
}

/// Geofabrik 세대의 `meta.toml`([`META_FORMAT_V2`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotMetaV2 {
    /// 형식 이름([`META_FORMAT_V2`]).
    pub meta_format: String,
    /// 출처: `geofabrik`.
    pub source: String,
    /// 사용자 영역(여유 폭을 더하기 전).
    pub bbox: Bbox,
    /// 만든 시각(UTC RFC 3339). 같은 원본 비교에서 제외하는 필드다.
    pub created_at: String,
    /// 본문 파일 이름.
    pub graph_file: String,
    /// 본문 SHA-256.
    pub graph_sha256: String,
    /// 본문 크기(바이트).
    pub graph_bytes: u64,
    /// 원본 기록.
    pub geofabrik: GeofabrikSource,
    /// 연결 정보 파일 기록.
    pub connectivity: ConnectivityRecord,
}

/// 세대 폴더의 `meta.toml`(형식별).
#[derive(Debug, Clone, PartialEq)]
pub enum GenerationMeta {
    /// Overpass 세대(기존 형식, `meta_format` 없음).
    Overpass(SnapshotMeta),
    /// Geofabrik 세대([`META_FORMAT_V2`]).
    Geofabrik(Box<SnapshotMetaV2>),
}

/// 읽은 도로 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadSnapshot {
    /// 노드 ID를 보존한 way.
    pub ways: Vec<OsmGraphWay>,
    /// 경계 연결 정보. Overpass 스냅샷이면 `None`(연결 검증 정보 없음).
    pub connectivity: Option<Connectivity>,
}

/// 바이트열의 SHA-256을 소문자 16진 문자열로 만든다.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 허용 도로 목록을 정렬·중복 제거한다(연결 정보 결속 비교 기준).
pub fn normalized_highways(allowed: &[String]) -> Vec<String> {
    let set: BTreeSet<&String> = allowed.iter().collect();
    set.into_iter().cloned().collect()
}

/// 파일을 읽는다(오류 문구에 경로를 넣는다).
fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// 스냅샷 본문 파일을 읽는다. Geofabrik 추출 본문이면 같은 폴더의 메타·연결 정보를 함께 검사해 읽는다.
pub fn load_road_snapshot(graph_path: &Path) -> Result<RoadSnapshot, String> {
    let body = read(graph_path)?;
    let text =
        std::str::from_utf8(&body).map_err(|_| format!("{} is not UTF-8", graph_path.display()))?;
    let (generator, ways) = parse_graph_body(text)?;
    if !generator
        .as_deref()
        .is_some_and(|g| g.starts_with(EXTRACT_GENERATOR_PREFIX))
    {
        return Ok(RoadSnapshot {
            ways,
            connectivity: None,
        });
    }
    let dir = graph_path.parent().unwrap_or(Path::new("."));
    let meta = match read_generation_meta(dir)? {
        GenerationMeta::Geofabrik(meta) => meta,
        GenerationMeta::Overpass(_) => {
            return Err(format!(
                "{} is a Geofabrik extract but {} is an Overpass record",
                graph_path.display(),
                dir.join(map_fetch::META_FILE).display()
            ));
        }
    };
    let file_name = graph_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if meta.graph_file != file_name {
        return Err(format!(
            "meta.toml describes `{}`, not `{file_name}`",
            meta.graph_file
        ));
    }
    let connectivity = verify_extract(&meta, &body, &ways, dir)?;
    Ok(RoadSnapshot {
        ways,
        connectivity: Some(connectivity),
    })
}

/// Geofabrik 세대의 메타·본문·연결 정보가 서로 맞는지 검사하고 연결 정보를 돌려준다.
fn verify_extract(
    meta: &SnapshotMetaV2,
    body: &[u8],
    ways: &[OsmGraphWay],
    dir: &Path,
) -> Result<Connectivity, String> {
    if meta.meta_format != META_FORMAT_V2 || meta.source != "geofabrik" {
        return Err(format!(
            "unexpected snapshot record `{}` / source `{}`",
            meta.meta_format, meta.source
        ));
    }
    if meta.geofabrik.extract_rule_version != EXTRACT_RULE_VERSION {
        return Err(format!(
            "extract rule version {} is not known (expected {EXTRACT_RULE_VERSION})",
            meta.geofabrik.extract_rule_version
        ));
    }
    let graph_sha = sha256_hex(body);
    if body.len() as u64 != meta.graph_bytes || graph_sha != meta.graph_sha256 {
        return Err("graph body does not match meta.toml (size or SHA-256)".to_owned());
    }
    if meta.connectivity.file != CONNECTIVITY_FILE
        || meta.connectivity.format != CONNECTIVITY_FORMAT
    {
        return Err(format!(
            "unexpected connectivity record `{}` / `{}`",
            meta.connectivity.file, meta.connectivity.format
        ));
    }
    let bytes = read(&dir.join(CONNECTIVITY_FILE))?;
    if sha256_hex(&bytes) != meta.connectivity.sha256 {
        return Err("connectivity.json does not match meta.toml (SHA-256)".to_owned());
    }
    let connectivity: Connectivity = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid connectivity.json: {error}"))?;
    if connectivity.format != CONNECTIVITY_FORMAT {
        return Err(format!(
            "unknown connectivity format `{}`",
            connectivity.format
        ));
    }
    if connectivity.rule_version != EXTRACT_RULE_VERSION {
        return Err(format!(
            "connectivity rule version {} is not known (expected {EXTRACT_RULE_VERSION})",
            connectivity.rule_version
        ));
    }
    if connectivity.graph_sha256 != graph_sha {
        return Err("connectivity.json was made for a different graph body".to_owned());
    }
    if connectivity.allowed_highways != normalized_highways(&connectivity.allowed_highways)
        || connectivity.allowed_highways != meta.geofabrik.allowed_highways
    {
        return Err("connectivity.json allowed_highways does not match meta.toml".to_owned());
    }
    let nodes: BTreeSet<u64> = ways
        .iter()
        .flat_map(|way| way.nodes.iter().map(|node| node.id))
        .collect();
    if connectivity.checked_nodes != nodes.len() as u64
        || connectivity.checked_nodes != meta.connectivity.checked_nodes
        || connectivity.incomplete.len() as u64 != meta.connectivity.incomplete_nodes
    {
        return Err("connectivity.json node counts do not match the snapshot".to_owned());
    }
    let mut previous = None;
    for entry in &connectivity.incomplete {
        if previous.is_some_and(|id| id >= entry.node) || !nodes.contains(&entry.node) {
            return Err(format!(
                "connectivity.json entry for node {} is out of order or not in the snapshot",
                entry.node
            ));
        }
        previous = Some(entry.node);
    }
    Ok(connectivity)
}

/// 세대 폴더의 `meta.toml`을 형식에 맞게 읽는다. `meta_format`이 없으면 기존 Overpass 형식이다.
///
/// Overpass 세대는 기존 검사([`map_fetch::load_generation`])를, Geofabrik 세대는 본문·연결 정보 검사를 함께 한다.
pub fn read_generation_meta(dir: &Path) -> Result<GenerationMeta, String> {
    /// `meta_format` 필드만 먼저 읽는 구조.
    #[derive(Deserialize)]
    struct Probe {
        meta_format: Option<String>,
    }
    let path = dir.join(map_fetch::META_FILE);
    let text =
        String::from_utf8(read(&path)?).map_err(|_| format!("{} is not UTF-8", path.display()))?;
    let probe: Probe =
        toml::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))?;
    match probe.meta_format.as_deref() {
        None => {
            let meta: SnapshotMeta = toml::from_str(&text)
                .map_err(|error| format!("invalid {}: {error}", path.display()))?;
            map_fetch::load_generation(dir, &meta.bbox)?;
            Ok(GenerationMeta::Overpass(meta))
        }
        Some(META_FORMAT_V2) => {
            let meta: SnapshotMetaV2 = toml::from_str(&text)
                .map_err(|error| format!("invalid {}: {error}", path.display()))?;
            Ok(GenerationMeta::Geofabrik(Box::new(meta)))
        }
        Some(other) => Err(format!("unknown snapshot record format `{other}`")),
    }
}

/// Geofabrik 세대 폴더 전체(메타·본문·연결 정보)를 읽고 검사한다.
pub fn load_extract_generation(dir: &Path) -> Result<(SnapshotMetaV2, RoadSnapshot), String> {
    let GenerationMeta::Geofabrik(meta) = read_generation_meta(dir)? else {
        return Err(format!("{} is not a Geofabrik generation", dir.display()));
    };
    let snapshot = load_road_snapshot(&dir.join(&meta.graph_file))?;
    Ok((*meta, snapshot))
}

/// 경로가 경계 연결 정보와 맞는지 검사한다. 경로 계획과 저장 경로 재생이 모두 이 검사를 거친다.
///
/// 1. 지금 쓰는 허용 도로 목록이 연결 정보를 만들 때의 목록과 **같아야** 한다(부분집합도 허용하지 않음).
/// 2. 코스 형상에 쓰는 노드([`route::shape_nodes`]: 경로 노드, 경로 위 교차로의 팔·곁가지 노드) 중
///    불완전 노드가 있으면 거부한다. 오류에 노드 ID와 빠진 way ID를 적는다.
pub fn check_route_connectivity(
    connectivity: &Connectivity,
    ways: &[OsmGraphWay],
    allowed_highways: &[String],
    route_nodes: &[u64],
) -> Result<(), String> {
    let allowed = normalized_highways(allowed_highways);
    if allowed != connectivity.allowed_highways {
        return Err(format!(
            "allowed highways {allowed:?} differ from the extract's {:?}; extract again with these settings",
            connectivity.allowed_highways
        ));
    }
    let set: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    let used = route::shape_nodes(ways, &set, route_nodes)?;
    let incomplete: BTreeMap<u64, &IncompleteNode> = connectivity
        .incomplete
        .iter()
        .map(|entry| (entry.node, entry))
        .collect();
    if let Some(entry) = used.iter().find_map(|id| incomplete.get(id)) {
        let ways: BTreeSet<u64> = entry.missing.iter().map(|[way, _]| *way).collect();
        return Err(format!(
            "node {} near the route connects to roads outside the extract (missing way(s) {ways:?}); extract a larger area",
            entry.node
        ));
    }
    Ok(())
}

/// Geofabrik 세대 폴더 경로에서 본문 경로를 만든다.
pub fn graph_path(dir: &Path) -> PathBuf {
    dir.join(map_fetch::SNAPSHOT_FILE)
}
