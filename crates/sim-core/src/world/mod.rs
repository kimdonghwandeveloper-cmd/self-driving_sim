//! 주행 코스 도메인 로직 중 엔진과 무관한 부분(개발 코스 정의, 교차로 면, 굽은 도로 형상).
//!
//! 화면 메시 생성은 각 엔진 쪽(Bevy 루트 앱, Godot 확장)에서 이 데이터로 한다.

pub mod course;
pub mod junction;
pub mod road_geom;

pub use course::{CourseDef, CourseId, ShapeError};
