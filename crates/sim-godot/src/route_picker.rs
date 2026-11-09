//! 경로 선택 `SimRoutePicker`(Godot 인터페이스 계약 v1 7-2).
//!
//! 지역 받기(4-4 `FetchJob`)·스냅샷 읽기·점 편집·경로 계산·저장은 Bevy 판과 같은 `sim_core` 코드를 쓴다.
//! 경로는 모두 저장소 루트 기준이다(`cache/maps`, `cache/routes`, `config/road_graph.toml`).

use std::path::{Path, PathBuf};

use godot::prelude::*;
use sim_core::picker::model::PickerModel;
use sim_core::picker::region::{
    self, ROUTE_DIR, cached_regions, load_model, region_to_load, snapshot_graph_path,
    valid_route_name,
};
use sim_core::picker::tiles;
use sim_core::world::map::LatLon;
use sim_core::world::map_fetch::{self, Bbox, FetchJob, FetchPolicy, FetchSettings};
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

/// `{ok, error}` 사전을 만든다.
fn result(ok: bool, error: &str) -> VarDictionary {
    vdict! { "ok" => ok, "error" => error }
}

/// 진행 이벤트 사전(`{type, message, request_id}`)을 만든다.
fn event(kind: &str, message: &str, request_id: i64) -> VarDictionary {
    vdict! { "type" => kind, "message" => message, "request_id" => request_id }
}

/// 받는 중인 지역 요청.
struct Loading {
    job: FetchJob,
    bbox: Bbox,
    request_id: i64,
}

/// 경로 선택 상태. 화면(Godot)은 표시·입력만 하고 계산은 여기서 한다.
#[derive(GodotClass)]
#[class(init, base = RefCounted)]
pub struct SimRoutePicker {
    /// `false`면 캐시만 쓰고 캐시에 없으면 `failed` 이벤트를 보낸다. 헤드리스 시험은 `false`.
    #[var]
    #[init(val = true)]
    network: bool,
    model: Option<PickerModel>,
    /// 지금 열린 지역 범위(지역 받기로 연 경우).
    loaded: Option<Bbox>,
    loading: Option<Loading>,
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

    /// 디스크 캐시에 있는 지역 범위(지도 좌표).
    #[func]
    fn cached_regions(&self) -> Array<Rect2> {
        cached_regions(&repo_path(map_fetch::CACHE_DIR))
            .iter()
            .map(bbox_rect)
            .collect()
    }

    /// 지금 열린 지역 범위. 지역 받기로 열지 않았으면 스냅샷 도로 범위, 아무것도 없으면 크기 0.
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

    /// 가운데 1.5 × 1.5 km 지역 받기를 시작한다(이미 받은 범위면 캐시만). 받는 중이면 거부한다.
    #[func]
    fn request_region(&mut self, center_latlon: PackedFloat64Array) -> VarDictionary {
        let failure =
            |error: &str| vdict! { "ok" => false, "error" => error, "request_id" => 0_i64 };
        if self.loading.is_some() {
            return failure("already loading");
        }
        let [lat, lon] = center_latlon.as_slice() else {
            return failure("center_latlon must be [lat, lon]");
        };
        let center = LatLon {
            lat: *lat,
            lon: *lon,
        };
        let cache_root = repo_path(map_fetch::CACHE_DIR);
        let (bbox, mut policy) = match region_to_load(center, &cached_regions(&cache_root)) {
            Ok(found) => found,
            Err(error) => return failure(&error),
        };
        if !self.network {
            policy = FetchPolicy::CacheOnly;
        }
        match FetchJob::start(cache_root, bbox, policy, FetchSettings::default()) {
            Ok(job) => {
                self.next_request += 1;
                let request_id = self.next_request;
                self.message = match policy {
                    FetchPolicy::CacheOnly => "이 지역의 저장된 도로를 여는 중...".to_owned(),
                    FetchPolicy::Refresh => "Overpass에서 도로를 받는 중...".to_owned(),
                };
                self.events
                    .push(event("progress", &self.message, request_id));
                self.loading = Some(Loading {
                    job,
                    bbox,
                    request_id,
                });
                vdict! { "ok" => true, "error" => "", "request_id" => request_id }
            }
            Err(error) => failure(&error),
        }
    }

    /// 받는 중인 요청을 취소한다. 그 요청의 결과는 버린다(이미 보낸 네트워크 요청은 끝까지 간다).
    #[func]
    fn cancel_region(&mut self) {
        if let Some(loading) = self.loading.take() {
            self.message = "지역 받기를 취소했습니다".to_owned();
            self.events
                .push(event("failed", &self.message, loading.request_id));
        }
    }

    /// 진행 이벤트를 꺼낸다. 매 프레임 부른다. 받기가 끝났으면 지역을 교체하고 점·경로를 비운다.
    #[func]
    fn poll(&mut self) -> Array<VarDictionary> {
        if let Some(outcome) = self.loading.as_ref().and_then(|l| l.job.try_result()) {
            let loading = self.loading.take().expect("checked above");
            let opened = outcome.and_then(|obtained| {
                self.open_model(&obtained.cached.dir.join(map_fetch::SNAPSHOT_FILE))
                    .map(|()| obtained.warning)
            });
            match opened {
                Ok(warning) => {
                    self.loaded = Some(loading.bbox);
                    self.message = warning.map_or_else(
                        || "지역 도로를 열었습니다".to_owned(),
                        |w| format!("경고: {w}"),
                    );
                    self.events
                        .push(event("region_loaded", &self.message, loading.request_id));
                }
                Err(error) => {
                    self.message = format!("지역을 열 수 없습니다: {error}");
                    self.events
                        .push(event("failed", &self.message, loading.request_id));
                }
            }
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
            None => vec!["지역 없음: 지도를 확대하고 \"이 지역 도로 받기\"를 누르세요".to_owned()],
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
