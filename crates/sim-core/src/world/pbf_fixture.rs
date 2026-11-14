//! 시험용 작은 `.osm.pbf` 작성기(Phase 4-5G 정답 fixture). 시험에서만 쓴다.
//!
//! `osmpbf`는 읽기 전용이므로 PBF 형식(OSMHeader·OSMData 블록, 압축하지 않은 `raw` 블롭)을 직접 쓴다.
//! 노드는 DenseNodes 또는 일반 Node로, way는 일반 Way로 쓴다. granularity 100(기본), 좌표 오프셋 0이므로
//! 저장값 = 1e-7° 정수 좌표다. 기대값은 시험 코드에 사람이 직접 적고, 이 작성기는 입력만 만든다.

use super::osm_graph::fixtures;

/// 시험 노드: (ID, 동쪽 m, 북쪽 m, 버전·시각(초)).
pub type FxNode = (u64, f64, f64, Option<(i32, i64)>);
/// 시험 way: (ID, 태그, 노드 ID, 버전·시각(초)).
pub type FxWay = (
    u64,
    Vec<(&'static str, &'static str)>,
    Vec<u64>,
    Option<(i32, i64)>,
);

/// 시험 관계: (ID, 태그, 멤버 (종류 `"node"`·`"way"`, ID, 역할)).
pub type FxRelation = (
    u64,
    Vec<(&'static str, &'static str)>,
    Vec<(&'static str, u64, &'static str)>,
);

/// 시험 PBF 내용.
#[derive(Debug, Clone, Default)]
pub struct Fixture {
    /// 노드.
    pub nodes: Vec<FxNode>,
    /// way.
    pub ways: Vec<FxWay>,
    /// 머리부 복제 (시각 초, 순번). 없으면 쓰지 않는다.
    pub replication: Option<(i64, i64)>,
    /// 노드를 DenseNodes로 쓸지.
    pub dense: bool,
    /// 노드 태그 (노드 ID, 태그). 없는 노드는 태그 없음.
    pub node_tags: Vec<(u64, Vec<(&'static str, &'static str)>)>,
    /// 관계. 비어 있으면 관계 블록을 쓰지 않는다.
    pub relations: Vec<FxRelation>,
}

/// 합성 기준점에서 (동쪽, 북쪽) m인 점의 1e-7° 정수 좌표 [위도, 경도].
pub fn e7(east: f64, north: f64) -> [i32; 2] {
    let point = fixtures::offset(east, north);
    [
        (point.lat * 1e7).round() as i32,
        (point.lon * 1e7).round() as i32,
    ]
}

/// 부호 없는 varint를 붙인다.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// zigzag 부호화(sint).
fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// varint 필드를 붙인다.
fn field_varint(out: &mut Vec<u8>, number: u32, value: u64) {
    varint(out, u64::from(number) << 3);
    varint(out, value);
}

/// 길이 붙은 필드(문자열·바이트·하위 메시지·packed 배열)를 붙인다.
fn field_bytes(out: &mut Vec<u8>, number: u32, bytes: &[u8]) {
    varint(out, (u64::from(number) << 3) | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// packed varint 배열을 만든다.
fn packed(values: impl IntoIterator<Item = u64>) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        varint(&mut out, value);
    }
    out
}

/// 값 목록을 앞 값과의 차이(DELTA)로 바꾼다.
fn deltas(values: &[i64]) -> Vec<i64> {
    let mut previous = 0;
    values
        .iter()
        .map(|&value| {
            let delta = value - previous;
            previous = value;
            delta
        })
        .collect()
}

/// 문자열표. 0번은 빈 문자열이어야 한다.
struct Strings(Vec<String>);

impl Strings {
    /// 빈 문자열만 있는 표를 만든다.
    fn new() -> Self {
        Self(vec![String::new()])
    }

    /// 문자열 번호(없으면 추가).
    fn id(&mut self, text: &str) -> u64 {
        if let Some(index) = self.0.iter().position(|known| known == text) {
            return index as u64;
        }
        self.0.push(text.to_owned());
        (self.0.len() - 1) as u64
    }

    /// StringTable 메시지.
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for text in &self.0 {
            field_bytes(&mut out, 1, text.as_bytes());
        }
        out
    }
}

/// Info 메시지(버전, 시각 초 — date_granularity 1000 ms 기준).
fn info((version, seconds): (i32, i64)) -> Vec<u8> {
    let mut out = Vec::new();
    field_varint(&mut out, 1, version as u64);
    field_varint(&mut out, 2, seconds as u64);
    out
}

/// PrimitiveBlock 메시지(문자열표 + 그룹 하나).
fn primitive_block(strings: &Strings, group: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    field_bytes(&mut out, 1, &strings.encode());
    field_bytes(&mut out, 2, group);
    out
}

/// 블롭 하나(BlobHeader 길이 + BlobHeader + 압축 안 한 Blob)를 붙인다.
fn blob(out: &mut Vec<u8>, kind: &str, data: &[u8]) {
    let mut body = Vec::new();
    field_bytes(&mut body, 1, data);
    field_varint(&mut body, 2, data.len() as u64);
    let mut header = Vec::new();
    field_bytes(&mut header, 1, kind.as_bytes());
    field_varint(&mut header, 3, body.len() as u64);
    out.extend_from_slice(&(header.len() as u32).to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&body);
}

impl Fixture {
    /// 노드 ID의 태그(없으면 빈 목록).
    fn tags_of(&self, id: u64) -> &[(&'static str, &'static str)] {
        self.node_tags
            .iter()
            .find(|(node, _)| *node == id)
            .map_or(&[], |(_, tags)| tags.as_slice())
    }

    /// PBF 바이트열을 만든다: 머리부 블록, 노드 블록, way 블록, (있으면) 관계 블록 순서.
    pub fn to_pbf(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut header = Vec::new();
        field_bytes(&mut header, 4, b"OsmSchema-V0.6");
        field_bytes(&mut header, 4, b"DenseNodes");
        field_bytes(&mut header, 16, b"self-driving-sim test fixture");
        if let Some((seconds, sequence)) = self.replication {
            field_varint(&mut header, 32, seconds as u64);
            field_varint(&mut header, 33, sequence as u64);
        }
        blob(&mut out, "OSMHeader", &header);

        let mut strings = Strings::new();
        let mut group = Vec::new();
        if self.dense {
            let ids: Vec<i64> = self.nodes.iter().map(|n| n.0 as i64).collect();
            let coords: Vec<[i32; 2]> = self.nodes.iter().map(|n| e7(n.1, n.2)).collect();
            let lats: Vec<i64> = coords.iter().map(|c| i64::from(c[0])).collect();
            let lons: Vec<i64> = coords.iter().map(|c| i64::from(c[1])).collect();
            let mut dense = Vec::new();
            field_bytes(&mut dense, 1, &packed(deltas(&ids).into_iter().map(zigzag)));
            let meta: Vec<(i32, i64)> = self.nodes.iter().map(|n| n.3.unwrap_or((1, 0))).collect();
            let mut dense_info = Vec::new();
            field_bytes(&mut dense_info, 1, &packed(meta.iter().map(|m| m.0 as u64)));
            let times: Vec<i64> = meta.iter().map(|m| m.1).collect();
            field_bytes(
                &mut dense_info,
                2,
                &packed(deltas(&times).into_iter().map(zigzag)),
            );
            let zeros = || packed(std::iter::repeat_n(0, meta.len()));
            field_bytes(&mut dense_info, 3, &zeros());
            field_bytes(&mut dense_info, 4, &zeros());
            field_bytes(&mut dense_info, 5, &zeros());
            field_bytes(&mut dense, 5, &dense_info);
            field_bytes(
                &mut dense,
                8,
                &packed(deltas(&lats).into_iter().map(zigzag)),
            );
            field_bytes(
                &mut dense,
                9,
                &packed(deltas(&lons).into_iter().map(zigzag)),
            );
            if !self.node_tags.is_empty() {
                // keys_vals: 노드마다 (키, 값) 문자열 번호를 늘어놓고 0으로 끝낸다.
                let mut keys_vals = Vec::new();
                for node in &self.nodes {
                    for (key, value) in self.tags_of(node.0) {
                        keys_vals.push(strings.id(key));
                        keys_vals.push(strings.id(value));
                    }
                    keys_vals.push(0);
                }
                field_bytes(&mut dense, 10, &packed(keys_vals));
            }
            field_bytes(&mut group, 2, &dense);
        } else {
            for &(id, east, north, meta) in &self.nodes {
                let [lat, lon] = e7(east, north);
                let mut node = Vec::new();
                field_varint(&mut node, 1, zigzag(id as i64));
                let tags = self.tags_of(id);
                if !tags.is_empty() {
                    let keys: Vec<u64> = tags.iter().map(|(k, _)| strings.id(k)).collect();
                    let vals: Vec<u64> = tags.iter().map(|(_, v)| strings.id(v)).collect();
                    field_bytes(&mut node, 2, &packed(keys));
                    field_bytes(&mut node, 3, &packed(vals));
                }
                if let Some(meta) = meta {
                    field_bytes(&mut node, 4, &info(meta));
                }
                field_varint(&mut node, 8, zigzag(i64::from(lat)));
                field_varint(&mut node, 9, zigzag(i64::from(lon)));
                field_bytes(&mut group, 1, &node);
            }
        }
        blob(&mut out, "OSMData", &primitive_block(&strings, &group));

        let mut strings = Strings::new();
        let mut group = Vec::new();
        for (id, tags, refs, meta) in &self.ways {
            let mut way = Vec::new();
            field_varint(&mut way, 1, *id);
            let keys: Vec<u64> = tags.iter().map(|(k, _)| strings.id(k)).collect();
            let vals: Vec<u64> = tags.iter().map(|(_, v)| strings.id(v)).collect();
            field_bytes(&mut way, 2, &packed(keys));
            field_bytes(&mut way, 3, &packed(vals));
            if let Some(meta) = meta {
                field_bytes(&mut way, 4, &info(*meta));
            }
            let refs: Vec<i64> = refs.iter().map(|&r| r as i64).collect();
            field_bytes(&mut way, 8, &packed(deltas(&refs).into_iter().map(zigzag)));
            field_bytes(&mut group, 3, &way);
        }
        blob(&mut out, "OSMData", &primitive_block(&strings, &group));

        if !self.relations.is_empty() {
            let mut strings = Strings::new();
            let mut group = Vec::new();
            for (id, tags, members) in &self.relations {
                let mut relation = Vec::new();
                field_varint(&mut relation, 1, *id);
                let keys: Vec<u64> = tags.iter().map(|(k, _)| strings.id(k)).collect();
                let vals: Vec<u64> = tags.iter().map(|(_, v)| strings.id(v)).collect();
                field_bytes(&mut relation, 2, &packed(keys));
                field_bytes(&mut relation, 3, &packed(vals));
                let roles: Vec<u64> = members.iter().map(|m| strings.id(m.2)).collect();
                let ids: Vec<i64> = members.iter().map(|m| m.1 as i64).collect();
                // 멤버 종류: 0 노드, 1 way, 2 관계.
                let types = members.iter().map(|m| match m.0 {
                    "node" => 0,
                    "way" => 1,
                    _ => 2,
                });
                field_bytes(&mut relation, 8, &packed(roles));
                field_bytes(
                    &mut relation,
                    9,
                    &packed(deltas(&ids).into_iter().map(zigzag)),
                );
                field_bytes(&mut relation, 10, &packed(types));
                field_bytes(&mut group, 4, &relation);
            }
            blob(&mut out, "OSMData", &primitive_block(&strings, &group));
        }
        out
    }
}
