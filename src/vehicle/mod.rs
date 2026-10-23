//! 차량 도메인 로직(상태 머신, 이후 컨트롤러·운동 모델·안전 처리).

pub mod state;

pub use state::{
    EngineState, Gear, TransitionError, TransitionOutcome, VehicleCommand, VehicleInput,
    VehicleState,
};
