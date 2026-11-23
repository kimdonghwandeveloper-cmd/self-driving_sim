use sim_core::vehicle::dynamics::{DynamicsParams, Motion, step_on_grade};
use sim_core::vehicle::state::{Gear, VehicleCommand, VehicleInput, VehicleState};

/// ??? ???? ??????????? ??? ????.
#[test]
fn grade_changes_speed_under_equal_throttle() {
    let run = |grade| {
        let mut state = VehicleState::new(0.01);
        state.apply(VehicleCommand::ToggleEngine).unwrap();
        state
            .apply(VehicleCommand::RequestGear(Gear::Drive))
            .unwrap();
        state.set_input(VehicleInput {
            throttle: 0.5,
            ..VehicleInput::default()
        });
        let mut motion = Motion::default();
        for _ in 0..128 {
            step_on_grade(
                &mut state,
                &mut motion,
                &DynamicsParams::default(),
                1.0 / 64.0,
                grade,
            );
        }
        state.speed_mps()
    };
    assert!(run(0.07) < run(0.0));
    assert!(run(0.0) < run(-0.07));
}
