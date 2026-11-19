//! 고정 도로망의 기대 동작. 실제 구현보다 먼저 작성한다.
use super::*;
use crate::vehicle::GroundRect;
use crate::world::{
    osm_graph::{
        fixtures::{self, FixtureWay},
        parse_overpass_graph,
    },
    road_blocks::build_road_blocks,
    route_plan::RoadGraphConfig,
};

/// 합성 미터 좌표를 실제 OSM 파서와 블록 구성기로 통과시킨다.
fn model(ways: Vec<FixtureWay<'_>>) -> RoadBlockModel {
    build_road_blocks(
        &parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap(),
        &[],
        &RoadGraphConfig::default(),
    )
    .unwrap()
}
/// 합성 자료의 명시 원점으로 고정 도로망을 만든다.
fn build(m: &RoadBlockModel) -> Result<RoadNetworkGeometry, NetworkFailure> {
    build_road_network(m, fixtures::offset(0.0, 0.0), NetworkSettings::default())
}
/// 원점에서 세 방향으로 뻗는 교차로를 준비한다.
fn tee() -> RoadBlockModel {
    model(vec![
        (
            10,
            "service",
            &[],
            vec![(1, -50.0, 0.0), (2, 0.0, 0.0), (3, 50.0, 0.0)],
        ),
        (20, "service", &[], vec![(2, 0.0, 0.0), (4, 0.0, 50.0)]),
    ])
}
/// 점이 실제 충돌 상자 안에 있는지 검사한다.
fn inside(rect: GroundRect, point: Vec2) -> bool {
    let (sin, cos) = rect.heading_rad.sin_cos();
    let p = point - rect.center;
    p.dot(Vec2::new(cos, -sin)).abs() < rect.half_size.x
        && p.dot(Vec2::new(-sin, -cos)).abs() < rect.half_size.y
}
/// 막다른 도로는 양쪽 연석만 만들며 출발·도착 위치를 막는 끝 벽은 생성하지 않는다.
#[test]
fn straight_has_two_sides_and_no_end_walls() {
    let g = build(&model(vec![(
        10,
        "service",
        &[],
        vec![(1, 0.0, 0.0), (2, 100.0, 0.0)],
    )]))
    .unwrap();
    assert_eq!(g.blocks.len(), 1);
    assert!(g.joints.is_empty());
    assert_eq!(g.colliders.len(), 2);
    assert!(
        g.colliders
            .iter()
            .all(|c| c.kind == crate::world::course::ColliderKind::Curb)
    );
    assert!(
        !g.colliders
            .iter()
            .any(|c| inside(c.rect, Vec2::new(50.0, 0.0)))
    );
}
/// 선택 가능한 직진·좌회전·우회전 접속 순서를 바꾸거나 제거해도 물리 도로는 동일하다.
#[test]
fn selected_directions_cannot_change_network_geometry() {
    let m = tee();
    let expected = build(&m).unwrap();
    let mut selected = m.clone();
    selected.connections.reverse();
    selected.connections.truncate(1);
    selected.restrictions.clear();
    for b in &mut selected.blocks {
        b.directions.clear();
    }
    assert_eq!(expected, build(&selected).unwrap());
    assert_eq!(expected.blocks.len(), 3);
    assert_eq!(expected.joints.len(), 1);
}
/// 교차로 입구의 양쪽 끝점은 띠 단면과 일치하고 입구 내부를 가로막는 연석이 없다.
#[test]
fn joint_openings_match_strips_and_remain_passable() {
    let g = build(&tee()).unwrap();
    let j = &g.joints[0];
    assert_eq!(j.node_id, 2);
    assert_eq!(j.openings.len(), 3);
    for opening in &j.openings {
        assert!(
            g.blocks
                .iter()
                .any(|b| [0, b.strip.left.len() - 1].iter().any(|&i| {
                    let a = b.strip.left[i];
                    let z = b.strip.right[i];
                    (a.distance(opening[0]) < 0.001 && z.distance(opening[1]) < 0.001)
                        || (a.distance(opening[1]) < 0.001 && z.distance(opening[0]) < 0.001)
                }))
        );
        for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let p = opening[0].lerp(opening[1], t);
            assert!(
                !g.colliders.iter().any(|c| inside(c.rect, p)),
                "blocked {p}"
            );
        }
    }
}
/// 공유 노드가 없는 평행 도로와 입체 교차는 위치가 가까워도 접속부로 합치지 않는다.
#[test]
fn coordinate_crossings_and_parallel_roads_are_not_connected() {
    let m = model(vec![
        (10, "service", &[], vec![(1, -50.0, 0.0), (2, 50.0, 0.0)]),
        (20, "service", &[], vec![(3, -50.0, 10.0), (4, 50.0, 10.0)]),
        (
            30,
            "service",
            &[("bridge", "yes"), ("layer", "1")],
            vec![(5, 0.0, -50.0), (6, 0.0, 50.0)],
        ),
    ]);
    let g = build(&m).unwrap();
    assert_eq!(g.blocks.len(), 3);
    assert!(g.joints.is_empty());
}
/// 폭 변화 지점도 접속부로 이어 각 블록 고유 폭을 유지한다.
#[test]
fn width_transition_keeps_each_block_width() {
    let m = model(vec![
        (
            10,
            "service",
            &[("width", "4")],
            vec![(1, -50.0, 0.0), (2, 0.0, 0.0)],
        ),
        (
            20,
            "service",
            &[("width", "6")],
            vec![(2, 0.0, 0.0), (3, 50.0, 0.0)],
        ),
    ]);
    let g = build(&m).unwrap();
    assert_eq!(g.joints.len(), 1);
    for block in &g.blocks {
        let w = m
            .blocks
            .iter()
            .find(|b| b.id == block.block_id)
            .unwrap()
            .attributes
            .width_m;
        assert!((block.strip.left[0].distance(block.strip.right[0]) - w).abs() < 0.001);
    }
    // 도로 폭 전환의 접속부도 실제 화면 형상 검사와 차량 주행을 통과해야 한다.
    drive_network(
        &g,
        &[Vec2::new(-50.0, 0.0), Vec2::ZERO, Vec2::new(50.0, 0.0)],
    );
}
/// 차량 최소 회전 반경이나 접근 제한은 도로 존재 자체를 없애는 조건이 아니다.
#[test]
fn narrow_private_road_is_preserved() {
    let g = build(&model(vec![(
        10,
        "service",
        &[("width", "0.8"), ("access", "private")],
        vec![(1, 0.0, 0.0), (2, 30.0, 0.0)],
    )]))
    .unwrap();
    assert_eq!(g.blocks.len(), 1);
    assert!((g.blocks[0].strip.left[0].distance(g.blocks[0].strip.right[0]) - 0.8).abs() < 0.001);
}
/// 축에 비스듬한 직선은 부동소수점 잡음으로 거부하지 않는다.
#[test]
fn rotated_roads_all_generate() {
    for angle in 0..90 {
        let a = (angle as f64).to_radians();
        let mut nodes = Vec::new();
        for i in 0..7 {
            nodes.push((i + 1, a.cos() * i as f64 * 10.0, a.sin() * i as f64 * 10.0));
        }
        build(&model(vec![(10, "service", &[], nodes)])).unwrap();
    }
}

/// 짧은 원본 표본 뒤에 곡선이 시작돼도 도로 띠와 교차로의 실제 개구부가 맞아야 한다.
#[test]
fn curved_joint_mouth_matches_trimmed_strip() {
    let m = model(vec![
        (
            10,
            "service",
            &[("width", "6")],
            vec![
                (1, 0., 0.),
                (2, 1., 0.),
                (3, 4., 0.4),
                (4, 20., 2.),
                (5, 60., 2.),
            ],
        ),
        (
            20,
            "service",
            &[("width", "6")],
            vec![(1, 0., 0.), (6, -60., 0.)],
        ),
        (
            30,
            "service",
            &[("width", "6")],
            vec![(1, 0., 0.), (7, 0., -60.)],
        ),
    ]);
    let g = build(&m).unwrap();
    for opening in &g.joints[0].openings {
        assert!(g.blocks.iter().any(|s| {
            let n = s.strip.left.len() - 1;
            [
                (s.strip.left[0], s.strip.right[0]),
                (s.strip.left[n], s.strip.right[n]),
            ]
            .iter()
            .any(|(l, r)| {
                (l.distance(opening[0]) < 0.001 && r.distance(opening[1]) < 0.001)
                    || (r.distance(opening[0]) < 0.001 && l.distance(opening[1]) < 0.001)
            })
        }));
    }
}

/// 곡선 면을 삼각분할하더라도 실제 자기 교차나 뒤집힌 면은 허용하지 않는다.
#[test]
fn curved_joint_does_not_accept_crossed_polygon() {
    assert!(
        joint_surface_areas(
            Vec2::ZERO,
            &[
                Vec2::new(-2., -2.),
                Vec2::new(2., 2.),
                Vec2::new(-2., 2.),
                Vec2::new(2., -2.)
            ]
        )
        .is_err()
    );
    assert!(
        joint_surface_areas(
            Vec2::ZERO,
            &[Vec2::new(-2., -2.), Vec2::new(0., 2.), Vec2::new(2., -2.)]
        )
        .is_err()
    );
}

/// 실제 곡선 접속은 지도 원점을 옮겨도 삼각분할/면 검증을 통과해야 한다.
#[test]
fn real_curved_joint_survives_shifted_origins() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/block-real/expo-curved-joint.osm.json");
    let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
    let model = build_road_blocks(&snapshot.ways, &[], &RoadGraphConfig::default()).unwrap();
    let base = snapshot.ways[0].nodes[0].pos;
    for north in [-800., 0., 800.] {
        let origin = LatLon {
            lat: base.lat + (north / 6_371_008.8_f64).to_degrees(),
            lon: base.lon,
        };
        let network = build_road_network(&model, origin, NetworkSettings::default()).unwrap();
        let mut course = crate::world::course::training_course();
        course.strips = network.blocks.iter().map(|b| b.strip.clone()).collect();
        course.areas = network
            .joints
            .iter()
            .flat_map(|j| j.areas.clone())
            .collect();
        course.validate().unwrap();
    }
}

/// 실제 원본 블록/방향을 고정해 선택·저장·동일 재생·공통 목표 도달을 검사하며 다른 경로로 재시도하지 않는다.
#[test]
fn real_city_joint_samples_build_routes() {
    use crate::world::{
        block_route,
        road_blocks::{BlockDirection, BlockTraversal},
        road_snapshot,
    };
    for (file, endpoints) in [
        (
            "expo-internal.osm.json",
            [
                (532526365, 13388325415, 8664556115),
                (532526365, 13388325415, 8664556115),
            ],
        ),
        (
            "expo-curved-joint.osm.json",
            [
                (1286142779, 772406248, 794175423),
                (1286142779, 794175423, 12468412566),
            ],
        ),
        (
            "gangnam-working-area.osm.json",
            [
                (129731721, 4944067989, 1906756396),
                (504227065, 1906756396, 3089780844),
            ],
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/block-real")
            .join(file);
        let snapshot = road_snapshot::load_road_snapshot(&path).unwrap();
        let origin = snapshot.ways[0].nodes[0].pos;
        let model = build_road_blocks(
            &snapshot.ways,
            &snapshot.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let choose = |(way, entry, exit)| {
            let b = model
                .blocks
                .iter()
                .find(|b| {
                    b.source_edges.iter().any(|e| e.way_id == way)
                        && ((b.nodes[0].id == entry && b.nodes.last().unwrap().id == exit)
                            || (b.nodes[0].id == exit && b.nodes.last().unwrap().id == entry))
                })
                .expect("exact source block exists");
            BlockTraversal {
                block_id: b.id.clone(),
                direction: if b.nodes[0].id == entry {
                    BlockDirection::Forward
                } else {
                    BlockDirection::Backward
                },
            }
        };
        let mut selected: Vec<_> = endpoints.into_iter().map(choose).collect();
        if selected[0] == selected[1] {
            selected.pop();
        }
        let result = block_route::build_block_route_course(
            "real-map-drive",
            &model,
            origin,
            NetworkSettings::default(),
            &selected,
            10000.,
        )
        .unwrap();
        assert_eq!(result.network.blocks.len(), model.blocks.len());
        let dir = std::env::temp_dir().join(format!(
            "sim-real-replay-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let relative = crate::picker::model::relative_path(&dir, &path).unwrap();
        let (saved_file, prepared) = crate::world::block_route_file::prepare_block_route_file(
            &dir,
            &crate::world::block_route_file::BlockRouteRequest {
                name: "real-map-drive",
                snapshot: &relative,
                origin,
                graph: &RoadGraphConfig::default(),
                network: NetworkSettings::default(),
                selected: &selected,
            },
        )
        .unwrap();
        let saved = dir.join("real-route.toml");
        crate::world::block_route_file::save_block_route_file(&saved, &saved_file).unwrap();
        let replayed = crate::world::block_route_file::load_block_route_file(&saved).unwrap();
        assert_eq!(prepared, replayed);
        assert_eq!(result, replayed);
        for speed in [3.0, 8.0] {
            let drive = crate::world::test_drive::follow_path_to_goal(
                &replayed.course,
                &replayed.guide,
                speed,
            );
            eprintln!(
                "REAL DRIVE {file}, length {:.1} m, speed {speed}: {drive:?}",
                replayed.plan.length_m
            );
            assert_eq!(drive.impacts, 0, "{file}: {drive:?}");
            assert!(drive.goal_reached, "{file}: {drive:?}");
        }
    }
}
/// 입력 블록 순서와 생성 반복 횟수로 결과 순서나 형상이 변하지 않는다.
#[test]
fn block_order_is_deterministic() {
    let mut m = tee();
    let expected = build(&m).unwrap();
    m.blocks.reverse();
    assert_eq!(expected, build(&m).unwrap());
    assert_eq!(expected, build(&m).unwrap());
}
/// 접속부 절단 거리를 확보할 수 없으면 실패 블록과 노드를 명시하고 일부 도로를 제외하지 않는다.
#[test]
fn short_block_reports_source_without_detour() {
    let m = model(vec![
        (
            10,
            "service",
            &[],
            vec![(1, -50.0, 0.0), (2, 0.0, 0.0), (3, 1.0, 0.0)],
        ),
        (20, "service", &[], vec![(2, 0.0, 0.0), (4, 0.0, 50.0)]),
    ]);
    let error = build(&m).unwrap_err();
    assert!(error.block_id.is_some());
    assert_eq!(error.node_id, Some(2));
    assert!(error.message.contains("short"), "{error:?}");
}
/// 고리의 이음은 열린 끝 단면이나 끝 벽 없이 닫힌 띠로 이어진다.
#[test]
fn closed_loop_has_seamless_offsets() {
    let m = model(vec![(
        10,
        "service",
        &[],
        vec![
            (1, 0.0, 0.0),
            (2, 60.0, 0.0),
            (3, 60.0, 60.0),
            (4, 0.0, 60.0),
            (1, 0.0, 0.0),
        ],
    )]);
    let g = build(&m).unwrap();
    assert!(g.joints.is_empty());
    let s = &g.blocks[0].strip;
    assert_eq!(s.left.first(), s.left.last());
    assert_eq!(s.right.first(), s.right.last());
    assert!(s.left.iter().chain(&s.right).all(|p| p.is_finite()));
}
/// 잘못된 원점·설정값은 자료가 비어 있어도 거부한다.
#[test]
fn invalid_settings_and_origin_are_rejected() {
    let m = build_road_blocks(&[], &[], &RoadGraphConfig::default()).unwrap();
    let origin = fixtures::offset(0.0, 0.0);
    assert!(
        build_road_network(
            &m,
            LatLon {
                lat: f64::NAN,
                lon: 0.0
            },
            NetworkSettings::default()
        )
        .is_err()
    );
    for s in [
        NetworkSettings {
            corner_radius_m: 0.0,
            ..Default::default()
        },
        NetworkSettings {
            arc_step_deg: 0.0,
            ..Default::default()
        },
        NetworkSettings {
            curb_thickness_m: f32::NAN,
            ..Default::default()
        },
        NetworkSettings {
            min_arm_gap_deg: 0.0,
            ..Default::default()
        },
    ] {
        assert!(build_road_network(&m, origin, s).is_err());
    }
    assert!(build(&m).unwrap().blocks.is_empty());
}
/// 지정 원점을 바꾸면 도로만 같은 이동량으로 평행 이동한다.
#[test]
fn explicit_origin_controls_background_alignment() {
    let m = model(vec![(
        10,
        "service",
        &[],
        vec![(1, 0.0, 0.0), (2, 60.0, 0.0)],
    )]);
    let a = build(&m).unwrap();
    let origin = fixtures::offset(100.0, 0.0);
    let b = build_road_network(&m, origin, NetworkSettings::default()).unwrap();
    assert!((a.blocks[0].centerline[0].x - b.blocks[0].centerline[0].x - 100.0).abs() < 0.01);
}

/// 생성한 도로망을 기존 충돌·운동 모델에 연결해 시험한다. 경로 생성 정책은 넣지 않는다.
fn drive_network(g: &RoadNetworkGeometry, path: &[Vec2]) {
    let mut course = crate::world::course::training_course();
    course.surfaces.clear();
    course.markings.clear();
    course.strips = g.blocks.iter().map(|b| b.strip.clone()).collect();
    course.areas = g.joints.iter().flat_map(|j| j.areas.clone()).collect();
    course.colliders = g.colliders.clone();
    course.start = crate::world::map::start_pose(path).unwrap();
    course.goal = Some(crate::world::map::goal_pose(path).unwrap());
    course.validate().unwrap();
    for speed in [3.0, 8.0] {
        let result = crate::world::test_drive::follow_path(&course, path, speed);
        assert_eq!(result.impacts, 0, "{result:?}");
        assert!(result.remaining_m < 12.5, "{result:?}");
    }
}
/// 십자 교차로를 지나도 기존 운동 모델의 차체가 접속부 연석에 걸리지 않는다.
#[test]
fn four_way_network_supports_straight_driving() {
    let m = model(vec![
        (
            10,
            "service",
            &[],
            vec![(1, -60.0, 0.0), (2, 0.0, 0.0), (3, 60.0, 0.0)],
        ),
        (
            20,
            "service",
            &[],
            vec![(4, 0.0, -60.0), (2, 0.0, 0.0), (5, 0.0, 60.0)],
        ),
    ]);
    let g = build(&m).unwrap();
    assert_eq!(g.joints[0].openings.len(), 4);
    drive_network(
        &g,
        &[Vec2::new(-60.0, 0.0), Vec2::ZERO, Vec2::new(60.0, 0.0)],
    );
    drive_network(
        &g,
        &[Vec2::new(0.0, 60.0), Vec2::ZERO, Vec2::new(0.0, -60.0)],
    );
}
/// 원본 곡선 중심선을 유지한 노면과 연석으로 실제 운동 모델이 주행할 수 있다.
#[test]
fn curved_network_preserves_centerline_and_drives() {
    let m = model(fixtures::curved_road());
    let g = build(&m).unwrap();
    assert_eq!(g.blocks.len(), 1);
    drive_network(&g, &g.blocks[0].centerline);
}
/// 각 도로 띠 삼각형은 뒤집히거나 넓이 0이 되지 않아야 한다.
#[test]
fn strip_mesh_winding_remains_valid() {
    for m in [
        tee(),
        model(fixtures::curved_road()),
        model(vec![(
            10,
            "service",
            &[],
            vec![
                (1, 0.0, 0.0),
                (2, 60.0, 0.0),
                (3, 60.0, 60.0),
                (4, 0.0, 60.0),
                (1, 0.0, 0.0),
            ],
        )]),
    ] {
        for b in build(&m).unwrap().blocks {
            for (l, r) in b.strip.left.windows(2).zip(b.strip.right.windows(2)) {
                for (a, b, c) in [(l[0], r[0], l[1]), (r[0], r[1], l[1])] {
                    assert!((b - a).perp_dot(c - a) < -1.0e-6, "inverted road triangle");
                }
            }
        }
    }
}
/// 원본이 겹치는 되돌림은 부분 성공으로 숨기지 않고 블록 ID를 포함해 거부한다.
#[test]
fn invalid_offset_geometry_reports_block() {
    let m = model(vec![(
        10,
        "service",
        &[],
        vec![(1, 0.0, 0.0), (2, 30.0, 0.0), (3, 0.0, 0.1)],
    )]);
    let error = build(&m).unwrap_err();
    assert!(error.block_id.is_some());
}
/// 고리에 접속로가 있으면 고리 양쪽과 접속로 모두 같은 입구 면에 연결한다.
#[test]
fn attached_loop_keeps_three_openings() {
    let m = model(vec![
        (
            10,
            "service",
            &[],
            vec![
                (1, 0.0, 0.0),
                (2, 60.0, 0.0),
                (3, 60.0, 60.0),
                (4, 0.0, 60.0),
                (1, 0.0, 0.0),
            ],
        ),
        (20, "service", &[], vec![(1, 0.0, 0.0), (5, -60.0, 0.0)]),
    ]);
    let g = build(&m).unwrap();
    assert_eq!(g.joints.len(), 1);
    assert_eq!(g.joints[0].openings.len(), 3);
}
/// 엑스포 내부 주차 통로의 실제 OSM 표본을 동일 파서·블록·도로망 경로로 생성하고 주행한다.
/// 지도의 통로 존재를 검증하며 실제 출입 허가나 실측 폭을 뜻하지 않는다.
#[test]
fn expo_parking_aisle_snapshot_generates_and_drives() {
    let ways = parse_overpass_graph(include_str!(
        "../../../../testdata/road-network/expo-service.osm.json"
    ))
    .unwrap();
    assert_eq!(ways[0].id, 532526365);
    let m = build_road_blocks(&ways, &[], &RoadGraphConfig::default()).unwrap();
    let g = build_road_network(&m, ways[0].nodes[0].pos, NetworkSettings::default()).unwrap();
    assert_eq!(g.blocks.len(), 1);
    assert_eq!(
        m.blocks[0]
            .attributes
            .tags
            .get("service")
            .map(String::as_str),
        Some("parking_aisle")
    );
    drive_network(&g, &g.blocks[0].centerline);
}

/// 양 끝의 폭 전환 접속부가 4 m 블록을 모두 소비하지 않고 원본 블록을 보존해야 한다.
#[test]
fn short_junctions_allocate_cuts_between_neighbors() {
    let m = model(vec![
        (
            10,
            "service",
            &[("width", "8")],
            vec![(1, -60., 0.), (2, 0., 0.)],
        ),
        (
            20,
            "service",
            &[("width", "6")],
            vec![(2, 0., 0.), (3, 4., 0.)],
        ),
        (
            30,
            "service",
            &[("width", "8")],
            vec![(3, 4., 0.), (4, 64., 0.)],
        ),
    ]);
    let g = build(&m).expect("short connector must share its cut budget");
    assert_eq!(g.blocks.len(), m.blocks.len());
    assert_eq!(g.joints.len(), 2);
    let short = m
        .blocks
        .iter()
        .find(|b| b.source_edges[0].way_id == 20)
        .unwrap();
    let s = g.blocks.iter().find(|b| b.block_id == short.id).unwrap();
    assert!(road_geom::polyline_length(&s.centerline) >= 0.05);
    assert!(
        s.strip
            .left
            .iter()
            .zip(&s.strip.right)
            .all(|(l, r)| (l.distance(*r) - 6.).abs() < 0.001)
    );
    drive_network(&g, &[Vec2::new(-60., 0.), Vec2::new(64., 0.)]);
    let mut reversed = m.clone();
    reversed.blocks.reverse();
    assert_eq!(g, build(&reversed).unwrap());
}
/// 실제 폭 전환/곡선 교차로를 원본 좌표·폭·모든 입구 그대로 생성해야 한다.
#[test]
fn short_junctions_real_expo_and_gangnam_regressions() {
    let mut failures = Vec::new();
    for (name, node) in [("expo", 806285558_u64), ("gangnam", 414684605)] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../testdata/junction-regressions/{name}-joint.osm.json"
        ));
        let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
        let m = build_road_blocks(
            &snapshot.ways,
            &snapshot.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let origin = snapshot
            .ways
            .iter()
            .flat_map(|w| &w.nodes)
            .find(|n| n.id == node)
            .unwrap()
            .pos;
        let g = match build_road_network(&m, origin, NetworkSettings::default()) {
            Ok(g) => g,
            Err(e) => {
                failures.push(format!("{name}: {e:?}"));
                continue;
            }
        };
        assert_eq!(g.blocks.len(), m.blocks.len());
        assert!(g.joints.iter().any(|j| j.node_id == node));
        let mut course = crate::world::course::training_course();
        course.strips = g.blocks.iter().map(|b| b.strip.clone()).collect();
        course.areas = g.joints.iter().flat_map(|j| j.areas.clone()).collect();
        course.colliders = g.colliders.clone();
        course.validate().unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
/// 전체 고정 지역을 생성해 특정 실패 way만 지우거나 경로 밖 도로를 생략하지 않아야 한다.
#[test]
fn short_junctions_complete_real_regions_preserve_all_blocks() {
    for name in ["expo", "gangnam"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../testdata/junction-regressions/{name}-region.osm.json"
        ));
        let snapshot = crate::world::road_snapshot::load_road_snapshot(&path).unwrap();
        let m = build_road_blocks(
            &snapshot.ways,
            &snapshot.restrictions,
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let origin = if name == "expo" {
            LatLon {
                lat: 34.7526,
                lon: 127.7471,
            }
        } else {
            LatLon {
                lat: 37.4979,
                lon: 127.0276,
            }
        };
        let g = build_road_network(&m, origin, NetworkSettings::default()).expect(name);
        assert_eq!(g.blocks.len(), m.blocks.len());
    }
}
