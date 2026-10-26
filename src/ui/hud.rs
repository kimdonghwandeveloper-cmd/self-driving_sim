//! 차량 상태·입력·명령 결과를 보여 주는 개발용 HUD.

use std::fmt::{self, Write};

use bevy::prelude::*;

use crate::input::KeyBindings;
use crate::input::mapping::KeyAction;
use crate::vehicle::controller::{ControlFeedback, Vehicle, engine_label, gear_label};
use crate::vehicle::{Gear, VehicleState};

/// HUD 텍스트 엔티티 표시용 마커.
#[derive(Component)]
pub struct VehicleHud;

/// 화면 왼쪽 위에 HUD 텍스트를 만든다.
pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Text::new(""),
        TextFont::from_font_size(16.0),
        TextColor(Color::WHITE),
        VehicleHud,
    ));
}

/// `update_hud`가 프레임 사이에 재사용하는 문자열.
#[derive(Default)]
pub struct HudCache {
    /// 키 매핑이 바뀔 때만 다시 만드는 키 안내.
    key_guide: String,
    /// 매 프레임 상태 문자열을 쓰는 버퍼. 할당을 재사용한다.
    buffer: String,
}

/// 매 프레임 HUD 텍스트를 현재 상태로 갱신한다. 내용이 같으면 텍스트를 건드리지 않는다.
pub fn update_hud(
    vehicle: Res<Vehicle>,
    feedback: Res<ControlFeedback>,
    bindings: Res<KeyBindings>,
    mut cache: Local<HudCache>,
    mut query: Query<&mut Text, With<VehicleHud>>,
) {
    let cache = &mut *cache;
    if bindings.is_changed() || cache.key_guide.is_empty() {
        cache.key_guide = key_guide_text(&bindings);
    }
    write_hud(&mut cache.buffer, &vehicle.0, &feedback, &cache.key_guide);
    for mut text in &mut query {
        if text.0 != cache.buffer {
            text.0.clone_from(&cache.buffer);
        }
    }
}

/// HUD에 표시할 전체 문자열을 만든다.
pub fn hud_text(
    state: &VehicleState,
    feedback: &ControlFeedback,
    bindings: &KeyBindings,
) -> String {
    let mut out = String::new();
    write_hud(&mut out, state, feedback, &key_guide_text(bindings));
    out
}

/// `out`을 비우고 상태 줄과 키 안내를 이어 쓴다.
fn write_hud(out: &mut String, state: &VehicleState, feedback: &ControlFeedback, key_guide: &str) {
    out.clear();
    write_status(out, state, feedback).expect("writing to a String cannot fail");
    out.push_str("\n\n");
    out.push_str(key_guide);
}

/// 차량 상태·입력·마지막 명령 결과 줄을 쓴다.
fn write_status(out: &mut String, state: &VehicleState, feedback: &ControlFeedback) -> fmt::Result {
    let input = state.input();
    let motion = if !state.speed_valid() {
        "INVALID"
    } else if state.is_stopped() {
        "stopped"
    } else {
        "moving"
    };
    let drive = if state.drive_permitted() { "yes" } else { "no" };

    writeln!(
        out,
        "SIMULATOR ONLY - provisional values, not verified against the real vehicle"
    )?;
    writeln!(
        out,
        "Engine: {}   Gear: {}",
        engine_label(state.engine()),
        gear_label(state.gear())
    )?;
    writeln!(
        out,
        "Speed: {:.2} m/s ({motion}, no dynamics yet)",
        state.speed_mps()
    )?;
    writeln!(
        out,
        "Throttle: {:.2} (effective {:.2})   Brake: {:.2}   Steering: {:+.2}",
        input.throttle,
        state.effective_throttle(),
        input.brake,
        input.steering
    )?;
    writeln!(out, "Drive permitted: {drive}")?;
    match &feedback.last {
        Some(entry) if entry.accepted => write!(out, "Last: [OK] {}", entry.message),
        Some(entry) => write!(out, "Last: [REJECTED] {}", entry.message),
        None => write!(out, "Last: -"),
    }
}

/// 키 매핑으로 HUD 아래쪽 키 안내 세 줄을 만든다.
pub fn key_guide_text(bindings: &KeyBindings) -> String {
    let key = |action| bindings.label(action);
    [
        format!(
            "Keys  throttle {} | brake {} | steer {} / {}",
            key(KeyAction::Throttle),
            key(KeyAction::Brake),
            key(KeyAction::SteerLeft),
            key(KeyAction::SteerRight)
        ),
        format!(
            "      gear P {} | R {} | N {} | D {}",
            key(KeyAction::Gear(Gear::Park)),
            key(KeyAction::Gear(Gear::Reverse)),
            key(KeyAction::Gear(Gear::Neutral)),
            key(KeyAction::Gear(Gear::Drive))
        ),
        format!(
            "      engine {} | e-stop {} | release {}",
            key(KeyAction::EngineToggle),
            key(KeyAction::EmergencyStop),
            key(KeyAction::EmergencyRelease)
        ),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::controller::Feedback;

    /// HUD 문자열에 상태·마지막 거부 사유·키 안내·임시값 경고가 들어가는지 확인한다.
    #[test]
    fn hud_shows_state_rejection_and_keys() {
        let state = VehicleState::default();
        let feedback = ControlFeedback {
            last: Some(Feedback {
                accepted: false,
                message: "gear D: engine is off".to_owned(),
            }),
        };
        let text = hud_text(&state, &feedback, &KeyBindings::default());
        assert!(text.contains("Engine: OFF   Gear: P"));
        assert!(text.contains("Last: [REJECTED] gear D: engine is off"));
        assert!(text.contains("e-stop Space"));
        assert!(text.contains("not verified"));
    }

    /// 속도가 잘못된 값이면 HUD에 INVALID로 표시하는지 확인한다.
    #[test]
    fn hud_marks_invalid_speed() {
        let mut state = VehicleState::default();
        state.set_speed_mps(f32::NAN);
        let text = hud_text(&state, &ControlFeedback::default(), &KeyBindings::default());
        assert!(text.contains("INVALID"));
    }
}
