//! 방향 있는 도로 블록의 순서 선택·거리 기반 탐색·고정 도로망 위 차량 안내선.
use super::{
    course::CourseDef,
    map::LatLon,
    road_blocks::{BlockId, BlockTraversal, RoadBlockModel},
    road_network::{NetworkSettings, RoadNetworkGeometry},
};
use glam::Vec2;

/// 선택 순서를 보존해 확정한 전체 블록 통과 경로. 길이는 원본 중심선의 미터 합이다.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRoutePlan {
    pub selected: Vec<BlockTraversal>,
    pub traversals: Vec<BlockTraversal>,
    pub length_m: f64,
}
/// 실패가 탐색·도로망·안내선·차체 배치 중 어디에서 발생했는지 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureStage {
    Selection,
    Planning,
    Network,
    Guide,
    Placement,
}
/// 사용자가 선택한 원본을 유지하면서 실패 위치와 원인을 표시한다.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRouteFailure {
    pub stage: FailureStage,
    pub block_id: Option<BlockId>,
    pub node_id: Option<u64>,
    pub selection_index: Option<usize>,
    pub message: String,
}
/// 도로망은 경로와 독립적으로 유지하고 안내선·차량 자세만 경로에 맞춘다.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRouteCourse {
    pub plan: BlockRoutePlan,
    pub network: RoadNetworkGeometry,
    pub course: CourseDef,
    pub guide: Vec<Vec2>,
}
/// 지정 블록/방향을 순서대로 모두 통과하는 최단 미터 경로를 구한다. 같은 거리의 경로는 결정적으로 선택한다.
pub fn plan_block_route(
    _model: &RoadBlockModel,
    _selected: &[BlockTraversal],
    _max_length_m: f64,
) -> Result<BlockRoutePlan, BlockRouteFailure> {
    Err(BlockRouteFailure {
        stage: FailureStage::Planning,
        block_id: None,
        node_id: None,
        selection_index: None,
        message: "not implemented".into(),
    })
}
/// 탐색을 한 번만 수행한 후 고정 도로망 위에 차량 안내선을 만든다. 형상 실패로 재탐색하지 않는다.
pub fn build_block_route_course(
    _name: &str,
    _model: &RoadBlockModel,
    _origin: LatLon,
    _settings: NetworkSettings,
    _selected: &[BlockTraversal],
    _max_length_m: f64,
) -> Result<BlockRouteCourse, BlockRouteFailure> {
    Err(BlockRouteFailure {
        stage: FailureStage::Guide,
        block_id: None,
        node_id: None,
        selection_index: None,
        message: "not implemented".into(),
    })
}
#[cfg(test)]
#[path = "block_route_tests.rs"]
mod tests;
