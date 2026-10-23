//! 차량 상태·입력·명령 결과를 보여 주는 개발용 HUD.

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

/// 매 프레임 HUD 텍스트를 현재 상태로 갱신한다.
pub fn update_hud(
    vehicle: Res<Vehicle>,
    feedback: Res<ControlFeedback>,
    bindings: Res<KeyBindings>,
    mut query: Query<&mut Text, With<VehicleHud>>,
) {
    let content = hud_text(&vehicle.0, &feedback, &bindings);
    for mut text in &mut query {
        if text.0 != content {
            text.0.clone_from(&content);
        }
    }
}

/// HUD에 표시할 전체 문자열을 만든다.
pub fn hud_text(
    state: &VehicleState,
    feedback: &ControlFeedback,
    bindings: &KeyBindings,
) -> String {
    let input = state.input();
    let last = match &feedback.last {
        Some(entry) if entry.accepted => format!("[OK] {}", entry.message),
        Some(entry) => format!("[REJECTED] {}", entry.message),
        None => "-".to_owned(),
    };
    let stopped = if state.is_stopped() {
        "stopped"
    } else {
        "moving"
    };
    let drive = if state.drive_permitted() { "yes" } else { "no" };
    let key = |action| bindings.label(action);

    [
        "SIMULATOR ONLY - provisional values, not verified against the real vehicle".to_owned(),
        format!(
            "Engine: {}   Gear: {}",
            engine_label(state.engine()),
            gear_label(state.gear())
        ),
        format!(
            "Speed: {:.2} m/s ({stopped}, no dynamics yet)",
            state.speed_mps()
        ),
        format!(
            "Throttle: {:.2} (effective {:.2})   Brake: {:.2}   Steering: {:+.2}",
            input.throttle,
            state.effective_throttle(),
            input.brake,
            input.steering
        ),
        format!("Drive permitted: {drive}"),
        format!("Last: {last}"),
        String::new(),
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
}
