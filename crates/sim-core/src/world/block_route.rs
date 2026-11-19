//! 방향 있는 도로 블록의 순서 선택·거리 기반 탐색·고정 도로망 위 차량 안내선.
use super::{
    course::CourseDef,
    map::LatLon,
    road_blocks::{BlockId, BlockTraversal, RoadBlockModel},
    road_network::{NetworkSettings, RoadNetworkGeometry},
};
use super::{
    map,
    road_blocks::{BlockDirection, RoadBlock},
    road_geom,
};
use crate::vehicle::{Motion, VEHICLE_FOOTPRINT, collision};
use glam::Vec2;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};

/// 차량 기본 치수·운동 모델·안내선 생성/검증 규칙의 재생 판본. 관련 동작 변경 시 올린다.
pub const BLOCK_GUIDE_VERSION: &str = "block-guide-v1";

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
/// 지정 블록/방향을 순서대로 모두 통과하는 최단 미터 경로를 구한다.
/// 동일 비용은 정렬된 블록 ID/방향과 탐색 순서로 결정한다. 접근 제한 태그로 블록을 제외하지 않는다.
/// 연속된 같은 방향/블록 중복 선택은 거부하며, 단일 블록 선택은 허용한다.
pub fn plan_block_route(
    model: &RoadBlockModel,
    selected: &[BlockTraversal],
    max_length_m: f64,
) -> Result<BlockRoutePlan, BlockRouteFailure> {
    let invalid =
        |message| BlockRouteFailure::new(FailureStage::Selection, None, None, None, message);
    if !(max_length_m.is_finite()
        && max_length_m > 0.0
        && max_length_m <= super::route_plan::ROUTE_LENGTH_CAP_M)
    {
        return Err(invalid(
            "maximum route length must be positive and at most 10000 m".into(),
        ));
    }
    if selected.is_empty() {
        return Err(invalid("select at least one road block".into()));
    }
    if model.version != super::road_blocks::BLOCK_MODEL_VERSION {
        return Err(invalid("unsupported road block model version".into()));
    }
    let mut blocks = BTreeMap::new();
    for b in &model.blocks {
        if blocks.insert(b.id.clone(), b).is_some()
            || b.nodes.len() < 2
            || !(b.length_m.is_finite() && b.length_m > 0.0)
        {
            return Err(invalid(format!("invalid block {} or length", b.id.0)));
        }
    }
    for (index, t) in selected.iter().enumerate() {
        let fail = |message| {
            BlockRouteFailure::new(
                FailureStage::Selection,
                Some(t.block_id.clone()),
                None,
                Some(index),
                message,
            )
        };
        let b = blocks
            .get(&t.block_id)
            .ok_or_else(|| fail("selected block not found".into()))?;
        if !b.directions.contains(&t.direction) {
            return Err(fail("selected direction is not allowed".into()));
        }
        if index > 0 && selected[index - 1] == *t {
            return Err(fail("consecutive duplicate block selection".into()));
        }
    }
    let mut adjacent: BTreeMap<BlockTraversal, BTreeSet<BlockTraversal>> = BTreeMap::new();
    for c in &model.connections {
        let fail = || invalid(format!("invalid block connection at node {}", c.via_node));
        let from = blocks.get(&c.from.block_id).ok_or_else(fail)?;
        let to = blocks.get(&c.to.block_id).ok_or_else(fail)?;
        if !from.directions.contains(&c.from.direction)
            || !to.directions.contains(&c.to.direction)
            || traversal_nodes(from, c.from.direction).1 != c.via_node
            || traversal_nodes(to, c.to.direction).0 != c.via_node
        {
            return Err(fail());
        }
        adjacent
            .entry(c.from.clone())
            .or_default()
            .insert(c.to.clone());
    }
    let mut traversals = vec![selected[0].clone()];
    let mut length_m = blocks[&selected[0].block_id].length_m;
    if length_m > max_length_m {
        return Err(BlockRouteFailure::new(
            FailureStage::Planning,
            Some(selected[0].block_id.clone()),
            None,
            Some(0),
            "selected start block exceeds route length limit".into(),
        ));
    }
    for (index, pair) in selected.windows(2).enumerate() {
        let fail = || {
            BlockRouteFailure::new(
                FailureStage::Planning,
                Some(pair[1].block_id.clone()),
                None,
                Some(index + 1),
                "selected block is unreachable within route length limit".into(),
            )
        };
        let (path, cost) = shortest_leg(
            &adjacent,
            &blocks,
            &pair[0],
            &pair[1],
            max_length_m - length_m,
        )
        .ok_or_else(fail)?;
        length_m += cost;
        traversals.extend(path.into_iter().skip(1));
    }
    // 경유 구간별 묶음 합산과 재생 시 순차 합산의 부동소수점 차이를 없앤다.
    length_m = traversals
        .iter()
        .map(|t| blocks[&t.block_id].length_m)
        .sum();
    if !length_m.is_finite() || length_m > max_length_m {
        return Err(BlockRouteFailure::new(
            FailureStage::Planning,
            Some(selected.last().unwrap().block_id.clone()),
            None,
            Some(selected.len() - 1),
            "resolved route exceeds length limit".into(),
        ));
    }
    Ok(BlockRoutePlan {
        selected: selected.to_vec(),
        traversals,
        length_m,
    })
}

/// 탐색을 한 번만 수행한 후 고정 도로망 위에 차량 안내선을 만든다. 형상 실패로 재탐색하지 않는다.
/// 순수 데이터 단계이며 기존 점 선택 UI/route-v1 저장 파일에는 아직 연결하지 않는다.
pub fn build_block_route_course(
    name: &str,
    model: &RoadBlockModel,
    origin: LatLon,
    settings: NetworkSettings,
    selected: &[BlockTraversal],
    max_length_m: f64,
) -> Result<BlockRouteCourse, BlockRouteFailure> {
    let plan = plan_block_route(model, selected, max_length_m)?;
    build_resolved_block_route_course(name, model, origin, settings, plan, max_length_m)
}

/// 저장된 확정 순서를 검사해 코스를 복원한다. Dijkstra나 경로 대체를 실행하지 않는다.
pub(crate) fn build_resolved_block_route_course(
    name: &str,
    model: &RoadBlockModel,
    origin: LatLon,
    settings: NetworkSettings,
    plan: BlockRoutePlan,
    max_length_m: f64,
) -> Result<BlockRouteCourse, BlockRouteFailure> {
    validate_resolved_plan(model, &plan, max_length_m)?;
    let network =
        super::road_network::build_road_network(model, origin, settings).map_err(|e| {
            BlockRouteFailure::new(
                FailureStage::Network,
                e.block_id,
                e.node_id,
                None,
                e.message,
            )
        })?;
    let source: BTreeMap<_, _> = model.blocks.iter().map(|b| (b.id.clone(), b)).collect();
    let shapes: BTreeMap<_, _> = network
        .blocks
        .iter()
        .map(|b| (b.block_id.clone(), b))
        .collect();
    let mut tagged: Vec<GuidePoint> = Vec::new();
    let mut block_spans = Vec::new();
    for (index, t) in plan.traversals.iter().enumerate() {
        let b = source[&t.block_id];
        let fail = |stage, node, message| {
            BlockRouteFailure::new(stage, Some(t.block_id.clone()), node, None, message)
        };
        if b.attributes.width_m < VEHICLE_FOOTPRINT.width_m + 2.0 * collision::CONTACT_MARGIN_M {
            return Err(fail(
                FailureStage::Placement,
                None,
                "road block is narrower than vehicle footprint and contact margin".into(),
            ));
        }
        let mut line = shapes[&t.block_id].centerline.clone();
        if t.direction == BlockDirection::Backward {
            line.reverse();
        }
        let spec = road_geom::RoadSpec {
            width_m: b.attributes.width_m,
            curb_thickness_m: settings.curb_thickness_m,
            vehicle_min_radius_m: map::vehicle_min_radius_m(),
            arc_step_deg: settings.arc_step_deg,
        };
        let mut line = road_geom::build_geometry(&line, &spec)
            .map_err(|message| fail(FailureStage::Guide, None, message))?
            .centerline;
        if index > 0 {
            let previous = &plan.traversals[index - 1];
            let connection = model
                .connections
                .iter()
                .find(|c| c.from == *previous && c.to == *t)
                .expect("validated route connection");
            let joint = network
                .joints
                .iter()
                .find(|j| j.node_id == connection.via_node);
            // 순수 고리의 반복은 같은 방향으로 닫힌 중심선을 다시 따라간다.
            let center = joint.map(|j| j.center).unwrap_or(line[0]);
            extend_turn_entry(&mut tagged, center, &mut line);
            let n = tagged.len();
            let from = tagged[n - 1].point;
            let before = tagged[n - 2].point;
            if from.distance(line[0]) > 1.0e-4 {
                let joined = join_guide(from, before, center, &line).map_err(|message| {
                    BlockRouteFailure::new(
                        FailureStage::Guide,
                        Some(previous.block_id.clone()),
                        Some(connection.via_node),
                        None,
                        message,
                    )
                })?;
                for point in joined {
                    append(
                        &mut tagged,
                        point,
                        &previous.block_id,
                        Some(connection.via_node),
                    );
                }
            }
        }
        let begin = tagged.len().saturating_sub(1);
        for point in line {
            append(&mut tagged, point, &t.block_id, None);
        }
        block_spans.push((begin, tagged.len() - 1));
    }
    validate_curvature(&tagged)?;
    let guide: Vec<_> = tagged.iter().map(|p| p.point).collect();
    let first = &plan.traversals[0].block_id;
    let last = &plan.traversals.last().unwrap().block_id;
    let placement = |id: &BlockId, message| {
        BlockRouteFailure::new(
            FailureStage::Placement,
            Some(id.clone()),
            None,
            None,
            message,
        )
    };
    let total = road_geom::polyline_length(&guide);
    // 출발 후륜축 5.7 m와 도착 전방 여유 8.5 m가 역전되지 않게 한다.
    if total < 15.0 {
        return Err(placement(
            first,
            "route is too short for separate start and goal poses".into(),
        ));
    }
    let start_distance =
        5.0 + VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    let goal_distance =
        total - 5.0 - VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    if road_geom::polyline_length(&guide[..=block_spans[0].1]) <= start_distance + 0.01 {
        return Err(placement(
            first,
            "start pose would leave the selected start block".into(),
        ));
    }
    if road_geom::polyline_length(&guide[..=block_spans.last().unwrap().0]) >= goal_distance - 0.01
    {
        return Err(placement(
            last,
            "goal pose would precede the selected goal block".into(),
        ));
    }
    let start = map::start_pose(&guide).map_err(|m| placement(first, m))?;
    let goal = map::goal_pose(&guide).map_err(|m| placement(last, m))?;
    let course = map::finish_course(
        name,
        vec![
            map::OSM_ATTRIBUTION.into(),
            "Road widths may use simulator defaults; block routes are for simulation.".into(),
        ],
        start,
        network.blocks.iter().map(|b| b.strip.clone()).collect(),
        network
            .joints
            .iter()
            .flat_map(|j| j.areas.clone())
            .collect(),
        network.colliders.clone(),
        (12_100.0, 12_100.0),
    )
    .map_err(|m| placement(first, m))?;
    let mut course = course;
    course.goal = Some(goal);
    course.notices.extend(model.diagnostics.iter().cloned());
    course
        .validate()
        .map_err(|m| placement(first, m.to_string()))?;
    validate_vehicle(&tagged, &course)?;
    Ok(BlockRouteCourse {
        plan,
        network,
        course,
        guide,
    })
}

/// 고정 경로의 방향·공유 접속·선택 순서·총 길이를 검사하며 최단 경로를 다시 계산하지 않는다.
fn validate_resolved_plan(
    model: &RoadBlockModel,
    plan: &BlockRoutePlan,
    limit: f64,
) -> Result<(), BlockRouteFailure> {
    let fail =
        |id, message| BlockRouteFailure::new(FailureStage::Planning, id, None, None, message);
    if model.version != super::road_blocks::BLOCK_MODEL_VERSION
        || !(limit.is_finite() && limit > 0.0 && limit <= super::route_plan::ROUTE_LENGTH_CAP_M)
        || plan.selected.is_empty()
        || plan.traversals.is_empty()
    {
        return Err(fail(None, "invalid resolved route or version".into()));
    }
    if plan.selected.first() != plan.traversals.first()
        || plan.selected.last() != plan.traversals.last()
        || (plan.selected.len() == 1 && plan.traversals.len() != 1)
        || plan.selected.windows(2).any(|p| p[0] == p[1])
    {
        return Err(fail(
            None,
            "resolved route does not match selected endpoint blocks".into(),
        ));
    }
    let blocks: BTreeMap<_, _> = model.blocks.iter().map(|b| (&b.id, b)).collect();
    if blocks.len() != model.blocks.len() {
        return Err(fail(None, "duplicate block ID".into()));
    }
    let mut length = 0.0;
    for t in &plan.traversals {
        let b = blocks
            .get(&t.block_id)
            .ok_or_else(|| fail(Some(t.block_id.clone()), "stored block not found".into()))?;
        if b.nodes.len() < 2
            || !b.directions.contains(&t.direction)
            || !(b.length_m.is_finite() && b.length_m > 0.0)
        {
            return Err(fail(
                Some(t.block_id.clone()),
                "invalid stored block direction or length".into(),
            ));
        }
        length += b.length_m;
    }
    for pair in plan.traversals.windows(2) {
        let from = blocks[&pair[0].block_id];
        let to = blocks[&pair[1].block_id];
        let via = traversal_nodes(from, pair[0].direction).1;
        if traversal_nodes(to, pair[1].direction).0 != via
            || !model
                .connections
                .iter()
                .any(|c| c.from == pair[0] && c.to == pair[1] && c.via_node == via)
        {
            return Err(fail(
                Some(pair[0].block_id.clone()),
                format!("stored connection at node {via} is not allowed"),
            ));
        }
    }
    let mut next = 0;
    for selected in &plan.selected {
        let found = plan.traversals[next..]
            .iter()
            .position(|t| t == selected)
            .ok_or_else(|| {
                fail(
                    Some(selected.block_id.clone()),
                    "stored route omits an ordered selection".into(),
                )
            })?;
        next += found + 1;
    }
    if !length.is_finite() || length > limit || length.to_bits() != plan.length_m.to_bits() {
        return Err(fail(
            None,
            "stored route length mismatch or limit exceeded".into(),
        ));
    }
    Ok(())
}

impl BlockRouteFailure {
    /// 실패 단계와 원본 선택/접속 식별자를 보존한다.
    fn new(
        stage: FailureStage,
        block_id: Option<BlockId>,
        node_id: Option<u64>,
        selection_index: Option<usize>,
        message: String,
    ) -> Self {
        Self {
            stage,
            block_id,
            node_id,
            selection_index,
            message,
        }
    }
}
/// 지정 방향으로 통과할 때의 시작/끝 노드를 반환한다.
fn traversal_nodes(b: &RoadBlock, d: BlockDirection) -> (u64, u64) {
    let pair = (b.nodes[0].id, b.nodes.last().unwrap().id);
    if d == BlockDirection::Forward {
        pair
    } else {
        (pair.1, pair.0)
    }
}
/// 도착 블록 길이를 간선 비용으로 더하는 Dijkstra. 양의 유한 비용만 받아 길이 제한으로 탐색을 제한한다.
fn shortest_leg(
    adjacent: &BTreeMap<BlockTraversal, BTreeSet<BlockTraversal>>,
    blocks: &BTreeMap<BlockId, &RoadBlock>,
    start: &BlockTraversal,
    goal: &BlockTraversal,
    budget: f64,
) -> Option<(Vec<BlockTraversal>, f64)> {
    let mut queue = BinaryHeap::new();
    let mut best = BTreeMap::new();
    // 양의 f64 비트는 정수 순서와 같아 별도 부동소수점 의존성 없이 정렬할 수 있다.
    queue.push(Reverse((0u64, vec![start.clone()])));
    best.insert(start.clone(), 0.0);
    while let Some(Reverse((bits, path))) = queue.pop() {
        let cost = f64::from_bits(bits);
        let current = path.last().unwrap();
        if cost > best[current] {
            continue;
        }
        if current == goal {
            return Some((path, cost));
        }
        for next in adjacent.get(current).into_iter().flatten() {
            let candidate = cost + blocks[&next.block_id].length_m;
            if candidate > budget || best.get(next).is_some_and(|known| *known <= candidate) {
                continue;
            }
            best.insert(next.clone(), candidate);
            let mut next_path = path.clone();
            next_path.push(next.clone());
            queue.push(Reverse((candidate.to_bits(), next_path)));
        }
    }
    None
}
/// 안내선 각 점에 블록·접속점 문맥을 붙여 이후 곡률/차체 오류가 원본을 가리키게 한다.
#[derive(Clone)]
struct GuidePoint {
    point: Vec2,
    block_id: BlockId,
    node_id: Option<u64>,
}
/// 원호 접점 중복은 제거하되 새 점의 원본 문맥은 보존한다.
fn append(points: &mut Vec<GuidePoint>, point: Vec2, id: &BlockId, node_id: Option<u64>) {
    if points
        .last()
        .is_none_or(|p| p.point.distance(point) > 1.0e-4)
    {
        points.push(GuidePoint {
            point,
            block_id: id.clone(),
            node_id,
        });
    }
}
/// 기존 도로 끝의 직선 구간 안에서만 원호 진입을 앞당겨 고속 추종 시의 조향 여유를 확보한다.
/// 도로 면·연석·선택 블록은 수정하지 않으며 남은 직선 길이를 넘지 않는다.
fn extend_turn_entry(points: &mut [GuidePoint], center: Vec2, next: &mut [Vec2]) {
    let n = points.len();
    let from = points[n - 1].point;
    let before = points[n - 2].point;
    let entry = (from - before).normalize();
    let exit = (next[1] - next[0]).normalize();
    let turn = entry.angle_to(exit).abs();
    if !(1.0e-3..170.0f32.to_radians()).contains(&turn) {
        return;
    }
    let factor = (turn / 2.0).tan();
    let old = from.distance(center).min(next[0].distance(center));
    // 최소 반경 미달은 원래 접속점 오류로 보존한다. 여유 확장은 정상 접속의 동적 추종 보정이다.
    if old / factor + 0.01 < map::vehicle_min_radius_m() {
        return;
    }
    let desired = (map::vehicle_min_radius_m() * 1.7 * factor).max(old);
    let extra = (desired - old)
        .min((from.distance(before) - 0.5).max(0.0))
        .min((next[1].distance(next[0]) - 0.5).max(0.0));
    points[n - 1].point -= entry * extra;
    next[0] += exit * extra;
}

/// 고정된 교차로 절단 거리 안에서 차량 최소 반경을 만족하는 접선 원호를 만든다.
fn join_guide(from: Vec2, before: Vec2, center: Vec2, next: &[Vec2]) -> Result<Vec<Vec2>, String> {
    let entry = (from - before).normalize();
    let exit = (next[1] - next[0]).normalize();
    let turn = entry.angle_to(exit).abs();
    if turn >= 170.0f32.to_radians() {
        return Err("joint turn is too close to a U-turn".into());
    }
    if turn > 1.0e-3 {
        let radius = from.distance(center).min(next[0].distance(center)) / (turn / 2.0).tan();
        if radius + 0.01 < map::vehicle_min_radius_m() {
            return Err(format!(
                "joint radius {radius:.2} m is below vehicle minimum {:.2} m",
                map::vehicle_min_radius_m()
            ));
        }
    }
    Ok(super::route::crossing_guide(from, before, center, next))
}
/// 세 점의 외접원으로 곡률을 검사하고 차량 최소 반경 미달 위치를 원본 블록과 반환한다.
fn validate_curvature(points: &[GuidePoint]) -> Result<(), BlockRouteFailure> {
    let minimum = map::vehicle_min_radius_m();
    for triple in points.windows(3) {
        let a = triple[1].point - triple[0].point;
        let b = triple[2].point - triple[1].point;
        let twice = a.perp_dot(b).abs() * 2.0;
        let product = a.length() * b.length() * (a + b).length();
        if product <= 1.0e-8 || twice * minimum > product * 1.01 {
            let p = &triple[1];
            return Err(BlockRouteFailure::new(
                FailureStage::Guide,
                Some(p.block_id.clone()),
                p.node_id,
                None,
                "guide exceeds vehicle curvature limit".into(),
            ));
        }
    }
    Ok(())
}
/// 점에서 선분까지의 제곱 거리로 충돌 상자 후보를 보수적으로 추린다.
fn distance_squared(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance_squared(a + ab * t)
}
/// 0.5 m 이하 안내선 표본으로 자세를 만들고 실제 차체의 연속 충돌 검사를 수행한다.
/// 조향기의 동적 오차까지 보장하지 않으므로 별도의 실제 운동 모델 회귀 시험도 유지한다.
fn validate_vehicle(tagged: &[GuidePoint], course: &CourseDef) -> Result<(), BlockRouteFailure> {
    let rects = course.collision_rects();
    let mut samples = vec![(tagged[0].clone(), 0.0f32)];
    let mut length = 0.0;
    for pair in tagged.windows(2) {
        let segment = pair[0].point.distance(pair[1].point);
        let pieces = (segment / 0.5).ceil().max(1.0) as usize;
        for k in 1..=pieces {
            let t = k as f32 / pieces as f32;
            let mut p = pair[1].clone();
            p.point = pair[0].point.lerp(pair[1].point, t);
            samples.push((p, length + segment * t));
        }
        length += segment;
    }
    let start_distance =
        5.0 + VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    let end_distance =
        length - 5.0 - VEHICLE_FOOTPRINT.length_m / 2.0 - VEHICLE_FOOTPRINT.rear_axle_to_center_m;
    let mut poses = vec![(course.start, &tagged[0])];
    for i in 1..samples.len() - 1 {
        if samples[i].1 <= start_distance || samples[i].1 >= end_distance {
            continue;
        }
        let point = &samples[i].0;
        let before = (point.point - samples[i - 1].0.point).normalize();
        let after = (samples[i + 1].0.point - point.point).normalize();
        let direction = (before + after).normalize();
        poses.push((
            Motion {
                x_m: point.point.x,
                z_m: point.point.y,
                heading_rad: road_geom::heading_of(direction),
                ..Default::default()
            },
            point,
        ));
    }
    poses.push((course.goal.unwrap(), &tagged[tagged.len() - 1]));
    for pair in poses.windows(2) {
        let a = Vec2::new(pair[0].0.x_m, pair[0].0.z_m);
        let b = Vec2::new(pair[1].0.x_m, pair[1].0.z_m);
        let candidates: Vec<_> = rects
            .iter()
            .copied()
            .filter(|r| distance_squared(r.center, a, b) <= (r.half_size.length() + 5.0).powi(2))
            .collect();
        if collision::in_contact(&VEHICLE_FOOTPRINT, &pair[0].0, &candidates)
            || collision::in_contact(&VEHICLE_FOOTPRINT, &pair[1].0, &candidates)
            || collision::sweep(&VEHICLE_FOOTPRINT, &pair[0].0, &pair[1].0, &candidates).blocked()
        {
            let p = pair[1].1;
            return Err(BlockRouteFailure::new(
                FailureStage::Placement,
                Some(p.block_id.clone()),
                p.node_id,
                None,
                "vehicle footprint touches a curb along the selected guide".into(),
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "block_route_tests.rs"]
mod tests;
