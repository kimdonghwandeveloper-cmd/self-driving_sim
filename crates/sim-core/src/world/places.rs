//! 건물·장소로 출발·도착 고르기(PLAN 4-9).
//!
//! 사용자가 요청할 때만 Overpass로 열린 지역의 건물(`building`)·이름 있는 장소(`name`, 도로 제외)·출입구(`entrance`)
//! 노드를 받는다. 받기·캐시 규칙은 도로와 같다([`map_fetch::Layer`], 캐시 [`PLACES_CACHE_DIR`]).
//! 네이버·카카오 지도 자료는 이용 약관상 쓰지 않고 OSM만 쓴다(ODbL 출처 표기).
//!
//! - 장소: 닫힌 way(건물이거나 이름이 있음)는 윤곽이 있는 장소, 이름 있는 노드는 점 장소다. 다중 다각형 관계는
//!   1차 판에서 읽지 않는다.
//! - 진입 지점([`access_point`]): 출입구 노드가 있으면 그중, 없으면 윤곽(점 장소는 그 점)에서 가장 가까운 탐색 가능
//!   도로 위 점이다. [`MAX_ACCESS_DISTANCE_M`]보다 멀면 거부한다.
//! - 경로 파일에는 진입 지점을 점으로 저장하고 장소 이름은 표시용으로만 남긴다. 코스 생성은 장소 자료 없이 된다.
//! - 한국 OSM은 건물 이름·주소가 빈 곳이 많다. 주소 검색(지오코딩)은 하지 않는다.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::map::LatLon;
use super::map_fetch::{self, Bbox, Layer};
use super::osm_graph::OsmGraphWay;
use super::route_plan::{self, RoadGraphConfig};

/// 세대 폴더 안 장소 원본 파일 이름.
pub const PLACES_FILE: &str = "places.osm.json";
/// 장소 자료 캐시 폴더(저장소가 추적하지 않음). 구조는 도로 캐시와 같다.
pub const PLACES_CACHE_DIR: &str = "cache/places";
/// 장소에서 진입 지점(도로 위 점)까지 허용하는 최대 거리(m). 임시값.
pub const MAX_ACCESS_DISTANCE_M: f64 = 100.0;
/// 이름 검색 결과 최대 개수.
pub const MAX_SEARCH_RESULTS: usize = 20;
/// 점 장소를 클릭으로 고를 수 있는 거리(m).
pub const NODE_HIT_M: f64 = 15.0;
/// 윤곽에서 진입 지점을 찾을 때 변을 나누는 간격(m).
const OUTLINE_STEP_M: f64 = 2.0;
/// 장소 종류를 정할 때 보는 태그(앞선 것이 우선).
const KIND_KEYS: [&str; 9] = [
    "amenity", "shop", "tourism", "leisure", "office", "historic", "building", "place", "man_made",
];

/// 건물·장소 자료(PLAN 4-9). 원본은 [`PLACES_FILE`]이다.
pub const PLACES: Layer = Layer {
    body_file: PLACES_FILE,
    query: places_query,
    check: check_places,
};

/// 장소 Overpass 질의문: 건물 way, 이름 있는 way·노드(도로 제외), 출입구 노드를 `out body geom`으로 받는다.
pub fn places_query(bbox: &Bbox, timeout_s: u64) -> String {
    let area = format!(
        "({:.7},{:.7},{:.7},{:.7})",
        bbox.south, bbox.west, bbox.north, bbox.east
    );
    format!(
        "[out:json][timeout:{timeout_s}];\n(\n  way[\"building\"]{area};\n  way[\"name\"][!\"highway\"]{area};\n  node[\"name\"][!\"highway\"]{area};\n  node[\"entrance\"]{area};\n);\nout body geom;"
    )
}

/// 장소 응답을 검사하고 `timestamp_osm_base`를 꺼낸다.
pub fn check_places(body: &[u8]) -> Result<String, String> {
    let (text, timestamp) = map_fetch::check_header(body)?;
    parse_places(text)?;
    Ok(timestamp)
}

/// 건물 또는 장소 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// OSM 요소(`way/<ID>` 또는 `node/<ID>`).
    pub id: String,
    /// 이름(`name`). 없으면 빈 문자열.
    pub name: String,
    /// 종류(`amenity=cafe`, `building` 등).
    pub kind: String,
    /// 윤곽(닫는 점 중복 없음). 점 장소는 비어 있다.
    pub outline: Vec<LatLon>,
    /// 윤곽 꼭짓점의 평균(점 장소는 그 점).
    pub center: LatLon,
    /// 윤곽 위 출입구 노드 위치.
    pub entrances: Vec<LatLon>,
}

impl Place {
    /// 화면·경로 파일에 쓸 이름. 이름이 없으면 OSM 요소 이름이다.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }
}

/// 장소 응답 중 필요한 부분.
#[derive(Debug, Deserialize)]
struct PlacesResponse {
    elements: Vec<PlaceElement>,
}

/// 장소 응답 요소(노드·way). 관계는 무시한다.
#[derive(Debug, Deserialize)]
struct PlaceElement {
    #[serde(rename = "type")]
    kind: String,
    id: u64,
    /// 노드 위도.
    lat: Option<f64>,
    /// 노드 경도.
    lon: Option<f64>,
    /// way 노드 ID.
    #[serde(default)]
    nodes: Vec<u64>,
    /// way 노드 좌표.
    #[serde(default)]
    geometry: Vec<Option<LatLon>>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

/// 좌표가 유한하고 범위 안인지 확인한다.
fn valid(point: LatLon) -> bool {
    point.lat.is_finite()
        && point.lon.is_finite()
        && point.lat.abs() <= 90.0
        && point.lon.abs() <= 180.0
}

/// 태그로 장소 종류를 정한다. 아무 종류 태그도 없으면 빈 문자열.
fn place_kind(tags: &BTreeMap<String, String>) -> String {
    KIND_KEYS
        .iter()
        .find_map(|key| {
            tags.get(*key).map(|value| match value.as_str() {
                "yes" => (*key).to_owned(),
                _ => format!("{key}={value}"),
            })
        })
        .unwrap_or_default()
}

/// 장소 응답(Overpass JSON)에서 장소 목록을 읽는다. OSM 요소 이름 순으로 정렬한다.
///
/// 윤곽 way는 닫혀 있고(첫 노드 = 끝 노드) 꼭짓점이 3개 이상이며 좌표가 모두 있어야 한다. 맞지 않는 way와
/// 좌표가 잘못된 노드는 건너뛴다(영역 가장자리에서 잘린 자료 등). JSON 자체가 잘못되면 오류다.
pub fn parse_places(json: &str) -> Result<Vec<Place>, String> {
    let response: PlacesResponse =
        serde_json::from_str(json).map_err(|error| format!("invalid places JSON: {error}"))?;
    let point = |e: &PlaceElement| match (e.lat, e.lon) {
        (Some(lat), Some(lon)) => Some(LatLon { lat, lon }).filter(|p| valid(*p)),
        _ => None,
    };
    let entrances: BTreeMap<u64, LatLon> = response
        .elements
        .iter()
        .filter(|e| e.kind == "node" && e.tags.contains_key("entrance"))
        .filter_map(|e| point(e).map(|p| (e.id, p)))
        .collect();
    let mut places = Vec::new();
    for element in &response.elements {
        let name = element.tags.get("name").cloned().unwrap_or_default();
        match element.kind.as_str() {
            "node" if !name.is_empty() => {
                if let Some(at) = point(element) {
                    places.push(Place {
                        id: format!("node/{}", element.id),
                        name,
                        kind: place_kind(&element.tags),
                        outline: Vec::new(),
                        center: at,
                        entrances: Vec::new(),
                    });
                }
            }
            "way" => {
                if name.is_empty() && !element.tags.contains_key("building") {
                    continue;
                }
                let ids = &element.nodes;
                let closed = ids.len() >= 4 && ids.first() == ids.last();
                if !closed || ids.len() != element.geometry.len() {
                    continue;
                }
                let Some(mut outline) = element
                    .geometry
                    .iter()
                    .map(|p| p.filter(|p| valid(*p)))
                    .collect::<Option<Vec<LatLon>>>()
                else {
                    continue;
                };
                outline.pop();
                let count = outline.len() as f64;
                let center = LatLon {
                    lat: outline.iter().map(|p| p.lat).sum::<f64>() / count,
                    lon: outline.iter().map(|p| p.lon).sum::<f64>() / count,
                };
                let doors: BTreeSet<u64> = ids.iter().copied().collect();
                places.push(Place {
                    id: format!("way/{}", element.id),
                    name,
                    kind: place_kind(&element.tags),
                    outline,
                    center,
                    entrances: doors
                        .iter()
                        .filter_map(|id| entrances.get(id).copied())
                        .collect(),
                });
            }
            _ => {}
        }
    }
    places.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(places)
}

/// 장소 파일을 읽는다.
pub fn load_places(path: &std::path::Path) -> Result<Vec<Place>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_places(&text)
}

/// 이름에 `query`가 들어 있는(대소문자 무시) 장소 번호를 이름이 짧은 순으로 최대 [`MAX_SEARCH_RESULTS`]개 돌려준다.
/// 빈 검색어는 결과가 없다.
pub fn search(places: &[Place], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<usize> = (0..places.len())
        .filter(|&i| places[i].name.to_lowercase().contains(&query))
        .collect();
    found.sort_by(|&a, &b| {
        let (a, b) = (&places[a], &places[b]);
        (a.name.chars().count(), &a.name, &a.id).cmp(&(b.name.chars().count(), &b.name, &b.id))
    });
    found.truncate(MAX_SEARCH_RESULTS);
    found
}

/// 점이 다각형(지역 좌표) 안에 있는지 판정한다(짝수-홀수 규칙).
fn inside(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut result = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
        j = i;
    }
    result
}

/// 다각형(지역 좌표)의 넓이(m²).
fn area(polygon: &[[f64; 2]]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

/// 위경도 `point`를 가리키는 장소 번호. 윤곽 안이면 그중 가장 작은 건물, 아니면 [`NODE_HIT_M`] 안의 가장 가까운
/// 점 장소다. 없으면 `None`.
pub fn hit_test(places: &[Place], point: LatLon) -> Option<usize> {
    let local = |p: LatLon| route_plan::local(point, p);
    let inside_best = places
        .iter()
        .enumerate()
        .filter(|(_, place)| place.outline.len() >= 3)
        .filter_map(|(i, place)| {
            let polygon: Vec<[f64; 2]> = place.outline.iter().map(|p| local(*p)).collect();
            inside([0.0, 0.0], &polygon).then(|| (i, area(&polygon)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = inside_best {
        return Some(i);
    }
    places
        .iter()
        .enumerate()
        .filter(|(_, place)| place.outline.is_empty())
        .map(|(i, place)| {
            let [x, y] = local(place.center);
            (i, x.hypot(y))
        })
        .filter(|(_, d)| *d <= NODE_HIT_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// 선분 `a`–`b` 위에서 `p`에 가장 가까운 점.
fn closest_on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length2 = d[0] * d[0] + d[1] * d[1];
    let t = if length2 > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

/// 장소의 진입 지점(가장 가까운 탐색 가능 도로 위 점, 위경도)을 구한다.
///
/// 출입구 노드가 있으면 그 점들, 없으면 윤곽 변을 [`OUTLINE_STEP_M`] 간격으로 나눈 점들(점 장소는 그 점)에서
/// 경로 탐색에 쓰는 도로([`route_plan::searchable`])까지 가장 가까운 곳을 고른다. 같은 거리면 앞선 way·구간이다.
/// [`MAX_ACCESS_DISTANCE_M`]보다 멀면 오류다.
pub fn access_point(
    place: &Place,
    ways: &[OsmGraphWay],
    config: &RoadGraphConfig,
) -> Result<LatLon, String> {
    let origin = place.center;
    let local = |p: LatLon| route_plan::local(origin, p);
    let sources: Vec<[f64; 2]> = if !place.entrances.is_empty() {
        place.entrances.iter().map(|p| local(*p)).collect()
    } else if place.outline.len() >= 3 {
        let corners: Vec<[f64; 2]> = place.outline.iter().map(|p| local(*p)).collect();
        let mut samples = Vec::new();
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            let steps = (length / OUTLINE_STEP_M).ceil().max(1.0) as usize;
            samples.extend((0..steps).map(|k| {
                let t = k as f64 / steps as f64;
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            }));
        }
        samples
    } else {
        vec![[0.0, 0.0]]
    };
    let allowed: BTreeSet<&str> = config.allowed_highways.iter().map(String::as_str).collect();
    let reach =
        MAX_ACCESS_DISTANCE_M + sources.iter().map(|p| p[0].hypot(p[1])).fold(0.0, f64::max);
    let mut best: Option<(f64, [f64; 2])> = None;
    for way in ways
        .iter()
        .filter(|way| route_plan::searchable(way, &allowed, config.nav_rules))
    {
        for pair in way.nodes.windows(2) {
            let (a, b) = (local(pair[0].pos), local(pair[1].pos));
            // 장소에서 아주 먼 구간은 건너뛴다(끝점이 모두 같은 쪽으로 멀면 선분 전체가 멀다).
            if a[0].min(b[0]) > reach
                || a[0].max(b[0]) < -reach
                || a[1].min(b[1]) > reach
                || a[1].max(b[1]) < -reach
            {
                continue;
            }
            for &source in &sources {
                let q = closest_on_segment(source, a, b);
                let gap = (q[0] - source[0]).hypot(q[1] - source[1]);
                if best.is_none_or(|(known, _)| gap < known) {
                    best = Some((gap, q));
                }
            }
        }
    }
    let label = place.label();
    let (gap, q) = best.ok_or_else(|| {
        format!("no drivable road within the {MAX_ACCESS_DISTANCE_M} m limit of place `{label}`")
    })?;
    if gap > MAX_ACCESS_DISTANCE_M {
        return Err(format!(
            "place `{label}` is {gap:.0} m from the nearest drivable road (limit {MAX_ACCESS_DISTANCE_M} m)"
        ));
    }
    Ok(route_plan::unlocal(origin, q))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::fixtures::{self, FixtureWay};
    use crate::world::osm_graph::parse_overpass_graph;

    /// 합성 장소 JSON: (요소 종류, ID, 태그, 노드 (ID, 동쪽 m, 북쪽 m) 목록). 노드는 점 하나, way는 윤곽이다.
    type FixturePlace<'a> = (&'a str, u64, &'a [(&'a str, &'a str)], Vec<(u64, f64, f64)>);

    /// 합성 장소 목록을 Overpass `out body geom` 형식 JSON으로 만든다.
    fn places_json(items: &[FixturePlace]) -> String {
        let elements: Vec<String> = items
            .iter()
            .map(|(kind, id, tags, nodes)| {
                let tags: Vec<String> = tags
                    .iter()
                    .map(|(k, v)| format!("\"{k}\":\"{v}\""))
                    .collect();
                let tags = tags.join(",");
                if *kind == "node" {
                    let p = fixtures::offset(nodes[0].1, nodes[0].2);
                    format!(
                        "{{\"type\":\"node\",\"id\":{id},\"lat\":{},\"lon\":{},\"tags\":{{{tags}}}}}",
                        p.lat, p.lon
                    )
                } else {
                    let ids: Vec<String> = nodes.iter().map(|(n, _, _)| n.to_string()).collect();
                    let geometry: Vec<String> = nodes
                        .iter()
                        .map(|(_, e, n)| {
                            let p = fixtures::offset(*e, *n);
                            format!("{{\"lat\":{},\"lon\":{}}}", p.lat, p.lon)
                        })
                        .collect();
                    format!(
                        "{{\"type\":\"way\",\"id\":{id},\"nodes\":[{}],\"geometry\":[{}],\"tags\":{{{tags}}}}}",
                        ids.join(","),
                        geometry.join(",")
                    )
                }
            })
            .collect();
        format!(
            "{{\"osm3s\":{{\"timestamp_osm_base\":\"2026-11-11T00:00:00Z\"}},\"elements\":[{}]}}",
            elements.join(",")
        )
    }

    /// 사각형 윤곽 노드 목록(닫는 점 포함): 중심 (e, n), 반 변 `half`(m), 노드 ID는 `first`부터.
    fn square(first: u64, e: f64, n: f64, half: f64) -> Vec<(u64, f64, f64)> {
        vec![
            (first, e - half, n - half),
            (first + 1, e + half, n - half),
            (first + 2, e + half, n + half),
            (first + 3, e - half, n + half),
            (first, e - half, n - half),
        ]
    }

    /// 동서 도로 하나(y = 0, -200..200 m)와 북쪽 일방통행 아닌 service 길 하나.
    fn roads() -> Vec<OsmGraphWay> {
        let ways: Vec<FixtureWay> = vec![
            (
                1,
                "residential",
                &[],
                vec![(1, -200.0, 0.0), (2, 200.0, 0.0)],
            ),
            (2, "footway", &[], vec![(3, -200.0, 30.0), (4, 200.0, 30.0)]),
        ];
        parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap()
    }

    /// 읽기: 건물·이름 있는 way·이름 있는 노드를 장소로, 출입구를 윤곽에 붙이고, 이름 없는 비건물·열린 way는 뺀다.
    #[test]
    fn places_are_parsed_with_entrances() {
        let mut door_way = square(10, 0.0, 40.0, 10.0);
        door_way.insert(1, (99, 0.0, 30.0));
        let json = places_json(&[
            (
                "way",
                500,
                &[("building", "yes"), ("name", "도서관")],
                door_way,
            ),
            ("node", 99, &[("entrance", "main")], vec![(99, 0.0, 30.0)]),
            (
                "node",
                600,
                &[("name", "버스 정류장"), ("amenity", "bench")],
                vec![(600, 50.0, 5.0)],
            ),
            (
                "way",
                501,
                &[("landuse", "grass")],
                square(20, 80.0, 40.0, 5.0),
            ),
            (
                "way",
                502,
                &[("building", "house")],
                vec![(30, 0.0, 0.0), (31, 5.0, 0.0)],
            ),
        ]);
        let places = parse_places(&json).unwrap();
        let ids: Vec<&str> = places.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["node/600", "way/500"]);
        let library = &places[1];
        assert_eq!(library.kind, "building");
        assert_eq!(library.outline.len(), 5);
        assert_eq!(library.entrances.len(), 1);
        assert_eq!(places[0].kind, "amenity=bench");
        assert!(check_places(json.as_bytes()).is_ok());
        assert!(parse_places("{").is_err());
    }

    /// 진입 지점: 출입구가 있으면 출입구에서, 없으면 윤곽에서 가장 가까운 탐색 가능 도로(보도 제외) 위 점이고,
    /// 너무 멀면 거부한다.
    #[test]
    fn access_point_prefers_entrances_and_drivable_roads() {
        let ways = roads();
        let config = RoadGraphConfig::default();
        let near = |p: LatLon, e: f64, n: f64| {
            let [x, y] = route_plan::local(fixtures::offset(e, n), p);
            x.hypot(y) < 0.05
        };
        // 건물 (40..60, 40..60): 윤곽 남쪽 변에서 도로까지 40 m. 보도(y = 30)는 쓰지 않는다.
        let json = places_json(&[(
            "way",
            1,
            &[("building", "yes")],
            square(10, 50.0, 50.0, 10.0),
        )]);
        let building = &parse_places(&json).unwrap()[0];
        let at = access_point(building, &ways, &config).unwrap();
        assert!(near(at, 40.0, 0.0), "{at:?}");
        // 출입구가 동쪽 끝(60, 50)에 있으면 그 아래 도로 점(60, 0)이다.
        let mut outline = square(10, 50.0, 50.0, 10.0);
        outline.insert(2, (77, 60.0, 50.0));
        let json = places_json(&[
            ("way", 1, &[("building", "yes")], outline),
            ("node", 77, &[("entrance", "yes")], vec![(77, 60.0, 50.0)]),
        ]);
        let with_door = &parse_places(&json).unwrap()[0];
        let at = access_point(with_door, &ways, &config).unwrap();
        assert!(near(at, 60.0, 0.0), "{at:?}");
        // 도로에서 150 m 떨어진 점 장소는 거부한다.
        let json = places_json(&[("node", 5, &[("name", "먼 곳")], vec![(5, 0.0, 150.0)])]);
        let far = &parse_places(&json).unwrap()[0];
        let error = access_point(far, &ways, &config).unwrap_err();
        assert!(
            error.contains("먼 곳") && error.contains("limit"),
            "{error}"
        );
    }

    /// 검색은 이름 포함(대소문자 무시)·짧은 이름 순이고, 클릭은 윤곽 안의 가장 작은 건물 또는 가까운 점 장소를 고른다.
    #[test]
    fn search_and_hit_test() {
        let json = places_json(&[
            (
                "way",
                1,
                &[("building", "yes"), ("name", "Expo Hall")],
                square(10, 0.0, 50.0, 30.0),
            ),
            (
                "way",
                2,
                &[("building", "yes"), ("name", "Expo Hall Annex")],
                square(20, 0.0, 50.0, 5.0),
            ),
            ("node", 3, &[("name", "expo gate")], vec![(3, 100.0, 50.0)]),
        ]);
        let places = parse_places(&json).unwrap();
        let names = |found: Vec<usize>| -> Vec<String> {
            found.into_iter().map(|i| places[i].name.clone()).collect()
        };
        assert_eq!(
            names(search(&places, "EXPO")),
            ["Expo Hall", "expo gate", "Expo Hall Annex"]
        );
        assert!(search(&places, "  ").is_empty());
        let label = |point: LatLon| hit_test(&places, point).map(|i| places[i].name.clone());
        assert_eq!(
            label(fixtures::offset(1.0, 51.0)).as_deref(),
            Some("Expo Hall Annex")
        );
        assert_eq!(
            label(fixtures::offset(20.0, 60.0)).as_deref(),
            Some("Expo Hall")
        );
        assert_eq!(
            label(fixtures::offset(108.0, 50.0)).as_deref(),
            Some("expo gate")
        );
        assert_eq!(label(fixtures::offset(200.0, 200.0)), None);
    }

    /// 장소 질의문에 건물·이름·출입구 조건과 범위가 들어 있다.
    #[test]
    fn places_query_lists_buildings_names_and_entrances() {
        let bbox = Bbox {
            south: 34.748,
            west: 127.746,
            north: 34.759,
            east: 127.76,
        };
        let query = places_query(&bbox, 90);
        for part in [
            "way[\"building\"](34.7480000,127.7460000,34.7590000,127.7600000);",
            "node[\"name\"][!\"highway\"]",
            "node[\"entrance\"]",
            "out body geom;",
        ] {
            assert!(query.contains(part), "{query}");
        }
    }
}
