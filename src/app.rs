use std::path::{Path, PathBuf};

use bevy::render::view::screenshot::{Capturing, Screenshot, save_to_disk};
use bevy::{app::AppExit, prelude::*};
use self_driving_sim::{
    calibration::CalibrationPlugin,
    camera::{ActiveView, CameraPlugin, ViewDirection},
    config, configure_sim_sets,
    input::InputPlugin,
    scene::ScenePlugin,
    training::TrainingPlugin,
    ui::{HudPlugin, hud::VehicleHud},
    vehicle::{Vehicle, VehicleDynamics, VehicleMotion, VehiclePlugin, VehicleState},
    world::{ActiveCourse, CourseSelection},
};

/// 기본 창 크기(px).
const DEFAULT_WINDOW: (u32, u32) = (1280, 720);
/// 캡처 모드에서 렌더 파이프라인 준비를 기다리는 시간(s).
const CAPTURE_WARMUP_S: f32 = 3.0;
/// 캡처 모드에서 시점을 바꾼 뒤 저장까지 기다리는 프레임 수.
const CAPTURE_SETTLE_FRAMES: u32 = 20;

/// 명령줄에서 정한 실행 방식.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// 약 5초 후 자동 종료.
    pub smoke_test: bool,
    /// 시점 캡처를 저장할 폴더. 있으면 캡처 후 종료한다.
    pub capture_dir: Option<PathBuf>,
    /// 창 크기(px). 없으면 [`DEFAULT_WINDOW`].
    pub window: Option<(u32, u32)>,
    /// 주행 코스.
    pub course: CourseSelection,
}

/// `"1498x700"` 형식의 창 크기를 읽는다. 0이나 형식 오류는 `None`.
pub fn parse_window_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    let size = (width.trim().parse().ok()?, height.trim().parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// 앱을 구성하고 실행한다.
pub fn run(options: RunOptions) -> AppExit {
    let mut app = App::new();
    let (width, height) = options.window.unwrap_or(DEFAULT_WINDOW);
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Shuttle Training Simulator - Development Setup".into(),
            resolution: (width, height).into(),
            ..default()
        }),
        ..default()
    }))
    // 낮 하늘색 배경: 창 양옆의 어두운 내장재와 창 영역이 구분되게 한다.
    .insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.82)));

    // 로그 플러그인이 등록된 뒤에 설정을 읽어 로드 결과가 로그에 남도록 한다.
    let vehicle_config = config::load_vehicle_config(Path::new(config::VEHICLE_CONFIG_PATH));
    let key_bindings = config::load_key_bindings(Path::new(config::INPUT_CONFIG_PATH));
    let gamepad_bindings = config::load_gamepad_bindings(Path::new(config::INPUT_CONFIG_PATH));
    let camera_settings = config::load_camera_config(Path::new(config::CAMERA_CONFIG_PATH));
    let (course, fallback) = options.course.resolve();
    if let Some(error) = fallback {
        warn!("map course unavailable ({error}); using training course");
    }
    info!("course: {}", course.name);
    configure_sim_sets(&mut app);
    app.insert_resource(VehicleMotion(course.start))
        .insert_resource(ActiveCourse(course));
    app.insert_resource(Vehicle(VehicleState::new(
        vehicle_config.state_machine.stop_threshold_mps,
    )))
    .insert_resource(VehicleDynamics(vehicle_config.dynamics))
    .insert_resource(key_bindings)
    .insert_resource(gamepad_bindings)
    .insert_resource(camera_settings)
    .add_plugins((
        InputPlugin,
        VehiclePlugin,
        TrainingPlugin,
        CalibrationPlugin,
        ScenePlugin,
        CameraPlugin,
        HudPlugin,
    ));

    if options.smoke_test {
        app.add_systems(Update, finish_smoke_test);
    }
    if let Some(dir) = options.capture_dir {
        if let Err(error) = std::fs::create_dir_all(&dir) {
            error!("cannot create capture folder {}: {error}", dir.display());
        }
        app.insert_resource(CaptureViews { dir, ..default() })
            .add_systems(Update, capture_views);
    }

    app.run()
}

/// 자동 검증 모드에서 5초가 지나면 앱을 정상 종료한다.
fn finish_smoke_test(time: Res<Time>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() >= 5.0 {
        info!("Window/render smoke test completed.");
        exit.write(AppExit::Success);
    }
}

/// 캡처 순서(시점, HUD 표시 여부). 시점 `None`은 외부 디버그 카메라다.
/// 기준 사진과 구도를 비교하도록 HUD를 숨긴 캡처를 먼저 찍고, HUD 확인용 전방 캡처를 마지막에 찍는다.
const CAPTURE_ORDER: [(Option<ViewDirection>, bool); 6] = [
    (Some(ViewDirection::Front), false),
    (Some(ViewDirection::Rear), false),
    (Some(ViewDirection::Left), false),
    (Some(ViewDirection::Right), false),
    (None, false),
    (Some(ViewDirection::Front), true),
];

/// 시점 캡처 진행 상태.
#[derive(Resource, Debug, Default)]
struct CaptureViews {
    /// 저장 폴더.
    dir: PathBuf,
    /// 다음에 찍을 [`CAPTURE_ORDER`] 위치.
    next: usize,
    /// 시점을 바꾼 뒤 지난 프레임 수.
    frames: u32,
    /// 현재 시점을 이미 설정했는지.
    applied: bool,
}

/// 렌더 준비 후 시점을 하나씩 바꿔 PNG로 저장하고, 저장이 모두 끝나면 앱을 종료한다.
fn capture_views(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<CaptureViews>,
    mut view: ResMut<ActiveView>,
    capturing: Query<(), With<Capturing>>,
    mut huds: Query<&mut Visibility, With<VehicleHud>>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs() < CAPTURE_WARMUP_S {
        return;
    }
    let Some((target, show_hud)) = CAPTURE_ORDER.get(state.next).copied() else {
        if capturing.is_empty() {
            info!("View capture completed: {}", state.dir.display());
            exit.write(AppExit::Success);
        }
        return;
    };
    if !state.applied {
        *view = ActiveView {
            direction: target.unwrap_or_default(),
            debug_external: target.is_none(),
        };
        for mut visibility in &mut huds {
            *visibility = if show_hud {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        state.applied = true;
        state.frames = 0;
        return;
    }
    state.frames += 1;
    if state.frames < CAPTURE_SETTLE_FRAMES || !capturing.is_empty() {
        return;
    }
    let name = target.map_or("external", |direction| direction.label());
    let suffix = if show_hud { "-hud" } else { "" };
    let path = state
        .dir
        .join(format!("view-{}{suffix}.png", name.to_ascii_lowercase()));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    state.next += 1;
    state.applied = false;
}
