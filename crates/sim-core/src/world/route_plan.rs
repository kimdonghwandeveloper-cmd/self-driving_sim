//! 도로 그래프와 경로 탐색, 경로 파일(Phase 4-3).
//!
//! 사용자가 찍은 점(시작·경유·끝, 위경도)을 가장 가까운 허용 도로 위 점에 붙이고, 이웃한 점 사이를 A*로 찾아
//! OSM·지역 보정 노드 ID 순서의 경로를 만든다. 결과는 경로 파일(TOML, [`RouteFile`])로 저장하며, `--course map:` 로더가
//! 이 파일과 저장된 스냅샷만으로 같은 코스를 다시 만든다(코스 생성은 [`route::build_route_course_with`]).
//!
//! - 도로 그래프: 허용 `highway` 종류의 way를 같은 노드 ID에서 나눈 구간으로 잇는다. 일방통행
//!   ([`RoadGraphConfig::nav_rules`], PLAN 4-8)을 쓰면 일방통행 way는 진행 방향 간선만 넣고, 쓰지 않으면(기존
//!   경로 파일) 예전처럼 일방통행 way를 뺀다. 방향은 [`route::travel`]로 정한다.
//! - 회전 제한(PLAN 4-8): 스냅샷에 회전 제한 관계([`TurnRestriction`])가 있으면 (노드, 들어온 way) 상태로 A*를 해
//!   금지된 회전(`no_*`)과 지정 외 회전(`only_*`)을 하지 않는다. 경로 파일을 다시 읽을 때도 같은 규칙으로 검사한다.
//! - 우회(PLAN 4-8): 찾은 경로의 교차로를 만들 수 없으면(너무 가까움·좁은 팔 각도 등) 그 교차로를 피해 다시 찾는다.
//!   최대 [`MAX_DETOURS`]곳까지 피한다.
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
use super::map_patch::{self, PatchAudience};
use super::map_patch_graph::{self, PatchedGraph};
use super::osm_graph::{OsmGraphWay, OsmNode, TurnRestriction};
use super::road_snapshot::{self, Connectivity};
use super::route::{self, BuildFailure, JunctionSettings, RouteCourse, Travel};

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
/// 교차로를 만들 수 없을 때 피해 다시 찾는 최대 교차로 수(임시값).
pub const MAX_DETOURS: usize = 12;

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
    /// 내비게이션 규칙(PLAN 4-8)을 쓸지: 일방통행 way를 진행 방향으로 탐색하고, 형상이 맞지 않는 교차로를
    /// 곧게 지나면 단순화한다. 없으면 `false`(예전 규칙: 일방통행 제외, 교차로 단순화 없음).
    #[serde(default)]
    pub nav_rules: bool,
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
            ("tertiary_link", 4.5),
            ("secondary", 6.5),
            ("secondary_link", 4.5),
            ("primary", 7.0),
            ("primary_link", 4.5),
            ("trunk", 7.0),
            ("trunk_link", 4.5),
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
            nav_rules: true,
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
    /// 경로 노드 ID 순서. `[2^62, 2^63)`은 찍은 점을 붙인 가상 노드이고 `2^63` 이상은 지역 보정 합성 노드다.
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
    /// 내비게이션 규칙(PLAN 4-8: 일방통행 방향 탐색, 교차로 단순화)으로 만들었는지. 없는 기존 파일은 `false`로 읽어
    /// 예전 규칙으로 재생한다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nav_rules: bool,
    /// 경로가 실제로 지난 지역 보정. 없는 기존 `route-v1`은 OSM만 사용한 경로로 읽는다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_patches: Vec<RoutePatchRef>,
    /// 점마다 표시 이름(건물·장소로 고른 점은 그 이름, 그냥 찍은 점은 빈 문자열, PLAN 4-9). 코스 생성에는 쓰지 않는다.
    /// 비어 있거나 점 개수와 같아야 한다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub point_labels: Vec<String>,
}

/// 경로 파일이 재생에 요구하는 지역 보정 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePatchRef {
    /// 보정의 안정적인 ID.
    pub id: String,
    /// 보정 파일 형식.
    pub format: String,
    /// 계획할 때 읽은 보정 원문 바이트의 SHA-256.
    pub sha256: String,
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
pub(crate) fn local(origin: LatLon, point: LatLon) -> [f64; 2] {
    let cos_lat0 = origin.lat.to_radians().cos();
    [
        EARTH_RADIUS_M * (point.lon - origin.lon).to_radians() * cos_lat0,
        EARTH_RADIUS_M * (point.lat - origin.lat).to_radians(),
    ]
}

/// [`local`]의 역변환.
pub(crate) fn unlocal(origin: LatLon, [east, north]: [f64; 2]) -> LatLon {
    LatLon {
        lat: origin.lat + (north / EARTH_RADIUS_M).to_degrees(),
        lon: origin.lon + (east / (EARTH_RADIUS_M * origin.lat.to_radians().cos())).to_degrees(),
    }
}

/// 두 지역 좌표 사이 거리(m).
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// 탐색에서 way의 통행 방향. `nav_rules`가 `false`면 예전 규칙(일방통행 way는 빼고 나머지는 양방향)이다.
pub fn way_travel(way: &OsmGraphWay, nav_rules: bool) -> Travel {
    if nav_rules {
        route::travel(way)
    } else if route::is_oneway(way) {
        Travel::Closed
    } else {
        Travel::Both
    }
}

/// 경로 탐색에 쓰는 way인지(허용 종류이고 탐색할 수 있는 방향이 있음) 판정한다.
pub fn searchable(way: &OsmGraphWay, allowed: &BTreeSet<&str>, nav_rules: bool) -> bool {
    way.highway().is_some_and(|kind| allowed.contains(kind))
        && way_travel(way, nav_rules) != Travel::Closed
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
    nav_rules: bool,
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
            if !searchable(way, allowed, nav_rules) {
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
        let (gap, w, s, t) = best.ok_or("no allowed roads in the snapshot")?;
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

/// A* 우선순위 큐 항목(추정 총비용이 작은 것, 같으면 상태가 작은 것이 먼저).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    /// 지금까지 비용 + 남은 직선 거리(m).
    estimate: f64,
    /// 탐색 상태.
    state: State,
}

/// 탐색 상태: (노드 ID, 들어온 way ID, 직전 노드 ID). 출발점처럼 들어온 way·직전 노드가 없으면 [`NO_WAY`]다.
/// 직전 노드로 되돌아가는 간선(도로 가운데서 되돌기)은 쓰지 않는다.
type State = (u64, u64, u64);
/// 들어온 way·직전 노드가 없음을 뜻하는 값.
const NO_WAY: u64 = u64::MAX;

impl Eq for Open {}

impl Ord for Open {
    /// [`BinaryHeap`]이 최대 힙이므로 순서를 뒤집는다.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.state.cmp(&self.state))
    }
}

impl PartialOrd for Open {
    /// [`Ord`]와 같은 순서.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 방향 있는 간선.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    /// 도착 노드 ID.
    to: u64,
    /// 구간 길이(m).
    length: f64,
    /// 이 구간의 way ID.
    way: u64,
}

/// 탐색 가능한 way로 만든 방향 있는 노드 연결과 노드 좌표, 회전 제한.
struct SearchGraph {
    /// 노드 ID별 지역 좌표.
    pos: BTreeMap<u64, [f64; 2]>,
    /// 노드 ID별 나가는 간선.
    adjacent: BTreeMap<u64, Vec<Edge>>,
    /// (via 노드, 들어온 way)별 회전 제한.
    restrictions: BTreeMap<(u64, u64), Vec<TurnRestriction>>,
}

impl SearchGraph {
    /// 탐색 가능한 way를 같은 노드 ID에서 나눈 구간으로 잇는다. 일방통행 way는 진행 방향 간선만 넣는다.
    fn new(
        ways: &[OsmGraphWay],
        allowed: &BTreeSet<&str>,
        nav_rules: bool,
        restrictions: &[TurnRestriction],
        origin: LatLon,
    ) -> Self {
        let mut pos = BTreeMap::new();
        let mut adjacent: BTreeMap<u64, Vec<Edge>> = BTreeMap::new();
        for way in ways
            .iter()
            .filter(|way| searchable(way, allowed, nav_rules))
        {
            for node in &way.nodes {
                pos.insert(node.id, local(origin, node.pos));
            }
            let travel = way_travel(way, nav_rules);
            for pair in way.nodes.windows(2) {
                let length = distance(local(origin, pair[0].pos), local(origin, pair[1].pos));
                let (a, b) = (pair[0].id, pair[1].id);
                let directions: &[(u64, u64)] = match travel {
                    Travel::Both => &[(a, b), (b, a)],
                    Travel::Forward => &[(a, b)],
                    Travel::Backward => &[(b, a)],
                    Travel::Closed => &[],
                };
                for &(from, to) in directions {
                    adjacent.entry(from).or_default().push(Edge {
                        to,
                        length,
                        way: way.id,
                    });
                }
            }
        }
        let mut by_turn: BTreeMap<(u64, u64), Vec<TurnRestriction>> = BTreeMap::new();
        for restriction in restrictions {
            by_turn
                .entry((restriction.via_node, restriction.from_way))
                .or_default()
                .push(*restriction);
        }
        Self {
            pos,
            adjacent,
            restrictions: by_turn,
        }
    }

    /// `from_way`로 `via`에 들어와 `to_way`로 나가는 회전이 회전 제한에 걸리지 않는지 판정한다.
    fn turn_allowed(&self, from_way: u64, via: u64, to_way: u64) -> bool {
        self.restrictions
            .get(&(via, from_way))
            .into_iter()
            .flatten()
            .all(|r| {
                if r.only {
                    r.to_way == to_way
                } else {
                    r.to_way != to_way
                }
            })
    }

    /// `from`(들어온 way·직전 노드 `entry`)에서 `to`까지 가장 짧은 노드 순서를 A*(직선 거리 추정)로 찾는다.
    /// 일방통행 방향과 회전 제한을 지키고, 되돌지 않으며, `avoid` 노드는 지나지 않는다(도착 노드 제외).
    /// 노드 순서와 도착 때의 (들어온 way, 직전 노드)를 돌려준다. 이어지지 않으면 오류다.
    fn find_path(
        &self,
        from: u64,
        entry: (u64, u64),
        to: u64,
        avoid: &BTreeSet<u64>,
    ) -> Result<(Vec<State>, (u64, u64)), String> {
        let goal = self.pos[&to];
        let start: State = (from, entry.0, entry.1);
        let mut best: BTreeMap<State, f64> = [(start, 0.0)].into_iter().collect();
        let mut came: BTreeMap<State, State> = BTreeMap::new();
        let mut closed = BTreeSet::new();
        let mut open = BinaryHeap::from([Open {
            estimate: distance(self.pos[&from], goal),
            state: start,
        }]);
        while let Some(Open { state, .. }) = open.pop() {
            let (node, way, previous) = state;
            if node == to {
                let mut path = vec![state];
                while let Some(&before) = came.get(path.last().unwrap()) {
                    path.push(before);
                }
                path.reverse();
                return Ok((path, (way, previous)));
            }
            if !closed.insert(state) {
                continue;
            }
            for edge in self.adjacent.get(&node).into_iter().flatten() {
                if edge.to == previous
                    || (avoid.contains(&edge.to) && edge.to != to)
                    || (way != NO_WAY && !self.turn_allowed(way, node, edge.way))
                {
                    continue;
                }
                let next: State = (edge.to, edge.way, node);
                let cost = best[&state] + edge.length;
                if best.get(&next).is_none_or(|&known| cost < known) {
                    best.insert(next, cost);
                    came.insert(next, state);
                    open.push(Open {
                        estimate: cost + distance(self.pos[&edge.to], goal),
                        state: next,
                    });
                }
            }
        }
        Err(format!(
            "no road path from node {from} to node {to} (the road graph is disconnected, or one-way roads and turn restrictions block it)"
        ))
    }

    /// 노드 순서가 간선 방향과 회전 제한을 지키고 되돌지 않는지 검사한다(경로 파일 재생용).
    fn check_path(&self, nodes: &[u64]) -> Result<(), String> {
        let mut way = NO_WAY;
        if let Some(triple) = nodes.windows(3).find(|t| t[0] == t[2]) {
            return Err(format!("route turns back at node {}", triple[1]));
        }
        for pair in nodes.windows(2) {
            let edges: Vec<&Edge> = self
                .adjacent
                .get(&pair[0])
                .into_iter()
                .flatten()
                .filter(|edge| edge.to == pair[1])
                .collect();
            if edges.is_empty() {
                return Err(format!(
                    "route goes from node {} to node {} against a one-way road or without a road segment",
                    pair[0], pair[1]
                ));
            }
            let Some(edge) = edges
                .iter()
                .find(|edge| way == NO_WAY || self.turn_allowed(way, pair[0], edge.way))
            else {
                return Err(format!(
                    "route breaks a turn restriction at node {}",
                    pair[0]
                ));
            };
            way = edge.way;
        }
        Ok(())
    }

    /// 노드 순서의 길이(m).
    fn length(&self, nodes: &[u64]) -> f64 {
        nodes
            .windows(2)
            .map(|pair| distance(self.pos[&pair[0]], self.pos[&pair[1]]))
            .sum()
    }
}

/// A*가 선택한 노드와 각 노드 사이의 way ID 순서.
struct SearchedRoute {
    /// 경로 노드 순서.
    nodes: Vec<u64>,
    /// `nodes`의 각 인접 쌍에 대응하는 way ID.
    ways: Vec<u64>,
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
///
/// 연결 정보·회전 제한이 없는 스냅샷용이다. 그 밖에는 [`plan_route_full`]을 쓴다.
pub fn plan_route(
    ways: &[OsmGraphWay],
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
) -> Result<RouteFile, String> {
    plan_route_detailed(ways, None, snapshot, points, config).map(|planned| planned.file)
}

/// 경로 계획 결과(경로 선택 화면 표시용).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRoute {
    /// 경로 파일 내용.
    pub file: RouteFile,
    /// 만든 코스와 안내선.
    pub course: RouteCourse,
    /// 노드 순서 기준 경로 길이(m).
    pub length_m: f64,
    /// 경로 노드의 위경도(노드 순서, 가상 노드 포함).
    pub path: Vec<LatLon>,
    /// 만들 수 없어 피해 간 교차로 노드 ID(피한 순서).
    pub avoided: Vec<u64>,
}

/// [`plan_route`]와 같고, 만든 코스·경로 길이·노드 위치도 함께 돌려준다. 회전 제한은 쓰지 않는다.
///
/// `connectivity`가 있으면(Geofabrik 추출) 코스를 만들기 전에 경계 연결 검사를 한다([`build_route_file_course`]).
pub fn plan_route_detailed(
    ways: &[OsmGraphWay],
    connectivity: Option<&Connectivity>,
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
) -> Result<PlannedRoute, String> {
    plan_route_full(ways, &[], connectivity, snapshot, points, config)
}

/// 회전 제한까지 써서 경로를 계획한다(PLAN 4-8).
///
/// 찾은 경로의 교차로를 만들 수 없으면 그 교차로를 피해 다시 찾는다(최대 [`MAX_DETOURS`]곳). 찍은 점이 붙은
/// 노드는 피하지 않는다. 더 피할 수 없으면 마지막 실패 이유를 돌려준다.
pub fn plan_route_full(
    ways: &[OsmGraphWay],
    restrictions: &[TurnRestriction],
    connectivity: Option<&Connectivity>,
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
) -> Result<PlannedRoute, String> {
    plan_route_full_inner(
        ways,
        restrictions,
        connectivity,
        snapshot,
        points,
        config,
        None,
    )
}

/// 지역 보정을 조립한 그래프에서 경로를 계획하고 실제로 지난 보정만 경로 파일에 기록한다.
pub fn plan_route_full_with_patches(
    graph: &PatchedGraph,
    restrictions: &[TurnRestriction],
    connectivity: Option<&Connectivity>,
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
) -> Result<PlannedRoute, String> {
    plan_route_full_inner(
        &graph.ways,
        restrictions,
        connectivity,
        snapshot,
        points,
        config,
        Some(graph),
    )
}

/// OSM 전용·지역 보정 경로 계획의 공통 본체.
fn plan_route_full_inner(
    ways: &[OsmGraphWay],
    restrictions: &[TurnRestriction],
    connectivity: Option<&Connectivity>,
    snapshot: &str,
    points: &[[f64; 2]],
    config: &RoadGraphConfig,
    patched: Option<&PatchedGraph>,
) -> Result<PlannedRoute, String> {
    config.validate()?;
    let allowed: BTreeSet<&str> = config.allowed_highways.iter().map(String::as_str).collect();
    let picked = korean_points(points)?;
    let snapped = snap_points(
        ways,
        &allowed,
        config.nav_rules,
        &picked,
        config.snap_distance_m,
    )?;
    let graph = SearchGraph::new(
        &snapped.ways,
        &allowed,
        config.nav_rules,
        restrictions,
        picked[0],
    );
    let mut avoid: BTreeSet<u64> = BTreeSet::new();
    let mut avoided: Vec<u64> = Vec::new();
    // 처음 피한 교차로의 실패 이유. 돌아갈 길이 없으면 이 이유를 알린다.
    let mut first_failure = String::new();
    loop {
        let searched = match search_route(&graph, &snapped.nodes, &avoid) {
            Ok(searched) => searched,
            Err(error) if avoided.is_empty() => return Err(error),
            Err(_) => {
                return Err(format!(
                    "{first_failure}; no detour around {} intersection(s) that cannot be built",
                    avoided.len()
                ));
            }
        };
        let length = graph.length(&searched.nodes);
        if length > config.max_route_length_m {
            return Err(format!(
                "route is {length:.0} m long (limit {} m)",
                config.max_route_length_m
            ));
        }
        let (width_m, source) = route_width(&snapped.ways, &allowed, config, &searched.nodes)?;
        let map_patches = patched
            .map(|graph| route_patch_refs(graph, &searched.ways))
            .unwrap_or_default();
        let file = RouteFile {
            format: ROUTE_FORMAT.to_owned(),
            snapshot: snapshot.to_owned(),
            allowed_highways: config.allowed_highways.clone(),
            points: points.to_vec(),
            nodes: searched.nodes,
            width_m,
            width_source: source.as_str().to_owned(),
            corner_radius_m: config.corner_radius_m,
            stub_length_m: config.stub_length_m,
            snap_distance_m: config.snap_distance_m,
            max_route_length_m: config.max_route_length_m,
            nav_rules: config.nav_rules,
            map_patches,
            point_labels: Vec::new(),
        };
        match build_course_checked(&file, ways, restrictions, connectivity) {
            Ok(course) => {
                let path = file
                    .nodes
                    .iter()
                    .map(|id| unlocal(picked[0], graph.pos[id]))
                    .collect();
                return Ok(PlannedRoute {
                    file,
                    course,
                    length_m: length,
                    path,
                    avoided,
                });
            }
            Err(BuildFailure {
                node: Some(node),
                message,
            }) if avoided.len() < MAX_DETOURS
                && !snapped.nodes.contains(&node)
                && avoid.insert(node) =>
            {
                if avoided.is_empty() {
                    first_failure = message;
                }
                avoided.push(node);
            }
            Err(failure) => return Err(failure.message),
        }
    }
}

/// A*가 선택한 way ID에서 실제 사용한 보정 참조만 ID 순서로 만든다.
fn route_patch_refs(graph: &PatchedGraph, route_ways: &[u64]) -> Vec<RoutePatchRef> {
    let used: BTreeSet<u64> = route_ways.iter().copied().collect();
    graph
        .applied
        .iter()
        .filter(|patch| patch.way_ids.iter().any(|id| used.contains(id)))
        .map(|patch| RoutePatchRef {
            id: patch.id.clone(),
            format: patch.format.clone(),
            sha256: patch.sha256.clone(),
        })
        .collect()
}

/// 찍은 점이 붙은 노드를 차례로 A*로 잇는다. 구간 사이에서도 들어온 way를 이어받아 회전 제한을 지킨다.
fn search_route(
    graph: &SearchGraph,
    stops: &[u64],
    avoid: &BTreeSet<u64>,
) -> Result<SearchedRoute, String> {
    let mut nodes: Vec<u64> = vec![stops[0]];
    let mut ways = Vec::new();
    let mut entry = (NO_WAY, NO_WAY);
    for pair in stops.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!(
                "two consecutive points snap to the same place (node {})",
                pair[0]
            ));
        }
        let (path, last) = graph.find_path(pair[0], entry, pair[1], avoid)?;
        nodes.extend(path.iter().skip(1).map(|state| state.0));
        ways.extend(path.iter().skip(1).map(|state| state.1));
        entry = last;
    }
    Ok(SearchedRoute { nodes, ways })
}

/// 경로가 지나는 way 중 가장 좁은 폭과 그 출처를 정한다(코스 전체에 쓴다).
fn route_width(
    ways: &[OsmGraphWay],
    allowed: &BTreeSet<&str>,
    config: &RoadGraphConfig,
    nodes: &[u64],
) -> Result<(f32, WidthSource), String> {
    let mut width: Option<(f32, WidthSource)> = None;
    for pair in nodes.windows(2) {
        let way = ways
            .iter()
            .filter(|way| searchable(way, allowed, config.nav_rules))
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
    width.ok_or_else(|| "route has no road segment".to_owned())
}

/// 경로 파일과 스냅샷 way로 코스를 만든다. 찍은 점을 같은 방법으로 다시 붙여 가상 노드를 복원한다.
///
/// 파일은 사용자가 고쳤거나 다른 스냅샷과 짝지어졌을 수 있으므로 계획 때와 같은 조건을 모두 다시 검사한다:
/// 형식, 점 붙이기 거리·경로 길이 상한 값과 그 적용, 대한민국 범위, 폭 출처, 폭·교차로 형상 값.
///
/// `connectivity`가 있으면(Geofabrik 추출) 경로 파일의 허용 도로 목록이 연결 정보와 같은지, 코스 형상에 쓰는 노드가
/// 불완전 노드가 아닌지 검사한다([`road_snapshot::check_route_connectivity`]). 경로 계획과 재생이 모두 여기를 거친다.
/// 회전 제한이 있는 스냅샷은 [`build_route_file_course_full`]을 쓴다.
pub fn build_route_file_course(
    file: &RouteFile,
    ways: &[OsmGraphWay],
    connectivity: Option<&Connectivity>,
) -> Result<RouteCourse, String> {
    build_route_file_course_full(file, ways, &[], connectivity)
}

/// [`build_route_file_course`]와 같고, 노드 순서가 일방통행 방향·회전 제한을 지키는지도 검사한다(PLAN 4-8).
pub fn build_route_file_course_full(
    file: &RouteFile,
    ways: &[OsmGraphWay],
    restrictions: &[TurnRestriction],
    connectivity: Option<&Connectivity>,
) -> Result<RouteCourse, String> {
    build_course_checked(file, ways, restrictions, connectivity).map_err(|failure| failure.message)
}

/// [`build_route_file_course_full`]의 본체. 교차로 때문에 실패하면 그 노드를 함께 돌려준다.
fn build_course_checked(
    file: &RouteFile,
    ways: &[OsmGraphWay],
    restrictions: &[TurnRestriction],
    connectivity: Option<&Connectivity>,
) -> Result<RouteCourse, BuildFailure> {
    if file.format != ROUTE_FORMAT {
        return Err(format!(
            "unknown route file format `{}` (expected `{ROUTE_FORMAT}`)",
            file.format
        )
        .into());
    }
    check_limits(file.snap_distance_m, file.max_route_length_m)?;
    if ![WidthSource::Tag, WidthSource::Lanes, WidthSource::Default]
        .iter()
        .any(|source| source.as_str() == file.width_source)
    {
        return Err(format!("unknown width_source `{}`", file.width_source).into());
    }
    if !file.point_labels.is_empty() && file.point_labels.len() != file.points.len() {
        return Err(format!(
            "point_labels has {} entries but there are {} points",
            file.point_labels.len(),
            file.points.len()
        )
        .into());
    }
    let max_route_length_m = file.max_route_length_m;
    let allowed: BTreeSet<&str> = file.allowed_highways.iter().map(String::as_str).collect();
    let picked = korean_points(&file.points)?;
    let snapped = snap_points(
        ways,
        &allowed,
        file.nav_rules,
        &picked,
        file.snap_distance_m,
    )?;
    let graph = SearchGraph::new(
        &snapped.ways,
        &allowed,
        file.nav_rules,
        restrictions,
        picked[0],
    );
    for &id in &file.nodes {
        let Some(&pos) = graph.pos.get(&id) else {
            return Err(format!("route node {id} is not on an allowed searchable road").into());
        };
        if !in_korea(unlocal(picked[0], pos)) {
            return Err(format!("route node {id} is outside the South Korea area").into());
        }
    }
    graph.check_path(&file.nodes)?;
    let length = graph.length(&file.nodes);
    if length > max_route_length_m {
        return Err(format!("route is {length:.0} m long (limit {max_route_length_m} m)").into());
    }
    let width_notice = if file.width_source == WidthSource::Tag.as_str() {
        format!("Road width {:.1} m from the OSM width tag", file.width_m)
    } else {
        format!(
            "Road width {:.1} m is a temporary value ({})",
            file.width_m, file.width_source
        )
    };
    let mut notices = vec![
        map::OSM_ATTRIBUTION.to_owned(),
        "Virtual training course from an OSM route (not a surveyed road)".to_owned(),
        width_notice,
    ];
    if file.nav_rules {
        notices.push(
            "One-way and divided roads are drawn as a single carriageway (not the real lane layout)"
                .to_owned(),
        );
    }
    if let Some(connectivity) = connectivity {
        road_snapshot::check_route_connectivity(
            connectivity,
            &snapped.ways,
            &file.allowed_highways,
            &file.nodes,
        )?;
    }
    let settings = JunctionSettings {
        corner_radius_m: file.corner_radius_m,
        stub_length_m: file.stub_length_m,
    };
    route::build_route_course_detailed(
        "route",
        &snapped.ways,
        &allowed,
        &file.nodes,
        file.width_m,
        &settings,
        notices,
        file.nav_rules,
    )
}

/// 경로 파일 내용과 그 파일이 있는 폴더로 코스를 만든다. 스냅샷은 `base_dir` 기준 상대 경로다.
///
/// 스냅샷은 [`road_snapshot::load_road_snapshot`]으로 읽으므로 Geofabrik 추출이면 연결 정보 검사를 건너뛰지 않는다.
/// 스냅샷에 회전 제한이 있으면 함께 검사한다.
pub fn load_route_course(file: &RouteFile, base_dir: &Path) -> Result<CourseDef, String> {
    load_route_course_with_patch_dir(
        file,
        base_dir,
        Path::new(map_patch::PATCH_DIR),
        PatchAudience::Distribution,
    )
}

/// 경로가 요구한 지역 보정을 지정 디렉터리에서 읽고 ID·형식·SHA를 확인한 뒤 같은 그래프로 재생한다.
///
/// `map_patches`가 비어 있는 기존 경로는 보정 디렉터리를 읽지 않고 이전 OSM 전용 경로로 처리한다.
pub fn load_route_course_with_patch_dir(
    file: &RouteFile,
    base_dir: &Path,
    patch_dir: &Path,
    audience: PatchAudience,
) -> Result<CourseDef, String> {
    let snapshot = road_snapshot::load_road_snapshot(&base_dir.join(&file.snapshot))?;
    if file.map_patches.is_empty() {
        return build_route_file_course_full(
            file,
            &snapshot.ways,
            &snapshot.restrictions,
            snapshot.connectivity.as_ref(),
        )
        .map(|built| built.course);
    }
    validate_route_patch_refs(&file.map_patches)?;
    let available = map_patch::load_patch_dir(patch_dir, audience)
        .map_err(|error| format!("cannot load route map patches: {error}"))?;
    let by_id: BTreeMap<&str, &map_patch::MapPatch> = available
        .iter()
        .map(|patch| (patch.id.as_str(), patch))
        .collect();
    let mut selected = Vec::with_capacity(file.map_patches.len());
    for required in &file.map_patches {
        let patch = by_id.get(required.id.as_str()).copied().ok_or_else(|| {
            format!(
                "route requires missing map patch `{}` in {}",
                required.id,
                patch_dir.display()
            )
        })?;
        if patch.format != required.format {
            return Err(format!(
                "map patch `{}` format mismatch: route has `{}`, file has `{}`",
                required.id, required.format, patch.format
            ));
        }
        if patch.sha256 != required.sha256 {
            return Err(format!(
                "map patch `{}` SHA-256 mismatch: route has {}, file has {}",
                required.id, required.sha256, patch.sha256
            ));
        }
        selected.push(patch.clone());
    }
    let graph = map_patch_graph::apply_map_patches(&snapshot.ways, &selected)
        .map_err(|error| format!("cannot rebuild route map patches: {error}"))?;
    build_route_file_course_full(
        file,
        &graph.ways,
        &snapshot.restrictions,
        snapshot.connectivity.as_ref(),
    )
    .map(|built| built.course)
}

/// 경로 파일의 보정 참조가 정렬·고유하고 형식과 SHA-256 모양이 유효한지 검사한다.
fn validate_route_patch_refs(references: &[RoutePatchRef]) -> Result<(), String> {
    let mut previous: Option<&str> = None;
    for reference in references {
        if reference.id.trim().is_empty() {
            return Err("route map patch ID must not be empty".to_owned());
        }
        if reference.format != map_patch::FORMAT {
            return Err(format!(
                "route map patch `{}` has unsupported format `{}`",
                reference.id, reference.format
            ));
        }
        if reference.sha256.len() != 64
            || !reference
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!(
                "route map patch `{}` has invalid SHA-256 `{}`",
                reference.id, reference.sha256
            ));
        }
        if previous.is_some_and(|id| id >= reference.id.as_str()) {
            return Err("route map_patches must have unique IDs in ascending order".to_owned());
        }
        previous = Some(&reference.id);
    }
    Ok(())
}

/// 경로 파일 내용을 TOML 문자열로 만든다.
pub fn route_file_toml(file: &RouteFile) -> Result<String, String> {
    toml::to_string(file).map_err(|error| format!("cannot write route file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::fixtures::{self, FixtureWay};
    use crate::world::osm_graph::parse_overpass_graph;
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
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/graph-2026-11-06.osm.json");
        parse_overpass_graph(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// 저장소의 `config/road_graph.toml`이 오류 없이 읽히고 코드 기본값과 같은지 확인한다.
    #[test]
    fn shipped_road_graph_config_matches_defaults() {
        let config =
            parse_road_graph_config(include_str!("../../../../config/road_graph.toml")).unwrap();
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
        let first = build_route_file_course(&file, &ways, None).unwrap();
        let second = build_route_file_course(&reread, &ways, None).unwrap();
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
        let error = build_route_file_course(&file, &ways, None).unwrap_err();
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
        assert!(build_route_file_course(&file, &straight, None).is_ok());
        // 저장한 점을 약 111 m 북쪽으로 옮긴 파일(사용자 수정)은 거부한다.
        let mut moved = file.clone();
        for point in &mut moved.points {
            point[0] += 0.001;
        }
        let error = build_route_file_course(&moved, &straight, None).unwrap_err();
        assert!(error.contains("nearest"), "{error}");
        // 스냅샷이 바뀌어 가까운 도로가 사라지고 먼 도로만 남으면 거부한다.
        let changed = parse(vec![(
            196,
            "service",
            &[],
            vec![(3, 0.0, 120.0), (4, 100.0, 120.0)],
        )]);
        let error = build_route_file_course(&file, &changed, None).unwrap_err();
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
            let error = build_route_file_course(&edited, &straight, None).unwrap_err();
            assert!(error.contains("must be a positive number"), "{error}");
        }
        // 저장한 경로 길이 상한도 적용한다.
        let short = RouteFile {
            max_route_length_m: 50.0,
            ..file.clone()
        };
        let error = build_route_file_course(&short, &straight, None).unwrap_err();
        assert!(error.contains("long"), "{error}");
        let unknown = RouteFile {
            width_source: "guess".to_owned(),
            ..file
        };
        assert!(build_route_file_course(&unknown, &straight, None).is_err());
    }

    /// 저장소의 예제 경로 파일이 `--course map:` 로더로 읽히고, 같은 점으로 다시 계획하면 같은 파일이 되며,
    /// 실제 교차로를 지나 경로 추종 조향기가 충돌 없이 끝 부근까지 가는지 확인한다.
    #[test]
    fn committed_route_file_is_reproducible_and_drivable() {
        let dir =
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join("testdata/yeosu-expo");
        let text = std::fs::read_to_string(dir.join("route-junctions.toml")).unwrap();
        let file: RouteFile = toml::from_str(&text).unwrap();
        let ways = real_snapshot();
        // 이 파일은 4-8 이전 규칙(작은 도로만, 일방통행 제외)으로 만들었다.
        let legacy = RoadGraphConfig {
            allowed_highways: file.allowed_highways.clone(),
            nav_rules: false,
            ..RoadGraphConfig::default()
        };
        let replanned = plan_route(&ways, &file.snapshot, &file.points, &legacy).unwrap();
        assert_eq!(replanned, file);
        // 지금 기본 규칙(큰 도로·일방통행 포함)으로 계획해도 이 구간은 같은 노드 순서다.
        let current = plan_route(
            &ways,
            &file.snapshot,
            &file.points,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        assert_eq!(current.nodes, file.nodes);
        let built = build_route_file_course(&file, &ways, None).unwrap();
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

    /// 4-6 자동 주행 회귀: 저장된 여수 스냅샷에서 고정한 점 쌍 4개로 경로를 계획하면 지나는 교차로와 길이가
    /// 기록과 같고, 경로 끝 목표가 있으며, 경로 추종 조향기가 굽은 길·교차로를 지나 충돌 없이 끝 부근까지 가는지 확인한다.
    /// 경로·코스 생성이나 운동 모델·충돌 판정이 바뀌면 여기서 드러난다. 사람의 주행 확인을 대신하지 않는다.
    #[test]
    fn fixed_yeosu_routes_drive_to_goal() {
        let ways = real_snapshot();
        let config = RoadGraphConfig::default();
        /// (찍은 두 점, 기록한 경로 길이 m, 지나는 교차로 노드).
        type FixedRoute = ([[f64; 2]; 2], f64, &'static [u64]);
        let cases: [FixedRoute; 4] = [
            (
                [[34.749_661_4, 127.748_854_7], [34.751_844_2, 127.750_709_2]],
                972.0,
                &[
                    5_143_354_386,
                    5_239_572_002,
                    5_143_358_070,
                    5_143_358_064,
                    5_167_348_575,
                    1_784_623_475,
                    11_250_563_544,
                ],
            ),
            (
                [[34.747_886, 127.746_555_2], [34.754_151_8, 127.752_092_5]],
                863.0,
                &[
                    5_239_572_765,
                    5_167_348_577,
                    5_178_188_925,
                    5_143_354_386,
                    5_239_572_002,
                    5_143_358_070,
                    5_143_358_064,
                    5_167_348_575,
                ],
            ),
            (
                [[34.749_896_1, 127.745_697_9], [34.752_479_9, 127.748_534]],
                433.0,
                &[5_528_563_752, 5_528_563_751, 5_143_358_021, 1_784_623_428],
            ),
            (
                [[34.752_391_2, 127.746_953_7], [34.748_973_2, 127.750_058_2]],
                603.0,
                &[5_143_358_021, 5_143_358_070, 5_239_572_002, 5_143_354_386],
            ),
        ];
        for (points, length_m, junctions) in cases {
            let planned = plan_route_detailed(&ways, None, "snapshot", &points, &config)
                .unwrap_or_else(|e| panic!("{points:?}: {e}"));
            assert_eq!(planned.course.junctions, junctions, "{points:?}");
            assert!(
                (planned.length_m - length_m).abs() < 1.0,
                "{points:?}: {}",
                planned.length_m
            );
            assert!(planned.course.course.goal.is_some(), "{points:?}");
            let result = follow_path(&planned.course.course, &planned.course.guide, 4.0);
            assert_eq!(result.impacts, 0, "{points:?}: {result:?}");
            assert!(result.remaining_m < 12.5, "{points:?}: {result:?}");
            assert!(result.max_offset_m < 1.2, "{points:?}: {result:?}");
        }
    }

    /// 노드 ID가 없는 스냅샷(`out geom`)을 가리키는 경로 파일은 거부한다.
    #[test]
    fn snapshot_without_node_ids_is_rejected() {
        let dir =
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).join("testdata/yeosu-expo");
        let text = std::fs::read_to_string(dir.join("route-junctions.toml")).unwrap();
        let mut file: RouteFile = toml::from_str(&text).unwrap();
        file.snapshot = "survey-2026-11-06.osm.json".to_owned();
        let error = load_route_course(&file, &dir).unwrap_err();
        assert!(error.contains("node IDs"), "{error}");
    }

    /// 예전에 교차로 연석에 막혔던 여수 경로가 끝까지 가는지 확인한다(4-6).
    ///
    /// - 12333444324: 135° 급회전. 안내선 원호 반경이 최소 회전 반경에 가까워 바깥 연석에 닿았다.
    /// - 5178188925: 100° 회전. 같은 원인.
    /// - 12669768610: 180°보다 아주 조금 작은 직진 쪽 모서리에 중심을 도는 연석 고리가 생겨 직진 중 막혔다.
    #[test]
    fn yeosu_sharp_junctions_are_drivable() {
        let ways = real_snapshot();
        let config = RoadGraphConfig::default();
        let cases: [([[f64; 2]; 2], u64); 3] = [
            (
                [[34.754_661, 127.753_155_1], [34.754_151_8, 127.752_092_5]],
                12_333_444_324,
            ),
            (
                [[34.747_886, 127.746_555_2], [34.750_139_2, 127.747_713_6]],
                5_178_188_925,
            ),
            (
                [[34.749_896_1, 127.745_697_9], [34.748_973_2, 127.750_058_2]],
                12_669_768_610,
            ),
        ];
        for (points, junction) in cases {
            let planned = plan_route_detailed(&ways, None, "snapshot", &points, &config)
                .unwrap_or_else(|e| panic!("{points:?}: {e}"));
            assert!(planned.course.junctions.contains(&junction), "{points:?}");
            for speed in [2.0, 4.0, 6.0] {
                let result = follow_path(&planned.course.course, &planned.course.guide, speed);
                assert_eq!(result.impacts, 0, "{points:?} {speed} m/s: {result:?}");
                assert!(
                    result.remaining_m < 12.5,
                    "{points:?} {speed} m/s: {result:?}"
                );
            }
        }
    }

    /// 4-8 합성 자료: 동서로 J1(0, 0)–J2(120, 0) 직통로(way 201)와 북쪽으로 돌아가는 길(way 202, (0, 60)–(120, 60)),
    /// 양쪽 진입로(way 200, 203). `direct_tags`는 직통로 태그다.
    ///
    /// `spur`가 있으면 J2를 (120, 40)으로 올려 직통로가 가운데 노드 30(60, 0)에서 34° 꺾이게 하고, 노드 30에서 좁은 각으로
    /// 갈라지는 막다른 길을 더한다. 경로가 꺾는 곳이라 단순화할 수 없으므로 그 교차로를 피해야 한다.
    fn ladder(
        direct_tags: &'static [(&'static str, &'static str)],
        spur: bool,
    ) -> Vec<FixtureWay<'static>> {
        let j2 = if spur { 40.0 } else { 0.0 };
        let direct = if spur {
            vec![(10, 0.0, 0.0), (30, 60.0, 0.0), (11, 120.0, j2)]
        } else {
            vec![(10, 0.0, 0.0), (11, 120.0, j2)]
        };
        let mut ways: Vec<FixtureWay> = vec![
            (
                200,
                "residential",
                &[],
                vec![(1, -100.0, 0.0), (10, 0.0, 0.0)],
            ),
            (201, "residential", direct_tags, direct),
            (
                202,
                "residential",
                &[],
                vec![
                    (10, 0.0, 0.0),
                    (20, 0.0, 60.0),
                    (21, 120.0, 80.0),
                    (11, 120.0, j2),
                ],
            ),
            (
                203,
                "residential",
                &[],
                vec![(11, 120.0, j2), (2, 220.0, j2)],
            ),
        ];
        if spur {
            ways.push((
                204,
                "residential",
                &[],
                vec![(30, 60.0, 0.0), (31, 110.0, 15.0)],
            ));
        }
        ways
    }

    /// 일방통행: 정방향은 직통로로, 역방향은 북쪽으로 돌아간다. 예전 규칙(일방통행 제외)은 정방향도 돌아가고,
    /// 경로 파일의 노드를 거꾸로 고치면 재생에서 거부된다.
    #[test]
    fn one_way_roads_are_followed_in_their_direction() {
        let ways = parse(ladder(&[("oneway", "yes")], false));
        let config = RoadGraphConfig::default();
        let (west, east) = (at(-80.0, 0.0), at(200.0, 0.0));
        let forward = plan_route_detailed(&ways, None, "s", &[west, east], &config).unwrap();
        assert!(
            !forward.file.nodes.contains(&20),
            "{:?}",
            forward.file.nodes
        );
        assert!(forward.file.nav_rules);
        let backward = plan_route_detailed(&ways, None, "s", &[east, west], &config).unwrap();
        assert!(
            backward.file.nodes.contains(&20),
            "{:?}",
            backward.file.nodes
        );
        for planned in [&forward, &backward] {
            let result = follow_path(&planned.course.course, &planned.course.guide, 4.0);
            assert_eq!(result.impacts, 0, "{result:?}");
        }
        let legacy = RoadGraphConfig {
            nav_rules: false,
            ..config.clone()
        };
        let old = plan_route_detailed(&ways, None, "s", &[west, east], &legacy).unwrap();
        assert!(old.file.nodes.contains(&20), "{:?}", old.file.nodes);
        // 직통로를 역방향으로 지나도록 고친 파일은 거부한다.
        let mut against = backward.file.clone();
        let detour: Vec<u64> = against
            .nodes
            .iter()
            .copied()
            .filter(|&id| id != 20 && id != 21)
            .collect();
        against.nodes = detour;
        let error = build_route_file_course(&against, &ways, None).unwrap_err();
        assert!(error.contains("one-way"), "{error}");
        // 회전교차로 태그만 있어도 정방향 일방통행으로 본다.
        let roundabout = parse(ladder(&[("junction", "roundabout")], false));
        let backward = plan_route_detailed(&roundabout, None, "s", &[east, west], &config).unwrap();
        assert!(
            backward.file.nodes.contains(&20),
            "{:?}",
            backward.file.nodes
        );
    }

    /// 회전 제한: J1에서 직진 금지(`no_straight_on`)나 좌회전만 허용(`only_left_turn`)이면 북쪽으로 돌아가고,
    /// 제한 없이 만든 직통 경로는 제한이 있는 스냅샷으로 재생하면 거부된다.
    #[test]
    fn turn_restrictions_are_respected() {
        let fixture = ladder(&[], false);
        let plain = parse(fixture.clone());
        let config = RoadGraphConfig::default();
        let points = [at(-80.0, 0.0), at(200.0, 0.0)];
        let direct = plan_route_detailed(&plain, None, "s", &points, &config).unwrap();
        assert!(!direct.file.nodes.contains(&20), "{:?}", direct.file.nodes);
        for (value, to) in [("no_straight_on", 201), ("only_left_turn", 202)] {
            let json = fixtures::overpass_json_with(&fixture, &[(900, value, 200, 10, to)]);
            let body = crate::world::osm_graph::parse_graph_full(&json).unwrap();
            assert_eq!(body.restrictions.len(), 1, "{value}");
            let planned =
                plan_route_full(&body.ways, &body.restrictions, None, "s", &points, &config)
                    .unwrap();
            assert!(
                planned.file.nodes.contains(&20),
                "{value}: {:?}",
                planned.file.nodes
            );
            let error =
                build_route_file_course_full(&direct.file, &body.ways, &body.restrictions, None)
                    .unwrap_err();
            assert!(error.contains("turn restriction"), "{value}: {error}");
        }
        // 승용차 예외가 있거나 via가 way인 관계는 읽지 않는다.
        let json = fixtures::overpass_json(&fixture).replace(
            "]}",
            r#",{"type":"relation","id":901,"members":[{"type":"way","ref":200,"role":"from"},{"type":"way","ref":202,"role":"via"},{"type":"way","ref":201,"role":"to"}],"tags":{"type":"restriction","restriction":"no_straight_on"}},{"type":"relation","id":902,"members":[{"type":"way","ref":200,"role":"from"},{"type":"node","ref":10,"role":"via"},{"type":"way","ref":201,"role":"to"}],"tags":{"type":"restriction","restriction":"no_straight_on","except":"motorcar"}}]}"#,
        );
        let body = crate::world::osm_graph::parse_graph_full(&json).unwrap();
        assert!(body.restrictions.is_empty(), "{:?}", body.restrictions);
    }

    /// 우회: 직통로 가운데 교차로(노드 30)는 팔 사이 각이 좁아 만들 수 없으므로 그 교차로를 피해 북쪽으로 돌아간다.
    #[test]
    fn unbuildable_junctions_are_avoided() {
        let ways = parse(ladder(&[], true));
        let config = RoadGraphConfig::default();
        let points = [at(-80.0, 0.0), at(200.0, 40.0)];
        let planned = plan_route_detailed(&ways, None, "s", &points, &config).unwrap();
        assert_eq!(planned.avoided, vec![30]);
        assert!(planned.file.nodes.contains(&20), "{:?}", planned.file.nodes);
        let result = follow_path(&planned.course.course, &planned.course.guide, 4.0);
        assert_eq!(result.impacts, 0, "{result:?}");
        // 돌아갈 길이 없으면 피한 교차로 수와 함께 실패한다.
        let mut blocked = ladder(&[], true);
        blocked.retain(|way| way.0 != 202);
        let error = plan_route_detailed(&parse(blocked), None, "s", &points, &config).unwrap_err();
        assert!(error.contains("no detour around 1"), "{error}");
    }

    /// 4-8 여수 스냅샷: 두 점만으로 큰 도로(일방통행 `primary`·`primary_link`)를 따라가는 경로가 생기고, 곁가지 형상이
    /// 맞지 않는 교차로는 단순화되며, 경로 추종 조향기가 충돌 없이 끝 부근까지 간다. 예전 규칙으로는 경로가 없다.
    #[test]
    fn yeosu_two_points_follow_big_one_way_roads() {
        let ways = real_snapshot();
        let config = RoadGraphConfig::default();
        let points = [
            [34.752_844_4, 127.746_254_3],
            [34.758_249_15, 127.748_446_4],
        ];
        let planned = plan_route_detailed(&ways, None, "snapshot", &points, &config).unwrap();
        let used: BTreeSet<u64> = ways
            .iter()
            .filter(|way| {
                way.nodes.windows(2).any(|p| {
                    planned
                        .file
                        .nodes
                        .windows(2)
                        .any(|q| q == [p[0].id, p[1].id] || q == [p[1].id, p[0].id])
                })
            })
            .filter(|way| route::travel(way) == Travel::Forward)
            .map(|way| way.id)
            .collect();
        for way in [66_645_672, 128_379_958] {
            assert!(used.contains(&way), "{used:?}");
        }
        assert_eq!(
            planned.course.simplified,
            vec![1_748_929_006, 1_418_972_591]
        );
        assert!(
            planned
                .course
                .course
                .notices
                .iter()
                .any(|n| n.contains("Simplified 2")),
            "{:?}",
            planned.course.course.notices
        );
        for speed in [2.0, 4.0] {
            let result = follow_path(&planned.course.course, &planned.course.guide, speed);
            assert_eq!(result.impacts, 0, "{speed} m/s: {result:?}");
            assert!(result.remaining_m < 12.5, "{speed} m/s: {result:?}");
        }
        // 저장한 파일로 다시 만들면 같은 코스다.
        let rebuilt = build_route_file_course(&planned.file, &ways, None).unwrap();
        assert_eq!(rebuilt, planned.course);
        let legacy = RoadGraphConfig {
            allowed_highways: [
                "service",
                "pedestrian",
                "residential",
                "unclassified",
                "tertiary",
            ]
            .map(str::to_owned)
            .to_vec(),
            nav_rules: false,
            ..config
        };
        assert!(plan_route_detailed(&ways, None, "snapshot", &points, &legacy).is_err());
    }

    /// 저장·재생 시험에 쓰는 두 개의 끊긴 OSM service way를 만든다.
    fn patch_replay_osm() -> Vec<OsmGraphWay> {
        vec![
            OsmGraphWay {
                id: 10,
                tags: BTreeMap::from([("highway".to_owned(), "service".to_owned())]),
                nodes: vec![
                    OsmNode {
                        id: 1,
                        pos: LatLon {
                            lat: 35.0,
                            lon: 127.0,
                        },
                    },
                    OsmNode {
                        id: 2,
                        pos: LatLon {
                            lat: 35.0,
                            lon: 127.001,
                        },
                    },
                ],
            },
            OsmGraphWay {
                id: 20,
                tags: BTreeMap::from([("highway".to_owned(), "service".to_owned())]),
                nodes: vec![
                    OsmNode {
                        id: 3,
                        pos: LatLon {
                            lat: 35.0,
                            lon: 127.002,
                        },
                    },
                    OsmNode {
                        id: 4,
                        pos: LatLon {
                            lat: 35.0,
                            lon: 127.003,
                        },
                    },
                ],
            },
        ]
    }

    /// 저장·재생 시험에 쓰는 `map-patch-v1` TOML을 만든다.
    fn patch_replay_toml(id: &str, connector: bool) -> String {
        let (anchors, nodes, ways) = if connector {
            (
                r#"[[anchors]]
id = "west"
osm_node = 2
lat = 35.0
lon = 127.001
max_error_m = 1.0

[[anchors]]
id = "east"
osm_node = 3
lat = 35.0
lon = 127.002
max_error_m = 1.0"#,
                r#"[[nodes]]
id = "middle"
lat = 35.0001
lon = 127.0015"#,
                r#"[[ways]]
id = "connector"
nodes = ["anchor:west", "middle", "anchor:east"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
            )
        } else {
            (
                r#"[[anchors]]
id = "start"
osm_node = 1
lat = 35.0
lon = 127.0
max_error_m = 1.0"#,
                r#"[[nodes]]
id = "end"
lat = 35.0005
lon = 127.0"#,
                r#"[[ways]]
id = "spur"
nodes = ["anchor:start", "end"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
            )
        };
        format!(
            r#"format = "map-patch-v1"
id = "{id}"
name = "test patch"
revision = 1
bbox = [34.999, 126.999, 35.002, 127.004]
source_kind = "synthetic"
source_description = "unit test"
verified_at = "2026-11-13"
license = "test-only"
status = "synthetic"
geometry_accuracy = "synthetic"
access_assumption = "test only"
notice = "not a real road"

{anchors}
{nodes}
{ways}
"#
        )
    }

    /// 저장·재생 시험용 OSM 스냅샷 JSON을 만든다.
    fn patch_replay_snapshot() -> String {
        r#"{"generator":"Overpass API test","elements":[{"type":"way","id":10,"nodes":[1,2],"geometry":[{"lat":35.0,"lon":127.0},{"lat":35.0,"lon":127.001}],"tags":{"highway":"service"}},{"type":"way","id":20,"nodes":[3,4],"geometry":[{"lat":35.0,"lon":127.002},{"lat":35.0,"lon":127.003}],"tags":{"highway":"service"}}]}"#.to_owned()
    }

    /// 시험마다 겹치지 않는 임시 폴더를 새로 만든다.
    fn patch_replay_temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sds-map-patch-replay-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 경로 파일에는 A*가 실제로 지난 보정만 기록되고 TOML 왕복 뒤에도 같은 참조가 남는다.
    #[test]
    fn route_records_only_used_map_patches() {
        let bridge_text = patch_replay_toml("bridge", true);
        let spur_text = patch_replay_toml("unused-spur", false);
        let patches = [
            map_patch::parse_patch(bridge_text.as_bytes(), "bridge.toml", PatchAudience::Test)
                .unwrap(),
            map_patch::parse_patch(spur_text.as_bytes(), "unused.toml", PatchAudience::Test)
                .unwrap(),
        ];
        let graph = map_patch_graph::apply_map_patches(&patch_replay_osm(), &patches).unwrap();
        let planned = plan_route_full_with_patches(
            &graph,
            &[],
            None,
            "snapshot.json",
            &[[35.0, 127.0002], [35.0, 127.0028]],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        assert_eq!(planned.file.map_patches.len(), 1);
        assert_eq!(planned.file.map_patches[0].id, "bridge");
        assert_eq!(planned.file.map_patches[0].format, map_patch::FORMAT);
        assert_eq!(planned.file.map_patches[0].sha256, patches[0].sha256);

        let text = route_file_toml(&planned.file).unwrap();
        let reread: RouteFile = toml::from_str(&text).unwrap();
        assert_eq!(reread, planned.file);
        assert!(text.contains("[[map_patches]]"));
    }

    /// 동일한 보정 파일은 재생되고 누락·변조·revision 변경은 거부되며 원본 복구 뒤 같은 코스가 만들어진다.
    #[test]
    fn patched_route_replay_verifies_file_bytes() {
        let root = patch_replay_temp("verify");
        let patch_dir = root.join("patches");
        std::fs::create_dir_all(&patch_dir).unwrap();
        std::fs::write(root.join("snapshot.json"), patch_replay_snapshot()).unwrap();
        let patch_path = patch_dir.join("bridge.toml");
        let original = patch_replay_toml("bridge", true);
        std::fs::write(&patch_path, &original).unwrap();

        let parsed =
            map_patch::parse_patch(original.as_bytes(), &patch_path, PatchAudience::Test).unwrap();
        let graph = map_patch_graph::apply_map_patches(&patch_replay_osm(), &[parsed]).unwrap();
        let planned = plan_route_full_with_patches(
            &graph,
            &[],
            None,
            "snapshot.json",
            &[[35.0, 127.0002], [35.0, 127.0028]],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let first =
            load_route_course_with_patch_dir(&planned.file, &root, &patch_dir, PatchAudience::Test)
                .unwrap();

        std::fs::write(&patch_path, original.replace("unit test", "unit tests")).unwrap();
        let error =
            load_route_course_with_patch_dir(&planned.file, &root, &patch_dir, PatchAudience::Test)
                .unwrap_err();
        assert!(error.contains("SHA-256 mismatch"), "{error}");

        std::fs::write(
            &patch_path,
            original.replace("revision = 1", "revision = 2"),
        )
        .unwrap();
        let error =
            load_route_course_with_patch_dir(&planned.file, &root, &patch_dir, PatchAudience::Test)
                .unwrap_err();
        assert!(error.contains("SHA-256 mismatch"), "{error}");

        std::fs::remove_file(&patch_path).unwrap();
        let error =
            load_route_course_with_patch_dir(&planned.file, &root, &patch_dir, PatchAudience::Test)
                .unwrap_err();
        assert!(error.contains("missing map patch"), "{error}");

        std::fs::write(&patch_path, &original).unwrap();
        let restored =
            load_route_course_with_patch_dir(&planned.file, &root, &patch_dir, PatchAudience::Test)
                .unwrap();
        assert_eq!(restored, first);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// `map_patches`가 없는 기존 `route-v1`은 보정 폴더가 없어도 이전 OSM 전용 방식으로 재생된다.
    #[test]
    fn legacy_route_replay_does_not_require_patch_directory() {
        let root = patch_replay_temp("legacy");
        std::fs::write(root.join("snapshot.json"), patch_replay_snapshot()).unwrap();
        let mut file = plan_route(
            &patch_replay_osm(),
            "snapshot.json",
            &[[35.0, 127.0001], [35.0, 127.0009]],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        file.map_patches.clear();
        let text = route_file_toml(&file).unwrap();
        assert!(!text.contains("map_patches"));
        let reread: RouteFile = toml::from_str(&text).unwrap();
        load_route_course_with_patch_dir(
            &reread,
            &root,
            &root.join("does-not-exist"),
            PatchAudience::Distribution,
        )
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
