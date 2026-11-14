//! Lanelet2 OSM XML 부분집합 파서와 독립 검증(최소 구현 1단계).
//!
//! `node`, `way`, `type=lanelet` relation의 `left`·`right`와 선택적 `centerline`만 읽는다.
//! 형상 생성, 좌표 투영, 코스 조립과 Godot 연결은 후속 단계에서 맡는다.

use std::collections::{BTreeMap, BTreeSet};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// 한 번에 읽을 Lanelet2 XML의 최대 크기(32 MiB). 1단계는 작은 지역 자료만 다룬다.
const MAX_XML_BYTES: usize = 32 * 1024 * 1024;
/// 한 파일에 허용할 node 수 상한.
const MAX_NODES: usize = 1_000_000;
/// 한 파일에 허용할 way 수 상한.
const MAX_WAYS: usize = 250_000;
/// 한 파일에 허용할 lanelet 수 상한.
const MAX_LANELETS: usize = 100_000;

/// 위경도 좌표를 가진 Lanelet2 OSM node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneletNode {
    /// OSM XML의 0이 아닌 node ID. 업로드 전 Lanelet2 자료의 음수 ID도 보존한다.
    pub id: i64,
    /// WGS84 위도(deg).
    pub lat: f64,
    /// WGS84 경도(deg).
    pub lon: f64,
}

/// node 순서를 보존하는 Lanelet2 OSM way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneletWay {
    /// OSM XML의 0이 아닌 way ID.
    pub id: i64,
    /// 경계 또는 중심선을 이루는 node ID 순서.
    pub nodes: Vec<i64>,
}

/// 파서가 지원하는 차선 진행 규칙.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneletDirection {
    /// relation 진행 방향으로만 통행한다.
    OneWay,
    /// 양방향 통행을 허용한다.
    Bidirectional,
}

/// 검증을 마친 `type=lanelet`, `subtype=road` relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneletRelation {
    /// OSM XML relation ID.
    pub id: i64,
    /// 왼쪽 경계 way ID.
    pub left: i64,
    /// 오른쪽 경계 way ID.
    pub right: i64,
    /// 원본에 명시된 중심선 way ID. 없으면 후속 형상 단계가 생성한다.
    pub centerline: Option<i64>,
    /// 일방통행 여부.
    pub direction: LaneletDirection,
}

/// Lanelet2 XML에서 읽고 참조 검증까지 마친 결정적 자료.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneletMap {
    /// node ID 순으로 정렬된 node.
    pub nodes: BTreeMap<i64, LaneletNode>,
    /// way ID 순으로 정렬된 way.
    pub ways: BTreeMap<i64, LaneletWay>,
    /// relation ID 순으로 정렬된 도로 lanelet.
    pub lanelets: BTreeMap<i64, LaneletRelation>,
    /// 입력 XML 원문 바이트의 SHA-256.
    pub sha256: String,
}

/// XML을 읽는 동안 way 하나에 모으는 임시 정보.
#[derive(Debug)]
struct RawWay {
    id: i64,
    nodes: Vec<i64>,
}

/// XML을 읽는 동안 relation member 하나에 모으는 임시 정보.
#[derive(Debug)]
struct RawMember {
    member_type: String,
    reference: i64,
    role: String,
}

/// XML을 읽는 동안 relation 하나에 모으는 임시 정보.
#[derive(Debug)]
struct RawRelation {
    id: i64,
    members: Vec<RawMember>,
    tags: BTreeMap<String, String>,
}

/// 현재 XML 부모 요소에 따라 자식 `tag`를 받을 대상을 나타낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parent {
    None,
    Way,
    Relation,
}

/// 속성 이름 하나를 UTF-8 문자열로 읽고 필수 여부를 검사한다.
fn attribute(element: &BytesStart<'_>, name: &[u8]) -> Result<String, String> {
    for item in element.attributes().with_checks(true) {
        let item = item.map_err(|error| format!("invalid XML attribute: {error}"))?;
        if item.key.as_ref() == name {
            return std::str::from_utf8(item.value.as_ref())
                .map(str::to_owned)
                .map_err(|_| {
                    format!("attribute `{}` is not UTF-8", String::from_utf8_lossy(name))
                });
        }
    }
    Err(format!(
        "element `{}` is missing attribute `{}`",
        String::from_utf8_lossy(element.name().as_ref()),
        String::from_utf8_lossy(name)
    ))
}

/// 0이 아닌 OSM ID 속성을 읽는다. Lanelet2 편집 자료의 음수 임시 ID도 허용한다.
fn osm_id(element: &BytesStart<'_>, name: &[u8]) -> Result<i64, String> {
    let text = attribute(element, name)?;
    let id = text
        .parse::<i64>()
        .map_err(|_| format!("invalid OSM ID `{text}`"))?;
    if id == 0 {
        return Err("OSM ID must not be zero".to_owned());
    }
    Ok(id)
}

/// node 시작 요소를 검증된 위경도 node로 바꾼다.
fn parse_node(element: &BytesStart<'_>) -> Result<LaneletNode, String> {
    let id = osm_id(element, b"id")?;
    let lat_text = attribute(element, b"lat")?;
    let lon_text = attribute(element, b"lon")?;
    let lat = lat_text
        .parse::<f64>()
        .map_err(|_| format!("node {id} has invalid latitude `{lat_text}`"))?;
    let lon = lon_text
        .parse::<f64>()
        .map_err(|_| format!("node {id} has invalid longitude `{lon_text}`"))?;
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err(format!("node {id} latitude {lat} is outside -90..=90"));
    }
    if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
        return Err(format!("node {id} longitude {lon} is outside -180..=180"));
    }
    Ok(LaneletNode { id, lat, lon })
}

/// relation member 시작 요소를 임시 member로 바꾼다.
fn parse_member(element: &BytesStart<'_>) -> Result<RawMember, String> {
    Ok(RawMember {
        member_type: attribute(element, b"type")?,
        reference: osm_id(element, b"ref")?,
        role: attribute(element, b"role")?,
    })
}

/// `tag` 시작 요소에서 key와 value를 읽는다.
fn parse_tag(element: &BytesStart<'_>) -> Result<(String, String), String> {
    Ok((attribute(element, b"k")?, attribute(element, b"v")?))
}

/// key가 중복되지 않도록 태그를 추가한다.
fn insert_tag(
    tags: &mut BTreeMap<String, String>,
    key: String,
    value: String,
) -> Result<(), String> {
    if tags.insert(key.clone(), value).is_some() {
        return Err(format!("duplicate tag `{key}`"));
    }
    Ok(())
}

/// 같은 역할이 두 번 나오지 않도록 lanelet way member를 찾는다.
fn member_for_role(
    relation: &RawRelation,
    role: &str,
    required: bool,
) -> Result<Option<i64>, String> {
    let members: Vec<&RawMember> = relation
        .members
        .iter()
        .filter(|member| member.role == role)
        .collect();
    if members.len() > 1 {
        return Err(format!(
            "lanelet {} has duplicate `{role}` members",
            relation.id
        ));
    }
    let Some(member) = members.first() else {
        return if required {
            Err(format!(
                "lanelet {} is missing `{role}` member",
                relation.id
            ))
        } else {
            Ok(None)
        };
    };
    if member.member_type != "way" {
        return Err(format!(
            "lanelet {} `{role}` member must have type=way",
            relation.id
        ));
    }
    Ok(Some(member.reference))
}

/// 원시 relation이 지원 대상이면 검증된 도로 lanelet으로 바꾸고, 일반 OSM relation이면 건너뛴다.
fn validate_relation(
    relation: RawRelation,
    ways: &BTreeMap<i64, LaneletWay>,
) -> Result<Option<LaneletRelation>, String> {
    if relation.tags.get("type").map(String::as_str) != Some("lanelet") {
        return Ok(None);
    }
    if relation.tags.get("subtype").map(String::as_str) != Some("road") {
        return Err(format!(
            "lanelet {} must have subtype=road in the minimal parser",
            relation.id
        ));
    }
    for member in &relation.members {
        if !matches!(member.role.as_str(), "left" | "right" | "centerline") {
            return Err(format!(
                "lanelet {} has unsupported member role `{}`",
                relation.id, member.role
            ));
        }
    }
    let left = member_for_role(&relation, "left", true)?.expect("required member exists");
    let right = member_for_role(&relation, "right", true)?.expect("required member exists");
    let centerline = member_for_role(&relation, "centerline", false)?;
    if left == right || centerline.is_some_and(|center| center == left || center == right) {
        return Err(format!("lanelet {} reuses a boundary way", relation.id));
    }
    for reference in [Some(left), Some(right), centerline].into_iter().flatten() {
        if !ways.contains_key(&reference) {
            return Err(format!(
                "lanelet {} references missing way {reference}",
                relation.id
            ));
        }
    }
    let direction = match relation.tags.get("one_way").map(String::as_str) {
        None | Some("yes") => LaneletDirection::OneWay,
        Some("no") => LaneletDirection::Bidirectional,
        Some(value) => {
            return Err(format!(
                "lanelet {} has unsupported one_way value `{value}`",
                relation.id
            ));
        }
    };
    Ok(Some(LaneletRelation {
        id: relation.id,
        left,
        right,
        centerline,
        direction,
    }))
}

/// way의 node 참조, 최소 길이와 연속 중복을 검사한다.
fn validate_ways(
    ways: &BTreeMap<i64, LaneletWay>,
    nodes: &BTreeMap<i64, LaneletNode>,
) -> Result<(), String> {
    for way in ways.values() {
        if way.nodes.len() < 2 {
            return Err(format!("way {} has fewer than 2 nodes", way.id));
        }
        let mut previous = None;
        for &reference in &way.nodes {
            if !nodes.contains_key(&reference) {
                return Err(format!(
                    "way {} references missing node {reference}",
                    way.id
                ));
            }
            if previous == Some(reference) {
                return Err(format!(
                    "way {} repeats node {reference} consecutively",
                    way.id
                ));
            }
            previous = Some(reference);
        }
    }
    Ok(())
}

/// Lanelet2 OSM XML 바이트를 읽어 ID 순으로 정렬된 검증 결과를 만든다.
pub fn parse_lanelet2(bytes: &[u8]) -> Result<LaneletMap, String> {
    if bytes.is_empty() || bytes.len() > MAX_XML_BYTES {
        return Err(format!(
            "Lanelet2 XML size {} is outside 1..={MAX_XML_BYTES}",
            bytes.len()
        ));
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut nodes = BTreeMap::new();
    let mut ways = BTreeMap::new();
    let mut relations = Vec::new();
    let mut current_way: Option<RawWay> = None;
    let mut current_relation: Option<RawRelation> = None;
    let mut parent = Parent::None;
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| format!("invalid Lanelet2 XML: {error}"))?;
        match event {
            Event::Start(element) if element.name().as_ref() == b"way" => {
                if parent != Parent::None {
                    return Err("nested way is not allowed".to_owned());
                }
                current_way = Some(RawWay {
                    id: osm_id(&element, b"id")?,
                    nodes: Vec::new(),
                });
                parent = Parent::Way;
            }
            Event::Start(element) if element.name().as_ref() == b"relation" => {
                if parent != Parent::None {
                    return Err("nested relation is not allowed".to_owned());
                }
                current_relation = Some(RawRelation {
                    id: osm_id(&element, b"id")?,
                    members: Vec::new(),
                    tags: BTreeMap::new(),
                });
                parent = Parent::Relation;
            }
            Event::Start(element) | Event::Empty(element) if element.name().as_ref() == b"node" => {
                let node = parse_node(&element)?;
                if nodes.insert(node.id, node).is_some() {
                    return Err(format!("duplicate node ID {}", node.id));
                }
                if nodes.len() > MAX_NODES {
                    return Err(format!("node count exceeds {MAX_NODES}"));
                }
            }
            Event::Empty(element) if element.name().as_ref() == b"nd" && parent == Parent::Way => {
                current_way
                    .as_mut()
                    .expect("way parent has a value")
                    .nodes
                    .push(osm_id(&element, b"ref")?);
            }
            Event::Empty(element)
                if element.name().as_ref() == b"member" && parent == Parent::Relation =>
            {
                current_relation
                    .as_mut()
                    .expect("relation parent has a value")
                    .members
                    .push(parse_member(&element)?);
            }
            Event::Empty(element) if element.name().as_ref() == b"tag" => {
                let (key, value) = parse_tag(&element)?;
                match parent {
                    Parent::Relation => insert_tag(
                        &mut current_relation
                            .as_mut()
                            .expect("relation parent has a value")
                            .tags,
                        key,
                        value,
                    )?,
                    Parent::Way | Parent::None => {}
                }
            }
            Event::End(element) if element.name().as_ref() == b"way" => {
                let raw = current_way.take().ok_or("way end without start")?;
                let way = LaneletWay {
                    id: raw.id,
                    nodes: raw.nodes,
                };
                if ways.insert(way.id, way).is_some() {
                    return Err(format!("duplicate way ID {}", raw.id));
                }
                if ways.len() > MAX_WAYS {
                    return Err(format!("way count exceeds {MAX_WAYS}"));
                }
                parent = Parent::None;
            }
            Event::End(element) if element.name().as_ref() == b"relation" => {
                relations.push(
                    current_relation
                        .take()
                        .ok_or("relation end without start")?,
                );
                parent = Parent::None;
            }
            Event::Text(text)
                if !text
                    .iter()
                    .copied()
                    .all(|byte: u8| byte.is_ascii_whitespace()) =>
            {
                return Err("unexpected text inside Lanelet2 XML".to_owned());
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if parent != Parent::None {
        return Err("Lanelet2 XML ended inside an element".to_owned());
    }
    validate_ways(&ways, &nodes)?;
    let mut lanelets = BTreeMap::new();
    let mut relation_ids = BTreeSet::new();
    for relation in relations {
        if !relation_ids.insert(relation.id) {
            return Err(format!("duplicate relation ID {}", relation.id));
        }
        if let Some(lanelet) = validate_relation(relation, &ways)? {
            if lanelets.insert(lanelet.id, lanelet).is_some() {
                unreachable!("relation IDs were checked above");
            }
            if lanelets.len() > MAX_LANELETS {
                return Err(format!("lanelet count exceeds {MAX_LANELETS}"));
            }
        }
    }
    if lanelets.is_empty() {
        return Err("Lanelet2 XML contains no supported road lanelets".to_owned());
    }
    Ok(LaneletMap {
        nodes,
        ways,
        lanelets,
        sha256: super::road_snapshot::sha256_hex(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 저장소의 정상 합성 Lanelet2 fixture를 읽는다.
    fn straight_fixture() -> &'static [u8] {
        include_bytes!("../../../../testdata/lanelet2/synthetic-straight.osm")
    }

    /// 정상 fixture의 특정 문자열 하나를 바꾼 시험 입력을 만든다.
    fn changed(from: &str, to: &str) -> Vec<u8> {
        String::from_utf8(straight_fixture().to_vec())
            .unwrap()
            .replacen(from, to, 1)
            .into_bytes()
    }

    /// 정상 직선 lanelet의 node·way·relation·방향과 SHA-256을 읽는다.
    #[test]
    fn parses_straight_lanelet() {
        let map = parse_lanelet2(straight_fixture()).unwrap();
        assert_eq!(map.nodes.len(), 6);
        assert_eq!(map.ways.len(), 3);
        assert_eq!(map.sha256.len(), 64);
        assert_eq!(
            map.lanelets.get(&301),
            Some(&LaneletRelation {
                id: 301,
                left: 201,
                right: 202,
                centerline: Some(203),
                direction: LaneletDirection::OneWay,
            })
        );
    }

    /// XML 요소 순서가 달라도 ID 정렬 결과와 의미는 같다.
    #[test]
    fn element_order_does_not_change_parsed_map() {
        let reordered =
            include_bytes!("../../../../testdata/lanelet2/synthetic-straight-reordered.osm");
        let first = parse_lanelet2(straight_fixture()).unwrap();
        let second = parse_lanelet2(reordered).unwrap();
        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.ways, second.ways);
        assert_eq!(first.lanelets, second.lanelets);
        assert_ne!(first.sha256, second.sha256);
    }

    /// 중심선은 선택 사항이고 one_way=no는 양방향으로 읽는다.
    #[test]
    fn centerline_is_optional_and_bidirectional_is_supported() {
        let without_center = changed("<member type=\"way\" ref=\"203\" role=\"centerline\"/>", "");
        let without_center = String::from_utf8(without_center)
            .unwrap()
            .replace("one_way\" v=\"yes", "one_way\" v=\"no");
        let map = parse_lanelet2(without_center.as_bytes()).unwrap();
        let lanelet = &map.lanelets[&301];
        assert_eq!(lanelet.centerline, None);
        assert_eq!(lanelet.direction, LaneletDirection::Bidirectional);
    }

    /// 태그를 가진 비어 있지 않은 node와 업로드 전 음수 ID를 그대로 읽는다.
    #[test]
    fn tagged_nodes_and_negative_ids_are_supported() {
        let xml = changed(
            "  <node id=\"1\" lat=\"34.0000000\" lon=\"127.0000000\"/>",
            "  <node id=\"-1\" lat=\"34.0000000\" lon=\"127.0000000\"><tag k=\"ele\" v=\"4.2\"/></node>",
        );
        let xml = String::from_utf8(xml)
            .unwrap()
            .replace("ref=\"1\"", "ref=\"-1\"");
        let map = parse_lanelet2(xml.as_bytes()).unwrap();
        assert_eq!(map.nodes[&-1].lat, 34.0);
        assert_eq!(map.ways[&201].nodes[0], -1);
    }

    /// 누락 node·way 참조와 2개 미만 node를 가진 way를 거부한다.
    #[test]
    fn rejects_missing_and_short_references() {
        let missing_node = changed("ref=\"6\"", "ref=\"999\"");
        assert!(
            parse_lanelet2(&missing_node)
                .unwrap_err()
                .contains("missing node 999")
        );
        let missing_way = changed("ref=\"202\" role=\"right\"", "ref=\"999\" role=\"right\"");
        assert!(
            parse_lanelet2(&missing_way)
                .unwrap_err()
                .contains("missing way 999")
        );
        let short = String::from_utf8(changed("<nd ref=\"2\"/>", ""))
            .unwrap()
            .replacen("<nd ref=\"3\"/>", "", 1);
        assert!(
            parse_lanelet2(short.as_bytes())
                .unwrap_err()
                .contains("fewer than 2 nodes")
        );
    }

    /// 중복 ID와 중복 필수 역할을 거부한다.
    #[test]
    fn rejects_duplicate_ids_and_roles() {
        let duplicate_node = changed("id=\"2\"", "id=\"1\"");
        assert!(
            parse_lanelet2(&duplicate_node)
                .unwrap_err()
                .contains("duplicate node ID")
        );
        let duplicate_role = changed("role=\"centerline\"", "role=\"left\"");
        assert!(
            parse_lanelet2(&duplicate_role)
                .unwrap_err()
                .contains("duplicate `left`")
        );
    }

    /// 비정상 좌표, 잘못된 일방통행 값과 도로가 아닌 lanelet을 거부한다.
    #[test]
    fn rejects_invalid_coordinates_and_tags() {
        let latitude = changed("lat=\"34.0000000\"", "lat=\"91\"");
        assert!(
            parse_lanelet2(&latitude)
                .unwrap_err()
                .contains("outside -90..=90")
        );
        let direction = changed("one_way\" v=\"yes", "one_way\" v=\"sometimes");
        assert!(
            parse_lanelet2(&direction)
                .unwrap_err()
                .contains("unsupported one_way")
        );
        let subtype = changed("subtype\" v=\"road", "subtype\" v=\"crosswalk");
        assert!(
            parse_lanelet2(&subtype)
                .unwrap_err()
                .contains("subtype=road")
        );
    }

    /// 규제 요소처럼 최소 구현이 해석할 수 없는 member 역할은 조용히 버리지 않는다.
    #[test]
    fn rejects_unsupported_relation_members() {
        let regulatory = changed(
            "    <tag k=\"type\" v=\"lanelet\"/>",
            "    <member type=\"relation\" ref=\"900\" role=\"regulatory_element\"/>\n    <tag k=\"type\" v=\"lanelet\"/>",
        );
        assert!(
            parse_lanelet2(&regulatory)
                .unwrap_err()
                .contains("unsupported member role `regulatory_element`")
        );
    }
}
