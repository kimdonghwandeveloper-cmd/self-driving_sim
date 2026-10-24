//! 상태 머신과 Bevy 앱을 연결하는 차량 컨트롤러.

use bevy::prelude::*;

use super::state::{EngineState, Gear, TransitionOutcome, VehicleCommand, VehicleState};
use crate::SimSet;
use crate::input::{ControlRequest, DriverInput};

/// 앱에서 사용하는 차량 상태 리소스.
#[derive(Resource, Debug, Clone, Default)]
pub struct Vehicle(pub VehicleState);

/// 마지막 명령 처리 결과. HUD에 표시한다.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlFeedback {
    pub last: Option<Feedback>,
}

/// 명령 하나의 처리 결과와 표시 문장.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub accepted: bool,
    pub message: String,
}

/// 차량 리소스와 명령 처리 시스템을 등록하는 플러그인.
pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    /// 차량 관련 리소스와 시스템을 앱에 등록한다. 이미 넣은 `Vehicle`은 유지한다.
    fn build(&self, app: &mut App) {
        app.init_resource::<Vehicle>()
            .init_resource::<ControlFeedback>()
            .add_systems(Update, apply_controls.in_set(SimSet::Control));
    }
}

/// 연속 입력을 상태에 반영하고, 받은 요청을 순서대로 처리해 결과를 로그와 HUD에 남긴다.
pub fn apply_controls(
    mut vehicle: ResMut<Vehicle>,
    driver: Res<DriverInput>,
    mut requests: MessageReader<ControlRequest>,
    mut feedback: ResMut<ControlFeedback>,
) {
    vehicle.0.set_input(driver.0);
    for request in requests.read() {
        let result = handle_request(&mut vehicle.0, request);
        if result.accepted {
            info!("control: {}", result.message);
        } else {
            warn!("control rejected: {}", result.message);
        }
        feedback.last = Some(result);
    }
}

/// 요청 하나를 상태 머신에 적용하고 처리 결과를 만든다.
pub fn handle_request(state: &mut VehicleState, request: &ControlRequest) -> Feedback {
    match request {
        ControlRequest::Command(command) => match state.apply(*command) {
            Ok(outcome) => Feedback {
                accepted: true,
                message: format!("{}: {}", command_label(*command), outcome_label(outcome)),
            },
            Err(error) => Feedback {
                accepted: false,
                message: format!("{}: {error}", command_label(*command)),
            },
        },
        ControlRequest::GearConflict(gears) => {
            let names: Vec<_> = gears.iter().map(|gear| gear_label(*gear)).collect();
            Feedback {
                accepted: false,
                message: format!(
                    "gear request ignored: simultaneous gear keys ({})",
                    names.join("+")
                ),
            }
        }
    }
}

/// 명령의 표시 이름을 반환한다.
pub fn command_label(command: VehicleCommand) -> String {
    match command {
        VehicleCommand::ToggleEngine => "engine toggle".to_owned(),
        VehicleCommand::RequestGear(gear) => format!("gear {}", gear_label(gear)),
        VehicleCommand::EngageEmergencyStop => "emergency stop".to_owned(),
        VehicleCommand::ReleaseEmergencyStop => "emergency release".to_owned(),
    }
}

/// 전이 결과의 표시 문장을 반환한다.
fn outcome_label(outcome: TransitionOutcome) -> String {
    match outcome {
        TransitionOutcome::EngineChanged(engine) => format!("engine -> {}", engine_label(engine)),
        TransitionOutcome::GearChanged { from, to } => {
            format!("{} -> {}", gear_label(from), gear_label(to))
        }
        TransitionOutcome::Unchanged => "no change".to_owned(),
    }
}

/// 기어의 한 글자 표시(P/R/N/D)를 반환한다.
pub fn gear_label(gear: Gear) -> &'static str {
    match gear {
        Gear::Park => "P",
        Gear::Reverse => "R",
        Gear::Neutral => "N",
        Gear::Drive => "D",
    }
}

/// 시동 상태의 표시 이름을 반환한다.
pub fn engine_label(engine: EngineState) -> &'static str {
    match engine {
        EngineState::Off => "OFF",
        EngineState::Ready => "READY",
        EngineState::EmergencyStop => "EMERGENCY STOP",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_command_reports_transition() {
        let mut state = VehicleState::default();
        let result = handle_request(
            &mut state,
            &ControlRequest::Command(VehicleCommand::ToggleEngine),
        );
        assert!(result.accepted);
        assert_eq!(result.message, "engine toggle: engine -> READY");
        assert_eq!(state.engine(), EngineState::Ready);
    }

    #[test]
    fn rejected_command_reports_reason() {
        let mut state = VehicleState::default();
        let result = handle_request(
            &mut state,
            &ControlRequest::Command(VehicleCommand::RequestGear(Gear::Drive)),
        );
        assert!(!result.accepted);
        assert_eq!(result.message, "gear D: engine is off");
    }

    #[test]
    fn gear_conflict_is_rejected_without_state_change() {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        let before = state.clone();
        let result = handle_request(
            &mut state,
            &ControlRequest::GearConflict(vec![Gear::Reverse, Gear::Drive]),
        );
        assert!(!result.accepted);
        assert_eq!(
            result.message,
            "gear request ignored: simultaneous gear keys (R+D)"
        );
        assert_eq!(state, before);
    }
}
