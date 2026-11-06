//! 입석 조작자 1인칭 카메라(Phase 3-1).
//!
//! - 기본·리셋 시점은 `61.jpg` 구도를 기준으로 한 전방 시야다.
//! - 전·후·좌·우 전환은 같은 눈 위치에서 yaw만 바꾸며 차량 운동·기어·조향에 영향을 주지 않는다.
//! - 외관 차체는 [`EXTERIOR_LAYER`]에 두어 1인칭 카메라에서 제외하고, 최소 실내 형상만 보이게 한다.
//! - 개발용 외부 카메라는 명시적으로 켜는 디버그 모드로만 남긴다.

pub mod interior;
pub mod views;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::SimSet;
use crate::calibration::CourseResetDone;
use crate::input::KeyBindings;
use crate::input::mapping::KeyAction;
use crate::scene::external_camera_transform;
use crate::vehicle::VehicleMotion;

pub use views::{OperatorView, ViewDirection, operator_camera_transform};

/// 외관 차체를 두는 렌더 레이어. 1인칭 카메라는 기본 레이어(0)만 그린다.
pub const EXTERIOR_LAYER: usize = 1;

/// 입석 조작자 카메라 설정(설정 파일에서 읽은 값).
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct CameraSettings(pub OperatorView);

/// 현재 시점 상태. 주행 입력과 별도로 관리한다.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveView {
    /// 입석 조작자 시선 방향.
    pub direction: ViewDirection,
    /// 개발용 외부(뒤쪽 위) 카메라 사용 여부.
    pub debug_external: bool,
}

/// 입석 조작자 카메라 엔티티 표시용 마커.
#[derive(Component)]
pub struct OperatorCamera;

/// 카메라 생성, 시점 키 처리, 카메라 자세 갱신 시스템을 등록하는 플러그인.
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    /// 시점 리소스와 카메라 생성(시작 시)·시점 선택·카메라 갱신(매 프레임) 시스템을 등록한다.
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSettings>()
            .init_resource::<ActiveView>()
            .add_systems(Startup, spawn_operator_camera)
            .add_systems(
                Update,
                (select_view, sync_operator_camera)
                    .chain()
                    .in_set(SimSet::Display),
            );
    }
}

/// 설정값(수직 FOV·near clip)으로 입석 조작자 카메라를 만든다.
fn spawn_operator_camera(
    mut commands: Commands,
    settings: Res<CameraSettings>,
    motion: Res<VehicleMotion>,
) {
    let view = &settings.0;
    commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: view.vertical_fov_deg.to_radians(),
            near: view.near_clip_m,
            ..default()
        }),
        operator_camera_transform(&motion.0, view, ViewDirection::default()),
        RenderLayers::layer(0),
        OperatorCamera,
    ));
}

/// 시점 키와 코스 복귀 결과로 [`ActiveView`]를 갱신한다.
///
/// 코스 복귀가 적용되면 먼저 전방으로 되돌린 뒤 같은 프레임의 시점 키를 처리한다.
/// 디버그 외부 카메라 키는 누를 때마다 켜고 끈다.
pub fn select_view(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut resets: MessageReader<CourseResetDone>,
    mut active: ResMut<ActiveView>,
) {
    let mut next = *active;
    if resets.read().count() > 0 {
        next.direction = ViewDirection::Front;
    }
    let pressed = |direction| {
        bindings
            .keys(KeyAction::View(direction))
            .iter()
            .any(|key| keys.pressed(*key))
    };
    let just_pressed = |direction| {
        bindings
            .keys(KeyAction::View(direction))
            .iter()
            .any(|key| keys.just_pressed(*key))
    };
    let held: Vec<ViewDirection> = ViewDirection::ALL
        .into_iter()
        .filter(|direction| pressed(*direction))
        .collect();
    let new: Vec<ViewDirection> = ViewDirection::ALL
        .into_iter()
        .filter(|direction| just_pressed(*direction))
        .collect();
    next.direction = views::resolve_view_selection(next.direction, &new, &held);
    if bindings
        .keys(KeyAction::DebugCamera)
        .iter()
        .any(|key| keys.just_pressed(*key))
    {
        next.debug_external = !next.debug_external;
    }
    if next != *active {
        info!(
            "view: {}{}",
            next.direction.label(),
            if next.debug_external {
                " (debug external camera)"
            } else {
                ""
            }
        );
        *active = next;
    }
}

/// 차량 자세와 시점 상태로 카메라 자세와 렌더 레이어를 갱신한다. 1인칭 시점은 보간 없이 즉시 따른다.
pub fn sync_operator_camera(
    motion: Res<VehicleMotion>,
    settings: Res<CameraSettings>,
    active: Res<ActiveView>,
    mut cameras: Query<(&mut Transform, &mut RenderLayers), With<OperatorCamera>>,
) {
    let (transform, layers) = if active.debug_external {
        (
            external_camera_transform(&motion.0),
            RenderLayers::from_layers(&[0, EXTERIOR_LAYER]),
        )
    } else {
        (
            operator_camera_transform(&motion.0, &settings.0, active.direction),
            RenderLayers::layer(0),
        )
    };
    for (mut camera, mut camera_layers) in &mut cameras {
        *camera = transform;
        if *camera_layers != layers {
            *camera_layers = layers.clone();
        }
    }
}
