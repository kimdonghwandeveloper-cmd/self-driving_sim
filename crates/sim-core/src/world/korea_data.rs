//! 전국 지도 자료(PLAN 4-11 1단계).
//!
//! Geofabrik 전국 PBF를 한 번 변환해 [`KOREA_DATA_DIR`]에 둔다(저장소 제외, 실행 파일과 함께 배포). 켤 때 네트워크로
//! 지역 도로를 받지 않고 여기서 바로 연다.
//!
//! - `manifest.toml`: 형식·원본 PBF·자료 시각·묶음 파일 크기와 SHA-256.
//! - `roads.pack`: 0.1° 조각별 도로 way(노드 ID·좌표·남긴 태그)와 회전 제한 관계. way는 자기 범위가 걸친 모든 조각에 들어간다.
//! - `places.pack`: 0.1° 조각별 건물·이름 있는 장소(윤곽·출입구). 장소는 가운데가 있는 조각에 들어간다.
//!
//! 묶음 파일은 머리 [`PACK_MAGIC`], 조각 수(u32), 조각 목록(위도 칸 i32, 경도 칸 i32, 위치 u64, 길이 u32; 모두 little-endian,
//! 칸 오름차순), 조각마다 deflate로 압축한 본문 순서다. 본문 정수는 LEB128 varint(부호 있는 값은 zigzag)이고, 좌표는
//! 앞 좌표와의 1e-7° 정수 차이다(조각 첫 좌표는 조각 남서 모서리 기준). 같은 입력이면 같은 바이트다.
//!
//! 지역을 열 때([`KoreaData::region_snapshot`])는 범위를 덮는 조각만 풀어, 범위와 겹치는 way 전체(범위 밖 노드 포함,
//! Overpass `way(bbox)`와 같은 규칙)와 회전 제한을 지금 스냅샷 형식(`graph.osm.json`)으로 [`REGION_CACHE_DIR`]에 쓴다.
//! 경로 파일은 이 스냅샷을 가리키므로 자료를 새로 만들어도 저장한 경로는 그대로 재생된다.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use osmpbf::{Element, RelMemberType};
use serde::{Deserialize, Serialize};

use super::map::LatLon;
use super::map_fetch::Bbox;
use super::pbf_extract::{self, Area, PbfSource, fixed7};
use super::places::{self, Place};

/// 전국 자료 폴더(실행 폴더 기준, 저장소가 추적하지 않음).
pub const KOREA_DATA_DIR: &str = "data/korea";
/// 지역 스냅샷을 쓰는 캐시 폴더. 자료 판마다 하위 폴더를 둔다.
pub const REGION_CACHE_DIR: &str = "cache/regions";
/// 자료 형식 이름. 바뀌면 읽지 않는다.
pub const FORMAT: &str = "korea-data-v1";
/// 지역 스냅샷 본문의 `generator` 값(Geofabrik 추출 접두어와 다르다: 연결 정보 없는 Overpass 형식으로 읽힌다).
pub const GENERATOR: &str = "self-driving-sim korea-data v1";
/// 머리 정보 파일.
pub const MANIFEST_FILE: &str = "manifest.toml";
/// 도로 묶음 파일.
pub const ROADS_PACK: &str = "roads.pack";
/// 건물·장소 묶음 파일.
pub const PLACES_PACK: &str = "places.pack";
/// 묶음 파일 머리.
pub const PACK_MAGIC: &[u8; 8] = b"SDSKPK1\n";
/// 조각 크기(1e-7° 단위, 0.1° ≈ 남북 11 km × 동서 9 km).
pub const TILE_E7: i64 = 1_000_000;
/// 넣는 도로 종류(`highway`). 허용 목록(`config/road_graph.toml`)보다 넓게 넣고, 탐색 여부는 그 설정이 정한다.
pub const ROAD_KINDS: [&str; 17] = [
    "service",
    "pedestrian",
    "residential",
    "unclassified",
    "tertiary",
    "tertiary_link",
    "secondary",
    "secondary_link",
    "primary",
    "primary_link",
    "trunk",
    "trunk_link",
    "motorway",
    "motorway_link",
    "living_street",
    "road",
    "busway",
];
/// 넣지 않는 `service` 값(주차장 통로·진입로, 사용자 동의 2026-11-12).
pub const SKIPPED_SERVICE: [&str; 3] = ["parking_aisle", "driveway", "drive-through"];
/// 도로에 남기는 태그. 순서가 형식의 일부다(번호로 저장).
pub const ROAD_KEYS: [&str; 15] = [
    "highway",
    "name",
    "oneway",
    "junction",
    "lanes",
    "width",
    "layer",
    "bridge",
    "tunnel",
    "service",
    "access",
    "motor_vehicle",
    "maxspeed",
    "area",
    "incline",
];
/// 회전 제한 관계에 남기는 태그. 순서가 형식의 일부다.
pub const RESTRICTION_KEYS: [&str; 4] = ["type", "restriction", "restriction:motorcar", "except"];

/// 원본 PBF 기록.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// 원본 파일 이름(로컬 경로는 쓰지 않는다).
    pub pbf_file_name: String,
    /// 원본 SHA-256.
    pub pbf_sha256: String,
    /// 원본 크기(바이트).
    pub pbf_bytes: u64,
    /// PBF 머리부 복제 시각(유닉스 초). 없으면 생략.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replication_timestamp: Option<i64>,
    /// 일부 범위만 변환했으면 그 범위(`남,서,북,동`). 시험 자료용.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
}

/// 묶음 파일 기록.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRecord {
    /// 파일 이름.
    pub file: String,
    /// 크기(바이트).
    pub bytes: u64,
    /// SHA-256.
    pub sha256: String,
    /// 조각 수.
    pub tiles: u64,
}

/// 개수 기록.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    /// 도로 way.
    pub roads: u64,
    /// 회전 제한.
    pub restrictions: u64,
    /// 건물·장소 전체.
    pub places: u64,
    /// 그중 건물.
    pub buildings: u64,
    /// 좌표가 PBF에 없어 뺀 way.
    pub skipped_ways: u64,
}

/// `manifest.toml` 내용.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// 형식([`FORMAT`]).
    pub format: String,
    /// 만든 프로그램.
    pub generator: String,
    /// 조각 크기(1e-7°).
    pub tile_e7: i64,
    /// 원본.
    pub source: SourceRecord,
    /// 개수.
    pub counts: Counts,
    /// 도로 묶음.
    pub roads: PackRecord,
    /// 건물·장소 묶음.
    pub places: PackRecord,
}

/// (태그 번호, 값) 목록. 번호는 [`ROAD_KEYS`] 또는 [`RESTRICTION_KEYS`]의 위치다.
type Tags = Vec<(u8, String)>;
/// 도로 조각 하나에 들어갈 way·회전 제한.
type RoadTile<'a> = (Vec<&'a Road>, Vec<&'a Restriction>);

/// 도로 way(좌표는 1e-7° 정수).
#[derive(Debug, Clone, PartialEq)]
struct Road {
    id: u64,
    /// (태그 번호([`ROAD_KEYS`]), 값). 번호 오름차순.
    tags: Vec<(u8, String)>,
    nodes: Vec<(u64, [i32; 2])>,
}

/// 회전 제한 관계(from way, via 노드, to way 하나씩인 것만).
#[derive(Debug, Clone, PartialEq)]
struct Restriction {
    id: u64,
    from: u64,
    via: u64,
    to: u64,
    /// (태그 번호([`RESTRICTION_KEYS`]), 값). 번호 오름차순.
    tags: Vec<(u8, String)>,
}

/// 건물·장소(좌표는 1e-7° 정수).
#[derive(Debug, Clone, PartialEq)]
struct PlaceRecord {
    /// way면 `true`(윤곽 있음), 노드면 `false`.
    way: bool,
    id: u64,
    name: String,
    kind: String,
    /// 윤곽(닫는 점 중복 없음). 노드면 그 점 하나.
    outline: Vec<[i32; 2]>,
    /// 윤곽 위 출입구.
    entrances: Vec<[i32; 2]>,
}

impl PlaceRecord {
    /// 윤곽 꼭짓점의 정수 평균(조각 배정용).
    fn anchor(&self) -> [i32; 2] {
        let n = self.outline.len().max(1) as i64;
        let sum = |i: usize| self.outline.iter().map(|p| i64::from(p[i])).sum::<i64>() / n;
        [sum(0) as i32, sum(1) as i32]
    }

    /// [`places::parse_places`]와 같은 규칙의 [`Place`]로 바꾼다.
    fn to_place(&self) -> Place {
        let outline: Vec<LatLon> = self.outline.iter().map(|&p| latlon(p)).collect();
        let count = outline.len() as f64;
        let center = LatLon {
            lat: outline.iter().map(|p| p.lat).sum::<f64>() / count,
            lon: outline.iter().map(|p| p.lon).sum::<f64>() / count,
        };
        Place {
            id: format!("{}/{}", if self.way { "way" } else { "node" }, self.id),
            name: self.name.clone(),
            kind: self.kind.clone(),
            outline: if self.way { outline } else { Vec::new() },
            center,
            entrances: self.entrances.iter().map(|&p| latlon(p)).collect(),
        }
    }
}

/// 1e-7° 정수 좌표를 위경도로 바꾼다.
fn latlon([lat, lon]: [i32; 2]) -> LatLon {
    LatLon {
        lat: f64::from(lat) / 1e7,
        lon: f64::from(lon) / 1e7,
    }
}

/// 좌표가 속한 조각 칸.
fn tile_of([lat, lon]: [i32; 2]) -> (i32, i32) {
    (
        i64::from(lat).div_euclid(TILE_E7) as i32,
        i64::from(lon).div_euclid(TILE_E7) as i32,
    )
}

/// 조각 남서 모서리(1e-7°).
fn tile_origin((lat, lon): (i32, i32)) -> [i64; 2] {
    [i64::from(lat) * TILE_E7, i64::from(lon) * TILE_E7]
}

/// 도 단위 위경도를 1e-7° 정수로 바꾼다(조각 칸 계산용).
fn e7(degrees: f64) -> i32 {
    (degrees * 1e7).floor() as i32
}

/// 범위를 덮는 조각 칸 목록(오름차순).
fn tiles_covering(bbox: &Bbox) -> Vec<(i32, i32)> {
    let (south, west) = tile_of([e7(bbox.south), e7(bbox.west)]);
    let (north, east) = tile_of([e7(bbox.north), e7(bbox.east)]);
    (south..=north)
        .flat_map(|lat| (west..=east).map(move |lon| (lat, lon)))
        .collect()
}

/// 1e-7° 정수 좌표를 [`Area`] 검사용 도 단위로 바꾼다.
fn degrees([lat, lon]: [i32; 2]) -> [f64; 2] {
    [f64::from(lat) / 1e7, f64::from(lon) / 1e7]
}

/// 점열이 영역과 겹치는지(점이 안에 있거나 선분이 지나감).
fn touches(area: &Area, points: &[[i32; 2]]) -> bool {
    points.iter().any(|&p| area.contains(degrees(p)))
        || points
            .windows(2)
            .any(|pair| area.segment_hits(degrees(pair[0]), degrees(pair[1])))
}

/// 조각 본문을 쓰는 버퍼.
#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
    /// 앞 좌표(조각 첫 좌표는 조각 모서리).
    previous: [i64; 2],
}

impl Encoder {
    /// 조각 모서리를 기준으로 새 버퍼를 만든다.
    fn new(tile: (i32, i32)) -> Self {
        Self {
            bytes: Vec::new(),
            previous: tile_origin(tile),
        }
    }

    /// 부호 없는 varint를 붙인다.
    fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.bytes.push((value as u8) | 0x80);
            value >>= 7;
        }
        self.bytes.push(value as u8);
    }

    /// 부호 있는 값을 zigzag varint로 붙인다.
    fn svarint(&mut self, value: i64) {
        self.varint(((value << 1) ^ (value >> 63)) as u64);
    }

    /// 길이를 앞에 붙인 문자열을 붙인다.
    fn text(&mut self, value: &str) {
        self.varint(value.len() as u64);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    /// 좌표를 앞 좌표와의 차이로 붙인다.
    fn coord(&mut self, [lat, lon]: [i32; 2]) {
        let current = [i64::from(lat), i64::from(lon)];
        self.svarint(current[0] - self.previous[0]);
        self.svarint(current[1] - self.previous[1]);
        self.previous = current;
    }

    /// (번호, 값) 태그 목록을 붙인다.
    fn tags(&mut self, tags: &[(u8, String)]) {
        self.varint(tags.len() as u64);
        for (key, value) in tags {
            self.bytes.push(*key);
            self.text(value);
        }
    }
}

/// 조각 본문을 읽는 커서. 끝을 넘거나 값이 잘못되면 오류다.
struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
    previous: [i64; 2],
}

impl<'a> Decoder<'a> {
    /// 조각 모서리를 기준으로 읽기 시작한다.
    fn new(bytes: &'a [u8], tile: (i32, i32)) -> Self {
        Self {
            bytes,
            at: 0,
            previous: tile_origin(tile),
        }
    }

    /// 바이트 하나를 읽는다.
    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or("korea data tile ends early")?;
        self.at += 1;
        Ok(byte)
    }

    /// 부호 없는 varint를 읽는다.
    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("korea data varint is too long".to_owned())
    }

    /// zigzag varint를 읽는다.
    fn svarint(&mut self) -> Result<i64, String> {
        let raw = self.varint()?;
        Ok(((raw >> 1) as i64) ^ -((raw & 1) as i64))
    }

    /// 개수를 읽는다(남은 바이트보다 많으면 잘못된 자료).
    fn count(&mut self) -> Result<usize, String> {
        let count = self.varint()?;
        usize::try_from(count)
            .ok()
            .filter(|&count| count <= self.bytes.len() - self.at)
            .ok_or_else(|| format!("korea data count {count} is out of range"))
    }

    /// 문자열을 읽는다.
    fn text(&mut self) -> Result<String, String> {
        let length = self.count()?;
        let slice = &self.bytes[self.at..self.at + length];
        self.at += length;
        String::from_utf8(slice.to_vec()).map_err(|_| "korea data text is not UTF-8".to_owned())
    }

    /// 앞 좌표와의 차이로 쓴 좌표를 읽는다.
    fn coord(&mut self) -> Result<[i32; 2], String> {
        let lat = self.previous[0] + self.svarint()?;
        let lon = self.previous[1] + self.svarint()?;
        self.previous = [lat, lon];
        match (i32::try_from(lat), i32::try_from(lon)) {
            (Ok(lat), Ok(lon)) => Ok([lat, lon]),
            _ => Err("korea data coordinate is out of range".to_owned()),
        }
    }

    /// (번호, 값) 태그 목록을 읽는다. 번호가 `limit` 이상이면 오류.
    fn tags(&mut self, limit: usize) -> Result<Vec<(u8, String)>, String> {
        let count = self.count()?;
        (0..count)
            .map(|_| {
                let key = self.byte()?;
                if usize::from(key) >= limit {
                    return Err(format!("korea data tag key {key} is unknown"));
                }
                Ok((key, self.text()?))
            })
            .collect()
    }

    /// 모두 읽었는지 확인한다.
    fn finish(&self) -> Result<(), String> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err("korea data tile has trailing bytes".to_owned())
        }
    }
}

/// 도로 조각 본문을 만든다(way ID 오름차순, 회전 제한 ID 오름차순).
fn encode_roads(tile: (i32, i32), roads: &[&Road], restrictions: &[&Restriction]) -> Vec<u8> {
    let mut out = Encoder::new(tile);
    out.varint(roads.len() as u64);
    let (mut previous_id, mut previous_node) = (0_u64, 0_i64);
    for road in roads {
        out.varint(road.id - previous_id);
        previous_id = road.id;
        out.tags(&road.tags);
        out.varint(road.nodes.len() as u64);
        for &(id, coord) in &road.nodes {
            out.svarint(id as i64 - previous_node);
            previous_node = id as i64;
            out.coord(coord);
        }
    }
    out.varint(restrictions.len() as u64);
    let mut previous_id = 0_u64;
    for restriction in restrictions {
        out.varint(restriction.id - previous_id);
        previous_id = restriction.id;
        out.varint(restriction.from);
        out.varint(restriction.via);
        out.varint(restriction.to);
        out.tags(&restriction.tags);
    }
    out.bytes
}

/// 도로 조각 본문을 읽는다.
fn decode_roads(tile: (i32, i32), bytes: &[u8]) -> Result<(Vec<Road>, Vec<Restriction>), String> {
    let mut input = Decoder::new(bytes, tile);
    let count = input.count()?;
    let mut roads = Vec::with_capacity(count);
    let (mut previous_id, mut previous_node) = (0_u64, 0_i64);
    for _ in 0..count {
        let id = previous_id + input.varint()?;
        previous_id = id;
        let tags = input.tags(ROAD_KEYS.len())?;
        let length = input.count()?;
        let mut nodes = Vec::with_capacity(length);
        for _ in 0..length {
            let node = previous_node + input.svarint()?;
            previous_node = node;
            let node = u64::try_from(node).map_err(|_| "korea data node ID is negative")?;
            nodes.push((node, input.coord()?));
        }
        roads.push(Road { id, tags, nodes });
    }
    let count = input.count()?;
    let mut restrictions = Vec::with_capacity(count);
    let mut previous_id = 0_u64;
    for _ in 0..count {
        let id = previous_id + input.varint()?;
        previous_id = id;
        restrictions.push(Restriction {
            id,
            from: input.varint()?,
            via: input.varint()?,
            to: input.varint()?,
            tags: input.tags(RESTRICTION_KEYS.len())?,
        });
    }
    input.finish()?;
    Ok((roads, restrictions))
}

/// 건물·장소 조각 본문을 만든다((way 여부, ID) 오름차순).
fn encode_places(tile: (i32, i32), places: &[&PlaceRecord]) -> Vec<u8> {
    let mut out = Encoder::new(tile);
    out.varint(places.len() as u64);
    for place in places {
        out.bytes.push(u8::from(place.way));
        out.varint(place.id);
        out.text(&place.name);
        out.text(&place.kind);
        out.varint(place.outline.len() as u64);
        for &point in &place.outline {
            out.coord(point);
        }
        out.varint(place.entrances.len() as u64);
        for &point in &place.entrances {
            out.coord(point);
        }
    }
    out.bytes
}

/// 건물·장소 조각 본문을 읽는다.
fn decode_places(tile: (i32, i32), bytes: &[u8]) -> Result<Vec<PlaceRecord>, String> {
    let mut input = Decoder::new(bytes, tile);
    let count = input.count()?;
    let mut places = Vec::with_capacity(count);
    for _ in 0..count {
        let way = match input.byte()? {
            0 => false,
            1 => true,
            other => return Err(format!("korea data place type {other} is unknown")),
        };
        let id = input.varint()?;
        let name = input.text()?;
        let kind = input.text()?;
        let outline = (0..input.count()?)
            .map(|_| input.coord())
            .collect::<Result<Vec<_>, _>>()?;
        let entrances = (0..input.count()?)
            .map(|_| input.coord())
            .collect::<Result<Vec<_>, _>>()?;
        if outline.is_empty() {
            return Err(format!("korea data place {id} has no coordinates"));
        }
        places.push(PlaceRecord {
            way,
            id,
            name,
            kind,
            outline,
            entrances,
        });
    }
    input.finish()?;
    Ok(places)
}

/// 바이트를 deflate(최고 압축)로 줄인다.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

/// deflate 본문을 푼다.
fn inflate(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|error| format!("korea data tile cannot be decompressed: {error}"))?;
    Ok(out)
}

/// 바이트의 SHA-256(소문자 16진).
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 조각별 본문으로 묶음 파일 바이트를 만든다(칸 오름차순).
fn pack(tiles: &BTreeMap<(i32, i32), Vec<u8>>) -> Vec<u8> {
    let compressed: Vec<((i32, i32), Vec<u8>)> = tiles
        .iter()
        .map(|(&key, body)| (key, deflate(body)))
        .collect();
    let header = PACK_MAGIC.len() + 4 + compressed.len() * 20;
    let mut out = Vec::with_capacity(header);
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    let mut offset = header as u64;
    for ((lat, lon), body) in &compressed {
        out.extend_from_slice(&lat.to_le_bytes());
        out.extend_from_slice(&lon.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        offset += body.len() as u64;
    }
    for (_, body) in &compressed {
        out.extend_from_slice(body);
    }
    out
}

/// 열린 묶음 파일(조각 목록만 읽어 둔다).
#[derive(Debug, Clone)]
struct Pack {
    path: PathBuf,
    /// 칸 → (위치, 길이).
    index: BTreeMap<(i32, i32), (u64, u32)>,
}

impl Pack {
    /// 묶음 파일의 머리와 조각 목록을 읽고 크기를 기록과 대조한다.
    fn open(path: PathBuf, record: &PackRecord) -> Result<Self, String> {
        let mut file = std::fs::File::open(&path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let bytes = file
            .metadata()
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?
            .len();
        if bytes != record.bytes {
            return Err(format!(
                "{} is {bytes} bytes but manifest.toml says {}",
                path.display(),
                record.bytes
            ));
        }
        let mut head = [0_u8; 12];
        file.read_exact(&mut head)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if &head[..8] != PACK_MAGIC {
            return Err(format!("{} is not a korea data pack", path.display()));
        }
        let count = u32::from_le_bytes(head[8..12].try_into().expect("4 bytes")) as usize;
        if count as u64 != record.tiles {
            return Err(format!(
                "{} tile count differs from manifest.toml",
                path.display()
            ));
        }
        let mut table = vec![0_u8; count * 20];
        file.read_exact(&mut table)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let mut index = BTreeMap::new();
        let (entries, _) = table.as_chunks::<20>();
        for entry in entries {
            let word = |range: std::ops::Range<usize>| entry[range].to_vec();
            let lat = i32::from_le_bytes(word(0..4).try_into().expect("4 bytes"));
            let lon = i32::from_le_bytes(word(4..8).try_into().expect("4 bytes"));
            let offset = u64::from_le_bytes(word(8..16).try_into().expect("8 bytes"));
            let length = u32::from_le_bytes(word(16..20).try_into().expect("4 bytes"));
            if offset + u64::from(length) > bytes {
                return Err(format!("{} has a tile past the end", path.display()));
            }
            index.insert((lat, lon), (offset, length));
        }
        Ok(Self { path, index })
    }

    /// 칸의 본문을 풀어 돌려준다. 그 칸에 자료가 없으면 `None`.
    fn tile(&self, key: (i32, i32)) -> Result<Option<Vec<u8>>, String> {
        let Some(&(offset, length)) = self.index.get(&key) else {
            return Ok(None);
        };
        let mut file = std::fs::File::open(&self.path)
            .map_err(|error| format!("cannot open {}: {error}", self.path.display()))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("cannot read {}: {error}", self.path.display()))?;
        let mut body = vec![0_u8; length as usize];
        file.read_exact(&mut body)
            .map_err(|error| format!("cannot read {}: {error}", self.path.display()))?;
        inflate(&body).map(Some)
    }
}

/// 한 지역에서 꺼낸 자료.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// 지금 스냅샷 형식 본문(`graph.osm.json`, 도로·회전 제한).
    pub snapshot: String,
    /// 도로 way 수.
    pub ways: usize,
    /// 건물·장소([`places::parse_places`]와 같은 규칙·순서).
    pub places: Vec<Place>,
}

/// 열린 전국 자료.
#[derive(Debug, Clone)]
pub struct KoreaData {
    dir: PathBuf,
    /// 머리 정보.
    pub manifest: Manifest,
    roads: Pack,
    places: Pack,
}

impl KoreaData {
    /// 자료 폴더를 연다. 머리 정보 형식·조각 크기·묶음 파일 크기·조각 수가 맞지 않으면 오류다(내용 해시는
    /// [`KoreaData::verify`]에서 따로 검사한다).
    pub fn open(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let manifest: Manifest = toml::from_str(&text)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        if manifest.format != FORMAT {
            return Err(format!(
                "{} has format `{}`; this program reads `{FORMAT}` (rebuild the data)",
                path.display(),
                manifest.format
            ));
        }
        if manifest.tile_e7 != TILE_E7 {
            return Err(format!("{} has an unknown tile size", path.display()));
        }
        let roads = Pack::open(dir.join(&manifest.roads.file), &manifest.roads)?;
        let places = Pack::open(dir.join(&manifest.places.file), &manifest.places)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            manifest,
            roads,
            places,
        })
    }

    /// 묶음 파일 내용 해시를 머리 정보와 대조한다(배포 확인용, 수백 ms 걸린다).
    pub fn verify(&self) -> Result<(), String> {
        for record in [&self.manifest.roads, &self.manifest.places] {
            let path = self.dir.join(&record.file);
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            if sha256(&bytes) != record.sha256 {
                return Err(format!(
                    "{} does not match manifest.toml (SHA-256)",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// 자료 판 이름(도로 묶음 SHA-256 앞 12자). 지역 스냅샷 캐시 폴더 이름으로 쓴다.
    pub fn version(&self) -> &str {
        &self.manifest.roads.sha256[..12.min(self.manifest.roads.sha256.len())]
    }

    /// 범위와 겹치는 도로·회전 제한·건물·장소를 꺼낸다. 도로가 하나도 없으면 오류다.
    pub fn region(&self, bbox: &Bbox) -> Result<Region, String> {
        bbox.validate()?;
        let area = Area::around(bbox, 0.0);
        let tiles = tiles_covering(bbox);
        let mut roads: BTreeMap<u64, Road> = BTreeMap::new();
        let mut restrictions: BTreeMap<u64, Restriction> = BTreeMap::new();
        let mut found: BTreeMap<(bool, u64), PlaceRecord> = BTreeMap::new();
        for &tile in &tiles {
            if let Some(body) = self.roads.tile(tile)? {
                let (tile_roads, tile_restrictions) = decode_roads(tile, &body)?;
                for road in tile_roads {
                    let points: Vec<[i32; 2]> = road.nodes.iter().map(|n| n.1).collect();
                    if touches(&area, &points) {
                        roads.insert(road.id, road);
                    }
                }
                for restriction in tile_restrictions {
                    restrictions.insert(restriction.id, restriction);
                }
            }
            if let Some(body) = self.places.tile(tile)? {
                for place in decode_places(tile, &body)? {
                    if area.contains(degrees(place.anchor())) || touches(&area, &place.outline) {
                        found.insert((place.way, place.id), place);
                    }
                }
            }
        }
        if roads.is_empty() {
            return Err("no roads in this area".to_owned());
        }
        let nodes: BTreeSet<u64> = roads
            .values()
            .flat_map(|road| road.nodes.iter().map(|n| n.0))
            .collect();
        let restrictions: Vec<&Restriction> = restrictions
            .values()
            .filter(|r| {
                nodes.contains(&r.via) && roads.contains_key(&r.from) && roads.contains_key(&r.to)
            })
            .collect();
        let road_list: Vec<&Road> = roads.values().collect();
        let mut places: Vec<Place> = found.values().map(PlaceRecord::to_place).collect();
        places.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Region {
            snapshot: snapshot_json(&road_list, &restrictions),
            ways: road_list.len(),
            places,
        })
    }

    /// 범위의 지역 스냅샷을 `cache_root/<자료 판>/<범위 이름>.osm.json`에 쓰고(이미 있으면 그대로 둔다) 경로와 건물·장소를
    /// 돌려준다.
    pub fn region_snapshot(
        &self,
        bbox: &Bbox,
        cache_root: &Path,
    ) -> Result<(PathBuf, Vec<Place>), String> {
        let region = self.region(bbox)?;
        let dir = cache_root.join(self.version());
        let path = dir.join(format!("{}.osm.json", bbox.cache_name()));
        if !path.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
            let staging = dir.join(format!("{}.{}.tmp", bbox.cache_name(), std::process::id()));
            std::fs::write(&staging, region.snapshot.as_bytes())
                .and_then(|()| std::fs::rename(&staging, &path))
                .map_err(|error| {
                    let _ = std::fs::remove_file(&staging);
                    format!("cannot write {}: {error}", path.display())
                })?;
        }
        Ok((path, region.places))
    }
}

/// 도로·회전 제한으로 지금 스냅샷 형식 본문을 만든다(way·관계 ID 오름차순, 태그 키 오름차순).
fn snapshot_json(roads: &[&Road], restrictions: &[&Restriction]) -> String {
    let quote = |text: &str| serde_json::to_string(text).expect("string serializes");
    let tags = |list: &[(u8, String)], keys: &[&str]| {
        let sorted: BTreeMap<&str, &str> = list
            .iter()
            .map(|(key, value)| (keys[usize::from(*key)], value.as_str()))
            .collect();
        sorted
            .iter()
            .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut elements: Vec<String> = roads
        .iter()
        .map(|road| {
            let ids: Vec<String> = road.nodes.iter().map(|(id, _)| id.to_string()).collect();
            let geometry: Vec<String> = road
                .nodes
                .iter()
                .map(|(_, [lat, lon])| {
                    format!("{{\"lat\":{},\"lon\":{}}}", fixed7(*lat), fixed7(*lon))
                })
                .collect();
            format!(
                "{{\"type\":\"way\",\"id\":{},\"nodes\":[{}],\"geometry\":[{}],\"tags\":{{{}}}}}",
                road.id,
                ids.join(","),
                geometry.join(","),
                tags(&road.tags, &ROAD_KEYS)
            )
        })
        .collect();
    elements.extend(restrictions.iter().map(|r| {
        format!(
            "{{\"type\":\"relation\",\"id\":{},\"members\":[{{\"type\":\"way\",\"ref\":{},\"role\":\"from\"}},{{\"type\":\"node\",\"ref\":{},\"role\":\"via\"}},{{\"type\":\"way\",\"ref\":{},\"role\":\"to\"}}],\"tags\":{{{}}}}}",
            r.id,
            r.from,
            r.via,
            r.to,
            tags(&r.tags, &RESTRICTION_KEYS)
        )
    }));
    format!(
        "{{\"generator\":{},\"elements\":[{}]}}\n",
        quote(GENERATOR),
        elements.join(",")
    )
}

/// 변환 보고.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildReport {
    /// 쓴 머리 정보.
    pub manifest: Manifest,
    /// 단계별 시간(초).
    pub timings: Vec<(&'static str, f64)>,
}

/// 1차 읽기에서 모은 건물·장소 way(좌표 전).
struct PlaceWay {
    id: u64,
    name: String,
    kind: String,
    refs: Vec<u64>,
    building: bool,
}

/// 태그 목록에서 [`ROAD_KEYS`]·[`RESTRICTION_KEYS`] 같은 키 목록에 든 것만 (번호, 값)으로 남긴다.
fn keep_tags<'a>(
    tags: impl Iterator<Item = (&'a str, &'a str)>,
    keys: &[&str],
) -> Vec<(u8, String)> {
    let mut kept: Vec<(u8, String)> = tags
        .filter_map(|(key, value)| {
            keys.iter()
                .position(|known| *known == key)
                .map(|index| (index as u8, value.to_owned()))
        })
        .collect();
    kept.sort();
    kept.dedup_by_key(|(key, _)| *key);
    kept
}

/// 원본 PBF를 두 번 읽어 전국 자료를 `out_dir`에 쓴다. `open`은 같은 파일을 처음부터 읽는 리더를 만든다.
/// `clip`을 주면 그 범위와 겹치는 것만 넣는다(시험 자료용). 다 만든 뒤 기존 폴더를 바꾼다.
pub fn build<R, F>(
    open: F,
    source: &PbfSource,
    out_dir: &Path,
    clip: Option<&Bbox>,
) -> Result<BuildReport, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let started = Instant::now();
    let mut road_ways: Vec<(u64, Tags, Vec<u64>)> = Vec::new();
    let mut place_ways: Vec<PlaceWay> = Vec::new();
    let mut relations: Vec<Restriction> = Vec::new();
    let positive = |id: i64| u64::try_from(id).ok().filter(|&id| id > 0);
    let header = pbf_extract::scan(open()?, |element| {
        match element {
            Element::Way(way) => {
                let Some(id) = positive(way.id()) else {
                    return Ok(());
                };
                let lookup = |key: &str| way.tags().find(|(k, _)| *k == key).map(|(_, v)| v);
                let refs: Vec<u64> = way.refs().filter_map(positive).collect();
                if lookup("highway").is_some_and(|kind| ROAD_KINDS.contains(&kind))
                    && !lookup("service").is_some_and(|value| SKIPPED_SERVICE.contains(&value))
                {
                    road_ways.push((id, keep_tags(way.tags(), &ROAD_KEYS), refs));
                    return Ok(());
                }
                let name = lookup("name").unwrap_or_default();
                let building = lookup("building").is_some();
                let closed = refs.len() >= 4 && refs.first() == refs.last();
                if closed && lookup("highway").is_none() && (building || !name.is_empty()) {
                    let tags: BTreeMap<String, String> = way
                        .tags()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect();
                    place_ways.push(PlaceWay {
                        id,
                        name: name.to_owned(),
                        kind: places::place_kind(&tags),
                        refs,
                        building,
                    });
                }
            }
            Element::Relation(relation) => {
                let Some(id) = positive(relation.id()) else {
                    return Ok(());
                };
                if !relation
                    .tags()
                    .any(|(k, v)| k == "type" && v == "restriction")
                {
                    return Ok(());
                }
                let mut from = Vec::new();
                let mut via = Vec::new();
                let mut to = Vec::new();
                for member in relation.members() {
                    let target = match (member.role().unwrap_or(""), member.member_type) {
                        ("from", RelMemberType::Way) => &mut from,
                        ("via", RelMemberType::Node) => &mut via,
                        ("to", RelMemberType::Way) => &mut to,
                        ("from" | "via" | "to", _) => return Ok(()),
                        _ => continue,
                    };
                    target.extend(positive(member.member_id));
                }
                if let ([from], [via], [to]) = (&from[..], &via[..], &to[..]) {
                    relations.push(Restriction {
                        id,
                        from: *from,
                        via: *via,
                        to: *to,
                        tags: keep_tags(relation.tags(), &RESTRICTION_KEYS),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    let after_ways = started.elapsed().as_secs_f64();

    let mut node_ids: Vec<u64> = road_ways
        .iter()
        .flat_map(|way| way.2.iter().copied())
        .chain(place_ways.iter().flat_map(|way| way.refs.iter().copied()))
        .collect();
    node_ids.sort_unstable();
    node_ids.dedup();
    let mut coords: Vec<Option<[i32; 2]>> = vec![None; node_ids.len()];
    let mut entrances: BTreeMap<u64, [i32; 2]> = BTreeMap::new();
    let mut node_places: Vec<PlaceRecord> = Vec::new();
    let mut visit_node = |id: i64, at: [i32; 2], map: BTreeMap<String, String>| {
        let Some(id) = positive(id) else {
            return;
        };
        if let Ok(index) = node_ids.binary_search(&id) {
            coords[index] = Some(at);
        }
        if map.is_empty() {
            return;
        }
        if map.contains_key("entrance") {
            entrances.insert(id, at);
        }
        let name = map.get("name").cloned().unwrap_or_default();
        if !name.is_empty() && !map.contains_key("highway") {
            node_places.push(PlaceRecord {
                way: false,
                id,
                name,
                kind: places::place_kind(&map),
                outline: vec![at],
                entrances: Vec::new(),
            });
        }
    };
    pbf_extract::scan(open()?, |element| {
        match element {
            Element::Node(node) => visit_node(
                node.id(),
                [node.decimicro_lat(), node.decimicro_lon()],
                node.tags()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect(),
            ),
            Element::DenseNode(node) => visit_node(
                node.id(),
                [node.decimicro_lat(), node.decimicro_lon()],
                node.tags()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect(),
            ),
            _ => {}
        }
        Ok(())
    })?;
    let after_nodes = started.elapsed().as_secs_f64();

    let coord = |id: u64| {
        node_ids
            .binary_search(&id)
            .ok()
            .and_then(|index| coords[index])
    };
    let clip_area = clip.map(|bbox| Area::around(bbox, 0.0));
    let wanted = |points: &[[i32; 2]]| clip_area.as_ref().is_none_or(|area| touches(area, points));
    let mut skipped = 0_u64;
    let mut roads: Vec<Road> = Vec::new();
    for (id, tags, refs) in road_ways {
        let nodes: Option<Vec<(u64, [i32; 2])>> =
            refs.iter().map(|&r| coord(r).map(|c| (r, c))).collect();
        match nodes {
            Some(nodes) if nodes.len() >= 2 => {
                let points: Vec<[i32; 2]> = nodes.iter().map(|n| n.1).collect();
                if wanted(&points) {
                    roads.push(Road { id, tags, nodes });
                }
            }
            _ => skipped += 1,
        }
    }
    roads.sort_by_key(|road| road.id);
    let road_nodes: BTreeMap<u64, [i32; 2]> = roads
        .iter()
        .flat_map(|road| road.nodes.iter().copied())
        .collect();
    let road_ids: BTreeSet<u64> = roads.iter().map(|road| road.id).collect();
    let mut restrictions: Vec<(Restriction, [i32; 2])> = relations
        .into_iter()
        .filter(|r| road_ids.contains(&r.from) && road_ids.contains(&r.to))
        .filter_map(|r| road_nodes.get(&r.via).copied().map(|at| (r, at)))
        .collect();
    restrictions.sort_by_key(|(r, _)| r.id);
    let mut place_records: Vec<PlaceRecord> = Vec::new();
    let mut buildings = 0_u64;
    for way in place_ways {
        let Some(mut outline) = way
            .refs
            .iter()
            .map(|&r| coord(r))
            .collect::<Option<Vec<_>>>()
        else {
            skipped += 1;
            continue;
        };
        outline.pop();
        let doors: BTreeSet<u64> = way.refs.iter().copied().collect();
        let record = PlaceRecord {
            way: true,
            id: way.id,
            name: way.name,
            kind: way.kind,
            outline,
            entrances: doors
                .iter()
                .filter_map(|id| entrances.get(id).copied())
                .collect(),
        };
        if wanted(&record.outline) {
            buildings += u64::from(way.building);
            place_records.push(record);
        }
    }
    place_records.extend(node_places.into_iter().filter(|p| wanted(&p.outline)));
    place_records.sort_by_key(|p| (p.way, p.id));

    let mut road_tiles: BTreeMap<(i32, i32), RoadTile> = BTreeMap::new();
    for road in &roads {
        let lats = road.nodes.iter().map(|n| n.1[0]);
        let lons = road.nodes.iter().map(|n| n.1[1]);
        let (south, west) = tile_of([
            lats.clone().min().expect("2+ nodes"),
            lons.clone().min().expect("2+ nodes"),
        ]);
        let (north, east) = tile_of([lats.max().expect("2+ nodes"), lons.max().expect("2+ nodes")]);
        for lat in south..=north {
            for lon in west..=east {
                road_tiles.entry((lat, lon)).or_default().0.push(road);
            }
        }
    }
    for (restriction, at) in &restrictions {
        road_tiles
            .entry(tile_of(*at))
            .or_default()
            .1
            .push(restriction);
    }
    let mut place_tiles: BTreeMap<(i32, i32), Vec<&PlaceRecord>> = BTreeMap::new();
    for place in &place_records {
        place_tiles
            .entry(tile_of(place.anchor()))
            .or_default()
            .push(place);
    }
    let road_bodies: BTreeMap<(i32, i32), Vec<u8>> = road_tiles
        .iter()
        .map(|(&key, (list, rules))| (key, encode_roads(key, list, rules)))
        .collect();
    let place_bodies: BTreeMap<(i32, i32), Vec<u8>> = place_tiles
        .iter()
        .map(|(&key, list)| (key, encode_places(key, list)))
        .collect();
    let road_pack = pack(&road_bodies);
    let place_pack = pack(&place_bodies);
    let after_encode = started.elapsed().as_secs_f64();

    let record = |file: &str, bytes: &[u8], tiles: usize| PackRecord {
        file: file.to_owned(),
        bytes: bytes.len() as u64,
        sha256: sha256(bytes),
        tiles: tiles as u64,
    };
    let manifest = Manifest {
        format: FORMAT.to_owned(),
        generator: GENERATOR.to_owned(),
        tile_e7: TILE_E7,
        source: SourceRecord {
            pbf_file_name: source.file_name.clone(),
            pbf_sha256: source.sha256.clone(),
            pbf_bytes: source.bytes,
            replication_timestamp: header.replication_timestamp,
            clip: clip.map(|b| format!("{},{},{},{}", b.south, b.west, b.north, b.east)),
        },
        counts: Counts {
            roads: roads.len() as u64,
            restrictions: restrictions.len() as u64,
            places: place_records.len() as u64,
            buildings,
            skipped_ways: skipped,
        },
        roads: record(ROADS_PACK, &road_pack, road_bodies.len()),
        places: record(PLACES_PACK, &place_pack, place_bodies.len()),
    };
    let manifest_text = toml::to_string(&manifest)
        .map_err(|error| format!("manifest cannot be written: {error}"))?;
    let staging = out_dir.with_extension(format!("building-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let written = std::fs::create_dir_all(&staging)
        .and_then(|()| std::fs::write(staging.join(ROADS_PACK), &road_pack))
        .and_then(|()| std::fs::write(staging.join(PLACES_PACK), &place_pack))
        .and_then(|()| std::fs::write(staging.join(MANIFEST_FILE), manifest_text))
        .and_then(|()| {
            if out_dir.exists() {
                std::fs::remove_dir_all(out_dir)?;
            }
            std::fs::rename(&staging, out_dir)
        });
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("cannot write {}: {error}", out_dir.display()));
    }
    let total = started.elapsed().as_secs_f64();
    Ok(BuildReport {
        manifest,
        timings: vec![
            ("read ways", after_ways),
            ("read nodes", after_nodes - after_ways),
            ("encode", after_encode - after_nodes),
            ("write", total - after_encode),
        ],
    })
}

/// 로컬 PBF 파일로 전국 자료를 만든다(`--build-korea-data` 명령).
pub fn build_from_file(
    pbf: &Path,
    out_dir: &Path,
    clip: Option<&Bbox>,
) -> Result<BuildReport, String> {
    let source = pbf_extract::hash_file(pbf)?;
    build(
        || {
            std::fs::File::open(pbf)
                .map(std::io::BufReader::new)
                .map_err(|error| format!("cannot open {}: {error}", pbf.display()))
        },
        &source,
        out_dir,
        clip,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::{fixtures, parse_graph_full};
    use crate::world::pbf_fixture::{Fixture, e7 as fx_e7};

    /// 시험 폴더(프로세스·이름별).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sds-korea-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 시험 원본 기록.
    fn source(bytes: &[u8]) -> PbfSource {
        PbfSource {
            file_name: "fixture.osm.pbf".to_owned(),
            sha256: sha256(bytes),
            bytes: bytes.len() as u64,
        }
    }

    /// 합성 PBF로 자료를 만든다.
    fn build_fixture(fixture: &Fixture, dir: &Path, clip: Option<&Bbox>) -> BuildReport {
        let bytes = fixture.to_pbf();
        build(
            || Ok(std::io::Cursor::new(bytes.clone())),
            &source(&bytes),
            dir,
            clip,
        )
        .unwrap()
    }

    /// 기준점 주변 (동쪽, 북쪽) m 범위(반 변 `half_m`).
    fn around(east: f64, north: f64, half_m: f64) -> Bbox {
        let a = fixtures::offset(east - half_m, north - half_m);
        let b = fixtures::offset(east + half_m, north + half_m);
        Bbox {
            south: a.lat,
            west: a.lon,
            north: b.lat,
            east: b.lon,
        }
    }

    /// 도로·주차장 통로·보도·건물(출입구)·이름 있는 점·버스 정류장·회전 제한·다른 관계를 담은 합성 자료.
    /// 기준점(34.75, 127.75)에서 동쪽 4.5 km 근처에 경도 127.8(조각 경계)을 지나는 도로가 있다.
    fn fixture() -> Fixture {
        let node = |id, east, north| (id, east, north, None);
        let way =
            |id, tags: Vec<(&'static str, &'static str)>, refs: Vec<u64>| (id, tags, refs, None);
        Fixture {
            nodes: vec![
                node(1, 0.0, 0.0),
                node(2, 100.0, 0.0),
                node(3, 200.0, 0.0),
                node(4, 100.0, 100.0),
                node(5, 100.0, -100.0),
                node(6, 300.0, 50.0),
                node(7, 0.0, 50.0),
                node(11, 20.0, 20.0),
                node(12, 40.0, 20.0),
                node(13, 40.0, 40.0),
                node(14, 20.0, 40.0),
                node(15, 30.0, 20.0),
                node(16, 60.0, 60.0),
                node(21, 4500.0, 0.0),
                node(22, 4650.0, 0.0),
                node(31, 9000.0, 9000.0),
                node(32, 9100.0, 9000.0),
            ],
            ways: vec![
                way(
                    100,
                    vec![
                        ("highway", "residential"),
                        ("name", "시험로"),
                        ("surface", "asphalt"),
                    ],
                    vec![1, 2, 3],
                ),
                way(
                    101,
                    vec![("highway", "tertiary"), ("oneway", "yes")],
                    vec![4, 2, 5],
                ),
                way(
                    102,
                    vec![("highway", "service"), ("service", "parking_aisle")],
                    vec![3, 6],
                ),
                way(103, vec![("highway", "footway")], vec![1, 7]),
                way(
                    104,
                    vec![("building", "yes"), ("name", "시험관")],
                    vec![11, 15, 12, 13, 14, 11],
                ),
                way(105, vec![("highway", "residential")], vec![21, 22]),
                way(106, vec![("highway", "residential")], vec![31, 32]),
            ],
            node_tags: vec![
                (15, vec![("entrance", "main")]),
                (16, vec![("name", "시험카페"), ("amenity", "cafe")]),
                (7, vec![("name", "정류장"), ("highway", "bus_stop")]),
            ],
            relations: vec![
                (
                    900,
                    vec![
                        ("type", "restriction"),
                        ("restriction", "no_left_turn"),
                        ("note", "x"),
                    ],
                    vec![("way", 100, "from"), ("node", 2, "via"), ("way", 101, "to")],
                ),
                (901, vec![("type", "route")], vec![("way", 100, "")]),
            ],
            dense: true,
            ..Fixture::default()
        }
    }

    /// 변환 → 열기 → 지역 꺼내기: 남긴 태그·좌표·회전 제한·건물 출입구·이름 있는 점이 원본과 같고, 주차장 통로·보도·
    /// 버스 정류장·모르는 태그는 빠진다. 스냅샷 본문은 지금 스냅샷 읽기 규칙으로 읽힌다.
    #[test]
    fn build_and_read_region() {
        let dir = temp_dir("roundtrip");
        let report = build_fixture(&fixture(), &dir, None);
        assert_eq!(report.manifest.counts.roads, 4);
        assert_eq!(report.manifest.counts.restrictions, 1);
        assert_eq!(report.manifest.counts.buildings, 1);
        assert_eq!(report.manifest.counts.places, 2);
        let data = KoreaData::open(&dir).unwrap();
        data.verify().unwrap();
        let region = data.region(&around(100.0, 0.0, 300.0)).unwrap();
        let body = parse_graph_full(&region.snapshot).unwrap();
        assert_eq!(body.generator.as_deref(), Some(GENERATOR));
        let ids: Vec<u64> = body.ways.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![100, 101]);
        let first = &body.ways[0];
        assert_eq!(first.tags.get("name").map(String::as_str), Some("시험로"));
        assert!(!first.tags.contains_key("surface"));
        let [lat, lon] = fx_e7(200.0, 0.0);
        assert_eq!(first.nodes[2].id, 3);
        assert_eq!(first.nodes[2].pos.lat, fixed7(lat).parse::<f64>().unwrap());
        assert_eq!(first.nodes[2].pos.lon, fixed7(lon).parse::<f64>().unwrap());
        assert_eq!(
            body.ways[1].tags.get("oneway").map(String::as_str),
            Some("yes")
        );
        assert_eq!(body.restrictions.len(), 1);
        let rule = &body.restrictions[0];
        assert_eq!(
            (
                rule.id,
                rule.from_way,
                rule.via_node,
                rule.to_way,
                rule.only
            ),
            (900, 100, 2, 101, false)
        );
        let names: Vec<(&str, &str, usize)> = region
            .places
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str(), p.entrances.len()))
            .collect();
        assert_eq!(
            names,
            vec![("node/16", "시험카페", 0), ("way/104", "시험관", 1)]
        );
        assert_eq!(region.places[1].outline.len(), 5);
        assert_eq!(region.places[0].kind, "amenity=cafe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 같은 입력이면 같은 바이트이고, 조각 경계를 지나는 도로는 양쪽 조각에 있어도 한 번만 나온다.
    #[test]
    fn same_input_same_bytes_and_cross_tile_road() {
        let (a, b) = (temp_dir("same-a"), temp_dir("same-b"));
        build_fixture(&fixture(), &a, None);
        build_fixture(&fixture(), &b, None);
        for file in [MANIFEST_FILE, ROADS_PACK, PLACES_PACK] {
            assert_eq!(
                std::fs::read(a.join(file)).unwrap(),
                std::fs::read(b.join(file)).unwrap()
            );
        }
        let data = KoreaData::open(&a).unwrap();
        // 4.5 km 동쪽 도로(105)는 경도 127.8을 지나 두 조각에 들어 있다.
        let wide = around(4575.0, 0.0, 200.0);
        assert_eq!(tiles_covering(&wide).len(), 2);
        let body = parse_graph_full(&data.region(&wide).unwrap().snapshot).unwrap();
        assert_eq!(
            body.ways.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![105]
        );
        // 멀리 떨어진 곳(도로 없음)은 오류.
        assert!(data.region(&around(-20000.0, -20000.0, 200.0)).is_err());
        for dir in [a, b] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// 범위 밖 노드를 가진 도로도 범위를 지나면 통째로 넣고, 지역 스냅샷 파일은 자료 판 폴더에 한 번만 쓴다.
    #[test]
    fn region_keeps_whole_ways_and_writes_snapshot_once() {
        let dir = temp_dir("snapshot");
        build_fixture(&fixture(), &dir, None);
        let data = KoreaData::open(&dir).unwrap();
        // 도로 101(남북)은 노드 4·2·5 모두 범위 밖이지만 가운데 선분이 범위를 지난다.
        let narrow = around(100.0, 0.0, 20.0);
        let body = parse_graph_full(&data.region(&narrow).unwrap().snapshot).unwrap();
        let way = body.ways.iter().find(|w| w.id == 101).unwrap();
        assert_eq!(way.nodes.len(), 3);
        let cache = temp_dir("snapshot-cache");
        let (path, places) = data
            .region_snapshot(&around(100.0, 0.0, 300.0), &cache)
            .unwrap();
        assert!(path.starts_with(cache.join(data.version())));
        assert_eq!(places.len(), 2);
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        let (again, _) = data
            .region_snapshot(&around(100.0, 0.0, 300.0), &cache)
            .unwrap();
        assert_eq!(again, path);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
        assert!(snapshot.connectivity.is_none());
        assert_eq!(snapshot.restrictions.len(), 1);
        for dir in [dir, cache] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// 형식 이름·묶음 크기·내용 해시가 맞지 않으면 열기·검사가 실패한다. 잘린 조각 본문도 오류다.
    #[test]
    fn mismatched_data_is_rejected() {
        let dir = temp_dir("reject");
        build_fixture(&fixture(), &dir, None);
        let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        std::fs::write(
            dir.join(MANIFEST_FILE),
            manifest.replace(FORMAT, "korea-data-v0"),
        )
        .unwrap();
        assert!(KoreaData::open(&dir).unwrap_err().contains("korea-data-v0"));
        std::fs::write(dir.join(MANIFEST_FILE), &manifest).unwrap();
        let mut pack_bytes = std::fs::read(dir.join(ROADS_PACK)).unwrap();
        let last = pack_bytes.len() - 1;
        pack_bytes[last] ^= 0xff;
        std::fs::write(dir.join(ROADS_PACK), &pack_bytes).unwrap();
        let data = KoreaData::open(&dir).unwrap();
        assert!(data.verify().is_err());
        pack_bytes.push(0);
        std::fs::write(dir.join(ROADS_PACK), &pack_bytes).unwrap();
        assert!(KoreaData::open(&dir).is_err());
        let tile = (0, 0);
        let body = encode_places(
            tile,
            &[&PlaceRecord {
                way: false,
                id: 1,
                name: "a".to_owned(),
                kind: String::new(),
                outline: vec![[1, 1]],
                entrances: Vec::new(),
            }],
        );
        assert!(decode_places(tile, &body[..body.len() - 1]).is_err());
        let mut longer = body.clone();
        longer.push(0);
        assert!(decode_places(tile, &longer).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 범위를 정해 변환하면 그 범위와 겹치는 것만 남고 머리 정보에 범위가 남는다.
    #[test]
    fn clip_keeps_only_the_area() {
        let dir = temp_dir("clip");
        let clip = around(100.0, 0.0, 400.0);
        let report = build_fixture(&fixture(), &dir, Some(&clip));
        assert_eq!(report.manifest.counts.roads, 2);
        assert!(report.manifest.source.clip.is_some());
        let data = KoreaData::open(&dir).unwrap();
        assert!(data.region(&around(4575.0, 0.0, 200.0)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
