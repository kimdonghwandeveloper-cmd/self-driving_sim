//! 코스 형상을 Godot 사전으로 바꾸는 `SimCourse`(인터페이스 계약 3절).
//!
//! 형상 계산은 `sim-core`가 하고, 여기서는 (x, z) 지면 좌표를 `Vector3(x, 0, z)`로 옮겨 담기만 한다.

use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::Vec2;
use godot::prelude::*;
use sim_core::vehicle::GroundRect;
use sim_core::world::course::{ColliderKind, CourseDef, CourseId, MarkingKind, StripKind};

/// 지면 좌표 (x, z)를 높이 0인 Godot 3차원 좌표로 바꾼다.
fn ground(point: Vec2) -> Vector3 {
    Vector3::new(point.x, 0.0, point.y)
}

/// 지면 점 목록을 `PackedVector3Array`로 바꾼다.
fn ground_points(points: &[Vec2]) -> PackedVector3Array {
    points.iter().map(|point| ground(*point)).collect()
}

/// 지면 직사각형을 계약의 `Rect` 사전(중심, 전체 크기, +Y yaw)으로 바꾼다.
fn rect_dict(rect: &GroundRect) -> VarDictionary {
    vdict! {
        "center" => ground(rect.center),
        "size" => Vector2::new(rect.half_size.x * 2.0, rect.half_size.y * 2.0),
        "heading_rad" => rect.heading_rad,
    }
}

/// 띠 종류의 계약 이름을 반환한다.
fn strip_kind(kind: StripKind) -> &'static str {
    match kind {
        StripKind::Road => "road",
        StripKind::EdgeLine => "edge_line",
    }
}

/// 노면 표시 종류의 계약 이름을 반환한다.
fn marking_kind(kind: MarkingKind) -> &'static str {
    match kind {
        MarkingKind::EdgeLine => "edge_line",
        MarkingKind::StopLine => "stop_line",
    }
}

/// 충돌 경계 종류의 계약 이름을 반환한다.
fn collider_kind(kind: ColliderKind) -> &'static str {
    match kind {
        ColliderKind::Curb => "curb",
        ColliderKind::Wall => "wall",
        ColliderKind::Barrier => "barrier",
        ColliderKind::Obstacle => "obstacle",
    }
}

/// 검사를 통과한 코스 정의를 계약 3절의 성공 사전으로 바꾼다. 빈 목록 키도 항상 넣는다.
pub fn course_dict(course: &CourseDef) -> VarDictionary {
    let notices: PackedStringArray = course
        .notices
        .iter()
        .map(|notice| GString::from(notice.as_str()))
        .collect();
    let surfaces: VarArray = course
        .surfaces
        .iter()
        .map(|rect| rect_dict(rect).to_variant())
        .collect();
    let strips: VarArray = course
        .strips
        .iter()
        .map(|strip| {
            vdict! {
                "kind" => strip_kind(strip.kind),
                "left" => &ground_points(&strip.left),
                "right" => &ground_points(&strip.right),
            }
            .to_variant()
        })
        .collect();
    let areas: VarArray = course
        .areas
        .iter()
        .map(|area| {
            vdict! {
                "center" => ground(area.center),
                "boundary" => &ground_points(&area.boundary),
            }
            .to_variant()
        })
        .collect();
    let markings: VarArray = course
        .markings
        .iter()
        .map(|marking| {
            vdict! {
                "kind" => marking_kind(marking.kind),
                "rect" => &rect_dict(&marking.rect),
            }
            .to_variant()
        })
        .collect();
    let colliders: VarArray = course
        .colliders
        .iter()
        .map(|collider| {
            vdict! {
                "kind" => collider_kind(collider.kind),
                "rect" => &rect_dict(&collider.rect),
                "height_m" => collider.height_m,
            }
            .to_variant()
        })
        .collect();
    vdict! {
        "ok" => true,
        "error" => "",
        "name" => course.name.as_str(),
        "notices" => &notices,
        "start" => &vdict! {
            "position" => Vector3::new(course.start.x_m, 0.0, course.start.z_m),
            "heading_rad" => course.start.heading_rad,
        },
        "surfaces" => &surfaces,
        "strips" => &strips,
        "areas" => &areas,
        "markings" => &markings,
        "colliders" => &colliders,
    }
}

/// 계약 3절의 실패 사전(`ok`, `error`만)을 만든다.
pub fn failure_dict(error: &str) -> VarDictionary {
    vdict! { "ok" => false, "error" => error }
}

/// 패닉 내용에서 사람이 읽을 문구를 꺼낸다.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "알 수 없는 패닉".to_owned()
    }
}

/// 코스 이름으로 형상을 만들고 계약 검사를 한다. 실패하면 사람이 읽는 한국어 사유를 돌려준다.
///
/// 패닉도 잡아서 `"내부 오류: ..."` 사유로 바꾼다(계약 6절).
pub fn build_course(name: &str) -> Result<CourseDef, String> {
    let id = CourseId::parse(name).ok_or_else(|| {
        format!("알 수 없는 코스 이름입니다: \"{name}\" (training, calibration 중 하나)")
    })?;
    let course = catch_unwind(AssertUnwindSafe(|| id.build()))
        .map_err(|payload| format!("내부 오류: {}", panic_message(payload.as_ref())))?;
    course.validate().map_err(|error| error.to_string())?;
    Ok(course)
}

/// 차량 없이 코스 형상만 필요할 때 쓰는 정적 함수 모음(메뉴 미리보기 등).
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct SimCourse {}

#[godot_api]
impl SimCourse {
    /// 코스 이름(`"training"`, `"calibration"`)으로 형상 사전을 만든다. 실패하면 `{ok: false, error}`만 돌려준다.
    #[func]
    fn build(name: GString) -> VarDictionary {
        match build_course(&name.to_string()) {
            Ok(course) => course_dict(&course),
            Err(error) => failure_dict(&error),
        }
    }
}
