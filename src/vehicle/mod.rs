//! 차량 도메인 로직(상태 머신, 컨트롤러, 운동 모델, 충돌 판정).
//!
//! 상태 머신·운동 모델·충돌 판정은 엔진과 무관해 `sim-core`로 옮겼다(Phase GD-1a).
//! 기존 경로(`crate::vehicle::state` 등)는 그대로 쓸 수 있게 다시 내보낸다.

pub mod controller;

pub use sim_core::vehicle::{collision, dynamics, state};

pub use collision::{ContactState, GroundRect, Obstacle, VEHICLE_FOOTPRINT};
pub use controller::{
    CollisionEvent, Vehicle, VehicleContact, VehicleDynamics, VehicleMotion, VehiclePlugin,
};
pub use dynamics::{DynamicsParams, Motion, SteerReturn};
pub use state::{
    EngineState, Gear, TransitionError, TransitionOutcome, VehicleCommand, VehicleInput,
    VehicleState,
};
