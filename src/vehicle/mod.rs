//! 차량 도메인 로직(상태 머신, 컨트롤러, 이후 운동 모델·안전 처리).

pub mod controller;
pub mod state;

pub use controller::{Vehicle, VehiclePlugin};
pub use state::{
    EngineState, Gear, TransitionError, TransitionOutcome, VehicleCommand, VehicleInput,
    VehicleState,
};
