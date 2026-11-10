//! 경로 선택 화면의 엔진 무관 부분(Phase GD-1c): 점·경로 계산 모델과 OSM 배경 타일 받기·캐시.
//!
//! 화면·조작은 Godot 쪽(`SimRoutePicker`·`SimTiles`)에 있다.

pub mod model;
pub mod region;
pub mod tiles;
