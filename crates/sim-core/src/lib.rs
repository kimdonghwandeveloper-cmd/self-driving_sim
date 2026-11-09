//! 엔진과 무관한 시뮬레이터 로직(Phase GD-1).
//!
//! Bevy 앱과 Godot 확장이 같은 계산을 쓰도록 렌더링·ECS에 기대지 않는 코드만 둔다.
//! `bevy` 기능을 켜면 Bevy 앱에서 쓰는 컴포넌트 derive가 붙는다.

pub mod config;
pub mod input;
pub mod session;
pub mod vehicle;
pub mod world;
