//! 자율주행 셔틀 안전관리자 조작 훈련 시뮬레이터의 도메인 로직과 Bevy 플러그인.

pub mod calibration;
pub mod camera;
pub mod config;
pub mod input;
pub mod scene;
pub mod training;
pub mod ui;
pub mod vehicle;
pub mod world;

use bevy::app::{RunFixedMainLoop, RunFixedMainLoopSystems};
use bevy::prelude::*;

/// 프레임 안의 처리 순서. 입력 → 차량 제어 → (고정 주기 운동 계산) → 화면 표시 순으로 실행한다.
///
/// `Input`·`Control`은 고정 주기 루프 직전(`RunFixedMainLoop`의 `BeforeFixedMainLoop`)에 돌아서,
/// 이번 프레임의 가속·제동 입력과 비상 정지 등 명령이 같은 프레임의 운동 계산에 바로 반영된다.
/// `Display`는 운동 계산이 끝난 뒤 `Update`에서 돈다.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Input,
    Control,
    Display,
}

/// [`SimSet`] 실행 위치와 순서를 앱에 설정한다.
pub fn configure_sim_sets(app: &mut App) {
    app.configure_sets(
        RunFixedMainLoop,
        (SimSet::Input, SimSet::Control)
            .chain()
            .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::ButtonState;
    use bevy::input::gamepad::{
        GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
        RawGamepadButtonChangedEvent, RawGamepadEvent,
    };
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::calibration::{CalibrationLog, CalibrationPlugin};
    use crate::camera::{ActiveView, CameraPlugin, ViewDirection};
    use crate::input::{DriverInput, InputSource, InputStatus};
    use crate::training::{TrainingLog, TrainingPlugin};
    use crate::vehicle::{
        DynamicsParams, EngineState, Gear, GroundRect, Obstacle, Vehicle, VehicleCommand,
        VehicleContact, VehicleInput, VehicleMotion, VehiclePlugin,
    };

    /// 키보드 입력과 차량 시스템만 있는 창 없는 앱을 만든다. 한 번 갱신할 때마다 고정 주기 1단계가 돈다.
    fn headless_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
        configure_sim_sets(&mut app);
        app.add_plugins((
            input::InputPlugin,
            VehiclePlugin,
            TrainingPlugin,
            CalibrationPlugin,
        ));
        app
    }

    /// 창 없는 앱에 카메라 플러그인(시점 선택·카메라 갱신)을 더한다.
    fn headless_app_with_camera() -> App {
        let mut app = headless_app();
        app.add_plugins(CameraPlugin);
        app
    }

    /// 시동을 켜고 D로 바꾼다.
    fn engine_on_in_drive(app: &mut App) {
        let state = &mut app.world_mut().resource_mut::<Vehicle>().0;
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
    }

    /// 키 하나의 누름/뗌 메시지를 만든다. 실제 창 입력과 같은 경로(PreUpdate 키보드 시스템)를 탄다.
    fn key_message(key_code: KeyCode, state: ButtonState) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    /// `keys`만 눌린 상태가 되도록 누름/뗌 메시지를 보내고 앱을 한 번 갱신한다.
    ///
    /// `ButtonInput`을 직접 바꾸면 PreUpdate에서 "새로 눌림"이 지워지므로 메시지로 전달한다.
    fn update_with_keys(app: &mut App, keys: &[KeyCode]) {
        let held: Vec<KeyCode> = app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .get_pressed()
            .copied()
            .collect();
        for key in held.iter().filter(|key| !keys.contains(key)) {
            app.world_mut()
                .write_message(key_message(*key, ButtonState::Released));
        }
        for key in keys.iter().filter(|key| !held.contains(key)) {
            app.world_mut()
                .write_message(key_message(*key, ButtonState::Pressed));
        }
        app.update();
    }

    /// `keys`를 누른 채 가상 시간이 `until`(시작부터 누적)에 이를 때까지 앱을 갱신한다.
    ///
    /// 렌더 프레임 간격은 `frames`를 순서대로 반복해 쓰고, 마지막 프레임은 `until`에 딱 맞게 자른다.
    /// 한 프레임 간격이 고정 주기(1/64 s)보다 길면 그 프레임에 고정 단계가 여러 번 몰린다.
    fn run_frames_until(app: &mut App, keys: &[KeyCode], until: Duration, frames: &[Duration]) {
        for frame in frames.iter().cycle() {
            let elapsed = app.world().resource::<Time<Virtual>>().elapsed();
            if elapsed >= until {
                break;
            }
            let delta = (*frame).min(until - elapsed);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(delta));
            update_with_keys(app, keys);
        }
    }

    /// 렌더 프레임 간격이 30 Hz·144 Hz·불규칙(고정 단계 몰림 포함)으로 달라도,
    /// 입력→명령→고정 주기 운동 계산 전체 경로의 주행 결과가 같은지 확인한다.
    #[test]
    fn app_result_is_independent_of_frame_rate() {
        let ms = |value: f64| Duration::from_secs_f64(value / 1000.0);
        let patterns: [Vec<Duration>; 4] = [
            vec![ms(1000.0 / 30.0)],
            vec![ms(1000.0 / 144.0)],
            vec![ms(7.0), ms(23.0), ms(3.0), ms(41.0)],
            // 짧은 프레임 사이에 200 ms 멈춤: 한 프레임에 고정 단계 12~13번이 몰린다.
            vec![ms(1000.0 / 144.0), ms(1000.0 / 144.0), ms(200.0)],
        ];
        let drive = |frames: &[Duration]| {
            let mut app = headless_app();
            engine_on_in_drive(&mut app);
            let seconds = |value: u64| Duration::from_secs(value);
            run_frames_until(
                &mut app,
                &[KeyCode::KeyW, KeyCode::KeyD],
                seconds(3),
                frames,
            );
            run_frames_until(&mut app, &[KeyCode::KeyW], seconds(5), frames);
            run_frames_until(&mut app, &[KeyCode::KeyS], seconds(7), frames);
            let world = app.world();
            let motion = world.resource::<VehicleMotion>().0;
            let fixed_steps = world.resource::<Time<Fixed>>().elapsed().as_secs_f64() * 64.0;
            (
                motion.x_m,
                motion.z_m,
                motion.heading_rad,
                world.resource::<Vehicle>().0.speed_mps(),
                fixed_steps.round() as u32,
            )
        };
        let reference = drive(&patterns[0]);
        assert_eq!(reference.4, 64 * 7);
        assert!(reference.2.abs() > 0.1, "should have turned: {reference:?}");
        assert!(
            reference.1 < -5.0,
            "should have moved forward: {reference:?}"
        );
        for frames in &patterns[1..] {
            let result = drive(frames);
            assert_eq!(
                result.4, reference.4,
                "fixed step count differs for {frames:?}"
            );
            for (got, want) in [
                (result.0, reference.0),
                (result.1, reference.1),
                (result.2, reference.2),
                (result.3, reference.3),
            ] {
                assert!(
                    (got - want).abs() < 1e-4,
                    "{frames:?}: {result:?} vs {reference:?}"
                );
            }
        }
    }

    /// 같은 렌더 프레임 간격·입력 순서로 앱 전체를 두 번 돌리면 결과가 정확히 같은지 확인한다.
    #[test]
    fn app_replay_is_deterministic() {
        let frames = [Duration::from_millis(9), Duration::from_millis(31)];
        let replay = || {
            let mut app = headless_app();
            engine_on_in_drive(&mut app);
            run_frames_until(
                &mut app,
                &[KeyCode::KeyW, KeyCode::KeyA],
                Duration::from_secs(2),
                &frames,
            );
            run_frames_until(&mut app, &[KeyCode::KeyS], Duration::from_secs(4), &frames);
            let world = app.world();
            (
                world.resource::<Vehicle>().0.clone(),
                world.resource::<VehicleMotion>().0,
            )
        };
        assert_eq!(replay(), replay());
    }

    /// 가속 키를 누른 채 제동·비상 정지 키를 누르면, 그 프레임의 운동 계산부터 해당 감속이 적용되는지 확인한다.
    ///
    /// 가속 키를 계속 누르고 있으므로 입력·명령이 한 프레임 늦으면 속도가 오히려 오른다.
    #[test]
    fn brake_and_emergency_stop_apply_in_same_frame() {
        let params = DynamicsParams::default();
        let dt = 1.0 / 64.0;
        for (stop_key, expected_decel) in [
            (KeyCode::KeyS, params.max_brake_decel_mps2),
            (KeyCode::Space, params.emergency_decel_mps2),
        ] {
            let mut app = headless_app();
            engine_on_in_drive(&mut app);
            for _ in 0..64 {
                update_with_keys(&mut app, &[KeyCode::KeyW]);
            }
            let before = app.world().resource::<Vehicle>().0.speed_mps();
            assert!(before > 0.5, "vehicle should be moving, got {before}");

            update_with_keys(&mut app, &[KeyCode::KeyW, stop_key]);
            let state = &app.world().resource::<Vehicle>().0;
            let drop = before - state.speed_mps();
            assert!(
                drop >= expected_decel * dt * 0.99,
                "{stop_key:?}: speed changed from {before} to {} in the stop frame",
                state.speed_mps()
            );
            if stop_key == KeyCode::Space {
                assert_eq!(state.engine(), EngineState::EmergencyStop);
            }
        }
    }

    /// 가속 키를 누른 채 장애물로 달려가면 앱 전체 경로에서 관통 없이 멈추고,
    /// 밀고 있는 동안에도 교육 기록에는 충돌이 한 번만 남는지 확인한다.
    #[test]
    fn collision_stops_vehicle_and_is_recorded_once() {
        let mut app = headless_app();
        // 앞범퍼(z=-3.5)에서 4 m 앞에 벽의 앞면(z=-7.5)이 있다.
        let wall = GroundRect::axis_aligned(Vec2::new(0.0, -8.0), Vec2::new(6.0, 1.0));
        app.world_mut().spawn(Obstacle(wall));
        engine_on_in_drive(&mut app);
        for _ in 0..64 * 6 {
            update_with_keys(&mut app, &[KeyCode::KeyW]);
            let motion = app.world().resource::<VehicleMotion>().0;
            assert!(motion.z_m > -4.0 - 1e-3, "penetrated: z={}", motion.z_m);
        }

        let world = app.world();
        assert_eq!(world.resource::<Vehicle>().0.speed_mps(), 0.0);
        assert!(world.resource::<VehicleContact>().0.touching);
        assert_eq!(world.resource::<TrainingLog>().collision_count(), 1);
    }

    /// 게임패드 연결 메시지를 보내고 앱을 한 번 갱신해 게임패드 엔티티를 만든다.
    fn connect_gamepad(app: &mut App) -> Entity {
        let gamepad = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gamepad,
            GamepadConnection::Connected {
                name: "Test Pad".to_owned(),
                vendor_id: None,
                product_id: None,
            },
        ));
        app.update();
        gamepad
    }

    /// 게임패드 연결 해제 메시지를 보낸다.
    fn disconnect_gamepad(app: &mut App, gamepad: Entity) {
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gamepad,
            GamepadConnection::Disconnected,
        ));
    }

    /// 아날로그 축 값 변화를 실제 장치와 같은 원시 메시지 경로로 보낸다.
    fn set_pad_axis(app: &mut App, gamepad: Entity, axis: GamepadAxis, value: f32) {
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                gamepad, axis, value,
            )));
    }

    /// 버튼·트리거 값 변화를 실제 장치와 같은 원시 메시지 경로로 보낸다.
    fn set_pad_button(app: &mut App, gamepad: Entity, button: GamepadButton, value: f32) {
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                gamepad, button, value,
            )));
    }

    /// 게임패드 Start로 시동, X로 D를 넣고 왼쪽 스틱 위·오른쪽 스틱으로 주행하며,
    /// 왼쪽 스틱 아래로 멈추는지 앱 전체 경로로 확인한다.
    #[test]
    fn gamepad_starts_engine_and_drives() {
        let mut app = headless_app();
        let gamepad = connect_gamepad(&mut app);
        assert!(app.world().resource::<InputStatus>().gamepad.is_some());

        set_pad_button(&mut app, gamepad, GamepadButton::Start, 1.0);
        app.update();
        set_pad_button(&mut app, gamepad, GamepadButton::Start, 0.0);
        set_pad_button(&mut app, gamepad, GamepadButton::West, 1.0);
        app.update();
        let state = &app.world().resource::<Vehicle>().0;
        assert_eq!(state.engine(), EngineState::Ready);
        assert_eq!(state.gear(), Gear::Drive);

        set_pad_button(&mut app, gamepad, GamepadButton::West, 0.0);
        set_pad_axis(&mut app, gamepad, GamepadAxis::LeftStickY, 1.0);
        set_pad_axis(&mut app, gamepad, GamepadAxis::RightStickX, 1.0);
        for _ in 0..64 {
            app.update();
        }
        let world = app.world();
        assert!(world.resource::<Vehicle>().0.speed_mps() > 0.5);
        assert_eq!(world.resource::<DriverInput>().0.steering, 1.0);
        assert_eq!(world.resource::<InputStatus>().source, InputSource::Gamepad);

        set_pad_axis(&mut app, gamepad, GamepadAxis::LeftStickY, -1.0);
        for _ in 0..128 {
            app.update();
        }
        assert_eq!(app.world().resource::<Vehicle>().0.speed_mps(), 0.0);
    }

    /// 주행 중 게임패드 연결이 끊기면 입력이 그 프레임에 사라지고 비상 정지로 멈추는지 확인한다.
    #[test]
    fn gamepad_disconnect_while_driving_clears_input_and_stops() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let gamepad = connect_gamepad(&mut app);
        set_pad_axis(&mut app, gamepad, GamepadAxis::LeftStickY, 1.0);
        set_pad_axis(&mut app, gamepad, GamepadAxis::RightStickX, -1.0);
        for _ in 0..64 {
            app.update();
        }
        assert!(app.world().resource::<Vehicle>().0.speed_mps() > 0.5);

        disconnect_gamepad(&mut app, gamepad);
        app.update();
        let world = app.world();
        assert_eq!(world.resource::<DriverInput>().0, VehicleInput::default());
        let status = world.resource::<InputStatus>();
        assert_eq!(status.gamepad, None);
        assert_eq!(status.source, InputSource::Keyboard);
        assert_eq!(
            world.resource::<Vehicle>().0.engine(),
            EngineState::EmergencyStop
        );

        for _ in 0..128 {
            app.update();
        }
        assert_eq!(app.world().resource::<Vehicle>().0.speed_mps(), 0.0);
    }

    /// 중간 입력이 설정한 데드존 그대로 적용되어 Bevy 필터와 이중으로 줄어들지 않는지 앱 전체 경로로 확인한다.
    #[test]
    fn analog_values_use_only_configured_deadzone() {
        /// 원시 값을 넣고 앱을 한 번 갱신한 뒤 차량 입력을 반환한다.
        fn input_after(
            app: &mut App,
            gamepad: Entity,
            steering: f32,
            throttle: f32,
        ) -> VehicleInput {
            set_pad_axis(app, gamepad, GamepadAxis::RightStickX, steering);
            set_pad_axis(app, gamepad, GamepadAxis::LeftStickY, throttle);
            app.update();
            app.world().resource::<DriverInput>().0
        }
        let mut app = headless_app();
        let gamepad = connect_gamepad(&mut app);

        let inside = input_after(&mut app, gamepad, 0.11, 0.11);
        assert_eq!(inside, VehicleInput::default());

        // 이중 적용이면 원시 0.14는 데드존 안(약 0.164)이라 0이 된다.
        let just_outside = input_after(&mut app, gamepad, 0.14, 0.14);
        assert!((just_outside.steering - 0.02 / 0.88).abs() < 1e-4);
        assert!((just_outside.throttle - 0.02 / 0.88).abs() < 1e-4);

        let middle = input_after(&mut app, gamepad, -0.56, 0.56);
        assert!((middle.steering + 0.5).abs() < 1e-4);
        assert!((middle.throttle - 0.5).abs() < 1e-4);
    }

    /// 대체 게임패드에 가속·조향이 눌려 있으면 연결 해제 프레임과 이후에도 입력이 없고,
    /// 모든 입력을 놓은 뒤에야 조작할 수 있는지 확인한다.
    #[test]
    fn replacement_gamepad_needs_neutral_before_driving() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let first = connect_gamepad(&mut app);
        let second = connect_gamepad(&mut app);
        set_pad_axis(&mut app, second, GamepadAxis::LeftStickY, 1.0);
        set_pad_axis(&mut app, second, GamepadAxis::RightStickX, 1.0);
        app.update();

        disconnect_gamepad(&mut app, first);
        for _ in 0..32 {
            app.update();
            let world = app.world();
            assert_eq!(world.resource::<DriverInput>().0, VehicleInput::default());
            assert_eq!(world.resource::<Vehicle>().0.speed_mps(), 0.0);
        }
        let status = app.world().resource::<InputStatus>();
        let pad = status.gamepad.as_ref().expect("second pad selected");
        assert_eq!(pad.entity, second);
        assert!(!pad.ready);

        set_pad_axis(&mut app, second, GamepadAxis::RightStickX, 0.0);
        app.update();
        assert_eq!(
            app.world().resource::<DriverInput>().0,
            VehicleInput::default(),
            "left stick still pushed"
        );
        set_pad_axis(&mut app, second, GamepadAxis::LeftStickY, 0.0);
        app.update();
        set_pad_axis(&mut app, second, GamepadAxis::LeftStickY, 1.0);
        app.update();
        assert_eq!(app.world().resource::<DriverInput>().0.throttle, 1.0);
    }

    /// 대체 게임패드가 중립 대기 중이어도 비상 정지는 바로 적용되는지 확인한다.
    #[test]
    fn waiting_gamepad_can_still_emergency_stop() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let first = connect_gamepad(&mut app);
        let second = connect_gamepad(&mut app);
        set_pad_axis(&mut app, second, GamepadAxis::LeftStickY, 1.0);
        app.update();
        disconnect_gamepad(&mut app, first);
        app.update();

        set_pad_button(&mut app, second, GamepadButton::RightTrigger, 1.0);
        app.update();
        assert_eq!(
            app.world().resource::<Vehicle>().0.engine(),
            EngineState::EmergencyStop
        );
    }

    /// 같은 게임패드가 다시 연결되어도 중립 전에는 입력이 쓰이지 않는지 확인한다.
    #[test]
    fn reconnected_gamepad_needs_neutral_before_driving() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let gamepad = connect_gamepad(&mut app);
        disconnect_gamepad(&mut app, gamepad);
        app.update();
        assert_eq!(app.world().resource::<InputStatus>().gamepad, None);

        // 재연결 메시지와 밀린 스틱 값이 같은 프레임에 도착하는 경우.
        app.world_mut().write_message(GamepadConnectionEvent::new(
            gamepad,
            GamepadConnection::Connected {
                name: "Test Pad".to_owned(),
                vendor_id: None,
                product_id: None,
            },
        ));
        set_pad_axis(&mut app, gamepad, GamepadAxis::LeftStickY, 1.0);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<DriverInput>().0,
            VehicleInput::default()
        );
    }

    /// 키보드로 주행 중이면 게임패드가 끊겨도 비상 정지하지 않고 키보드 입력이 유지되는지 확인한다.
    #[test]
    fn gamepad_disconnect_while_keyboard_driving_keeps_engine_state() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let gamepad = connect_gamepad(&mut app);
        for _ in 0..64 {
            update_with_keys(&mut app, &[KeyCode::KeyW]);
        }
        disconnect_gamepad(&mut app, gamepad);
        update_with_keys(&mut app, &[KeyCode::KeyW]);
        let world = app.world();
        assert_eq!(world.resource::<Vehicle>().0.engine(), EngineState::Ready);
        assert_eq!(world.resource::<DriverInput>().0.throttle, 1.0);
    }

    /// 게임패드 비상 정지와 키보드 해제가 같은 프레임에 들어와도 비상 정지가 유지되는지 확인한다.
    #[test]
    fn gamepad_emergency_stop_beats_keyboard_release() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        let gamepad = connect_gamepad(&mut app);
        set_pad_button(&mut app, gamepad, GamepadButton::RightTrigger, 1.0);
        update_with_keys(&mut app, &[KeyCode::Backspace]);
        assert_eq!(
            app.world().resource::<Vehicle>().0.engine(),
            EngineState::EmergencyStop
        );
    }

    /// 키보드와 게임패드의 시동 입력이 같은 프레임에 겹치면 토글이 한 번만 적용되는지 확인한다.
    #[test]
    fn simultaneous_engine_toggle_applies_once() {
        let mut app = headless_app();
        let gamepad = connect_gamepad(&mut app);
        set_pad_button(&mut app, gamepad, GamepadButton::Start, 1.0);
        update_with_keys(&mut app, &[KeyCode::Enter]);
        assert_eq!(
            app.world().resource::<Vehicle>().0.engine(),
            EngineState::Ready
        );
    }

    /// 키보드 경로로 가속→최대 제동→시작 위치 복귀를 두 번 반복하면, 매번 같은 출발점에서
    /// 같은 정지거리가 측정되고 움직이는 중 복귀 키는 거부되는지 확인한다.
    #[test]
    fn brake_trial_repeats_from_start_pose() {
        let mut app = headless_app();
        engine_on_in_drive(&mut app);
        // 첫 갱신은 경과 시간이 0이라 고정 단계가 돌지 않으므로, 시험 전에 한 번 비워 둔다.
        update_with_keys(&mut app, &[]);
        let mut stops = Vec::new();
        for _ in 0..2 {
            for _ in 0..64 * 2 {
                update_with_keys(&mut app, &[KeyCode::KeyW]);
            }
            update_with_keys(&mut app, &[KeyCode::KeyW, KeyCode::KeyR]);
            let moved = app.world().resource::<VehicleMotion>().0;
            assert!(moved.z_m < -1.0, "reset must be rejected while moving");
            for _ in 0..64 {
                update_with_keys(&mut app, &[KeyCode::KeyS]);
            }
            let stop = app
                .world()
                .resource::<CalibrationLog>()
                .last_stop()
                .expect("brake trial recorded");
            stops.push((stop.from_speed_mps, stop.distance_m));
            update_with_keys(&mut app, &[]);
            update_with_keys(&mut app, &[KeyCode::KeyR]);
            let world = app.world();
            assert_eq!(world.resource::<VehicleMotion>().0, Default::default());
            assert_eq!(world.resource::<Vehicle>().0.gear(), Gear::Drive);
            update_with_keys(&mut app, &[]);
        }
        assert!(stops[0].1 > 0.3, "{stops:?}");
        assert_eq!(stops[0], stops[1]);
        let counts = app.world().resource::<CalibrationLog>().trial_counts();
        assert_eq!(counts.0, 2);
    }

    /// 같은 주행 입력을 재생할 때 시점 키를 계속 바꿔도 위치·속도·heading이 시점 전환이 없을 때와
    /// 정확히 같은지 확인한다. 시점 키는 주행·기어 상태도 바꾸지 않는다.
    #[test]
    fn view_switching_does_not_affect_vehicle() {
        let views = [
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit1,
            KeyCode::KeyC,
        ];
        let run = |switch_views: bool| {
            let mut app = headless_app_with_camera();
            engine_on_in_drive(&mut app);
            let mut seen = Vec::new();
            for frame in 0..64 * 6 {
                let mut keys = match frame {
                    0..128 => vec![KeyCode::KeyW, KeyCode::KeyD],
                    128..256 => vec![KeyCode::KeyW, KeyCode::KeyA],
                    _ => vec![KeyCode::KeyS],
                };
                if switch_views && frame % 10 < 2 {
                    keys.push(views[frame / 10 % views.len()]);
                }
                update_with_keys(&mut app, &keys);
                seen.push(*app.world().resource::<ActiveView>());
            }
            let world = app.world();
            (
                world.resource::<Vehicle>().0.clone(),
                world.resource::<VehicleMotion>().0,
                seen,
            )
        };
        let (plain_state, plain_motion, plain_views) = run(false);
        let (switched_state, switched_motion, switched_views) = run(true);
        assert_eq!(plain_state, switched_state);
        assert_eq!(plain_motion, switched_motion);
        assert!(
            plain_views
                .iter()
                .all(|view| *view == ActiveView::default())
        );
        for direction in ViewDirection::ALL {
            assert!(
                switched_views
                    .iter()
                    .any(|view| view.direction == direction),
                "{direction:?} never selected"
            );
        }
        assert!(switched_views.iter().any(|view| view.debug_external));
    }

    /// 시점 키 동시 입력은 무시되고, 시작 위치 복귀가 적용되면 전방으로, 거부되면 그대로인지 확인한다.
    #[test]
    fn view_keys_conflict_and_reset_to_front() {
        let mut app = headless_app_with_camera();
        update_with_keys(&mut app, &[KeyCode::Digit2]);
        assert_eq!(
            app.world().resource::<ActiveView>().direction,
            ViewDirection::Rear
        );
        update_with_keys(&mut app, &[]);
        update_with_keys(&mut app, &[KeyCode::Digit3, KeyCode::Digit4]);
        assert_eq!(
            app.world().resource::<ActiveView>().direction,
            ViewDirection::Rear
        );

        // 움직이는 중 복귀 요청은 거부되므로 시점도 그대로다.
        engine_on_in_drive(&mut app);
        for _ in 0..64 {
            update_with_keys(&mut app, &[KeyCode::KeyW]);
        }
        update_with_keys(&mut app, &[KeyCode::KeyW, KeyCode::KeyR]);
        assert_eq!(
            app.world().resource::<ActiveView>().direction,
            ViewDirection::Rear
        );
        for _ in 0..128 {
            update_with_keys(&mut app, &[KeyCode::KeyS]);
        }
        update_with_keys(&mut app, &[]);
        update_with_keys(&mut app, &[KeyCode::KeyR]);
        let world = app.world();
        assert_eq!(world.resource::<VehicleMotion>().0, Default::default());
        assert_eq!(
            world.resource::<ActiveView>().direction,
            ViewDirection::Front
        );
    }
}
