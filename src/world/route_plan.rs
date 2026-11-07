//! 도로 그래프와 경로 탐색, 경로 파일(Phase 4-3).
//!
//! 사용자가 찍은 점(시작·경유·끝, 위경도)을 가장 가까운 허용 도로 위 점에 붙이고, 이웃한 점 사이를 A*로 찾아
//! OSM 노드 ID 순서의 경로를 만든다. 결과는 경로 파일(TOML, [`RouteFile`])로 저장하며, `--course map:` 로더가
//! 이 파일과 저장된 스냅샷만으로 같은 코스를 다시 만든다(코스 생성은 [`route::build_route_course_with`]).
//!
//! - 도로 그래프: 허용 `highway` 종류의 양방향 way를 같은 노드 ID에서 나눈 구간으로 잇는다. 일방통행 way는
//!   1차 판에서 지원하지 않으므로 경로 탐색에서 뺀다(교차로 팔로 붙으면 4-2 규칙대로 거부된다).
//! - 점 붙이기: 찍은 점마다 [`RoadGraphConfig::snap_distance_m`] 안의 가장 가까운 구간 위 점을 고른다.
//!   기존 노드에서 [`NODE_SNAP_M`] 안이면 그 노드를, 아니면 구간 사이에 **가상 노드**를 넣는다.
//!   가상 노드 ID는 [`VIRTUAL_NODE_BASE`] + 점 번호다(OSM 노드가 아니다).
//! - 폭: `width` 태그 → `lanes` × 차로 폭 → 도로 종류별 기본값 순서로 정하고, 경로의 way 중 가장 좁은 값을
//!   코스 전체에 쓴다. 태그가 아닌 값은 임시 폭으로 화면에 표시한다. 모든 기본값은 **실측이 아니다**.
//! - 범위: 찍은 점과 경로 노드는 대한민국 근사 사각형([`KOREA_LAT`]·[`KOREA_LON`]) 안이어야 하고, 경로 길이는
//!   [`RoadGraphConfig::max_route_length_m`] 이하여야 한다. 코스 범위 상한은 [`route::ROUTE_MAX_EXTENT_M`]이다.
//! - 재생 계약: 계획에 쓴 점 붙이기 거리·경로 길이 상한을 경로 파일에 함께 저장하고, 다시 읽을 때도 같은 값으로
//!   검사한다. 두 값은 유한한 양수이고 각각 [`SNAP_DISTANCE_CAP_M`]·[`ROUTE_LENGTH_CAP_M`] 이하여야 한다.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::course::CourseDef;
use super::map::{self, LatLon};
use super::osm_graph::{OsmGraphWay, OsmNode, parse_overpass_graph};
use super::route::RouteCourse;
use super::route::{self, JunctionSettings};

/// 도로 그래프 설정 파일 경로.
pub const ROAD_GRAPH_CONFIG_PATH: &str = "config/road_graph.toml";
/// 경로 파일 형식 이름.
pub const ROUTE_FORMAT: &str = "route-v1";
/// 대한민국 범위 근사 사각형의 위도 범위(deg). 주변 해역·인접 지역 일부를 포함하는 근사다.
pub const KOREA_LAT: std::ops::RangeInclusive<f64> = 33.0..=38.7;
/// 대한민국 범위 근사 사각형의 경도 범위(deg).
pub const KOREA_LON: std::ops::RangeInclusive<f64> = 124.5..=132.0;
/// 가상 노드 ID 시작값(2^62). 이 값 이상인 경로 노드는 찍은 점을 도로에 붙인 점이다.
pub const VIRTUAL_NODE_BASE: u64 = 1 << 62;
/// 찍은 점을 붙인 곳이 기존 노드에서 이 거리(m) 안이면 가상 노드 대신 그 노드를 쓴다.
pub const NODE_SNAP_M: f64 = 1.0;
/// 점 붙이기 거리의 절대 상한(m). 설정·경로 파일 값이 이보다 크면 거부한다.
pub const SNAP_DISTANCE_CAP_M: f64 = 100.0;
/// 경로 길이의 절대 상한(m). PLAN 4 공통 계약의 2 km.
pub const ROUTE_LENGTH_CAP_M: f64 = 2000.0;
/// 지구 평균 반지름(m). [`map::project`]와 같은 값.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// `config/road_graph.toml` 전체 구조. 경로를 **계획할 때** 쓰는 값이며, 결과는 경로 파일에 함께 저장한다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadGraphConfig {
    /// 도로 그래프에 넣을 `highway` 종류.
    pub allowed_highways: Vec<String>,
    /// `lanes` 태그로 폭을 정할 때 쓰는 차로 폭(m). 임시값.
    pub lane_width_m: f32,
    /// 도로 종류별 기본 폭(m). 임시값. 허용 종류마다 있어야 한다.
    pub default_width_m: BTreeMap<String, f32>,
    /// 찍은 점을 도로에 붙일 수 있는 최대 거리(m).
    pub snap_distance_m: f64,
    /// 경로 길이 상한(m).
    pub max_route_length_m: f64,
    /// 교차로 모서리 원호 반경(m). 임시값.
    pub corner_radius_m: f32,
    /// 곁가지 길이(m). 임시값.
    pub stub_length_m: f32,
}

impl Default for RoadGraphConfig {
    /// 저장소의 `config/road_graph.toml`과 같은 기본값(모두 임시값, 실측 아님).
    fn default() -> Self {
        let kinds = [
            ("service", 4.0),
            ("pedestrian", 4.0),
            ("residential", 5.0),
            ("unclassified", 5.0),
            ("tertiary", 6.0),
        ];
        Self {
            allowed_highways: kinds.iter().map(|(kind, _)| (*kind).to_owned()).collect(),
            lane_width_m: 3.0,
            default_width_m: kinds
                .iter()
                .map(|(kind, width)| ((*kind).to_owned(), *width))
                .collect(),
            snap_distance_m: 30.0,
            max_route_length_m: 2000.0,
            corner_radius_m: route::CORNER_RADIUS_M,
            stub_length_m: route::STUB_LENGTH_M,
        }
    }
}

impl RoadGraphConfig {
    /// 값의 범위와 허용 종류별 기본 폭이 모두 있는지 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        if self.allowed_highways.is_empty() {
            return Err("allowed_highways is empty".to_owned());
        }
        for kind in &self.allowed_highways {
            let width = self
                .default_width_m
                .get(kind)
                .ok_or_else(|| format!("default_width_m has no value for `{kind}`"))?;
            map::check_width(*width)?;
        }
        let lane = f64::from(self.lane_width_m);
        if !(lane.is_finite() && lane > 0.0) {
            return Err(format!(
                "lane_width_m must be positive, got {}",
                self.lane_width_m
            ));
        }
        check_limits(self.snap_distance_m, self.max_route_length_m)?;
        self.junction_settings().validate()
    }

    /// 교차로 형상 설정값.
    pub fn junction_settings(&self) -> JunctionSettings {
        JunctionSettings {
            corner_radius_m: self.corner_radius_m,
            stub_length_m: self.stub_length_m,
        }
    }
}

/// 점 붙이기 거리와 경로 길이 상한이 유한한 양수이고 절대 상한 이하인지 검사한다(설정·경로 파일 공통).
fn check_limits(snap_distance_m: f64, max_route_length_m: f64) -> Result<(), String> {
    for (name, value, cap) in [
        ("snap_distance_m", snap_distance_m, SNAP_DISTANCE_CAP_M),
        ("max_route_length_m", max_route_length_m, ROUTE_LENGTH_CAP_M),
    ] {
        if !(value.is_finite() && value > 0.0 && value <= cap) {
            return Err(format!(
                "{name} must be a positive number up to {cap} m, got {value}"
            ));
        }
    }
    Ok(())
}

/// 설정 파일 내용을 읽고 검사한다.
pub fn parse_road_graph_config(text: &str) -> Result<RoadGraphConfig, String> {
    let config: RoadGraphConfig =
        toml::from_str(text).map_err(|error| format!("invalid road graph config: {error}"))?;
    config.validate()?;
    Ok(config)
}

/// 경로 파일(TOML). 저장된 스냅샷과 이 파일만으로 같은 코스를 다시 만들 수 있도록 생성에 쓰는 값을 모두 담는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteFile {
    /// 형식 이름([`ROUTE_FORMAT`]).
    pub format: String,
    /// 노드 ID를 보존한 스냅샷(`out body geom`) 경로. 이 파일 위치 기준 상대 경로다.
    pub snapshot: String,
    /// 도로 그래프에 넣은 `highway` 종류.
    pub allowed_highways: Vec<String>,
    /// 사용자가 찍은 점 [위도, 경도](deg). 시작·경유·끝 순서.
    pub points: Vec<[f64; 2]>,
    /// 경로 노드 ID 순서. [`VIRTUAL_NODE_BASE`] 이상은 찍은 점을 붙인 가상 노드다.
    pub nodes: Vec<u64>,
    /// 코스 전체에 쓰는 폭(m).
    pub width_m: f32,
    /// 폭의 출처: `tag`(OSM `width`), `lanes`(차로 수 × 차로 폭, 임시), `default`(종류별 기본값, 임시).
    pub width_source: String,
    /// 교차로 모서리 원호 반경(m). 임시값.
    pub corner_radius_m: f32,
    /// 곁가지 길이(m). 임시값.
    pub stub_length_m: f32,
    /// 계획에 쓴 점 붙이기 거리(m). 다시 읽을 때도 찍은 점이 이 거리 안의 도로에 붙어야 한다.
    pub snap_distance_m: f64,
    /// 계획에 쓴 경로 길이 상한(m). 다시 읽을 때도 검사한다.
    pub max_route_length_m: f64,
}

/// 폭을 정한 출처.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthSource {
    /// OSM `width` 태그.
    Tag,
    /// `lanes` 태그 × 차로 폭(임시).
    Lanes,
    /// 도로 종류별 기본값(임시).
    Default,
}

impl WidthSource {
    /// 경로 파일에 쓰는 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Lanes => "lanes",
            Self::Default => "default",
        }
    }
}

/// way 하나의 폭을 `width` 태그 → `lanes` × 차로 폭 → 종류별 기본값 순서로 정한다.
///
/// `width`는 숫자 또는 숫자 뒤 `m`(예: `4`, `4.5 m`)만 읽는다. 읽을 수 없거나 0 이하이면 다음 순서로 넘어간다.
pub fn resolve_width(
    way: &OsmGraphWay,
    config: &RoadGraphConfig,
) -> Result<(f32, WidthSource), String> {
    let number = |key: &str| -> Option<f32> {
        let text = way.tags.get(key)?.trim();
        let text = text.strip_suffix('m').unwrap_or(text).trim();
        text.parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
    };
    if let Some(width) = number("width") {
        return Ok((width, WidthSource::Tag));
    }
    if let Some(lanes) = number("lanes").filter(|lanes| lanes.fract() == 0.0) {
        return Ok((lanes * config.lane_width_m, WidthSource::Lanes));
    }
    let kind = way.highway().unwrap_or("");
    config
        .default_width_m
        .get(kind)
        .map(|width| (*width, WidthSource::Default))
        .ok_or_else(|| format!("way {}: no default width for highway `{kind}`", way.id))
}

/// 위경도가 대한민국 근사 사각형 안인지 확인한다.
pub fn in_korea(point: LatLon) -> bool {
    KOREA_LAT.contains(&point.lat) && KOREA_LON.contains(&point.lon)
}

/// 기준점에서 본 지역 평면 좌표(동쪽 m, 북쪽 m)를 f64로 구한다. 점 붙이기와 경로 탐색에 쓴다.
fn local(origin: LatLon, point: LatLon) -> [f64; 2] {
    let cos_lat0 = origin.lat.to_radians().cos();
    [
        EARTH_RADIUS_M * (point.lon - origin.lon).to_radians() * cos_lat0,
        EARTH_RADIUS_M * (point.lat - origin.lat).to_radians(),
    ]
}

/// [`local`]의 역변환.
fn unlocal(origin: LatLon, [east, north]: [f64; 2]) -> LatLon {
    LatLon {
        lat: origin.lat + (north / EARTH_RADIUS_M).to_degrees(),
        lon: origin.lon + (east / (EARTH_RADIUS_M * origin.lat.to_radians().cos())).to_degrees(),
    }
}

/// 두 지역 좌표 사이 거리(m).
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 경로 탐색에 쓰는 way인지(허용 종류이고 일방통행이 아님) 판정한다.
fn searchable(way: &OsmGraphWay, allowed: &BTreeSet<&str>) -> bool {
    way.highway().is_some_and(|kind| allowed.contains(kind)) && !route::is_oneway(way)
}

/// 찍은 점들을 도로에 붙인 결과.
struct Snapped {
    /// 가상 노드를 넣은 way 목록(원본 순서 유지).
    ways: Vec<OsmGraphWay>,
    /// 점마다 붙은 노드 ID.
    nodes: Vec<u64>,
}

/// 찍은 점들을 가장 가까운 탐색 가능한 구간 위 점에 붙인다. `max_distance_m`보다 멀면 오류다.
///
/// 같은 거리면 way 순서·구간 순서가 앞선 쪽을 고른다(결과가 항상 같다).
fn snap_points(
    ways: &[OsmGraphWay],
    allowed: &BTreeSet<&str>,
    points: &[LatLon],
    max_distance_m: f64,
) -> Result<Snapped, String> {
    let origin = points[0];
    // (way 번호, 구간 번호) → (구간 위 비율, 가상 노드).
    let mut inserts: BTreeMap<(usize, usize), Vec<(f64, OsmNode)>> = BTreeMap::new();
    let mut nodes = Vec::with_capacity(points.len());
    for (k, &point) in points.iter().enumerate() {
        let p = local(origin, point);
        let mut best: Option<(f64, usize, usize, f64)> = None;
        for (w, way) in ways.iter().enumerate() {
            if !searchable(way, allowed) {
                continue;
            }
            for s in 0..way.nodes.len() - 1 {
                let (a, b) = (
                    local(origin, way.nodes[s].pos),
                    local(origin, way.nodes[s + 1].pos),
                );
                let d = [b[0] - a[0], b[1] - a[1]];
                let length2 = d[0] * d[0] + d[1] * d[1];
                let t = if length2 > 0.0 {
                    (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let gap = distance(p, [a[0] + d[0] * t, a[1] + d[1] * t]);
                if best.is_none_or(|(g, ..)| gap < g) {
                    best = Some((gap, w, s, t));
                }
            }
        }
        let (gap, w, s, t) = best.ok_or("no allowed two-way roads in the snapshot")?;
        if gap > max_distance_m {
            return Err(format!(
                "point #{k} {:?} is {gap:.0} m from the nearest allowed road (limit {max_distance_m} m)",
                [point.lat, point.lon]
            ));
        }
        let way = &ways[w];
        let (a, b) = (
            local(origin, way.nodes[s].pos),
            local(origin, way.nodes[s + 1].pos),
        );
        let length = distance(a, b);
        if t * length <= NODE_SNAP_M {
            nodes.push(way.nodes[s].id);
        } else if (1.0 - t) * length <= NODE_SNAP_M {
            nodes.push(way.nodes[s + 1].id);
        } else {
            let id = VIRTUAL_NODE_BASE + k as u64;
            let pos = unlocal(origin, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            inserts
                .entry((w, s))
                .or_default()
                .push((t, OsmNode { id, pos }));
            nodes.push(id);
        }
    }
    let mut ways = ways.to_vec();
    // 뒤 구간부터 넣어 앞 구간 번호가 밀리지 않게 한다.
    for ((w, s), mut list) in inserts.into_iter().rev() {
        list.sort_by(|x, y| x.0.total_cmp(&y.0));
        let at = s + 1;
        ways[w]
            .nodes
            .splice(at..at, list.into_iter().map(|(_, node)| node));
    }
    Ok(Snapped { ways, nodes })
}

/// A* 우선순위 큐 항목(추정 총비용이 작은 것, 같으면 노드 ID가 작은 것이 먼저).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    /// 지금까지 비용 + 남은 직선 거리(m).
    estimate: f64,
    /// 노드 ID.
    node: u64,
}

impl Eq for Open {}

impl Ord for Open {
    /// [`BinaryHeap`]이 최대 힙이므로 순서를 뒤집는다.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for Open {
    /// [`Ord`]와 같은 순서.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 탐색 가능한 way로 만든 노드 연결(이웃 노드, 구간 길이 m)과 노드 좌표.
struct SearchGraph {
    /// 노드 ID별 지역 좌표.
    pos: BTreeMap<u64, [f64; 2]>,
    /// 노드 ID별 이웃과 구간 길이.
    adjacent: BTreeMap<u64, Vec<(u64, f64)>>,
}

impl SearchGraph {
    /// 탐색 가능한 way를 같은 노드 ID에서 나눈 구간으로 잇는다.
    fn new(ways: &[OsmGraphWay], allowed: &BTreeSet<&str>, origin: LatLon) -> Self {
        let mut pos = BTreeMap::new();
        let mut adjacent: BTreeMap<u64, Vec<(u64, f64)>> = BTreeMap::new();
        for way in ways.iter().filter(|way| searchable(way, allowed)) {
            for node in &way.nodes {
                pos.insert(node.id, local(origin, node.pos));
            }
            for pair in way.nodes.windows(2) {
                let length = distance(local(origin, pair[0].pos), local(origin, pair[1].pos));
                for (from, to) in [(pair[0].id, pair[1].id), (pair[1].id, pair[0].id)] {
                    adjacent.entry(from).or_default().push((to, length));
                }
            }
        }
        Self { pos, adjacent }
    }

    /// `from`에서 `to`까지 가장 짧은 노드 순서를 A*(직선 거리 추정)로 찾는다. 이어지지 않으면 오류다.
    fn find_path(&self, from: u64, to: u64) -> Result<Vec<u64>, String> {
        let goal = self.pos[&to];
        let mut best: BTreeMap<u64, f64> = [(from, 0.0)].into_iter().collect();
        let mut came: BTreeMap<u64, u64> = BTreeMap::new();
        let mut closed = BTreeSet::new();
        let mut open = BinaryHeap::from([Open {
            estimate: distance(self.pos[&from], goal),
            node: from,
        }]);
        while let Some(Open { node, .. }) = open.pop() {
            if node == to {
                let mut path = vec![to];
                while let Some(&previous) = came.get(path.last().unwrap()) {
                    path.push(previous);
                }
                path.reverse();
                return Ok(path);
            }
            if !closed.insert(node) {
                continue;
            }
            for &(next, length) in self.adjacent.get(&node).into_iter().flatten() {
                let cost = best[&node] + length;
                if best.get(&next).is_none_or(|&known| cost < known) {
                    best.insert(next, cost);
                    came.insert(next, node);
                    open.push(Open {
                        estimate: cost + distance(self.pos[&next], goal),
                        node: next,
                    });
                }
            }
        }
        Err(format!(
            "no road path from node {from} to node {to} (the road graph is disconnected)"
        ))
    }

    /// 노드 순서의 길이(m).
    fn length(&self, nodes: &[u64]) -> f64 {
        nodes
            .windows(2)
            .map(|pair| distance(self.pos[&pair[0]], self.pos[&pair[1]]))
            .sum()
    }
}

/// 위경도 배열을 [`LatLon`]으로 바꾸고 대한민국 범위인지 검사한다.
fn korean_points(points: &[[f64; 2]]) -> Result<Vec<LatLon>, String> {
    if points.len() < 2 {
        return Err("a route needs at least 2 points (start and end)".to_owned());
    }
    points
        .iter()
        .enumerate()
        .map(|(k, &[lat, lon])| {
            let point = LatLon { lat, lon };
            if in_korea(point) {
                Ok(point)
            } else {
                Err(format!(
                    "point #{k} [{lat}, {lon}] is outside the South Korea area (lat {:?}, lon {:?})",
                    KOREA_LAT, KOREA_LON
                ))
            }
        })
        .collect()
}

/// 스냅샷 way와 찍은 점으로 경로를 계획해 경로 파일 내용을 만든다. `snapshot`은 파일에 적을 스냅샷 경로다.
///
/// 점마다 도로에 붙이고, 이웃한 점 사이를 A*로 이어 노드 순서를 만든 뒤, 폭을 정하고 실제로 코스를 만들어 본다.
/// 붙일 도로 없음·끊긴 그래프·상한 초과·코스 생성 실패는 모두 오류다.
pub fn plan_route(
    ways: &[OsmGraphWay],
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
) -> Result<RouteFile, String> {
    config.validate()?;
    let allowed: BTreeSet<&str> = config.allowed_highways.iter().map(String::as_str).collect();
    let picked = korean_points(points)?;
    let snapped = snap_points(ways, &allowed, &picked, config.snap_distance_m)?;
    let graph = SearchGraph::new(&snapped.ways, &allowed, picked[0]);
    let mut nodes: Vec<u64> = vec![snapped.nodes[0]];
    for pair in snapped.nodes.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!(
                "two consecutive points snap to the same place (node {})",
                pair[0]
            ));
        }
        nodes.extend(graph.find_path(pair[0], pair[1])?.into_iter().skip(1));
    }
    let length = graph.length(&nodes);
    if length > config.max_route_length_m {
        return Err(format!(
            "route is {length:.0} m long (limit {} m)",
            config.max_route_length_m
        ));
    }
    // 경로가 지나는 way 중 가장 좁은 폭을 코스 전체에 쓴다.
    let mut width: Option<(f32, WidthSource)> = None;
    for pair in nodes.windows(2) {
        let way = snapped
            .ways
            .iter()
            .filter(|way| searchable(way, &allowed))
            .find(|way| {
                way.nodes
                    .windows(2)
                    .any(|w| [w[0].id, w[1].id] == pair[..] || [w[1].id, w[0].id] == pair[..])
            })
            .ok_or_else(|| format!("no way between nodes {} and {}", pair[0], pair[1]))?;
        let resolved = resolve_width(way, config)?;
        if width.is_none_or(|(known, _)| resolved.0 < known) {
            width = Some(resolved);
        }
    }
    let (width_m, source) = width.ok_or("route has no road segment")?;
    let file = RouteFile {
        format: ROUTE_FORMAT.to_owned(),
        snapshot: snapshot.to_owned(),
        allowed_highways: config.allowed_highways.clone(),
        points: points.to_vec(),
        nodes,
        width_m,
        width_source: source.as_str().to_owned(),
        corner_radius_m: config.corner_radius_m,
        stub_length_m: config.stub_length_m,
        snap_distance_m: config.snap_distance_m,
        max_route_length_m: config.max_route_length_m,
    };
    build_route_file_course(&file, ways)?;
    Ok(file)
}

/// 경로 파일과 스냅샷 way로 코스를 만든다. 찍은 점을 같은 방법으로 다시 붙여 가상 노드를 복원한다.
///
/// 파일은 사용자가 고쳤거나 다른 스냅샷과 짝지어졌을 수 있으므로 계획 때와 같은 조건을 모두 다시 검사한다:
/// 형식, 점 붙이기 거리·경로 길이 상한 값과 그 적용, 대한민국 범위, 폭 출처, 폭·교차로 형상 값.
pub fn build_route_file_course(
    file: &RouteFile,
    ways: &[OsmGraphWay],
) -> Result<RouteCourse, String> {
    if file.format != ROUTE_FORMAT {
        return Err(format!(
            "unknown route file format `{}` (expected `{ROUTE_FORMAT}`)",
            file.format
        ));
    }
    check_limits(file.snap_distance_m, file.max_route_length_m)?;
    if ![WidthSource::Tag, WidthSource::Lanes, WidthSource::Default]
        .iter()
        .any(|source| source.as_str() == file.width_source)
    {
        return Err(format!("unknown width_source `{}`", file.width_source));
    }
    let max_route_length_m = file.max_route_length_m;
    let allowed: BTreeSet<&str> = file.allowed_highways.iter().map(String::as_str).collect();
    let picked = korean_points(&file.points)?;
    let snapped = snap_points(ways, &allowed, &picked, file.snap_distance_m)?;
    let graph = SearchGraph::new(&snapped.ways, &allowed, picked[0]);
    for &id in &file.nodes {
        let Some(&pos) = graph.pos.get(&id) else {
            return Err(format!("route node {id} is not on an allowed two-way road"));
        };
        if !in_korea(unlocal(picked[0], pos)) {
            return Err(format!("route node {id} is outside the South Korea area"));
        }
    }
    let length = graph.length(&file.nodes);
    if length > max_route_length_m {
        return Err(format!(
            "route is {length:.0} m long (limit {max_route_length_m} m)"
        ));
    }
    let width_notice = if file.width_source == WidthSource::Tag.as_str() {
        format!("Road width {:.1} m from the OSM width tag", file.width_m)
    } else {
        format!(
            "Road width {:.1} m is a temporary value ({})",
            file.width_m, file.width_source
        )
    };
    let notices = vec![
        map::OSM_ATTRIBUTION.to_owned(),
        "Virtual training course from an OSM route (not a surveyed road)".to_owned(),
        width_notice,
    ];
    let settings = JunctionSettings {
        corner_radius_m: file.corner_radius_m,
        stub_length_m: file.stub_length_m,
    };
    route::build_route_course_with(
        "route",
        &snapped.ways,
        &allowed,
        &file.nodes,
        file.width_m,
        &settings,
        notices,
    )
}

/// 경로 파일 내용과 그 파일이 있는 폴더로 코스를 만든다. 스냅샷은 `base_dir` 기준 상대 경로다.
pub fn load_route_course(file: &RouteFile, base_dir: &Path) -> Result<CourseDef, String> {
    let source = base_dir.join(&file.snapshot);
    let json = std::fs::read_to_string(&source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let ways = parse_overpass_graph(&json)?;
    build_route_file_course(file, &ways).map(|built| built.course)
}

/// 경로 파일 내용을 TOML 문자열로 만든다.
pub fn route_file_toml(file: &RouteFile) -> Result<String, String> {
    toml::to_string(file).map_err(|error| format!("cannot write route file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::fixtures::{self, FixtureWay};
    use crate::world::test_drive::follow_path;

    /// 폭 시험 사례: (추가 태그, 기대 폭과 출처).
    type WidthCase<'a> = (&'a [(&'a str, &'a str)], (f32, WidthSource));
    /// 오류 시험 사례: (이름, 스냅샷 way, 찍은 점, 설정, 오류 문구 일부).
    type ErrorCase<'a> = (
        &'a str,
        Vec<OsmGraphWay>,
        Vec<[f64; 2]>,
        &'a RoadGraphConfig,
        &'a str,
    );

    /// 합성 자료를 그래프로 읽는다.
    fn parse(ways: Vec<FixtureWay>) -> Vec<OsmGraphWay> {
        parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap()
    }

    /// 합성 자료 기준점에서 동쪽·북쪽(m)만큼 떨어진 점을 [위도, 경도]로 만든다.
    fn at(east: f64, north: f64) -> [f64; 2] {
        let point = fixtures::offset(east, north);
        [point.lat, point.lon]
    }

    /// 저장소의 실제 스냅샷을 읽는다.
    fn real_snapshot() -> Vec<OsmGraphWay> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/maps/yeosu-expo/graph-2026-11-06.osm.json");
        parse_overpass_graph(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// 저장소의 `config/road_graph.toml`이 오류 없이 읽히고 코드 기본값과 같은지 확인한다.
    #[test]
    fn shipped_road_graph_config_matches_defaults() {
        let config = parse_road_graph_config(include_str!("../../config/road_graph.toml")).unwrap();
        assert_eq!(config, RoadGraphConfig::default());
        let mut broken = RoadGraphConfig::default();
        broken.default_width_m.remove("service");
        assert!(broken.validate().is_err());
        assert!(parse_road_graph_config("unknown = 1").is_err());
    }

    /// 폭은 `width` 태그 → `lanes` × 차로 폭 → 종류별 기본값 순서로 정한다.
    #[test]
    fn width_follows_tag_lanes_default_order() {
        let config = RoadGraphConfig::default();
        let way = |tags: &[(&str, &str)]| {
            let mut tags: BTreeMap<String, String> = tags
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
            tags.insert("highway".to_owned(), "residential".to_owned());
            OsmGraphWay {
                id: 1,
                tags,
                nodes: Vec::new(),
            }
        };
        let cases: [WidthCase; 5] = [
            (&[("width", "5.5"), ("lanes", "2")], (5.5, WidthSource::Tag)),
            (&[("width", "4.5 m")], (4.5, WidthSource::Tag)),
            (
                &[("width", "wide"), ("lanes", "2")],
                (6.0, WidthSource::Lanes),
            ),
            (&[("lanes", "1.5")], (5.0, WidthSource::Default)),
            (&[], (5.0, WidthSource::Default)),
        ];
        for (tags, expected) in cases {
            assert_eq!(
                resolve_width(&way(tags), &config).unwrap(),
                expected,
                "{tags:?}"
            );
        }
        let mut unknown = way(&[]);
        unknown
            .tags
            .insert("highway".to_owned(), "track".to_owned());
        assert!(resolve_width(&unknown, &config).is_err());
    }

    /// 합성 T자 교차로: 찍은 점이 가상 노드로 붙고, A*가 교차로를 지나 잇고, 같은 입력이면 같은 경로 파일이며,
    /// 경로 파일을 TOML로 저장했다 읽어도 같은 코스가 나오는지 확인한다(PLAN 4-3 완료 기준: 재현성).
    #[test]
    fn synthetic_route_is_planned_and_reproducible() {
        let ways = parse(fixtures::three_way());
        let config = RoadGraphConfig::default();
        // 서쪽 팔 위 두 점(같은 구간에 가상 노드 둘)과 남쪽 팔 끝 근처 점.
        let points = [at(-40.0, 2.0), at(-25.0, -1.0), at(1.5, -40.0)];
        let file = plan_route(&ways, "fixture.json", &points, &config).unwrap();
        assert_eq!(
            file.nodes,
            vec![
                VIRTUAL_NODE_BASE,
                VIRTUAL_NODE_BASE + 1,
                10,
                VIRTUAL_NODE_BASE + 2
            ]
        );
        assert_eq!((file.width_m, file.width_source.as_str()), (4.0, "default"));
        assert_eq!(
            plan_route(&ways, "fixture.json", &points, &config).unwrap(),
            file
        );
        let text = route_file_toml(&file).unwrap();
        let reread: RouteFile = toml::from_str(&text).unwrap();
        assert_eq!(reread, file);
        let first = build_route_file_course(&file, &ways).unwrap();
        let second = build_route_file_course(&reread, &ways).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.junctions, vec![10]);
        let result = follow_path(&first.course, &first.guide, 3.0);
        assert_eq!(result.impacts, 0, "{result:?}");
        assert!(result.remaining_m < 12.5, "{result:?}");
    }

    /// 오류 처리: 붙일 도로 없음, 끊긴 그래프, 경로 길이·코스 범위 상한 초과, 대한민국 범위 밖, 같은 곳에 붙는 점,
    /// 알 수 없는 형식(PLAN 4-3 완료 기준).
    #[test]
    fn planning_errors_are_reported() {
        let config = RoadGraphConfig::default();
        let three = parse(fixtures::three_way());
        let cross = parse(fixtures::crossing_without_shared_node());
        let short = RoadGraphConfig {
            max_route_length_m: 50.0,
            ..RoadGraphConfig::default()
        };
        let straight = |length: f64| {
            parse(vec![(
                190,
                "service",
                &[],
                vec![(1, 0.0, 0.0), (2, length, 0.0)],
            )])
        };
        let cases: Vec<ErrorCase> = vec![
            (
                "far",
                three.clone(),
                vec![at(-40.0, 100.0), at(40.0, 0.0)],
                &config,
                "nearest",
            ),
            (
                "disconnected",
                cross,
                vec![at(-40.0, 0.0), at(0.0, 40.0)],
                &config,
                "disconnected",
            ),
            (
                "length",
                three.clone(),
                vec![at(-40.0, 0.0), at(40.0, 0.0)],
                &short,
                "long",
            ),
            (
                "extent",
                straight(1800.0),
                vec![at(20.0, 0.0), at(1780.0, 0.0)],
                &config,
                "spans",
            ),
            (
                "korea",
                three.clone(),
                vec![[40.0, 127.75], at(40.0, 0.0)],
                &config,
                "outside",
            ),
            (
                "same",
                three.clone(),
                vec![at(-0.3, 0.2), at(0.4, -0.3)],
                &config,
                "same place",
            ),
            ("single", three, vec![at(-30.0, 0.0)], &config, "at least 2"),
        ];
        for (name, ways, points, config, expected) in cases {
            let error = plan_route(&ways, "fixture.json", &points, config).unwrap_err();
            assert!(error.contains(expected), "{name}: {error}");
        }
        // 3-4 단일 통로 상한(500 m)보다 긴 경로도 1.5 km 안이면 만든다.
        let long = straight(700.0);
        assert!(plan_route(&long, "f.json", &[at(20.0, 0.0), at(680.0, 0.0)], &config).is_ok());
        let ways = parse(fixtures::three_way());
        let mut file =
            plan_route(&ways, "f.json", &[at(-40.0, 0.0), at(40.0, 0.0)], &config).unwrap();
        file.format = "route-v0".to_owned();
        let error = build_route_file_course(&file, &ways).unwrap_err();
        assert!(error.contains("format"), "{error}");
    }

    /// 재생 계약: 다시 읽을 때도 저장한 점 붙이기 거리·경로 길이 상한을 적용하고, 값 자체도 검사한다
    /// (Codex 4-3 검토 P2 재현 사례 포함).
    #[test]
    fn replay_enforces_saved_limits() {
        let straight = parse(vec![(
            195,
            "service",
            &[],
            vec![(1, 0.0, 0.0), (2, 100.0, 0.0)],
        )]);
        let config = RoadGraphConfig::default();
        let file = plan_route(
            &straight,
            "f.json",
            &[at(10.0, 0.0), at(90.0, 0.0)],
            &config,
        )
        .unwrap();
        assert_eq!(
            (file.snap_distance_m, file.max_route_length_m),
            (30.0, 2000.0)
        );
        assert!(build_route_file_course(&file, &straight).is_ok());
        // 저장한 점을 약 111 m 북쪽으로 옮긴 파일(사용자 수정)은 거부한다.
        let mut moved = file.clone();
        for point in &mut moved.points {
            point[0] += 0.001;
        }
        let error = build_route_file_course(&moved, &straight).unwrap_err();
        assert!(error.contains("nearest"), "{error}");
        // 스냅샷이 바뀌어 가까운 도로가 사라지고 먼 도로만 남으면 거부한다.
        let changed = parse(vec![(
            196,
            "service",
            &[],
            vec![(3, 0.0, 120.0), (4, 100.0, 120.0)],
        )]);
        let error = build_route_file_course(&file, &changed).unwrap_err();
        assert!(error.contains("nearest"), "{error}");
        // 상한 값이 없거나(무한대·0) 절대 상한을 넘으면 거부한다.
        for (snap, length) in [
            (f64::INFINITY, 2000.0),
            (0.0, 2000.0),
            (150.0, 2000.0),
            (30.0, 5000.0),
            (30.0, f64::NAN),
        ] {
            let edited = RouteFile {
                snap_distance_m: snap,
                max_route_length_m: length,
                ..file.clone()
            };
            let error = build_route_file_course(&edited, &straight).unwrap_err();
            assert!(error.contains("must be a positive number"), "{error}");
        }
        // 저장한 경로 길이 상한도 적용한다.
        let short = RouteFile {
            max_route_length_m: 50.0,
            ..file.clone()
        };
        let error = build_route_file_course(&short, &straight).unwrap_err();
        assert!(error.contains("long"), "{error}");
        let unknown = RouteFile {
            width_source: "guess".to_owned(),
            ..file
        };
        assert!(build_route_file_course(&unknown, &straight).is_err());
    }

    /// 저장소의 예제 경로 파일이 `--course map:` 로더로 읽히고, 같은 점으로 다시 계획하면 같은 파일이 되며,
    /// 실제 교차로를 지나 경로 추종 조향기가 충돌 없이 끝 부근까지 가는지 확인한다.
    #[test]
    fn committed_route_file_is_reproducible_and_drivable() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps/yeosu-expo");
        let text = std::fs::read_to_string(dir.join("route-junctions.toml")).unwrap();
        let file: RouteFile = toml::from_str(&text).unwrap();
        let ways = real_snapshot();
        let replanned = plan_route(
            &ways,
            &file.snapshot,
            &file.points,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        assert_eq!(replanned, file);
        let built = build_route_file_course(&file, &ways).unwrap();
        for junction in [5_143_358_021, 5_143_358_070, 5_239_572_002] {
            assert!(built.junctions.contains(&junction), "{:?}", built.junctions);
        }
        let loaded = map::load_map_course(&dir.join("route-junctions.toml")).unwrap();
        assert_eq!(loaded, built.course);
        assert!(loaded.notices.iter().any(|n| n.contains("temporary")));
        let result = follow_path(&built.course, &built.guide, 4.0);
        assert_eq!(result.impacts, 0, "{result:?}");
        assert!(result.remaining_m < 12.5, "{result:?}");
    }

    /// 노드 ID가 없는 스냅샷(`out geom`)을 가리키는 경로 파일은 거부한다.
    #[test]
    fn snapshot_without_node_ids_is_rejected() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps/yeosu-expo");
        let text = std::fs::read_to_string(dir.join("route-junctions.toml")).unwrap();
        let mut file: RouteFile = toml::from_str(&text).unwrap();
        file.snapshot = "survey-2026-11-06.osm.json".to_owned();
        let error = load_route_course(&file, &dir).unwrap_err();
        assert!(error.contains("node IDs"), "{error}");
    }
}
