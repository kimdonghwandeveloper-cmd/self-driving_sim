//! 경로와 독립적인 도로 블록의 노면·접속부·연석 생성.
use super::course::{Area, Collider, Strip};
use super::map::LatLon;
use super::road_blocks::{BlockId, RoadBlockModel};
use glam::Vec2;

/// 동일 지도에서 경로 선택과 무관하게 고정하는 생성 설정.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetworkSettings {
    pub corner_radius_m: f32,
    pub curb_thickness_m: f32,
    pub arc_step_deg: f32,
    pub min_arm_gap_deg: f32,
}
impl Default for NetworkSettings {
    /// 교차로 반경 3 m, 연석 0.3 m의 개발 기본값을 반환한다.
    fn default() -> Self {
        Self {
            corner_radius_m: 3.0,
            curb_thickness_m: 0.3,
            arc_step_deg: 3.0,
            min_arm_gap_deg: 5.0,
        }
    }
}
/// 블록 원본과 대응하는 도로 띠 및 잘라 낸 중심선.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockSurface {
    pub block_id: BlockId,
    pub centerline: Vec<Vec2>,
    pub strip: Strip,
}
/// 공유 노드 접속부. 개구부를 가로막는 연석은 만들지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkJoint {
    pub node_id: u64,
    pub area: Area,
    pub openings: Vec<[Vec2; 2]>,
}
/// 경로 안내선·출발·도착 자세가 없는 고정 도로망 형상.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadNetworkGeometry {
    pub blocks: Vec<BlockSurface>,
    pub joints: Vec<NetworkJoint>,
    pub colliders: Vec<Collider>,
}
/// 실패 원본을 보존한다. 자동 우회나 실패 블록 제외는 하지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkFailure {
    pub block_id: Option<BlockId>,
    pub node_id: Option<u64>,
    pub message: String,
}
/// 경로를 입력받지 않고 전체 블록으로 고정 도로망을 생성한다.
pub fn build_road_network(
    _model: &RoadBlockModel,
    _origin: LatLon,
    _settings: NetworkSettings,
) -> Result<RoadNetworkGeometry, NetworkFailure> {
    Err(NetworkFailure {
        block_id: None,
        node_id: None,
        message: "not implemented".into(),
    })
}
#[cfg(test)]
#[path = "road_network_tests.rs"]
mod tests;
