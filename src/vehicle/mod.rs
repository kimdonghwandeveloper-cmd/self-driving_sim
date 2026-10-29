//! 차량 도메인 로직(상태 머신, 컨트롤러, 운동 모델, 충돌 판정).

pub mod collision;
pub mod controller;
pub mod dynamics;
pub mod state;

pub use collision::{ContactState, GroundRect, Obstacle, VEHICLE_FOOTPRINT};
pub use controller::{
    CollisionEvent, Vehicle, VehicleContact, VehicleDynamics, VehicleMotion, VehiclePlugin,
};
pub use dynamics::{DynamicsParams, Motion, SteerReturn};
pub use state::{
    EngineState, Gear, TransitionError, TransitionOutcome, VehicleCommand, VehicleInput,
    VehicleState,
};
