//! 자율주행 셔틀 안전관리자 조작 훈련 시뮬레이터의 도메인 로직과 Bevy 플러그인.

pub mod config;
pub mod input;
pub mod ui;
pub mod vehicle;

use bevy::prelude::*;

/// 프레임 안의 처리 순서. 입력 → 차량 제어 → 화면 표시 순으로 실행한다.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Input,
    Control,
    Display,
}
