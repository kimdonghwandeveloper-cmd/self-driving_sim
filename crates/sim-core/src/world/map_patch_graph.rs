//! 검증된 지역 보정을 OSM 도로 그래프의 읽기 전용 복사본에 조립한다(PLAN 4-11a).
//!
//! 좌표가 가깝다는 이유만으로 도로를 연결하지 않는다. 모든 접속은 보정 파일의 anchor가 가리키는 OSM 노드 또는
//! OSM way의 인접 노드 쌍을 확인한 뒤에만 만든다. 원본 OSM 자료와 [`MapPatch`](super::map_patch::MapPatch)는
//! 바꾸지 않으며, 호출할 때마다 같은 ID 순서의 새 way 목록을 만든다.

use std::collections::{BTreeMap, BTreeSet};

use super::map::LatLon;
use super::map_patch::{MapPatch, PatchAnchor};
use super::osm_graph::{OsmGraphWay, OsmNode};
use super::route_plan;

/// 좌표가 같은 노드라고 볼 수 있는 내부 검사 오차(m).
const POSITION_EPSILON_M: f64 = 0.001;
/// 선분 방향·교차 판정에서 쓰는 지역 좌표 오차(m).
const SEGMENT_EPSILON_M: f64 = 0.001;

/// 그래프에 실제로 조립된 보정 하나의 식별 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPatch {
    /// 보정의 안정적인 ID.
    pub id: String,
    /// 보정 파일 형식.
    pub format: String,
    /// 조립한 원본 파일의 SHA-256.
    pub sha256: String,
    /// 이 보정에서 추가한 합성 way ID.
    pub way_ids: Vec<u64>,
}

/// OSM과 지역 보정을 조립한 읽기 전용 그래프 입력.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchedGraph {
    /// 기존 OSM way와 보정 way. 기존 way 순서를 유지하고 보정 way는 보정 ID·로컬 ID 순이다.
    pub ways: Vec<OsmGraphWay>,
    /// 실제로 적용한 보정 목록(보정 ID 순).
    pub applied: Vec<AppliedPatch>,
    /// 중복 가능성처럼 적용은 허용하지만 사람 확인이 필요한 경고.
    pub warnings: Vec<String>,
}

/// way 중간 anchor를 원본 way의 어느 선분에 넣을지 기록한다.
#[derive(Debug, Clone, Copy)]
struct WayInsertion {
    /// 원본 OSM way 배열 번호.
    way_index: usize,
    /// 원본 OSM way 안의 앞 노드 번호.
    segment_index: usize,
    /// 앞 노드에서 뒤 노드까지의 비율.
    fraction: f64,
    /// 삽입할 합성 노드.
    node: OsmNode,
}

/// 교차 검사에 쓰는 선분과 그 출처.
#[derive(Debug, Clone, Copy)]
struct Segment<'a> {
    /// 첫 노드.
    a: OsmNode,
    /// 둘째 노드.
    b: OsmNode,
    /// 오류·경고에 표시할 출처.
    source: &'a str,
}

/// 두 선분의 관계.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentRelation {
    /// 만나지 않는다.
    Separate,
    /// 한 점에서 만난다.
    Touch,
    /// 같은 직선 위에서 길이가 겹친다.
    Overlap,
}

/// 두 위경도 사이의 지역 평면 거리(m)를 계산한다.
fn position_distance_m(a: LatLon, b: LatLon) -> f64 {
    let origin = LatLon {
        lat: (a.lat + b.lat) / 2.0,
        lon: (a.lon + b.lon) / 2.0,
    };
    let a = route_plan::local(origin, a);
    let b = route_plan::local(origin, b);
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// OSM 전체에서 노드 ID별 좌표를 만들고 서로 다른 좌표의 같은 ID를 거부한다.
fn osm_nodes(ways: &[OsmGraphWay]) -> Result<BTreeMap<u64, LatLon>, String> {
    let mut nodes = BTreeMap::new();
    for way in ways {
        for node in &way.nodes {
            if let Some(previous) = nodes.insert(node.id, node.pos)
                && position_distance_m(previous, node.pos) > POSITION_EPSILON_M
            {
                return Err(format!(
                    "OSM node {} has conflicting coordinates in the snapshot",
                    node.id
                ));
            }
        }
    }
    Ok(nodes)
}

/// OSM way ID가 유일한지 검사하고 ID별 배열 번호를 만든다.
fn osm_way_indices(ways: &[OsmGraphWay]) -> Result<BTreeMap<u64, usize>, String> {
    let mut indices = BTreeMap::new();
    for (index, way) in ways.iter().enumerate() {
        if let Some(previous) = indices.insert(way.id, index) {
            return Err(format!(
                "duplicate OSM way ID {} at indices {previous} and {index}",
                way.id
            ));
        }
    }
    Ok(indices)
}

/// 기준 좌표가 실제 OSM 좌표에서 anchor 허용 오차 안인지 검사한다.
fn check_anchor_error(anchor: &PatchAnchor, actual: LatLon) -> Result<(), String> {
    let expected = LatLon {
        lat: anchor.lat,
        lon: anchor.lon,
    };
    let error = position_distance_m(expected, actual);
    if error > anchor.max_error_m {
        return Err(format!(
            "anchor `{}` is {error:.2} m from its OSM target (limit {:.2} m)",
            anchor.id, anchor.max_error_m
        ));
    }
    Ok(())
}

/// 점을 선분에 투영해 선분 위 비율과 투영점, 수직 거리를 돌려준다.
fn project_to_segment(point: LatLon, a: LatLon, b: LatLon) -> (f64, LatLon, f64) {
    let origin = point;
    let p = route_plan::local(origin, point);
    let a_local = route_plan::local(origin, a);
    let b_local = route_plan::local(origin, b);
    let delta = [b_local[0] - a_local[0], b_local[1] - a_local[1]];
    let length2 = delta[0] * delta[0] + delta[1] * delta[1];
    let fraction = if length2 > 0.0 {
        (((p[0] - a_local[0]) * delta[0] + (p[1] - a_local[1]) * delta[1]) / length2)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let projected_local = [
        a_local[0] + delta[0] * fraction,
        a_local[1] + delta[1] * fraction,
    ];
    let projected = route_plan::unlocal(origin, projected_local);
    let error = (p[0] - projected_local[0]).hypot(p[1] - projected_local[1]);
    (fraction, projected, error)
}

/// node anchor 또는 way 선분 anchor를 OSM에 대조하고 그래프에서 쓸 노드를 만든다.
fn resolve_anchor(
    patch: &MapPatch,
    anchor: &PatchAnchor,
    ways: &[OsmGraphWay],
    nodes: &BTreeMap<u64, LatLon>,
    way_indices: &BTreeMap<u64, usize>,
) -> Result<(OsmNode, Option<WayInsertion>), String> {
    if let Some(id) = anchor.osm_node {
        let pos = nodes.get(&id).copied().ok_or_else(|| {
            format!(
                "patch `{}` anchor `{}` refers to missing OSM node {id}",
                patch.id, anchor.id
            )
        })?;
        check_anchor_error(anchor, pos)?;
        return Ok((OsmNode { id, pos }, None));
    }

    let way_id = anchor.osm_way.expect("validated way anchor has osm_way");
    let pair = anchor
        .between_nodes
        .expect("validated way anchor has between_nodes");
    let &way_index = way_indices.get(&way_id).ok_or_else(|| {
        format!(
            "patch `{}` anchor `{}` refers to missing OSM way {way_id}",
            patch.id, anchor.id
        )
    })?;
    let way = &ways[way_index];
    let segment_index = way
        .nodes
        .windows(2)
        .position(|nodes| [nodes[0].id, nodes[1].id] == pair || [nodes[1].id, nodes[0].id] == pair)
        .ok_or_else(|| {
            format!(
                "patch `{}` anchor `{}` nodes {} and {} are not adjacent in OSM way {way_id}",
                patch.id, anchor.id, pair[0], pair[1]
            )
        })?;
    let (fraction, projected, error) = project_to_segment(
        LatLon {
            lat: anchor.lat,
            lon: anchor.lon,
        },
        way.nodes[segment_index].pos,
        way.nodes[segment_index + 1].pos,
    );
    if error > anchor.max_error_m {
        return Err(format!(
            "patch `{}` anchor `{}` is {error:.2} m from OSM way {way_id} segment (limit {:.2} m)",
            patch.id, anchor.id, anchor.max_error_m
        ));
    }
    if fraction <= f64::EPSILON || fraction >= 1.0 - f64::EPSILON {
        return Err(format!(
            "patch `{}` anchor `{}` resolves to an OSM segment endpoint; use osm_node instead",
            patch.id, anchor.id
        ));
    }
    let id = patch.synthetic_ids[&format!("anchor/{}", anchor.id)];
    let node = OsmNode { id, pos: projected };
    Ok((
        node,
        Some(WayInsertion {
            way_index,
            segment_index,
            fraction,
            node,
        }),
    ))
}

/// 원본 way 복사본에 way anchor 노드를 선분별·비율별 결정 순서로 넣는다.
fn insert_way_anchors(ways: &mut [OsmGraphWay], insertions: &[WayInsertion]) -> Result<(), String> {
    let mut grouped: BTreeMap<(usize, usize), Vec<WayInsertion>> = BTreeMap::new();
    for insertion in insertions {
        grouped
            .entry((insertion.way_index, insertion.segment_index))
            .or_default()
            .push(*insertion);
    }
    for ((way_index, segment_index), mut group) in grouped.into_iter().rev() {
        group.sort_by(|left, right| {
            left.fraction
                .total_cmp(&right.fraction)
                .then_with(|| left.node.id.cmp(&right.node.id))
        });
        for pair in group.windows(2) {
            if position_distance_m(pair[0].node.pos, pair[1].node.pos) <= POSITION_EPSILON_M {
                return Err(format!(
                    "way anchors {} and {} resolve to the same OSM position",
                    pair[0].node.id, pair[1].node.id
                ));
            }
        }
        ways[way_index].nodes.splice(
            segment_index + 1..segment_index + 1,
            group.into_iter().map(|insertion| insertion.node),
        );
    }
    Ok(())
}

/// 보정 way의 태그를 기존 OSM 탐색기가 읽는 형태로 만든다.
fn patch_way_tags(patch: &MapPatch, way: &super::map_patch::PatchWay) -> BTreeMap<String, String> {
    let mut tags = BTreeMap::from([
        ("highway".to_owned(), way.highway.clone()),
        ("width".to_owned(), way.width_m.to_string()),
        ("width_source".to_owned(), way.width_source.clone()),
        ("map_patch".to_owned(), patch.id.clone()),
    ]);
    if way.oneway {
        tags.insert("oneway".to_owned(), "yes".to_owned());
    }
    tags
}

/// 보정 하나의 내부 노드와 anchor 참조를 실제 그래프 노드로 바꿔 way를 만든다.
fn build_patch_ways(
    patch: &MapPatch,
    anchors: &BTreeMap<String, OsmNode>,
) -> Result<Vec<OsmGraphWay>, String> {
    let internal: BTreeMap<&str, OsmNode> = patch
        .nodes
        .iter()
        .map(|node| {
            (
                node.id.as_str(),
                OsmNode {
                    id: patch.synthetic_ids[&format!("node/{}", node.id)],
                    pos: LatLon {
                        lat: node.lat,
                        lon: node.lon,
                    },
                },
            )
        })
        .collect();
    let mut ways = Vec::with_capacity(patch.ways.len());
    for way in &patch.ways {
        let mut nodes = Vec::with_capacity(way.nodes.len());
        for reference in &way.nodes {
            let node = if let Some(anchor) = reference.strip_prefix("anchor:") {
                *anchors.get(anchor).ok_or_else(|| {
                    format!(
                        "patch `{}` way `{}` has unresolved anchor `{anchor}`",
                        patch.id, way.id
                    )
                })?
            } else {
                internal[reference.as_str()]
            };
            nodes.push(node);
        }
        ways.push(OsmGraphWay {
            id: patch.synthetic_ids[&format!("way/{}", way.id)],
            tags: patch_way_tags(patch, way),
            nodes,
        });
    }
    ways.sort_by_key(|way| way.id);
    Ok(ways)
}

/// 2차원 벡터 외적을 계산한다.
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// 지역 좌표 선분 두 개가 분리·접촉·겹침 중 어느 관계인지 판정한다.
fn segment_relation(left: Segment<'_>, right: Segment<'_>) -> SegmentRelation {
    let origin = left.a.pos;
    let a = route_plan::local(origin, left.a.pos);
    let b = route_plan::local(origin, left.b.pos);
    let c = route_plan::local(origin, right.a.pos);
    let d = route_plan::local(origin, right.b.pos);
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let q = [c[0] - a[0], c[1] - a[1]];
    let denominator = cross(r, s);
    if denominator.abs() <= SEGMENT_EPSILON_M {
        if cross(q, r).abs() > SEGMENT_EPSILON_M {
            return SegmentRelation::Separate;
        }
        let axis = if r[0].abs() >= r[1].abs() { 0 } else { 1 };
        let scale = r[axis];
        if scale.abs() <= SEGMENT_EPSILON_M {
            return SegmentRelation::Separate;
        }
        let mut interval = [(c[axis] - a[axis]) / scale, (d[axis] - a[axis]) / scale];
        interval.sort_by(f64::total_cmp);
        let start = interval[0].max(0.0);
        let end = interval[1].min(1.0);
        return if end < start - f64::EPSILON {
            SegmentRelation::Separate
        } else if (end - start).abs() <= f64::EPSILON {
            SegmentRelation::Touch
        } else {
            SegmentRelation::Overlap
        };
    }
    let t = cross(q, s) / denominator;
    let u = cross(q, r) / denominator;
    if (-f64::EPSILON..=1.0 + f64::EPSILON).contains(&t)
        && (-f64::EPSILON..=1.0 + f64::EPSILON).contains(&u)
    {
        SegmentRelation::Touch
    } else {
        SegmentRelation::Separate
    }
}

/// 두 선분이 같은 노드 ID를 끝점으로 공유하는지 확인한다.
fn shares_endpoint(left: Segment<'_>, right: Segment<'_>) -> bool {
    [left.a.id, left.b.id]
        .iter()
        .any(|id| *id == right.a.id || *id == right.b.id)
}

/// way 목록을 교차 검사용 선분 목록으로 펼친다.
fn segments<'a>(ways: &'a [OsmGraphWay], source: &'a str) -> Vec<Segment<'a>> {
    ways.iter()
        .flat_map(|way| {
            way.nodes.windows(2).map(move |pair| Segment {
                a: pair[0],
                b: pair[1],
                source,
            })
        })
        .collect()
}

/// 보정 선분이 OSM 또는 다른 보정 선분과 명시되지 않은 교차를 만드는지 검사한다.
fn check_patch_intersections(
    osm_ways: &[OsmGraphWay],
    patch_ways: &[OsmGraphWay],
) -> Result<Vec<String>, String> {
    let osm = segments(osm_ways, "OSM");
    let patch = segments(patch_ways, "map patch");
    let mut warnings = BTreeSet::new();
    for (index, left) in patch.iter().copied().enumerate() {
        for right in osm.iter().copied().chain(patch.iter().copied().take(index)) {
            match segment_relation(left, right) {
                SegmentRelation::Separate => {}
                SegmentRelation::Touch if shares_endpoint(left, right) => {}
                SegmentRelation::Overlap => {
                    warnings.insert(format!(
                        "possible duplicate: {} segment {}-{} overlaps {} segment {}-{}",
                        left.source, left.a.id, left.b.id, right.source, right.a.id, right.b.id
                    ));
                }
                SegmentRelation::Touch => {
                    return Err(format!(
                        "unanchored segment intersection between {} {}-{} and {} {}-{}",
                        left.source, left.a.id, left.b.id, right.source, right.a.id, right.b.id
                    ));
                }
            }
        }
    }
    Ok(warnings.into_iter().collect())
}

/// 검증된 보정을 ID 순서로 OSM 그래프 복사본에 조립한다.
///
/// 접속 대상 누락·좌표 오차·ID 충돌·서로 다른 좌표의 같은 노드·명시되지 않은 선분 교차는 전체 적용을 거부한다.
/// 원본과 겹치는 선분은 새 도로로 조용히 중복시키지 않도록 경고로 돌려준다.
pub fn apply_map_patches(
    osm_ways: &[OsmGraphWay],
    patches: &[MapPatch],
) -> Result<PatchedGraph, String> {
    let osm_nodes = osm_nodes(osm_ways)?;
    let way_indices = osm_way_indices(osm_ways)?;
    let osm_way_ids: BTreeSet<u64> = osm_ways.iter().map(|way| way.id).collect();
    let mut patches: Vec<&MapPatch> = patches.iter().collect();
    patches.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(pair) = patches.windows(2).find(|pair| pair[0].id == pair[1].id) {
        return Err(format!("duplicate patch ID `{}`", pair[0].id));
    }

    let mut used_ids: BTreeSet<u64> = osm_nodes
        .keys()
        .copied()
        .chain(osm_way_ids.iter().copied())
        .collect();
    for patch in &patches {
        for (&id, name) in patch.synthetic_ids.values().zip(patch.synthetic_ids.keys()) {
            if !used_ids.insert(id) {
                return Err(format!(
                    "patch `{}` synthetic ID {id} for `{name}` collides with an OSM or patch ID",
                    patch.id
                ));
            }
        }
    }

    let mut ways = osm_ways.to_vec();
    let mut insertions = Vec::new();
    let mut additions = Vec::new();
    let mut applied = Vec::new();
    for patch in patches {
        let mut anchors = BTreeMap::new();
        for anchor in &patch.anchors {
            let (node, insertion) =
                resolve_anchor(patch, anchor, osm_ways, &osm_nodes, &way_indices)?;
            anchors.insert(anchor.id.clone(), node);
            if let Some(insertion) = insertion {
                insertions.push(insertion);
            }
        }
        let patch_ways = build_patch_ways(patch, &anchors)?;
        let way_ids = patch_ways.iter().map(|way| way.id).collect();
        additions.extend(patch_ways);
        applied.push(AppliedPatch {
            id: patch.id.clone(),
            format: patch.format.clone(),
            sha256: patch.sha256.clone(),
            way_ids,
        });
    }
    insert_way_anchors(&mut ways, &insertions)?;
    additions.sort_by_key(|way| way.id);
    let warnings = check_patch_intersections(&ways, &additions)?;
    ways.extend(additions);
    Ok(PatchedGraph {
        ways,
        applied,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_patch::{PatchAudience, parse_patch};
    use crate::world::route_plan::{RoadGraphConfig, plan_route};

    /// 시험용 보정 TOML을 지정한 anchor·way 본문으로 만든다.
    fn patch_text(id: &str, anchors: &str, nodes: &str, ways: &str) -> String {
        format!(
            r#"format = "map-patch-v1"
id = "{id}"
name = "test patch"
revision = 1
bbox = [34.999, 126.999, 35.002, 127.004]
source_kind = "synthetic"
source_description = "unit test"
verified_at = "2026-11-13"
license = "test-only"
status = "synthetic"
geometry_accuracy = "synthetic"
access_assumption = "test only"
notice = "not a real road"

{anchors}
{nodes}
{ways}
"#
        )
    }

    /// TOML 문자열을 자동 시험용 보정으로 읽는다.
    fn patch(text: &str) -> MapPatch {
        parse_patch(text.as_bytes(), "test.toml", PatchAudience::Test).unwrap()
    }

    /// 실제 OSM 파서가 만드는 것과 같은 시험용 service way를 명시 위경도로 만든다.
    fn osm_way(id: u64, nodes: &[(u64, f64, f64)]) -> OsmGraphWay {
        OsmGraphWay {
            id,
            tags: BTreeMap::from([("highway".to_owned(), "service".to_owned())]),
            nodes: nodes
                .iter()
                .map(|&(id, lat, lon)| OsmNode {
                    id,
                    pos: LatLon { lat, lon },
                })
                .collect(),
        }
    }

    /// 두 끊긴 OSM way와 그 끝 노드 anchor를 잇는 보정을 만든다.
    fn bridge_patch(oneway: bool) -> MapPatch {
        patch(&patch_text(
            "bridge",
            r#"[[anchors]]
id = "west"
osm_node = 2
lat = 35.0
lon = 127.001
max_error_m = 1.0

[[anchors]]
id = "east"
osm_node = 3
lat = 35.0
lon = 127.002
max_error_m = 1.0"#,
            r#"[[nodes]]
id = "middle"
lat = 35.0001
lon = 127.0015"#,
            &format!(
                r#"[[ways]]
id = "connector"
nodes = ["anchor:west", "middle", "anchor:east"]
highway = "service"
oneway = {oneway}
width_m = 4.0
width_source = "synthetic""#
            ),
        ))
    }

    /// 서로 끊긴 두 OSM way를 만든다.
    fn disconnected_ways() -> Vec<OsmGraphWay> {
        vec![
            osm_way(10, &[(1, 35.0, 127.0), (2, 35.0, 127.001)]),
            osm_way(20, &[(3, 35.0, 127.002), (4, 35.0, 127.003)]),
        ]
    }

    /// 보정 전에는 끊긴 두 도로가 보정 후 기존 A* 탐색으로 연결된다.
    #[test]
    fn patch_connects_disconnected_osm_roads() {
        let osm = disconnected_ways();
        let points = [[35.0, 127.0002], [35.0, 127.0028]];
        let config = RoadGraphConfig::default();
        assert!(plan_route(&osm, "test.json", &points, &config).is_err());
        let graph = apply_map_patches(&osm, &[bridge_patch(false)]).unwrap();
        let route = plan_route(&graph.ways, "test.json", &points, &config).unwrap();
        assert!(route.nodes.iter().any(|id| *id >= (1_u64 << 63)));
        assert_eq!(graph.applied[0].id, "bridge");
        assert!(graph.warnings.is_empty());
    }

    /// 보정 way의 일방통행은 배열 순방향만 허용한다.
    #[test]
    fn patch_oneway_is_directional() {
        let graph = apply_map_patches(&disconnected_ways(), &[bridge_patch(true)]).unwrap();
        let config = RoadGraphConfig::default();
        assert!(
            plan_route(
                &graph.ways,
                "test.json",
                &[[35.0, 127.0002], [35.0, 127.0028]],
                &config,
            )
            .is_ok()
        );
        assert!(
            plan_route(
                &graph.ways,
                "test.json",
                &[[35.0, 127.0028], [35.0, 127.0002]],
                &config,
            )
            .is_err()
        );
    }

    /// way anchor는 명시한 인접 선분에 합성 노드를 넣고 보정 way와 같은 ID로 연결한다.
    #[test]
    fn way_anchor_splits_only_the_named_segment() {
        let osm = vec![osm_way(
            10,
            &[(1, 35.0, 127.0), (2, 35.0, 127.002), (3, 35.0, 127.004)],
        )];
        let patch = patch(&patch_text(
            "side-road",
            r#"[[anchors]]
id = "join"
osm_way = 10
between_nodes = [1, 2]
lat = 35.0
lon = 127.001
max_error_m = 1.0"#,
            r#"[[nodes]]
id = "end"
lat = 35.001
lon = 127.001"#,
            r#"[[ways]]
id = "side"
nodes = ["anchor:join", "end"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
        ));
        let anchor_id = patch.synthetic_ids["anchor/join"];
        let graph = apply_map_patches(&osm, &[patch]).unwrap();
        assert_eq!(
            graph.ways[0]
                .nodes
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![1, anchor_id, 2, 3]
        );
        assert_eq!(graph.ways[1].nodes[0].id, anchor_id);
    }

    /// 없는 OSM anchor와 허용 오차를 넘긴 anchor는 전체 보정을 거부한다.
    #[test]
    fn missing_and_misaligned_anchors_are_rejected() {
        let osm = disconnected_ways();
        let mut missing = bridge_patch(false);
        missing.anchors[0].osm_node = Some(999);
        assert!(
            apply_map_patches(&osm, &[missing])
                .unwrap_err()
                .contains("missing OSM node")
        );

        let mut shifted = bridge_patch(false);
        shifted.anchors[0].lat += 0.001;
        assert!(
            apply_map_patches(&osm, &[shifted])
                .unwrap_err()
                .contains("from its OSM target")
        );
    }

    /// 명시한 way 안에서 인접하지 않은 노드 쌍은 접속점으로 쓰지 않는다.
    #[test]
    fn non_adjacent_way_anchor_is_rejected() {
        let osm = vec![osm_way(
            10,
            &[(1, 35.0, 127.0), (2, 35.0, 127.001), (3, 35.0, 127.002)],
        )];
        let text = patch_text(
            "bad-anchor",
            r#"[[anchors]]
id = "join"
osm_way = 10
between_nodes = [1, 3]
lat = 35.0
lon = 127.001
max_error_m = 1.0"#,
            r#"[[nodes]]
id = "end"
lat = 35.001
lon = 127.001"#,
            r#"[[ways]]
id = "side"
nodes = ["anchor:join", "end"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
        );
        let error = apply_map_patches(&osm, &[patch(&text)]).unwrap_err();
        assert!(error.contains("not adjacent"));
    }

    /// 좌표만 교차하고 같은 anchor 노드 ID를 공유하지 않는 선분은 연결하지 않고 거부한다.
    #[test]
    fn unanchored_coordinate_crossing_is_rejected() {
        let osm = vec![osm_way(10, &[(1, 35.0, 127.0), (2, 35.0, 127.002)])];
        let patch = patch(&patch_text(
            "crossing",
            r#"[[anchors]]
id = "start"
osm_node = 1
lat = 35.0
lon = 127.0
max_error_m = 1.0"#,
            r#"[[nodes]]
id = "north"
lat = 35.001
lon = 127.001

[[nodes]]
id = "south"
lat = 34.9995
lon = 127.001"#,
            r#"[[ways]]
id = "cross"
nodes = ["north", "south"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
        ));
        let error = apply_map_patches(&osm, &[patch]).unwrap_err();
        assert!(error.contains("unanchored segment intersection"));
    }

    /// 입력 보정 순서가 달라도 way·적용 목록·경고 결과가 같다.
    #[test]
    fn patch_application_is_order_independent() {
        let osm = disconnected_ways();
        let first = bridge_patch(false);
        let second = patch(&patch_text(
            "dead-end",
            r#"[[anchors]]
id = "start"
osm_node = 1
lat = 35.0
lon = 127.0
max_error_m = 1.0"#,
            r#"[[nodes]]
id = "end"
lat = 35.0005
lon = 127.0"#,
            r#"[[ways]]
id = "spur"
nodes = ["anchor:start", "end"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic""#,
        ));
        let forward = apply_map_patches(&osm, &[first.clone(), second.clone()]).unwrap();
        let reverse = apply_map_patches(&osm, &[second, first]).unwrap();
        assert_eq!(forward, reverse);
    }
}
