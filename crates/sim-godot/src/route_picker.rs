//! 경로 선택 `SimRoutePicker`(Godot 인터페이스 계약 v1 7-2).
//!
//! 스냅샷 읽기·점 편집·경로 계산·저장은 `sim_core` 코드를 쓴다. 경로는 모두 저장소 루트 기준이다
//! (`data/korea`, `cache/regions`, `cache/routes`, `config/road_graph.toml`).
//!
//! 지역 열기(PLAN 4-11): 네트워크로 받지 않고 내장 전국 자료([`KoreaData`])에서 가운데 1.5 km 지역의 도로·건물·장소를
//! 바로 꺼낸다(1 s 미만이라 같은 프레임에서 끝낸다). 도로는 지역 스냅샷 파일로 써서 경로 파일이 가리키게 한다.
//! 건물 클릭·이름 검색으로 고른 장소의 진입 지점을 점으로 더한다(PLAN 4-9).
//!
//! 전국 장소 검색(PLAN 4-10): Nominatim에 묻는 검색은 백그라운드에서 하나씩 하고(`cache/geocode`에 저장),
//! 결과는 `poll()`의 `search_results` 이벤트로 돌려준다.

use std::path::{Path, PathBuf};

use godot::prelude::*;
use sim_core::picker::geocode::{GEOCODE_CACHE_DIR, GeocodeHit, GeocodeJob, UreqGet};
use sim_core::picker::model::{Endpoint, PickerModel};
use sim_core::picker::region::{
    self, LINE_SNAP_M, LINE_STEP_M, ROUTE_DIR, bbox_around, load_model, resample_line, route_area,
    snapshot_graph_path, valid_route_name,
};
use sim_core::picker::tiles;
use sim_core::world::korea_data::{KOREA_DATA_DIR, KoreaData, REGION_CACHE_DIR};
use sim_core::world::map_fetch::Bbox;
use sim_core::world::places;
use sim_core::world::route_plan::ROAD_GRAPH_CONFIG_PATH;

use crate::paths::{repo_path, resolve_user_path};

/// 메르카토르 투영이 정의되는 위도 한계(도).
const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

/// 지역 범위를 지도 좌표 `Rect2`로 바꾼다.
fn bbox_rect(bbox: &Bbox) -> Rect2 {
    let (min, max) = tiles::view_rect([bbox.south, bbox.west, bbox.north, bbox.east]);
    Rect2::new(
        Vector2::new(min.x, min.y),
        Vector2::new(max.x - min.x, max.y - min.y),
    )
}

/// 끝의 위치(장소면 열린 장소의 가운데). 경로 범위 계산용.
fn endpoint_at(
    end: &Endpoint,
    picker: &SimRoutePicker,
) -> Result<sim_core::world::map::LatLon, String> {
    match end {
        Endpoint::Point { at, .. } => Ok(*at),
        Endpoint::Place(index) => picker
            .model
            .as_ref()
            .and_then(|model| model.places.get(*index))
            .map(|place| place.center)
            .ok_or_else(|| format!("no place #{index}")),
    }
}

/// `{ok, error}` 사전을 만든다.
fn result(ok: bool, error: &str) -> VarDictionary {
    vdict! { "ok" => ok, "error" => error }
}

/// 진행 이벤트 사전(`{type, message, request_id}`)을 만든다.
fn event(kind: &str, message: &str, request_id: i64) -> VarDictionary {
    vdict! { "type" => kind, "message" => message, "request_id" => request_id }
}

/// 경로 선택 상태. 화면(Godot)은 표시·입력만 하고 계산은 여기서 한다.
#[derive(GodotClass)]
#[class(init, base = RefCounted)]
pub struct SimRoutePicker {
    /// `false`면 전국 장소 검색이 저장한 결과만 쓴다. 헤드리스 시험은 `false`. 지역 열기는 네트워크를 쓰지 않는다.
    #[var]
    #[init(val = true)]
    network: bool,
    /// 전국 자료 폴더(저장소 루트 기준). 비어 있으면 [`KOREA_DATA_DIR`]. 시험이 작은 자료를 가리킬 때 바꾼다.
    data_dir: String,
    /// 열어 둔 전국 자료(처음 지역을 열 때 연다).
    korea: Option<KoreaData>,
    model: Option<PickerModel>,
    /// 지금 열린 지역 범위(전국 자료에서 연 경우).
    loaded: Option<Bbox>,
    /// 진행 중인 전국 검색(작업, 요청 번호).
    searching: Option<(GeocodeJob, i64)>,
    /// 마지막 전국 검색 결과.
    hits: Vec<GeocodeHit>,
    /// 다음 요청 번호(1부터).
    next_request: i64,
    /// 다음 `poll()`에 돌려줄 이벤트.
    events: Vec<VarDictionary>,
    /// 마지막 상태 안내(받기 진행·경고·오류).
    message: String,
}

#[godot_api]
impl SimRoutePicker {
    /// 스냅샷(폴더 또는 그 안의 `graph.osm.json`)을 연다. 실패하면 이전 상태를 그대로 둔다.
    #[func]
    fn load_snapshot(&mut self, path: GString) -> VarDictionary {
        let path = snapshot_graph_path(&resolve_user_path(&path.to_string()));
        match self.open_model(&path) {
            Ok(()) => {
                self.loaded = None;
                result(true, "")
            }
            Err(error) => result(false, &error),
        }
    }

    /// 전국 자료 폴더를 바꾼다(저장소 루트 기준, 빈 문자열이면 기본 `data/korea`). 열어 둔 자료는 닫고 다음에 다시 연다.
    #[func]
    fn use_data_dir(&mut self, dir: GString) {
        self.data_dir = dir.to_string();
        self.korea = None;
    }

    /// 전국 자료 상태 `{ok, error, data_time}`. 없거나 형식이 다르면 `ok = false`와 만드는 방법 안내(한국어).
    /// `data_time`은 원본 PBF 복제 시각(유닉스 초, 없으면 빈 문자열).
    #[func]
    fn data_status(&mut self) -> VarDictionary {
        match self.korea_data() {
            Ok(data) => {
                let time = data
                    .manifest
                    .source
                    .replication_timestamp
                    .map_or_else(String::new, |seconds| seconds.to_string());
                vdict! { "ok" => true, "error" => "", "data_time" => time.as_str() }
            }
            Err(error) => {
                let error = crate::korean::region_error(&error);
                vdict! { "ok" => false, "error" => error.as_str(), "data_time" => "" }
            }
        }
    }

    /// 지금 열린 지역 범위. 전국 자료에서 열지 않았으면 스냅샷 도로 범위, 아무것도 없으면 크기 0.
    #[func]
    fn loaded_region(&self) -> Rect2 {
        if let Some(bbox) = &self.loaded {
            return bbox_rect(bbox);
        }
        match &self.model {
            Some(model) if !model.segments.is_empty() => {
                let (min, max) = model.view_bounds();
                Rect2::new(
                    Vector2::new(min.x, min.y),
                    Vector2::new(max.x - min.x, max.y - min.y),
                )
            }
            _ => Rect2::default(),
        }
    }

    /// 지도 좌표를 `[lat, lon]`으로 바꾼다. 유한하지 않거나 메르카토르 유효 위도 밖이면 빈 배열.
    #[func]
    fn view_to_latlon(&self, view: Vector2) -> PackedFloat64Array {
        if !(view.x.is_finite() && view.y.is_finite()) {
            return PackedFloat64Array::new();
        }
        let point = tiles::from_view(glam::Vec2::new(view.x, view.y));
        if !(point.lat.is_finite() && point.lon.is_finite()) || point.lat.abs() > MAX_MERCATOR_LAT {
            return PackedFloat64Array::new();
        }
        PackedFloat64Array::from(&[point.lat, point.lon])
    }

    /// `[lat, lon]`을 지도 좌표로 바꾼다. 형식이 맞지 않으면 원점.
    #[func]
    fn latlon_to_view(&self, latlon: PackedFloat64Array) -> Vector2 {
        let [lat, lon] = latlon.as_slice() else {
            return Vector2::ZERO;
        };
        let view = tiles::to_view(sim_core::world::map::LatLon {
            lat: *lat,
            lon: *lon,
        });
        Vector2::new(view.x, view.y)
    }

    /// 가운데 `[lat, lon]` 주변 1.5 × 1.5 km 지역의 도로·건물·장소를 전국 자료에서 연다 `{ok, error}`. 바로 끝나며
    /// 점·경로는 비워진다. 실패하면 이전 지역을 그대로 둔다.
    #[func]
    fn open_region(&mut self, center_latlon: PackedFloat64Array) -> VarDictionary {
        let [lat, lon] = center_latlon.as_slice() else {
            return result(false, "center_latlon must be [lat, lon]");
        };
        let opened = bbox_around(*lat, *lon).and_then(|bbox| {
            let cache_root = repo_path(REGION_CACHE_DIR);
            let (path, list) = self.korea_data()?.region_snapshot(&bbox, &cache_root)?;
            self.open_model(&path)?;
            if let Some(model) = self.model.as_mut() {
                model.set_places(list);
            }
            Ok(bbox)
        });
        match opened {
            Ok(bbox) => {
                self.loaded = Some(bbox);
                self.message = "지역 도로·건물을 열었습니다".to_owned();
                result(true, "")
            }
            Err(error) => {
                let error = crate::korean::region_error(&error);
                self.message = error.clone();
                result(false, &error)
            }
        }
    }

    /// 출발·도착으로 경로를 만든다 `{ok, error}`(PLAN 4-11 2단계). 각 끝은 `{place: int, latlon: [lat, lon], label: String}`이고
    /// `place`가 0 이상이면 열린 건물·장소 번호(진입 지점), 아니면 `latlon` 위치(가까운 도로 위 점)를 쓴다. 두 끝을 감싸는 경로
    /// 범위의 도로를 전국 자료에서 열어 계산한다(건물·장소는 그대로 둔다). 실패하면 이전 상태를 그대로 두고 `error`는 한국어다.
    /// 경로 계산 자체의 실패(돌 수 없는 교차로 등)는 `ok = true`이고 `route()`의 `error`로 알린다.
    #[func]
    fn plan_endpoints(&mut self, start: VarDictionary, end: VarDictionary) -> VarDictionary {
        let built = self.endpoint(&start).and_then(|a| {
            let b = self.endpoint(&end)?;
            let area = route_area(&[endpoint_at(&a, self)?, endpoint_at(&b, self)?])?;
            let mut model = self.route_model(&area)?;
            model.set_endpoints([a, b])?;
            Ok((model, area))
        });
        self.finish_route(built.map(|(model, area)| (model, area, 0)))
    }

    /// 지도 위에 그린 선(지도 좌표)으로 경로를 만든다 `{ok, error, skipped}`(PLAN 4-11 2단계). 선을 50 m 간격으로 나눠
    /// 40 m 안의 도로에 붙이고 그 순서대로 경로를 계산한다. `skipped`는 붙일 도로가 없어 건너뛴 점 수다. 실패하면 이전 상태를 둔다.
    #[func]
    fn plan_line(&mut self, line: PackedVector2Array) -> VarDictionary {
        let drawn: Vec<sim_core::world::map::LatLon> = line
            .as_slice()
            .iter()
            .map(|view| tiles::from_view(glam::Vec2::new(view.x, view.y)))
            .collect();
        let built = (|| {
            if drawn.len() < 2 {
                return Err("draw a line along roads".to_owned());
            }
            let samples = resample_line(&drawn, LINE_STEP_M);
            let area = route_area(&samples)?;
            let mut model = self.route_model(&area)?;
            let skipped = model.set_line(&samples, LINE_SNAP_M)?;
            Ok((model, area, skipped))
        })();
        self.finish_route(built)
    }

    /// 장소 파일(`places.osm.json` 형식)을 직접 연다(시험·수동 확인용). 지역이 없으면 실패한다.
    #[func]
    fn load_places_file(&mut self, path: GString) -> VarDictionary {
        let Some(model) = self.model.as_mut() else {
            return result(false, "open a region first");
        };
        match places::load_places(&resolve_user_path(&path.to_string())) {
            Ok(list) => {
                model.set_places(list);
                result(true, "")
            }
            Err(error) => result(false, &error),
        }
    }

    /// 건물·장소 윤곽(지도 좌표). 점 장소는 점 하나짜리 배열이다.
    #[func]
    fn place_outlines(&self) -> Array<PackedVector2Array> {
        self.model
            .iter()
            .flat_map(PickerModel::place_views)
            .map(|outline| {
                outline
                    .into_iter()
                    .map(|p| Vector2::new(p.x, p.y))
                    .collect::<PackedVector2Array>()
            })
            .collect()
    }

    /// 지도 좌표가 가리키는 장소 번호. 없으면 -1.
    #[func]
    fn place_at(&self, view: Vector2) -> i64 {
        self.model
            .as_ref()
            .and_then(|model| model.place_at(glam::Vec2::new(view.x, view.y)))
            .map_or(-1, |index| index as i64)
    }

    /// 이름 검색 결과 `[{index, name, kind}]`(최대 20개).
    #[func]
    fn search_places(&self, text: GString) -> Array<VarDictionary> {
        let Some(model) = &self.model else {
            return Array::new();
        };
        places::search(&model.places, &text.to_string())
            .into_iter()
            .map(|index| {
                let place = &model.places[index];
                vdict! {
                    "index" => index as i64,
                    "name" => place.name.as_str(),
                    "kind" => place.kind.as_str(),
                }
            })
            .collect()
    }

    /// 장소 `index`의 진입 지점을 점으로 끝에 더한다. `{ok, error}`.
    #[func]
    fn add_place(&mut self, index: i64) -> VarDictionary {
        let Some(model) = self.model.as_mut() else {
            return result(false, "open a region first");
        };
        let Ok(index) = usize::try_from(index) else {
            return result(false, "invalid place index");
        };
        match model.add_place(index) {
            Ok(()) => result(true, ""),
            Err(error) => result(
                false,
                &crate::korean::picker_line(&sim_core::picker::model::describe_error(&error)),
            ),
        }
    }

    /// 장소 `index`의 표시 이름(이름이 없으면 OSM 요소 이름). 없는 번호면 빈 문자열.
    #[func]
    fn place_label(&self, index: i64) -> GString {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.model.as_ref()?.places.get(i))
            .map(|place| GString::from(place.label().as_str()))
            .unwrap_or_default()
    }

    /// 점마다 표시 이름(건물·장소로 고른 점만, 나머지는 빈 문자열).
    #[func]
    fn point_labels(&self) -> PackedStringArray {
        self.model
            .iter()
            .flat_map(|model| &model.labels)
            .map(|label| GString::from(label.as_str()))
            .collect()
    }

    /// 전국 장소 검색을 시작한다(Enter·검색 버튼에서만 부른다). 결과는 `poll()`의 `search_results`. 검색 중이면 거부한다.
    #[func]
    fn search_nationwide(&mut self, text: GString) -> VarDictionary {
        let failure =
            |error: &str| vdict! { "ok" => false, "error" => error, "request_id" => 0_i64 };
        if self.searching.is_some() {
            return failure("이미 검색 중입니다");
        }
        if text.to_string().trim().is_empty() {
            return failure("검색어를 입력하세요");
        }
        match GeocodeJob::start(
            repo_path(GEOCODE_CACHE_DIR),
            text.to_string(),
            self.network,
            Box::new(UreqGet),
        ) {
            Ok(job) => {
                self.next_request += 1;
                let request_id = self.next_request;
                self.searching = Some((job, request_id));
                vdict! { "ok" => true, "error" => "", "request_id" => request_id }
            }
            Err(error) => failure(&error),
        }
    }

    /// 전국 검색 결과 `index`를 가장 가까운 도로 위 점으로 이름과 함께 더한다. 열린 지역이 없거나 도로에서 멀면 실패.
    #[func]
    fn add_search_hit(&mut self, index: i64) -> VarDictionary {
        let Some(hit) = usize::try_from(index).ok().and_then(|i| self.hits.get(i)) else {
            return result(false, "검색 결과가 없습니다");
        };
        let Some(model) = self.model.as_mut() else {
            return result(false, "먼저 지역을 여세요");
        };
        match model.add_named_point(&hit.name, hit.at) {
            Ok(()) => result(true, ""),
            Err(error) => result(
                false,
                &crate::korean::picker_line(&sim_core::picker::model::describe_error(&error)),
            ),
        }
    }

    /// 진행 이벤트(전국 장소 검색 결과)를 꺼낸다. 매 프레임 부른다.
    #[func]
    fn poll(&mut self) -> Array<VarDictionary> {
        if let Some(outcome) = self
            .searching
            .as_ref()
            .and_then(|(job, _)| job.try_result())
        {
            let (_, request_id) = self.searching.take().expect("checked above");
            let mut found = event("search_results", "", request_id);
            match outcome {
                Ok(hits) => {
                    let results: Array<VarDictionary> = hits
                        .iter()
                        .enumerate()
                        .map(|(index, hit)| {
                            let view = tiles::to_view(hit.at);
                            vdict! {
                                "index" => index as i64,
                                "name" => hit.name.as_str(),
                                "address" => hit.address.as_str(),
                                "kind" => hit.kind.as_str(),
                                "view" => Vector2::new(view.x, view.y),
                            }
                        })
                        .collect();
                    found.set("ok", true);
                    found.set("message", format!("전국 검색 결과 {}곳", hits.len()));
                    found.set("results", &results);
                    self.hits = hits;
                }
                Err(error) => {
                    found.set("ok", false);
                    found.set("message", crate::korean::search_error(&error));
                    found.set("results", &Array::<VarDictionary>::new());
                }
            }
            self.events.push(found);
        }
        self.events.drain(..).collect()
    }

    /// 그릴 도로 선분(from, to 반복), 지도 좌표.
    #[func]
    fn road_segments(&self) -> PackedVector2Array {
        self.model
            .iter()
            .flat_map(|model| &model.segments)
            .flat_map(|segment| {
                [
                    Vector2::new(segment.from.x, segment.from.y),
                    Vector2::new(segment.to.x, segment.to.y),
                ]
            })
            .collect()
    }

    /// 선분별 경로 탐색 가능 여부(1/0).
    #[func]
    fn road_usable(&self) -> PackedByteArray {
        self.model
            .iter()
            .flat_map(|model| &model.segments)
            .map(|segment| u8::from(segment.usable))
            .collect()
    }

    /// 지도 좌표 위치에 점을 끝에 더하고 경로를 다시 계산한다. 지역이 없으면 무시한다.
    #[func]
    fn add_point(&mut self, view: Vector2) {
        if let Some(model) = self.model.as_mut() {
            model.add_point(glam::Vec2::new(view.x, view.y));
        }
    }

    /// 반경 안에서 가장 가까운 점 하나를 지운다. 지웠으면 `true`.
    #[func]
    fn remove_near(&mut self, view: Vector2, radius_m: f32) -> bool {
        self.model
            .as_mut()
            .is_some_and(|model| model.remove_near(glam::Vec2::new(view.x, view.y), radius_m))
    }

    /// 마지막 점을 지운다. 지웠으면 `true`.
    #[func]
    fn remove_last(&mut self) -> bool {
        let Some(model) = self.model.as_mut() else {
            return false;
        };
        let had = !model.points.is_empty();
        model.remove_last();
        had
    }

    /// 점을 모두 지운다.
    #[func]
    fn clear_points(&mut self) {
        if let Some(model) = self.model.as_mut() {
            model.clear();
        }
    }

    /// 찍은 점(지도 좌표, 시작 → 경유 → 끝). 클릭한 위치 그대로다.
    #[func]
    fn points(&self) -> PackedVector2Array {
        self.model
            .iter()
            .flat_map(PickerModel::point_views)
            .map(|point| Vector2::new(point.x, point.y))
            .collect()
    }

    /// 현재 점에 대한 계산 결과 `{ok, error, line, length_m, warnings}`. 항상 모든 키를 준다.
    #[func]
    fn route(&self) -> VarDictionary {
        let empty = |error: &str| {
            vdict! {
                "ok" => false,
                "error" => error,
                "line" => &PackedVector2Array::new(),
                "length_m" => 0.0_f64,
                "warnings" => &PackedStringArray::new(),
            }
        };
        let Some(model) = &self.model else {
            return empty("");
        };
        match &model.planned {
            None => empty(""),
            Some(Err(error)) => empty(&crate::korean::picker_line(
                &sim_core::picker::model::describe_error(error),
            )),
            Some(Ok(planned)) => {
                let line: PackedVector2Array = model
                    .route_views()
                    .into_iter()
                    .map(|point| Vector2::new(point.x, point.y))
                    .collect();
                let warnings: PackedStringArray = region::route_warnings(planned)
                    .iter()
                    .map(|warning| GString::from(crate::korean::picker_line(warning).as_str()))
                    .collect();
                vdict! {
                    "ok" => true,
                    "error" => "",
                    "line" => &line,
                    "length_m" => planned.length_m,
                    "warnings" => &warnings,
                }
            }
        }
    }

    /// Bevy 판 상태 문구와 받기 진행·오류 안내.
    #[func]
    fn status_lines(&self) -> PackedStringArray {
        let mut lines = match &self.model {
            // Bevy 판의 "Enter로 주행 시작" 안내는 Godot 판(경로 주행 제외)에 맞지 않아 뺀다.
            Some(model) => model
                .status_lines()
                .into_iter()
                .filter(|line| !line.starts_with("Press Enter"))
                .map(|line| crate::korean::picker_line(&line))
                .collect(),
            None => vec!["지역 없음: 지도를 확대하거나 원하는 곳을 클릭하세요".to_owned()],
        };
        if !self.message.is_empty() {
            lines.push(self.message.clone());
        }
        lines
            .iter()
            .map(|line| GString::from(line.as_str()))
            .collect()
    }

    /// 경로 파일을 `cache/routes/<name>.toml`로 저장한다. 이름은 `[A-Za-z0-9_-]{1,64}`만 허용한다.
    #[func]
    fn save(&self, name: GString, #[opt(default = false)] overwrite: bool) -> VarDictionary {
        let failure = |error: &str| vdict! { "ok" => false, "error" => error, "path" => "" };
        let name = name.to_string();
        if !valid_route_name(&name) {
            return failure("route name must match [A-Za-z0-9_-]{1,64}");
        }
        let Some(model) = &self.model else {
            return failure("no valid route to save");
        };
        let dir = repo_path(ROUTE_DIR);
        let file = format!("{name}.toml");
        if !overwrite && dir.join(&file).exists() {
            return failure(&format!("{file} already exists"));
        }
        match model.save(&dir, &file) {
            Ok(path) => vdict! {
                "ok" => true,
                "error" => "",
                "path" => path.display().to_string().as_str(),
            },
            Err(error) => failure(&error),
        }
    }
}

impl SimRoutePicker {
    /// 출발·도착 사전(`{place, latlon, label}`)을 [`Endpoint`]로 바꾼다.
    fn endpoint(&self, dict: &VarDictionary) -> Result<Endpoint, String> {
        let place = dict
            .get("place")
            .and_then(|v| v.try_to::<i64>().ok())
            .unwrap_or(-1);
        if place >= 0 {
            return Ok(Endpoint::Place(place as usize));
        }
        let latlon = dict
            .get("latlon")
            .and_then(|v| v.try_to::<PackedFloat64Array>().ok())
            .unwrap_or_default();
        let [lat, lon] = latlon.as_slice() else {
            return Err("an end needs a place or [lat, lon]".to_owned());
        };
        let name = dict
            .get("label")
            .and_then(|v| v.try_to::<GString>().ok())
            .map(|text| text.to_string())
            .unwrap_or_default();
        Ok(Endpoint::Point {
            name,
            at: sim_core::world::map::LatLon {
                lat: *lat,
                lon: *lon,
            },
        })
    }

    /// 경로 범위의 도로로 새 모델을 만든다. 지금 열린 건물·장소를 그대로 옮겨(번호 유지) 장소로 고른 끝을 쓸 수 있게 한다.
    fn route_model(&mut self, area: &Bbox) -> Result<PickerModel, String> {
        let cache_root = repo_path(REGION_CACHE_DIR);
        let path = self.korea_data()?.route_snapshot(area, &cache_root)?;
        let (mut model, warning) = load_model(&path, &repo_path(ROAD_GRAPH_CONFIG_PATH))?;
        if let Some(warning) = warning {
            godot_warn!("{warning}");
        }
        if let Some(old) = &self.model
            && old.places_loaded
        {
            model.set_places(old.places.clone());
        }
        Ok(model)
    }

    /// 새 경로 모델을 받아들이거나(성공) 이전 상태를 두고 한국어 오류를 돌려준다. `{ok, error, skipped}`.
    fn finish_route(&mut self, built: Result<(PickerModel, Bbox, usize), String>) -> VarDictionary {
        match built {
            Ok((model, area, skipped)) => {
                self.model = Some(model);
                self.loaded = Some(area);
                self.message = if skipped > 0 {
                    format!("선 위 점 {skipped}개는 가까운 도로가 없어 건너뛰었습니다")
                } else {
                    String::new()
                };
                vdict! { "ok" => true, "error" => "", "skipped" => skipped as i64 }
            }
            Err(error) => {
                let error = crate::korean::route_error(&error);
                self.message = error.clone();
                vdict! { "ok" => false, "error" => error.as_str(), "skipped" => 0_i64 }
            }
        }
    }

    /// 전국 자료를 연다(처음 한 번). 폴더는 `data_dir`, 비어 있으면 [`KOREA_DATA_DIR`].
    fn korea_data(&mut self) -> Result<&KoreaData, String> {
        if self.korea.is_none() {
            let dir = if self.data_dir.is_empty() {
                repo_path(KOREA_DATA_DIR)
            } else {
                resolve_user_path(&self.data_dir)
            };
            self.korea = Some(KoreaData::open(&dir)?);
        }
        Ok(self.korea.as_ref().expect("opened above"))
    }

    /// 스냅샷 파일로 모델을 만들어 교체한다. 점·경로는 비워진다.
    fn open_model(&mut self, graph_path: &Path) -> Result<(), String> {
        let config_path: PathBuf = repo_path(ROAD_GRAPH_CONFIG_PATH);
        let (model, warning) = load_model(graph_path, &config_path)?;
        if let Some(warning) = warning {
            godot_warn!("{warning}");
        }
        self.model = Some(model);
        Ok(())
    }
}
