//! 날짜가 다른 기존 Overpass 스냅샷과 Geofabrik 추출 비교(Phase 4-5G, 제안서 §7.2).
//!
//! 두 자료는 기준 시각이 달라 일치를 요구하지 않는다. 대신 모든 차이를 분류한다.
//!
//! - **규칙 차이(확인됨):** 사용자 영역 안에 노드가 하나도 없고 선분만 영역을 지나는 way(Geofabrik에만 있음).
//!   좌표로 직접 확인한 경우에만 이 분류로 둔다.
//! - **데이터 동등성 미확인:** 그 밖의 모든 차이. 추출 코드 결함으로 단정하지 않는다. PBF 요소의 버전 시각이
//!   Overpass 기준 시각(`timestamp_osm_base`)보다 뒤면 "변경 가능성 있음"을 표시하지만, 이는 변경이 있었다는
//!   근거일 뿐 정상 변경을 확정하지 않는다. 노드 좌표 차이는 **그 노드**의 시각만 본다.
//! - "정상 변경(확인됨)"은 편집 이력 확인이 필요하므로 이 도구는 붙이지 않는다(네트워크는 사용자 요청 시에만).
//!
//! PBF에 way가 없다는 사실만으로 삭제를 확정하지 않는다.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use osmpbf::{Element, ElementReader};

use super::map::LatLon;
use super::osm_graph::{OsmGraphWay, parse_overpass_graph};
use super::pbf_extract::Area;
use super::road_snapshot;
use super::route_plan::{self, RoadGraphConfig};

/// 차이 분류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// 추출 규칙이 Overpass와 달라 생긴 차이(좌표로 확인).
    RuleDifference,
    /// 데이터 동등성 미확인. `possible_change`는 PBF 요소 시각이 기준 시각 이후인지.
    Unverified {
        /// 변경 가능성 있음(시각 근거만).
        possible_change: bool,
    },
}

impl Class {
    /// 보고용 이름.
    pub fn label(self) -> &'static str {
        match self {
            Self::RuleDifference => "rule difference (confirmed by coordinates)",
            Self::Unverified {
                possible_change: true,
            } => "equivalence unverified, possible change (timestamp only)",
            Self::Unverified {
                possible_change: false,
            } => "equivalence unverified",
        }
    }
}

/// 차이 하나.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Difference {
    /// 분류.
    pub class: Class,
    /// 종류(`overpass-only way`, `extract-only way`, `way tags`, `way nodes`, `node coordinate`, `route`).
    pub kind: &'static str,
    /// OSM ID(경로 차이는 0).
    pub id: u64,
    /// 설명.
    pub note: String,
}

/// 비교 결과.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompareReport {
    /// Overpass 기준 시각(없으면 `None`).
    pub base_time: Option<String>,
    /// 차이 목록(분류·종류·ID 순).
    pub differences: Vec<Difference>,
    /// 비교한 way 수 (Overpass 허용 도로, 추출).
    pub ways: (usize, usize),
}

impl CompareReport {
    /// 분류별 개수.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for difference in &self.differences {
            *counts.entry(difference.class.label()).or_insert(0) += 1;
        }
        counts
    }
}

/// `YYYY-MM-DDThh:mm:ssZ`를 유닉스 초로 바꾼다(H. Hinnant의 days_from_civil). 형식이 다르면 `None`.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// PBF 요소 기록: (버전, 시각 초).
type Stamp = (Option<i32>, Option<i64>);

/// PBF의 way 기록.
#[derive(Debug, Clone)]
struct PbfWay {
    /// 태그.
    tags: BTreeMap<String, String>,
    /// 버전·시각.
    stamp: Stamp,
}

/// PBF에서 읽은 (way ID별 way 기록, 노드 ID별 버전·시각).
type Stamps = (HashMap<u64, PbfWay>, HashMap<u64, Stamp>);

/// PBF에서 `way_ids`의 way와 `node_ids`의 노드 시각을 읽는다.
fn read_stamps<R, F>(
    open: &F,
    way_ids: &HashSet<u64>,
    node_ids: &HashSet<u64>,
) -> Result<Stamps, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let mut ways = HashMap::new();
    let mut nodes = HashMap::new();
    ElementReader::new(open()?)
        .for_each(|element| match element {
            Element::Way(way) if way_ids.contains(&(way.id() as u64)) => {
                let info = way.info();
                ways.insert(
                    way.id() as u64,
                    PbfWay {
                        tags: way
                            .tags()
                            .map(|(k, v)| (k.to_owned(), v.to_owned()))
                            .collect(),
                        stamp: (info.version(), info.milli_timestamp().map(|ms| ms / 1000)),
                    },
                );
            }
            Element::Node(node) if node_ids.contains(&(node.id() as u64)) => {
                let info = node.info();
                nodes.insert(
                    node.id() as u64,
                    (info.version(), info.milli_timestamp().map(|ms| ms / 1000)),
                );
            }
            Element::DenseNode(node) if node_ids.contains(&(node.id() as u64)) => {
                let stamp = node.info().map_or((None, None), |info| {
                    (Some(info.version()), Some(info.milli_timestamp() / 1000))
                });
                nodes.insert(node.id() as u64, stamp);
            }
            _ => {}
        })
        .map_err(|error| format!("cannot read PBF: {error}"))?;
    Ok((ways, nodes))
}

/// 1e-7°로 반올림한 좌표.
fn rounded(pos: LatLon) -> [i64; 2] {
    [
        (pos.lat * 1e7).round() as i64,
        (pos.lon * 1e7).round() as i64,
    ]
}

/// 기존 Overpass 스냅샷(`overpass_path`)과 Geofabrik 세대(`dir`)를 비교한다. `open`은 그 세대의 원본 PBF다.
/// `points`가 2개 이상이면 두 자료로 같은 점의 경로를 계획해 노드 순서·교차로도 비교한다.
pub fn compare_with_overpass<R, F>(
    overpass_path: &Path,
    dir: &Path,
    open: F,
    points: &[[f64; 2]],
) -> Result<CompareReport, String>
where
    R: Read + Send,
    F: Fn() -> Result<R, String>,
{
    let text = std::fs::read_to_string(overpass_path)
        .map_err(|error| format!("cannot read {}: {error}", overpass_path.display()))?;
    let base_time = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value["osm3s"]["timestamp_osm_base"]
                .as_str()
                .map(str::to_owned)
        });
    let base = base_time.as_deref().and_then(parse_rfc3339);
    let (meta, snapshot) = road_snapshot::load_extract_generation(dir)?;
    let allowed: BTreeSet<&str> = meta
        .geofabrik
        .allowed_highways
        .iter()
        .map(String::as_str)
        .collect();
    let old: BTreeMap<u64, OsmGraphWay> = parse_overpass_graph(&text)?
        .into_iter()
        .filter(|way| way.highway().is_some_and(|kind| allowed.contains(kind)))
        .map(|way| (way.id, way))
        .collect();
    let new: BTreeMap<u64, &OsmGraphWay> = snapshot.ways.iter().map(|way| (way.id, way)).collect();
    let way_ids: HashSet<u64> = old.keys().chain(new.keys()).copied().collect();
    let node_ids: HashSet<u64> = old
        .values()
        .chain(new.values().copied())
        .flat_map(|way| way.nodes.iter().map(|node| node.id))
        .collect();
    let (pbf_ways, pbf_nodes) = read_stamps(&open, &way_ids, &node_ids)?;
    let after_base = |stamp: Option<&Stamp>| match (stamp.and_then(|s| s.1), base) {
        (Some(time), Some(base)) => time > base,
        _ => false,
    };
    let way_after = |id: u64| after_base(pbf_ways.get(&id).map(|way| &way.stamp));
    let user_area = Area::around(&meta.bbox, 0.0);
    let mut report = CompareReport {
        base_time,
        ways: (old.len(), new.len()),
        ..CompareReport::default()
    };
    let mut found: Vec<Difference> = Vec::new();
    let mut push = |class, kind, id, note: String| {
        found.push(Difference {
            class,
            kind,
            id,
            note,
        });
    };
    for (&id, _) in old.iter().filter(|(id, _)| !new.contains_key(id)) {
        let note = match pbf_ways.get(&id) {
            None => "not in the PBF (deletion is not confirmed)".to_owned(),
            Some(way)
                if !way
                    .tags
                    .get("highway")
                    .is_some_and(|k| allowed.contains(k.as_str())) =>
            {
                "in the PBF but no longer an allowed road".to_owned()
            }
            Some(_) => "allowed road in the PBF but not extracted (check the audit)".to_owned(),
        };
        push(
            Class::Unverified {
                possible_change: way_after(id),
            },
            "overpass-only way",
            id,
            note,
        );
    }
    for (&id, way) in new.iter().filter(|(id, _)| !old.contains_key(id)) {
        let points: Vec<[f64; 2]> = way.nodes.iter().map(|n| [n.pos.lat, n.pos.lon]).collect();
        let crossing_only = !points.iter().any(|&p| user_area.contains(p))
            && points
                .windows(2)
                .any(|pair| user_area.segment_hits(pair[0], pair[1]));
        if crossing_only {
            push(
                Class::RuleDifference,
                "extract-only way",
                id,
                "no node inside the area; a segment crosses it".to_owned(),
            );
        } else {
            push(
                Class::Unverified {
                    possible_change: way_after(id),
                },
                "extract-only way",
                id,
                "has a node inside the area or lies in the margin".to_owned(),
            );
        }
    }
    let mut old_nodes: BTreeMap<u64, LatLon> = BTreeMap::new();
    let mut new_nodes: BTreeMap<u64, LatLon> = BTreeMap::new();
    for (&id, way) in old.iter() {
        old_nodes.extend(way.nodes.iter().map(|n| (n.id, n.pos)));
        let Some(fresh) = new.get(&id) else {
            continue;
        };
        if way.tags != fresh.tags {
            push(
                Class::Unverified {
                    possible_change: way_after(id),
                },
                "way tags",
                id,
                String::new(),
            );
        }
        let (a, b): (Vec<u64>, Vec<u64>) = (
            way.nodes.iter().map(|n| n.id).collect(),
            fresh.nodes.iter().map(|n| n.id).collect(),
        );
        if a != b {
            push(
                Class::Unverified {
                    possible_change: way_after(id),
                },
                "way nodes",
                id,
                String::new(),
            );
        }
    }
    for way in new.values() {
        new_nodes.extend(way.nodes.iter().map(|n| (n.id, n.pos)));
    }
    for (&id, &pos) in &old_nodes {
        if let Some(&fresh) = new_nodes.get(&id)
            && rounded(pos) != rounded(fresh)
        {
            push(
                Class::Unverified {
                    possible_change: after_base(pbf_nodes.get(&id)),
                },
                "node coordinate",
                id,
                "only this node's timestamp is used".to_owned(),
            );
        }
    }
    report.differences = found;
    if points.len() >= 2 {
        let config = RoadGraphConfig::default();
        let old_ways: Vec<OsmGraphWay> = old.into_values().collect();
        let plan_old = route_plan::plan_route_detailed(&old_ways, None, "", points, &config);
        let plan_new = route_plan::plan_route_detailed(
            &snapshot.ways,
            snapshot.connectivity.as_ref(),
            "",
            points,
            &config,
        );
        let summary = |plan: &Result<route_plan::PlannedRoute, String>| match plan {
            Ok(planned) => format!(
                "nodes {:?}, junctions {:?}",
                planned.file.nodes, planned.course.junctions
            ),
            Err(error) => format!("error: {error}"),
        };
        let (a, b) = (summary(&plan_old), summary(&plan_new));
        if a != b {
            let explained = report.differences.iter().all(|d| {
                matches!(
                    d.class,
                    Class::RuleDifference
                        | Class::Unverified {
                            possible_change: true
                        }
                )
            });
            let possible_change = explained && !report.differences.is_empty();
            report.differences.push(Difference {
                class: Class::Unverified { possible_change },
                kind: "route",
                id: 0,
                note: format!("overpass: {a} / extract: {b}"),
            });
        }
    }
    report.differences.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_fetch::Bbox;
    use crate::world::osm_graph::fixtures;
    use crate::world::pbf_extract::{self, PbfSource};
    use crate::world::pbf_fixture::Fixture;
    use std::io::Cursor;

    /// RFC 3339 읽기: 알려진 시각과 잘못된 형식.
    #[test]
    fn rfc3339_parsing() {
        assert_eq!(parse_rfc3339("2026-10-26T07:33:20Z"), Some(1_793_000_000));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-26 07:33:20"), None);
    }

    /// 분류 규칙 확인(기대값은 손으로 정함):
    /// - way 1: 태그가 다르고 way 시각이 기준 이후 → 미확인 + 변경 가능성 있음
    /// - way 2: 노드 없이 선분만 영역을 지남, Overpass에 없음 → 규칙 차이
    /// - 노드 3: 좌표가 다르지만 노드 시각이 기준 이전(way 시각은 쓰지 않음) → 미확인(표시 없음)
    /// - way 9: Overpass에만 있고 PBF에 없음 → 미확인(삭제 확정 안 함)
    #[test]
    fn differences_are_classified_without_confirming_changes() {
        let base = 1_793_000_000;
        let fixture = Fixture {
            nodes: vec![
                (1, -50.0, 0.0, Some((1, base - 100))),
                (2, 0.0, 0.0, Some((1, base - 100))),
                (3, 50.0, 0.0, Some((2, base - 50))),
                (21, -150.0, 50.0, Some((1, base - 100))),
                (22, 150.0, 50.0, Some((1, base - 100))),
            ],
            ways: vec![
                (
                    1,
                    vec![("highway", "residential"), ("name", "new")],
                    vec![1, 2, 3],
                    Some((3, base + 500)),
                ),
                (
                    2,
                    vec![("highway", "residential")],
                    vec![21, 22],
                    Some((1, base - 100)),
                ),
            ],
            replication: None,
            dense: true,
        };
        let pbf = fixture.to_pbf();
        let bytes = pbf.clone();
        let open = || Ok(Cursor::new(bytes.clone()));
        let allowed = RoadGraphConfig::default().allowed_highways;
        let roads = pbf_extract::read_national_roads(open, &allowed).unwrap();
        let bbox = Bbox {
            south: fixtures::offset(0.0, -100.0).lat,
            west: fixtures::offset(-100.0, 0.0).lon,
            north: fixtures::offset(0.0, 100.0).lat,
            east: fixtures::offset(100.0, 0.0).lon,
        };
        let source = PbfSource {
            file_name: "f.osm.pbf".to_owned(),
            sha256: road_snapshot::sha256_hex(&pbf),
            bytes: pbf.len() as u64,
        };
        let extract = pbf_extract::extract_region(&roads, &source, &bbox, 0.0, &allowed).unwrap();
        let root = std::env::temp_dir().join(format!("sds-compare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = pbf_extract::write_generation(&root, &extract).unwrap();
        // 기존 Overpass 자료: way 1(이름 old, 노드 3이 5 m 다른 곳), way 9(지금 PBF에 없음).
        let old = fixtures::overpass_json(&[
            (
                1,
                "residential",
                &[("name", "old")],
                vec![(1, -50.0, 0.0), (2, 0.0, 0.0), (3, 55.0, 0.0)],
            ),
            (
                9,
                "residential",
                &[],
                vec![(91, -50.0, 30.0), (92, 50.0, 30.0)],
            ),
        ]);
        let old = old.replacen(
            '{',
            "{\"osm3s\":{\"timestamp_osm_base\":\"2026-10-26T07:33:20Z\"},",
            1,
        );
        let old_path = root.join("old.json");
        std::fs::write(&old_path, old).unwrap();
        let report = compare_with_overpass(&old_path, &dir, open, &[]).unwrap();
        let got: Vec<(Class, &str, u64)> = report
            .differences
            .iter()
            .map(|d| (d.class, d.kind, d.id))
            .collect();
        assert_eq!(
            got,
            vec![
                (Class::RuleDifference, "extract-only way", 2),
                (
                    Class::Unverified {
                        possible_change: false
                    },
                    "node coordinate",
                    3
                ),
                (
                    Class::Unverified {
                        possible_change: false
                    },
                    "overpass-only way",
                    9
                ),
                (
                    Class::Unverified {
                        possible_change: true
                    },
                    "way tags",
                    1
                ),
            ]
        );
        assert!(
            report
                .differences
                .iter()
                .any(|d| d.note.contains("deletion is not confirmed"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
