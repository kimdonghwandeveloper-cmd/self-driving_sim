//! 경로 선택 화면(Phase 4-5).
//!
//! 앱 상태 [`AppMode::RoutePicking`]에서 도로 그래프를 위에서 본 선으로 그리고, 마우스로 점을 찍어 경로를 계산한다.
//! "주행 시작"(버튼 또는 Enter)을 누르면 경로 파일을 `cache/routes/`에 저장하고 그 코스로 [`AppMode::Driving`]에
//! 들어간다. 지도 타일 이미지는 쓰지 않고 받은 벡터 자료만 그리며, OSM 출처 표기를 계속 보인다.
//!
//! 조작(개발용):
//! - 왼쪽 클릭: 점 추가(시작 → 경유 → 끝). 왼쪽 끌기: 화면 이동. 휠: 확대·축소.
//! - 오른쪽 클릭: 가까운 점 지우기. `Backspace`: 마지막 점 지우기. `C`: 모두 지우기. `Enter`: 주행 시작.

pub mod model;

use std::f32::consts::FRAC_PI_2;
use std::path::{Path, PathBuf};

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::AppMode;
use crate::vehicle::VehicleMotion;
use crate::world::ActiveCourse;
use crate::world::map::OSM_ATTRIBUTION;
use crate::world::map_fetch::{self, Bbox, FetchJob, FetchPolicy, FetchSettings};
use crate::world::osm_graph::parse_overpass_graph;
use crate::world::route_plan::{self, RoadGraphConfig};
use model::PickerModel;

/// 미리 정한 지역 이름과 그 지역의 저장된 노드 ID 스냅샷.
pub const PRESET_REGIONS: [(&str, &str); 1] =
    [("yeosu", "assets/maps/yeosu-expo/graph-2026-11-06.osm.json")];
/// 위경도 입력으로 받을 범위의 반 변(m). 한 변 1.5 km(PLAN 코스 범위 상한과 같음, 2 km 받기 상한 안).
pub const FETCH_HALF_SIDE_M: f64 = 750.0;
/// 경로 파일 저장 폴더(실행 폴더 기준, 저장소에서 제외되는 `cache/` 아래).
pub const ROUTE_DIR: &str = "cache/routes";
/// 카메라 높이(m). 직교 투영이라 값 자체는 보이는 크기에 영향이 없다.
const CAMERA_HEIGHT_M: f32 = 500.0;
/// 처음 확대 수준(m/px)과 허용 범위.
const START_SCALE: f32 = 1.0;
const SCALE_RANGE: (f32, f32) = (0.05, 5.0);
/// 이 픽셀보다 많이 움직이면 클릭이 아니라 끌기로 본다.
const DRAG_THRESHOLD_PX: f32 = 5.0;
/// 오른쪽 클릭으로 점을 지울 수 있는 화면 거리(px).
const REMOVE_RADIUS_PX: f32 = 14.0;
/// 점 표시 반경(px).
const POINT_RADIUS_PX: f32 = 6.0;

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

/// 위경도를 가운데로 하는 한 변 2 × [`FETCH_HALF_SIDE_M`] 범위를 만들고 검사한다.
pub fn bbox_around(lat: f64, lon: f64) -> Result<Bbox, String> {
    if !(lat.is_finite() && lon.is_finite() && lat.abs() < 85.0) {
        return Err(format!("invalid point [{lat}, {lon}]"));
    }
    let radius = 6_371_008.8_f64;
    let d_lat = (FETCH_HALF_SIDE_M / radius).to_degrees();
    let d_lon = (FETCH_HALF_SIDE_M / (radius * lat.to_radians().cos())).to_degrees();
    let bbox = Bbox {
        south: lat - d_lat,
        west: lon - d_lon,
        north: lat + d_lat,
        east: lon + d_lon,
    };
    bbox.validate()?;
    Ok(bbox)
}

/// 스냅샷 파일을 읽어 경로 선택 상태를 만든다.
pub fn load_model(path: &Path) -> Result<PickerModel, String> {
    let json = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let ways = parse_overpass_graph(&json)?;
    let config = std::fs::read_to_string(route_plan::ROAD_GRAPH_CONFIG_PATH)
        .map_err(|error| error.to_string())
        .and_then(|text| route_plan::parse_road_graph_config(&text))
        .unwrap_or_else(|error| {
            warn!("road graph config unavailable ({error}); using defaults");
            RoadGraphConfig::default()
        });
    PickerModel::new(path.to_path_buf(), ways, config)
}

/// 경로 선택 화면 플러그인. 앱이 [`AppMode::RoutePicking`]으로 시작할 때만 등록한다.
pub struct PickerPlugin {
    /// 다룰 지역.
    pub source: PickerSource,
    /// 지도를 불러온 뒤 미리 찍어 둘 점 [위도, 경도].
    pub preset_points: Vec<[f64; 2]>,
    /// 지도를 불러오면 곧바로 주행 시작을 요청한다(화면 전환 자동 검증용).
    pub auto_start: bool,
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
    /// 화면 구성(상태 진입 시)과 매 프레임 입력·그리기·안내 갱신을 등록한다.
    fn build(&self, app: &mut App) {
        let picking = in_state(AppMode::RoutePicking);
        app.insert_resource(Picker {
            source: Some(self.source.clone()),
            preset_points: self.preset_points.clone(),
            auto_start: self.auto_start,
            ..default()
        })
        .add_systems(
            OnEnter(AppMode::RoutePicking),
            (spawn_picker, start_loading),
        )
        .add_systems(
            Update,
            (
                poll_loading,
                handle_pointer,
                handle_keys,
                start_driving,
                draw_picker,
                update_picker_text,
            )
                .chain()
                .run_if(picking),
        );
    }
}

/// 경로 선택 화면 상태.
#[derive(Resource, Default)]
pub struct Picker {
    /// 다룰 지역.
    source: Option<PickerSource>,
    /// 불러온 상태. 받는 중이거나 실패하면 `None`.
    model: Option<PickerModel>,
    /// 진행 중인 지도 받기.
    loading: Option<FetchJob>,
    /// 마지막 알림(불러오기 실패, 저장 결과 등).
    message: Option<String>,
    /// 왼쪽 버튼을 누른 곳(px)과 끌기 중인지.
    press: Option<(Vec2, bool)>,
    /// 주행 시작 요청.
    start_requested: bool,
    /// 지도를 불러온 뒤 미리 찍어 둘 점.
    preset_points: Vec<[f64; 2]>,
    /// 지도를 불러오면 곧바로 주행 시작을 요청할지.
    auto_start: bool,
}

impl Picker {
    /// 불러온 상태를 받아 들이고, 미리 찍을 점과 자동 시작 요청을 적용한다.
    fn accept(&mut self, mut model: PickerModel) {
        if !self.preset_points.is_empty() {
            model.set_points(&std::mem::take(&mut self.preset_points));
        }
        self.model = Some(model);
        if self.auto_start {
            self.start_requested = true;
        }
    }
}

impl Default for PickerSource {
    /// 기본 지역은 여수 스냅샷이다.
    fn default() -> Self {
        Self::Snapshot(PathBuf::from(PRESET_REGIONS[0].1))
    }
}

/// 경로 선택 화면의 위에서 본 카메라.
#[derive(Component)]
pub struct PickerCamera;

/// 경로 선택 안내 문구.
#[derive(Component)]
struct PickerText;

/// 주행 시작 버튼.
#[derive(Component)]
struct StartButton;

/// 위에서 본 직교 카메라, 안내 문구, 출처 표기, 주행 시작 버튼을 만든다. 모두 상태를 나가면 지운다.
fn spawn_picker(mut commands: Commands) {
    let scoped = DespawnOnExit(AppMode::RoutePicking);
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scale: START_SCALE,
            far: CAMERA_HEIGHT_M * 2.0,
            ..OrthographicProjection::default_3d()
        }),
        // 아래를 보고 화면 위쪽이 북쪽(-Z)이 되게 한다. 화면 오른쪽은 동쪽(+X)이다.
        Transform::from_xyz(0.0, CAMERA_HEIGHT_M, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
        PickerCamera,
        scoped.clone(),
    ));
    let panel = |top: Option<f32>, bottom: Option<f32>| Node {
        position_type: PositionType::Absolute,
        top: top.map_or(Val::Auto, Val::Px),
        bottom: bottom.map_or(Val::Auto, Val::Px),
        left: Val::Px(12.0),
        padding: UiRect::all(Val::Px(8.0)),
        ..default()
    };
    commands.spawn((
        panel(Some(12.0), None),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Text::new("Loading map data..."),
        TextFont::from_font_size(16.0),
        TextColor(Color::WHITE),
        PickerText,
        scoped.clone(),
    ));
    commands.spawn((
        panel(None, Some(12.0)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Text::new(format!(
            "{OSM_ATTRIBUTION}\nVirtual training course (not a surveyed road). Vector data only, no map tiles."
        )),
        TextFont::from_font_size(14.0),
        TextColor(Color::WHITE),
        scoped.clone(),
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                right: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                ..default()
            },
            Button,
            BackgroundColor(Color::srgb(0.15, 0.45, 0.2)),
            StartButton,
            scoped,
        ))
        .with_child((
            Text::new("Start driving (Enter)"),
            TextFont::from_font_size(16.0),
            TextColor(Color::WHITE),
        ));
}

/// 지역 스냅샷을 불러오거나, 위경도 지역이면 백그라운드 받기를 시작한다.
fn start_loading(mut picker: ResMut<Picker>) {
    match picker.source.clone().unwrap_or_default() {
        PickerSource::Snapshot(path) => match load_model(&path) {
            Ok(model) => picker.accept(model),
            Err(error) => picker.message = Some(format!("Cannot load the map: {error}")),
        },
        PickerSource::Fetch(bbox) => {
            match FetchJob::start(
                PathBuf::from(map_fetch::CACHE_DIR),
                bbox,
                FetchPolicy::Refresh,
                FetchSettings::default(),
            ) {
                Ok(job) => {
                    picker.loading = Some(job);
                    picker.message = Some("Downloading map data from Overpass...".to_owned());
                }
                Err(error) => picker.message = Some(format!("Cannot download the map: {error}")),
            }
        }
    }
}

/// 받기 작업이 끝났으면 받은(또는 캐시의) 스냅샷을 불러온다. 기다리지 않는다.
fn poll_loading(mut picker: ResMut<Picker>) {
    let Some(result) = picker.loading.as_ref().and_then(FetchJob::try_result) else {
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
            picker.message = warning.map(|warning| format!("Warning: {warning}"));
        }
        Err(error) => picker.message = Some(format!("Cannot load the map: {error}")),
    }
}

/// 화면 위치(px)를 지역 평면 좌표로 바꾼다. 카메라가 북쪽을 위로 두고 똑바로 내려다본다는 전제다.
fn cursor_to_ground(cursor: Vec2, window_size: Vec2, camera: Vec3, scale: f32) -> Vec2 {
    let offset = (cursor - window_size / 2.0) * scale;
    Vec2::new(camera.x + offset.x, camera.z + offset.y)
}

/// 마우스: 휠 확대·축소(커서 위치 고정), 왼쪽 끌기 이동, 왼쪽 클릭 점 추가, 오른쪽 클릭 점 지우기.
/// 주행 시작 버튼 위에서는 점을 찍지 않는다.
fn handle_pointer(
    mut picker: ResMut<Picker>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<PickerCamera>>,
    start_button: Query<&Interaction, With<StartButton>>,
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
    let over_button = start_button
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    // 휠: 커서 아래 지점이 그대로 있도록 카메라를 옮기며 확대·축소한다.
    let steps = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if steps != 0.0 {
        let before = cursor_to_ground(cursor, size, transform.translation, ortho.scale);
        ortho.scale = (ortho.scale * 1.2f32.powf(-steps)).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
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

/// 키: `Backspace` 마지막 점 지우기, `C` 모두 지우기, `Enter` 주행 시작. 버튼을 눌러도 주행 시작을 요청한다.
fn handle_keys(
    mut picker: ResMut<Picker>,
    keys: Res<ButtonInput<KeyCode>>,
    pressed: Query<&Interaction, (Changed<Interaction>, With<StartButton>)>,
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
    if keys.just_pressed(KeyCode::Enter)
        || pressed
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        picker.start_requested = true;
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

/// 도로(경로 탐색에 쓰는 도로는 밝게, 그 밖은 어둡게), 계산된 경로, 찍은 점을 그린다.
fn draw_picker(
    picker: Res<Picker>,
    mut gizmos: Gizmos,
    cameras: Query<&Projection, With<PickerCamera>>,
) {
    let Some(model) = picker.model.as_ref() else {
        return;
    };
    let scale = match cameras.single() {
        Ok(Projection::Orthographic(ortho)) => ortho.scale,
        _ => 1.0,
    };
    let at = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    for segment in &model.segments {
        let color = if segment.usable {
            Color::srgb(0.95, 0.95, 0.95)
        } else {
            Color::srgb(0.35, 0.38, 0.42)
        };
        gizmos.line(at(segment.from, 0.0), at(segment.to, 0.0), color);
    }
    let route = model.route_views();
    let route_color = match &model.planned {
        Some(Ok(_)) => Color::srgb(1.0, 0.55, 0.0),
        _ => Color::srgb(0.9, 0.1, 0.1),
    };
    for pair in route.windows(2) {
        gizmos.line(at(pair[0], 1.0), at(pair[1], 1.0), route_color);
    }
    let points = model.point_views();
    let last = points.len().saturating_sub(1);
    for (index, point) in points.iter().enumerate() {
        let color = match index {
            0 => Color::srgb(0.1, 0.85, 0.2),
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

/// 안내 문구를 갱신한다: 조작법, 점·경로·폭·경고, 마지막 알림.
fn update_picker_text(picker: Res<Picker>, mut texts: Query<&mut Text, With<PickerText>>) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    let mut lines = vec![
        "Route picker - left click: add point, drag: move, wheel: zoom".to_owned(),
        "Right click: remove point, Backspace: remove last, C: clear, Enter: start".to_owned(),
    ];
    match &picker.model {
        Some(model) => {
            lines.push(format!("Map: {}", model.snapshot_path.display()));
            lines.extend(model.status_lines());
        }
        None if picker.loading.is_some() => lines.push("Downloading map data...".to_owned()),
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
}
