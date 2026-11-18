//! 고정 스냅샷과 확정 블록 경로의 저장·오프라인 재생. 기존 route-v1은 별도 로더로 유지한다.
use super::{
    block_route::BlockRouteCourse, map::LatLon, road_blocks::BlockTraversal,
    road_network::NetworkSettings, route_plan::RoadGraphConfig,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
/// 블록 경로 파일 판본.
pub const BLOCK_ROUTE_FORMAT: &str = "block-route-v1";
/// 상대 경로와 원문 바이트 해시로 고정한 입력 파일.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedFile {
    pub path: String,
    pub sha256: String,
}
/// 블록 분할·폭 산정에 사용하는 설정만 저장한다. 옛 점 스냅 옵션은 새 경로에 사용하지 않는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockBuildSettings {
    pub allowed_highways: Vec<String>,
    pub lane_width_m: f32,
    pub default_width_m: BTreeMap<String, f32>,
}
/// 원본과 생성 정책·실제 통과 순서·결과 해시를 고정한 저장 파일.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRouteFile {
    pub format: String,
    pub name: String,
    pub block_model_version: String,
    pub network_version: String,
    pub guide_version: String,
    pub origin: [f64; 2],
    pub max_route_length_m: f64,
    pub blocks: BlockBuildSettings,
    pub network: NetworkSettings,
    pub snapshot: LockedFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LockedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connectivity: Option<LockedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub places: Option<LockedFile>,
    pub selected: Vec<BlockTraversal>,
    pub traversals: Vec<BlockTraversal>,
    pub length_m: f64,
    pub result_sha256: String,
}
/// 새 저장 파일을 만들 때 필요한 명시 입력. 실행 폴더나 전역 설정을 추측하지 않는다.
pub struct BlockRouteRequest<'a> {
    pub name: &'a str,
    pub snapshot: &'a str,
    pub origin: LatLon,
    pub graph: &'a RoadGraphConfig,
    pub network: NetworkSettings,
    pub selected: &'a [BlockTraversal],
}
/// 자료를 읽어 새 경로를 탐색하고 재생에 필요한 참조와 결과를 함께 준비한다.
pub fn prepare_block_route_file(
    _base_dir: &Path,
    _request: &BlockRouteRequest<'_>,
) -> Result<(BlockRouteFile, BlockRouteCourse), String> {
    Err("not implemented".into())
}
/// 저장 파일의 문법과 지원 판본·설정·참조 형식을 검사한다.
pub fn parse_block_route_file(_text: &str) -> Result<BlockRouteFile, String> {
    Err("not implemented".into())
}
/// 저장 파일을 결정적인 TOML 바이트 순서로 직렬화한다.
pub fn block_route_file_toml(_file: &BlockRouteFile) -> Result<String, String> {
    Err("not implemented".into())
}
/// 저장된 실제 통과 순서로만 재생하며 새 최단 경로를 찾지 않는다.
pub fn replay_block_route_file(
    _file: &BlockRouteFile,
    _base_dir: &Path,
) -> Result<BlockRouteCourse, String> {
    Err("not implemented".into())
}
/// 파일 기준 상대 경로를 해석해 오프라인으로 코스를 재생한다.
pub fn load_block_route_file(_path: &Path) -> Result<BlockRouteCourse, String> {
    Err("not implemented".into())
}
/// 재생 검증을 통과한 파일만 새 이름으로 저장한다. 기존 파일은 덮어쓰지 않는다.
pub fn save_block_route_file(_path: &Path, _file: &BlockRouteFile) -> Result<(), String> {
    Err("not implemented".into())
}
#[cfg(test)]
#[path = "block_route_file_tests.rs"]
mod tests;
