//! 경로 선택 화면(Phase 4-5, 4-5b 전국 배경 지도).
//!
//! 앱 상태 [`AppMode::RoutePicking`]에서 OSM 표준 래스터 타일([`tiles`])을 배경으로 깔고, 받은 도로 그래프를
//! 그 위에 선으로 그린다. 타일은 **보기 전용**이며, 코스 형상·재현은 계속 노드 ID 스냅샷(벡터)으로만 한다.
//! 타일을 받지 못하면 그 칸은 빈 회색이고, 도로 받기·경로 계산·저장·주행은 그대로 된다.
//!
//! 흐름: 전국 지도에서 지역을 찾고 확대 → "이 지역 도로 받기"(`L`)로 화면 가운데 1.5 × 1.5 km를 4-4
//! `FetchJob`으로 받기(이미 받은 범위 안이면 캐시만 사용) → 도로 위에 점 찍기 → "주행 시작"(`Enter`).
//!
//! 조작(개발용):
//! - 왼쪽 클릭: 점 추가(시작 → 경유 → 끝). 왼쪽 끌기: 화면 이동. 휠: 확대·축소.
//! - 오른쪽 클릭: 가까운 점 지우기. `Backspace`: 마지막 점 지우기. `C`: 모두 지우기. `Enter`: 주행 시작.
//! - `L`: 이 지역 도로 받기. 맞춤 보기: `K` 전국, `G` 받은 지역, `F` 경로. `Esc` 두 번: 메인 화면.
//!
//! 자동 맞춤은 지역을 받았을 때 그 지역에, 경로를 처음 계산했을 때 경로에 **한 번씩만** 한다. 그 뒤에는 사용자가
//! 화면을 움직여도 다시 맞추지 않는다.

// 점·경로 모델과 타일 받기·캐시(GD-1c), 지역 범위·캐시 목록·모델 열기(GD-2d)는 엔진과 무관해
// `sim-core`로 옮겼다.
pub use sim_core::picker::region::{
    FETCH_HALF_SIDE_M, ROUTE_DIR, bbox_around, cached_regions, region_to_load,
};
pub use sim_core::picker::{model, region, tiles};

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::AppMode;
use crate::vehicle::VehicleMotion;
use crate::world::ActiveCourse;
use crate::world::map::OSM_ATTRIBUTION;
use crate::world::map_fetch::{self, Bbox, FetchJob, FetchPolicy, FetchSettings};
use crate::world::route_plan;
use model::PickerModel;
use tiles::{TileFetcher, TileId, TileOutcome, TileSettings, UreqTileTransport};

/// 미리 정한 지역 이름과 그 지역의 저장된 노드 ID 스냅샷.
pub const PRESET_REGIONS: [(&str, &str); 1] =
    [("yeosu", "assets/maps/yeosu-expo/graph-2026-11-06.osm.json")];
/// 카메라 높이. 직교 투영이라 값 자체는 보이는 크기에 영향이 없다.
const CAMERA_HEIGHT_M: f32 = 500.0;
/// 확대 수준 허용 범위(화면 1 px당 메르카토르 m). 전국(약 3 km/px)부터 거리(0.15 m/px)까지.
const SCALE_RANGE: (f32, f32) = (0.15, 3000.0);
/// 도로 받기를 허용하는 최대 축척(m/px). 이보다 멀리서 보면 확대하라고 안내한다.
const LOAD_MAX_SCALE: f32 = 20.0;
/// 이 픽셀보다 많이 움직이면 클릭이 아니라 끌기로 본다.
const DRAG_THRESHOLD_PX: f32 = 5.0;
/// 오른쪽 클릭으로 점을 지울 수 있는 화면 거리(px).
const REMOVE_RADIUS_PX: f32 = 14.0;
/// 점 표시 반경(px).
const POINT_RADIUS_PX: f32 = 6.0;
/// 한 화면에 그릴 타일 수 상한.
const MAX_VISIBLE_TILES: usize = 48;
/// 메모리에 둘 타일 이미지 수 상한(임시값). 넘으면 오래 안 보인 것부터 버린다.
const MEMORY_BUDGET_TILES: usize = 256;
/// 타일을 그리는 높이(도로 선 아래).
const TILE_Y: f32 = -2.0;

/// 경로 선택 화면이 다룰 지역.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerSource {
    /// 저장된 노드 ID 스냅샷 파일.
    Snapshot(PathBuf),
    /// 위경도 주변 범위를 받아 캐시에 저장한 뒤 쓴다(사용자가 명령줄로 요청한 경우만).
    Fetch(Bbox),
}

impl PickerSource {
    /// `yeosu`(미리 정한 지역), `latlon:<위도>,<경도>`(주변 1.5 km 받기), 그 밖은 스냅샷 파일 경로로 읽는다.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if let Some((_, path)) = PRESET_REGIONS.iter().find(|(name, _)| *name == text) {
            return Ok(Self::Snapshot(PathBuf::from(path)));
        }
        if let Some(rest) = text.strip_prefix("latlon:") {
            let values: Vec<f64> = rest
                .split(',')
                .map(|part| part.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`{text}` must be latlon:<lat>,<lon>"))?;
            let [lat, lon] = values[..] else {
                return Err(format!("`{text}` must be latlon:<lat>,<lon>"));
            };
            return Ok(Self::Fetch(bbox_around(lat, lon)?));
        }
        if text.is_empty() {
            return Err(
                "--pick-route needs yeosu, latlon:<lat>,<lon> or a snapshot path".to_owned(),
            );
        }
        Ok(Self::Snapshot(PathBuf::from(text)))
    }
}

/// 스냅샷 파일을 읽어 경로 선택 상태를 만든다. 도로 그래프 설정을 읽지 못하면 경고 후 기본값을 쓴다.
pub fn load_model(path: &Path) -> Result<PickerModel, String> {
    let (model, warning) = region::load_model(path, Path::new(route_plan::ROAD_GRAPH_CONFIG_PATH))?;
    if let Some(warning) = warning {
        warn!("{warning}");
    }
    Ok(model)
}

/// 경로 선택 화면 플러그인. 메인 화면의 "맵 생성하기"나 `--pick-route`로 들어온다.
pub struct PickerPlugin {
    /// 처음 다룰 지역(`--pick-route`). 없으면 전국 지도에서 시작한다.
    pub source: Option<PickerSource>,
    /// 지도를 불러온 뒤 미리 찍어 둘 점 [위도, 경도].
    pub preset_points: Vec<[f64; 2]>,
    /// 지도를 불러오면 곧바로 주행 시작을 요청한다(화면 전환 자동 검증용).
    pub auto_start: bool,
    /// 배경 타일 설정(자동 검증이면 네트워크 끔).
    pub tiles: TileSettings,
}

/// `위도,경도;위도,경도;...` 형식의 점 목록을 읽는다.
pub fn parse_points(text: &str) -> Result<Vec<[f64; 2]>, String> {
    text.split(';')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let values: Vec<f64> = part
                .split(',')
                .map(|value| value.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`{part}` must be <lat>,<lon>"))?;
            match values[..] {
                [lat, lon] if lat.is_finite() && lon.is_finite() => Ok([lat, lon]),
                _ => Err(format!("`{part}` must be <lat>,<lon>")),
            }
        })
        .collect()
}

impl Plugin for PickerPlugin {
    /// 화면 구성(상태 진입 시), 매 프레임 입력·타일·그리기·안내 갱신, 나갈 때 정리를 등록한다.
    fn build(&self, app: &mut App) {
        let picking = in_state(AppMode::RoutePicking);
        app.insert_resource(Picker {
            source: self.source.clone(),
            preset_points: self.preset_points.clone(),
            auto_start: self.auto_start,
            ..default()
        })
        .insert_resource(TileLayer {
            settings: self.tiles.clone(),
            ..default()
        })
        .add_systems(
            OnEnter(AppMode::RoutePicking),
            (enter_picker, spawn_picker, start_loading).chain(),
        )
        .add_systems(OnExit(AppMode::RoutePicking), leave_picker)
        .add_systems(
            Update,
            (
                poll_loading,
                handle_pointer,
                handle_keys,
                apply_fit,
                start_driving,
                update_tiles,
                draw_picker,
                update_picker_text,
            )
                .chain()
                .run_if(picking),
        );
    }
}

/// 맞춤 보기 대상.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// 대한민국 전체.
    Korea,
    /// 받은 지역(도로 범위).
    Region,
    /// 계산된 경로.
    Route,
}

/// 경로 선택 화면 상태.
#[derive(Resource, Default)]
pub struct Picker {
    /// 처음 다룰 지역. 화면에 들어올 때 한 번 쓴다.
    source: Option<PickerSource>,
    /// 불러온 상태. 받는 중이거나 아직 고르지 않았으면 `None`.
    model: Option<PickerModel>,
    /// 진행 중인 지도 받기와 그 범위.
    loading: Option<(FetchJob, Bbox)>,
    /// 마지막 알림(불러오기 실패, 저장 결과 등).
    message: Option<String>,
    /// 왼쪽 버튼을 누른 곳(px)과 끌기 중인지.
    press: Option<(Vec2, bool)>,
    /// 주행 시작 요청.
    start_requested: bool,
    /// 도로 받기 요청.
    load_requested: bool,
    /// 지도를 불러온 뒤 미리 찍어 둘 점.
    preset_points: Vec<[f64; 2]>,
    /// 지도를 불러오면 곧바로 주행 시작을 요청할지.
    auto_start: bool,
    /// 다음 프레임에 맞출 보기.
    fit: Option<Fit>,
    /// 이번 지역에서 경로에 한 번 자동으로 맞췄는지.
    route_fitted: bool,
    /// 이미 받은 범위.
    cached: Vec<Bbox>,
}

impl Picker {
    /// 불러온 상태를 받아 들이고, 미리 찍을 점과 자동 시작 요청을 적용한다. 그 지역에 한 번 맞춘다.
    fn accept(&mut self, mut model: PickerModel) {
        if !self.preset_points.is_empty() {
            model.set_points(&std::mem::take(&mut self.preset_points));
        }
        self.model = Some(model);
        self.fit = Some(Fit::Region);
        self.route_fitted = false;
        if self.auto_start {
            self.start_requested = true;
        }
    }

    /// 메인 화면의 "맵 생성하기"로 들어갈 때: 처음 지역 없이 전국 지도에서 시작한다.
    pub fn prepare_browse(&mut self) {
        self.source = None;
        self.preset_points.clear();
        self.auto_start = false;
    }
}

impl Default for PickerSource {
    /// 기본 지역은 여수 스냅샷이다.
    fn default() -> Self {
        Self::Snapshot(PathBuf::from(PRESET_REGIONS[0].1))
    }
}

/// 배경 타일 상태.
#[derive(Resource, Default)]
pub struct TileLayer {
    /// 받기 설정.
    settings: TileSettings,
    /// 백그라운드 받기(처음 들어올 때 시작).
    fetcher: Option<TileFetcher>,
    /// 받은 타일 재질, 마지막으로 보인 프레임, 만료 시각(유닉스 초).
    materials: HashMap<TileId, (Handle<StandardMaterial>, u64, u64)>,
    /// 지금 그리는 타일 엔티티.
    entities: HashMap<TileId, Entity>,
    /// 타일 사각형 메시(가로세로 1, 위에서 본 이미지 방향).
    mesh: Option<Handle<Mesh>>,
    /// 프레임 번호.
    frame: u64,
}

/// 경로 선택 화면의 위에서 본 카메라.
#[derive(Component)]
pub struct PickerCamera;

/// 경로 선택 안내 문구.
#[derive(Component)]
struct PickerText;

/// 화면 오른쪽 위 버튼이 하는 일.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PickerButton {
    /// 주행 시작.
    Start,
    /// 이 지역 도로 받기.
    Load,
    /// 맞춤 보기.
    Fit(Fit),
}

/// 화면에 들어올 때: 눌린 키를 비우고, 이전 방문의 상태(점·경로·알림·받기)를 지운다. 처음 지역 인자는 남긴다.
fn enter_picker(world: &mut World) {
    crate::clear_pressed_buttons(world);
    let mut picker = world.resource_mut::<Picker>();
    picker.model = None;
    picker.loading = None;
    picker.message = None;
    picker.press = None;
    picker.start_requested = false;
    picker.load_requested = false;
    picker.route_fitted = false;
    picker.fit = Some(Fit::Korea);
    picker.cached = cached_regions(Path::new(map_fetch::CACHE_DIR));
}

/// 화면을 나갈 때: 타일 엔티티 기록을 지우고(엔티티는 상태 수명으로 지워진다), 아직 시작하지 않은 타일 요청을
/// 버린다(이미 전송 중인 것은 끝나도록 둔다). 받던 지도는 결과를 버린다.
fn leave_picker(mut layer: ResMut<TileLayer>, mut picker: ResMut<Picker>) {
    layer.entities.clear();
    if let Some(fetcher) = layer.fetcher.as_mut() {
        fetcher.cancel_pending();
    }
    picker.loading = None;
}

/// 위에서 본 직교 카메라, 안내 문구, 출처 표기, 버튼 줄을 만든다. 모두 상태를 나가면 지운다.
fn spawn_picker(mut commands: Commands) {
    let scoped = DespawnOnExit(AppMode::RoutePicking);
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0,
            far: CAMERA_HEIGHT_M * 2.0,
            ..OrthographicProjection::default_3d()
        }),
        // 아래를 보고 화면 위쪽이 북쪽(-Z)이 되게 한다. 화면 오른쪽은 동쪽(+X)이다.
        Transform::from_xyz(0.0, CAMERA_HEIGHT_M, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
        PickerCamera,
        scoped.clone(),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            max_width: Val::Percent(60.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
        Text::new("Loading..."),
        TextFont::from_font_size(15.0),
        TextColor(Color::WHITE),
        PickerText,
        scoped.clone(),
    ));
    // 정책상 출처 표기는 지도 오른쪽 아래에 늘 보이고 다른 UI가 가리지 않게 한다.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(0.0),
            right: Val::Px(0.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Text::new(format!(
            "{OSM_ATTRIBUTION} | Map tiles: OpenStreetMap | Virtual training course (not a surveyed road)"
        )),
        TextFont::from_font_size(13.0),
        TextColor(Color::BLACK),
        GlobalZIndex(100),
        scoped.clone(),
    ));
    let buttons = [
        (
            "Start driving (Enter)",
            PickerButton::Start,
            Color::srgb(0.15, 0.45, 0.2),
        ),
        (
            "Load roads here (L)",
            PickerButton::Load,
            Color::srgb(0.15, 0.35, 0.55),
        ),
        (
            "Korea (K)",
            PickerButton::Fit(Fit::Korea),
            Color::srgb(0.3, 0.3, 0.35),
        ),
        (
            "Region (G)",
            PickerButton::Fit(Fit::Region),
            Color::srgb(0.3, 0.3, 0.35),
        ),
        (
            "Route (F)",
            PickerButton::Fit(Fit::Route),
            Color::srgb(0.3, 0.3, 0.35),
        ),
    ];
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                right: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            scoped,
        ))
        .with_children(|parent| {
            for (label, action, color) in buttons {
                parent
                    .spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                            ..default()
                        },
                        Button,
                        BackgroundColor(color),
                        action,
                    ))
                    .with_child((
                        Text::new(label),
                        TextFont::from_font_size(15.0),
                        TextColor(Color::WHITE),
                    ));
            }
        });
}

/// 처음 지역 인자가 있으면 불러오거나 받기를 시작한다. 없으면 전국 지도에서 기다린다.
fn start_loading(mut picker: ResMut<Picker>) {
    match picker.source.take() {
        Some(PickerSource::Snapshot(path)) => match load_model(&path) {
            Ok(model) => picker.accept(model),
            Err(error) => picker.message = Some(format!("Cannot load the map: {error}")),
        },
        Some(PickerSource::Fetch(bbox)) => begin_fetch(&mut picker, bbox, FetchPolicy::Refresh),
        None => {
            picker.message = Some(
                "Find an area on the map, zoom in and press \"Load roads here\" (L)".to_owned(),
            )
        }
    }
}

/// 백그라운드 받기를 시작한다. 이미 받는 중이면 알림만 남긴다(동시 작업 1개).
fn begin_fetch(picker: &mut Picker, bbox: Bbox, policy: FetchPolicy) {
    if picker.loading.is_some() {
        picker.message = Some("Already loading roads; please wait".to_owned());
        return;
    }
    match FetchJob::start(
        PathBuf::from(map_fetch::CACHE_DIR),
        bbox,
        policy,
        FetchSettings::default(),
    ) {
        Ok(job) => {
            picker.loading = Some((job, bbox));
            picker.message = Some(match policy {
                FetchPolicy::CacheOnly => "Opening roads saved for this area...".to_owned(),
                _ => "Downloading roads from Overpass...".to_owned(),
            });
        }
        Err(error) => picker.message = Some(format!("Cannot download the map: {error}")),
    }
}

/// 받기 작업이 끝났으면 받은(또는 캐시의) 스냅샷을 불러온다. 기다리지 않는다.
fn poll_loading(mut picker: ResMut<Picker>) {
    let Some(result) = picker
        .loading
        .as_ref()
        .and_then(|(job, _)| job.try_result())
    else {
        return;
    };
    picker.loading = None;
    match result.and_then(|obtained| {
        let warning = obtained.warning.clone();
        load_model(&obtained.cached.dir.join(map_fetch::SNAPSHOT_FILE))
            .map(|model| (model, warning))
    }) {
        Ok((model, warning)) => {
            picker.accept(model);
            picker.cached = cached_regions(Path::new(map_fetch::CACHE_DIR));
            picker.message = warning.map(|warning| format!("Warning: {warning}"));
        }
        Err(error) => picker.message = Some(format!("Cannot load the map: {error}")),
    }
}

/// 화면 위치(px)를 화면 평면 좌표로 바꾼다. 카메라가 북쪽을 위로 두고 똑바로 내려다본다는 전제다.
fn cursor_to_ground(cursor: Vec2, window_size: Vec2, camera: Vec3, scale: f32) -> Vec2 {
    let offset = (cursor - window_size / 2.0) * scale;
    Vec2::new(camera.x + offset.x, camera.z + offset.y)
}

/// 화면 범위 (최소, 최대)를 창 크기에 맞춰 보이도록 카메라 위치(가운데)와 축척을 정한다(여백 10 %).
pub fn fit_view(min: Vec2, max: Vec2, window_size: Vec2) -> (Vec2, f32) {
    let size = (max - min).max(Vec2::splat(1.0)) * 1.1;
    let scale = (size.x / window_size.x.max(1.0)).max(size.y / window_size.y.max(1.0));
    ((min + max) / 2.0, scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1))
}

/// 마우스: 휠 확대·축소(커서 위치 고정), 왼쪽 끌기 이동, 왼쪽 클릭 점 추가, 오른쪽 클릭 점 지우기.
/// 버튼 위에서는 점을 찍지 않는다.
fn handle_pointer(
    mut picker: ResMut<Picker>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<PickerCamera>>,
    ui_buttons: Query<&Interaction, With<PickerButton>>,
) {
    let (Ok(window), Ok((mut transform, mut projection))) =
        (windows.single(), cameras.single_mut())
    else {
        return;
    };
    let Projection::Orthographic(ortho) = projection.as_mut() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        picker.press = None;
        return;
    };
    let size = window.size();
    let over_button = ui_buttons
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    // 휠: 커서 아래 지점이 그대로 있도록 카메라를 옮기며 확대·축소한다.
    let steps = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if steps != 0.0 {
        let before = cursor_to_ground(cursor, size, transform.translation, ortho.scale);
        ortho.scale = (ortho.scale * 1.25f32.powf(-steps)).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
        let after = cursor_to_ground(cursor, size, transform.translation, ortho.scale);
        transform.translation.x += before.x - after.x;
        transform.translation.z += before.y - after.y;
    }
    if buttons.just_pressed(MouseButton::Left) && !over_button {
        picker.press = Some((cursor, false));
    }
    if let Some((last, dragging)) = picker.press
        && buttons.pressed(MouseButton::Left)
    {
        let moved = cursor - last;
        if dragging || moved.length() > DRAG_THRESHOLD_PX {
            transform.translation.x -= moved.x * ortho.scale;
            transform.translation.z -= moved.y * ortho.scale;
            picker.press = Some((cursor, true));
        }
    }
    if buttons.just_released(MouseButton::Left) {
        if let Some((_, false)) = picker.press {
            let ground = cursor_to_ground(cursor, size, transform.translation, ortho.scale);
            if let Some(model) = picker.model.as_mut() {
                model.add_point(ground);
            } else {
                picker.message = Some(
                    "No roads loaded here yet: press \"Load roads here\" (L) first".to_owned(),
                );
            }
        }
        picker.press = None;
    }
    if buttons.just_pressed(MouseButton::Right) && !over_button {
        let ground = cursor_to_ground(cursor, size, transform.translation, ortho.scale);
        let radius = REMOVE_RADIUS_PX * ortho.scale;
        if let Some(model) = picker.model.as_mut() {
            model.remove_near(ground, radius);
        }
    }
}

/// 키와 버튼: `Backspace` 마지막 점 지우기, `C` 모두 지우기, `Enter` 주행 시작, `L` 도로 받기,
/// `K`·`G`·`F` 맞춤 보기.
fn handle_keys(
    mut picker: ResMut<Picker>,
    keys: Res<ButtonInput<KeyCode>>,
    pressed: Query<(&Interaction, &PickerButton), Changed<Interaction>>,
) {
    if keys.just_pressed(KeyCode::Backspace)
        && let Some(model) = picker.model.as_mut()
    {
        model.remove_last();
    }
    if keys.just_pressed(KeyCode::KeyC)
        && let Some(model) = picker.model.as_mut()
    {
        model.clear();
    }
    let mut actions: Vec<PickerButton> = pressed
        .iter()
        .filter(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, action)| *action)
        .collect();
    for (key, action) in [
        (KeyCode::Enter, PickerButton::Start),
        (KeyCode::KeyL, PickerButton::Load),
        (KeyCode::KeyK, PickerButton::Fit(Fit::Korea)),
        (KeyCode::KeyG, PickerButton::Fit(Fit::Region)),
        (KeyCode::KeyF, PickerButton::Fit(Fit::Route)),
    ] {
        if keys.just_pressed(key) {
            actions.push(action);
        }
    }
    for action in actions {
        match action {
            PickerButton::Start => picker.start_requested = true,
            PickerButton::Load => picker.load_requested = true,
            PickerButton::Fit(fit) => picker.fit = Some(fit),
        }
    }
}

/// 맞춤 보기 요청과 도로 받기 요청을 카메라 기준으로 처리하고, 경로를 처음 계산하면 한 번 경로에 맞춘다.
fn apply_fit(
    mut picker: ResMut<Picker>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<PickerCamera>>,
) {
    let (Ok(window), Ok((mut transform, mut projection))) =
        (windows.single(), cameras.single_mut())
    else {
        return;
    };
    let Projection::Orthographic(ortho) = projection.as_mut() else {
        return;
    };
    if std::mem::take(&mut picker.load_requested) {
        if ortho.scale > LOAD_MAX_SCALE {
            picker.message =
                Some("Zoom in closer before loading roads (about 1.5 km area)".to_owned());
        } else {
            let center =
                tiles::from_view(Vec2::new(transform.translation.x, transform.translation.z));
            match region_to_load(center, &picker.cached) {
                Ok((bbox, policy)) => begin_fetch(&mut picker, bbox, policy),
                Err(error) => picker.message = Some(format!("Cannot load roads here: {error}")),
            }
        }
    }
    let has_route = picker
        .model
        .as_ref()
        .is_some_and(|model| model.route_bounds().is_some());
    if has_route && !picker.route_fitted && picker.fit.is_none() {
        picker.route_fitted = true;
        picker.fit = Some(Fit::Route);
    }
    let Some(fit) = picker.fit.take() else {
        return;
    };
    let target = match fit {
        Fit::Korea => Some(tiles::view_rect(tiles::KOREA_VIEW)),
        Fit::Region => picker.model.as_ref().map(PickerModel::view_bounds),
        Fit::Route => picker.model.as_ref().and_then(PickerModel::route_bounds),
    };
    match target {
        Some((min, max)) => {
            let (center, scale) = fit_view(min, max, window.size());
            transform.translation.x = center.x;
            transform.translation.z = center.y;
            ortho.scale = scale;
        }
        None => picker.message = Some("Nothing to fit yet".to_owned()),
    }
}

/// 주행 시작 요청을 처리한다: 경로 파일을 저장하고, 그 코스로 주행 상태에 들어간다. 경로가 없거나 저장에
/// 실패하면 알림만 남기고 화면에 머문다.
fn start_driving(
    mut picker: ResMut<Picker>,
    mut course: ResMut<ActiveCourse>,
    mut motion: ResMut<VehicleMotion>,
    mut next: ResMut<NextState<AppMode>>,
) {
    if !std::mem::take(&mut picker.start_requested) {
        return;
    }
    let Some(model) = picker.model.as_ref() else {
        picker.message = Some("No map loaded yet".to_owned());
        return;
    };
    let planned = match &model.planned {
        Some(Ok(planned)) => planned.clone(),
        Some(Err(error)) => {
            picker.message = Some(format!("Cannot start: {}", model::describe_error(error)));
            return;
        }
        None => {
            picker.message = Some("Cannot start: pick at least 2 points".to_owned());
            return;
        }
    };
    let name = format!("route-{}.toml", map_fetch::utc_now_compact());
    match model.save(Path::new(ROUTE_DIR), &name) {
        Ok(path) => {
            info!("route file saved: {}", path.display());
            motion.0 = planned.course.course.start;
            course.0 = planned.course.course;
            next.set(AppMode::Driving);
        }
        Err(error) => picker.message = Some(format!("Cannot save the route: {error}")),
    }
}

/// 위에서 본 타일 사각형 메시(가로세로 1, x·z 0..1). 북서쪽 모서리(0,0)가 이미지 왼쪽 위다.
fn tile_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// 배경 타일: 화면에 보이는 타일만 요청하고, 받은 타일을 사각형으로 깔고, 안 보이는 타일은 지운다.
/// 메모리 예산을 넘으면 오래 안 보인 타일 재질을 버린다.
#[allow(clippy::too_many_arguments)]
fn update_tiles(
    mut commands: Commands,
    mut layer: ResMut<TileLayer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Transform, &Projection), With<PickerCamera>>,
) {
    let (Ok(window), Ok((transform, Projection::Orthographic(ortho)))) =
        (windows.single(), cameras.single())
    else {
        return;
    };
    let layer = &mut *layer;
    layer.frame += 1;
    let frame = layer.frame;
    if layer.fetcher.is_none() {
        layer.fetcher = Some(TileFetcher::start(
            layer.settings.clone(),
            Arc::new(UreqTileTransport),
        ));
    }
    let mesh = layer
        .mesh
        .get_or_insert_with(|| meshes.add(tile_mesh()))
        .clone();
    let half = window.size() / 2.0 * ortho.scale;
    let center = Vec2::new(transform.translation.x, transform.translation.z);
    let zoom = tiles::zoom_for_scale(ortho.scale);
    let wanted = tiles::visible_tiles(center - half, center + half, zoom, MAX_VISIBLE_TILES);
    let fetcher = layer.fetcher.as_mut().expect("fetcher started");
    // 없는 타일과 만료된 타일을 요청한다(만료된 타일은 새 결과가 올 때까지 그대로 보인다).
    let now = tiles::now_s();
    let missing: Vec<TileId> = wanted
        .iter()
        .filter(|tile| {
            layer
                .materials
                .get(tile)
                .is_none_or(|(_, _, expires)| *expires <= now)
        })
        .copied()
        .collect();
    fetcher.request(&missing);
    for (tile, outcome) in fetcher.poll() {
        let TileOutcome::Image { bytes, expires_at } = outcome else {
            continue;
        };
        let Ok(image) = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::linear(),
            RenderAssetUsages::RENDER_WORLD,
        ) else {
            // 디코딩하지 못하면 캐시를 지우고 실패로 셈한다(매 프레임 다시 처리하지 않게).
            fetcher.report_bad(tile);
            continue;
        };
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            cull_mode: None,
            ..default()
        });
        layer.materials.insert(tile, (material, frame, expires_at));
        // 새로 받은(갱신된) 타일은 다시 그리도록 기존 엔티티를 지운다.
        if let Some(entity) = layer.entities.remove(&tile) {
            commands.entity(entity).despawn();
        }
    }
    for tile in &wanted {
        if let Some((material, seen, _)) = layer.materials.get_mut(tile) {
            *seen = frame;
            if !layer.entities.contains_key(tile) {
                let (min, max) = tile.view_rect();
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(min.x, TILE_Y, min.y).with_scale(Vec3::new(
                            max.x - min.x,
                            1.0,
                            max.y - min.y,
                        )),
                        DespawnOnExit(AppMode::RoutePicking),
                    ))
                    .id();
                layer.entities.insert(*tile, entity);
            }
        }
    }
    let stale: Vec<TileId> = layer
        .entities
        .keys()
        .filter(|tile| !wanted.contains(tile))
        .copied()
        .collect();
    for tile in stale {
        if let Some(entity) = layer.entities.remove(&tile) {
            commands.entity(entity).despawn();
        }
    }
    if layer.materials.len() > MEMORY_BUDGET_TILES {
        let mut by_age: Vec<(u64, TileId)> = layer
            .materials
            .iter()
            .filter(|(tile, _)| !layer.entities.contains_key(tile))
            .map(|(tile, (_, seen, _))| (*seen, *tile))
            .collect();
        by_age.sort();
        let extra = layer.materials.len() - MEMORY_BUDGET_TILES;
        for (_, tile) in by_age.into_iter().take(extra) {
            layer.materials.remove(&tile);
        }
    }
}

/// 이미 받은 범위(노랑)·받는 중인 범위(하늘색), 도로(경로 탐색에 쓰는 도로는 진하게, 그 밖은 옅게),
/// 계산된 경로, 찍은 점을 그린다.
fn draw_picker(
    picker: Res<Picker>,
    mut gizmos: Gizmos,
    cameras: Query<&Projection, With<PickerCamera>>,
) {
    let scale = match cameras.single() {
        Ok(Projection::Orthographic(ortho)) => ortho.scale,
        _ => 1.0,
    };
    let at = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    let outline = |gizmos: &mut Gizmos, bbox: &Bbox, color: Color| {
        let (min, max) = tiles::view_rect([bbox.south, bbox.west, bbox.north, bbox.east]);
        let corners = [
            min,
            Vec2::new(max.x, min.y),
            max,
            Vec2::new(min.x, max.y),
            min,
        ];
        for pair in corners.windows(2) {
            gizmos.line(at(pair[0], 0.5), at(pair[1], 0.5), color);
        }
    };
    for bbox in &picker.cached {
        outline(&mut gizmos, bbox, Color::srgb(0.95, 0.75, 0.1));
    }
    if let Some((_, bbox)) = &picker.loading {
        outline(&mut gizmos, bbox, Color::srgb(0.2, 0.8, 1.0));
    }
    let Some(model) = picker.model.as_ref() else {
        return;
    };
    for segment in &model.segments {
        let color = if segment.usable {
            Color::srgb(0.1, 0.25, 0.75)
        } else {
            Color::srgb(0.45, 0.45, 0.5)
        };
        gizmos.line(at(segment.from, 0.0), at(segment.to, 0.0), color);
    }
    let route = model.route_views();
    let route_color = match &model.planned {
        Some(Ok(_)) => Color::srgb(1.0, 0.45, 0.0),
        _ => Color::srgb(0.9, 0.1, 0.1),
    };
    for pair in route.windows(2) {
        gizmos.line(at(pair[0], 1.0), at(pair[1], 1.0), route_color);
    }
    let points = model.point_views();
    let last = points.len().saturating_sub(1);
    for (index, point) in points.iter().enumerate() {
        let color = match index {
            0 => Color::srgb(0.1, 0.75, 0.2),
            i if i == last => Color::srgb(0.9, 0.15, 0.15),
            _ => Color::srgb(0.2, 0.5, 1.0),
        };
        gizmos.circle(
            Isometry3d::new(at(*point, 2.0), Quat::from_rotation_x(FRAC_PI_2)),
            POINT_RADIUS_PX * scale,
            color,
        );
    }
}

/// 안내 문구를 갱신한다: 조작법, 지도 상태, 점·경로·폭·경고, 마지막 알림.
fn update_picker_text(
    picker: Res<Picker>,
    layer: Res<TileLayer>,
    mut texts: Query<&mut Text, With<PickerText>>,
) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    let mut lines = vec![
        "Route picker - left click: add point, drag: move, wheel: zoom, Esc twice: menu".to_owned(),
        "Right click: remove point, Backspace: remove last, C: clear, Enter: start".to_owned(),
        "L: load roads here, K/G/F: fit Korea/region/route. Bright blue = usable roads, yellow box = saved area"
            .to_owned(),
    ];
    if !layer.settings.network {
        lines.push("Map tiles: offline (cached tiles only)".to_owned());
    }
    match &picker.model {
        Some(model) => {
            lines.push(format!("Map: {}", model.snapshot_path.display()));
            lines.extend(model.status_lines());
        }
        None if picker.loading.is_some() => lines.push("Loading roads...".to_owned()),
        None => {}
    }
    if let Some(message) = &picker.message {
        lines.push(message.clone());
    }
    let joined = lines.join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 지역 인자: 미리 정한 지역, 위경도(대한민국·크기 검사), 스냅샷 경로, 잘못된 값.
    #[test]
    fn source_is_parsed() {
        assert_eq!(
            PickerSource::parse("yeosu").unwrap(),
            PickerSource::Snapshot(PathBuf::from(PRESET_REGIONS[0].1))
        );
        let PickerSource::Fetch(bbox) = PickerSource::parse("latlon:34.75,127.75").unwrap() else {
            panic!();
        };
        let (east_west, north_south) = bbox.size_m();
        assert!((east_west - 1500.0).abs() < 1.0 && (north_south - 1500.0).abs() < 1.0);
        assert!(PickerSource::parse("latlon:40.0,127.0").is_err());
        assert!(PickerSource::parse("latlon:34.75").is_err());
        assert_eq!(
            PickerSource::parse("my.json").unwrap(),
            PickerSource::Snapshot(PathBuf::from("my.json"))
        );
        assert!(PickerSource::parse(" ").is_err());
    }

    /// 점 목록 인자: 세미콜론으로 나눈 위도·경도 쌍, 잘못된 값은 오류.
    #[test]
    fn points_are_parsed() {
        assert_eq!(
            parse_points("34.75,127.74; 34.74,127.75;").unwrap(),
            vec![[34.75, 127.74], [34.74, 127.75]]
        );
        assert!(parse_points("34.75").is_err());
        assert!(parse_points("a,b").is_err());
        assert!(parse_points("NaN,1").is_err());
    }

    /// 화면 위치 → 지역 평면: 화면 가운데가 카메라 위치, 오른쪽이 동쪽(+X), 아래쪽이 남쪽(+Z)이다.
    #[test]
    fn cursor_maps_to_ground() {
        let size = Vec2::new(800.0, 600.0);
        let camera = Vec3::new(10.0, 500.0, -20.0);
        assert_eq!(
            cursor_to_ground(size / 2.0, size, camera, 2.0),
            Vec2::new(10.0, -20.0)
        );
        assert_eq!(
            cursor_to_ground(Vec2::new(500.0, 400.0), size, camera, 2.0),
            Vec2::new(210.0, 180.0)
        );
    }

    /// 맞춤 보기: 범위 가운데로 옮기고 창에 들어오는 축척(여백 10 %)을 고른다.
    #[test]
    fn fit_view_centers_and_scales() {
        let (center, scale) = fit_view(
            Vec2::new(0.0, 0.0),
            Vec2::new(1000.0, 500.0),
            Vec2::new(1000.0, 1000.0),
        );
        assert_eq!(center, Vec2::new(500.0, 250.0));
        assert!((scale - 1.1).abs() < 1e-5);
        let (korea_min, korea_max) = tiles::view_rect(tiles::KOREA_VIEW);
        let (_, scale) = fit_view(korea_min, korea_max, Vec2::new(1280.0, 720.0));
        assert!(scale > 500.0 && scale <= SCALE_RANGE.1, "{scale}");
    }
}
