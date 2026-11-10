//! 엔진과 무관한 시뮬레이터 로직(Phase GD-1).
//!
//! Godot 확장(`sim-godot`)과 개발 명령(`sim-tools`)이 같은 계산을 쓰도록 렌더링에 기대지 않는 코드만 둔다.
//! (GD-5에서 Bevy 앱을 지웠다.)

pub mod camera;
pub mod config;
pub mod input;
pub mod picker;
pub mod scenario;
pub mod session;
pub mod vehicle;
pub mod world;
