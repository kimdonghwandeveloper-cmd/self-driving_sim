//! 경로와 독립적인 도로 블록의 노면·접속부·연석 생성.
use super::course::{Area, Collider, ColliderKind, Strip, StripKind};
use super::map::LatLon;
use super::road_blocks::{BlockId, RoadBlockModel};
use super::{
    junction::{ArmInput, road_join_shape},
    map::project,
    road_geom,
};
use glam::Vec2;
use std::collections::{BTreeMap, BTreeSet};

/// 도로망 형상 규칙 판본. 재생에 영향을 주는 생성 규칙을 바꾸면 올린다.
pub const NETWORK_GEOMETRY_VERSION: &str = "road-network-v1";

/// 동일 지도에서 경로 선택과 무관하게 고정하는 생성 설정.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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
/// 노드 ID만 접속 기준으로 쓰며 일방통행·회전 제한은 물리 입구를 닫지 않는다.
/// 폭은 블록별로 유지한다. 차량 곡률 검사는 안내선 단계의 책임이며 여기서는 원본 중심선을 유지한다.
/// 현재 접속부는 절단 구간이 곧아야 한다. 맞지 않으면 원본 블록/노드를 반환하며 자동 우회하지 않는다.
pub fn build_road_network(
    model: &RoadBlockModel,
    origin: LatLon,
    settings: NetworkSettings,
) -> Result<RoadNetworkGeometry, NetworkFailure> {
    settings.validate().map_err(NetworkFailure::global)?;
    if !(origin.lat.is_finite()
        && origin.lon.is_finite()
        && (-85.0..=85.0).contains(&origin.lat)
        && (-180.0..=180.0).contains(&origin.lon))
    {
        return Err(NetworkFailure::global("invalid network origin".into()));
    }
    if model.version != super::road_blocks::BLOCK_MODEL_VERSION {
        return Err(NetworkFailure::global(
            "unsupported block model version".into(),
        ));
    }
    let mut ordered: Vec<_> = model.blocks.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ids = BTreeSet::new();
    let mut node_positions = BTreeMap::new();
    let mut lines = Vec::new();
    let mut ends: BTreeMap<u64, Vec<End>> = BTreeMap::new();
    for (index, block) in ordered.iter().enumerate() {
        let fail = |message| NetworkFailure::block(&block.id, None, message);
        if !ids.insert(&block.id)
            || block.nodes.len() < 2
            || !(block.attributes.width_m.is_finite()
                && block.attributes.width_m > 0.0
                && block.attributes.width_m <= 30.0)
        {
            return Err(fail("invalid block ID, nodes or width".into()));
        }
        let mut points = Vec::new();
        for node in &block.nodes {
            if !(node.pos.lat.is_finite()
                && node.pos.lon.is_finite()
                && (-90.0..=90.0).contains(&node.pos.lat)
                && (-180.0..=180.0).contains(&node.pos.lon))
            {
                return Err(fail(format!("node {} has invalid coordinate", node.id)));
            }
            if node_positions
                .insert(node.id, node.pos)
                .is_some_and(|pos| pos != node.pos)
            {
                return Err(fail(format!("node {} has conflicting coordinate", node.id)));
            }
            let point = project(origin, node.pos);
            if !point.is_finite() || point.abs().max_element() > 12_000.0 {
                return Err(fail(format!(
                    "node {} is outside local network extent",
                    node.id
                )));
            }
            if points
                .last()
                .is_some_and(|last: &Vec2| last.distance(point) < 1.0e-4)
            {
                return Err(fail(format!("node {} collapses in projection", node.id)));
            }
            points.push(point);
        }
        if block.closed != (block.nodes.first().unwrap().id == block.nodes.last().unwrap().id) {
            return Err(fail("inconsistent closed block".into()));
        }
        let points = road_geom::drop_collinear(&points);
        for (end, node) in [
            (0, block.nodes.first().unwrap()),
            (1, block.nodes.last().unwrap()),
        ] {
            ends.entry(node.id).or_default().push(End { index, end });
        }
        lines.push(points);
    }
    let mut cuts = vec![[0.0f32; 2]; ordered.len()];
    let mut joints = Vec::new();
    let mut colliders = Vec::new();
    for (node_id, incident) in &ends {
        if incident.len() < 2 {
            continue;
        }
        // 다른 접속로가 없는 순수 고리의 이음은 닫힌 오프셋으로 처리한다.
        if incident.len() == 2 && incident[0].index == incident[1].index {
            continue;
        }
        let center = project(origin, node_positions[node_id]);
        let arms: Vec<_> = incident
            .iter()
            .map(|end| {
                let line = &lines[end.index];
                let direction = if end.end == 0 {
                    line[1] - line[0]
                } else {
                    line[line.len() - 2] - line[line.len() - 1]
                };
                ArmInput {
                    direction: direction.normalize(),
                    half_width_m: ordered[end.index].attributes.width_m / 2.0,
                    min_cut_m: 0.0,
                    ccw_corner_radius_m: 0.0,
                }
            })
            .collect();
        let shape = road_join_shape(
            center,
            &arms,
            settings.corner_radius_m,
            settings.arc_step_deg,
            settings.min_arm_gap_deg,
        )
        .map_err(|message| NetworkFailure {
            block_id: None,
            node_id: Some(*node_id),
            message,
        })?;
        let mut openings = Vec::new();
        for (k, end) in incident.iter().enumerate() {
            let cut = shape.cut_m[k];
            cuts[end.index][end.end] = cut;
            let outward = arms[k].direction;
            let normal = road_geom::right_of(outward) * arms[k].half_width_m;
            let mouth = center + outward * cut;
            openings.push([mouth - normal, mouth + normal]);
            check_straight_end(&lines[end.index], end.end, cut).map_err(|message| {
                NetworkFailure::block(&ordered[end.index].id, Some(*node_id), message)
            })?;
        }
        for corner in &shape.corners {
            colliders.extend(
                road_geom::side_curb_rects(corner, 0.0, settings.curb_thickness_m, -1.0)
                    .into_iter()
                    .map(curb),
            );
        }
        joints.push(NetworkJoint {
            node_id: *node_id,
            area: Area {
                center,
                boundary: shape.boundary,
            },
            openings,
        });
    }
    let mut blocks = Vec::new();
    for (index, block) in ordered.iter().enumerate() {
        let fail = |message| {
            NetworkFailure::block(
                &block.id,
                if cuts[index][0] > 0.0 {
                    Some(block.nodes[0].id)
                } else if cuts[index][1] > 0.0 {
                    Some(block.nodes.last().unwrap().id)
                } else {
                    None
                },
                message,
            )
        };
        let pure_loop = block.closed && cuts[index] == [0.0, 0.0];
        let centerline = if pure_loop {
            lines[index].clone()
        } else {
            trim(&lines[index], cuts[index]).map_err(fail)?
        };
        let half = block.attributes.width_m / 2.0;
        let (left, right, rects) = if pure_loop {
            let padded = pad_loop(&centerline);
            let n = centerline.len();
            let left = road_geom::offset_line(&padded, -half)[1..=n].to_vec();
            let right = road_geom::offset_line(&padded, half)[1..=n].to_vec();
            let mut rects = Vec::new();
            for side in [-1.0, 1.0] {
                let all =
                    road_geom::side_curb_rects(&padded, half, settings.curb_thickness_m, side);
                rects.extend_from_slice(&all[1..n]);
            }
            validate_loop(&left, &right).map_err(fail)?;
            (left, right, rects)
        } else {
            road_geom::check_simple_boundary(&centerline, half + settings.curb_thickness_m)
                .map_err(fail)?;
            (
                road_geom::offset_line(&centerline, -half),
                road_geom::offset_line(&centerline, half),
                road_geom::curb_rects(
                    &centerline,
                    block.attributes.width_m,
                    settings.curb_thickness_m,
                ),
            )
        };
        if left.iter().chain(&right).any(|p| !p.is_finite()) {
            return Err(fail("non-finite road offsets".into()));
        }
        colliders.extend(rects.into_iter().map(curb));
        blocks.push(BlockSurface {
            block_id: block.id.clone(),
            centerline,
            strip: Strip {
                kind: StripKind::Road,
                left,
                right,
            },
        });
    }
    Ok(RoadNetworkGeometry {
        blocks,
        joints,
        colliders,
    })
}

/// 끝점의 원본 블록과 시작/종료 번호를 보관한다.
#[derive(Clone, Copy)]
struct End {
    index: usize,
    end: usize,
}

impl NetworkSettings {
    /// 양의 유한값과 각도·치수 상한으로 무한 분할과 잘못된 충돌 상자를 방지한다.
    pub(crate) fn validate(self) -> Result<(), String> {
        for (name, value, cap) in [
            ("corner radius", self.corner_radius_m, 30.0),
            ("curb thickness", self.curb_thickness_m, 0.49),
            ("arc step", self.arc_step_deg, 45.0),
            ("minimum arm gap", self.min_arm_gap_deg, 90.0),
        ] {
            if !(value.is_finite() && value > 0.0 && value <= cap) {
                return Err(format!("invalid network {name}"));
            }
        }
        if self.arc_step_deg < 0.5 {
            return Err("network arc step must be at least 0.5 degrees".into());
        }
        Ok(())
    }
}
impl NetworkFailure {
    /// 전체 설정 오류를 원본 블록 없는 진단으로 반환한다.
    fn global(message: String) -> Self {
        Self {
            block_id: None,
            node_id: None,
            message,
        }
    }
    /// 블록 ID와 접속 노드를 보존해 형상 생성 실패를 반환한다.
    fn block(id: &BlockId, node_id: Option<u64>, message: String) -> Self {
        Self {
            block_id: Some(id.clone()),
            node_id,
            message,
        }
    }
}
/// 연석 충돌 상자를 기존 코스 표현과 동일한 높이로 감싼다.
fn curb(rect: crate::vehicle::GroundRect) -> Collider {
    Collider {
        rect,
        height_m: 0.25,
        kind: ColliderKind::Curb,
    }
}

/// 접속부를 잘라 낼 구간이 초기 접선과 5 cm 이내로 일치하는지 확인한다.
fn check_straight_end(line: &[Vec2], end: usize, cut: f32) -> Result<(), String> {
    let points: Vec<_> = if end == 0 {
        line.to_vec()
    } else {
        line.iter().copied().rev().collect()
    };
    let direction = (points[1] - points[0]).normalize();
    let Some((at, _)) = road_geom::point_at_distance(&points, cut) else {
        return Err("block too short for joint cut".into());
    };
    if at.distance(points[0] + direction * cut) > 0.05 {
        return Err("block bends inside joint cut".into());
    }
    let mut walked = 0.0;
    for pair in points.windows(2) {
        walked += pair[0].distance(pair[1]);
        if walked >= cut {
            break;
        }
        if (pair[1] - points[0]).perp_dot(direction).abs() > 0.05 {
            return Err("block bends inside joint cut".into());
        }
    }
    Ok(())
}
/// 양끝 접속부를 호 길이로 자르고 내부의 원본 꺾임을 보존한다.
fn trim(line: &[Vec2], cuts: [f32; 2]) -> Result<Vec<Vec2>, String> {
    let length = road_geom::polyline_length(line);
    if length - cuts[0] - cuts[1] < 0.05 {
        return Err("block too short after joint cuts".into());
    }
    let start = road_geom::point_at_distance(line, cuts[0])
        .ok_or("invalid start cut")?
        .0;
    let finish = road_geom::point_at_distance(line, length - cuts[1])
        .ok_or("invalid end cut")?
        .0;
    let mut result = vec![start];
    let mut walked = 0.0;
    for pair in line.windows(2) {
        walked += pair[0].distance(pair[1]);
        if walked > cuts[0] + 1.0e-4 && walked < length - cuts[1] - 1.0e-4 {
            result.push(pair[1]);
        }
    }
    result.push(finish);
    Ok(result)
}
/// 고리 첫/끝 꼭짓점의 양쪽 접선을 얻도록 앞뒤 점을 보충한다.
fn pad_loop(line: &[Vec2]) -> Vec<Vec2> {
    let mut padded = vec![line[line.len() - 2]];
    padded.extend_from_slice(line);
    padded.push(line[1]);
    padded
}
/// 닫힌 띠의 양쪽 경계 자기 교차와 서로의 교차를 검사한다.
fn validate_loop(left: &[Vec2], right: &[Vec2]) -> Result<(), String> {
    for line in [left, right] {
        let count = line.len() - 1;
        for i in 0..count {
            for j in i + 2..count {
                if i == 0 && j == count - 1 {
                    continue;
                }
                if road_geom::segments_touch((line[i], line[i + 1]), (line[j], line[j + 1])) {
                    return Err("closed block boundary self-intersects".into());
                }
            }
        }
    }
    for a in left.windows(2) {
        for b in right.windows(2) {
            if road_geom::segments_touch((a[0], a[1]), (b[0], b[1])) {
                return Err("closed block sides intersect".into());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "road_network_tests.rs"]
mod tests;
