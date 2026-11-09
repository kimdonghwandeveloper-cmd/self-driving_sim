//! 입석 조작자 시점의 방향·설정·차량 자세→카메라 변환(순수 함수).
//!
//! 차량 로컬 좌표: 원점은 후륜축 중심, +X 오른쪽, +Y 위, -Z 전방.
//! 기본 구도는 사용자 지정 기준 사진 `Car_photo/KakaoTalk_261030_131310_61.jpg`에 맞춘다.
//! 눈 위치·pitch·FOV는 사진 구도에 맞춘 **보정용 임시값**이며 실측값이 아니다.
//! 방향·설정 구조와 시점 키 판정은 Godot 판과 함께 쓰도록 `sim_core::camera`에 있다.

use bevy::prelude::*;

use crate::scene::body_transform;
use crate::vehicle::Motion;

pub use sim_core::camera::{
    FOV_RANGE_DEG, NEAR_CLIP_RANGE_M, OperatorView, PITCH_RANGE_DEG, ViewDirection,
    resolve_view_selection,
};

/// 차량 로컬 좌표에서의 카메라 자세(눈 위치 + 방향 yaw + 공통 pitch)를 만든다.
pub fn local_camera_transform(view: &OperatorView, direction: ViewDirection) -> Transform {
    Transform::from_translation(view.eye()).with_rotation(
        Quat::from_rotation_y(direction.yaw_rad())
            * Quat::from_rotation_x(view.pitch_deg.to_radians()),
    )
}

/// 차량 자세에 붙은 입석 조작자 카메라의 월드 Transform을 만든다. 보간 없이 차체 회전을 즉시 따른다.
pub fn operator_camera_transform(
    motion: &Motion,
    view: &OperatorView,
    direction: ViewDirection,
) -> Transform {
    body_transform(motion).mul_transform(local_camera_transform(view, direction))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 두 벡터가 오차 안에서 같은지 확인한다.
    fn assert_close(got: Vec3, want: Vec3) {
        assert!((got - want).length() < 1e-5, "got {got:?}, want {want:?}");
    }

    /// pitch 0인 설정을 만든다. 방향 검사에서 수평 성분만 보기 위해 쓴다.
    fn level_view() -> OperatorView {
        OperatorView {
            pitch_deg: 0.0,
            ..OperatorView::default()
        }
    }

    /// heading 0에서 네 방향 시선이 -Z/+Z/-X/+X이고 눈 위치가 같은지 확인한다.
    #[test]
    fn four_directions_at_zero_heading() {
        let view = level_view();
        let motion = Motion::default();
        for (direction, want) in [
            (ViewDirection::Front, Vec3::NEG_Z),
            (ViewDirection::Rear, Vec3::Z),
            (ViewDirection::Left, Vec3::NEG_X),
            (ViewDirection::Right, Vec3::X),
        ] {
            let camera = operator_camera_transform(&motion, &view, direction);
            assert_close(*camera.forward(), want);
            assert_close(camera.translation, view.eye());
        }
    }

    /// 회전·이동한 차량에서도 네 방향이 차량 기준으로 유지되고 눈 위치가 하나인지 확인한다.
    #[test]
    fn four_directions_follow_rotated_vehicle() {
        let view = level_view();
        let motion = Motion {
            x_m: 5.0,
            z_m: -3.0,
            heading_rad: 0.9,
            ..Motion::default()
        };
        let body = body_transform(&motion);
        let eye = body.transform_point(view.eye());
        for (direction, local) in [
            (ViewDirection::Front, Vec3::NEG_Z),
            (ViewDirection::Rear, Vec3::Z),
            (ViewDirection::Left, Vec3::NEG_X),
            (ViewDirection::Right, Vec3::X),
        ] {
            let camera = operator_camera_transform(&motion, &view, direction);
            assert_close(camera.translation, eye);
            assert_close(*camera.forward(), body.rotation * local);
        }
        // 전방 시선은 운동 모델의 전방 벡터와 같다.
        let front = operator_camera_transform(&motion, &view, ViewDirection::Front);
        let (x, z) = motion.forward();
        assert_close(*front.forward(), Vec3::new(x, 0.0, z));
    }

    /// 음수 pitch는 네 방향 모두 같은 각도로 아래를 보게 하는지 확인한다.
    #[test]
    fn pitch_is_shared_by_all_directions() {
        let view = OperatorView {
            pitch_deg: -10.0,
            ..OperatorView::default()
        };
        for direction in ViewDirection::ALL {
            let camera = operator_camera_transform(&Motion::default(), &view, direction);
            let down = (-camera.forward().y).asin().to_degrees();
            assert!((down - 10.0).abs() < 1e-3, "{direction:?}: {down}");
        }
    }
}
