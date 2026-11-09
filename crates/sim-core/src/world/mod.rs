//! 주행 코스 도메인 로직 중 엔진과 무관한 부분(개발 코스 정의, 교차로 면, 굽은 도로 형상,
//! 지도 코스·OSM 그래프·지역 받기·스냅샷·경로 계산, Geofabrik PBF 추출·대조).
//!
//! 화면 메시 생성은 각 엔진 쪽(Bevy 루트 앱, Godot 확장)에서 이 데이터로 한다.

pub mod course;
pub mod course_files;
pub mod junction;
pub mod map;
pub mod map_fetch;
pub mod osm_graph;
pub mod pbf_audit;
pub mod pbf_compare;
pub mod pbf_extract;
#[cfg(test)]
pub(crate) mod pbf_fixture;
pub mod road_geom;
pub mod road_snapshot;
pub mod route;
pub mod route_plan;
#[cfg(test)]
pub mod test_drive;

pub use course::{CourseDef, CourseId, ShapeError};
