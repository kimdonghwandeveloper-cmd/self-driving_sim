//! 개발용 3D 장면: 격자 바닥, 선택한 코스([`crate::world`]), 움직이는 차량 도형(외관 + 최소 실내).
//!
//! 외관 차체는 [`EXTERIOR_LAYER`]에 두어 입석 조작자 시점에서 보이지 않게 하고,
//! 기준 시야용 최소 실내 형상은 기본 레이어에 둔다. 카메라는 [`crate::camera`]가 만든다.
//! 차체 크기, 코스 배치, 외부 디버그 카메라 위치는 모두 **시뮬레이터 임시값**이다.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::SimSet;
use crate::camera::EXTERIOR_LAYER;
use crate::camera::interior::{BODY_HEIGHT_M, spawn_interior};
use crate::vehicle::{Motion, VEHICLE_FOOTPRINT, VehicleMotion};
use crate::world::{ActiveCourse, CourseDef, spawn_course};

/// 바닥 한 변의 최소 길이(m). 코스가 크면 [`ground_square`]가 더 넓힌다.
const GROUND_SIZE_M: f32 = 200.0;
/// 코스 둘레에 더 까는 바닥 여유(m). 1인칭 시야에서 바닥 끝이 가까이 보이지 않게 한다.
const GROUND_MARGIN_M: f32 = 100.0;
/// 격자선 간격(m). 이동 거리를 눈으로 가늠하는 기준이다.
const GRID_SPACING_M: f32 = 5.0;
/// 차체 크기(폭, 높이, 길이; m). 폭·길이는 충돌 박스와 같다. 실차 치수 미확인 임시값.
const BODY_SIZE_M: Vec3 = Vec3::new(
    VEHICLE_FOOTPRINT.width_m,
    BODY_HEIGHT_M,
    VEHICLE_FOOTPRINT.length_m,
);
/// 후륜축 중심에서 차체 중심까지의 전방 거리(m).
const REAR_AXLE_TO_CENTER_M: f32 = VEHICLE_FOOTPRINT.rear_axle_to_center_m;
/// 보조광 조도(lux). 주광(기본값)보다 훨씬 약하게 둔다.
const FILL_LIGHT_LUX: f32 = 1500.0;
/// 개발용 외부 디버그 카메라의 차량 기준 위치(오른쪽, 위, 뒤; m).
const EXTERNAL_CAMERA_OFFSET_M: Vec3 = Vec3::new(0.0, 5.0, 11.0);

/// 차량 도형 루트 엔티티 표시용 마커. Transform 원점은 후륜축 중심이다.
#[derive(Component)]
pub struct VehicleBody;

/// 장면 생성과 차량 도형 동기화 시스템을 등록하는 플러그인.
pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    /// 장면 생성(시작 시)과 매 프레임 차량 도형 갱신 시스템을 등록한다.
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene.run_if(crate::driving_active))
            .add_systems(crate::entering_driving_from_picker(), setup_scene)
            .add_systems(Update, sync_vehicle_body.in_set(SimSet::Display));
    }
}

/// 바닥, 격자선, 선택한 코스, 차량 도형(외관·실내), 조명을 배치한다.
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    course: Res<ActiveCourse>,
) {
    let (ground_center, ground_size) = ground_square(&course.0);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(ground_size, ground_size))),
        MeshMaterial3d(materials.add(Color::srgb(0.25, 0.30, 0.30))),
        Transform::from_xyz(ground_center.x, 0.0, ground_center.y),
    ));
    spawn_grid(
        &mut commands,
        &mut meshes,
        &mut materials,
        ground_center,
        ground_size,
    );
    spawn_course(&mut commands, &mut meshes, &mut materials, &course.0);

    let body = meshes.add(Cuboid::from_size(BODY_SIZE_M));
    let front_marker = meshes.add(Cuboid::new(BODY_SIZE_M.x * 0.8, 0.3, 0.3));
    commands
        .spawn((Transform::default(), Visibility::default(), VehicleBody))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(body),
                MeshMaterial3d(materials.add(Color::srgb(0.30, 0.65, 0.85))),
                Transform::from_xyz(0.0, BODY_SIZE_M.y / 2.0, -REAR_AXLE_TO_CENTER_M),
                RenderLayers::layer(EXTERIOR_LAYER),
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
                RenderLayers::layer(EXTERIOR_LAYER),
            ));
            spawn_interior(parent, &mut meshes, &mut materials);
        });

    // 1인칭(기본 레이어)과 외부 디버그(외관 레이어) 카메라 모두에서 같은 조명을 쓴다.
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, EXTERIOR_LAYER]),
    ));
    // 반대쪽 약한 보조광: 주광과 반대 방향을 보는 실내 면이 완전히 검게 보이지 않게 한다.
    commands.spawn((
        DirectionalLight {
            illuminance: FILL_LIGHT_LUX,
            ..default()
        },
        Transform::from_xyz(-4.0, 6.0, -3.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, EXTERIOR_LAYER]),
    ));
    info!("Development scene ready. Close the window to exit.");
}

/// 바닥 위에 일정 간격의 격자선을 깐다. 원점을 지나는 두 선은 밝게 표시한다.
///
/// `center`(x, z)는 격자 간격의 정수배여야 원점 축선과 격자가 맞는다.
fn spawn_grid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    center: Vec2,
    size: f32,
) {
    let along_x = meshes.add(Cuboid::new(size, 0.01, 0.05));
    let along_z = meshes.add(Cuboid::new(0.05, 0.01, size));
    let line = materials.add(Color::srgb(0.38, 0.44, 0.44));
    let axis = materials.add(Color::srgb(0.75, 0.75, 0.70));
    let half_lines = (size / 2.0 / GRID_SPACING_M) as i32;
    let (base_x, base_z) = (
        (center.x / GRID_SPACING_M).round() as i32,
        (center.y / GRID_SPACING_M).round() as i32,
    );
    for index in -half_lines..=half_lines {
        let (ix, iz) = (base_x + index, base_z + index);
        let z_material = if iz == 0 { &axis } else { &line };
        let x_material = if ix == 0 { &axis } else { &line };
        commands.spawn((
            Mesh3d(along_x.clone()),
            MeshMaterial3d(z_material.clone()),
            Transform::from_xyz(center.x, 0.005, iz as f32 * GRID_SPACING_M),
        ));
        commands.spawn((
            Mesh3d(along_z.clone()),
            MeshMaterial3d(x_material.clone()),
            Transform::from_xyz(ix as f32 * GRID_SPACING_M, 0.005, center.y),
        ));
    }
}

/// 코스 전체와 시작 위치를 덮는 정사각형 바닥의 (중심, 한 변 길이)를 구한다.
///
/// 각 영역은 중심에서 반 크기 대각선까지의 원으로 넉넉히 감싸고, 굽은 길 띠는 경계선 점을 모두 넣는다. 둘레에 [`GROUND_MARGIN_M`]을 더하고,
/// 한 변은 [`GROUND_SIZE_M`] 이상이며 중심은 격자 간격의 정수배로 맞춘다.
pub fn ground_square(course: &CourseDef) -> (Vec2, f32) {
    let start = Vec2::new(course.start.x_m, course.start.z_m);
    let (mut low, mut high) = (start, start);
    for rect in course
        .surfaces
        .iter()
        .chain(course.markings.iter().map(|marking| &marking.rect))
        .chain(course.colliders.iter().map(|collider| &collider.rect))
    {
        let reach = rect.half_size.length();
        low = low.min(rect.center - Vec2::splat(reach));
        high = high.max(rect.center + Vec2::splat(reach));
    }
    for point in course
        .strips
        .iter()
        .flat_map(|strip| strip.left.iter().chain(&strip.right))
        .chain(course.areas.iter().flat_map(|area| area.boundary.iter()))
    {
        low = low.min(*point);
        high = high.max(*point);
    }
    let center = ((low + high) / 2.0 / GRID_SPACING_M).round() * GRID_SPACING_M;
    let half = (high - center).max(center - low).max_element() + GROUND_MARGIN_M;
    let size = (2.0 * half).max(GROUND_SIZE_M);
    // 격자선이 바닥 끝에 맞도록 한 변을 격자 간격 2배의 정수배로 올린다.
    let step = 2.0 * GRID_SPACING_M;
    (center, (size / step).ceil() * step)
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

/// 자세를 후륜축 중심 기준 Transform으로 바꾼다.
pub fn body_transform(motion: &Motion) -> Transform {
    Transform::from_xyz(motion.x_m, 0.0, motion.z_m)
        .with_rotation(Quat::from_rotation_y(motion.heading_rad))
}

/// 개발용 외부 디버그 카메라 Transform(차량 뒤쪽 위에서 차체 중심을 바라봄)을 만든다.
pub fn external_camera_transform(motion: &Motion) -> Transform {
    let body = body_transform(motion);
    let center = body.transform_point(Vec3::new(0.0, 1.0, -REAR_AXLE_TO_CENTER_M));
    Transform::from_translation(body.transform_point(EXTERNAL_CAMERA_OFFSET_M))
        .looking_at(center, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CourseId;

    /// 기본 코스의 바닥이 최소 크기 이상이고, 원점에서 먼 지도 코스도 모든 영역이 바닥 안에 드는지 확인한다.
    #[test]
    fn ground_covers_course() {
        let (center, size) = ground_square(&CourseId::Training.build());
        assert!(
            center.x.abs() < 1e-6 && size >= GROUND_SIZE_M,
            "{center:?} {size}"
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/maps/yeosu-expo/course.toml");
        let course = crate::world::map::load_map_course(&path).unwrap();
        let (center, size) = ground_square(&course);
        assert_eq!(center % GRID_SPACING_M, Vec2::ZERO);
        for rect in course
            .surfaces
            .iter()
            .chain(course.collision_rects().iter())
        {
            let reach = rect.half_size.length();
            let offset = (rect.center - center).abs() + Vec2::splat(reach);
            assert!(
                offset.max_element() <= size / 2.0,
                "{rect:?} outside ground"
            );
        }
    }

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

    /// 외부 디버그 카메라가 차량 뒤쪽 위에 있는지 확인한다.
    #[test]
    fn external_camera_sits_behind_and_above_vehicle() {
        let camera = external_camera_transform(&Motion::default());
        assert!(camera.translation.z > 0.0);
        assert!(camera.translation.y > BODY_SIZE_M.y);
    }
}
