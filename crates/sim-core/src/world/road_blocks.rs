//! OSM 도로 블록 구성 계약. 1단계는 도로 면·경로 선택·주행 코드를 변경하지 않는다.

use super::osm_graph::{OsmGraphWay, OsmNode, TurnRestriction};
use super::route_plan::{RoadGraphConfig, WidthSource};
use std::collections::BTreeMap;

/// 분할·정렬 규칙 판본. 규칙이 바뀌면 저장 블록의 판본도 구분한다.
pub const BLOCK_MODEL_VERSION: &str = "osm-road-blocks-v1";

/// 원본 식별자와 정규화한 구간 순서로 만든 블록 식별자.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockId(pub String);

/// 정규화한 블록 노드 순서 기준 진행 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockDirection {
    Forward,
    Backward,
}

/// 원본 way의 인접 노드 구간. 노드 순서는 블록 중심선 방향이다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceEdge {
    pub way_id: u64,
    pub nodes: [u64; 2],
}

/// 동일 블록으로 합칠 수 있는 생성·통행 속성. 접근 정보는 제거 조건이 아니다.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockAttributes {
    pub highway: String,
    pub width_m: f32,
    pub width_source: WidthSource,
    pub tags: BTreeMap<String, String>,
}

/// 분기·속성·통행 방향 변화 사이의 연속 도로.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadBlock {
    pub id: BlockId,
    pub nodes: Vec<OsmNode>,
    pub source_edges: Vec<SourceEdge>,
    pub attributes: BlockAttributes,
    pub directions: Vec<BlockDirection>,
    pub length_m: f64,
    pub closed: bool,
}

/// 블록을 지정 방향으로 통과하는 선택 단위.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockTraversal {
    pub block_id: BlockId,
    pub direction: BlockDirection,
}

/// 공유 끝점에서 일방통행·회전 제한을 만족하는 블록 간 연결.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockConnection {
    pub via_node: u64,
    pub from: BlockTraversal,
    pub to: BlockTraversal,
}

/// 결정적으로 정렬된 블록과 연결 및 원본 회전 제한.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadBlockModel {
    pub version: &'static str,
    pub blocks: Vec<RoadBlock>,
    pub connections: Vec<BlockConnection>,
    pub restrictions: Vec<TurnRestriction>,
}

/// 블록 구성의 계약을 검증하기 위한 본체 자리. 테스트 실패 확인 후 구현한다.
pub fn build_road_blocks(
    _ways: &[OsmGraphWay],
    _restrictions: &[TurnRestriction],
    _config: &RoadGraphConfig,
) -> Result<RoadBlockModel, String> {
    Err("road block builder not implemented".to_owned())
}

#[cfg(test)]
#[path = "road_blocks_tests.rs"]
mod tests;
