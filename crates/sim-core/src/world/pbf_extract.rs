//! Geofabrik `.osm.pbf`에서 지역 도로 스냅샷을 만든다(Phase 4-5G, G-1).
//!
//! 두 번 읽는다.
//!
//! 1. **way:** 전국의 way 중 허용 `highway`만 골라 way ID·태그·노드 ID 목록을 모은다.
//! 2. **node:** 1에서 모은 노드 ID의 좌표만 읽는다.
//!
//! 영역 판별에는 좌표가 필요하므로 2가 끝날 때까지 **전국 허용 도로 전체를 메모리에 보유**한다.
//! 이 방식은 지역 자료만 읽는 방식이 아니다(인덱스는 G-2).
//!
//! 선택 규칙(제안서 §3.1): 사용자 영역에 여유 폭을 더한 영역 R에 노드가 하나라도 있거나, 연속한 두 노드의 선분이 R과
//! 교차하는 way를 고른다. 고른 way는 R 밖 노드까지 모두 보존한다. 노드 ID·순서·태그를 바꾸지 않는다.
//!
//! 경계 연결 검증(§3.2): 스냅샷의 모든 노드를 전국 자료와 (way ID, 이웃 노드 ID) 집합으로 대조해
//! [`Connectivity`]를 만든다. 지역에만 있는 연결이 나오면 추출 결함이므로 오류다.
//!
//! 직렬화(§7.1): way는 ID 오름차순, way 안 노드는 원래 순서, 태그는 키 오름차순, 좌표는 PBF 정수값(1e-7°)을
//! 소수 7자리 고정으로 쓴 한 줄 JSON이다. 같은 입력이면 본문·연결 정보 바이트가 같다.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use osmpbf::{BlobDecode, BlobReader, Element};

use super::map::LatLon;
use super::map_fetch::{self, Bbox};
use super::osm_graph::{EXTRACT_GENERATOR_PREFIX, OsmGraphWay, OsmNode};
use super::road_snapshot::{
    self, CONNECTIVITY_FILE, CONNECTIVITY_FORMAT, Connectivity, ConnectivityRecord,
    EXTRACT_RULE_VERSION, GeofabrikSource, IncompleteNode, META_FORMAT_V2, SnapshotMetaV2,
};

/// Geofabrik 추출 세대를 저장하는 캐시 폴더(저장소가 추적하지 않음).
pub const EXTRACT_DIR: &str = "cache/extracts";
/// 추출 본문의 `generator` 값.
pub const GENERATOR: &str = "self-driving-sim geofabrik-extract v1";
/// 기본 여유 폭(m). 점 붙이기 상한 100 m보다 크게 둔다(임시값).
pub const DEFAULT_MARGIN_M: f64 = 150.0;
/// 여유 폭 상한(m).
pub const MAX_MARGIN_M: f64 = 1000.0;
/// 읽을 수 있는 PBF 필수 기능.
const KNOWN_FEATURES: [&str; 2] = ["OsmSchema-V0.6", "DenseNodes"];
/// 지구 평균 반지름(m). [`super::map::project`]와 같은 값.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// PBF 머리부 정보.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PbfHeader {
    /// 복제 시각(유닉스 초). 없으면 `None`.
    pub replication_timestamp: Option<i64>,
    /// 복제 순번. 없으면 `None`.
    pub replication_sequence: Option<i64>,
}

/// 1차 읽기에서 모은 허용 도로 way(좌표 전).
#[derive(Debug, Clone, PartialEq)]
pub struct RoadWay {
    /// way ID.
    pub id: u64,
    /// 태그 전체(키 오름차순).
    pub tags: BTreeMap<String, String>,
    /// 노드 ID(원래 순서).
    pub refs: Vec<u64>,
}

/// 두 번 읽은 전국 허용 도로.
#[derive(Debug, Clone, PartialEq)]
pub struct NationalRoads {
    /// 머리부.
    pub header: PbfHeader,
    /// 허용 도로 way(ID 오름차순).
    pub ways: Vec<RoadWay>,
    /// way가 참조한 노드 ID(오름차순, 중복 없음).
    node_ids: Vec<u64>,
    /// `node_ids`와 같은 순서의 좌표 [위도, 경도](1e-7°). PBF에 없으면 `None`.
    coords: Vec<Option<[i32; 2]>>,
    /// 읽기 단계별 걸린 시간(초).
    pub timings: Vec<(&'static str, f64)>,
}

impl NationalRoads {
    /// 노드 좌표(1e-7°). 없으면 `None`.
    pub fn coord(&self, id: u64) -> Option<[i32; 2]> {
        self.node_ids
            .binary_search(&id)
            .ok()
            .and_then(|index| self.coords[index])
    }

    /// way가 참조한 서로 다른 노드 수.
    pub fn node_count(&self) -> usize {
        self.node_ids.len()
    }

    /// 좌표가 PBF에 없는 노드 수.
    pub fn missing_coord_count(&self) -> usize {
        self.coords.iter().filter(|coord| coord.is_none()).count()
    }
}

/// PBF 블록을 차례로 풀어 요소마다 `visit`를 부른다. 머리부가 처음에 와야 하고, 모든 블록을 끝까지 풀어야 한다
/// (손상·잘린 파일은 여기서 오류). 머리부를 돌려준다. 전국 자료 변환([`super::korea_data`])도 쓴다.
pub(crate) fn scan<R: Read + Send>(
    reader: R,
    mut visit: impl FnMut(Element) -> Result<(), String>,
) -> Result<PbfHeader, String> {
    let mut header: Option<PbfHeader> = None;
    for (index, blob) in BlobReader::new(reader).enumerate() {
        let blob = blob.map_err(|error| format!("PBF block {index} cannot be read: {error}"))?;
        match blob
            .decode()
            .map_err(|error| format!("PBF block {index} cannot be decoded: {error}"))?
        {
            BlobDecode::OsmHeader(block) => {
                if index != 0 {
                    return Err(format!("PBF header block appears at block {index}"));
                }
                if let Some(feature) = block
                    .required_features()
                    .iter()
                    .find(|feature| !KNOWN_FEATURES.contains(&feature.as_str()))
                {
                    return Err(format!("PBF requires unsupported feature `{feature}`"));
                }
                header = Some(PbfHeader {
                    replication_timestamp: block.osmosis_replication_timestamp(),
                    replication_sequence: block.osmosis_replication_sequence_number(),
                });
            }
            BlobDecode::OsmData(block) => {
                if header.is_none() {
                    return Err("PBF data appears before the header block".to_owned());
                }
                let mut failure = None;
                block.for_each_element(|element| {
                    if failure.is_none()
                        && let Err(error) = visit(element)
                    {
                        failure = Some(error);
                    }
                });
                if let Some(error) = failure {
                    return Err(error);
                }
            }
            BlobDecode::Unknown(_) => {}
        }
    }
    header.ok_or_else(|| "PBF has no header block".to_owned())
}

/// 양수 OSM ID를 `u64`로 바꾼다. 0 이하(편집 중 임시 ID 등)는 오류다.
fn positive_id(kind: &str, id: i64) -> Result<u64, String> {
    u64::try_from(id)
        .ok()
        .filter(|&id| id > 0)
        .ok_or_else(|| format!("{kind} has a non-positive ID {id}"))
}

/// PBF를 두 번 읽어 전국 허용 도로를 모은다. `open`은 같은 파일을 처음부터 읽는 리더를 만든다.
///
/// 같은 way ID가 두 번 나오거나 같은 노드 ID에 다른 좌표가 나오면 오류다.
pub fn read_national_roads<R, F>(open: F, allowed: &[String]) -> Result<NationalRoads, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    let started = Instant::now();
    let mut ways: Vec<RoadWay> = Vec::new();
    let header = scan(open()?, |element| {
        let Element::Way(way) = element else {
            return Ok(());
        };
        let mut tags = BTreeMap::new();
        for (key, value) in way.tags() {
            tags.insert(key.to_owned(), value.to_owned());
        }
        if !tags
            .get("highway")
            .is_some_and(|kind| allowed.contains(kind.as_str()))
        {
            return Ok(());
        }
        let id = positive_id("way", way.id())?;
        let refs = way
            .refs()
            .map(|node| positive_id("way node", node))
            .collect::<Result<Vec<_>, _>>()?;
        ways.push(RoadWay { id, tags, refs });
        Ok(())
    })?;
    ways.sort_by_key(|way| way.id);
    if let Some(pair) = ways.windows(2).find(|pair| pair[0].id == pair[1].id) {
        return Err(format!("way {} appears twice in the PBF", pair[0].id));
    }
    let after_ways = started.elapsed().as_secs_f64();
    let mut node_ids: Vec<u64> = ways
        .iter()
        .flat_map(|way| way.refs.iter().copied())
        .collect();
    node_ids.sort_unstable();
    node_ids.dedup();
    let mut coords: Vec<Option<[i32; 2]>> = vec![None; node_ids.len()];
    let mut place = |id: i64, lat: i32, lon: i32| -> Result<(), String> {
        let id = positive_id("node", id)?;
        if let Ok(index) = node_ids.binary_search(&id) {
            match coords[index] {
                Some(known) if known != [lat, lon] => {
                    return Err(format!(
                        "node {id} appears twice with different coordinates"
                    ));
                }
                _ => coords[index] = Some([lat, lon]),
            }
        }
        Ok(())
    };
    scan(open()?, |element| match element {
        Element::Node(node) => place(node.id(), node.decimicro_lat(), node.decimicro_lon()),
        Element::DenseNode(node) => place(node.id(), node.decimicro_lat(), node.decimicro_lon()),
        _ => Ok(()),
    })?;
    let total = started.elapsed().as_secs_f64();
    Ok(NationalRoads {
        header,
        ways,
        node_ids,
        coords,
        timings: vec![
            ("read ways", after_ways),
            ("read nodes", total - after_ways),
        ],
    })
}

/// 영역(deg). 남·서·북·동.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// 남쪽 위도.
    pub south: f64,
    /// 서쪽 경도.
    pub west: f64,
    /// 북쪽 위도.
    pub north: f64,
    /// 동쪽 경도.
    pub east: f64,
}

impl Area {
    /// 사용자 영역에 여유 폭 `margin_m`을 사방으로 더한다. 동서 폭은 가운데 위도에서 잰다.
    pub fn around(bbox: &Bbox, margin_m: f64) -> Self {
        let d_lat = (margin_m / EARTH_RADIUS_M).to_degrees();
        let middle = ((bbox.south + bbox.north) / 2.0).to_radians();
        let d_lon = (margin_m / (EARTH_RADIUS_M * middle.cos())).to_degrees();
        Self {
            south: bbox.south - d_lat,
            west: bbox.west - d_lon,
            north: bbox.north + d_lat,
            east: bbox.east + d_lon,
        }
    }

    /// 점 [위도, 경도]가 영역 안(경계 포함)인지.
    pub fn contains(&self, [lat, lon]: [f64; 2]) -> bool {
        (self.south..=self.north).contains(&lat) && (self.west..=self.east).contains(&lon)
    }

    /// 선분 `a`–`b`가 영역과 겹치는지(Liang–Barsky 자르기). 끝점이 모두 밖이어도 영역을 지나면 `true`.
    pub fn segment_hits(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        let (d_lat, d_lon) = (b[0] - a[0], b[1] - a[1]);
        let mut range = (0.0_f64, 1.0_f64);
        for (p, q) in [
            (-d_lat, a[0] - self.south),
            (d_lat, self.north - a[0]),
            (-d_lon, a[1] - self.west),
            (d_lon, self.east - a[1]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return false;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    range.0 = range.0.max(t);
                } else {
                    range.1 = range.1.min(t);
                }
                if range.0 > range.1 {
                    return false;
                }
            }
        }
        true
    }
}

/// 1e-7° 정수 좌표를 도로 바꾼다.
fn degrees([lat, lon]: [i32; 2]) -> [f64; 2] {
    [f64::from(lat) / 1e7, f64::from(lon) / 1e7]
}

/// 추출한 way 하나(좌표는 PBF 정수값 그대로).
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedWay {
    /// way ID.
    pub id: u64,
    /// 태그.
    pub tags: BTreeMap<String, String>,
    /// (노드 ID, [위도, 경도] 1e-7°). 원래 순서.
    pub nodes: Vec<(u64, [i32; 2])>,
}

/// 영역 R(= `area`)에 노드가 있거나 선분이 지나는 허용 도로 way를 고른다(§3.1). 고른 way는 노드를 모두 보존한다.
///
/// 고른 way의 노드 좌표가 PBF에 없으면 오류다. 좌표가 빠진 way는 아는 좌표만으로 판정한다.
pub fn select_ways(roads: &NationalRoads, area: &Area) -> Result<Vec<ExtractedWay>, String> {
    let mut selected = Vec::new();
    for way in &roads.ways {
        let points: Vec<Option<[f64; 2]>> = way
            .refs
            .iter()
            .map(|&id| roads.coord(id).map(degrees))
            .collect();
        let inside = points.iter().flatten().any(|&point| area.contains(point));
        let crosses = points.windows(2).any(|pair| match (pair[0], pair[1]) {
            (Some(a), Some(b)) => area.segment_hits(a, b),
            _ => false,
        });
        if !(inside || crosses) {
            continue;
        }
        let nodes = way
            .refs
            .iter()
            .map(|&id| {
                roads.coord(id).map(|coord| (id, coord)).ok_or_else(|| {
                    format!(
                        "way {} uses node {id}, which has no coordinate in the PBF",
                        way.id
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.len() < 2 {
            return Err(format!("way {} has fewer than 2 nodes", way.id));
        }
        selected.push(ExtractedWay {
            id: way.id,
            tags: way.tags.clone(),
            nodes,
        });
    }
    if selected.is_empty() {
        return Err("no allowed roads in the selected area".to_owned());
    }
    Ok(selected)
}

/// (way ID, 이웃 노드 ID) 연결을 노드별로 모은다. `keep`에 있는 노드만 기록한다.
fn adjacency<'a>(
    ways: impl IntoIterator<Item = (u64, &'a [u64])>,
    keep: &BTreeSet<u64>,
) -> BTreeMap<u64, BTreeSet<[u64; 2]>> {
    let mut map: BTreeMap<u64, BTreeSet<[u64; 2]>> = BTreeMap::new();
    for (way, refs) in ways {
        for pair in refs.windows(2) {
            for (node, neighbor) in [(pair[0], pair[1]), (pair[1], pair[0])] {
                if keep.contains(&node) && node != neighbor {
                    map.entry(node).or_default().insert([way, neighbor]);
                }
            }
        }
    }
    map
}

/// 지역 way의 모든 노드를 전국 way와 (way ID, 이웃 노드 ID) 집합으로 대조한다(§3.2).
///
/// 반환값은 (대조한 노드 수, 불완전 노드 목록). 지역에만 있는 연결이 있으면(같은 원본에서는 생길 수 없음) 오류다.
pub fn compare_connectivity<'a>(
    national: impl IntoIterator<Item = (u64, &'a [u64])>,
    local: impl IntoIterator<Item = (u64, &'a [u64])> + Clone,
) -> Result<(u64, Vec<IncompleteNode>), String> {
    let nodes: BTreeSet<u64> = local
        .clone()
        .into_iter()
        .flat_map(|(_, refs)| refs.iter().copied())
        .collect();
    let national = adjacency(national, &nodes);
    let local = adjacency(local, &nodes);
    let empty = BTreeSet::new();
    let mut incomplete = Vec::new();
    for &node in &nodes {
        let whole = national.get(&node).unwrap_or(&empty);
        let here = local.get(&node).unwrap_or(&empty);
        if let Some(extra) = here.difference(whole).next() {
            return Err(format!(
                "node {node} has connection {extra:?} that the national data does not have"
            ));
        }
        if whole != here {
            incomplete.push(IncompleteNode {
                node,
                national_degree: whole.len() as u32,
                local_degree: here.len() as u32,
                missing: whole.difference(here).copied().collect(),
            });
        }
    }
    Ok((nodes.len() as u64, incomplete))
}

/// 1e-7° 정수를 소수 7자리 고정 문자열로 쓴다(예: `347512345` → `34.7512345`).
pub(crate) fn fixed7(value: i32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let abs = value.unsigned_abs();
    format!("{sign}{}.{:07}", abs / 10_000_000, abs % 10_000_000)
}

/// 추출 way로 본문(`graph.osm.json`)을 만든다. way ID 오름차순, 정해진 직렬화(모듈 설명 참고).
pub fn graph_body(ways: &[ExtractedWay]) -> String {
    let mut sorted: Vec<&ExtractedWay> = ways.iter().collect();
    sorted.sort_by_key(|way| way.id);
    let quote = |text: &str| serde_json::to_string(text).expect("string serializes");
    let elements: Vec<String> = sorted
        .iter()
        .map(|way| {
            let ids: Vec<String> = way.nodes.iter().map(|(id, _)| id.to_string()).collect();
            let geometry: Vec<String> = way
                .nodes
                .iter()
                .map(|(_, [lat, lon])| {
                    format!("{{\"lat\":{},\"lon\":{}}}", fixed7(*lat), fixed7(*lon))
                })
                .collect();
            let tags: Vec<String> = way
                .tags
                .iter()
                .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
                .collect();
            format!(
                "{{\"type\":\"way\",\"id\":{},\"nodes\":[{}],\"geometry\":[{}],\"tags\":{{{}}}}}",
                way.id,
                ids.join(","),
                geometry.join(","),
                tags.join(",")
            )
        })
        .collect();
    format!(
        "{{\"generator\":{},\"elements\":[{}]}}\n",
        quote(GENERATOR),
        elements.join(",")
    )
}

/// 원본 PBF 식별 정보.
#[derive(Debug, Clone, PartialEq)]
pub struct PbfSource {
    /// 파일 이름.
    pub file_name: String,
    /// SHA-256.
    pub sha256: String,
    /// 크기(바이트).
    pub bytes: u64,
}

/// 파일을 조금씩 읽어 SHA-256과 크기를 구한다(큰 PBF를 메모리에 다 올리지 않는다).
pub fn hash_file(path: &Path) -> Result<PbfSource, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    let mut bytes = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok(PbfSource {
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown.osm.pbf")
            .to_owned(),
        sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    })
}

/// 지역 추출 결과(세대 폴더에 쓸 내용).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionExtract {
    /// 본문.
    pub body: String,
    /// 연결 정보.
    pub connectivity: Connectivity,
    /// 메타(`created_at`은 실행 시각).
    pub meta: SnapshotMetaV2,
    /// 고른 way 수.
    pub ways: usize,
}

/// 전국 허용 도로에서 지역을 골라 본문·연결 정보·메타를 만든다.
pub fn extract_region(
    roads: &NationalRoads,
    source: &PbfSource,
    bbox: &Bbox,
    margin_m: f64,
    allowed: &[String],
) -> Result<RegionExtract, String> {
    if !(margin_m.is_finite() && (0.0..=MAX_MARGIN_M).contains(&margin_m)) {
        return Err(format!(
            "margin must be 0..={MAX_MARGIN_M} m, got {margin_m}"
        ));
    }
    bbox.validate()?;
    let allowed = road_snapshot::normalized_highways(allowed);
    let selected = select_ways(roads, &Area::around(bbox, margin_m))?;
    let body = graph_body(&selected);
    let local_refs: Vec<(u64, Vec<u64>)> = selected
        .iter()
        .map(|way| (way.id, way.nodes.iter().map(|(id, _)| *id).collect()))
        .collect();
    let (checked, incomplete) = compare_connectivity(
        roads.ways.iter().map(|way| (way.id, way.refs.as_slice())),
        local_refs.iter().map(|(id, refs)| (*id, refs.as_slice())),
    )?;
    let graph_sha256 = road_snapshot::sha256_hex(body.as_bytes());
    let connectivity = Connectivity {
        format: CONNECTIVITY_FORMAT.to_owned(),
        rule_version: EXTRACT_RULE_VERSION,
        graph_sha256: graph_sha256.clone(),
        allowed_highways: allowed.clone(),
        checked_nodes: checked,
        incomplete,
    };
    let connectivity_json = connectivity.to_json();
    let header = &roads.header;
    let meta = SnapshotMetaV2 {
        meta_format: META_FORMAT_V2.to_owned(),
        source: "geofabrik".to_owned(),
        bbox: *bbox,
        created_at: map_fetch::utc_now(),
        graph_file: map_fetch::SNAPSHOT_FILE.to_owned(),
        graph_sha256,
        graph_bytes: body.len() as u64,
        geofabrik: GeofabrikSource {
            pbf_file_name: source.file_name.clone(),
            pbf_url: None,
            pbf_sha256: source.sha256.clone(),
            pbf_bytes: source.bytes,
            // G-1은 사용자가 준 로컬 파일을 쓰므로 제공 체크섬과 대조하지 않는다.
            integrity: "unverified".to_owned(),
            replication_timestamp: header
                .replication_timestamp
                .and_then(|seconds| u64::try_from(seconds).ok())
                .map(map_fetch::rfc3339),
            replication_sequence: header.replication_sequence,
            data_time: if header.replication_timestamp.is_some() {
                "header"
            } else {
                "unknown"
            }
            .to_owned(),
            extract_rule_version: EXTRACT_RULE_VERSION,
            margin_m,
            allowed_highways: allowed,
        },
        connectivity: ConnectivityRecord {
            file: CONNECTIVITY_FILE.to_owned(),
            format: CONNECTIVITY_FORMAT.to_owned(),
            sha256: road_snapshot::sha256_hex(connectivity_json.as_bytes()),
            checked_nodes: checked,
            incomplete_nodes: connectivity.incomplete.len() as u64,
        },
    };
    Ok(RegionExtract {
        body,
        connectivity,
        meta,
        ways: selected.len(),
    })
}

/// 추출 결과를 새 세대 폴더 `root/<bbox 이름>/<UTC 시각>-<본문 해시 앞 16자리>/`에 쓴다.
///
/// 임시 폴더에 모두 쓰고 다시 읽어 검사한 뒤 이름을 바꿔 게시한다. 이미 있는 세대 폴더는 고치지 않는다.
pub fn write_generation(root: &Path, extract: &RegionExtract) -> Result<PathBuf, String> {
    let bbox_dir = root.join(extract.meta.bbox.cache_name());
    std::fs::create_dir_all(&bbox_dir)
        .map_err(|error| format!("cannot create {}: {error}", bbox_dir.display()))?;
    let time: String = extract
        .meta
        .created_at
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let generation = format!("{time}-{}", &extract.meta.graph_sha256[..16]);
    let dir = bbox_dir.join(&generation);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    let meta = toml::to_string(&extract.meta)
        .map_err(|error| format!("cannot write snapshot record: {error}"))?;
    let staging = bbox_dir.join(format!(".partial-{generation}"));
    let _ = std::fs::remove_dir_all(&staging);
    let staged = std::fs::create_dir(&staging)
        .map_err(|error| format!("cannot create {}: {error}", staging.display()))
        .and_then(|()| {
            map_fetch::write_atomic(
                &staging.join(map_fetch::SNAPSHOT_FILE),
                extract.body.as_bytes(),
            )
        })
        .and_then(|()| {
            map_fetch::write_atomic(
                &staging.join(CONNECTIVITY_FILE),
                extract.connectivity.to_json().as_bytes(),
            )
        })
        .and_then(|()| {
            map_fetch::write_atomic(&staging.join(map_fetch::META_FILE), meta.as_bytes())
        })
        .and_then(|()| road_snapshot::load_extract_generation(&staging).map(|_| ()))
        .and_then(|()| {
            std::fs::rename(&staging, &dir)
                .map_err(|error| format!("cannot publish {}: {error}", dir.display()))
        });
    if let Err(error) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(dir)
}

/// 추출 실행 보고.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractReport {
    /// 만든 세대 폴더.
    pub dir: PathBuf,
    /// 전국 허용 도로 way 수.
    pub national_ways: usize,
    /// 전국 허용 도로가 참조한 노드 수.
    pub national_nodes: usize,
    /// 좌표가 PBF에 없는 노드 수.
    pub missing_coords: usize,
    /// 고른 way 수.
    pub ways: usize,
    /// 대조한 노드 수.
    pub checked_nodes: u64,
    /// 불완전 노드 수.
    pub incomplete_nodes: usize,
    /// 단계별 시간(초).
    pub timings: Vec<(&'static str, f64)>,
    /// 실행 중 최대 작업 메모리(바이트). 잴 수 없으면 `None`.
    pub peak_memory_bytes: Option<u64>,
}

/// 로컬 PBF 파일에서 지역을 추출해 `root` 아래 새 세대로 저장한다(`--extract-pbf` 명령).
pub fn extract_pbf_file(
    pbf: &Path,
    bbox: &Bbox,
    margin_m: f64,
    allowed: &[String],
    root: &Path,
) -> Result<ExtractReport, String> {
    let started = Instant::now();
    let source = hash_file(pbf)?;
    let hashed = started.elapsed().as_secs_f64();
    let roads = read_national_roads(
        || {
            std::fs::File::open(pbf)
                .map(std::io::BufReader::new)
                .map_err(|error| format!("cannot open {}: {error}", pbf.display()))
        },
        allowed,
    )?;
    let before_extract = Instant::now();
    let extract = extract_region(&roads, &source, bbox, margin_m, allowed)?;
    let dir = write_generation(root, &extract)?;
    let mut timings = vec![("hash PBF", hashed)];
    timings.extend(roads.timings.iter().copied());
    timings.push((
        "select, check, write",
        before_extract.elapsed().as_secs_f64(),
    ));
    Ok(ExtractReport {
        dir,
        national_ways: roads.ways.len(),
        national_nodes: roads.node_count(),
        missing_coords: roads.missing_coord_count(),
        ways: extract.ways,
        checked_nodes: extract.connectivity.checked_nodes,
        incomplete_nodes: extract.connectivity.incomplete.len(),
        timings,
        peak_memory_bytes: peak_memory_bytes(),
    })
}

/// 본문 way를 [`OsmGraphWay`]로 바꾼다(시험·비교용). 좌표는 1e-7° 정수를 도로 나눈 값이다.
pub fn to_graph_ways(ways: &[ExtractedWay]) -> Vec<OsmGraphWay> {
    ways.iter()
        .map(|way| OsmGraphWay {
            id: way.id,
            tags: way.tags.clone(),
            nodes: way
                .nodes
                .iter()
                .map(|&(id, coord)| {
                    let [lat, lon] = degrees(coord);
                    OsmNode {
                        id,
                        pos: LatLon { lat, lon },
                    }
                })
                .collect(),
        })
        .collect()
}

/// 이 프로세스의 최대 작업 집합(Windows `PeakWorkingSetSize`, 바이트). 다른 OS에서는 `None`.
#[cfg(windows)]
pub fn peak_memory_bytes() -> Option<u64> {
    /// Win32 `PROCESS_MEMORY_COUNTERS`.
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: 현재 프로세스 의사 핸들과 크기를 맞춘 구조체 포인터만 넘긴다.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some(counters.peak_working_set_size as u64)
}

/// 이 프로세스의 최대 작업 집합. Windows가 아니면 잴 수 없다.
#[cfg(not(windows))]
pub fn peak_memory_bytes() -> Option<u64> {
    None
}

/// 본문 `generator`가 추출 접두어로 시작하는지 컴파일 시점에 확인한다.
const _: () = {
    let (a, b) = (GENERATOR.as_bytes(), EXTRACT_GENERATOR_PREFIX.as_bytes());
    assert!(a.len() >= b.len());
    let mut i = 0;
    while i < b.len() {
        assert!(a[i] == b[i]);
        i += 1;
    }
};

#[cfg(test)]
mod tests {
    //! 정답 fixture 대조(제안서 §7.3). 기대값은 아래에 사람이 직접 적었고, 추출 함수 출력에서 만들지 않았다.

    use super::*;
    use crate::world::osm_graph::{fixtures, parse_overpass_graph};
    use crate::world::pbf_fixture::{Fixture, e7};
    use crate::world::route_plan::{self, RoadGraphConfig};
    use std::io::Cursor;

    /// 합성 기준점 둘레 반 변 `half_m`(m) 정사각형 사용자 영역.
    fn square(half_m: f64) -> Bbox {
        Bbox {
            south: fixtures::offset(0.0, -half_m).lat,
            west: fixtures::offset(-half_m, 0.0).lon,
            north: fixtures::offset(0.0, half_m).lat,
            east: fixtures::offset(half_m, 0.0).lon,
        }
    }

    /// 기본 허용 도로 목록.
    fn allowed() -> Vec<String> {
        RoadGraphConfig::default().allowed_highways
    }

    /// 시험용 원본 식별 정보.
    fn source(bytes: &[u8]) -> PbfSource {
        PbfSource {
            file_name: "fixture.osm.pbf".to_owned(),
            sha256: road_snapshot::sha256_hex(bytes),
            bytes: bytes.len() as u64,
        }
    }

    /// 메모리의 PBF를 두 번 읽는다.
    fn read(bytes: &[u8]) -> Result<NationalRoads, String> {
        let bytes = bytes.to_vec();
        read_national_roads(|| Ok(Cursor::new(bytes.clone())), &allowed())
    }

    /// 시험마다 비운 임시 폴더.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sds-pbf-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 선택·연결 시험 자료. 사용자 영역은 반 변 100 m(여유 폭 0).
    ///
    /// - way 1 (residential) 1(-50,0)–2(0,0)–3(50,0): 영역 안 → 선택
    /// - way 2 (residential) 21(-150,50)–22(150,50): 양 끝은 밖, 선분이 지남 → 선택, 21·22 보존
    /// - way 3 (residential) 31(120,-150)–32(150,-120): 밖, 선분도 안 지남 → 제외
    /// - way 5 (residential) 22–51(200,50): 밖 → 제외, 그래서 노드 22는 불완전(빠진 연결 [5, 51])
    /// - way 6 (footway) 2–61(0,-50): 허용 도로 아님 → 제외, 노드 2 연결에도 세지 않음
    /// - way 8 (service) 81(80,-80)–82(300,-80): 81이 안 → 선택, 82 보존
    fn selection_fixture(dense: bool) -> Fixture {
        let node = |id, east, north| (id, east, north, None);
        let way = |id, kind, refs: Vec<u64>| (id, vec![("highway", kind)], refs, None);
        Fixture {
            nodes: vec![
                node(1, -50.0, 0.0),
                node(2, 0.0, 0.0),
                node(3, 50.0, 0.0),
                node(21, -150.0, 50.0),
                node(22, 150.0, 50.0),
                node(31, 120.0, -150.0),
                node(32, 150.0, -120.0),
                node(51, 200.0, 50.0),
                node(61, 0.0, -50.0),
                node(81, 80.0, -80.0),
                node(82, 300.0, -80.0),
            ],
            ways: vec![
                way(1, "residential", vec![1, 2, 3]),
                way(2, "residential", vec![21, 22]),
                way(3, "residential", vec![31, 32]),
                way(5, "residential", vec![22, 51]),
                way(6, "footway", vec![2, 61]),
                way(8, "service", vec![81, 82]),
            ],
            replication: Some((1_793_000_000, 4242)),
            dense,
            ..Fixture::default()
        }
    }

    /// §7.3 표의 선택·보존·연결 기대값과 같은지 확인한다(DenseNodes·일반 Node 둘 다).
    #[test]
    fn fixture_extract_matches_hand_written_expectations() {
        for dense in [true, false] {
            let pbf = selection_fixture(dense).to_pbf();
            let roads = read(&pbf).unwrap();
            assert_eq!(roads.ways.len(), 5, "footway is not read (dense {dense})");
            let extract =
                extract_region(&roads, &source(&pbf), &square(100.0), 0.0, &allowed()).unwrap();
            let ways = parse_graph_ways(&extract.body);
            let ids: Vec<u64> = ways.iter().map(|w| w.id).collect();
            assert_eq!(ids, vec![1, 2, 8], "dense {dense}");
            let expected_nodes: [(u64, &[u64]); 3] =
                [(1, &[1, 2, 3]), (2, &[21, 22]), (8, &[81, 82])];
            for (way, (id, refs)) in ways.iter().zip(expected_nodes) {
                assert_eq!(way.id, id);
                let got: Vec<u64> = way.nodes.iter().map(|n| n.id).collect();
                assert_eq!(got, refs, "way {id} keeps all nodes in order");
            }
            // 영역 밖 노드 좌표도 원본 정수값 그대로 보존한다.
            for (id, east, north) in [(21, -150.0, 50.0), (22, 150.0, 50.0), (82, 300.0, -80.0)] {
                let node = ways
                    .iter()
                    .flat_map(|w| &w.nodes)
                    .find(|n| n.id == id)
                    .unwrap();
                let [lat, lon] = e7(east, north);
                assert_eq!(
                    [
                        (node.pos.lat * 1e7).round() as i32,
                        (node.pos.lon * 1e7).round() as i32
                    ],
                    [lat, lon],
                    "node {id}"
                );
            }
            let connectivity = &extract.connectivity;
            assert_eq!(connectivity.checked_nodes, 7);
            assert_eq!(
                connectivity.incomplete,
                vec![IncompleteNode {
                    node: 22,
                    national_degree: 2,
                    local_degree: 1,
                    missing: vec![[5, 51]],
                }]
            );
            assert_eq!(extract.meta.geofabrik.replication_sequence, Some(4242));
            assert_eq!(
                extract.meta.geofabrik.replication_timestamp.as_deref(),
                Some("2026-10-26T07:33:20Z")
            );
            assert_eq!(extract.meta.geofabrik.data_time, "header");
            assert_eq!(extract.meta.geofabrik.integrity, "unverified");
        }
    }

    /// 본문을 출처 확인 없이 읽는다(시험용).
    fn parse_graph_ways(body: &str) -> Vec<OsmGraphWay> {
        crate::world::osm_graph::parse_graph_body(body).unwrap().1
    }

    /// 같은 입력이면 본문·연결 정보 바이트가 같고, 메타는 만든 시각만 다를 수 있다(§7.1 재현성).
    #[test]
    fn same_input_gives_same_bytes() {
        let pbf = selection_fixture(true).to_pbf();
        let run = || {
            let roads = read(&pbf).unwrap();
            extract_region(&roads, &source(&pbf), &square(100.0), 0.0, &allowed()).unwrap()
        };
        let (first, mut second) = (run(), run());
        assert_eq!(first.body, second.body);
        assert_eq!(first.connectivity.to_json(), second.connectivity.to_json());
        second.meta.created_at = first.meta.created_at.clone();
        assert_eq!(first.meta, second.meta);
    }

    /// 머리부 복제 정보가 없으면 자료 시각을 `unknown`으로 기록한다.
    #[test]
    fn header_without_replication_is_unknown_time() {
        let mut fixture = selection_fixture(true);
        fixture.replication = None;
        let pbf = fixture.to_pbf();
        let extract = extract_region(
            &read(&pbf).unwrap(),
            &source(&pbf),
            &square(100.0),
            0.0,
            &allowed(),
        )
        .unwrap();
        assert_eq!(extract.meta.geofabrik.replication_timestamp, None);
        assert_eq!(extract.meta.geofabrik.data_time, "unknown");
    }

    /// 세대 폴더를 쓰고 다시 읽으며, 본문·연결 정보·규칙 버전이 맞지 않으면 거부하고,
    /// 기존 Overpass 읽기 함수로는 추출 본문을 읽을 수 없는지 확인한다.
    #[test]
    fn generation_roundtrip_and_tampering() {
        let pbf = selection_fixture(true).to_pbf();
        let extract = extract_region(
            &read(&pbf).unwrap(),
            &source(&pbf),
            &square(100.0),
            0.0,
            &allowed(),
        )
        .unwrap();
        let root = temp_dir("roundtrip");
        let dir = write_generation(&root, &extract).unwrap();
        let (meta, snapshot) = road_snapshot::load_extract_generation(&dir).unwrap();
        assert_eq!(meta.graph_sha256, extract.meta.graph_sha256);
        assert_eq!(snapshot.connectivity.as_ref(), Some(&extract.connectivity));
        assert!(
            write_generation(&root, &extract).is_err(),
            "existing generation is not rewritten"
        );
        let body = std::fs::read_to_string(dir.join(map_fetch::SNAPSHOT_FILE)).unwrap();
        assert!(
            parse_overpass_graph(&body)
                .unwrap_err()
                .contains("Geofabrik extract")
        );

        let copy = |name: &str| {
            let target = root.join(name);
            std::fs::create_dir_all(&target).unwrap();
            for file in [
                map_fetch::SNAPSHOT_FILE,
                map_fetch::META_FILE,
                CONNECTIVITY_FILE,
            ] {
                std::fs::copy(dir.join(file), target.join(file)).unwrap();
            }
            target
        };
        let bad = copy("bad-connectivity");
        std::fs::write(
            bad.join(CONNECTIVITY_FILE),
            extract.connectivity.to_json().replace("22", "23"),
        )
        .unwrap();
        assert!(road_snapshot::load_extract_generation(&bad).is_err());
        let bad = copy("bad-graph");
        std::fs::write(
            bad.join(map_fetch::SNAPSHOT_FILE),
            body.replace("\"service\"", "\"servic3\""),
        )
        .unwrap();
        assert!(road_snapshot::load_road_snapshot(&bad.join(map_fetch::SNAPSHOT_FILE)).is_err());
        let bad = copy("no-connectivity");
        std::fs::remove_file(bad.join(CONNECTIVITY_FILE)).unwrap();
        assert!(road_snapshot::load_road_snapshot(&bad.join(map_fetch::SNAPSHOT_FILE)).is_err());
        let bad = copy("unknown-rule");
        let meta_text = std::fs::read_to_string(bad.join(map_fetch::META_FILE)).unwrap();
        std::fs::write(
            bad.join(map_fetch::META_FILE),
            meta_text.replace("extract_rule_version = 1", "extract_rule_version = 2"),
        )
        .unwrap();
        assert!(
            road_snapshot::load_extract_generation(&bad)
                .unwrap_err()
                .contains("rule version")
        );
        let bad = copy("no-meta");
        std::fs::remove_file(bad.join(map_fetch::META_FILE)).unwrap();
        assert!(road_snapshot::load_road_snapshot(&bad.join(map_fetch::SNAPSHOT_FILE)).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 지역에만 있는 연결(차수는 같지만 이웃이 다른 노드)은 같은 원본에서 생길 수 없으므로 오류로 거부한다.
    #[test]
    fn local_only_connection_is_rejected() {
        let national: Vec<(u64, Vec<u64>)> = vec![(1, vec![1, 2, 3])];
        let local: Vec<(u64, Vec<u64>)> = vec![(1, vec![1, 2]), (9, vec![2, 4])];
        let error = compare_connectivity(
            national.iter().map(|(id, refs)| (*id, refs.as_slice())),
            local.iter().map(|(id, refs)| (*id, refs.as_slice())),
        )
        .unwrap_err();
        assert!(error.contains("node 2"), "{error}");
    }

    /// 선택 way의 좌표가 PBF에 없거나 영역에 도로가 없으면 오류다.
    #[test]
    fn missing_coordinates_and_empty_area_are_errors() {
        let mut fixture = selection_fixture(true);
        fixture.nodes.retain(|node| node.0 != 3);
        let pbf = fixture.to_pbf();
        let error = extract_region(
            &read(&pbf).unwrap(),
            &source(&pbf),
            &square(100.0),
            0.0,
            &allowed(),
        )
        .unwrap_err();
        assert!(error.contains("node 3"), "{error}");
        let mut far = Fixture {
            dense: true,
            ..Fixture::default()
        };
        far.nodes = vec![(1, 900.0, 900.0, None), (2, 950.0, 900.0, None)];
        far.ways = vec![(1, vec![("highway", "service")], vec![1, 2], None)];
        let pbf = far.to_pbf();
        assert!(
            extract_region(
                &read(&pbf).unwrap(),
                &source(&pbf),
                &square(100.0),
                0.0,
                &allowed()
            )
            .is_err()
        );
        assert!(
            extract_region(
                &read(&pbf).unwrap(),
                &source(&pbf),
                &square(100.0),
                -1.0,
                &allowed()
            )
            .is_err()
        );
    }

    /// 잘린 PBF는 끝까지 풀 수 없으므로 오류다.
    #[test]
    fn truncated_pbf_is_rejected() {
        let pbf = selection_fixture(true).to_pbf();
        assert!(read(&pbf[..pbf.len() - 7]).is_err());
    }

    /// 선분-영역 교차: 지나는 선분, 밖의 선분, 모서리를 스치는 선분, 안의 점.
    #[test]
    fn segment_area_cases() {
        let area = Area {
            south: 0.0,
            west: 0.0,
            north: 1.0,
            east: 1.0,
        };
        assert!(area.segment_hits([0.5, -1.0], [0.5, 2.0]));
        assert!(!area.segment_hits([2.0, -1.0], [2.0, 2.0]));
        // (0,2)→(2,0)은 북동 모서리 (1,1)을 지나고, (0,2.5)→(2.5,0)은 (1.25,1.25)를 지나 영역 밖이다.
        assert!(
            area.segment_hits([0.0, 2.0], [2.0, 0.0]),
            "touches the corner"
        );
        assert!(!area.segment_hits([0.0, 2.5], [2.5, 0.0]));
        assert!(area.contains([0.5, 0.5]));
        assert_eq!(fixed7(347_512_345), "34.7512345");
        assert_eq!(fixed7(-5), "-0.0000005");
    }

    /// T자 교차로 자료. 사용자 영역은 반 변 60 m.
    ///
    /// way 110 1(-50,0)–10(0,0)–2(70,0), way 111 10–3(0,-50), way 112 2–4(200,0).
    /// 여유 폭 0이면 way 112가 빠져 노드 2(교차로 10의 동쪽 팔 끝, 70 m)가 불완전하다.
    fn junction_fixture() -> Fixture {
        let node = |id, east, north| (id, east, north, None);
        let way = |id, refs: Vec<u64>| (id, vec![("highway", "service")], refs, None);
        Fixture {
            nodes: vec![
                node(1, -50.0, 0.0),
                node(10, 0.0, 0.0),
                node(2, 70.0, 0.0),
                node(3, 0.0, -50.0),
                node(4, 200.0, 0.0),
            ],
            ways: vec![
                way(110, vec![1, 10, 2]),
                way(111, vec![10, 3]),
                way(112, vec![2, 4]),
            ],
            replication: None,
            dense: true,
            ..Fixture::default()
        }
    }

    /// 합성 기준점에서 (동쪽, 북쪽) m인 점 [위도, 경도].
    fn at(east: f64, north: f64) -> [f64; 2] {
        let point = fixtures::offset(east, north);
        [point.lat, point.lon]
    }

    /// 여유 폭이 작아 교차로 팔 범위에 불완전 노드가 있으면 경로 계획과 재생을 모두 거부하고,
    /// 여유 폭을 늘리면 통과하며, 허용 도로 목록을 바꾼 재생도 거부하는지 확인한다(§3.5).
    #[test]
    fn boundary_check_blocks_planning_and_replay() {
        let pbf = junction_fixture().to_pbf();
        let roads = read(&pbf).unwrap();
        let config = RoadGraphConfig::default();
        let root = temp_dir("junction");
        let small = extract_region(&roads, &source(&pbf), &square(60.0), 0.0, &allowed()).unwrap();
        assert_eq!(
            small.connectivity.incomplete_ids(),
            [2].into_iter().collect()
        );
        let large =
            extract_region(&roads, &source(&pbf), &square(60.0), 150.0, &allowed()).unwrap();
        assert!(large.connectivity.incomplete.is_empty());
        let (small_dir, large_dir) = (
            write_generation(&root.join("small"), &small).unwrap(),
            write_generation(&root.join("large"), &large).unwrap(),
        );
        let load = |dir: &Path| {
            road_snapshot::load_road_snapshot(&dir.join(map_fetch::SNAPSHOT_FILE)).unwrap()
        };
        let (small_snap, large_snap) = (load(&small_dir), load(&large_dir));
        let points = [at(-40.0, 2.0), at(1.5, -40.0)];

        let error = route_plan::plan_route_detailed(
            &small_snap.ways,
            small_snap.connectivity.as_ref(),
            "graph.osm.json",
            &points,
            &config,
        )
        .unwrap_err();
        assert!(error.contains("node 2") && error.contains("112"), "{error}");
        // 교차로를 지나지 않는 남쪽 팔 경로는 불완전 노드와 무관하므로 통과한다.
        assert!(
            route_plan::plan_route_detailed(
                &small_snap.ways,
                small_snap.connectivity.as_ref(),
                "graph.osm.json",
                &[at(1.0, -12.0), at(1.0, -45.0)],
                &config,
            )
            .is_ok()
        );
        let planned = route_plan::plan_route_detailed(
            &large_snap.ways,
            large_snap.connectivity.as_ref(),
            "graph.osm.json",
            &points,
            &config,
        )
        .unwrap();
        assert_eq!(planned.course.junctions, vec![10]);

        // 같은 경로 파일을 여유 폭 0 세대로 재생하면(검사 우회 없이) 거부한다.
        assert!(route_plan::load_route_course(&planned.file, &large_dir).is_ok());
        let error = route_plan::load_route_course(&planned.file, &small_dir).unwrap_err();
        assert!(error.contains("node 2"), "{error}");
        // 허용 도로 목록을 바꾼 경로 파일은 연결 정보와 맞지 않으므로 거부한다.
        let mut edited = planned.file.clone();
        edited.allowed_highways.retain(|kind| kind != "pedestrian");
        let error = route_plan::load_route_course(&edited, &large_dir).unwrap_err();
        assert!(error.contains("allowed highways"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
