//! 자율주행 셔틀 안전관리자 조작 훈련 시뮬레이터의 도메인 로직과 Bevy 플러그인.

pub mod config;
pub mod input;
pub mod scene;
pub mod training;
pub mod ui;
pub mod vehicle;

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
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::training::{TrainingLog, TrainingPlugin};
    use crate::vehicle::{
        DynamicsParams, EngineState, Gear, GroundRect, Obstacle, Vehicle, VehicleCommand,
        VehicleContact, VehicleMotion, VehiclePlugin,
    };

    /// 키보드 입력과 차량 시스템만 있는 창 없는 앱을 만든다. 한 번 갱신할 때마다 고정 주기 1단계가 돈다.
    fn headless_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
        configure_sim_sets(&mut app);
        app.add_plugins((input::InputPlugin, VehiclePlugin, TrainingPlugin));
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
}
