//! 주행 코스(Phase 3-2/3-4): 선택한 코스 정의와 그 노면·표시선·충돌 경계 메시 생성.
//!
//! 지도 코스([`map`])가 없거나 읽지 못해도 [`course`]의 개발 코스만으로 조작·카메라를 검증할 수 있다.

pub mod course;
pub mod map;

use std::path::PathBuf;

use bevy::prelude::*;

pub use course::{CourseDef, CourseId};

use crate::vehicle::{GroundRect, Obstacle};
use course::{ColliderKind, MarkingKind};

/// 노면 표시 높이(m). 바닥 격자선(높이 0.01 m)을 덮는다.
const SURFACE_Y_M: f32 = 0.012;
/// 표시선 높이(m). 노면보다 조금 위다.
const MARKING_Y_M: f32 = 0.016;
/// 노면·표시선 두께(m).
const PAINT_THICKNESS_M: f32 = 0.004;

/// 현재 실행 중인 코스. 시작 자세와 리셋 자세를 이 정의에서 읽는다.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ActiveCourse(pub CourseDef);

impl Default for ActiveCourse {
    /// 기본 훈련 코스를 선택한다.
    fn default() -> Self {
        Self(CourseId::default().build())
    }
}

/// 명령줄에서 고른 코스.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CourseSelection {
    /// 내장 개발 코스.
    Builtin(CourseId),
    /// 지도 코스 선택 파일(TOML) 경로.
    Map(PathBuf),
}

impl Default for CourseSelection {
    /// 기본 훈련 코스를 고른다.
    fn default() -> Self {
        Self::Builtin(CourseId::default())
    }
}

impl CourseSelection {
    /// `training`, `calibration`, `map:<선택 파일 경로>`를 읽는다. 모르는 값이면 `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().strip_prefix("map:") {
            Some(path) if !path.trim().is_empty() => Some(Self::Map(PathBuf::from(path.trim()))),
            Some(_) => None,
            None => CourseId::parse(text).map(Self::Builtin),
        }
    }

    /// 코스 정의를 만든다. 지도 코스를 읽지 못하면 이유와 함께 기본 훈련 코스를 돌려준다.
    pub fn resolve(&self) -> (CourseDef, Option<String>) {
        match self {
            Self::Builtin(id) => (id.build(), None),
            Self::Map(path) => match map::load_map_course(path) {
                Ok(course) => (course, None),
                Err(error) => (CourseId::default().build(), Some(error)),
            },
        }
    }
}

/// 코스 안내 패널 표시용 마커.
#[derive(Component)]
pub struct CourseNotice;

/// 코스 안내(지도 출처, 임시 폭 등)가 있으면 화면 오른쪽 아래에 계속 보이게 띄운다.
pub fn spawn_course_notice(commands: &mut Commands, course: &CourseDef) {
    if course.notices.is_empty() {
        return;
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            right: Val::Px(12.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Text::new(course.notices.join("\n")),
        TextFont::from_font_size(14.0),
        TextColor(Color::WHITE),
        CourseNotice,
    ));
}

/// 지면 직사각형 `rect`를 바닥 높이 `bottom_y`부터 `height` 높이로 세운 상자의 (Transform, 크기)를 만든다.
///
/// 상자 바닥면은 `rect`와 정확히 같아서 화면 메시와 충돌 판정이 어긋나지 않는다.
pub fn box_on_rect(rect: &GroundRect, bottom_y: f32, height: f32) -> (Transform, Vec3) {
    let transform = Transform::from_xyz(rect.center.x, bottom_y + height / 2.0, rect.center.y)
        .with_rotation(Quat::from_rotation_y(rect.heading_rad));
    let size = Vec3::new(rect.half_size.x * 2.0, height, rect.half_size.y * 2.0);
    (transform, size)
}

/// 코스의 노면·표시선(시각 전용)과 충돌 경계(메시 + [`Obstacle`]), 안내 패널을 만든다.
pub fn spawn_course(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    course: &CourseDef,
) {
    let road = materials.add(Color::srgb(0.20, 0.21, 0.22));
    for rect in &course.surfaces {
        let (transform, size) =
            box_on_rect(rect, SURFACE_Y_M - PAINT_THICKNESS_M, PAINT_THICKNESS_M);
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(road.clone()),
            transform,
        ));
    }
    let edge = materials.add(Color::srgb(0.92, 0.92, 0.88));
    let stop = materials.add(Color::srgb(0.98, 0.98, 0.98));
    for marking in &course.markings {
        let material = match marking.kind {
            MarkingKind::EdgeLine => edge.clone(),
            MarkingKind::StopLine => stop.clone(),
        };
        let (transform, size) = box_on_rect(
            &marking.rect,
            MARKING_Y_M - PAINT_THICKNESS_M,
            PAINT_THICKNESS_M,
        );
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(material),
            transform,
        ));
    }
    let curb = materials.add(Color::srgb(0.70, 0.70, 0.66));
    let wall = materials.add(Color::srgb(0.55, 0.50, 0.46));
    let barrier = materials.add(Color::srgb(0.95, 0.75, 0.15));
    let obstacle = materials.add(Color::srgb(0.85, 0.35, 0.25));
    for collider in &course.colliders {
        let material = match collider.kind {
            ColliderKind::Curb => curb.clone(),
            ColliderKind::Wall => wall.clone(),
            ColliderKind::Barrier => barrier.clone(),
            ColliderKind::Obstacle => obstacle.clone(),
        };
        let (transform, size) = box_on_rect(&collider.rect, 0.0, collider.height_m);
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(material),
            transform,
            Obstacle(collider.rect),
        ));
    }
    info!(
        "course `{}` ready: {} colliders, {} markings",
        course.name,
        course.colliders.len(),
        course.markings.len()
    );
    // 로그는 보조 기록이다. 출처 표기는 화면 안내 패널이 맡는다.
    for notice in &course.notices {
        info!("course notice: {notice}");
    }
    spawn_course_notice(commands, course);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 코스 선택 문자열을 읽고, 지도 코스를 읽지 못하면 이유와 함께 훈련 코스로 대체하는지 확인한다.
    #[test]
    fn course_selection_parses_and_falls_back() {
        assert_eq!(
            CourseSelection::parse("calibration"),
            Some(CourseSelection::Builtin(CourseId::Calibration))
        );
        assert_eq!(
            CourseSelection::parse("map:assets/maps/x/course.toml"),
            Some(CourseSelection::Map(PathBuf::from(
                "assets/maps/x/course.toml"
            )))
        );
        assert_eq!(CourseSelection::parse("map:"), None);
        assert_eq!(CourseSelection::parse("yeosu"), None);

        let (course, error) = CourseSelection::Map(PathBuf::from("does/not/exist.toml")).resolve();
        assert!(error.is_some());
        assert_eq!(course, CourseId::Training.build());
        let (course, error) = CourseSelection::default().resolve();
        assert!(error.is_none());
        assert_eq!(course.name, "training");
    }

    /// 상자 바닥면의 네 모서리가 충돌 영역의 네 모서리와 같은지 확인한다(회전한 영역 포함).
    #[test]
    fn box_footprint_matches_collider() {
        let rect = GroundRect {
            center: Vec2::new(3.0, -7.0),
            half_size: Vec2::new(0.5, 2.0),
            heading_rad: 0.6,
        };
        let (transform, size) = box_on_rect(&rect, 0.0, 1.0);
        assert!((transform.translation.y - 0.5).abs() < 1e-6);
        // 직사각형 축: 오른쪽 (cos, -sin), 전방 (-sin, -cos) — collision 모듈과 같은 규칙.
        let (sin, cos) = rect.heading_rad.sin_cos();
        let (right, forward) = (Vec2::new(cos, -sin), Vec2::new(-sin, -cos));
        for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let local = Vec3::new(sx * size.x / 2.0, -size.y / 2.0, -sz * size.z / 2.0);
            let world = transform.transform_point(local);
            let want =
                rect.center + right * sx * rect.half_size.x + forward * sz * rect.half_size.y;
            assert!(
                (Vec2::new(world.x, world.z) - want).length() < 1e-5,
                "corner {world:?} != {want:?}"
            );
            assert!(world.y.abs() < 1e-6);
        }
    }

    /// 코스를 생성하면 충돌 경계마다 같은 영역의 [`Obstacle`]이 하나씩 생기는지 확인한다.
    #[test]
    fn spawned_obstacles_match_course_colliders() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>();
        let course = CourseId::Training.build();
        let spawn = course.clone();
        app.add_systems(
            Update,
            move |mut commands: Commands,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<StandardMaterial>>| {
                spawn_course(&mut commands, &mut meshes, &mut materials, &spawn);
            },
        );
        app.update();
        let world = app.world_mut();
        let spawned: Vec<GroundRect> = world
            .query::<&Obstacle>()
            .iter(world)
            .map(|obstacle| obstacle.0)
            .collect();
        assert_eq!(spawned.len(), course.colliders.len());
        for rect in course.collision_rects() {
            assert!(spawned.contains(&rect), "{rect:?} missing");
        }
    }
}
