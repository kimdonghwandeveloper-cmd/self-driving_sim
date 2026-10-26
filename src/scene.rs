//! 개발용 3D 장면: 격자 바닥, 움직이는 차량 도형, 차량을 따라가는 카메라.
//!
//! Phase 3의 입석 조작자 카메라·테스트맵이 생기기 전까지 운동 모델을 눈으로 확인하는 용도다.
//! 차체 크기와 카메라 위치는 모두 **시뮬레이터 임시값**이다.

use bevy::prelude::*;

use crate::SimSet;
use crate::vehicle::{Motion, VehicleMotion};

/// 바닥 한 변 길이(m).
const GROUND_SIZE_M: f32 = 200.0;
/// 격자선 간격(m). 이동 거리를 눈으로 가늠하는 기준이다.
const GRID_SPACING_M: f32 = 5.0;
/// 차체 크기(폭, 높이, 길이; m). 실차 치수 미확인 임시값.
const BODY_SIZE_M: Vec3 = Vec3::new(1.8, 2.2, 4.2);
/// 후륜축 중심에서 차체 중심까지의 전방 거리(m).
const REAR_AXLE_TO_CENTER_M: f32 = 1.4;
/// 차량 기준 카메라 위치(오른쪽, 위, 뒤; m).
const CAMERA_OFFSET_M: Vec3 = Vec3::new(0.0, 5.0, 11.0);
/// 카메라가 차량을 따라가는 빠르기(1/s). 클수록 바로 붙는다.
const CAMERA_FOLLOW_RATE: f32 = 4.0;

/// 차량 도형 루트 엔티티 표시용 마커. Transform 원점은 후륜축 중심이다.
#[derive(Component)]
pub struct VehicleBody;

/// 차량을 따라가는 개발용 카메라 마커.
#[derive(Component)]
pub struct FollowCamera;

/// 장면 생성과 차량 도형·카메라 동기화 시스템을 등록하는 플러그인.
pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    /// 장면 생성(시작 시)과 매 프레임 차량 도형·카메라 갱신 시스템을 등록한다.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene).add_systems(
            Update,
            (sync_vehicle_body, follow_vehicle)
                .chain()
                .in_set(SimSet::Display),
        );
    }
}

/// 바닥, 격자선, 차량 도형, 조명, 카메라를 배치한다.
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROUND_SIZE_M, GROUND_SIZE_M))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.30, 0.30))),
    ));
    spawn_grid(&mut commands, &mut meshes, &mut materials);

    let body = meshes.add(Cuboid::from_size(BODY_SIZE_M));
    let front_marker = meshes.add(Cuboid::new(BODY_SIZE_M.x * 0.8, 0.3, 0.3));
    commands
        .spawn((Transform::default(), Visibility::default(), VehicleBody))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(body),
                MeshMaterial3d(materials.add(Color::srgb(0.30, 0.65, 0.85))),
                Transform::from_xyz(0.0, BODY_SIZE_M.y / 2.0, -REAR_AXLE_TO_CENTER_M),
            ));
            // 전방(-Z) 위쪽 노란 막대: 차량이 어느 쪽을 보는지 표시한다.
            parent.spawn((
                Mesh3d(front_marker),
                MeshMaterial3d(materials.add(Color::srgb(0.95, 0.80, 0.20))),
                Transform::from_xyz(
                    0.0,
                    BODY_SIZE_M.y + 0.15,
                    -REAR_AXLE_TO_CENTER_M - BODY_SIZE_M.z / 2.0 + 0.3,
                ),
            ));
        });

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        camera_transform(&Motion::default()),
        FollowCamera,
    ));
    info!("Development scene ready. Close the window to exit.");
}

/// 바닥 위에 일정 간격의 격자선을 깐다. 원점을 지나는 두 선은 밝게 표시한다.
fn spawn_grid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let along_x = meshes.add(Cuboid::new(GROUND_SIZE_M, 0.01, 0.05));
    let along_z = meshes.add(Cuboid::new(0.05, 0.01, GROUND_SIZE_M));
    let line = materials.add(Color::srgb(0.38, 0.44, 0.44));
    let axis = materials.add(Color::srgb(0.75, 0.75, 0.70));
    let half_lines = (GROUND_SIZE_M / 2.0 / GRID_SPACING_M) as i32;
    for index in -half_lines..=half_lines {
        let offset = index as f32 * GRID_SPACING_M;
        let material = if index == 0 { &axis } else { &line };
        commands.spawn((
            Mesh3d(along_x.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(0.0, 0.005, offset),
        ));
        commands.spawn((
            Mesh3d(along_z.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(offset, 0.005, 0.0),
        ));
    }
}

/// 운동 모델의 자세를 차량 도형 Transform에 반영한다.
fn sync_vehicle_body(
    motion: Res<VehicleMotion>,
    mut bodies: Query<&mut Transform, With<VehicleBody>>,
) {
    for mut transform in &mut bodies {
        *transform = body_transform(&motion.0);
    }
}

/// 카메라를 차량 뒤쪽 위 목표 위치로 부드럽게 옮기고 차량을 바라보게 한다.
fn follow_vehicle(
    time: Res<Time>,
    motion: Res<VehicleMotion>,
    mut cameras: Query<&mut Transform, With<FollowCamera>>,
) {
    let target = camera_transform(&motion.0);
    let blend = 1.0 - (-CAMERA_FOLLOW_RATE * time.delta_secs()).exp();
    for mut transform in &mut cameras {
        transform.translation = transform.translation.lerp(target.translation, blend);
        transform.rotation = transform.rotation.slerp(target.rotation, blend);
    }
}

/// 자세를 후륜축 중심 기준 Transform으로 바꾼다.
pub fn body_transform(motion: &Motion) -> Transform {
    Transform::from_xyz(motion.x_m, 0.0, motion.z_m)
        .with_rotation(Quat::from_rotation_y(motion.heading_rad))
}

/// 자세에 대한 카메라 목표 Transform(차량 뒤쪽 위에서 차체 중심을 바라봄)을 만든다.
pub fn camera_transform(motion: &Motion) -> Transform {
    let body = body_transform(motion);
    let center = body.transform_point(Vec3::new(0.0, 1.0, -REAR_AXLE_TO_CENTER_M));
    Transform::from_translation(body.transform_point(CAMERA_OFFSET_M)).looking_at(center, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 차량 도형의 전방(-Z)이 운동 모델의 전방 벡터와 같은 방향인지 확인한다.
    #[test]
    fn body_forward_matches_motion_forward() {
        let motion = Motion {
            x_m: 3.0,
            z_m: -2.0,
            heading_rad: 0.7,
            ..Motion::default()
        };
        let forward = body_transform(&motion).forward();
        let (x, z) = motion.forward();
        assert!((forward.x - x).abs() < 1e-5 && (forward.z - z).abs() < 1e-5);
        assert_eq!(
            body_transform(&motion).translation,
            Vec3::new(3.0, 0.0, -2.0)
        );
    }

    /// 카메라 목표가 차량 뒤쪽 위에 있는지 확인한다.
    #[test]
    fn camera_sits_behind_and_above_vehicle() {
        let camera = camera_transform(&Motion::default());
        assert!(camera.translation.z > 0.0);
        assert!(camera.translation.y > BODY_SIZE_M.y);
    }
}
