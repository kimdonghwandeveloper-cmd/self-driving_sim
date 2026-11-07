//! 노드 ID를 보존한 OSM 도로 자료(Phase 4 선행 자료).
//!
//! Overpass `out body geom` 응답에서 way마다 노드 ID·좌표·태그를 함께 읽는다. 도로 연결은 **같은 OSM 노드 ID**로만
//! 판단하며, 좌표가 같거나 가깝다는 이유로 연결을 추정하지 않는다. 노드 ID가 없는 자료(`out geom`만 받은 자료)나
//! ID와 좌표 개수가 맞지 않는 자료, 빠진 노드가 있는 자료는 오류로 거부한다.
//!
//! 교차로 판정(차수 3 이상)은 이 단계에서 자료 검증용으로만 쓴다. 교차로 형상 생성은 PLAN 4-2에서 한다.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::map::LatLon;

/// 노드 ID와 좌표.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsmNode {
    /// OSM 노드 ID.
    pub id: u64,
    /// 위경도.
    pub pos: LatLon,
}

/// 노드 ID·태그를 보존한 OSM way.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmGraphWay {
    /// OSM way ID.
    pub id: u64,
    /// 태그(`highway`, `width`, `lanes`, `oneway` 등). 순서가 일정하도록 정렬해 둔다.
    pub tags: BTreeMap<String, String>,
    /// 진행 순서대로의 노드.
    pub nodes: Vec<OsmNode>,
}

impl OsmGraphWay {
    /// `highway` 태그 값을 반환한다. 없으면 `None`.
    pub fn highway(&self) -> Option<&str> {
        self.tags.get("highway").map(String::as_str)
    }
}

/// Overpass JSON 응답 중 필요한 부분.
#[derive(Debug, Deserialize)]
struct GraphResponse {
    elements: Vec<GraphElement>,
}

/// Overpass 요소. way가 아니면 무시한다.
#[derive(Debug, Deserialize)]
struct GraphElement {
    #[serde(rename = "type")]
    kind: String,
    id: u64,
    /// `out body`의 노드 ID 목록. `out geom`만 받은 자료에는 없다.
    nodes: Option<Vec<u64>>,
    /// `out geom`의 노드 좌표. 응답에 없는 노드는 `null`로 온다.
    #[serde(default)]
    geometry: Vec<Option<LatLon>>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

/// `out body geom` Overpass JSON에서 노드 ID·좌표·태그를 보존한 way 목록을 읽는다.
///
/// way에 노드 ID 배열이 없거나, ID와 좌표 개수가 다르거나, 좌표가 빠졌거나(`null`) 범위를 벗어나거나,
/// 노드가 2개 미만이면 오류로 처리한다. way가 하나도 없어도 오류다.
pub fn parse_overpass_graph(json: &str) -> Result<Vec<OsmGraphWay>, String> {
    let response: GraphResponse =
        serde_json::from_str(json).map_err(|error| format!("invalid Overpass JSON: {error}"))?;
    let mut ways = Vec::new();
    for element in response.elements.into_iter().filter(|e| e.kind == "way") {
        let id = element.id;
        let ids = element.nodes.ok_or_else(|| {
            format!("way {id} has no node IDs; fetch with `out body geom` to keep node IDs")
        })?;
        if ids.len() != element.geometry.len() {
            return Err(format!(
                "way {id} has {} node IDs but {} coordinates",
                ids.len(),
                element.geometry.len()
            ));
        }
        if ids.len() < 2 {
            return Err(format!("way {id} has fewer than 2 nodes"));
        }
        let mut nodes = Vec::with_capacity(ids.len());
        for (index, (node_id, pos)) in ids.into_iter().zip(element.geometry).enumerate() {
            let pos = pos.ok_or_else(|| {
                format!("way {id} node {node_id} (index {index}) is missing from the response")
            })?;
            if !(pos.lat.is_finite()
                && pos.lon.is_finite()
                && pos.lat.abs() <= 90.0
                && pos.lon.abs() <= 180.0)
            {
                return Err(format!("way {id} node {node_id} has an invalid coordinate"));
            }
            nodes.push(OsmNode { id: node_id, pos });
        }
        ways.push(OsmGraphWay {
            id,
            tags: element.tags,
            nodes,
        });
    }
    if ways.is_empty() {
        return Err("the response contains no ways".to_string());
    }
    Ok(ways)
}

/// 허용한 `highway` 종류의 way만으로 노드 ID별 연결 차수를 센다.
///
/// way 끝 노드는 1, 중간 노드는 2를 더한다. 차수 3 이상이 교차로(갈래 수 = 차수)다.
/// 연결은 노드 ID로만 판단한다.
pub fn node_degrees(ways: &[OsmGraphWay], allowed: &BTreeSet<&str>) -> BTreeMap<u64, u32> {
    let mut degrees = BTreeMap::new();
    for way in ways
        .iter()
        .filter(|way| way.highway().is_some_and(|kind| allowed.contains(kind)))
    {
        let last = way.nodes.len() - 1;
        for (index, node) in way.nodes.iter().enumerate() {
            let add = if index == 0 || index == last { 1 } else { 2 };
            *degrees.entry(node.id).or_insert(0) += add;
        }
    }
    degrees
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! 합성 시험 자료(PLAN 4 선행 자료). 지역 미터 좌표로 형상을 정의하고 위경도로 바꿔
    //! 실제 자료와 같은 경로([`parse_overpass_graph`])를 거치게 한다. 실제 도로가 아니다.

    use super::*;

    /// 합성 자료의 기준점(여수 부근 임의 좌표, 실제 도로 아님).
    pub const ORIGIN: LatLon = LatLon {
        lat: 34.75,
        lon: 127.75,
    };
    /// 지구 반지름(m). `map::project`와 같은 값.
    const EARTH_RADIUS_M: f64 = 6_371_008.8;

    /// 기준점에서 동쪽 `east`, 북쪽 `north`(m)만큼 떨어진 위경도를 만든다(투영의 역변환).
    pub fn offset(east: f64, north: f64) -> LatLon {
        LatLon {
            lat: ORIGIN.lat + (north / EARTH_RADIUS_M).to_degrees(),
            lon: ORIGIN.lon
                + (east / (EARTH_RADIUS_M * ORIGIN.lat.to_radians().cos())).to_degrees(),
        }
    }

    /// 합성 way 정의: (way ID, highway, 추가 태그, 노드 (ID, 동쪽 m, 북쪽 m) 목록).
    pub type FixtureWay<'a> = (u64, &'a str, &'a [(&'a str, &'a str)], Vec<(u64, f64, f64)>);

    /// 합성 way 목록을 `out body geom` 형식의 Overpass JSON 문자열로 만든다.
    pub fn overpass_json(ways: &[FixtureWay]) -> String {
        let elements: Vec<String> = ways
            .iter()
            .map(|(id, highway, extra, nodes)| {
                let ids: Vec<String> = nodes.iter().map(|(n, _, _)| n.to_string()).collect();
                let geometry: Vec<String> = nodes
                    .iter()
                    .map(|(_, east, north)| {
                        let p = offset(*east, *north);
                        format!("{{\"lat\":{},\"lon\":{}}}", p.lat, p.lon)
                    })
                    .collect();
                let mut tags = vec![format!("\"highway\":\"{highway}\"")];
                tags.extend(extra.iter().map(|(k, v)| format!("\"{k}\":\"{v}\"")));
                format!(
                    "{{\"type\":\"way\",\"id\":{id},\"nodes\":[{}],\"geometry\":[{}],\"tags\":{{{}}}}}",
                    ids.join(","),
                    geometry.join(","),
                    tags.join(",")
                )
            })
            .collect();
        format!("{{\"elements\":[{}]}}", elements.join(","))
    }

    /// 중심 (cx, cn), 반경 r(m)의 원호를 시작각 a0부터 a1까지(도, 동쪽 0°·반시계) `steps`조각으로 나눈
    /// 노드 목록을 만든다. 노드 ID는 `first_id`부터 차례로 붙인다.
    pub fn arc(
        first_id: u64,
        center: (f64, f64),
        radius: f64,
        degrees: (f64, f64),
        steps: usize,
    ) -> Vec<(u64, f64, f64)> {
        (0..=steps)
            .map(|i| {
                let a =
                    (degrees.0 + (degrees.1 - degrees.0) * i as f64 / steps as f64).to_radians();
                (
                    first_id + i as u64,
                    center.0 + radius * a.cos(),
                    center.1 + radius * a.sin(),
                )
            })
            .collect()
    }

    /// 굽은 길: 직선 40 m → 반경 20 m로 90° 좌회전 → 직선 40 m. 정상 생성 대상.
    pub fn curved_road() -> Vec<FixtureWay<'static>> {
        let mut nodes = vec![(1, 0.0, -40.0)];
        nodes.extend(arc(2, (-20.0, 0.0), 20.0, (0.0, 90.0), 9));
        nodes.push((20, -60.0, 20.0));
        vec![(100, "service", &[], nodes)]
    }

    /// 넓은 도로 + 짧은 구간: 폭 12 m 도로가 4 m 간격으로 45°씩 두 번 꺾인다. 반경 축소 거부 대상.
    pub fn wide_short_zigzag() -> Vec<FixtureWay<'static>> {
        vec![(
            101,
            "service",
            &[("width", "12")],
            vec![
                (1, 0.0, 0.0),
                (2, 0.0, 30.0),
                (3, 2.83, 32.83),
                (4, 2.83, 62.83),
            ],
        )]
    }

    /// S자: 반경 15 m 원호 두 개를 반대 방향으로 이은 길. 정상 생성 대상.
    pub fn s_curve() -> Vec<FixtureWay<'static>> {
        let mut nodes = arc(1, (-15.0, 0.0), 15.0, (0.0, 90.0), 9);
        nodes.extend(
            arc(11, (-15.0, 30.0), 15.0, (270.0, 180.0), 9)
                .into_iter()
                .skip(1),
        );
        vec![(102, "service", &[], nodes)]
    }

    /// 되돌아오는 길: 북쪽 50 m → 반경 10 m로 180° 좌회전 → 남쪽 30 m → 다시 반경 10 m로 180° 좌회전 →
    /// 처음 직선(x=0) 위를 북쪽으로 20 m 겹쳐 달린다. 반경은 정상이지만 노면이 겹치고 공유 노드가 없어 거부 대상.
    pub fn loop_overlap() -> Vec<FixtureWay<'static>> {
        let mut nodes = vec![(1, 0.0, 0.0)];
        nodes.extend(arc(2, (-10.0, 50.0), 10.0, (0.0, 180.0), 9));
        nodes.extend(arc(20, (-10.0, 20.0), 10.0, (180.0, 360.0), 9));
        nodes.push((40, 0.0, 40.0));
        vec![(103, "service", &[], nodes)]
    }

    /// 3갈래 교차로: 동서 직선 way 하나의 중간 노드 10에 남쪽 way가 붙는다(T자).
    pub fn three_way() -> Vec<FixtureWay<'static>> {
        vec![
            (
                110,
                "service",
                &[],
                vec![(1, -50.0, 0.0), (10, 0.0, 0.0), (2, 50.0, 0.0)],
            ),
            (111, "service", &[], vec![(10, 0.0, 0.0), (3, 0.0, -50.0)]),
        ]
    }

    /// 4갈래 교차로: 동서 way와 남북 way가 노드 10을 공유한다(십자).
    pub fn four_way() -> Vec<FixtureWay<'static>> {
        vec![
            (
                120,
                "service",
                &[],
                vec![(1, -50.0, 0.0), (10, 0.0, 0.0), (2, 50.0, 0.0)],
            ),
            (
                121,
                "residential",
                &[],
                vec![(3, 0.0, -50.0), (10, 0.0, 0.0), (4, 0.0, 50.0)],
            ),
        ]
    }

    /// 좁은 팔: 동서 직선 중간 노드 10에서 동쪽 팔과 20°만 벌어진 팔이 붙는다. 25° 미만이라 거부 대상.
    pub fn narrow_arms() -> Vec<FixtureWay<'static>> {
        let (s, c) = 20f64.to_radians().sin_cos();
        vec![
            (
                140,
                "service",
                &[],
                vec![(1, -50.0, 0.0), (10, 0.0, 0.0), (2, 50.0, 0.0)],
            ),
            (
                141,
                "service",
                &[],
                vec![(10, 0.0, 0.0), (3, 50.0 * c, 50.0 * s)],
            ),
        ]
    }

    /// 가까운 교차로: 동서 직선 위 4 m 떨어진 노드 10·11에 각각 남·북 팔이 붙는다. 잘라 낼 거리가 겹쳐 거부 대상.
    pub fn close_junctions() -> Vec<FixtureWay<'static>> {
        vec![
            (
                150,
                "service",
                &[],
                vec![
                    (1, -50.0, 0.0),
                    (10, 0.0, 0.0),
                    (11, 4.0, 0.0),
                    (2, 50.0, 0.0),
                ],
            ),
            (151, "service", &[], vec![(10, 0.0, 0.0), (3, 0.0, -50.0)]),
            (152, "service", &[], vec![(11, 4.0, 0.0), (4, 4.0, 50.0)]),
        ]
    }

    /// 자기 곁가지로 돌아오는 경로: 동서 직선의 노드 10에 남쪽 팔(way 171)이 붙고, 경로 way 170은 동쪽으로 갔다가
    /// 남쪽·서쪽으로 돌아 노드 10의 남쪽 곁가지 안(0, -10)에서 끝난다. 겹침으로 거부 대상(Codex 4-2 검토 재현 사례).
    pub fn return_into_stub() -> Vec<FixtureWay<'static>> {
        vec![
            (
                170,
                "service",
                &[],
                vec![
                    (1, -50.0, 0.0),
                    (10, 0.0, 0.0),
                    (2, 50.0, 0.0),
                    (3, 50.0, -50.0),
                    (4, 0.0, -50.0),
                    (5, 0.0, -10.0),
                ],
            ),
            (171, "service", &[], vec![(10, 0.0, 0.0), (20, 0.0, -60.0)]),
        ]
    }

    /// 일방통행 팔: T자 교차로의 남쪽 팔이 `oneway=yes`다. 1차 판에서는 거부 대상.
    pub fn oneway_arm() -> Vec<FixtureWay<'static>> {
        vec![
            (
                160,
                "service",
                &[],
                vec![(1, -50.0, 0.0), (10, 0.0, 0.0), (2, 50.0, 0.0)],
            ),
            (
                161,
                "service",
                &[("oneway", "yes")],
                vec![(10, 0.0, 0.0), (3, 0.0, -50.0)],
            ),
        ]
    }

    /// 좌표만 같고 노드 ID가 다른 교차: 두 way가 (0, 0)에서 겹치지만 공유 노드가 없어 연결이 아니다.
    pub fn crossing_without_shared_node() -> Vec<FixtureWay<'static>> {
        vec![
            (
                130,
                "service",
                &[],
                vec![(1, -50.0, 0.0), (10, 0.0, 0.0), (2, 50.0, 0.0)],
            ),
            (
                131,
                "service",
                &[],
                vec![(3, 0.0, -50.0), (11, 0.0, 0.0), (4, 0.0, 50.0)],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::world::map::project;

    /// 저장소의 여수 노드 ID 스냅샷 경로(PLAN 4 선행 자료).
    const SNAPSHOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/maps/yeosu-expo/graph-2026-11-06.osm.json"
    );
    /// 교차로 판정에 쓰는 `highway` 허용 목록(시험용, 4-3에서 설정 파일로 옮긴다).
    const ALLOWED: [&str; 5] = [
        "service",
        "pedestrian",
        "residential",
        "unclassified",
        "tertiary",
    ];

    /// 허용 목록 집합을 만든다.
    fn allowed() -> BTreeSet<&'static str> {
        ALLOWED.into_iter().collect()
    }

    /// 저장소 스냅샷을 읽는다.
    fn snapshot() -> Vec<OsmGraphWay> {
        let json = std::fs::read_to_string(SNAPSHOT).expect("snapshot file");
        parse_overpass_graph(&json).expect("snapshot parses")
    }

    /// 실제 스냅샷: 모든 way가 노드 ID를 가지고, 기록한 개수(way 146개)와 같은지 확인한다.
    #[test]
    fn snapshot_keeps_node_ids() {
        let ways = snapshot();
        assert_eq!(ways.len(), 146);
        assert!(ways.iter().all(|way| way.nodes.len() >= 2));
        assert!(ways.iter().all(|way| way.highway().is_some()));
    }

    /// 실제 스냅샷: 문서에 기록한 3갈래·4갈래 교차로가 공유 노드 ID로 인식되는지 확인한다.
    #[test]
    fn snapshot_has_documented_intersections() {
        let degrees = node_degrees(&snapshot(), &allowed());
        // 3갈래: 고정 코스 way 532526349와 way 541900361이 만나는 노드.
        assert_eq!(degrees.get(&5_239_572_002), Some(&3));
        // 4갈래: way 529495033과 way 529495041이 만나는 노드.
        assert_eq!(degrees.get(&5_143_358_021), Some(&4));
        // 4갈래: way 529495033과 way 532526348이 만나는 노드.
        assert_eq!(degrees.get(&5_528_563_751), Some(&4));
        let three = degrees.values().filter(|&&d| d == 3).count();
        let four_plus = degrees.values().filter(|&&d| d >= 4).count();
        assert_eq!((three, four_plus), (39, 8));
    }

    /// 실제 스냅샷: 문서에 기록한 굽은 경로(way 529495041, service 약 321 m)가 있고, 원점 1 km 안인지 확인한다.
    #[test]
    fn snapshot_has_documented_curve() {
        let ways = snapshot();
        let way = ways.iter().find(|way| way.id == 529_495_041).unwrap();
        assert_eq!(way.highway(), Some("service"));
        let points: Vec<_> = way.nodes.iter().map(|n| project(ORIGIN, n.pos)).collect();
        let length: f32 = points.windows(2).map(|p| p[0].distance(p[1])).sum();
        assert!((length - 321.0).abs() < 2.0, "{length}");
        assert!(points.iter().all(|p| p.length() < 1000.0));
    }

    /// 노드 ID가 없는 기존 조사 자료(`out tags geom`)는 그래프 자료로 거부하는지 확인한다.
    #[test]
    fn geometry_only_snapshot_is_rejected() {
        for file in [
            "survey-2026-11-06.osm.json",
            "course-way-532526349.osm.json",
        ] {
            let path = format!(
                "{}/assets/maps/yeosu-expo/{file}",
                env!("CARGO_MANIFEST_DIR")
            );
            let json = std::fs::read_to_string(path).unwrap();
            let error = parse_overpass_graph(&json).unwrap_err();
            assert!(error.contains("no node IDs"), "{file}: {error}");
        }
    }

    /// ID·좌표 개수 불일치, 빠진 노드, 잘못된 좌표, 노드 1개, way 없음을 거부하는지 확인한다.
    #[test]
    fn malformed_graph_is_rejected() {
        for (json, expected) in [
            (
                r#"{"elements":[{"type":"way","id":1,"nodes":[1,2],"geometry":[{"lat":34.7,"lon":127.7}]}]}"#,
                "2 node IDs but 1 coordinates",
            ),
            (
                r#"{"elements":[{"type":"way","id":1,"nodes":[1,2],"geometry":[{"lat":34.7,"lon":127.7},null]}]}"#,
                "missing",
            ),
            (
                r#"{"elements":[{"type":"way","id":1,"nodes":[1,2],"geometry":[{"lat":34.7,"lon":127.7},{"lat":95.0,"lon":127.7}]}]}"#,
                "invalid coordinate",
            ),
            (
                r#"{"elements":[{"type":"way","id":1,"nodes":[1],"geometry":[{"lat":34.7,"lon":127.7}]}]}"#,
                "fewer than 2",
            ),
            (r#"{"elements":[{"type":"node","id":1}]}"#, "no ways"),
        ] {
            let error = parse_overpass_graph(json).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    /// 합성 3갈래·4갈래 교차로는 공유 노드 10의 차수로 인식되고, 좌표만 겹치는 교차는 연결로 보지 않는지 확인한다.
    #[test]
    fn fixture_intersections_use_node_ids_only() {
        let parse = |ways: Vec<FixtureWay>| parse_overpass_graph(&overpass_json(&ways)).unwrap();
        assert_eq!(
            node_degrees(&parse(three_way()), &allowed()).get(&10),
            Some(&3)
        );
        assert_eq!(
            node_degrees(&parse(four_way()), &allowed()).get(&10),
            Some(&4)
        );
        let crossing = node_degrees(&parse(crossing_without_shared_node()), &allowed());
        assert!(crossing.values().all(|&d| d <= 2), "{crossing:?}");
        // 허용 목록에 없는 종류는 세지 않는다.
        let only_service: BTreeSet<&str> = ["service"].into_iter().collect();
        assert_eq!(
            node_degrees(&parse(four_way()), &only_service).get(&10),
            Some(&2)
        );
    }

    /// 합성 형상 자료가 의도한 치수로 투영되는지 확인한다(굽은 길 반경, 되돌아오는 길 간격, 넓은 도로 폭 태그).
    #[test]
    fn fixture_shapes_have_intended_geometry() {
        let parse = |ways: Vec<FixtureWay>| parse_overpass_graph(&overpass_json(&ways)).unwrap();
        let curve = parse(curved_road());
        let arc_points: Vec<_> = curve[0].nodes[1..11]
            .iter()
            .map(|n| project(ORIGIN, n.pos))
            .collect();
        // 원호 중심 (-20, 0) → 지역 좌표 (-20, 0)(북쪽 0 → z 0).
        for p in &arc_points {
            assert!(
                (p.distance(bevy::math::Vec2::new(-20.0, 0.0)) - 20.0).abs() < 0.01,
                "{p}"
            );
        }
        // 되돌아오는 길의 마지막 직선은 첫 직선(x=0, 북쪽 0..50 m) 위에 겹치지만 노드 ID는 공유하지 않는다.
        let looped = parse(loop_overlap());
        let last = project(ORIGIN, looped[0].nodes.last().unwrap().pos);
        assert!(
            last.x.abs() < 0.01 && (last.y + 40.0).abs() < 0.01,
            "{last}"
        );
        let ids: BTreeSet<u64> = looped[0].nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), looped[0].nodes.len());
        let wide = parse(wide_short_zigzag());
        assert_eq!(wide[0].tags.get("width").map(String::as_str), Some("12"));
        let s = parse(s_curve());
        assert_eq!(s[0].nodes.len(), 19);
    }
}
