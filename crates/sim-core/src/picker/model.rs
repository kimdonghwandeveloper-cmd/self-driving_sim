//! 경로 선택 화면의 상태와 계산(Bevy와 무관한 순수 로직, Phase 4-5).
//!
//! 화면 좌표는 배경 지도 타일과 같은 웹 메르카토르 평면([`tiles::to_view`], x=동쪽, z=남쪽, 메르카토르 m)이다.
//! 대한민국 위도에서 메르카토르 1 m는 실제 약 0.8 m다. 코스 계산은 위경도로 한다.
//! 점을 찍거나 지울 때마다 [`route_plan::plan_route_full`]로 경로를 다시 계산하고, 길이·폭·경고 문구를 만든다.
//! 건물·장소(PLAN 4-9)를 고르면 그 진입 지점([`places::access_point`])을 점으로 더하고 장소 이름을 표시 이름으로 둔다.

use std::path::{Component, Path, PathBuf};

use glam::Vec2;

use super::{region, tiles};
use crate::world::map::LatLon;
use crate::world::osm_graph::{OsmGraphWay, TurnRestriction, node_degrees};
use crate::world::places::{self, Place};
use crate::world::road_snapshot::Connectivity;
use crate::world::route::Travel;
use crate::world::route_plan::{self, PlannedRoute, RoadGraphConfig, WidthSource};

/// 이보다 좁은 폭이면 경고한다(m). 차체 폭 1.8 m + 양옆 0.6 m(개발 코스 좁은 통로와 같은 기준, 임시값).
pub const NARROW_WIDTH_M: f32 = 3.0;
/// 그린 선을 도로에 붙인 점끼리 이보다 가까우면 합친다(m, 같은 곳에 붙은 두 점이 경로를 끊지 않게). 임시값.
pub const LINE_MIN_GAP_M: f64 = 20.0;
/// 그린 선에서 경유점을 두는 간격(m, 선을 따라 잼). 처음 값으로 안 되면(경로 실패·크게 돌아감) 다음 값으로 넓히고, 끝까지 안 되면
/// 출발·도착만으로 찾는다. 경유점이 촘촘하면 손떨림 때문에 경로가 되돌아가거나 블록을 돈다(2026-11-12 측정). 임시값.
pub const LINE_GAPS_M: [f64; 3] = [200.0, 400.0, 800.0];
/// 그린 선 방향과 도로 방향이 이보다 다르면 그 도로에 붙이지 않는다(deg, 옆길·건너편 일방통행 차로 방지). 임시값.
pub const LINE_MAX_ANGLE_DEG: f64 = 45.0;
/// 경유점은 교차로 노드에서 이만큼 떨어진 곳에만 둔다(m, 교차로 바로 너머 다른 팔에 붙는 것 방지). 임시값.
pub const LINE_JUNCTION_CLEAR_M: f64 = 20.0;
/// 그린 선의 출발·도착은 교차로 노드에서 이만큼 떨어진 점으로 한다(m, 코스는 교차로에서 시작·끝날 수 없음). 임시값.
pub const LINE_END_CLEAR_M: f64 = 5.0;
/// 그린 선을 따른 경로가 (선 길이 × 이 배수 + 200 m)보다 길면 돌아간 것으로 보고 경유점을 줄여 다시 찾는다. 임시값.
pub const LINE_DETOUR_FACTOR: f64 = 1.5;

/// 출발 또는 도착(PLAN 4-11 2단계).
#[derive(Debug, Clone, PartialEq)]
pub enum Endpoint {
    /// 열린 건물·장소 번호(그 진입 지점을 쓴다).
    Place(usize),
    /// 이름과 위치(전국 검색 결과·지도에서 누른 곳). 가장 가까운 탐색 가능 도로 위 점을 쓴다.
    Point {
        /// 표시 이름.
        name: String,
        /// 위치.
        at: LatLon,
    },
}

/// 지구 평균 반지름(m).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// `origin` 기준 등장방형 근사 지역 좌표 [동쪽 m, 북쪽 m].
fn local_m(origin: LatLon, point: LatLon) -> [f64; 2] {
    [
        (point.lon - origin.lon).to_radians() * EARTH_RADIUS_M * origin.lat.to_radians().cos(),
        (point.lat - origin.lat).to_radians() * EARTH_RADIUS_M,
    ]
}

/// `point`에서 `max_m` 안의 가장 가까운 도로 선분 위 점. 없으면 `None`. 거리는 `point` 기준 등장방형 근사(m)다.
///
/// `heading`(선의 진행 방향 단위 벡터 [동, 북])을 주면 그 방향과 [`LINE_MAX_ANGLE_DEG`] 안으로 달리는 선분만 쓴다. 양방향 도로는
/// 앞뒤 어느 쪽이든, 일방통행은 통행 방향만 맞으면 된다.
fn nearest_on_roads(
    roads: &[(&OsmGraphWay, Travel)],
    point: LatLon,
    heading: Option<[f64; 2]>,
    max_m: f64,
) -> Option<LatLon> {
    let cos = point.lat.to_radians().cos();
    let local = |p: LatLon| local_m(point, p);
    let min_cos = LINE_MAX_ANGLE_DEG.to_radians().cos();
    let mut best: Option<(f64, [f64; 2])> = None;
    for (way, travel) in roads {
        for pair in way.nodes.windows(2) {
            let (a, b) = (local(pair[0].pos), local(pair[1].pos));
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0].hypot(d[1]);
            if let Some(h) = heading {
                if length == 0.0 {
                    continue;
                }
                let along = (d[0] * h[0] + d[1] * h[1]) / length;
                let fits = match travel {
                    Travel::Forward => along >= min_cos,
                    Travel::Backward => -along >= min_cos,
                    Travel::Both => along.abs() >= min_cos,
                    Travel::Closed => false,
                };
                if !fits {
                    continue;
                }
            }
            if let Some(q) = closest_on_segment(a, d) {
                let distance = q[0].hypot(q[1]);
                if distance <= max_m && best.is_none_or(|(known, _)| distance < known) {
                    best = Some((distance, q));
                }
            }
        }
    }
    best.map(|(_, [east, north])| LatLon {
        lat: point.lat + (north / EARTH_RADIUS_M).to_degrees(),
        lon: point.lon + (east / (EARTH_RADIUS_M * cos)).to_degrees(),
    })
}

/// 원점에서 가장 가까운 선분(시작 `a`, 방향·길이 `d`) 위 점. 값이 유한하지 않으면 `None`.
fn closest_on_segment(a: [f64; 2], d: [f64; 2]) -> Option<[f64; 2]> {
    let length2 = d[0] * d[0] + d[1] * d[1];
    let t = if length2 == 0.0 {
        0.0
    } else {
        (-(a[0] * d[0] + a[1] * d[1]) / length2).clamp(0.0, 1.0)
    };
    let q = [a[0] + d[0] * t, a[1] + d[1] * t];
    (q[0].is_finite() && q[1].is_finite()).then_some(q)
}

/// 화면에 그릴 도로 선분.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadSegment {
    /// 시작점(지역 평면 좌표).
    pub from: Vec2,
    /// 끝점.
    pub to: Vec2,
    /// 경로 탐색에 쓰는 도로(허용 종류이고 탐색할 수 있는 방향이 있음)인지.
    pub usable: bool,
}

/// 경로 선택 상태.
#[derive(Debug, Clone)]
pub struct PickerModel {
    /// 스냅샷 파일 경로.
    pub snapshot_path: PathBuf,
    /// 스냅샷 way.
    pub ways: Vec<OsmGraphWay>,
    /// 경계 연결 정보(Geofabrik 추출). Overpass 스냅샷이면 `None`(연결 검증 정보 없음).
    pub connectivity: Option<Connectivity>,
    /// 회전 제한(PLAN 4-8). 관계를 받지 않은 스냅샷이면 비어 있다.
    pub restrictions: Vec<TurnRestriction>,
    /// 경로 계획 설정.
    pub config: RoadGraphConfig,
    /// 스냅샷 도로 범위의 가운데.
    pub origin: LatLon,
    /// 그릴 도로 선분.
    pub segments: Vec<RoadSegment>,
    /// 찍은 점 [위도, 경도](시작·경유·끝 순서).
    pub points: Vec<[f64; 2]>,
    /// 점마다 표시 이름(건물·장소로 고른 점은 장소 이름, 그냥 찍은 점은 빈 문자열). `points`와 길이가 같다.
    pub labels: Vec<String>,
    /// 받은 건물·장소(PLAN 4-9). 받지 않았으면 비어 있다.
    pub places: Vec<Place>,
    /// 장소 자료를 받았는지(받았는데 장소가 하나도 없을 수 있다).
    pub places_loaded: bool,
    /// 마지막 계산 결과. 점이 2개 미만이면 `None`.
    pub planned: Option<Result<PlannedRoute, String>>,
}

impl PickerModel {
    /// 스냅샷 way와 설정으로 상태를 만든다. way가 없으면 오류다.
    pub fn new(
        snapshot_path: PathBuf,
        ways: Vec<OsmGraphWay>,
        config: RoadGraphConfig,
    ) -> Result<Self, String> {
        config.validate()?;
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for node in ways.iter().flat_map(|way| &way.nodes) {
            low = [low[0].min(node.pos.lat), low[1].min(node.pos.lon)];
            high = [high[0].max(node.pos.lat), high[1].max(node.pos.lon)];
        }
        if !low[0].is_finite() {
            return Err("the snapshot has no road nodes".to_owned());
        }
        let origin = LatLon {
            lat: (low[0] + high[0]) / 2.0,
            lon: (low[1] + high[1]) / 2.0,
        };
        let allowed: std::collections::BTreeSet<&str> =
            config.allowed_highways.iter().map(String::as_str).collect();
        let segments = ways
            .iter()
            .flat_map(|way| {
                let usable = route_plan::searchable(way, &allowed, config.nav_rules);
                way.nodes.windows(2).map(move |pair| RoadSegment {
                    from: tiles::to_view(pair[0].pos),
                    to: tiles::to_view(pair[1].pos),
                    usable,
                })
            })
            .collect();
        Ok(Self {
            snapshot_path,
            ways,
            connectivity: None,
            restrictions: Vec::new(),
            config,
            origin,
            segments,
            points: Vec::new(),
            labels: Vec::new(),
            places: Vec::new(),
            places_loaded: false,
            planned: None,
        })
    }

    /// 받은 건물·장소를 바꾼다. 이미 찍은 점과 경로는 그대로 둔다.
    pub fn set_places(&mut self, places: Vec<Place>) {
        self.places = places;
        self.places_loaded = true;
    }

    /// 건물·장소 윤곽의 화면 좌표(점 장소는 점 하나).
    pub fn place_views(&self) -> Vec<Vec<Vec2>> {
        self.places
            .iter()
            .map(|place| {
                if place.outline.is_empty() {
                    vec![self.to_view(place.center)]
                } else {
                    place.outline.iter().map(|p| self.to_view(*p)).collect()
                }
            })
            .collect()
    }

    /// 화면 좌표 `view`가 가리키는 장소 번호([`places::hit_test`]). 없으면 `None`.
    pub fn place_at(&self, view: Vec2) -> Option<usize> {
        let [lat, lon] = self.from_view(view);
        places::hit_test(&self.places, LatLon { lat, lon })
    }

    /// 장소 `index`의 진입 지점을 점으로 끝에 더하고 경로를 다시 계산한다. 진입 지점이 없으면 오류이고 점은 그대로다.
    pub fn add_place(&mut self, index: usize) -> Result<(), String> {
        let place = self
            .places
            .get(index)
            .ok_or_else(|| format!("no place #{index}"))?;
        let at = places::access_point(place, &self.ways, &self.config)?;
        let label = place.label();
        self.points.push([at.lat, at.lon]);
        self.labels.push(label);
        self.recompute();
        Ok(())
    }

    /// 전국 검색 결과(이름, 위치)를 가장 가까운 탐색 가능 도로 위 점(4-9 진입 지점 규칙)으로 끝에 더하고 경로를 다시
    /// 계산한다. 도로에서 너무 멀면 오류이고 점은 그대로다.
    pub fn add_named_point(&mut self, name: &str, at: LatLon) -> Result<(), String> {
        let place = Place {
            id: String::new(),
            name: name.to_owned(),
            kind: String::new(),
            outline: Vec::new(),
            center: at,
            entrances: Vec::new(),
        };
        let access = places::access_point(&place, &self.ways, &self.config)?;
        self.points.push([access.lat, access.lon]);
        self.labels.push(name.to_owned());
        self.recompute();
        Ok(())
    }

    /// 출발·도착을 정하고(기존 점은 지운다) 경로를 다시 계산한다(PLAN 4-11 2단계 출발·도착 검색). 각 끝은 장소 번호면
    /// 그 진입 지점, 위치면 가장 가까운 탐색 가능 도로 위 점(4-9 규칙)이다. 한쪽이라도 도로에서 멀면 오류이고
    /// 오류문 앞에 `start: `·`end: `를 붙인다. 오류면 기존 점·경로를 그대로 둔다.
    pub fn set_endpoints(&mut self, ends: [Endpoint; 2]) -> Result<(), String> {
        let mut points = Vec::with_capacity(2);
        let mut labels = Vec::with_capacity(2);
        for (side, end) in ["start", "end"].into_iter().zip(ends) {
            let (at, label) = match end {
                Endpoint::Place(index) => {
                    let place = self
                        .places
                        .get(index)
                        .ok_or_else(|| format!("{side}: no place #{index}"))?;
                    let at = places::access_point(place, &self.ways, &self.config)
                        .map_err(|error| format!("{side}: {error}"))?;
                    (at, place.label())
                }
                Endpoint::Point { name, at } => {
                    let place = Place {
                        id: String::new(),
                        name: name.clone(),
                        kind: String::new(),
                        outline: Vec::new(),
                        center: at,
                        entrances: Vec::new(),
                    };
                    let at = places::access_point(&place, &self.ways, &self.config)
                        .map_err(|error| format!("{side}: {error}"))?;
                    (at, name)
                }
            };
            points.push([at.lat, at.lon]);
            labels.push(label);
        }
        self.points = points;
        self.labels = labels;
        self.recompute();
        Ok(())
    }

    /// 그린 선을 경로 점으로 바꾸고 경로를 다시 계산한다(PLAN 4-11 2단계 선으로 그리기). `line`은 이미 일정 간격으로 나눈 점이다.
    ///
    /// 1. 점마다 선 진행 방향과 [`LINE_MAX_ANGLE_DEG`] 안으로 달리는(일방통행은 통행 방향이 맞는) `max_m` 안의 가장 가까운 도로
    ///    위 점을 찾는다. 없는 점은 건너뛰고 그 수를 돌려준다.
    /// 2. 처음·끝 점을 출발·도착으로, 그 사이는 선을 따라 [`LINE_GAPS_M`]의 첫 간격마다(교차로에서 [`LINE_JUNCTION_CLEAR_M`] 떨어진 점만)
    ///    경유점으로 두고 계산한다.
    /// 3. 경로가 안 되거나 (선 길이 × [`LINE_DETOUR_FACTOR`] + 200 m)보다 길면 간격을 넓혀 다시, 끝까지 안 되면 출발·도착만으로
    ///    찾는다. 모두 안 되면 처음 계산된(돌아간) 경로를, 그것도 없으면 마지막 실패를 남긴다.
    ///
    /// 붙은 점이 2개 미만이면 오류이고 기존 점·경로는 그대로다.
    pub fn set_line(&mut self, line: &[LatLon], max_m: f64) -> Result<usize, String> {
        let allowed: std::collections::BTreeSet<&str> = self
            .config
            .allowed_highways
            .iter()
            .map(String::as_str)
            .collect();
        let nav_rules = self.config.nav_rules;
        let roads: Vec<(&OsmGraphWay, Travel)> = self
            .ways
            .iter()
            .filter(|way| route_plan::searchable(way, &allowed, nav_rules))
            .map(|way| (way, route_plan::way_travel(way, nav_rules)))
            .collect();
        let positions: std::collections::BTreeMap<u64, LatLon> = self
            .ways
            .iter()
            .flat_map(|way| way.nodes.iter().map(|node| (node.id, node.pos)))
            .collect();
        let junctions: Vec<LatLon> = node_degrees(&self.ways, &allowed)
            .into_iter()
            .filter(|&(_, degree)| degree >= 3)
            .filter_map(|(id, _)| positions.get(&id).copied())
            .collect();
        let snapped: Vec<Option<LatLon>> = (0..line.len())
            .map(|i| {
                let (from, to) = (line[i.saturating_sub(1)], line[(i + 1).min(line.len() - 1)]);
                let d = local_m(line[i], to);
                let back = local_m(line[i], from);
                let h = [d[0] - back[0], d[1] - back[1]];
                let length = h[0].hypot(h[1]);
                let heading = (length > 0.0).then(|| [h[0] / length, h[1] / length]);
                nearest_on_roads(&roads, line[i], heading, max_m)
            })
            .collect();
        let skipped = snapped.iter().filter(|p| p.is_none()).count();
        let found: Vec<usize> = (0..line.len()).filter(|&i| snapped[i].is_some()).collect();
        // 코스는 교차로에서 시작·끝날 수 없으므로 출발·도착은 교차로에서 [`LINE_END_CLEAR_M`] 떨어진 처음·마지막 점으로 한다
        // (그런 점이 없으면 처음·마지막 점).
        let away_from_junctions = |i: &&usize| {
            let p = snapped[**i].expect("found");
            junctions
                .iter()
                .all(|&j| region::distance_m(j, p) >= LINE_END_CLEAR_M)
        };
        let first = found.iter().find(away_from_junctions).or(found.first());
        let last = found
            .iter()
            .rev()
            .find(away_from_junctions)
            .or(found.last());
        let (Some(&first), Some(&last)) = (first, last) else {
            return Err(format!(
                "the drawn line does not follow a road (no point within {max_m} m of a road in the drawn direction)"
            ));
        };
        let (start, end) = (
            snapped[first].expect("found"),
            snapped[last].expect("found"),
        );
        if first >= last || region::distance_m(start, end) < LINE_MIN_GAP_M {
            return Err(format!(
                "the drawn line does not follow a road (only 1 point(s) within {max_m} m of a road in the drawn direction)"
            ));
        }
        let line_length: f64 = line
            .windows(2)
            .map(|pair| region::distance_m(pair[0], pair[1]))
            .sum();
        let clear_of_junctions = |p: LatLon| {
            junctions
                .iter()
                .all(|&j| region::distance_m(j, p) >= LINE_JUNCTION_CLEAR_M)
        };
        let mut fallback: Option<Vec<LatLon>> = None;
        for gap in LINE_GAPS_M.into_iter().chain([f64::INFINITY]) {
            let mut kept = vec![start];
            let mut since = 0.0;
            for i in first + 1..last {
                since += region::distance_m(line[i - 1], line[i]);
                let Some(p) = snapped[i] else {
                    continue;
                };
                if since >= gap
                    && clear_of_junctions(p)
                    && region::distance_m(*kept.last().expect("start"), p) >= LINE_MIN_GAP_M
                    && region::distance_m(p, end) >= LINE_MIN_GAP_M
                {
                    kept.push(p);
                    since = 0.0;
                }
            }
            kept.push(end);
            self.points = kept.iter().map(|p| [p.lat, p.lon]).collect();
            self.labels = vec![String::new(); kept.len()];
            self.recompute();
            if let Some(Ok(planned)) = &self.planned {
                if planned.length_m <= line_length * LINE_DETOUR_FACTOR + 200.0 {
                    return Ok(skipped);
                }
                if fallback.is_none() {
                    fallback = Some(kept);
                }
            }
        }
        if let Some(kept) = fallback {
            self.points = kept.iter().map(|p| [p.lat, p.lon]).collect();
            self.labels = vec![String::new(); kept.len()];
            self.recompute();
        }
        Ok(skipped)
    }

    /// 경계 연결 정보를 붙인다. 이후 경로 계산은 이 정보로 경계 연결 검사를 한다.
    pub fn with_connectivity(mut self, connectivity: Option<Connectivity>) -> Self {
        self.connectivity = connectivity;
        self
    }

    /// 회전 제한을 붙인다. 이후 경로 계산은 이 제한을 지킨다.
    pub fn with_restrictions(mut self, restrictions: Vec<TurnRestriction>) -> Self {
        self.restrictions = restrictions;
        self
    }

    /// 위경도를 화면 좌표로 바꾼다.
    pub fn to_view(&self, point: LatLon) -> Vec2 {
        tiles::to_view(point)
    }

    /// 화면 좌표를 [위도, 경도]로 바꾼다([`map::project`]의 역변환).
    pub fn from_view(&self, view: Vec2) -> [f64; 2] {
        let point = tiles::from_view(view);
        [point.lat, point.lon]
    }

    /// 도로 전체의 화면 범위 (최소, 최대).
    pub fn view_bounds(&self) -> (Vec2, Vec2) {
        bounds(self.segments.iter().flat_map(|s| [s.from, s.to]))
    }

    /// 계산된 경로의 화면 범위. 경로가 없으면 `None`.
    pub fn route_bounds(&self) -> Option<(Vec2, Vec2)> {
        let route = self.route_views();
        (route.len() >= 2).then(|| bounds(route))
    }

    /// 찍은 점들의 화면 좌표.
    pub fn point_views(&self) -> Vec<Vec2> {
        self.points
            .iter()
            .map(|&[lat, lon]| self.to_view(LatLon { lat, lon }))
            .collect()
    }

    /// 화면 좌표 위치에 점을 끝에 더하고 경로를 다시 계산한다.
    pub fn add_point(&mut self, view: Vec2) {
        self.points.push(self.from_view(view));
        self.labels.push(String::new());
        self.recompute();
    }

    /// 화면 좌표 `view`에서 `radius_m` 안의 가장 가까운 점을 지우고 경로를 다시 계산한다. 지웠으면 `true`.
    pub fn remove_near(&mut self, view: Vec2, radius_m: f32) -> bool {
        let nearest = self
            .point_views()
            .into_iter()
            .enumerate()
            .map(|(index, point)| (index, point.distance(view)))
            .filter(|(_, distance)| *distance <= radius_m)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = nearest else {
            return false;
        };
        self.points.remove(index);
        self.labels.remove(index);
        self.recompute();
        true
    }

    /// 마지막 점을 지우고 경로를 다시 계산한다.
    pub fn remove_last(&mut self) {
        if self.points.pop().is_some() {
            self.labels.pop();
            self.recompute();
        }
    }

    /// 찍은 점을 [위도, 경도] 목록으로 바꾸고 경로를 다시 계산한다(미리 찍은 점 불러오기용).
    pub fn set_points(&mut self, points: &[[f64; 2]]) {
        self.points = points.to_vec();
        self.labels = vec![String::new(); points.len()];
        self.recompute();
    }

    /// 모든 점을 지운다.
    pub fn clear(&mut self) {
        self.points.clear();
        self.labels.clear();
        self.planned = None;
    }

    /// 점이 2개 이상이면 경로를 다시 계산한다. 이름 있는 점이 있으면 경로 파일에 점 이름을 함께 둔다.
    fn recompute(&mut self) {
        let labels = if self.labels.iter().any(|label| !label.is_empty()) {
            self.labels.clone()
        } else {
            Vec::new()
        };
        self.planned = (self.points.len() >= 2).then(|| {
            route_plan::plan_route_full(
                &self.ways,
                &self.restrictions,
                self.connectivity.as_ref(),
                "",
                &self.points,
                &self.config,
            )
            .map(|mut planned| {
                planned.file.point_labels = labels;
                planned
            })
        });
    }

    /// 계산된 경로 노드의 화면 좌표. 계산되지 않았으면 비어 있다.
    pub fn route_views(&self) -> Vec<Vec2> {
        match &self.planned {
            Some(Ok(planned)) => planned.path.iter().map(|p| self.to_view(*p)).collect(),
            _ => Vec::new(),
        }
    }

    /// 화면 안내 문구: 점 개수, 경로 길이·폭·출처, 경고(좁은 폭·임시 폭), 오류(돌 수 없는 꺾임·거부된 교차로 등).
    pub fn status_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Points: {}", self.points.len())];
        lines.push(match &self.connectivity {
            Some(connectivity) => format!(
                "Road data: Geofabrik extract, boundary connections checked ({} incomplete node(s) near the edge)",
                connectivity.incomplete.len()
            ),
            None => "Road data: no boundary connectivity record (Overpass snapshot)".to_owned(),
        });
        lines.push(if self.restrictions.is_empty() {
            "Turn restrictions: none in this snapshot".to_owned()
        } else {
            format!("Turn restrictions: {} applied", self.restrictions.len())
        });
        lines.push(if self.places_loaded {
            format!(
                "Places: {} (click a building or search by name)",
                self.places.len()
            )
        } else {
            "Places: not loaded".to_owned()
        });
        let last = self.labels.len().saturating_sub(1);
        for (index, label) in self.labels.iter().enumerate() {
            if label.is_empty() {
                continue;
            }
            let role = match index {
                0 => "Start",
                i if i == last => "End",
                _ => "Via",
            };
            lines.push(format!("{role}: {label}"));
        }
        match &self.planned {
            None => lines.push("Click at least 2 points on the roads (start, end)".to_owned()),
            Some(Ok(planned)) => {
                let file = &planned.file;
                lines.push(format!(
                    "Route: {:.0} m, {} intersection(s)",
                    planned.length_m,
                    planned.course.junctions.len()
                ));
                if !planned.course.simplified.is_empty() {
                    lines.push(format!(
                        "Simplified {} intersection(s): side roads with unsupported shapes are not drawn",
                        planned.course.simplified.len()
                    ));
                }
                if !planned.avoided.is_empty() {
                    lines.push(format!(
                        "Detour: avoided {} intersection(s) that cannot be built",
                        planned.avoided.len()
                    ));
                }
                lines.push(format!(
                    "Width: {:.1} m ({})",
                    file.width_m, file.width_source
                ));
                if file.width_source != WidthSource::Tag.as_str() {
                    lines.push(
                        "Note: width is a temporary value, not from an OSM width tag".to_owned(),
                    );
                }
                if file.width_m < NARROW_WIDTH_M {
                    lines.push(format!(
                        "Warning: narrow road ({:.1} m < {NARROW_WIDTH_M} m)",
                        file.width_m
                    ));
                }
                lines.push("Press Enter or the button to start driving".to_owned());
            }
            Some(Err(error)) => lines.push(describe_error(error)),
        }
        lines
    }

    /// 계산된 경로를 `dir/name`에 경로 파일로 저장하고 경로를 돌려준다. 스냅샷 경로는 저장 폴더 기준 상대 경로다.
    pub fn save(&self, dir: &Path, name: &str) -> Result<PathBuf, String> {
        let Some(Ok(planned)) = &self.planned else {
            return Err("no valid route to save".to_owned());
        };
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut file = planned.file.clone();
        file.snapshot = relative_path(dir, &self.snapshot_path)?;
        let text = route_plan::route_file_toml(&file)?;
        let path = dir.join(name);
        std::fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(path)
    }
}

/// 점들의 (최소, 최대).
fn bounds(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
    points.into_iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

/// 경로 계획 오류를 화면 문구로 바꾼다. 꺾임·교차로 거부는 종류를 앞에 붙인다.
pub fn describe_error(error: &str) -> String {
    let kind = if error.contains("too sharp") || error.contains("short for the road width") {
        "Turn the vehicle cannot make"
    } else if error.contains("intersection")
        || error.contains("junction")
        || error.contains("one-way")
    {
        "Intersection rejected"
    } else if error.contains("overlap") {
        "Roads overlap"
    } else {
        "Route error"
    };
    format!("{kind}: {error}")
}

/// `from_dir`에서 `to_file`로 가는 상대 경로(`/` 구분)를 만든다. 드라이브가 다르면 절대 경로를 쓴다.
pub fn relative_path(from_dir: &Path, to_file: &Path) -> Result<String, String> {
    let canonical = |path: &Path| {
        path.canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()))
    };
    let (from, to) = (canonical(from_dir)?, canonical(to_file)?);
    let from_parts: Vec<Component> = from.components().collect();
    let to_parts: Vec<Component> = to.components().collect();
    if from_parts.first() != to_parts.first() {
        return Ok(to.to_string_lossy().replace('\\', "/"));
    }
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from_parts.len() - common];
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map;
    use crate::world::osm_graph::{fixtures, parse_overpass_graph};

    /// 저장소의 실제 스냅샷 경로.
    fn snapshot_path() -> PathBuf {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/graph-2026-11-06.osm.json")
    }

    /// 실제 스냅샷으로 상태를 만든다.
    fn yeosu() -> PickerModel {
        let path = snapshot_path();
        let ways = parse_overpass_graph(&std::fs::read_to_string(&path).unwrap()).unwrap();
        PickerModel::new(path, ways, RoadGraphConfig::default()).unwrap()
    }

    /// 화면 좌표 ↔ 위경도 변환이 서로 역변환인지 확인한다.
    #[test]
    fn view_coordinates_round_trip() {
        let model = yeosu();
        let point = LatLon {
            lat: 34.75264,
            lon: 127.74712,
        };
        let view = model.to_view(point);
        let [lat, lon] = model.from_view(view);
        assert!((lat - point.lat).abs() < 1e-7 && (lon - point.lon).abs() < 1e-7);
        assert!(model.segments.iter().any(|s| s.usable));
        assert!(model.segments.iter().any(|s| !s.usable));
    }

    /// 점 찍기·지우기: 두 점이면 경로가 계산되고 안내 문구가 나오며, 지우면 다시 계산하고, 멀리서는 지우지 않는다.
    #[test]
    fn points_drive_route_planning() {
        let mut model = yeosu();
        let start = model.to_view(LatLon {
            lat: 34.75264,
            lon: 127.74712,
        });
        let end = model.to_view(LatLon {
            lat: 34.74972,
            lon: 127.74816,
        });
        model.add_point(start);
        assert!(model.planned.is_none());
        model.add_point(end);
        let Some(Ok(planned)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert!(
            (300.0..800.0).contains(&planned.length_m),
            "{}",
            planned.length_m
        );
        for junction in [5_143_358_021, 5_143_358_070, 5_239_572_002] {
            assert!(planned.course.junctions.contains(&junction));
        }
        let route = model.route_views();
        assert!(route.first().unwrap().distance(start) < 30.0);
        assert!(route.last().unwrap().distance(end) < 30.0);
        let lines = model.status_lines().join("\n");
        assert!(
            lines.contains("Route:") && lines.contains("temporary"),
            "{lines}"
        );
        // 같은 점이면 저장소 예제 경로 파일과 같은 노드 순서다.
        let example: route_plan::RouteFile = toml::from_str(
            &std::fs::read_to_string(
                Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                    .join("testdata/yeosu-expo/route-junctions.toml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(planned.file.nodes, example.nodes);
        assert!(!model.remove_near(start + Vec2::new(50.0, 0.0), 10.0));
        assert!(model.remove_near(start + Vec2::new(2.0, 0.0), 10.0));
        assert_eq!(model.points.len(), 1);
        assert!(model.planned.is_none());
        model.remove_last();
        assert!(model.points.is_empty());
    }

    /// 저장한 경로 파일을 `--course map:` 로더로 다시 열면 같은 코스가 나온다(PLAN 4-5 완료 기준).
    #[test]
    fn saved_route_reopens_as_same_course() {
        let mut model = yeosu();
        for point in [[34.75264, 127.74712], [34.74972, 127.74816]] {
            let view = model.to_view(LatLon {
                lat: point[0],
                lon: point[1],
            });
            model.add_point(view);
        }
        let dir = std::env::temp_dir().join(format!("sds-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = model.save(&dir, "route.toml").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("graph-2026-11-06.osm.json"), "{text}");
        assert!(!text.contains('\\'), "{text}");
        let loaded = map::load_map_course(&path).unwrap();
        let Some(Ok(planned)) = &model.planned else {
            panic!();
        };
        assert_eq!(loaded, planned.course.course);
        assert_eq!(map::load_map_course(&path).unwrap(), loaded);
        let _ = std::fs::remove_dir_all(&dir);
        let empty = PickerModel {
            planned: None,
            ..model
        };
        assert!(empty.save(&dir, "x.toml").is_err());
    }

    /// 경고·오류 문구: 좁은 폭 경고, 돌 수 없는 꺾임, 거부된 교차로를 구분해 보여 준다.
    #[test]
    fn warnings_name_the_problem() {
        let config = RoadGraphConfig {
            default_width_m: [("service".to_owned(), 2.5)].into_iter().collect(),
            allowed_highways: vec!["service".to_owned()],
            ..RoadGraphConfig::default()
        };
        let straight: Vec<fixtures::FixtureWay> =
            vec![(1, "service", &[], vec![(1, 0.0, 0.0), (2, 100.0, 0.0)])];
        let ways = parse_overpass_graph(&fixtures::overpass_json(&straight)).unwrap();
        let mut narrow = PickerModel::new(PathBuf::from("f.json"), ways, config).unwrap();
        let origin = narrow.to_view(fixtures::offset(0.0, 0.0));
        narrow.add_point(origin + Vec2::new(10.0, 0.0));
        narrow.add_point(origin + Vec2::new(90.0, 0.0));
        assert!(
            narrow.status_lines().join("\n").contains("narrow road"),
            "{:?}",
            narrow.status_lines()
        );
        let ways =
            parse_overpass_graph(&fixtures::overpass_json(&fixtures::narrow_arms())).unwrap();
        // 예전 규칙은 좁은 팔 교차로를 거부하고, 4-8 규칙은 곧게 지나는 그 교차로를 단순화한다.
        let legacy = RoadGraphConfig {
            nav_rules: false,
            ..RoadGraphConfig::default()
        };
        for (config, expected) in [
            (legacy, "Intersection rejected"),
            (RoadGraphConfig::default(), "Simplified 1 intersection"),
        ] {
            let mut junction =
                PickerModel::new(PathBuf::from("f.json"), ways.clone(), config).unwrap();
            let origin = junction.to_view(fixtures::offset(0.0, 0.0));
            junction.add_point(origin + Vec2::new(-40.0, 0.0));
            junction.add_point(origin + Vec2::new(40.0, 0.0));
            let lines = junction.status_lines().join("\n");
            assert!(lines.contains(expected), "{lines}");
        }
        assert!(describe_error("bend is too sharp").starts_with("Turn the vehicle cannot make"));
        assert!(describe_error("no road path").starts_with("Route error"));
    }

    /// 상대 경로: 같은 드라이브면 `..`로 올라가 `/`로 잇는다.
    #[test]
    fn relative_path_walks_up_and_down() {
        let root = std::env::temp_dir().join(format!("sds-rel-{}", std::process::id()));
        let (a, b) = (root.join("cache/routes"), root.join("assets/maps"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("g.json"), "{}").unwrap();
        assert_eq!(
            relative_path(&a, &b.join("g.json")).unwrap(),
            "../../assets/maps/g.json"
        );
        assert!(relative_path(&a, &b.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 4-9: 합성 장소 자료에서 이름 검색으로 출발 건물을, 윤곽 클릭으로 도착 건물을 고르면 진입 지점으로 경로가
    /// 계산되고, 안내에 출발·도착 이름이 나오며, 저장한 경로 파일에 점 이름이 남고 같은 코스로 다시 열린다.
    #[test]
    fn buildings_are_picked_by_search_and_click() {
        let mut model = yeosu();
        assert!(
            model
                .status_lines()
                .iter()
                .any(|l| l == "Places: not loaded")
        );
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("testdata/yeosu-expo/places-synthetic.osm.json");
        model.set_places(places::load_places(&path).unwrap());
        let found = places::search(&model.places, "도서관");
        assert_eq!(found.len(), 1);
        model.add_place(found[0]).unwrap();
        let gym = places::search(&model.places, "체육관")[0];
        let center = model.to_view(model.places[gym].center);
        let clicked = model.place_at(center).unwrap();
        assert_eq!(clicked, gym);
        model.add_place(clicked).unwrap();
        let lines = model.status_lines();
        for expected in [
            "Places: 3 (click a building or search by name)",
            "Start: 시험 도서관 (합성)",
            "End: 시험 체육관 (합성)",
        ] {
            assert!(lines.iter().any(|l| l == expected), "{lines:?}");
        }
        let Some(Ok(planned)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert_eq!(
            planned.file.point_labels,
            ["시험 도서관 (합성)", "시험 체육관 (합성)"]
        );
        let dir = std::env::temp_dir().join(format!("picker-places-{}", std::process::id()));
        let saved = model.save(&dir, "places.toml").unwrap();
        let text = std::fs::read_to_string(&saved).unwrap();
        assert!(text.contains("point_labels"), "{text}");
        assert_eq!(map::load_map_course(&saved).unwrap(), planned.course.course);
        let _ = std::fs::remove_dir_all(&dir);
        // 점을 지우면 이름도 같이 지워진다.
        model.remove_last();
        assert_eq!(model.labels, ["시험 도서관 (합성)"]);
        model.add_point(center);
        assert_eq!(model.labels, ["시험 도서관 (합성)", ""]);
    }

    /// 전국 검색 결과는 가까운 도로 위 점으로 이름과 함께 더해지고, 도로에서 먼 곳은 거부된다.
    #[test]
    fn search_hits_become_named_points_on_roads() {
        let mut model = yeosu();
        let gym = {
            let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                .join("testdata/yeosu-expo/places-synthetic.osm.json");
            let list = places::load_places(&path).unwrap();
            let index = places::search(&list, "체육관")[0];
            list[index].center
        };
        model.add_named_point("여수 체육관", gym).unwrap();
        assert_eq!(model.labels, ["여수 체육관"]);
        let far = LatLon {
            lat: gym.lat + 0.05,
            lon: gym.lon,
        };
        assert!(model.add_named_point("먼 곳", far).is_err());
        assert_eq!(model.points.len(), 1, "failed hit must not add a point");
    }

    /// 여수 예제 경로의 출발·도착 위치.
    fn yeosu_ends() -> [LatLon; 2] {
        [
            LatLon {
                lat: 34.75264,
                lon: 127.74712,
            },
            LatLon {
                lat: 34.74972,
                lon: 127.74816,
            },
        ]
    }

    /// 출발·도착을 한 번에 정하면 경로가 계산되고 이름이 붙는다. 한쪽이 도로에서 멀면 어느 쪽인지 알리고 기존 경로를 둔다.
    #[test]
    fn endpoints_set_start_and_end_together() {
        let mut model = yeosu();
        let [start, end] = yeosu_ends();
        let point = |name: &str, at| Endpoint::Point {
            name: name.to_owned(),
            at,
        };
        model
            .set_endpoints([point("출발", start), point("도착", end)])
            .unwrap();
        assert_eq!(model.labels, ["출발", "도착"]);
        assert!(matches!(model.planned, Some(Ok(_))), "{:?}", model.planned);
        let far = LatLon {
            lat: end.lat + 0.05,
            lon: end.lon,
        };
        let error = model
            .set_endpoints([point("출발", start), point("먼 곳", far)])
            .unwrap_err();
        assert!(error.starts_with("end: "), "{error}");
        assert_eq!(model.labels, ["출발", "도착"], "failed ends keep the route");
        assert!(
            model
                .set_endpoints([Endpoint::Place(99), point("도착", end)])
                .is_err()
        );
    }

    /// 실제 자료 조각(`testdata/korea-yeosu`): 출발·도착으로 만든 경로를 그대로 선으로 그리면 같은 길이의 경로가 된다.
    #[test]
    fn drawn_line_matches_clicked_route_on_real_data() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let data =
            crate::world::korea_data::KoreaData::open(&root.join("testdata/korea-yeosu")).unwrap();
        let [start, end] = yeosu_ends();
        let area = region::route_area(&[start, end]).unwrap();
        let cache = std::env::temp_dir().join(format!("sds-line-real-{}", std::process::id()));
        let path = data.route_snapshot(&area, &cache).unwrap();
        let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
        let mut model = PickerModel::new(path, snapshot.ways, RoadGraphConfig::default())
            .unwrap()
            .with_restrictions(snapshot.restrictions);
        let point = |at| Endpoint::Point {
            name: String::new(),
            at,
        };
        model.set_endpoints([point(start), point(end)]).unwrap();
        let Some(Ok(clicked)) = model.planned.clone() else {
            panic!("{:?}", model.planned);
        };
        let line = region::resample_line(&clicked.path, region::LINE_STEP_M);
        model.clear();
        model.set_line(&line, region::LINE_SNAP_M).unwrap();
        let Some(Ok(drawn)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert!(
            (drawn.length_m - clicked.length_m).abs() < clicked.length_m * 0.2,
            "{} vs {} (points {:?}, line {} samples)",
            drawn.length_m,
            clicked.length_m,
            model.points,
            line.len()
        );
        let _ = std::fs::remove_dir_all(&cache);
    }

    /// 선으로 그리기: 도로를 따라 그린 선은 그 도로로 경로가 되고, 도로에서 먼 점은 건너뛰어 개수를 알린다.
    /// 도로를 전혀 따르지 않는 선은 오류다.
    #[test]
    fn drawn_line_follows_roads() {
        let mut model = yeosu();
        let [start, end] = yeosu_ends();
        model.set_points(&[[start.lat, start.lon], [end.lat, end.lon]]);
        let Some(Ok(planned)) = model.planned.clone() else {
            panic!("{:?}", model.planned);
        };
        // 계산된 경로를 사람이 따라 그린 선으로 쓰고, 가운데에 도로에서 먼 점 하나를 끼운다.
        let mut line = region::resample_line(&planned.path, region::LINE_STEP_M);
        let middle = line.len() / 2;
        let off = LatLon {
            lat: line[middle].lat + 0.01,
            lon: line[middle].lon,
        };
        line.insert(middle, off);
        model.clear();
        let skipped = model.set_line(&line, region::LINE_SNAP_M).unwrap();
        // 먼 점과, 그 점 때문에 진행 방향이 틀어진 앞뒤 점이 건너뛰어진다.
        assert!((1..=3).contains(&skipped), "{skipped}");
        let Some(Ok(drawn)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert!(
            (drawn.length_m - planned.length_m).abs() < planned.length_m * 0.1,
            "{} vs {}",
            drawn.length_m,
            planned.length_m
        );
        assert!(model.points.len() > 2);
        // 손떨림처럼 좌우로 8 m 흔들린 선도(예전 방식은 5 m에서 되돌아감·고리로 실패) 같은 길로 경로가 된다.
        let samples = region::resample_line(&planned.path, region::LINE_STEP_M);
        let shaky: Vec<LatLon> = samples
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let north = 8.0 * ((i as f64) * 1.3).sin();
                LatLon {
                    lat: p.lat + (north / EARTH_RADIUS_M).to_degrees(),
                    lon: p.lon,
                }
            })
            .collect();
        model.clear();
        model.set_line(&shaky, region::LINE_SNAP_M).unwrap();
        let Some(Ok(wobbly)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert!(
            (wobbly.length_m - planned.length_m).abs() < planned.length_m * 0.1,
            "{} vs {}",
            wobbly.length_m,
            planned.length_m
        );
        let away: Vec<LatLon> = line
            .iter()
            .map(|p| LatLon {
                lat: p.lat + 0.02,
                lon: p.lon,
            })
            .collect();
        let before = model.points.clone();
        assert!(model.set_line(&away, region::LINE_SNAP_M).is_err());
        assert_eq!(model.points, before, "failed line keeps the route");
    }
}
