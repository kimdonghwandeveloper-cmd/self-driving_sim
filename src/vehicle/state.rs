//! 차량 시동·기어·비상 정지 상태 머신.
//!
//! 모든 규칙은 실차 절차가 확인되기 전의 **시뮬레이터 임시 규칙**이다.
//! 금지된 전이는 상태를 바꾸지 않고 [`TransitionError`]로 사유를 반환한다.

use std::fmt;

/// 정지로 간주하는 기본 속도 임계값(m/s). 설정 파일 연동 전 임시값.
pub const DEFAULT_STOP_THRESHOLD_MPS: f32 = 0.05;

/// 시동 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineState {
    #[default]
    Off,
    Ready,
    EmergencyStop,
}

/// 기어 위치.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gear {
    #[default]
    Park,
    Reverse,
    Neutral,
    Drive,
}

impl Gear {
    /// 구동력을 전달하는 기어의 진행 방향 부호를 반환한다(D=+1, R=-1, P/N=None).
    pub fn drive_direction(self) -> Option<f32> {
        match self {
            Gear::Drive => Some(1.0),
            Gear::Reverse => Some(-1.0),
            Gear::Park | Gear::Neutral => None,
        }
    }
}

/// 조작 입력. throttle/brake는 0..=1, steering은 -1(좌)..=1(우).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VehicleInput {
    pub throttle: f32,
    pub brake: f32,
    pub steering: f32,
}

impl VehicleInput {
    /// 각 축을 허용 범위로 자르고 NaN/무한대는 0으로 바꾼 입력을 반환한다.
    pub fn sanitized(self) -> Self {
        Self {
            throttle: sanitize(self.throttle, 0.0, 1.0),
            brake: sanitize(self.brake, 0.0, 1.0),
            steering: sanitize(self.steering, -1.0, 1.0),
        }
    }
}

/// 유한한 값은 범위로 자르고, 유한하지 않은 값은 0으로 처리한다.
fn sanitize(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        0.0
    }
}

/// 상태 머신에 요청하는 조작 명령.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehicleCommand {
    ToggleEngine,
    RequestGear(Gear),
    EngageEmergencyStop,
    ReleaseEmergencyStop,
}

/// 명령이 받아들여졌을 때의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionOutcome {
    EngineChanged(EngineState),
    GearChanged {
        from: Gear,
        to: Gear,
    },
    /// 이미 요청한 상태여서 변화가 없음.
    Unchanged,
}

/// 명령이 거부된 사유. HUD/로그 표시에 사용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    EngineOff,
    EmergencyStopActive,
    NotStopped,
    EmergencyStopNotActive,
}

impl fmt::Display for TransitionError {
    /// 거부 사유를 사람이 읽을 수 있는 문장으로 출력한다.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            TransitionError::EngineOff => "engine is off",
            TransitionError::EmergencyStopActive => "emergency stop is active",
            TransitionError::NotStopped => "vehicle must be fully stopped",
            TransitionError::EmergencyStopNotActive => "emergency stop is not active",
        };
        f.write_str(message)
    }
}

impl std::error::Error for TransitionError {}

/// 차량의 시동·기어·속도·조향·입력 상태.
///
/// 속도는 차량 전방 기준 부호 있는 값(m/s)이며 운동 모델(Phase 2)이 갱신한다.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    engine: EngineState,
    gear: Gear,
    speed_mps: f32,
    steering: f32,
    input: VehicleInput,
    stop_threshold_mps: f32,
}

impl Default for VehicleState {
    /// 시동 OFF, P, 정지 상태의 기본 차량을 만든다.
    fn default() -> Self {
        Self::new(DEFAULT_STOP_THRESHOLD_MPS)
    }
}

impl VehicleState {
    /// 정지 판정 임계값(m/s)을 지정해 시동 OFF, P, 정지 상태로 생성한다.
    pub fn new(stop_threshold_mps: f32) -> Self {
        Self {
            engine: EngineState::Off,
            gear: Gear::Park,
            speed_mps: 0.0,
            steering: 0.0,
            input: VehicleInput::default(),
            stop_threshold_mps: sanitize(stop_threshold_mps, 0.0, f32::MAX),
        }
    }

    /// 현재 시동 상태를 반환한다.
    pub fn engine(&self) -> EngineState {
        self.engine
    }

    /// 현재 기어를 반환한다.
    pub fn gear(&self) -> Gear {
        self.gear
    }

    /// 현재 부호 있는 속도(m/s)를 반환한다.
    pub fn speed_mps(&self) -> f32 {
        self.speed_mps
    }

    /// 현재 조향 값(-1..=1)을 반환한다.
    pub fn steering(&self) -> f32 {
        self.steering
    }

    /// 마지막으로 받은(정규화된) 조작 입력을 반환한다.
    pub fn input(&self) -> VehicleInput {
        self.input
    }

    /// 속도 크기가 정지 임계값 이하인지 확인한다.
    pub fn is_stopped(&self) -> bool {
        self.speed_mps.abs() <= self.stop_threshold_mps
    }

    /// 시동 Ready이고 D/R일 때만 구동 가속을 허용한다.
    pub fn drive_permitted(&self) -> bool {
        self.engine == EngineState::Ready && self.gear.drive_direction().is_some()
    }

    /// 안전 규칙을 적용한 가속 입력을 반환한다. 구동이 금지되면 0이다.
    pub fn effective_throttle(&self) -> f32 {
        if self.drive_permitted() {
            self.input.throttle
        } else {
            0.0
        }
    }

    /// 조작 입력을 정규화해 저장하고 조향 값을 갱신한다.
    pub fn set_input(&mut self, input: VehicleInput) {
        self.input = input.sanitized();
        self.steering = self.input.steering;
    }

    /// 운동 모델이 계산한 속도(m/s)를 반영한다. 유한하지 않은 값은 0으로 처리한다.
    pub fn set_speed_mps(&mut self, speed_mps: f32) {
        self.speed_mps = if speed_mps.is_finite() {
            speed_mps
        } else {
            0.0
        };
    }

    /// 명령을 적용한다. 거부되면 상태는 변하지 않는다.
    pub fn apply(&mut self, command: VehicleCommand) -> Result<TransitionOutcome, TransitionError> {
        match command {
            VehicleCommand::ToggleEngine => self.toggle_engine(),
            VehicleCommand::RequestGear(gear) => self.request_gear(gear),
            VehicleCommand::EngageEmergencyStop => self.engage_emergency_stop(),
            VehicleCommand::ReleaseEmergencyStop => self.release_emergency_stop(),
        }
    }

    /// 정지 상태에서만 시동을 ON/OFF 한다. 비상 정지 중에는 거부한다.
    fn toggle_engine(&mut self) -> Result<TransitionOutcome, TransitionError> {
        let next = match self.engine {
            EngineState::EmergencyStop => return Err(TransitionError::EmergencyStopActive),
            EngineState::Off => EngineState::Ready,
            EngineState::Ready => EngineState::Off,
        };
        if !self.is_stopped() {
            return Err(TransitionError::NotStopped);
        }
        self.engine = next;
        Ok(TransitionOutcome::EngineChanged(next))
    }

    /// 기어를 변경한다. 시동 Ready가 필요하며, N 이외(P/D/R)는 정지 상태에서만 허용한다.
    fn request_gear(&mut self, gear: Gear) -> Result<TransitionOutcome, TransitionError> {
        match self.engine {
            EngineState::Off => return Err(TransitionError::EngineOff),
            EngineState::EmergencyStop => return Err(TransitionError::EmergencyStopActive),
            EngineState::Ready => {}
        }
        if gear == self.gear {
            return Ok(TransitionOutcome::Unchanged);
        }
        if gear != Gear::Neutral && !self.is_stopped() {
            return Err(TransitionError::NotStopped);
        }
        let from = self.gear;
        self.gear = gear;
        Ok(TransitionOutcome::GearChanged { from, to: gear })
    }

    /// 어떤 시동 상태에서든 비상 정지로 진입한다. 이후 구동 가속은 차단된다.
    fn engage_emergency_stop(&mut self) -> Result<TransitionOutcome, TransitionError> {
        if self.engine == EngineState::EmergencyStop {
            return Ok(TransitionOutcome::Unchanged);
        }
        self.engine = EngineState::EmergencyStop;
        Ok(TransitionOutcome::EngineChanged(EngineState::EmergencyStop))
    }

    /// 정지 상태에서만 비상 정지를 해제한다. 해제 시 Ready + P로 돌아가 재출발에 기어 선택을 요구한다.
    fn release_emergency_stop(&mut self) -> Result<TransitionOutcome, TransitionError> {
        if self.engine != EngineState::EmergencyStop {
            return Err(TransitionError::EmergencyStopNotActive);
        }
        if !self.is_stopped() {
            return Err(TransitionError::NotStopped);
        }
        self.engine = EngineState::Ready;
        self.gear = Gear::Park;
        Ok(TransitionOutcome::EngineChanged(EngineState::Ready))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시동 Ready, 지정 기어, 정지 상태의 차량을 만든다.
    fn ready_in(gear: Gear) -> VehicleState {
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state.apply(VehicleCommand::RequestGear(gear)).unwrap();
        state
    }

    /// 전체 가속 입력을 만든다.
    fn full_throttle() -> VehicleInput {
        VehicleInput {
            throttle: 1.0,
            ..VehicleInput::default()
        }
    }

    #[test]
    fn default_is_engine_off_park_stopped() {
        let state = VehicleState::default();
        assert_eq!(state.engine(), EngineState::Off);
        assert_eq!(state.gear(), Gear::Park);
        assert!(state.is_stopped());
        assert!(!state.drive_permitted());
    }

    #[test]
    fn engine_off_blocks_gear_change_and_throttle() {
        let mut state = VehicleState::default();
        state.set_input(full_throttle());
        assert_eq!(
            state.apply(VehicleCommand::RequestGear(Gear::Drive)),
            Err(TransitionError::EngineOff)
        );
        assert_eq!(state.gear(), Gear::Park);
        assert_eq!(state.effective_throttle(), 0.0);
    }

    #[test]
    fn engine_toggles_only_when_stopped() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(2.0);
        assert_eq!(
            state.apply(VehicleCommand::ToggleEngine),
            Err(TransitionError::NotStopped)
        );
        assert_eq!(state.engine(), EngineState::Ready);

        state.set_speed_mps(0.0);
        assert_eq!(
            state.apply(VehicleCommand::ToggleEngine),
            Ok(TransitionOutcome::EngineChanged(EngineState::Off))
        );
    }

    #[test]
    fn throttle_only_in_drive_or_reverse_when_ready() {
        for (gear, permitted) in [
            (Gear::Park, false),
            (Gear::Neutral, false),
            (Gear::Drive, true),
            (Gear::Reverse, true),
        ] {
            let mut state = ready_in(gear);
            state.set_input(full_throttle());
            assert_eq!(state.drive_permitted(), permitted, "{gear:?}");
            let expected = if permitted { 1.0 } else { 0.0 };
            assert_eq!(state.effective_throttle(), expected, "{gear:?}");
        }
    }

    #[test]
    fn drive_reverse_park_change_rejected_while_moving() {
        for (from, to) in [
            (Gear::Drive, Gear::Reverse),
            (Gear::Reverse, Gear::Drive),
            (Gear::Drive, Gear::Park),
            (Gear::Neutral, Gear::Drive),
        ] {
            let mut state = ready_in(from);
            state.set_speed_mps(1.0);
            assert_eq!(
                state.apply(VehicleCommand::RequestGear(to)),
                Err(TransitionError::NotStopped),
                "{from:?} -> {to:?}"
            );
            assert_eq!(state.gear(), from);
        }
    }

    #[test]
    fn neutral_allowed_while_moving() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(1.0);
        assert_eq!(
            state.apply(VehicleCommand::RequestGear(Gear::Neutral)),
            Ok(TransitionOutcome::GearChanged {
                from: Gear::Drive,
                to: Gear::Neutral
            })
        );
    }

    #[test]
    fn gear_change_allowed_below_stop_threshold() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(DEFAULT_STOP_THRESHOLD_MPS * 0.5);
        assert!(
            state
                .apply(VehicleCommand::RequestGear(Gear::Reverse))
                .is_ok()
        );
        assert_eq!(state.gear(), Gear::Reverse);
    }

    #[test]
    fn same_gear_request_is_unchanged() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(1.0);
        assert_eq!(
            state.apply(VehicleCommand::RequestGear(Gear::Drive)),
            Ok(TransitionOutcome::Unchanged)
        );
    }

    #[test]
    fn emergency_stop_blocks_throttle_gear_and_engine() {
        let mut state = ready_in(Gear::Drive);
        state.set_input(full_throttle());
        state.set_speed_mps(1.5);
        assert_eq!(
            state.apply(VehicleCommand::EngageEmergencyStop),
            Ok(TransitionOutcome::EngineChanged(EngineState::EmergencyStop))
        );
        assert_eq!(state.effective_throttle(), 0.0);
        assert_eq!(
            state.apply(VehicleCommand::RequestGear(Gear::Neutral)),
            Err(TransitionError::EmergencyStopActive)
        );
        assert_eq!(
            state.apply(VehicleCommand::ToggleEngine),
            Err(TransitionError::EmergencyStopActive)
        );
        assert_eq!(
            state.apply(VehicleCommand::EngageEmergencyStop),
            Ok(TransitionOutcome::Unchanged)
        );
    }

    #[test]
    fn emergency_stop_engages_from_engine_off() {
        let mut state = VehicleState::default();
        assert!(state.apply(VehicleCommand::EngageEmergencyStop).is_ok());
        assert_eq!(state.engine(), EngineState::EmergencyStop);
    }

    #[test]
    fn emergency_release_requires_stop_and_returns_to_park() {
        let mut state = ready_in(Gear::Drive);
        state.set_speed_mps(1.0);
        state.apply(VehicleCommand::EngageEmergencyStop).unwrap();
        assert_eq!(
            state.apply(VehicleCommand::ReleaseEmergencyStop),
            Err(TransitionError::NotStopped)
        );
        assert_eq!(state.engine(), EngineState::EmergencyStop);

        state.set_speed_mps(0.0);
        assert_eq!(
            state.apply(VehicleCommand::ReleaseEmergencyStop),
            Ok(TransitionOutcome::EngineChanged(EngineState::Ready))
        );
        assert_eq!(state.gear(), Gear::Park);
        assert!(!state.drive_permitted());
    }

    #[test]
    fn release_without_emergency_stop_is_rejected() {
        let mut state = ready_in(Gear::Park);
        assert_eq!(
            state.apply(VehicleCommand::ReleaseEmergencyStop),
            Err(TransitionError::EmergencyStopNotActive)
        );
    }

    #[test]
    fn input_and_speed_are_sanitized() {
        let mut state = VehicleState::default();
        state.set_input(VehicleInput {
            throttle: 3.0,
            brake: f32::NAN,
            steering: -5.0,
        });
        assert_eq!(state.input().throttle, 1.0);
        assert_eq!(state.input().brake, 0.0);
        assert_eq!(state.steering(), -1.0);

        state.set_speed_mps(f32::INFINITY);
        assert_eq!(state.speed_mps(), 0.0);
    }

    /// 짧은 명령·속도 시퀀스를 모두 조합해, 어떤 순서에서도 불변 조건이 깨지지 않는지 확인한다.
    #[test]
    fn invariants_hold_for_all_short_command_sequences() {
        let commands = [
            VehicleCommand::ToggleEngine,
            VehicleCommand::RequestGear(Gear::Park),
            VehicleCommand::RequestGear(Gear::Reverse),
            VehicleCommand::RequestGear(Gear::Neutral),
            VehicleCommand::RequestGear(Gear::Drive),
            VehicleCommand::EngageEmergencyStop,
            VehicleCommand::ReleaseEmergencyStop,
        ];
        let speeds = [0.0, 1.0];
        let steps: Vec<(VehicleCommand, f32)> = commands
            .iter()
            .flat_map(|&command| speeds.iter().map(move |&speed| (command, speed)))
            .collect();

        let depth = 4;
        let total = steps.len().pow(depth);
        for index in 0..total {
            let mut state = VehicleState::default();
            state.set_input(full_throttle());
            let mut remaining = index;
            for _ in 0..depth {
                let (command, speed) = steps[remaining % steps.len()];
                remaining /= steps.len();
                state.set_speed_mps(speed);

                let before = state.clone();
                let result = state.apply(command);
                check_transition(&before, &state, command, result);
            }
        }
    }

    /// 한 번의 명령 적용 전후 상태가 안전 규칙을 지키는지 검사한다.
    fn check_transition(
        before: &VehicleState,
        after: &VehicleState,
        command: VehicleCommand,
        result: Result<TransitionOutcome, TransitionError>,
    ) {
        match result {
            Err(_) | Ok(TransitionOutcome::Unchanged) => {
                assert_eq!(before, after, "{command:?} must not change state")
            }
            Ok(_) => {}
        }
        if after.engine() != EngineState::Ready {
            assert_eq!(after.effective_throttle(), 0.0);
        }
        if before.gear() != after.gear() && after.gear() != Gear::Neutral {
            assert!(
                before.is_stopped(),
                "{command:?}: gear changed while moving"
            );
        }
        if before.engine() == EngineState::Off && after.gear() != before.gear() {
            panic!("{command:?}: gear changed while engine off");
        }
        if before.engine() != after.engine() && command != VehicleCommand::EngageEmergencyStop {
            assert!(
                before.is_stopped(),
                "{command:?}: engine changed while moving"
            );
        }
    }
}
