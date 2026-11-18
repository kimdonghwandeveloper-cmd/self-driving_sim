//! 경로와 독립적인 OSM 도로 블록 데이터 모델. 도로 면·차량 배치·경로 선택 화면은 이후 단계다.
//!
//! 같은 OSM 노드 ID로 연결된 인접 구간을 분기·속성·통행 방향·회전 제한 지점에서 나눈다.
//! 양방향 way의 원본 저장 방향과 입력 순서는 정규화하며, 다른 성분 추가나 좌표 수정으로 ID를 재번호화하지 않는다.
//! ID는 판본과 원본 way/node 구간 순서의 SHA-256이며 데이터 판본의 무결성 해시를 대신하지 않는다.
//! 접근 제한·좁은 폭은 속성으로 보존하고 생성 불가 판정에 사용하지 않는다. 입력의 좌표 모순은 전체 실패다.
//! 접속은 기존 via-node TurnRestriction 표현을 사용하며 블록 내부 즉시 U턴은 만들지 않는다.
//! 순수 고리는 최소 노드를 기준으로, 접속로가 있는 고리는 해당 분기점을 기준으로 정규화한다.

use super::osm_graph::{OsmGraphWay, OsmNode, TurnRestriction};
use super::road_snapshot::sha256_hex;
use super::route::{self, Travel};
use super::route_plan::resolve_width;
use super::route_plan::{RoadGraphConfig, WidthSource};
use std::collections::{BTreeMap, BTreeSet};

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

/// 원본 노드·구간·속성을 기준으로 블록을 구성하고 일방통행·회전 제한에 맞는 접속을 만든다.
/// 접근 제한은 속성으로만 보존한다. 도로 면 생성, 경로 탐색, 차량 배치 또는 자동 우회는 하지 않는다.
pub fn build_road_blocks(
    ways: &[OsmGraphWay],
    restrictions: &[TurnRestriction],
    config: &RoadGraphConfig,
) -> Result<RoadBlockModel, String> {
    config.validate()?;
    let allowed: BTreeSet<&str> = config.allowed_highways.iter().map(String::as_str).collect();
    let mut ordered = BTreeMap::new();
    let mut positions = BTreeMap::new();
    for way in ways {
        if ordered.insert(way.id, way).is_some() {
            return Err(format!("duplicate way {}", way.id));
        }
        if way.nodes.len() < 2 {
            return Err(format!("way {} needs at least two nodes", way.id));
        }
        for node in &way.nodes {
            if !(node.pos.lat.is_finite()
                && node.pos.lon.is_finite()
                && (-90.0..=90.0).contains(&node.pos.lat)
                && (-180.0..=180.0).contains(&node.pos.lon))
            {
                return Err(format!("node {} has invalid coordinate", node.id));
            }
            if positions
                .insert(node.id, *node)
                .is_some_and(|previous: OsmNode| previous.pos != node.pos)
            {
                return Err(format!("node {} has conflicting coordinate", node.id));
            }
        }
    }
    let mut edges = Vec::new();
    let mut adjacent: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let mut used_sources = BTreeSet::new();
    for way in ordered
        .values()
        .filter(|way| way.highway().is_some_and(|kind| allowed.contains(kind)))
    {
        let attributes = attributes(way, config)?;
        for pair in way.nodes.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.id == b.id || a.pos == b.pos {
                return Err(format!(
                    "way {} has degenerate edge {} -> {}",
                    way.id, a.id, b.id
                ));
            }
            if !used_sources.insert((way.id, a.id.min(b.id), a.id.max(b.id))) {
                return Err(format!(
                    "way {} has duplicate source edge {} -> {}",
                    way.id, a.id, b.id
                ));
            }
            let index = edges.len();
            adjacent.entry(a.id).or_default().push(index);
            adjacent.entry(b.id).or_default().push(index);
            edges.push(Edge {
                way_id: way.id,
                nodes: [a.id, b.id],
                attributes: attributes.clone(),
                travel: route::travel(way),
            });
        }
    }
    let mut rules = BTreeMap::new();
    for &restriction in restrictions {
        if rules.contains_key(&restriction.id) {
            return Err(format!("duplicate restriction {}", restriction.id));
        }
        let from = ordered.get(&restriction.from_way);
        let to = ordered.get(&restriction.to_way);
        let (Some(from), Some(to)) = (from, to) else {
            return Err(format!(
                "restriction {} refers to a missing way",
                restriction.id
            ));
        };
        if ![from, to]
            .iter()
            .all(|way| way.nodes.iter().any(|node| node.id == restriction.via_node))
        {
            return Err(format!(
                "restriction {} has an invalid via node {}",
                restriction.id, restriction.via_node
            ));
        }
        // 지정 출구가 허용 목록 밖이어도 관계를 보존해야 다른 출구로 잘못 진입하지 않는다.
        rules.insert(restriction.id, restriction);
    }
    let restrictions: Vec<_> = rules.into_values().collect();
    let via: BTreeSet<_> = restrictions.iter().map(|rule| rule.via_node).collect();
    let boundaries: BTreeSet<u64> = adjacent
        .iter()
        .filter_map(|(&node, incident)| {
            let boundary = incident.len() != 2
                || via.contains(&node)
                || !compatible_at(node, &edges[incident[0]], &edges[incident[1]]);
            boundary.then_some(node)
        })
        .collect();
    let mut visited = vec![false; edges.len()];
    let mut blocks = Vec::new();
    // 경계에서 시작하는 사슬을 먼저 걷는다. 경계 없는 성분만 순수 고리로 남는다.
    for &node in &boundaries {
        for &edge in &adjacent[&node] {
            if !visited[edge] {
                let path = walk(node, edge, &edges, &adjacent, &boundaries, &mut visited);
                blocks.push(make_block(path, &edges, &positions, false));
            }
        }
    }
    for edge in 0..edges.len() {
        if !visited[edge] {
            let path = walk(
                edges[edge].nodes[0],
                edge,
                &edges,
                &adjacent,
                &boundaries,
                &mut visited,
            );
            blocks.push(make_block(path, &edges, &positions, true));
        }
    }
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let connections = connections(&blocks, &restrictions);
    Ok(RoadBlockModel {
        version: BLOCK_MODEL_VERSION,
        blocks,
        connections,
        restrictions,
    })
}

/// 블록 구성 중 사용하는 원본 인접 구간. 진행 방향은 원본 노드 순서 기준이다.
struct Edge {
    way_id: u64,
    nodes: [u64; 2],
    attributes: BlockAttributes,
    travel: Travel,
}

impl Edge {
    /// 구간의 한 끝에서 반대 끝 노드 ID를 구한다.
    fn other(&self, node: u64) -> u64 {
        if self.nodes[0] == node {
            self.nodes[1]
        } else {
            self.nodes[0]
        }
    }

    /// 지정 노드에서 출발하는 방향과 그 역방향의 허용 여부를 반환한다.
    fn permissions(&self, from: u64) -> (bool, bool) {
        let forward = matches!(self.travel, Travel::Both | Travel::Forward);
        let backward = matches!(self.travel, Travel::Both | Travel::Backward);
        if from == self.nodes[0] {
            (forward, backward)
        } else {
            (backward, forward)
        }
    }
}

/// 폭은 기존 해석기를 재사용하되 차체가 들어가는지 여부는 다음 형상 단계에서 판단한다.
fn attributes(way: &OsmGraphWay, config: &RoadGraphConfig) -> Result<BlockAttributes, String> {
    let (width_m, width_source) = resolve_width(way, config)?;
    let keys = [
        "name",
        "lanes",
        "layer",
        "bridge",
        "tunnel",
        "access",
        "motor_vehicle",
        "service",
        "junction",
        "incline",
        "surface",
    ];
    let mut tags: BTreeMap<_, _> = keys
        .into_iter()
        .filter_map(|key| {
            way.tags
                .get(key)
                .map(|value| (key.to_owned(), value.clone()))
        })
        .collect();
    if route::travel(way) == Travel::Closed
        && let Some(value) = way.tags.get("oneway")
    {
        tags.insert("oneway".to_owned(), value.clone());
    }
    Ok(BlockAttributes {
        highway: way.highway().unwrap_or_default().to_owned(),
        width_m,
        width_source,
        tags,
    })
}

/// 차수 2인 지점에서 속성과 실제 진행 방향이 연속할 때만 두 구간을 합칠 수 있다.
fn compatible_at(node: u64, a: &Edge, b: &Edge) -> bool {
    a.other(node) != b.other(node)
        && a.attributes == b.attributes
        && a.permissions(a.other(node)) == b.permissions(node)
}

/// 사용하지 않은 구간부터 다음 경계 또는 닫힌 고리까지 한 번씩 걷는다.
fn walk(
    start: u64,
    first: usize,
    edges: &[Edge],
    adjacent: &BTreeMap<u64, Vec<usize>>,
    boundaries: &BTreeSet<u64>,
    visited: &mut [bool],
) -> Vec<(usize, u64)> {
    let mut path = Vec::new();
    let (mut node, mut edge) = (start, first);
    loop {
        if visited[edge] {
            break;
        }
        visited[edge] = true;
        path.push((edge, node));
        node = edges[edge].other(node);
        if boundaries.contains(&node) {
            break;
        }
        let Some(next) = adjacent[&node]
            .iter()
            .copied()
            .find(|&index| !visited[index])
        else {
            break;
        };
        edge = next;
    }
    path
}

/// 사슬 또는 고리의 반대 방향 표현을 만든다.
fn reverse_path(path: &[(usize, u64)], edges: &[Edge]) -> Vec<(usize, u64)> {
    path.iter()
        .rev()
        .map(|&(edge, from)| (edge, edges[edge].other(from)))
        .collect()
}

/// 중심선 노드 순서와 원본 way 순서로 두 표현의 정규 순서를 비교한다.
fn path_key(path: &[(usize, u64)], edges: &[Edge]) -> (Vec<u64>, Vec<u64>) {
    let mut nodes: Vec<_> = path.iter().map(|&(_, node)| node).collect();
    let &(edge, from) = path.last().expect("path has an edge");
    nodes.push(edges[edge].other(from));
    (
        nodes,
        path.iter().map(|&(edge, _)| edges[edge].way_id).collect(),
    )
}

/// 순수 고리는 최소 노드에서 시작하고 사슬은 양 끝 중 사전 순으로 작은 노드 순서를 선택한다.
fn canonical_path(
    mut path: Vec<(usize, u64)>,
    edges: &[Edge],
    pure_cycle: bool,
) -> Vec<(usize, u64)> {
    if pure_cycle {
        let minimum = path
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, node))| *node)
            .map(|(index, _)| index)
            .unwrap_or(0);
        path.rotate_left(minimum);
    }
    let reversed = reverse_path(&path, edges);
    if path_key(&reversed, edges) < path_key(&path, edges) {
        reversed
    } else {
        path
    }
}

/// 정규화한 원본 구간 목록으로 블록 ID·중심선·방향·길이를 결정한다.
fn make_block(
    path: Vec<(usize, u64)>,
    edges: &[Edge],
    positions: &BTreeMap<u64, OsmNode>,
    pure_cycle: bool,
) -> RoadBlock {
    let path = canonical_path(path, edges, pure_cycle);
    let (ids, _) = path_key(&path, edges);
    let nodes: Vec<_> = ids.iter().map(|id| positions[id]).collect();
    let source_edges: Vec<_> = path
        .iter()
        .map(|&(edge, from)| SourceEdge {
            way_id: edges[edge].way_id,
            nodes: [from, edges[edge].other(from)],
        })
        .collect();
    let mut bytes = BLOCK_MODEL_VERSION.as_bytes().to_vec();
    for source in &source_edges {
        bytes.extend_from_slice(&source.way_id.to_le_bytes());
        for node in source.nodes {
            bytes.extend_from_slice(&node.to_le_bytes());
        }
    }
    let first = &edges[path[0].0];
    let (forward, backward) = first.permissions(path[0].1);
    let mut directions = Vec::new();
    if forward {
        directions.push(BlockDirection::Forward);
    }
    if backward {
        directions.push(BlockDirection::Backward);
    }
    let length_m = nodes
        .windows(2)
        .map(|pair| {
            let a = pair[0].pos;
            let b = pair[1].pos;
            let north = (b.lat - a.lat).to_radians();
            let east = (b.lon - a.lon).to_radians() * ((a.lat + b.lat) * 0.5).to_radians().cos();
            6_371_008.8 * north.hypot(east)
        })
        .sum();
    RoadBlock {
        id: BlockId(format!("{}:{}", BLOCK_MODEL_VERSION, sha256_hex(&bytes))),
        closed: ids.first() == ids.last(),
        nodes,
        source_edges,
        attributes: first.attributes.clone(),
        directions,
        length_m,
    }
}

/// 접속 후보의 입구·출구 노드와 원본 way를 방향별로 보관한다.
struct TraversalEnds {
    traversal: BlockTraversal,
    entry: u64,
    exit: u64,
    first_way: u64,
    last_way: u64,
    closed: bool,
}

/// 블록별 허용 방향을 끝점 접속용 표현으로 변환한다.
fn traversal_ends(block: &RoadBlock) -> Vec<TraversalEnds> {
    block
        .directions
        .iter()
        .map(|&direction| {
            let first = &block.source_edges[0];
            let last = block.source_edges.last().expect("block has an edge");
            let (entry, exit, first_way, last_way) = match direction {
                BlockDirection::Forward => {
                    (first.nodes[0], last.nodes[1], first.way_id, last.way_id)
                }
                BlockDirection::Backward => {
                    (last.nodes[1], first.nodes[0], last.way_id, first.way_id)
                }
            };
            TraversalEnds {
                traversal: BlockTraversal {
                    block_id: block.id.clone(),
                    direction,
                },
                entry,
                exit,
                first_way,
                last_way,
                closed: block.closed,
            }
        })
        .collect()
}

/// 공유 끝점의 연결만 열거하고 금지·지정 회전과 블록 내 즉시 U턴을 제외한다.
fn connections(blocks: &[RoadBlock], restrictions: &[TurnRestriction]) -> Vec<BlockConnection> {
    let traversals: Vec<_> = blocks.iter().flat_map(traversal_ends).collect();
    let mut entries: BTreeMap<u64, Vec<&TraversalEnds>> = BTreeMap::new();
    let mut rules: BTreeMap<(u64, u64), Vec<&TurnRestriction>> = BTreeMap::new();
    for traversal in &traversals {
        entries.entry(traversal.entry).or_default().push(traversal);
    }
    for rule in restrictions {
        rules
            .entry((rule.via_node, rule.from_way))
            .or_default()
            .push(rule);
    }
    let mut result = BTreeSet::new();
    for from in &traversals {
        for &to in entries.get(&from.exit).into_iter().flatten() {
            if from.traversal.block_id == to.traversal.block_id
                && !(from.closed && from.traversal.direction == to.traversal.direction)
            {
                continue;
            }
            let applicable = rules
                .get(&(from.exit, from.last_way))
                .map(Vec::as_slice)
                .unwrap_or_default();
            if applicable
                .iter()
                .any(|rule| !rule.only && rule.to_way == to.first_way)
            {
                continue;
            }
            if applicable.iter().any(|rule| rule.only)
                && !applicable
                    .iter()
                    .any(|rule| rule.only && rule.to_way == to.first_way)
            {
                continue;
            }
            result.insert(BlockConnection {
                via_node: from.exit,
                from: from.traversal.clone(),
                to: to.traversal.clone(),
            });
        }
    }
    result.into_iter().collect()
}

#[cfg(test)]
#[path = "road_blocks_tests.rs"]
mod tests;
