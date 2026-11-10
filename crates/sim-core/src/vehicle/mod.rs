//! 차량 도메인 로직 중 엔진과 무관한 부분(상태 머신, 운동 모델, 충돌 판정).

pub mod calibration;
pub mod collision;
pub mod dynamics;
pub mod state;

pub use collision::{ContactState, GroundRect, Obstacle, VEHICLE_FOOTPRINT};
pub use dynamics::{DynamicsParams, Motion, SteerReturn};
pub use state::{
    EngineState, Gear, TransitionError, TransitionOutcome, VehicleCommand, VehicleInput,
    VehicleState,
};
