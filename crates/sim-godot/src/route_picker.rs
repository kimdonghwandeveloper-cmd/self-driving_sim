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
use sim_core::picker::model::PickerModel;
use sim_core::picker::region::{
    self, ROUTE_DIR, bbox_around, load_model, snapshot_graph_path, valid_route_name,
};
use sim_core::picker::tiles;
use sim_core::world::korea_data::{KOREA_DATA_DIR, KoreaData, REGION_CACHE_DIR};
use sim_core::world::map_fetch::Bbox;
use sim_core::world::map_patch::{self, PATCH_DIR, PatchAudience, PatchBbox};
use sim_core::world::map_patch_graph;
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

/// `{ok, error}` 사전을 만든다.
fn result(ok: bool, error: &str) -> VarDictionary {
    vdict! { "ok" => ok, "error" => error }
}

/// 선택 실패의 사용자 설명·원인 코드와 지도 강조용 두 블록 형상을 전달한다.
fn selection_failure(
    s: &sim_core::picker::block_selection::BlockSelection,
    e: &sim_core::picker::block_selection::SelectionFailure,
) -> VarDictionary {
    let highlights: Array<VarDictionary> = e
        .from
        .iter()
        .chain(e.to.iter())
        .filter_map(|t| {
            s.model
                .blocks
                .iter()
                .find(|b| b.id == t.block_id)
                .map(|b| block_item(b, t.direction))
        })
        .collect();
    vdict! {"ok"=>false,"error"=>e.message.as_str(),"code"=>e.code,"selection_index"=>e.index as i64,"highlights"=>&highlights}
}

/// 진행 이벤트 사전(`{type, message, request_id}`)을 만든다.
fn event(kind: &str, message: &str, request_id: i64) -> VarDictionary {
    vdict! { "type" => kind, "message" => message, "request_id" => request_id }
}

/// 모델에 든 OSM way 전체의 위경도 범위를 지역 보정 선택 범위로 만든다.
fn model_patch_bbox(model: &PickerModel) -> Result<PatchBbox, String> {
    let mut bbox = PatchBbox {
        south: f64::INFINITY,
        west: f64::INFINITY,
        north: f64::NEG_INFINITY,
        east: f64::NEG_INFINITY,
    };
    for node in model.ways.iter().flat_map(|way| &way.nodes) {
        bbox.south = bbox.south.min(node.pos.lat);
        bbox.west = bbox.west.min(node.pos.lon);
        bbox.north = bbox.north.max(node.pos.lat);
        bbox.east = bbox.east.max(node.pos.lon);
    }
    if bbox.south.is_finite() {
        Ok(bbox)
    } else {
        Err("the snapshot has no road nodes".to_owned())
    }
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
    /// 지역 보정 폴더(저장소 루트 기준). 비어 있으면 [`PATCH_DIR`]. 시험은 별도 폴더로 바꿀 수 있다.
    patch_dir: String,
    /// 열어 둔 전국 자료(처음 지역을 열 때 연다).
    korea: Option<KoreaData>,
    model: Option<PickerModel>,
    /// 블록 UI의 확정 선택과 마지막 검증된 저장·주행 결과.
    block_selection: Option<sim_core::picker::block_selection::BlockSelection>,
    block_preview: Option<(
        sim_core::world::block_route_file::BlockRouteFile,
        sim_core::world::block_route::BlockRouteCourse,
    )>,
    block_source_hash: String,
    block_error: String,
    /// 자유 주행 영역의 검증된 저장 파일. 지역 또는 선택을 바꾸면 폐기한다.
    area_preview: Option<sim_core::world::road_area::RoadAreaFile>,
    /// 접근 제한 도로를 포함하는 연습 모드. 실제 통행 허가와 무관하다.
    include_restricted: bool,
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
    /// 경로 통행 방향과 접근 제한으로 물리 도로 후보를 제외하지 않는다.
    #[func]
    fn road_area_candidates(&self, view: Vector2, radius: f32) -> Array<VarDictionary> {
        let Some(selection) = &self.block_selection else {
            return Array::new();
        };
        let latlon = tiles::from_view(glam::Vec2::new(view.x, view.y));
        sim_core::world::road_area::road_area_candidates(
            &selection.model,
            latlon,
            radius * latlon.lat.to_radians().cos() as f32,
        )
        .iter()
        .filter_map(|id| selection.model.blocks.iter().find(|b| &b.id == id))
        .map(|b| block_item(b, sim_core::world::road_blocks::BlockDirection::Forward))
        .collect()
    }
    /// 도착 지정 없이 선택 도로 주변을 생성하고 화살표에서 고른 출발 방향을 반영한다.
    #[func]
    fn preview_road_area(
        &mut self,
        id: GString,
        span_m: f32,
        #[opt(default = false)] reversed: bool,
    ) -> VarDictionary {
        self.area_preview = None;
        let prepared = (|| {
            let m = self.model.as_ref().ok_or("지역을 먼저 여세요")?;
            let bytes = std::fs::read(&m.snapshot_path).map_err(|e| e.to_string())?;
            if sim_core::world::road_snapshot::sha256_hex(&bytes) != self.block_source_hash {
                return Err("원본 자료가 바뀌었습니다. 지역을 다시 여세요".into());
            }
            let dir = repo_path(ROUTE_DIR);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let relative = sim_core::picker::model::relative_path(&dir, &m.snapshot_path)?;
            sim_core::world::road_area::prepare_directional_road_area(
                &dir,
                &relative,
                &m.config,
                &sim_core::world::road_area::DirectionalAreaRequest {
                    anchor: &sim_core::world::road_blocks::BlockId(id.to_string()),
                    distance_m: span_m,
                    origin: m.origin,
                    reversed,
                    source_bounds: self.loaded.map(|b| [b.south, b.west, b.north, b.east]),
                },
            )
        })();
        match prepared {
            Ok((file, course)) => {
                let origin = sim_core::world::map::LatLon {
                    lat: file.origin[0],
                    lon: file.origin[1],
                };
                let model = &self
                    .block_selection
                    .as_ref()
                    .expect("loaded block model")
                    .model;
                let ids: std::collections::BTreeSet<_> =
                    file.selected_blocks.iter().cloned().collect();
                let selected: Vec<_> = model
                    .blocks
                    .iter()
                    .filter(|b| ids.contains(&b.id))
                    .collect();
                let lines: Array<PackedVector2Array> = selected
                    .iter()
                    .map(|b| {
                        b.nodes
                            .iter()
                            .map(|n| {
                                let p = tiles::to_view(n.pos);
                                Vector2::new(p.x, p.y)
                            })
                            .collect()
                    })
                    .collect();
                let count = selected.len() as i64;
                let start = tiles::to_view(sim_core::world::map::LatLon {
                    lat: origin.lat - (course.start.z_m as f64 / 6_371_008.8_f64).to_degrees(),
                    lon: origin.lon
                        + (course.start.x_m as f64
                            / (6_371_008.8_f64 * origin.lat.to_radians().cos()))
                        .to_degrees(),
                });
                self.area_preview = Some(file);
                vdict! {"ok"=>true,"error"=>"","lines"=>&lines,"road_count"=>count,"building_count"=>course.buildings.len() as i64,"start_view"=>Vector2::new(start.x,start.y),"start_heading"=>course.start.heading_rad,"reversed"=>reversed}
            }
            Err(e) => {
                vdict! {"ok"=>false,"error"=>e.as_str(),"lines"=>&Array::<PackedVector2Array>::new(),"road_count"=>0_i64}
            }
        }
    }

    /// 검증된 도로 영역을 새 파일로 저장해 공통 자유 주행 로더로 전달한다.
    #[func]
    fn save_road_area(&self, name: GString) -> VarDictionary {
        let name = name.to_string();
        if !valid_route_name(&name) {
            return result(false, "잘못된 저장 이름입니다");
        }
        let Some(file) = &self.area_preview else {
            return result(false, "먼저 도로를 선택하세요");
        };
        let path = repo_path(ROUTE_DIR).join(format!("{name}.toml"));
        match sim_core::world::road_area::save_road_area(&path, file) {
            Ok(()) => vdict! {"ok"=>true,"error"=>"","path"=>path.to_string_lossy().as_ref()},
            Err(e) => result(false, &e),
        }
    }

    /// 영역 선택을 초기화한다. 이전 미리보기로 주행을 시작하지 못하게 한다.
    #[func]
    fn clear_road_area(&mut self) {
        self.area_preview = None;
    }
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

    /// 고정 OSM 스냅샷을 블록 UI용으로 연다. 지역 보정은 기존 점 경로에서만 지원한다.
    #[func]
    fn load_block_snapshot(&mut self, path: GString) -> VarDictionary {
        let path = snapshot_graph_path(&resolve_user_path(&path.to_string()));
        match self.initialize_blocks(&path) {
            Ok(()) => {
                self.loaded = None;
                result(true, "")
            }
            Err(error) => result(false, &error),
        }
    }
    /// 출발/도착 선택 전에 1.5~10 km 사각형 지역을 명시적으로 연다. 실패하면 기존 지역을 유지한다.
    #[func]
    fn open_block_region(&mut self, center: PackedFloat64Array, span_m: f64) -> VarDictionary {
        let [lat, lon] = center.as_slice() else {
            return result(false, "위도·경도 두 값이 필요합니다");
        };
        if !(span_m.is_finite()
            && (1500.0..=10000.0).contains(&span_m)
            && lat.is_finite()
            && lat.abs() < 85.0
            && lon.is_finite()
            && lon.abs() <= 180.0)
        {
            return result(false, "지역 범위는 1.5~10 km입니다");
        }
        let half = span_m / 2.0;
        let dy = (half / 6_371_008.8_f64).to_degrees();
        let dx = dy / lat.to_radians().cos();
        let bbox = Bbox {
            south: lat - dy,
            west: lon - dx,
            north: lat + dy,
            east: lon + dx,
        };
        let opened = (|| {
            bbox.validate_local_route()?;
            let (path, list) = self
                .korea_data()?
                .region_snapshot_local_route(&bbox, &repo_path(REGION_CACHE_DIR))?;
            self.initialize_blocks(&path)?;
            if let Some(model) = self.model.as_mut() {
                model.set_places(list);
            }
            Ok::<(), String>(())
        })();
        match opened {
            Ok(()) => {
                self.loaded = Some(bbox);
                result(true, "")
            }
            Err(error) => result(false, &error),
        }
    }
    /// 전체 블록의 중심선·허용 방향·원본·폭을 반환한다. 경로 선택과 도로 목록은 독립적이다.
    #[func]
    fn block_items(&self) -> Array<VarDictionary> {
        self.block_selection
            .iter()
            .flat_map(|s| &s.model.blocks)
            .map(|b| {
                let mut item = block_item(
                    b,
                    b.directions
                        .first()
                        .copied()
                        .unwrap_or(sim_core::world::road_blocks::BlockDirection::Forward),
                );
                let directions: PackedStringArray = b
                    .directions
                    .iter()
                    .map(|&d| GString::from(direction_name(d)))
                    .collect();
                item.set("directions", &directions);
                item
            })
            .collect()
    }

    /// 원본을 보존하면서 보수적으로 차단한 불완전 회전 제한을 UI에 명시한다.
    #[func]
    fn block_diagnostics(&self) -> PackedStringArray {
        self.block_selection
            .iter()
            .flat_map(|s| &s.model.diagnostics)
            .map(|s| GString::from(s.as_str()))
            .collect()
    }
    /// 클릭 주변 모든 방향 후보를 반환할 뿐, 가장 가까운 도로를 자동 확정하지 않는다.
    #[func]
    fn block_candidates(&self, view: Vector2, radius: f32) -> Array<VarDictionary> {
        let Some(selection) = &self.block_selection else {
            return Array::new();
        };
        selection
            .candidates(glam::Vec2::new(view.x, view.y), radius)
            .into_iter()
            .map(|c| {
                let b = selection
                    .model
                    .blocks
                    .iter()
                    .find(|b| b.id == c.traversal.block_id)
                    .expect("candidate belongs to model");
                let mut item = block_item(b, c.traversal.direction);
                item.set("distance", c.distance);
                item
            })
            .collect()
    }
    /// 사용자가 확인한 블록과 방향을 역할 슬롯에 넣고 이전 미리보기를 무효화한다.
    #[func]
    fn choose_block(&mut self, id: GString, direction: GString, role: GString) -> VarDictionary {
        use sim_core::world::road_blocks::{BlockDirection, BlockId, BlockTraversal};
        let dir = match direction.to_string().as_str() {
            "forward" => BlockDirection::Forward,
            "backward" => BlockDirection::Backward,
            _ => return result(false, "올바른 진행 방향을 고르세요"),
        };
        let Some(s) = self.block_selection.as_mut() else {
            return result(false, "지역을 먼저 여세요");
        };
        match s.select_checked(
            &role.to_string(),
            BlockTraversal {
                block_id: BlockId(id.to_string()),
                direction: dir,
            },
            sim_core::world::route_plan::ROUTE_LENGTH_CAP_M,
        ) {
            Ok(()) => {
                self.block_preview = None;
                self.block_error = "선택이 바뀌었습니다. 미리보기를 눌러 검증하세요".into();
                result(true, "")
            }
            Err(e) => selection_failure(s, &e),
        }
    }
    /// 선택 목록을 역할/경유 번호와 함께 반환한다.
    #[func]
    fn block_selections(&self) -> Array<VarDictionary> {
        let Some(s) = &self.block_selection else {
            return Array::new();
        };
        s.start
            .iter()
            .map(|t| ("start", 0, t))
            .chain(s.vias.iter().enumerate().map(|(i, t)| ("via", i, t)))
            .chain(s.goal.iter().map(|t| ("goal", 0, t)))
            .map(|(role, index, t)| {
                let b = s
                    .model
                    .blocks
                    .iter()
                    .find(|b| b.id == t.block_id)
                    .expect("validated selection");
                let mut item = block_item(b, t.direction);
                item.set("role", role);
                item.set("index", index as i64);
                item
            })
            .collect()
    }
    /// 경유 항목 이동/삭제를 처리하고 미리보기를 비운다. offset=0은 삭제다.
    #[func]
    fn edit_block_via(&mut self, index: i64, offset: i64) -> VarDictionary {
        let Some(s) = self.block_selection.as_mut() else {
            return result(false, "지역을 먼저 여세요");
        };
        let Ok(index) = usize::try_from(index) else {
            return result(false, "잘못된 경유 번호입니다");
        };
        if !matches!(offset, -1..=1) {
            return result(false, "경유 이동은 한 칸씩 가능합니다");
        }
        let edited = s.edit_checked(
            index,
            offset as isize,
            sim_core::world::route_plan::ROUTE_LENGTH_CAP_M,
        );
        match edited {
            Ok(()) => {
                self.block_preview = None;
                self.block_error = "경유가 바뀌었습니다. 다시 미리보기를 실행하세요".into();
                result(true, "")
            }
            Err(e) => selection_failure(s, &e),
        }
    }
    /// 선택 전체를 지우며 마지막 주행 결과도 함께 무효화한다.
    #[func]
    fn clear_blocks(&mut self) {
        if let Some(s) = self.block_selection.as_mut() {
            s.clear();
        }
        self.block_preview = None;
        self.block_error.clear();
    }
    /// 확정 선택만 기존 블록 경로 엔진으로 계획·기하·차체·저장 재생 검증한다. 실패 시 자동 우회하지 않는다.
    #[func]
    fn preview_blocks(&mut self) -> VarDictionary {
        use sim_core::world::block_route_file::{BlockRouteRequest, prepare_block_route_file};
        self.block_preview = None;
        if let Some(s) = &self.block_selection
            && let Err(e) = s.check(sim_core::world::route_plan::ROUTE_LENGTH_CAP_M)
        {
            self.block_error = e.message.clone();
            return selection_failure(s, &e);
        }
        let prepared = (|| {
            let s = self.block_selection.as_ref().ok_or("지역을 먼저 여세요")?;
            let model = self.model.as_ref().ok_or("지역을 먼저 여세요")?;
            let bytes = std::fs::read(&model.snapshot_path).map_err(|e| e.to_string())?;
            if sim_core::world::road_snapshot::sha256_hex(&bytes) != self.block_source_hash {
                return Err("열었던 원본 자료가 바뀌었습니다. 명시적으로 다시 여세요".into());
            }
            let dir = repo_path(ROUTE_DIR);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let relative = sim_core::picker::model::relative_path(&dir, &model.snapshot_path)?;
            prepare_block_route_file(
                &dir,
                &BlockRouteRequest {
                    name: "선택한 블록 경로",
                    snapshot: &relative,
                    origin: model.origin,
                    graph: &model.config,
                    network: sim_core::world::road_network::NetworkSettings::default(),
                    selected: &s.ordered()?,
                },
            )
        })();
        match prepared {
            Ok(value) => {
                self.block_preview = Some(value);
                self.block_error.clear();
                result(true, "")
            }
            Err(e) => {
                self.block_error = e.clone();
                result(false, &e)
            }
        }
    }
    /// 계산한 안내선과 오류를 반환한다. 편집 뒤 옛 미리보기가 주행 가능한 상태로 남지 않는다.
    #[func]
    fn block_route(&self) -> VarDictionary {
        let Some((file, route)) = &self.block_preview else {
            return vdict! {"ok"=>false,"error"=>self.block_error.as_str(),"line"=>&PackedVector2Array::new(),"length_m"=>0.0_f64};
        };
        let origin = sim_core::world::map::LatLon {
            lat: file.origin[0],
            lon: file.origin[1],
        };
        let line: PackedVector2Array = route
            .guide
            .iter()
            .map(|p| {
                let lat = origin.lat - (p.y as f64 / 6_371_008.8_f64).to_degrees();
                let lon = origin.lon
                    + (p.x as f64 / (6_371_008.8_f64 * origin.lat.to_radians().cos())).to_degrees();
                let v = tiles::to_view(sim_core::world::map::LatLon { lat, lon });
                Vector2::new(v.x, v.y)
            })
            .collect();
        vdict! {"ok"=>true,"error"=>"","line"=>&line,"length_m"=>route.plan.length_m}
    }
    /// 검증된 미리보기 파일만 새 이름으로 저장한다. 기존 경로와 변경된 원본을 덮어쓰지 않는다.
    #[func]
    fn save_blocks(&self, name: GString) -> VarDictionary {
        let failure = |e: &str| vdict! {"ok"=>false,"error"=>e,"path"=>""};
        let name = name.to_string();
        if !valid_route_name(&name) {
            return failure("저장 이름은 영문·숫자·_- 1~64자입니다");
        }
        let Some((file, _)) = &self.block_preview else {
            return failure("먼저 미리보기를 실행하세요");
        };
        let path = repo_path(ROUTE_DIR).join(format!("{name}.toml"));
        match sim_core::world::block_route_file::save_block_route_file(&path, file) {
            Ok(()) => vdict! {"ok"=>true,"error"=>"","path"=>path.to_string_lossy().as_ref()},
            Err(e) => failure(&e),
        }
    }

    /// 전국 자료 폴더를 바꾼다(저장소 루트 기준, 빈 문자열이면 기본 `data/korea`). 열어 둔 자료는 닫고 다음에 다시 연다.
    #[func]
    fn use_data_dir(&mut self, dir: GString) {
        self.data_dir = dir.to_string();
        self.korea = None;
    }

    /// 지역 보정 폴더를 바꾼다(저장소 루트 기준, 빈 문자열이면 `data/map-patches`). 다음 지역부터 적용한다.
    #[func]
    fn use_patch_dir(&mut self, dir: GString) {
        self.patch_dir = dir.to_string();
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

    /// 현재 지역의 판본 고정 스냅샷 경로를 진단·출처 확인용으로 반환한다.
    #[func]
    fn source_snapshot(&self) -> GString {
        let path = self
            .model
            .as_ref()
            .map(|model| model.snapshot_path.to_string_lossy().into_owned())
            .unwrap_or_default();
        GString::from(path.as_str())
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

    /// 선택한 반대편 지점을 보존하며 새 지점까지 필요한 오프라인 지역을 확장한다.
    #[func]
    fn expand_route_region(
        &mut self,
        target: PackedFloat64Array,
        destination: bool,
    ) -> VarDictionary {
        let [lat, lon] = target.as_slice() else {
            return result(false, "target must be [lat, lon]");
        };
        let Some(previous) = self.model.clone() else {
            return self.open_region(target);
        };
        let other: Vec<[f64; 2]> = if destination {
            previous.points.first().copied().into_iter().collect()
        } else {
            previous.points.get(1).copied().into_iter().collect()
        };
        let opened = region::route_bbox(&other, [*lat, *lon]).and_then(|bbox| {
            let cache_root = repo_path(REGION_CACHE_DIR);
            let (path, list) = self
                .korea_data()?
                .region_snapshot_local_route(&bbox, &cache_root)?;
            self.open_model(&path)?;
            if let Some(model) = self.model.as_mut() {
                model.set_places(list);
                model.restore_selection(&previous.points, &previous.labels);
            }
            Ok(bbox)
        });
        match opened {
            Ok(bbox) => {
                self.loaded = Some(bbox);
                result(true, "")
            }
            Err(error) => {
                self.model = Some(previous);
                result(false, &error)
            }
        }
    }

    /// 제한 도로 포함 정책을 바꾸고 선택된 경로도 같은 정책으로 다시 계산한다.
    #[func]
    fn set_restricted_roads(&mut self, enabled: bool) {
        self.include_restricted = enabled;
        if let Some(model) = self.model.as_mut() {
            model.set_include_restricted(enabled);
        }
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

    /// 새로 선택한 점을 출발 또는 도착으로 지정하고 반대쪽 지점은 유지한다.
    #[func]
    fn assign_last_endpoint(&mut self, destination: bool) {
        if let Some(model) = self.model.as_mut() {
            model.assign_last_endpoint(destination);
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

    /// 현재 핀으로 주행 가능한 대안을 계산한다.
    #[func]
    fn prepare_route_alternatives(&mut self) {
        if let Some(model) = &mut self.model {
            model.prepare_alternatives();
        }
    }

    /// 대안의 지도 선과 길이를 전달한다.
    #[func]
    fn route_alternatives(&self) -> Array<VarDictionary> {
        self.model
            .iter()
            .flat_map(PickerModel::alternative_views)
            .map(|(line, length_m)| {
                let line: PackedVector2Array =
                    line.into_iter().map(|p| Vector2::new(p.x, p.y)).collect();
                vdict! { "line" => &line, "length_m" => length_m }
            })
            .collect()
    }

    /// 선택한 대안을 현재 저장·주행 경로로 적용한다.
    #[func]
    fn choose_route_alternative(&mut self, index: i64) -> bool {
        usize::try_from(index)
            .ok()
            .zip(self.model.as_mut())
            .is_some_and(|(i, model)| model.choose_alternative(i))
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
    /// 점 선택/지역 보정을 끼워 넣지 않고 원본 스냅샷과 블록 모델을 함께 교체한다.
    fn initialize_blocks(&mut self, path: &Path) -> Result<(), String> {
        self.area_preview = None;
        let (model, _) = load_model(path, &repo_path(ROAD_GRAPH_CONFIG_PATH))?;
        let blocks = sim_core::world::road_blocks::build_road_blocks(
            &model.ways,
            &model.restrictions,
            &model.config,
        )?;
        let hash = sim_core::world::road_snapshot::sha256_hex(
            &std::fs::read(path).map_err(|e| e.to_string())?,
        );
        self.model = Some(model);
        self.block_selection = Some(sim_core::picker::block_selection::BlockSelection::new(
            blocks,
        ));
        self.block_preview = None;
        self.block_error.clear();
        self.block_source_hash = hash;
        Ok(())
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
        let (mut model, warning) = load_model(graph_path, &config_path)?;
        if let Some(warning) = warning {
            godot_warn!("{warning}");
        }
        model.set_include_restricted(self.include_restricted);
        let patch_dir = if self.patch_dir.is_empty() {
            repo_path(PATCH_DIR)
        } else {
            resolve_user_path(&self.patch_dir)
        };
        if patch_dir.is_dir() {
            let available = map_patch::load_patch_dir(&patch_dir, PatchAudience::Distribution)?;
            let bbox = model_patch_bbox(&model)?;
            let selected: Vec<_> = map_patch::patches_for_bbox(&available, &bbox)
                .into_iter()
                .cloned()
                .collect();
            if !selected.is_empty() {
                let graph = map_patch_graph::apply_map_patches(&model.ways, &selected)?;
                self.message = format!("지역 보정 {}개를 적용했습니다", graph.applied.len());
                model = model.with_map_patches(graph);
            } else {
                self.message.clear();
            }
        } else {
            self.message.clear();
        }
        self.model = Some(model);
        self.block_selection = None;
        self.block_preview = None;
        self.block_error.clear();
        Ok(())
    }
}

/// 정규화한 블록 방향의 외부 이름을 반환한다.
fn direction_name(direction: sim_core::world::road_blocks::BlockDirection) -> &'static str {
    match direction {
        sim_core::world::road_blocks::BlockDirection::Forward => "forward",
        sim_core::world::road_blocks::BlockDirection::Backward => "backward",
    }
}
/// 한 블록을 방향대로 그릴 수 있는 원본 설명 사전으로 만든다.
fn block_item(
    b: &sim_core::world::road_blocks::RoadBlock,
    direction: sim_core::world::road_blocks::BlockDirection,
) -> VarDictionary {
    let mut nodes: Vec<_> = b.nodes.iter().collect();
    if direction == sim_core::world::road_blocks::BlockDirection::Backward {
        nodes.reverse();
    }
    let line: PackedVector2Array = nodes
        .iter()
        .map(|n| {
            let v = tiles::to_view(n.pos);
            Vector2::new(v.x, v.y)
        })
        .collect();
    vdict! {"id"=>b.id.0.as_str(),"direction"=>direction_name(direction),"name"=>b.attributes.tags.get("name").map(String::as_str).unwrap_or(b.attributes.highway.as_str()),"line"=>&line,"length_m"=>b.length_m,"width_m"=>b.attributes.width_m,"from_node"=>nodes[0].id as i64,"to_node"=>nodes[nodes.len()-1].id as i64,"access"=>b.attributes.tags.get("access").map(String::as_str).unwrap_or(""),"way_id"=>b.source_edges[0].way_id as i64}
}
