//! OSM 누락 통로를 추가하는 `map-patch-v1` 파일 형식과 독립 검증(PLAN 4-11a 1단계).
//!
//! 이 모듈은 보정 파일을 읽고 출처·형상·ID를 검증할 뿐 OSM 그래프에는 적용하지 않는다. OSM 접속점 확인과
//! 경로 탐색 결합은 다음 단계에서 한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::map::LatLon;
use super::route_plan;

/// 배포할 지역 보정 파일의 기본 폴더.
pub const PATCH_DIR: &str = "data/map-patches";
/// 자동 시험 전용 지역 보정 파일의 기본 폴더.
pub const TEST_PATCH_DIR: &str = "testdata/map-patches";
/// 현재 지원하는 지역 보정 형식.
pub const FORMAT: &str = "map-patch-v1";
/// 보정 bbox 한 변의 상한(m). v1은 짧은 누락 통로만 다룬다.
pub const MAX_BBOX_SIDE_M: f64 = 2_000.0;
/// 접속점 허용 오차의 상한(m).
pub const MAX_ANCHOR_ERROR_M: f64 = 20.0;
/// 보정 도로 폭의 허용 범위(m).
pub const WIDTH_M: std::ops::RangeInclusive<f64> = 0.5..=30.0;
/// 합성 ID의 고정 상위 비트 `011`. TOML의 양수 `i64` 범위 안에서 OSM ID와 `2^62` 경로 점 영역을 피한다.
const SYNTHETIC_PREFIX: u64 = 3 << 61;
/// SHA-256에서 합성 ID에 보존할 하위 61비트.
const SYNTHETIC_HASH_MASK: u64 = (1 << 61) - 1;

/// 보정 파일을 어느 실행 환경에서 활성화할지 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchAudience {
    /// 배포 실행: 확인된 자료만 허용한다.
    Distribution,
    /// 개발 실행: 확인된 자료와 명시적인 임시 자료를 허용한다.
    Development,
    /// 자동 시험: 모든 상태를 허용한다.
    Test,
}

/// 보정 자료의 확인 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchStatus {
    /// 출처와 현장 조건을 확인해 배포할 수 있다.
    Verified,
    /// 개발 중 명시적으로만 켤 수 있다.
    Provisional,
    /// 합성 자동 시험에서만 쓸 수 있다.
    Synthetic,
}

impl PatchStatus {
    /// 파일과 화면 안내에 쓰는 안정적인 소문자 이름을 반환한다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Provisional => "provisional",
            Self::Synthetic => "synthetic",
        }
    }
}

/// 보정 범위(남·서·북·동).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatchBbox {
    /// 남쪽 위도.
    pub south: f64,
    /// 서쪽 경도.
    pub west: f64,
    /// 북쪽 위도.
    pub north: f64,
    /// 동쪽 경도.
    pub east: f64,
}

impl PatchBbox {
    /// TOML의 `[남, 서, 북, 동]` 배열을 이름 있는 범위로 바꾼다.
    fn from_array(values: [f64; 4]) -> Self {
        Self {
            south: values[0],
            west: values[1],
            north: values[2],
            east: values[3],
        }
    }

    /// 범위의 좌표·순서·대한민국 포함 여부·크기를 검사한다.
    fn validate(&self) -> Result<(), String> {
        let values = [self.south, self.west, self.north, self.east];
        if !values.iter().all(|value| value.is_finite()) {
            return Err(format!("bbox {values:?} has a non-finite value"));
        }
        if !(self.south < self.north && self.west < self.east) {
            return Err(format!(
                "bbox {values:?} must have south < north and west < east"
            ));
        }
        for (lat, lon) in [(self.south, self.west), (self.north, self.east)] {
            if !route_plan::in_korea(LatLon { lat, lon }) {
                return Err(format!("bbox {values:?} is outside the South Korea area"));
            }
        }
        let center_lat = (self.south + self.north) / 2.0;
        let width = longitude_m(self.east - self.west, center_lat);
        let height = latitude_m(self.north - self.south);
        if width > MAX_BBOX_SIDE_M || height > MAX_BBOX_SIDE_M {
            return Err(format!(
                "bbox is {width:.0} x {height:.0} m; limit is {MAX_BBOX_SIDE_M:.0} m per side"
            ));
        }
        Ok(())
    }

    /// 두 범위가 가장자리까지 포함해 겹치는지 판단한다.
    pub fn intersects(&self, other: &Self) -> bool {
        self.west <= other.east
            && other.west <= self.east
            && self.south <= other.north
            && other.south <= self.north
    }
}

/// OSM 도로에 붙일 명시적인 접속점.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchAnchor {
    /// 파일 안에서 고유한 접속점 이름.
    pub id: String,
    /// 기존 OSM 노드에 직접 붙일 때의 노드 ID.
    pub osm_node: Option<u64>,
    /// 기존 OSM way의 선분 중간에 붙일 때의 way ID.
    pub osm_way: Option<u64>,
    /// `osm_way` 안에서 접속 선분을 이루는 인접 노드 두 개.
    pub between_nodes: Option<[u64; 2]>,
    /// 접속 확인 위도.
    pub lat: f64,
    /// 접속 확인 경도.
    pub lon: f64,
    /// OSM 좌표와 허용할 최대 오차(m).
    pub max_error_m: f64,
}

/// 보정 파일 안에서만 쓰는 중심선 노드.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchNode {
    /// 파일 안에서 고유한 노드 이름.
    pub id: String,
    /// 노드 위도.
    pub lat: f64,
    /// 노드 경도.
    pub lon: f64,
}

/// OSM 그래프에 추가할 도로 조각.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchWay {
    /// 파일 안에서 고유한 way 이름.
    pub id: String,
    /// 내부 노드 ID 또는 `anchor:<ID>` 순서.
    pub nodes: Vec<String>,
    /// OSM과 같은 도로 종류 값.
    pub highway: String,
    /// 도로 진행 방향이 배열 순서 한 방향인지 여부.
    pub oneway: bool,
    /// 교육 코스에 사용할 폭(m).
    pub width_m: f64,
    /// 폭이 실측·추정·기본값 중 어디에서 왔는지 설명한다.
    pub width_source: String,
}

/// 역직렬화 직후의 `map-patch-v1` 본문.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchFile {
    format: String,
    id: String,
    name: String,
    revision: u64,
    bbox: [f64; 4],
    source_kind: String,
    source_description: String,
    verified_at: String,
    license: String,
    status: PatchStatus,
    geometry_accuracy: String,
    access_assumption: String,
    notice: String,
    anchors: Vec<PatchAnchor>,
    nodes: Vec<PatchNode>,
    ways: Vec<PatchWay>,
}

/// 검증을 마치고 SHA-256과 결정적 합성 ID를 붙인 지역 보정.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPatch {
    /// 형식 이름.
    pub format: String,
    /// 배포와 경로 파일에서 참조하는 안정적인 ID.
    pub id: String,
    /// 화면에 표시할 이름.
    pub name: String,
    /// 내용 변경 세대.
    pub revision: u64,
    /// 적용 후보를 고를 범위.
    pub bbox: PatchBbox,
    /// 근거 자료 종류.
    pub source_kind: String,
    /// 근거 자료 설명.
    pub source_description: String,
    /// 확인 날짜(`YYYY-MM-DD`).
    pub verified_at: String,
    /// 보정 파일 재배포 조건.
    pub license: String,
    /// 확인 상태.
    pub status: PatchStatus,
    /// 형상 정확도 설명.
    pub geometry_accuracy: String,
    /// 차량 접근 가능 여부와 남은 가정.
    pub access_assumption: String,
    /// 사용자에게 표시할 제한 안내.
    pub notice: String,
    /// OSM 접속점.
    pub anchors: Vec<PatchAnchor>,
    /// 보정 내부 노드.
    pub nodes: Vec<PatchNode>,
    /// 추가 도로.
    pub ways: Vec<PatchWay>,
    /// 원본 TOML 바이트의 SHA-256.
    pub sha256: String,
    /// `anchor/<ID>`·`node/<ID>`·`way/<ID>`에서 합성 `u64`로 가는 결정적 표.
    pub synthetic_ids: BTreeMap<String, u64>,
    /// 원본 파일 경로.
    pub path: PathBuf,
}

/// 경도 차이를 가운데 위도에서의 미터로 근사한다.
fn longitude_m(delta: f64, at_lat: f64) -> f64 {
    delta.abs() * 111_320.0 * at_lat.to_radians().cos()
}

/// 위도 차이를 미터로 근사한다.
fn latitude_m(delta: f64) -> f64 {
    delta.abs() * 111_320.0
}

/// 빈 문자열이 아닌지 확인하고 필드 이름을 포함한 오류를 만든다.
fn require_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

/// 안정적인 ID를 소문자 영숫자로 시작하는 ASCII 슬러그로 제한한다.
fn validate_id(value: &str, field: &str) -> Result<(), String> {
    require_text(value, field)?;
    let mut chars = value.chars();
    if !chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        || !chars.all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.')
        })
    {
        return Err(format!(
            "{field} `{value}` must be an ASCII slug starting with a lowercase letter or digit"
        ));
    }
    Ok(())
}

/// 위경도가 유한하고 대한민국 근사 범위 안인지 검사한다.
fn validate_position(lat: f64, lon: f64, field: &str) -> Result<(), String> {
    if !lat.is_finite() || !lon.is_finite() {
        return Err(format!("{field} position must be finite"));
    }
    if !route_plan::in_korea(LatLon { lat, lon }) {
        return Err(format!(
            "{field} position ({lat}, {lon}) is outside South Korea"
        ));
    }
    Ok(())
}

/// `YYYY-MM-DD`의 달력상 가능한 날짜인지 가볍게 검사한다.
fn validate_date(value: &str) -> Result<(), String> {
    let parts: Vec<&str> = value.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return Err("verified_at must be YYYY-MM-DD".to_owned());
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<u32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return Err("verified_at must be YYYY-MM-DD".to_owned());
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err("verified_at has an invalid month".to_owned()),
    };
    if year == 0 || day == 0 || day > days {
        return Err("verified_at has an invalid day".to_owned());
    }
    Ok(())
}

/// 보정 상태가 선택한 실행 환경에서 허용되는지 검사한다.
fn validate_audience(status: PatchStatus, audience: PatchAudience) -> Result<(), String> {
    let allowed = match audience {
        PatchAudience::Distribution => status == PatchStatus::Verified,
        PatchAudience::Development => status != PatchStatus::Synthetic,
        PatchAudience::Test => true,
    };
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "patch status {status:?} is not allowed for {audience:?}"
        ))
    }
}

/// 이름 공간과 로컬 ID를 SHA-256으로 고정된 양수 `i64` 합성 ID 영역으로 바꾼다.
fn synthetic_id(patch_id: &str, revision: u64, kind: &str, local_id: &str) -> u64 {
    let mut hasher = Sha256::new();
    for value in [FORMAT, patch_id, &revision.to_string(), kind, local_id] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    let bytes: [u8; 8] = hasher.finalize()[..8]
        .try_into()
        .expect("SHA-256 always has at least 8 bytes");
    (u64::from_be_bytes(bytes) & SYNTHETIC_HASH_MASK) | SYNTHETIC_PREFIX
}

/// way 중간 접속점, 내부 노드와 way의 합성 ID를 만들고 파일 안 충돌을 검사한다.
fn build_synthetic_ids(file: &PatchFile) -> Result<BTreeMap<String, u64>, String> {
    let mut by_name = BTreeMap::new();
    let mut used = BTreeMap::new();
    for (kind, local) in file
        .anchors
        .iter()
        .filter(|anchor| anchor.osm_way.is_some())
        .map(|anchor| ("anchor", anchor.id.as_str()))
        .chain(file.nodes.iter().map(|node| ("node", node.id.as_str())))
        .chain(file.ways.iter().map(|way| ("way", way.id.as_str())))
    {
        let key = format!("{kind}/{local}");
        let value = synthetic_id(&file.id, file.revision, kind, local);
        if let Some(previous) = used.insert(value, key.clone()) {
            return Err(format!(
                "synthetic ID {value} collides between {previous} and {key}"
            ));
        }
        by_name.insert(key, value);
    }
    Ok(by_name)
}

/// 하나의 역직렬화된 보정 파일에서 필드·참조·중복을 모두 검사한다.
fn validate_file(file: &PatchFile, audience: PatchAudience) -> Result<PatchBbox, String> {
    if file.format != FORMAT {
        return Err(format!(
            "unsupported format `{}`; expected `{FORMAT}`",
            file.format
        ));
    }
    validate_id(&file.id, "id")?;
    if file.revision == 0 {
        return Err("revision must be at least 1".to_owned());
    }
    for (field, value) in [
        ("name", file.name.as_str()),
        ("source_kind", file.source_kind.as_str()),
        ("source_description", file.source_description.as_str()),
        ("license", file.license.as_str()),
        ("geometry_accuracy", file.geometry_accuracy.as_str()),
        ("access_assumption", file.access_assumption.as_str()),
        ("notice", file.notice.as_str()),
    ] {
        require_text(value, field)?;
    }
    validate_date(&file.verified_at)?;
    validate_audience(file.status, audience)?;
    let bbox = PatchBbox::from_array(file.bbox);
    bbox.validate()?;
    if file.anchors.is_empty() {
        return Err("anchors must contain at least one item".to_owned());
    }
    if file.nodes.is_empty() {
        return Err("nodes must contain at least one item".to_owned());
    }
    if file.ways.is_empty() {
        return Err("ways must contain at least one item".to_owned());
    }

    let mut anchors = BTreeSet::new();
    for anchor in &file.anchors {
        validate_id(&anchor.id, "anchor.id")?;
        if !anchors.insert(anchor.id.as_str()) {
            return Err(format!("duplicate anchor ID `{}`", anchor.id));
        }
        let node_anchor =
            anchor.osm_node.is_some() && anchor.osm_way.is_none() && anchor.between_nodes.is_none();
        let way_anchor =
            anchor.osm_node.is_none() && anchor.osm_way.is_some() && anchor.between_nodes.is_some();
        if !node_anchor && !way_anchor {
            return Err(format!(
                "anchor `{}` must use exactly osm_node or osm_way with between_nodes",
                anchor.id
            ));
        }
        if anchor.between_nodes.is_some_and(|pair| pair[0] == pair[1]) {
            return Err(format!("anchor `{}` repeats between_nodes", anchor.id));
        }
        validate_position(anchor.lat, anchor.lon, &format!("anchor `{}`", anchor.id))?;
        if !(bbox.south..=bbox.north).contains(&anchor.lat)
            || !(bbox.west..=bbox.east).contains(&anchor.lon)
        {
            return Err(format!("anchor `{}` is outside patch bbox", anchor.id));
        }
        if !anchor.max_error_m.is_finite()
            || anchor.max_error_m <= 0.0
            || anchor.max_error_m > MAX_ANCHOR_ERROR_M
        {
            return Err(format!(
                "anchor `{}` max_error_m must be in (0, {MAX_ANCHOR_ERROR_M}]",
                anchor.id
            ));
        }
    }

    let mut nodes = BTreeSet::new();
    for node in &file.nodes {
        validate_id(&node.id, "node.id")?;
        if !nodes.insert(node.id.as_str()) {
            return Err(format!("duplicate node ID `{}`", node.id));
        }
        validate_position(node.lat, node.lon, &format!("node `{}`", node.id))?;
        if !(bbox.south..=bbox.north).contains(&node.lat)
            || !(bbox.west..=bbox.east).contains(&node.lon)
        {
            return Err(format!("node `{}` is outside patch bbox", node.id));
        }
    }

    let mut ways = BTreeSet::new();
    for way in &file.ways {
        validate_id(&way.id, "way.id")?;
        require_text(&way.highway, "way.highway")?;
        require_text(&way.width_source, "way.width_source")?;
        if !ways.insert(way.id.as_str()) {
            return Err(format!("duplicate way ID `{}`", way.id));
        }
        if way.nodes.len() < 2 {
            return Err(format!("way `{}` must have at least 2 nodes", way.id));
        }
        if !way.width_m.is_finite() || !WIDTH_M.contains(&way.width_m) {
            return Err(format!("way `{}` width_m must be in {:?}", way.id, WIDTH_M));
        }
        for reference in &way.nodes {
            let exists = reference.strip_prefix("anchor:").map_or_else(
                || nodes.contains(reference.as_str()),
                |id| anchors.contains(id),
            );
            if !exists {
                return Err(format!(
                    "way `{}` references unknown node `{reference}`",
                    way.id
                ));
            }
        }
        if way.nodes.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(format!("way `{}` repeats consecutive nodes", way.id));
        }
    }
    Ok(bbox)
}

/// TOML 바이트 하나를 읽고 완전히 검증된 지역 보정으로 바꾼다.
pub fn parse_patch(
    bytes: &[u8],
    path: impl Into<PathBuf>,
    audience: PatchAudience,
) -> Result<MapPatch, String> {
    let path = path.into();
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
    let file: PatchFile =
        toml::from_str(text).map_err(|error| format!("invalid {}: {error}", path.display()))?;
    let bbox = validate_file(&file, audience)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    let synthetic_ids = build_synthetic_ids(&file)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    let sha256 = super::road_snapshot::sha256_hex(bytes);
    Ok(MapPatch {
        format: file.format,
        id: file.id,
        name: file.name,
        revision: file.revision,
        bbox,
        source_kind: file.source_kind,
        source_description: file.source_description,
        verified_at: file.verified_at,
        license: file.license,
        status: file.status,
        geometry_accuracy: file.geometry_accuracy,
        access_assumption: file.access_assumption,
        notice: file.notice,
        anchors: file.anchors,
        nodes: file.nodes,
        ways: file.ways,
        sha256,
        synthetic_ids,
        path,
    })
}

/// 폴더 바로 아래의 `.toml` 보정 파일을 경로 순으로 읽고 파일 간 충돌을 검사한다.
pub fn load_patch_dir(dir: &Path, audience: PatchAudience) -> Result<Vec<MapPatch>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read patch directory {}: {error}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "cannot read patch directory entry in {}: {error}",
                dir.display()
            )
        })?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            paths.push(path);
        }
    }
    paths.sort();

    let mut patches = Vec::new();
    let mut patch_ids = BTreeMap::new();
    let mut synthetic_ids = BTreeMap::new();
    for path in paths {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let patch = parse_patch(&bytes, path, audience)?;
        if let Some(previous) = patch_ids.insert(patch.id.clone(), patch.path.clone()) {
            return Err(format!(
                "duplicate patch ID `{}` in {} and {}",
                patch.id,
                previous.display(),
                patch.path.display()
            ));
        }
        for (name, value) in &patch.synthetic_ids {
            let qualified = format!("{}:{name}", patch.id);
            if let Some(previous) = synthetic_ids.insert(*value, qualified.clone()) {
                return Err(format!(
                    "synthetic ID {value} collides between {previous} and {qualified}"
                ));
            }
        }
        patches.push(patch);
    }
    patches.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(patches)
}

/// 이미 검증한 보정 중 요청 범위와 겹치는 것만 ID 순서로 돌려준다.
pub fn patches_for_bbox<'a>(patches: &'a [MapPatch], bbox: &PatchBbox) -> Vec<&'a MapPatch> {
    let mut selected: Vec<&MapPatch> = patches
        .iter()
        .filter(|patch| patch.bbox.intersects(bbox))
        .collect();
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 모든 시험에서 필요한 최소 정상 보정 TOML을 만든다.
    fn valid(status: &str, id: &str) -> String {
        format!(
            r#"format = "map-patch-v1"
id = "{id}"
name = "시험 통로"
revision = 1
bbox = [34.7500, 127.7480, 34.7520, 127.7500]
source_kind = "field-survey"
source_description = "현장 확인"
verified_at = "2026-11-13"
license = "재배포 허용"
status = "{status}"
geometry_accuracy = "approximate"
access_assumption = "교육용 차량 통행 가능 여부 확인"
notice = "실측 정밀지도가 아님"

[[anchors]]
id = "north"
osm_node = 100
lat = 34.7518
lon = 127.7482
max_error_m = 2.0

[[anchors]]
id = "south"
osm_way = 200
between_nodes = [201, 202]
lat = 34.7502
lon = 127.7498
max_error_m = 2.0

[[nodes]]
id = "p1"
lat = 34.7512
lon = 127.7488

[[nodes]]
id = "p2"
lat = 34.7508
lon = 127.7492

[[ways]]
id = "passage"
nodes = ["anchor:north", "p1", "p2", "anchor:south"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "estimated"
"#
        )
    }

    /// 정상 파일의 공개 필드·해시·합성 ID를 읽는다.
    #[test]
    fn parses_valid_patch_and_builds_stable_ids() {
        let text = valid("verified", "alpha");
        let first =
            parse_patch(text.as_bytes(), "alpha.toml", PatchAudience::Distribution).unwrap();
        let second =
            parse_patch(text.as_bytes(), "copy.toml", PatchAudience::Distribution).unwrap();
        assert_eq!(first.id, "alpha");
        assert_eq!(first.sha256, second.sha256);
        assert_eq!(first.synthetic_ids, second.synthetic_ids);
        assert!(
            first
                .synthetic_ids
                .values()
                .all(|id| id & !SYNTHETIC_HASH_MASK == SYNTHETIC_PREFIX)
        );
        assert_ne!(
            first.synthetic_ids["node/p1"],
            first.synthetic_ids["way/passage"]
        );
    }

    /// 상태별 실행 환경 경계를 지킨다.
    #[test]
    fn enforces_patch_audience() {
        assert!(
            parse_patch(
                valid("verified", "a").as_bytes(),
                "a",
                PatchAudience::Distribution
            )
            .is_ok()
        );
        assert!(
            parse_patch(
                valid("provisional", "a").as_bytes(),
                "a",
                PatchAudience::Distribution
            )
            .is_err()
        );
        assert!(
            parse_patch(
                valid("provisional", "a").as_bytes(),
                "a",
                PatchAudience::Development
            )
            .is_ok()
        );
        assert!(
            parse_patch(
                valid("synthetic", "a").as_bytes(),
                "a",
                PatchAudience::Development
            )
            .is_err()
        );
        assert!(parse_patch(valid("synthetic", "a").as_bytes(), "a", PatchAudience::Test).is_ok());
    }

    /// 알 수 없는 필드와 잘못된 날짜를 조용히 무시하지 않는다.
    #[test]
    fn rejects_unknown_fields_and_bad_date() {
        let unknown = valid("verified", "a").replace("revision = 1", "revision = 1\nextra = true");
        assert!(
            parse_patch(unknown.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("unknown field")
        );
        let date = valid("verified", "a").replace("2026-11-13", "2026-02-30");
        assert!(
            parse_patch(date.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("invalid day")
        );
    }

    /// 안정적인 ID와 revision 계약을 검사한다.
    #[test]
    fn rejects_unstable_ids_and_zero_revision() {
        let id = valid("verified", "bad id");
        assert!(
            parse_patch(id.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("ASCII slug")
        );
        let revision = valid("verified", "a").replace("revision = 1", "revision = 0");
        assert!(
            parse_patch(revision.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("at least 1")
        );
    }

    /// 접속점은 노드 방식 또는 way 선분 방식 중 하나만 사용해야 한다.
    #[test]
    fn rejects_ambiguous_anchor() {
        let text = valid("verified", "a").replace("osm_node = 100", "osm_node = 100\nosm_way = 99");
        assert!(
            parse_patch(text.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("exactly osm_node")
        );
    }

    /// 존재하지 않는 노드 참조와 연속 중복 노드를 거부한다.
    #[test]
    fn rejects_bad_way_references() {
        let missing = valid("verified", "a").replace("\"p1\", \"p2\"", "\"missing\", \"p2\"");
        assert!(
            parse_patch(missing.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("unknown node")
        );
        let repeated = valid("verified", "a").replace("\"p1\", \"p2\"", "\"p1\", \"p1\"");
        assert!(
            parse_patch(repeated.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("repeats consecutive")
        );
    }

    /// 로컬 ID 중복과 필수 출처 누락을 거부한다.
    #[test]
    fn rejects_duplicate_ids_and_missing_source() {
        let duplicate = valid("verified", "a").replace("id = \"p2\"", "id = \"p1\"");
        assert!(
            parse_patch(duplicate.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("duplicate node ID")
        );
        let missing = valid("verified", "a").replace("license = \"재배포 허용\"\n", "");
        assert!(
            parse_patch(missing.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("missing field")
        );
    }

    /// 좌표·bbox·폭·접속 오차 상한을 검사한다.
    #[test]
    fn rejects_geometry_out_of_bounds() {
        let outside = valid("verified", "a").replace("lat = 34.7512", "lat = 35.0");
        assert!(
            parse_patch(outside.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("outside patch bbox")
        );
        let width = valid("verified", "a").replace("width_m = 4.0", "width_m = 0.0");
        assert!(
            parse_patch(width.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("width_m")
        );
        let error = valid("verified", "a").replace("max_error_m = 2.0", "max_error_m = 21.0");
        assert!(
            parse_patch(error.as_bytes(), "a", PatchAudience::Test)
                .unwrap_err()
                .contains("max_error_m")
        );
    }

    /// 폴더 로더가 파일명과 무관하게 ID 순으로 반환하고 README를 무시한다.
    #[test]
    fn directory_load_is_sorted_by_patch_id() {
        let dir = temp_dir("sorted");
        std::fs::write(dir.join("z.toml"), valid("verified", "alpha")).unwrap();
        std::fs::write(dir.join("a.toml"), valid("verified", "zulu")).unwrap();
        std::fs::write(dir.join("README.md"), "ignored").unwrap();
        let patches = load_patch_dir(&dir, PatchAudience::Distribution).unwrap();
        assert_eq!(
            patches
                .iter()
                .map(|patch| patch.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zulu"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// 폴더 안에서 같은 보정 ID를 두 번 정의하지 못하게 한다.
    #[test]
    fn directory_load_rejects_duplicate_patch_id() {
        let dir = temp_dir("duplicate");
        std::fs::write(dir.join("a.toml"), valid("verified", "same")).unwrap();
        std::fs::write(dir.join("b.toml"), valid("verified", "same")).unwrap();
        let error = load_patch_dir(&dir, PatchAudience::Distribution).unwrap_err();
        assert!(error.contains("duplicate patch ID"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// bbox 선택 결과가 입력 순서가 아니라 보정 ID 순서를 따른다.
    #[test]
    fn bbox_selection_is_deterministic() {
        let a = parse_patch(
            valid("verified", "alpha").as_bytes(),
            "a",
            PatchAudience::Test,
        )
        .unwrap();
        let z = parse_patch(
            valid("verified", "zulu").as_bytes(),
            "z",
            PatchAudience::Test,
        )
        .unwrap();
        let patches = [z, a];
        let selected = patches_for_bbox(
            &patches,
            &PatchBbox::from_array([34.751, 127.749, 34.753, 127.751]),
        );
        assert_eq!(
            selected
                .iter()
                .map(|patch| patch.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zulu"]
        );
    }

    /// 시험마다 겹치지 않는 임시 폴더를 새로 만든다.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sds-map-patch-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
