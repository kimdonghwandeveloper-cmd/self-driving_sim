//! 차량 상태·입력·명령 결과를 보여 주는 개발용 HUD.

use std::fmt::{self, Write};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::calibration::{ActiveTrial, CalibrationLog, CalibrationReadout};
use crate::camera::{ActiveView, ViewDirection};
use crate::input::mapping::KeyAction;
use crate::input::{GamepadBindings, InputStatus, KeyBindings};
use crate::training::TrainingLog;
use crate::vehicle::controller::{
    ControlFeedback, Vehicle, VehicleDynamics, engine_label, gear_label,
};
use crate::vehicle::{Gear, Motion, VehicleContact, VehicleMotion, VehicleState};

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

/// HUD에 표시할 충돌 상태.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollisionStatus {
    /// 지금 장애물에 닿아 있는지.
    pub touching: bool,
    /// 이번 세션에 기록된 충돌 횟수.
    pub collisions: usize,
}

/// HUD가 충돌 상태를 만들 때 읽는 리소스 묶음.
#[derive(SystemParam)]
pub struct CollisionSources<'w> {
    contact: Res<'w, VehicleContact>,
    log: Res<'w, TrainingLog>,
}

impl CollisionSources<'_> {
    /// 현재 접촉 여부와 누적 충돌 횟수로 표시용 상태를 만든다.
    fn status(&self) -> CollisionStatus {
        CollisionStatus {
            touching: self.contact.0.touching,
            collisions: self.log.collision_count(),
        }
    }
}

/// `update_hud`가 프레임 사이에 재사용하는 문자열.
#[derive(Default)]
pub struct HudCache {
    /// 매핑이 바뀔 때만 다시 만드는 키보드·게임패드 조작 안내.
    control_guide: String,
    /// 매 프레임 상태 문자열을 쓰는 버퍼. 할당을 재사용한다.
    buffer: String,
}

/// HUD가 보정 표시값을 만들 때 읽는 리소스 묶음.
#[derive(SystemParam)]
pub struct CalibrationSources<'w> {
    dynamics: Res<'w, VehicleDynamics>,
    log: Res<'w, CalibrationLog>,
}

impl CalibrationSources<'_> {
    /// 현재 상태·자세로 회전반경·정지거리·시험 결과 표시값을 만든다.
    fn readout(&self, state: &VehicleState, motion: &Motion) -> CalibrationReadout {
        CalibrationReadout::new(state, motion, &self.dynamics.0, &self.log)
    }
}

/// 매 프레임 HUD 텍스트를 현재 상태로 갱신한다. 내용이 같으면 텍스트를 건드리지 않는다.
#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    vehicle: Res<Vehicle>,
    motion: Res<VehicleMotion>,
    feedback: Res<ControlFeedback>,
    collision: CollisionSources,
    calibration: CalibrationSources,
    bindings: Res<KeyBindings>,
    gamepad: Res<GamepadBindings>,
    input: Res<InputStatus>,
    view: Res<ActiveView>,
    mut cache: Local<HudCache>,
    mut query: Query<&mut Text, With<VehicleHud>>,
) {
    let cache = &mut *cache;
    if bindings.is_changed() || gamepad.is_changed() || cache.control_guide.is_empty() {
        cache.control_guide = control_guide_text(&bindings, &gamepad);
    }
    write_hud(
        &mut cache.buffer,
        &vehicle.0,
        &motion.0,
        collision.status(),
        &calibration.readout(&vehicle.0, &motion.0),
        &feedback,
        &input,
        *view,
        &cache.control_guide,
    );
    for mut text in &mut query {
        if text.0 != cache.buffer {
            text.0.clone_from(&cache.buffer);
        }
    }
}

/// HUD에 표시할 전체 문자열을 만든다.
#[allow(clippy::too_many_arguments)]
pub fn hud_text(
    state: &VehicleState,
    motion: &Motion,
    collision: CollisionStatus,
    calibration: &CalibrationReadout,
    feedback: &ControlFeedback,
    input: &InputStatus,
    view: ActiveView,
    control_guide: &str,
) -> String {
    let mut out = String::new();
    write_hud(
        &mut out,
        state,
        motion,
        collision,
        calibration,
        feedback,
        input,
        view,
        control_guide,
    );
    out
}

/// `out`을 비우고 상태 줄, 보정 측정 줄, 조작 안내를 이어 쓴다.
#[allow(clippy::too_many_arguments)]
fn write_hud(
    out: &mut String,
    state: &VehicleState,
    motion: &Motion,
    collision: CollisionStatus,
    calibration: &CalibrationReadout,
    feedback: &ControlFeedback,
    input: &InputStatus,
    view: ActiveView,
    control_guide: &str,
) {
    out.clear();
    write_status(out, state, motion, collision, feedback, input, view)
        .expect("writing to a String cannot fail");
    out.push_str("\n\n");
    write_calibration(out, calibration).expect("writing to a String cannot fail");
    out.push_str("\n\n");
    out.push_str(control_guide);
}

/// 실제 부호 있는 속도로 이동 방향(전진/후진/정지)을 판정한다. 정지 판정은 상태 머신 임계값을 쓴다.
pub fn travel_label(state: &VehicleState) -> &'static str {
    if !state.speed_valid() {
        "INVALID"
    } else if state.is_stopped() {
        "STOPPED"
    } else if state.speed_mps() > 0.0 {
        "FORWARD"
    } else {
        "REVERSE"
    }
}

/// 시점 줄 문자열을 만든다. 후방을 보면서 전진하는 등 화면과 진행 방향이 다를 때 혼동을 막는다.
pub fn view_line(view: ActiveView, state: &VehicleState) -> String {
    let looking = if view.debug_external {
        "EXTERNAL (debug)".to_owned()
    } else {
        format!("{} window", view.direction.label())
    };
    let mut line = format!(
        "View: {looking} | Gear: {} | Moving: {}",
        gear_label(state.gear()),
        travel_label(state)
    );
    if !view.debug_external && view.direction != ViewDirection::Front {
        line.push_str("  (not facing vehicle front)");
    }
    line
}

/// 차량 상태·운동·충돌·입력·입력 장치·시점·마지막 명령 결과 줄을 쓴다.
fn write_status(
    out: &mut String,
    state: &VehicleState,
    motion: &Motion,
    collision: CollisionStatus,
    feedback: &ControlFeedback,
    status: &InputStatus,
    view: ActiveView,
) -> fmt::Result {
    let input = state.input();
    let moving = if !state.speed_valid() {
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
    writeln!(out, "{}", view_line(view, state))?;
    writeln!(
        out,
        "Engine: {}   Gear: {}",
        engine_label(state.engine()),
        gear_label(state.gear())
    )?;
    writeln!(
        out,
        "Speed: {:+.2} m/s ({:.1} km/h, {moving})   Accel: {:+.2} m/s^2",
        state.speed_mps(),
        state.speed_mps().abs() * 3.6,
        motion.accel_mps2
    )?;
    writeln!(
        out,
        "Wheel angle: {:+.1} deg   Heading: {:+.1} deg   Position: ({:.1}, {:.1}) m",
        motion.steer_angle_rad.to_degrees(),
        motion.heading_rad.to_degrees(),
        motion.x_m,
        motion.z_m
    )?;
    writeln!(
        out,
        "Obstacle contact: {}   Collisions: {}",
        if collision.touching { "YES" } else { "no" },
        collision.collisions
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
    match &status.gamepad {
        Some(gamepad) if gamepad.ready => write!(out, "Gamepad: connected ({})", gamepad.name)?,
        Some(gamepad) => write!(
            out,
            "Gamepad: connected ({}) - release all inputs to activate",
            gamepad.name
        )?,
        None => write!(out, "Gamepad: not connected")?,
    }
    writeln!(out, "   Active input: {}", status.source.label())?;
    match &feedback.last {
        Some(entry) if entry.accepted => write!(out, "Last: [OK] {}", entry.message),
        Some(entry) => write!(out, "Last: [REJECTED] {}", entry.message),
        None => write!(out, "Last: -"),
    }
}

/// 회전반경·예상 정지거리·진행 중 시험·마지막 시험 결과 줄을 쓴다. 모두 시뮬레이터 임시값 기준이다.
fn write_calibration(out: &mut String, calibration: &CalibrationReadout) -> fmt::Result {
    match calibration.turn_radius_m {
        Some(radius) => write!(out, "Turn radius (rear axle): {radius:.2} m")?,
        None => write!(out, "Turn radius (rear axle): straight")?,
    }
    writeln!(
        out,
        "   Min at full lock: {:.2} m",
        calibration.min_turn_radius_m
    )?;
    writeln!(
        out,
        "Full-brake stop estimate: {:.2} m",
        calibration.brake_estimate_m
    )?;
    match calibration.active {
        Some(ActiveTrial::Braking { distance_m, time_s }) => {
            writeln!(out, "Test: braking {distance_m:.2} m / {time_s:.2} s")?;
        }
        Some(ActiveTrial::Accelerating { distance_m, time_s }) => {
            writeln!(out, "Test: accelerating {distance_m:.2} m / {time_s:.2} s")?;
        }
        None => writeln!(out, "Test: -")?,
    }
    let (stops, accels) = calibration.trials;
    match calibration.last_stop {
        Some(stop) => writeln!(
            out,
            "Last stop #{stops}: {:.2} m from {:.2} m/s in {:.2} s ({})",
            stop.distance_m,
            stop.from_speed_mps,
            stop.time_s,
            if stop.emergency { "e-stop" } else { "brake" }
        )?,
        None => writeln!(out, "Last stop: -")?,
    }
    match calibration.last_accel {
        Some(accel) => write!(
            out,
            "Last accel #{accels}: 0 -> {:.2} m/s in {:.2} s, {:.2} m ({})",
            accel.to_speed_mps,
            accel.time_s,
            accel.distance_m,
            if accel.reached_limit {
                "top speed"
            } else {
                "released"
            }
        ),
        None => write!(out, "Last accel: -"),
    }
}

/// 키 매핑으로 HUD 아래쪽 키 안내 네 줄을 만든다.
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
            "      engine {} | e-stop {} | release {} | reset to start {}",
            key(KeyAction::EngineToggle),
            key(KeyAction::EmergencyStop),
            key(KeyAction::EmergencyRelease),
            key(KeyAction::ResetCourse)
        ),
        format!(
            "      view front {} | rear {} | left {} | right {} | debug ext. camera {}",
            key(KeyAction::View(ViewDirection::Front)),
            key(KeyAction::View(ViewDirection::Rear)),
            key(KeyAction::View(ViewDirection::Left)),
            key(KeyAction::View(ViewDirection::Right)),
            key(KeyAction::DebugCamera)
        ),
    ]
    .join("\n")
}

/// 게임패드 매핑으로 HUD 아래쪽 게임패드 안내 세 줄을 만든다.
pub fn gamepad_guide_text(bindings: &GamepadBindings) -> String {
    let button = |action| bindings.label(action);
    let (throttle, brake, steering) = bindings.drive_labels();
    [
        format!("Pad   throttle {throttle} | brake {brake} | steer {steering}"),
        format!(
            "      gear P {} | R {} | N {} | D {}",
            button(KeyAction::Gear(Gear::Park)),
            button(KeyAction::Gear(Gear::Reverse)),
            button(KeyAction::Gear(Gear::Neutral)),
            button(KeyAction::Gear(Gear::Drive))
        ),
        format!(
            "      engine {} | e-stop {} | release {}",
            button(KeyAction::EngineToggle),
            button(KeyAction::EmergencyStop),
            button(KeyAction::EmergencyRelease)
        ),
    ]
    .join("\n")
}

/// 키보드·게임패드 안내를 이어 HUD 조작 안내를 만든다.
pub fn control_guide_text(keys: &KeyBindings, gamepad: &GamepadBindings) -> String {
    format!("{}\n{}", key_guide_text(keys), gamepad_guide_text(gamepad))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputSource;
    use crate::input::merge::ConnectedGamepad;
    use crate::vehicle::controller::Feedback;

    /// 기본 매핑의 조작 안내를 만든다.
    fn default_guide() -> String {
        control_guide_text(&KeyBindings::default(), &GamepadBindings::default())
    }

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
        let text = hud_text(
            &state,
            &Motion::default(),
            CollisionStatus::default(),
            &CalibrationReadout::default(),
            &feedback,
            &InputStatus::default(),
            ActiveView::default(),
            &default_guide(),
        );
        assert!(text.contains("Engine: OFF   Gear: P"));
        assert!(text.contains("Last: [REJECTED] gear D: engine is off"));
        assert!(text.contains("e-stop Space"));
        assert!(text.contains("not verified"));
        assert!(text.contains("View: FRONT window | Gear: P | Moving: STOPPED"));
        assert!(text.contains("view front Digit1 | rear Digit2 | left Digit3 | right Digit4"));
    }

    /// 시점과 실제 이동 방향을 함께 표시하고, N 관성 이동·후방 시점 전진을 구분하는지 확인한다.
    #[test]
    fn hud_view_line_separates_view_and_travel() {
        use crate::vehicle::{Gear, VehicleCommand};
        let mut state = VehicleState::default();
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state.set_speed_mps(2.0);
        let rear = ActiveView {
            direction: ViewDirection::Rear,
            debug_external: false,
        };
        assert_eq!(
            view_line(rear, &state),
            "View: REAR window | Gear: D | Moving: FORWARD  (not facing vehicle front)"
        );

        state
            .apply(VehicleCommand::RequestGear(Gear::Neutral))
            .unwrap();
        let line = view_line(ActiveView::default(), &state);
        assert_eq!(line, "View: FRONT window | Gear: N | Moving: FORWARD");

        state.set_speed_mps(-1.0);
        assert!(view_line(rear, &state).contains("Moving: REVERSE"));
        state.set_speed_mps(0.0);
        let debug = ActiveView {
            debug_external: true,
            ..rear
        };
        assert_eq!(
            view_line(debug, &state),
            "View: EXTERNAL (debug) | Gear: N | Moving: STOPPED"
        );
    }

    /// 게임패드 연결 상태·선택 장치와 게임패드 조작 안내를 표시하는지 확인한다.
    #[test]
    fn hud_shows_gamepad_status_and_guide() {
        let text = |input: &InputStatus| {
            hud_text(
                &VehicleState::default(),
                &Motion::default(),
                CollisionStatus::default(),
                &CalibrationReadout::default(),
                &ControlFeedback::default(),
                input,
                ActiveView::default(),
                &default_guide(),
            )
        };
        let disconnected = text(&InputStatus::default());
        assert!(disconnected.contains("Gamepad: not connected   Active input: Keyboard"));
        assert!(
            disconnected.contains(
                "Pad   throttle left stick up | brake left stick down | steer right stick"
            )
        );
        assert!(disconnected.contains("gear P Y | R B | N A | D X"));
        assert!(disconnected.contains("engine Start | e-stop RB | release Back"));

        let connected = text(&InputStatus {
            gamepad: Some(ConnectedGamepad {
                entity: Entity::PLACEHOLDER,
                name: "Xbox Controller".to_owned(),
                ready: true,
            }),
            source: InputSource::Gamepad,
        });
        assert!(connected.contains("Gamepad: connected (Xbox Controller)   Active input: Gamepad"));

        let waiting = text(&InputStatus {
            gamepad: Some(ConnectedGamepad {
                entity: Entity::PLACEHOLDER,
                name: "Xbox Controller".to_owned(),
                ready: false,
            }),
            source: InputSource::Keyboard,
        });
        assert!(waiting.contains("release all inputs to activate"));
    }

    /// 속도가 잘못된 값이면 HUD에 INVALID로 표시하는지 확인한다.
    #[test]
    fn hud_marks_invalid_speed() {
        let mut state = VehicleState::default();
        state.set_speed_mps(f32::NAN);
        let text = hud_text(
            &state,
            &Motion::default(),
            CollisionStatus::default(),
            &CalibrationReadout::default(),
            &ControlFeedback::default(),
            &InputStatus::default(),
            ActiveView::default(),
            &default_guide(),
        );
        assert!(text.contains("INVALID"));
    }

    /// 장애물 접촉 여부와 누적 충돌 횟수를 표시하는지 확인한다.
    #[test]
    fn hud_shows_obstacle_contact() {
        let text = hud_text(
            &VehicleState::default(),
            &Motion::default(),
            CollisionStatus {
                touching: true,
                collisions: 2,
            },
            &CalibrationReadout::default(),
            &ControlFeedback::default(),
            &InputStatus::default(),
            ActiveView::default(),
            &default_guide(),
        );
        assert!(text.contains("Obstacle contact: YES   Collisions: 2"));
    }

    /// 회전반경·예상 정지거리·시험 결과와 복귀 키 안내를 표시하는지 확인한다.
    #[test]
    fn hud_shows_calibration_readout() {
        use crate::calibration::{AccelResult, StopResult};
        let readout = CalibrationReadout {
            turn_radius_m: Some(9.5),
            min_turn_radius_m: 4.85,
            brake_estimate_m: 1.25,
            active: Some(ActiveTrial::Braking {
                distance_m: 0.5,
                time_s: 0.25,
            }),
            last_stop: Some(StopResult {
                from_speed_mps: 4.0,
                distance_m: 2.42,
                time_s: 1.21,
                emergency: false,
            }),
            last_accel: Some(AccelResult {
                to_speed_mps: 5.0,
                distance_m: 12.54,
                time_s: 5.0,
                reached_limit: true,
            }),
            trials: (3, 1),
        };
        let text = hud_text(
            &VehicleState::default(),
            &Motion::default(),
            CollisionStatus::default(),
            &readout,
            &ControlFeedback::default(),
            &InputStatus::default(),
            ActiveView::default(),
            &default_guide(),
        );
        assert!(text.contains("Turn radius (rear axle): 9.50 m   Min at full lock: 4.85 m"));
        assert!(text.contains("Full-brake stop estimate: 1.25 m"));
        assert!(text.contains("Test: braking 0.50 m / 0.25 s"));
        assert!(text.contains("Last stop #3: 2.42 m from 4.00 m/s in 1.21 s (brake)"));
        assert!(text.contains("Last accel #1: 0 -> 5.00 m/s in 5.00 s, 12.54 m (top speed)"));
        assert!(text.contains("reset to start KeyR"));

        let straight = hud_text(
            &VehicleState::default(),
            &Motion::default(),
            CollisionStatus::default(),
            &CalibrationReadout::default(),
            &ControlFeedback::default(),
            &InputStatus::default(),
            ActiveView::default(),
            &default_guide(),
        );
        assert!(straight.contains("Turn radius (rear axle): straight"));
        assert!(straight.contains("Last stop: -"));
    }
}
