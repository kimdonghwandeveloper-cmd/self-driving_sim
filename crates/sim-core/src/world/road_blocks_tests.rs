//! 도로 블록의 기대 동작. 구현보다 먼저 작성하고 동일 입력의 결과를 검증한다.
use super::*;
use crate::world::osm_graph::{
    fixtures::{self, FixtureWay},
    parse_overpass_graph,
};

/// 미터 단위 합성 도로를 실제 OSM 파서로 읽는다.
fn parse(ways: Vec<FixtureWay<'_>>) -> Vec<OsmGraphWay> {
    parse_overpass_graph(&fixtures::overpass_json(&ways)).unwrap()
}

/// 기본 허용 도로 설정으로 블록을 구성한다.
fn model(ways: &[OsmGraphWay]) -> RoadBlockModel {
    build_road_blocks(ways, &[], &RoadGraphConfig::default()).unwrap()
}

/// 같은 속성으로 연속하는 두 way. 뒤 way는 원본 노드가 반대로 적혀 있다.
fn chain() -> Vec<OsmGraphWay> {
    parse(vec![
        (10, "service", &[], vec![(1, 0.0, 0.0), (2, 50.0, 0.0)]),
        (20, "service", &[], vec![(3, 100.0, 0.0), (2, 50.0, 0.0)]),
    ])
}

/// 블록 중심선 노드 ID를 읽기 편한 목록으로 바꾼다.
fn ids(block: &RoadBlock) -> Vec<u64> {
    block.nodes.iter().map(|node| node.id).collect()
}

/// 분기 없는 같은 속성의 way들을 한 블록으로 묶고 원본 구간과 길이를 보존한다.
#[test]
fn merges_continuous_ways_and_preserves_source_edges() {
    let built = model(&chain());
    assert_eq!(built.version, BLOCK_MODEL_VERSION);
    assert_eq!(built.blocks.len(), 1);
    let b = &built.blocks[0];
    assert_eq!(ids(b), [1, 2, 3]);
    assert_eq!(
        b.source_edges,
        [
            SourceEdge {
                way_id: 10,
                nodes: [1, 2]
            },
            SourceEdge {
                way_id: 20,
                nodes: [2, 3]
            }
        ]
    );
    assert_eq!(
        b.directions,
        [BlockDirection::Forward, BlockDirection::Backward]
    );
    assert!((b.length_m - 100.0).abs() < 0.1);
    assert!(!b.closed);
    assert!(
        built.connections.is_empty(),
        "막다른 끝에서 자동 U턴을 만들지 않는다"
    );
}

/// 단일 way 내부의 분기점도 분할하고 방향 있는 모든 블록 접속을 보존한다.
#[test]
fn splits_at_junction_inside_way() {
    let ways = parse(vec![
        (
            10,
            "service",
            &[],
            vec![(1, -50.0, 0.0), (2, 0.0, 0.0), (3, 50.0, 0.0)],
        ),
        (20, "service", &[], vec![(2, 0.0, 0.0), (4, 0.0, 50.0)]),
    ]);
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 3);
    assert!(
        built
            .blocks
            .iter()
            .all(|b| b.nodes.first().unwrap().id == 2 || b.nodes.last().unwrap().id == 2)
    );
    assert_eq!(built.connections.len(), 6);
    assert!(
        built
            .connections
            .iter()
            .all(|c| c.via_node == 2 && c.from.block_id != c.to.block_id)
    );
}

/// 폭·도로 종류·접근·높이·교량 속성이 달라지면 블록을 나눈다.
#[test]
fn splits_on_geometry_and_access_attribute_changes() {
    for (key, value) in [
        ("width", "5"),
        ("lanes", "2"),
        ("layer", "1"),
        ("bridge", "yes"),
        ("tunnel", "yes"),
        ("access", "private"),
        ("motor_vehicle", "no"),
        ("name", "다른 도로"),
        ("incline", "5%"),
    ] {
        let mut ways = chain();
        ways[1].tags.insert(key.into(), value.into());
        assert_eq!(model(&ways).blocks.len(), 2, "{key}");
    }
    let mut ways = chain();
    ways[1].tags.insert("highway".into(), "residential".into());
    assert_eq!(model(&ways).blocks.len(), 2);
}

/// 폭 표기나 무관한 설명 차이만으로 불필요하게 분할하지 않는다.
#[test]
fn equivalent_width_and_irrelevant_metadata_do_not_split() {
    let mut ways = chain();
    ways[0].tags.insert("width".into(), "4 m".into());
    ways[1].tags.insert("width".into(), "4.0".into());
    ways[1].tags.insert("note".into(), "설명만 다름".into());
    assert_eq!(model(&ways).blocks.len(), 1);
}

/// 접근 제한이 있거나 생성 폭이 좁아도 형상 단계 이전에 도로 자체를 제거하지 않는다.
#[test]
fn keeps_restricted_and_narrow_roads_in_physical_model() {
    let mut ways = chain();
    for way in &mut ways {
        way.tags.insert("access".into(), "private".into());
        way.tags.insert("width".into(), "0.8".into());
    }
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 1);
    assert_eq!(built.blocks[0].attributes.width_m, 0.8);
    assert_eq!(
        built.blocks[0]
            .attributes
            .tags
            .get("access")
            .map(String::as_str),
        Some("private")
    );
    let config = RoadGraphConfig {
        include_restricted: true,
        ..RoadGraphConfig::default()
    };
    assert_eq!(built, build_road_blocks(&ways, &[], &config).unwrap());
}

/// 저장 방향이 반대여도 실제 진행 방향이 이어지면 하나의 일방통행 블록으로 합친다.
#[test]
fn normalizes_oneway_orientation_across_ways() {
    let mut ways = chain();
    ways[0].tags.insert("oneway".into(), "yes".into());
    ways[1].tags.insert("oneway".into(), "-1".into());
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 1);
    assert_eq!(ids(&built.blocks[0]), [1, 2, 3]);
    assert_eq!(built.blocks[0].directions, [BlockDirection::Forward]);
    ways[1].tags.insert("oneway".into(), "yes".into());
    let opposed = model(&ways);
    assert_eq!(opposed.blocks.len(), 2);
    assert!(
        opposed.connections.is_empty(),
        "서로 마주 들어오는 일방통행을 연결하지 않는다"
    );
}

/// 양방향에서 일방통행으로 바뀌는 지점을 분리하고 허용 방향만 연결한다.
#[test]
fn splits_on_travel_change_and_connects_only_legal_directions() {
    let mut ways = chain();
    ways[1].tags.insert("oneway".into(), "-1".into());
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 2);
    assert_eq!(built.connections.len(), 1);
    let c = &built.connections[0];
    let to = built.blocks.iter().find(|b| b.id == c.to.block_id).unwrap();
    assert_eq!(to.source_edges[0].way_id, 20);
    assert_eq!(c.via_node, 2);
}

/// 알 수 없는 통행 방향은 도로를 보존하되 임의로 양방향 접속을 만들지 않는다.
#[test]
fn unknown_direction_remains_visible_but_has_no_traversal() {
    let mut ways = chain();
    ways[0].tags.insert("oneway".into(), "reversible".into());
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 2);
    assert!(
        built
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 10)
            .unwrap()
            .directions
            .is_empty()
    );
    assert!(built.connections.is_empty());
}

/// 좌표만 교차하는 입체 도로는 공유 노드가 없으면 연결하거나 분할하지 않는다.
#[test]
fn does_not_connect_coordinate_crossings() {
    let ways = parse(vec![
        (10, "service", &[], vec![(1, -50.0, 0.0), (2, 50.0, 0.0)]),
        (
            20,
            "service",
            &[("bridge", "yes"), ("layer", "1")],
            vec![(3, 0.0, -50.0), (4, 0.0, 50.0)],
        ),
    ]);
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 2);
    assert!(built.connections.is_empty());
}

/// 같은 끝점을 갖는 서로 다른 way는 고리 하나로 합쳐지지 않고 선택 가능한 별도 블록으로 남는다.
#[test]
fn keeps_parallel_edges_separate() {
    let ways = parse(vec![
        (10, "service", &[], vec![(1, 0.0, 0.0), (2, 100.0, 0.0)]),
        (20, "service", &[], vec![(1, 0.0, 0.0), (2, 100.0, 0.0)]),
    ]);
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 2);
    assert_ne!(built.blocks[0].id, built.blocks[1].id);
}

/// 닫힌 고리는 한 블록으로 보존하고 시작 위치·방향이 달라도 같은 결과를 만든다.
#[test]
fn canonicalizes_closed_loop() {
    let base = parse(vec![(
        10,
        "service",
        &[],
        vec![
            (1, 0.0, 0.0),
            (2, 100.0, 0.0),
            (3, 50.0, 100.0),
            (1, 0.0, 0.0),
        ],
    )]);
    let expected = model(&base);
    assert_eq!(expected.blocks.len(), 1);
    assert!(expected.blocks[0].closed);
    assert_eq!(ids(&expected.blocks[0]), [1, 2, 3, 1]);
    let mut rotated = base.clone();
    rotated[0].nodes.rotate_left(1);
    rotated[0].nodes = vec![
        base[0].nodes[1],
        base[0].nodes[2],
        base[0].nodes[0],
        base[0].nodes[1],
    ];
    assert_eq!(model(&rotated), expected);
    rotated[0].nodes.reverse();
    assert_eq!(model(&rotated), expected);
}

/// 접속로가 붙은 고리는 그 분기점을 끝점으로 유지한다.
#[test]
fn keeps_loop_attachment_as_endpoint() {
    let ways = parse(vec![
        (
            10,
            "service",
            &[],
            vec![
                (1, 0.0, 0.0),
                (2, 100.0, 0.0),
                (3, 50.0, 100.0),
                (1, 0.0, 0.0),
            ],
        ),
        (20, "service", &[], vec![(3, 50.0, 100.0), (4, 50.0, 150.0)]),
    ]);
    let built = model(&ways);
    assert_eq!(built.blocks.len(), 2);
    let loop_block = built.blocks.iter().find(|b| b.closed).unwrap();
    assert_eq!(loop_block.nodes.first().unwrap().id, 3);
    assert_eq!(loop_block.nodes.last().unwrap().id, 3);
    assert!(
        built
            .connections
            .iter()
            .any(|c| c.via_node == 3 && c.from.block_id != c.to.block_id)
    );
}

/// 회전 제한이 있으면 2차수 지점도 분할하고 금지 회전 또는 지정 외 회전 접속은 만들지 않는다.
#[test]
fn preserves_turn_restriction_boundaries_and_connections() {
    let ways = parse(vec![
        (10, "service", &[], vec![(1, -100.0, 0.0), (2, 0.0, 0.0)]),
        (20, "service", &[], vec![(2, 0.0, 0.0), (3, 100.0, 0.0)]),
        (30, "service", &[], vec![(2, 0.0, 0.0), (4, 0.0, 100.0)]),
    ]);
    for only in [false, true] {
        let r = TurnRestriction {
            id: 99,
            from_way: 10,
            via_node: 2,
            to_way: 20,
            only,
        };
        let built = build_road_blocks(&ways, &[r], &RoadGraphConfig::default()).unwrap();
        assert_eq!(built.restrictions, [r]);
        let from = built
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 10)
            .unwrap();
        let to = built
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 20)
            .unwrap();
        let has = built
            .connections
            .iter()
            .any(|c| c.from.block_id == from.id && c.to.block_id == to.id);
        assert_eq!(has, only);
        assert_eq!(
            built
                .connections
                .iter()
                .filter(|c| c.from.block_id == from.id)
                .count(),
            1
        );
    }
    let r = TurnRestriction {
        id: 100,
        from_way: 10,
        via_node: 2,
        to_way: 20,
        only: false,
    };
    assert_eq!(
        build_road_blocks(&ways[..2], &[r], &RoadGraphConfig::default())
            .unwrap()
            .blocks
            .len(),
        2
    );
}

/// 입력 순서나 양방향 way의 저장 방향이 바뀌어도 ID·속성·접속은 같다.
#[test]
fn input_order_and_storage_direction_do_not_change_identity() {
    let ways = chain();
    let expected = model(&ways);
    let mut changed = ways.clone();
    changed.reverse();
    for way in &mut changed {
        way.nodes.reverse();
    }
    assert_eq!(model(&changed), expected);
    let mut altered = ways;
    altered[0].nodes[0].pos.lat += 0.000001;
    let actual = model(&altered);
    assert_eq!(
        actual.blocks[0].id, expected.blocks[0].id,
        "좌표 수정은 원본 ID의 동일성을 바꾸지 않는다"
    );
}

/// 다른 연결 성분을 더해도 기존 블록 ID와 순서는 안정적이다.
#[test]
fn unrelated_component_does_not_renumber_existing_block() {
    let mut ways = chain();
    let expected = model(&ways).blocks[0].clone();
    ways.extend(parse(vec![(
        5,
        "service",
        &[],
        vec![(10, 0.0, 500.0), (11, 100.0, 500.0)],
    )]));
    let actual = model(&ways);
    assert_eq!(
        actual.blocks.iter().find(|b| b.id == expected.id),
        Some(&expected)
    );
    assert!(actual.blocks.windows(2).all(|pair| pair[0].id < pair[1].id));
}

/// 허용 종류 밖의 보도는 도로 블록과 분기 차수에서 제외한다.
#[test]
fn ignores_unconfigured_highway_in_branch_degree() {
    let mut ways = chain();
    ways.extend(parse(vec![(
        30,
        "footway",
        &[],
        vec![(2, 50.0, 0.0), (4, 50.0, 50.0)],
    )]));
    assert_eq!(model(&ways).blocks.len(), 1);
}

/// 부분 결과를 반환하지 않고 중복 ID·좌표 모순·퇴화 구간을 원인과 함께 거부한다.
#[test]
fn rejects_inconsistent_or_degenerate_input() {
    let original = chain();
    let mut duplicate = original.clone();
    duplicate.push(original[0].clone());
    let mut conflicting = original.clone();
    conflicting[1].nodes[1].pos.lat += 0.001;
    let mut nan = original.clone();
    nan[0].nodes[0].pos.lat = f64::NAN;
    let mut short = original.clone();
    short[0].nodes.truncate(1);
    let mut repeated = original.clone();
    repeated[0].nodes[1] = repeated[0].nodes[0];
    for (ways, word) in [
        (duplicate, "duplicate"),
        (conflicting, "coordinate"),
        (nan, "coordinate"),
        (short, "nodes"),
        (repeated, "degenerate"),
    ] {
        let error = build_road_blocks(&ways, &[], &RoadGraphConfig::default()).unwrap_err();
        assert!(error.contains(word), "{word}: {error}");
    }
}

/// 회전 제한의 참조가 없으면 조용히 버리지 않고 거부한다.
#[test]
fn rejects_invalid_turn_reference() {
    let r = TurnRestriction {
        id: 99,
        from_way: 999,
        via_node: 2,
        to_way: 20,
        only: false,
    };
    let error = build_road_blocks(&chain(), &[r], &RoadGraphConfig::default()).unwrap_err();
    assert!(error.contains("restriction"), "{error}");
}

/// 빈 입력은 빈 모델로 처리할 수 있다.
#[test]
fn empty_input_has_no_blocks_or_connections() {
    let built = model(&[]);
    assert!(built.blocks.is_empty() && built.connections.is_empty());
}
