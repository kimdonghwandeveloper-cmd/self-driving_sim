//! 블록 선택 순서·통행 방향·거리 기준·안내선과 차량 배치의 기대 동작.
use super::*;
use crate::world::{
    osm_graph::{
        TurnRestriction,
        fixtures::{self, FixtureWay},
        parse_overpass_graph,
    },
    road_blocks::{BlockDirection, build_road_blocks},
    route_plan::RoadGraphConfig,
};

/// 실제 OSM 파서와 블록 구성기를 통해 시험 그래프를 만든다.
fn model(ways: Vec<FixtureWay<'_>>) -> RoadBlockModel {
    build_road_blocks(
        &parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap(),
        &[],
        &RoadGraphConfig::default(),
    )
    .unwrap()
}
/// 해당 원본 way가 포함된 블록을 노드에서 출발하는 방향으로 선택한다.
fn choose(m: &RoadBlockModel, way: u64, from: u64) -> BlockTraversal {
    let b = m
        .blocks
        .iter()
        .find(|b| b.source_edges.iter().any(|e| e.way_id == way))
        .unwrap();
    BlockTraversal {
        block_id: b.id.clone(),
        direction: if b.nodes[0].id == from {
            BlockDirection::Forward
        } else {
            assert_eq!(b.nodes.last().unwrap().id, from);
            BlockDirection::Backward
        },
    }
}
/// 넓은 십자 교차로의 원본 네 블록을 만든다.
fn cross() -> RoadBlockModel {
    model(vec![
        (
            10,
            "service",
            &[("width", "6"), ("name", "west")],
            vec![(1, -60.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("width", "6"), ("name", "east")],
            vec![(2, 0.0, 0.0), (3, 60.0, 0.0)],
        ),
        (
            30,
            "service",
            &[("width", "6"), ("name", "north")],
            vec![(2, 0.0, 0.0), (4, 0.0, 60.0)],
        ),
        (
            40,
            "service",
            &[("width", "6"), ("name", "south")],
            vec![(2, 0.0, 0.0), (5, 0.0, -60.0)],
        ),
    ])
}
/// 차선 개수로는 짧지만 실제 길이는 긴 가지와, 블록 세 개로 나뉜 짧은 가지를 만든다.
fn fork() -> RoadBlockModel {
    model(vec![
        (
            10,
            "service",
            &[("name", "start")],
            vec![(1, -50.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("name", "long")],
            vec![(2, 0.0, 0.0), (3, 50.0, 100.0), (4, 100.0, 0.0)],
        ),
        (
            30,
            "service",
            &[("name", "short-a")],
            vec![(2, 0.0, 0.0), (5, 30.0, 0.0)],
        ),
        (
            31,
            "service",
            &[("name", "short-b")],
            vec![(5, 30.0, 0.0), (6, 60.0, 0.0)],
        ),
        (
            32,
            "service",
            &[("name", "short-c")],
            vec![(6, 60.0, 0.0), (4, 100.0, 0.0)],
        ),
        (
            40,
            "service",
            &[("name", "goal")],
            vec![(4, 100.0, 0.0), (7, 150.0, 0.0)],
        ),
    ])
}
/// 합성 지도와 선택으로 코스를 구성한다.
fn course(
    m: &RoadBlockModel,
    selected: &[BlockTraversal],
) -> Result<BlockRouteCourse, BlockRouteFailure> {
    build_block_route_course(
        "block test",
        m,
        fixtures::ORIGIN,
        NetworkSettings::default(),
        selected,
        10_000.0,
    )
}
/// 선택한 블록의 원본 way 순서를 읽는다.
fn ways(m: &RoadBlockModel, p: &BlockRoutePlan) -> Vec<u64> {
    p.traversals
        .iter()
        .map(|t| {
            m.blocks
                .iter()
                .find(|b| b.id == t.block_id)
                .unwrap()
                .source_edges[0]
                .way_id
        })
        .collect()
}
/// 블록 개수가 아닌 미터 길이를 최소화하고 출발·도착 블록도 끝까지 통과한다.
#[test]
fn shortest_route_uses_meters_not_block_count() {
    let m = fork();
    let selected = [choose(&m, 10, 1), choose(&m, 40, 4)];
    let p = plan_block_route(&m, &selected, 10_000.0).unwrap();
    assert_eq!(ways(&m, &p), [10, 30, 31, 32, 40]);
    assert!((p.length_m - 200.0).abs() < 0.1);
    assert_eq!(p.selected, selected);
}
/// 자동 최단 가지 대신 명시한 경유 블록을 지정 방향으로 반드시 통과한다.
#[test]
fn ordered_via_block_is_mandatory() {
    let m = fork();
    let selected = [choose(&m, 10, 1), choose(&m, 20, 2), choose(&m, 40, 4)];
    let p = plan_block_route(&m, &selected, 10_000.0).unwrap();
    assert_eq!(ways(&m, &p), [10, 20, 40]);
    assert_eq!(p.selected, selected);
}
/// 양방향 블록은 원본 저장 순서와 반대로도 탐색하고 출발·도착 방향을 보존한다.
#[test]
fn bidirectional_route_can_run_backward() {
    let m = cross();
    let selected = [choose(&m, 20, 3), choose(&m, 10, 2)];
    let p = plan_block_route(&m, &selected, 1000.0).unwrap();
    assert_eq!(p.traversals, selected);
}
/// 일방통행을 거꾸로 선택하면 가까운 반대 방향으로 몰래 바꾸지 않는다.
#[test]
fn one_way_reverse_selection_is_rejected() {
    let m = model(vec![(
        10,
        "service",
        &[("oneway", "yes")],
        vec![(1, 0.0, 0.0), (2, 60.0, 0.0)],
    )]);
    let error = plan_block_route(&m, &[choose(&m, 10, 2)], 1000.0).unwrap_err();
    assert_eq!(error.stage, FailureStage::Selection);
    assert_eq!(error.selection_index, Some(0));
    assert!(error.block_id.is_some());
}
/// 좌표만 가까운 끊긴 도로를 연결하거나 원래 선택을 다른 도로로 치환하지 않는다.
#[test]
fn disconnected_selection_reports_unreachable_goal() {
    let m = model(vec![
        (10, "service", &[], vec![(1, 0.0, 0.0), (2, 40.0, 0.0)]),
        (20, "service", &[], vec![(3, 40.0, 0.0), (4, 80.0, 0.0)]),
    ]);
    let goal = choose(&m, 20, 3);
    let e = plan_block_route(&m, &[choose(&m, 10, 1), goal.clone()], 1000.0).unwrap_err();
    assert_eq!(e.stage, FailureStage::Planning);
    assert_eq!(e.block_id, Some(goal.block_id));
    assert_eq!(e.selection_index, Some(1));
}
/// 회전 금지로 빠진 접속은 탐색에서 되살리지 않는다.
#[test]
fn turn_restriction_connections_are_respected() {
    let mut m = cross();
    let a = choose(&m, 10, 1);
    let b = choose(&m, 30, 2);
    assert!(plan_block_route(&m, &[a.clone(), b.clone()], 1000.0).is_ok());
    m.restrictions.push(TurnRestriction {
        id: 1,
        from_way: 10,
        via_node: 2,
        to_way: 30,
        only: false,
    });
    m.connections.retain(|c| !(c.from == a && c.to == b));
    assert!(plan_block_route(&m, &[a, b], 1000.0).is_err());
}
/// 같은 거리의 여러 선택지는 입력 나열 순서와 무관하게 같은 경로를 반환한다.
#[test]
fn equal_distance_ties_are_deterministic() {
    let mut m = fork();
    for b in &mut m.blocks {
        if b.source_edges[0].way_id == 20 {
            b.length_m = 100.0;
        }
    }
    let selected = [choose(&m, 10, 1), choose(&m, 40, 4)];
    let expected = plan_block_route(&m, &selected, 1000.0).unwrap();
    m.blocks.reverse();
    m.connections.reverse();
    assert_eq!(expected, plan_block_route(&m, &selected, 1000.0).unwrap());
}
/// 전체 선택 경로 길이 상한은 출발·경유·목표 블록을 모두 포함해 적용한다.
#[test]
fn length_limit_counts_all_selected_blocks() {
    let m = fork();
    let selected = [choose(&m, 10, 1), choose(&m, 40, 4)];
    assert!(plan_block_route(&m, &selected, 199.0).is_err());
    assert!(plan_block_route(&m, &selected, 201.0).is_ok());
    for limit in [0.0, -1.0, f64::NAN, 10_001.0] {
        assert!(plan_block_route(&m, &selected, limit).is_err());
    }
}
/// 빈 선택·없는 블록·모호한 연속 중복 선택은 구체적인 선택 오류로 거부한다.
#[test]
fn invalid_selections_are_rejected() {
    let m = cross();
    let a = choose(&m, 10, 1);
    for selected in [
        vec![],
        vec![BlockTraversal {
            block_id: BlockId("missing".into()),
            direction: BlockDirection::Forward,
        }],
        vec![a.clone(), a],
    ] {
        assert_eq!(
            plan_block_route(&m, &selected, 1000.0).unwrap_err().stage,
            FailureStage::Selection
        );
    }
}
/// 같은 블록의 단일 선택도 한 블록 주행 경로로 쓸 수 있다.
#[test]
fn single_block_is_a_complete_route() {
    let m = cross();
    let a = choose(&m, 10, 1);
    let p = plan_block_route(&m, &[a.clone()], 1000.0).unwrap();
    assert_eq!(p.traversals, [a]);
    assert!((p.length_m - 60.0).abs() < 0.1);
}
/// 기존 운동 모델로 안내선을 따라 충돌 없이 목표 근처까지 주행한다.
fn assert_drives(c: &BlockRouteCourse) {
    c.course.validate().unwrap();
    for speed in [3.0, 8.0] {
        let r = crate::world::test_drive::follow_path(&c.course, &c.guide, speed);
        assert_eq!(r.impacts, 0, "speed={speed}: {r:?}");
        assert!(r.remaining_m < 12.5, "{r:?}");
    }
}
/// 직진·좌회전·우회전 경로는 같은 물리 도로망 위에 서로 다른 안내선을 만든다.
#[test]
fn cross_routes_keep_network_fixed_and_drive() {
    let m = cross();
    let mut built = Vec::new();
    for (way, node) in [(20, 2), (30, 2), (40, 2)] {
        let c = course(&m, &[choose(&m, 10, 1), choose(&m, way, node)]).unwrap();
        assert_drives(&c);
        built.push(c);
    }
    for c in &built[1..] {
        assert_eq!(c.network, built[0].network);
        assert_eq!(c.course.strips, built[0].course.strips);
        assert_eq!(c.course.areas, built[0].course.areas);
        assert_eq!(c.course.colliders, built[0].course.colliders);
        assert_ne!(c.guide, built[0].guide);
    }
}
/// 역방향 주행에서도 안내선과 시작/목표 자세의 방향이 올바르다.
#[test]
fn reverse_guide_and_poses_are_drivable() {
    let m = cross();
    let c = course(&m, &[choose(&m, 20, 3), choose(&m, 10, 2)]).unwrap();
    assert!(c.guide[0].x > c.guide.last().unwrap().x);
    assert!(c.course.start.x_m > c.course.goal.unwrap().x_m);
    assert_drives(&c);
}
/// 곡선 블록의 안내선은 차량 최소 곡률과 차체 폭을 만족해 실제 운동 모델로 주행한다.
#[test]
fn curved_block_guide_is_drivable() {
    let m = model(fixtures::curved_road());
    let b = &m.blocks[0];
    let c = course(
        &m,
        &[BlockTraversal {
            block_id: b.id.clone(),
            direction: BlockDirection::Forward,
        }],
    )
    .unwrap();
    assert_drives(&c);
}
/// 차량이 들어갈 수 없는 좁은 도로는 블록을 그대로 지목하고 차체 배치 오류를 반환한다.
#[test]
fn narrow_block_reports_vehicle_failure() {
    let m = model(vec![(
        10,
        "service",
        &[("width", "0.8")],
        vec![(1, 0.0, 0.0), (2, 60.0, 0.0)],
    )]);
    let e = course(&m, &[choose(&m, 10, 1)]).unwrap_err();
    assert_eq!(e.stage, FailureStage::Placement);
    assert_eq!(e.block_id, Some(m.blocks[0].id.clone()));
}
/// 차량 최소 회전 반경보다 작은 회전은 네트워크를 넓히거나 다른 도로로 우회하지 않고 실패한다.
#[test]
fn impossible_turn_reports_original_joint_without_detour() {
    let m = model(vec![
        (
            10,
            "service",
            &[("width", "2.5"), ("name", "entry")],
            vec![(1, -60.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("width", "2.5"), ("name", "exit")],
            vec![(2, 0.0, 0.0), (3, 0.0, 60.0)],
        ),
    ]);
    let selected = [choose(&m, 10, 1), choose(&m, 20, 2)];
    let expected = plan_block_route(&m, &selected, 1000.0).unwrap();
    let e = course(&m, &selected).unwrap_err();
    assert_eq!(e.stage, FailureStage::Guide);
    assert_eq!(e.node_id, Some(2));
    assert_eq!(e.block_id, Some(selected[0].block_id.clone()));
    assert_eq!(expected.traversals, selected);
}
/// 접속부 도로 생성 실패를 경로 오류로 숨기지 않고 원본 블록/노드와 반환한다.
#[test]
fn network_failure_preserves_source_diagnostics() {
    let m = model(vec![
        (
            10,
            "service",
            &[("name", "entry")],
            vec![(1, -60.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("name", "short")],
            vec![(2, 0.0, 0.0), (3, 1.0, 0.0)],
        ),
        (30, "service", &[], vec![(2, 0.0, 0.0), (4, 0.0, 60.0)]),
    ]);
    let e = course(&m, &[choose(&m, 10, 1), choose(&m, 20, 2)]).unwrap_err();
    assert_eq!(e.stage, FailureStage::Network);
    assert_eq!(e.node_id, Some(2));
    assert!(e.block_id.is_some());
}
/// 동일 입력을 재생성하면 경로·물리 도로·안내선·차량 자세가 같아야 한다.
#[test]
fn course_rebuild_is_deterministic() {
    let mut m = cross();
    let selected = [choose(&m, 10, 1), choose(&m, 30, 2)];
    let a = course(&m, &selected).unwrap();
    m.blocks.reverse();
    m.connections.reverse();
    assert_eq!(a, course(&m, &selected).unwrap());
}
