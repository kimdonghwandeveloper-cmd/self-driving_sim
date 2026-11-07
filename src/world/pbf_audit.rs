//! 실제 추출 결과의 보존 규칙 대조(Phase 4-5G, 제안서 §7.4).
//!
//! 정답을 손으로 적을 수 없는 실제 자료에서, 세대 폴더가 선택 규칙(§3.1)과 경계 연결 규칙(§3.2)을 지켰는지
//! 원본 PBF와 전수 대조한다. 추출 코드([`super::pbf_extract`])의 선택·대조 함수를 쓰지 않고 따로 구현한다:
//! 읽기는 `osmpbf::ElementReader`, 자료 구조는 해시 표, 선분 교차는 네 변과의 방향 판정으로 한다.
//! 그래도 추출과 같은 잘못된 가정을 공유할 수 있으므로 정확성의 주 근거는 정답 fixture(§7.3)다.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use osmpbf::{Element, ElementReader};

use super::road_snapshot;

/// 보고할 문제 줄 수 상한(나머지는 개수만 센다).
const MAX_LISTED: usize = 50;
/// 지구 평균 반지름(m).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// 대조 결과.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuditReport {
    /// 규칙상 있어야 할 way 수.
    pub expected_ways: usize,
    /// 스냅샷 way 수.
    pub snapshot_ways: usize,
    /// 대조한 노드 수.
    pub checked_nodes: usize,
    /// 발견한 문제(앞 [`MAX_LISTED`]개).
    pub problems: Vec<String>,
    /// 발견한 문제 전체 수.
    pub problem_count: usize,
}

impl AuditReport {
    /// 문제를 기록한다.
    fn problem(&mut self, text: String) {
        self.problem_count += 1;
        if self.problems.len() < MAX_LISTED {
            self.problems.push(text);
        }
    }
}

/// 원본 PBF의 허용 도로 way 하나: (태그 (키, 값) 정렬 목록, 노드 ID).
type SourceWay = (Vec<(String, String)>, Vec<i64>);

/// 원본 PBF에서 읽은 (way ID별 허용 도로 way, 노드 ID별 1e-7° 좌표).
type SourceData = (HashMap<i64, SourceWay>, HashMap<i64, [i32; 2]>);

/// 방향 판정: 세 점이 반시계면 양수, 시계면 음수, 일직선이면 0.
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// 일직선인 점 `c`가 선분 `a`–`b`의 범위 안인지.
fn within(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    c[0] >= a[0].min(b[0])
        && c[0] <= a[0].max(b[0])
        && c[1] >= a[1].min(b[1])
        && c[1] <= a[1].max(b[1])
}

/// 두 선분이 만나는지(끝점 접촉·겹침 포함).
fn segments_meet(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let (d1, d2, d3, d4) = (
        orient(r, s, p),
        orient(r, s, q),
        orient(p, q, r),
        orient(p, q, s),
    );
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    (d1 == 0.0 && within(r, s, p))
        || (d2 == 0.0 && within(r, s, q))
        || (d3 == 0.0 && within(p, q, r))
        || (d4 == 0.0 && within(p, q, s))
}

/// 원본 PBF를 두 번 읽어 허용 도로 way와 그 노드 좌표를 모은다.
fn read_source<R, F>(open: &F, allowed: &HashSet<String>) -> Result<SourceData, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let mut ways: HashMap<i64, SourceWay> = HashMap::new();
    ElementReader::new(open()?)
        .for_each(|element| {
            if let Element::Way(way) = element {
                let mut tags: Vec<(String, String)> = way
                    .tags()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect();
                tags.sort();
                if tags
                    .iter()
                    .any(|(k, v)| k == "highway" && allowed.contains(v))
                {
                    ways.insert(way.id(), (tags, way.refs().collect()));
                }
            }
        })
        .map_err(|error| format!("cannot read PBF ways: {error}"))?;
    let needed: HashSet<i64> = ways
        .values()
        .flat_map(|(_, refs)| refs.iter().copied())
        .collect();
    let mut coords: HashMap<i64, [i32; 2]> = HashMap::new();
    ElementReader::new(open()?)
        .for_each(|element| {
            let (id, lat, lon) = match element {
                Element::Node(n) => (n.id(), n.decimicro_lat(), n.decimicro_lon()),
                Element::DenseNode(n) => (n.id(), n.decimicro_lat(), n.decimicro_lon()),
                _ => return,
            };
            if needed.contains(&id) {
                coords.insert(id, [lat, lon]);
            }
        })
        .map_err(|error| format!("cannot read PBF nodes: {error}"))?;
    Ok((ways, coords))
}

/// 노드별 (way, 이웃) 연결을 모은다. `keep`의 노드만.
fn links<'a>(
    ways: impl Iterator<Item = (i64, &'a [i64])>,
    keep: &HashSet<i64>,
) -> HashMap<i64, HashSet<(i64, i64)>> {
    let mut out: HashMap<i64, HashSet<(i64, i64)>> = HashMap::new();
    for (way, refs) in ways {
        for pair in refs.windows(2) {
            if pair[0] == pair[1] {
                continue;
            }
            if keep.contains(&pair[0]) {
                out.entry(pair[0]).or_default().insert((way, pair[1]));
            }
            if keep.contains(&pair[1]) {
                out.entry(pair[1]).or_default().insert((way, pair[0]));
            }
        }
    }
    out
}

/// 세대 폴더 `dir`을 원본 PBF(`open`)와 전수 대조한다. 읽기 오류는 `Err`, 규칙 위반은 보고의 `problems`다.
pub fn audit_generation<R, F>(open: F, dir: &Path) -> Result<AuditReport, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let (meta, snapshot) = road_snapshot::load_extract_generation(dir)?;
    let connectivity = snapshot
        .connectivity
        .ok_or("the generation has no connectivity record")?;
    let allowed: HashSet<String> = meta.geofabrik.allowed_highways.iter().cloned().collect();
    let (source, coords) = read_source(&open, &allowed)?;

    // 영역 R = 사용자 영역 + 여유 폭.
    let bbox = meta.bbox;
    let margin = meta.geofabrik.margin_m;
    let lat_pad = margin / EARTH_RADIUS_M * 180.0 / std::f64::consts::PI;
    let lon_pad = margin / (EARTH_RADIUS_M * ((bbox.south + bbox.north) / 2.0).to_radians().cos())
        * 180.0
        / std::f64::consts::PI;
    let (s, w, n, e) = (
        bbox.south - lat_pad,
        bbox.west - lon_pad,
        bbox.north + lat_pad,
        bbox.east + lon_pad,
    );
    let corners = [[s, w], [s, e], [n, e], [n, w]];
    let point = |id: &i64| {
        coords
            .get(id)
            .map(|c| [f64::from(c[0]) / 1e7, f64::from(c[1]) / 1e7])
    };
    let inside = |p: [f64; 2]| p[0] >= s && p[0] <= n && p[1] >= w && p[1] <= e;
    let touches = |refs: &[i64]| {
        refs.iter().filter_map(point).any(inside)
            || refs
                .windows(2)
                .any(|pair| match (point(&pair[0]), point(&pair[1])) {
                    (Some(a), Some(b)) => {
                        (0..4).any(|k| segments_meet(a, b, corners[k], corners[(k + 1) % 4]))
                    }
                    _ => false,
                })
    };

    let mut report = AuditReport::default();
    let expected: BTreeSet<i64> = source
        .iter()
        .filter(|(_, (_, refs))| touches(refs))
        .map(|(id, _)| *id)
        .collect();
    let present: BTreeSet<i64> = snapshot.ways.iter().map(|way| way.id as i64).collect();
    report.expected_ways = expected.len();
    report.snapshot_ways = present.len();
    for id in expected.difference(&present) {
        report.problem(format!(
            "way {id} touches the area but is missing from the snapshot"
        ));
    }
    for id in present.difference(&expected) {
        report.problem(format!(
            "way {id} is in the snapshot but does not touch the area (or is not an allowed road)"
        ));
    }

    for way in &snapshot.ways {
        let Some((tags, refs)) = source.get(&(way.id as i64)) else {
            continue;
        };
        let mut got: Vec<(String, String)> = way
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        got.sort();
        if &got != tags {
            report.problem(format!("way {} tags differ from the PBF", way.id));
        }
        let ids: Vec<i64> = way.nodes.iter().map(|node| node.id as i64).collect();
        if &ids != refs {
            report.problem(format!("way {} node list differs from the PBF", way.id));
            continue;
        }
        for node in &way.nodes {
            let stored = [
                (node.pos.lat * 1e7).round() as i32,
                (node.pos.lon * 1e7).round() as i32,
            ];
            if coords.get(&(node.id as i64)) != Some(&stored) {
                report.problem(format!("node {} coordinate differs from the PBF", node.id));
            }
        }
    }

    let keep: HashSet<i64> = snapshot
        .ways
        .iter()
        .flat_map(|way| way.nodes.iter().map(|node| node.id as i64))
        .collect();
    report.checked_nodes = keep.len();
    let national = links(
        source.iter().map(|(id, (_, refs))| (*id, refs.as_slice())),
        &keep,
    );
    let local_refs: Vec<(i64, Vec<i64>)> = snapshot
        .ways
        .iter()
        .map(|way| {
            (
                way.id as i64,
                way.nodes.iter().map(|node| node.id as i64).collect(),
            )
        })
        .collect();
    let local = links(
        local_refs.iter().map(|(id, refs)| (*id, refs.as_slice())),
        &keep,
    );
    let empty = HashSet::new();
    let mut expected_incomplete: Vec<(u64, Vec<[u64; 2]>)> = keep
        .iter()
        .filter_map(|node| {
            let (whole, here) = (
                national.get(node).unwrap_or(&empty),
                local.get(node).unwrap_or(&empty),
            );
            (whole != here).then(|| {
                let mut missing: Vec<[u64; 2]> = whole
                    .difference(here)
                    .map(|&(w, x)| [w as u64, x as u64])
                    .collect();
                missing.sort();
                (*node as u64, missing)
            })
        })
        .collect();
    expected_incomplete.sort();
    let recorded: Vec<(u64, Vec<[u64; 2]>)> = connectivity
        .incomplete
        .iter()
        .map(|entry| (entry.node, entry.missing.clone()))
        .collect();
    if recorded != expected_incomplete {
        report.problem(format!(
            "connectivity.json lists {} incomplete node(s); the PBF shows {}",
            recorded.len(),
            expected_incomplete.len()
        ));
    }
    if connectivity.checked_nodes != keep.len() as u64 {
        report.problem("connectivity.json checked_nodes differs from the snapshot".to_owned());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_fetch::{self, Bbox};
    use crate::world::osm_graph::fixtures;
    use crate::world::pbf_extract::{self, PbfSource, RegionExtract};
    use crate::world::pbf_fixture::Fixture;
    use crate::world::road_snapshot::{CONNECTIVITY_FILE, IncompleteNode};
    use crate::world::route_plan::RoadGraphConfig;
    use std::io::Cursor;

    /// 시험 자료: 안의 way 1, 선분만 지나는 way 2, 밖의 way 5(노드 22에 붙음).
    fn fixture() -> Fixture {
        let node = |id, east, north| (id, east, north, None);
        let way = |id, refs: Vec<u64>| (id, vec![("highway", "residential")], refs, None);
        Fixture {
            nodes: vec![
                node(1, -50.0, 0.0),
                node(2, 50.0, 0.0),
                node(21, -150.0, 50.0),
                node(22, 150.0, 50.0),
                node(51, 200.0, 50.0),
            ],
            ways: vec![
                way(1, vec![1, 2]),
                way(2, vec![21, 22]),
                way(5, vec![22, 51]),
            ],
            replication: None,
            dense: true,
        }
    }

    /// 반 변 100 m 정사각형.
    fn square() -> Bbox {
        Bbox {
            south: fixtures::offset(0.0, -100.0).lat,
            west: fixtures::offset(-100.0, 0.0).lon,
            north: fixtures::offset(0.0, 100.0).lat,
            east: fixtures::offset(100.0, 0.0).lon,
        }
    }

    /// 추출 결과를 임시 세대로 쓴다.
    fn write(name: &str, extract: &RegionExtract) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sds-audit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        pbf_extract::write_generation(&root, extract).unwrap()
    }

    /// 정상 추출은 문제 없이 통과하고, 선분만 지나는 way를 빠뜨린 추출(같은 누락을 매번 반복하는 결함)은
    /// 재현성 검사로는 못 잡지만 보존 규칙 대조에서 잡히는지 확인한다.
    #[test]
    fn audit_passes_good_extract_and_catches_dropped_crossing_way() {
        let pbf = fixture().to_pbf();
        let bytes = pbf.clone();
        let open = || Ok(Cursor::new(bytes.clone()));
        let allowed = RoadGraphConfig::default().allowed_highways;
        let roads = pbf_extract::read_national_roads(open, &allowed).unwrap();
        let source = PbfSource {
            file_name: "fixture.osm.pbf".to_owned(),
            sha256: road_snapshot::sha256_hex(&pbf),
            bytes: pbf.len() as u64,
        };
        let good = pbf_extract::extract_region(&roads, &source, &square(), 0.0, &allowed).unwrap();
        let dir = write("good", &good);
        let report = audit_generation(open, &dir).unwrap();
        assert_eq!(report.problems, Vec::<String>::new());
        assert_eq!(
            (
                report.expected_ways,
                report.snapshot_ways,
                report.checked_nodes
            ),
            (2, 2, 4)
        );

        // 결함 흉내: 선분만 지나는 way 2를 빼고, 남은 자료로 해시·연결 정보를 일관되게 다시 만든다.
        let mut kept =
            pbf_extract::select_ways(&roads, &pbf_extract::Area::around(&square(), 0.0)).unwrap();
        kept.retain(|way| way.id != 2);
        let mut bad = good.clone();
        bad.body = pbf_extract::graph_body(&kept);
        let refs: Vec<(u64, Vec<u64>)> = kept
            .iter()
            .map(|w| (w.id, w.nodes.iter().map(|(id, _)| *id).collect()))
            .collect();
        let (checked, incomplete) = pbf_extract::compare_connectivity(
            roads.ways.iter().map(|w| (w.id, w.refs.as_slice())),
            refs.iter().map(|(id, r)| (*id, r.as_slice())),
        )
        .unwrap();
        assert_eq!(incomplete, Vec::<IncompleteNode>::new());
        bad.connectivity.graph_sha256 = road_snapshot::sha256_hex(bad.body.as_bytes());
        bad.connectivity.checked_nodes = checked;
        bad.connectivity.incomplete = incomplete;
        bad.meta.graph_sha256 = bad.connectivity.graph_sha256.clone();
        bad.meta.graph_bytes = bad.body.len() as u64;
        bad.meta.connectivity.sha256 =
            road_snapshot::sha256_hex(bad.connectivity.to_json().as_bytes());
        bad.meta.connectivity.checked_nodes = checked;
        bad.meta.connectivity.incomplete_nodes = 0;
        let dir = write("bad", &bad);
        assert!(dir.join(CONNECTIVITY_FILE).exists() && dir.join(map_fetch::META_FILE).exists());
        let report = audit_generation(open, &dir).unwrap();
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("way 2 touches the area but is missing")),
            "{:?}",
            report.problems
        );
    }

    /// 선분 교차 판정: 가로지름, 끝점 접촉, 떨어짐, 일직선 겹침.
    #[test]
    fn segment_meeting_cases() {
        assert!(segments_meet(
            [0.0, 0.0],
            [2.0, 2.0],
            [0.0, 2.0],
            [2.0, 0.0]
        ));
        assert!(segments_meet(
            [0.0, 0.0],
            [1.0, 1.0],
            [1.0, 1.0],
            [2.0, 0.0]
        ));
        assert!(!segments_meet(
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0]
        ));
        assert!(segments_meet(
            [0.0, 0.0],
            [2.0, 0.0],
            [1.0, 0.0],
            [3.0, 0.0]
        ));
    }
}
