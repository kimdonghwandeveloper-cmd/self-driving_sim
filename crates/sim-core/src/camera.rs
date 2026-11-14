//! 입석 조작자 시점의 방향·설정(엔진 무관, Phase GD-2).
//!
//! 차량 로컬 좌표: 원점은 후륜축 중심, +X 오른쪽, +Y 위, -Z 전방.
//! 눈 위치·pitch·FOV는 사진 구도에 맞춘 **보정용 임시값**이며 실측값이 아니다.
//! 카메라 자세 계산(Transform)은 각 엔진 쪽에 있다.

use glam::Vec3;
use serde::Deserialize;

/// 수직 FOV 허용 범위(deg).
pub const FOV_RANGE_DEG: std::ops::RangeInclusive<f32> = 30.0..=110.0;
/// near clip 허용 범위(m).
pub const NEAR_CLIP_RANGE_M: std::ops::RangeInclusive<f32> = 0.01..=0.5;
/// 시선 pitch 허용 범위(deg). 음수는 아래를 본다.
pub const PITCH_RANGE_DEG: std::ops::RangeInclusive<f32> = -45.0..=45.0;
/// 차체 기울기를 카메라가 따라가는 정도 허용 범위(0 = 끔, 1 = 차체와 똑같이).
pub const BODY_FOLLOW_RANGE: std::ops::RangeInclusive<f32> = 0.0..=1.0;

/// 차량 기준 시선 방향. 같은 눈 위치에서 yaw만 바꾼다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ViewDirection {
    #[default]
    Front,
    Rear,
    Left,
    Right,
}

impl ViewDirection {
    /// 입력 처리·검증에 쓰는 전체 방향 목록.
    pub const ALL: [ViewDirection; 4] = [
        ViewDirection::Front,
        ViewDirection::Rear,
        ViewDirection::Left,
        ViewDirection::Right,
    ];

    /// 차량 전방(-Z)에서 +Y축으로 돌리는 각도(rad)를 반환한다. 좌측(-X)이 +90°다.
    pub fn yaw_rad(self) -> f32 {
        match self {
            ViewDirection::Front => 0.0,
            ViewDirection::Left => std::f32::consts::FRAC_PI_2,
            ViewDirection::Rear => std::f32::consts::PI,
            ViewDirection::Right => -std::f32::consts::FRAC_PI_2,
        }
    }

    /// HUD에 표시할 방향 이름을 반환한다.
    pub fn label(self) -> &'static str {
        match self {
            ViewDirection::Front => "FRONT",
            ViewDirection::Rear => "REAR",
            ViewDirection::Left => "LEFT",
            ViewDirection::Right => "RIGHT",
        }
    }
}

/// `config/camera.toml`의 `[operator_view]` 구역. 입석 조작자 1인칭 카메라 설정.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperatorView {
    /// 차량 로컬 좌표의 눈 위치(m; 오른쪽, 위, 뒤). 원점은 후륜축 중심.
    pub eye_m: [f32; 3],
    /// 시선 pitch(deg). 음수면 아래를 본다. 네 방향 모두 같은 값을 쓴다.
    pub pitch_deg: f32,
    /// 수직 시야각(deg). 가로 시야각은 창 화면비에 따라 정해진다.
    pub vertical_fov_deg: f32,
    /// near clip 거리(m).
    pub near_clip_m: f32,
    /// 차체 기울기·상하 움직임(2-8)을 카메라가 따라가는 정도(0..=1). 0이면 카메라는 기울지 않는다(멀미 대비).
    pub body_follow: f32,
}

impl Default for OperatorView {
    /// `61.jpg` 구도에 맞춘 임시 기본값을 만든다(실측값 아님).
    fn default() -> Self {
        Self {
            eye_m: [0.0, 1.65, -2.75],
            pitch_deg: -3.0,
            vertical_fov_deg: 62.0,
            near_clip_m: 0.05,
            body_follow: 0.5,
        }
    }
}

impl OperatorView {
    /// 눈 위치 유한값, FOV·near clip·pitch 범위를 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        if !self.eye_m.iter().all(|value| value.is_finite()) {
            return Err(format!(
                "operator_view.eye_m must be finite, got {:?}",
                self.eye_m
            ));
        }
        /// 값이 범위 안인지 확인하고, 아니면 필드 이름이 들어간 오류를 만든다.
        fn check(
            name: &str,
            value: f32,
            range: &std::ops::RangeInclusive<f32>,
        ) -> Result<(), String> {
            if range.contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "operator_view.{name} must be within {}..={}, got {value}",
                    range.start(),
                    range.end()
                ))
            }
        }
        check("vertical_fov_deg", self.vertical_fov_deg, &FOV_RANGE_DEG)?;
        check("near_clip_m", self.near_clip_m, &NEAR_CLIP_RANGE_M)?;
        check("pitch_deg", self.pitch_deg, &PITCH_RANGE_DEG)?;
        check("body_follow", self.body_follow, &BODY_FOLLOW_RANGE)
    }

    /// 눈 위치를 `Vec3`로 반환한다.
    pub fn eye(&self) -> Vec3 {
        Vec3::from_array(self.eye_m)
    }
}

/// 새로 눌린 시점 키와 눌려 있는 시점 키로 다음 방향을 정한다.
///
/// 새로 눌린 키가 없으면 지금 방향을 유지한다. 서로 다른 방향이 둘 이상 눌려 있으면
/// 동시 입력으로 보고 지금 방향을 유지한다.
pub fn resolve_view_selection(
    current: ViewDirection,
    just_pressed: &[ViewDirection],
    held: &[ViewDirection],
) -> ViewDirection {
    if just_pressed.is_empty() {
        return current;
    }
    let active: Vec<ViewDirection> = ViewDirection::ALL
        .into_iter()
        .filter(|direction| just_pressed.contains(direction) || held.contains(direction))
        .collect();
    match active.as_slice() {
        [direction] => *direction,
        _ => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 설정이 검증을 통과하는지 확인한다.
    #[test]
    fn default_view_is_valid() {
        assert_eq!(OperatorView::default().validate(), Ok(()));
    }

    /// NaN·무한대 눈 위치와 범위를 벗어난 FOV·near clip·pitch·차체 따라가기를 거부하는지 확인한다.
    #[test]
    fn invalid_view_values_are_rejected() {
        let base = OperatorView::default();
        let cases = [
            OperatorView {
                eye_m: [f32::NAN, 1.0, 0.0],
                ..base.clone()
            },
            OperatorView {
                eye_m: [0.0, f32::INFINITY, 0.0],
                ..base.clone()
            },
            OperatorView {
                vertical_fov_deg: 20.0,
                ..base.clone()
            },
            OperatorView {
                vertical_fov_deg: f32::NAN,
                ..base.clone()
            },
            OperatorView {
                near_clip_m: 0.6,
                ..base.clone()
            },
            OperatorView {
                pitch_deg: -60.0,
                ..base.clone()
            },
            OperatorView {
                body_follow: 1.5,
                ..base.clone()
            },
        ];
        for case in cases {
            assert!(case.validate().is_err(), "{case:?}");
        }
    }

    /// 한 방향 키만 새로 누르면 그 방향으로 바뀌는지 확인한다.
    #[test]
    fn single_press_selects_direction() {
        assert_eq!(
            resolve_view_selection(
                ViewDirection::Front,
                &[ViewDirection::Rear],
                &[ViewDirection::Rear]
            ),
            ViewDirection::Rear
        );
    }

    /// 누르고만 있거나 아무것도 누르지 않으면 지금 방향을 유지하는지 확인한다.
    #[test]
    fn held_or_idle_keeps_direction() {
        assert_eq!(
            resolve_view_selection(ViewDirection::Left, &[], &[ViewDirection::Rear]),
            ViewDirection::Left
        );
        assert_eq!(
            resolve_view_selection(ViewDirection::Left, &[], &[]),
            ViewDirection::Left
        );
    }

    /// 여러 방향을 동시에 누르거나 다른 방향을 누른 채 새로 누르면 지금 방향을 유지하는지 확인한다.
    #[test]
    fn simultaneous_presses_keep_direction() {
        assert_eq!(
            resolve_view_selection(
                ViewDirection::Front,
                &[ViewDirection::Left, ViewDirection::Right],
                &[]
            ),
            ViewDirection::Front
        );
        assert_eq!(
            resolve_view_selection(
                ViewDirection::Front,
                &[ViewDirection::Rear],
                &[ViewDirection::Rear, ViewDirection::Left]
            ),
            ViewDirection::Front
        );
    }

    /// 같은 방향을 반복해서 눌러도 그 방향에 머무는지 확인한다.
    #[test]
    fn repeated_press_is_idempotent() {
        let mut current = ViewDirection::Front;
        for _ in 0..3 {
            current = resolve_view_selection(current, &[ViewDirection::Right], &[]);
        }
        assert_eq!(current, ViewDirection::Right);
    }
}
