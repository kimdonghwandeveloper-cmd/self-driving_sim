//! 경로 선택 화면의 상태와 계산(Bevy와 무관한 순수 로직, Phase 4-5).
//!
//! 화면 좌표는 배경 지도 타일과 같은 웹 메르카토르 평면([`tiles::to_view`], x=동쪽, z=남쪽, 메르카토르 m)이다.
//! 대한민국 위도에서 메르카토르 1 m는 실제 약 0.8 m다. 코스 계산은 위경도로 한다.
//! 점을 찍거나 지울 때마다 [`route_plan::plan_route_detailed`]로 경로를 다시 계산하고, 길이·폭·경고 문구를 만든다.

use std::path::{Component, Path, PathBuf};

use glam::Vec2;

use super::tiles;
use crate::world::map::LatLon;
use crate::world::osm_graph::OsmGraphWay;
use crate::world::road_snapshot::Connectivity;
use crate::world::route_plan::{self, PlannedRoute, RoadGraphConfig, WidthSource};

/// 이보다 좁은 폭이면 경고한다(m). 차체 폭 1.8 m + 양옆 0.6 m(개발 코스 좁은 통로와 같은 기준, 임시값).
pub const NARROW_WIDTH_M: f32 = 3.0;

/// 화면에 그릴 도로 선분.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadSegment {
    /// 시작점(지역 평면 좌표).
    pub from: Vec2,
    /// 끝점.
    pub to: Vec2,
    /// 경로 탐색에 쓰는 도로(허용 종류·양방향)인지.
    pub usable: bool,
}

/// 경로 선택 상태.
#[derive(Debug, Clone)]
pub struct PickerModel {
    /// 스냅샷 파일 경로.
    pub snapshot_path: PathBuf,
    /// 스냅샷 way.
    pub ways: Vec<OsmGraphWay>,
    /// 경계 연결 정보(Geofabrik 추출). Overpass 스냅샷이면 `None`(연결 검증 정보 없음).
    pub connectivity: Option<Connectivity>,
    /// 경로 계획 설정.
    pub config: RoadGraphConfig,
    /// 스냅샷 도로 범위의 가운데.
    pub origin: LatLon,
    /// 그릴 도로 선분.
    pub segments: Vec<RoadSegment>,
    /// 찍은 점 [위도, 경도](시작·경유·끝 순서).
    pub points: Vec<[f64; 2]>,
    /// 마지막 계산 결과. 점이 2개 미만이면 `None`.
    pub planned: Option<Result<PlannedRoute, String>>,
}

impl PickerModel {
    /// 스냅샷 way와 설정으로 상태를 만든다. way가 없으면 오류다.
    pub fn new(
        snapshot_path: PathBuf,
        ways: Vec<OsmGraphWay>,
        config: RoadGraphConfig,
    ) -> Result<Self, String> {
        config.validate()?;
        let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for node in ways.iter().flat_map(|way| &way.nodes) {
            low = [low[0].min(node.pos.lat), low[1].min(node.pos.lon)];
            high = [high[0].max(node.pos.lat), high[1].max(node.pos.lon)];
        }
        if !low[0].is_finite() {
            return Err("the snapshot has no road nodes".to_owned());
        }
        let origin = LatLon {
            lat: (low[0] + high[0]) / 2.0,
            lon: (low[1] + high[1]) / 2.0,
        };
        let allowed: Vec<&str> = config.allowed_highways.iter().map(String::as_str).collect();
        let segments = ways
            .iter()
            .flat_map(|way| {
                let usable = way.highway().is_some_and(|kind| allowed.contains(&kind))
                    && !crate::world::route::is_oneway(way);
                way.nodes.windows(2).map(move |pair| RoadSegment {
                    from: tiles::to_view(pair[0].pos),
                    to: tiles::to_view(pair[1].pos),
                    usable,
                })
            })
            .collect();
        Ok(Self {
            snapshot_path,
            ways,
            connectivity: None,
            config,
            origin,
            segments,
            points: Vec::new(),
            planned: None,
        })
    }

    /// 경계 연결 정보를 붙인다. 이후 경로 계산은 이 정보로 경계 연결 검사를 한다.
    pub fn with_connectivity(mut self, connectivity: Option<Connectivity>) -> Self {
        self.connectivity = connectivity;
        self
    }

    /// 위경도를 화면 좌표로 바꾼다.
    pub fn to_view(&self, point: LatLon) -> Vec2 {
        tiles::to_view(point)
    }

    /// 화면 좌표를 [위도, 경도]로 바꾼다([`map::project`]의 역변환).
    pub fn from_view(&self, view: Vec2) -> [f64; 2] {
        let point = tiles::from_view(view);
        [point.lat, point.lon]
    }

    /// 도로 전체의 화면 범위 (최소, 최대).
    pub fn view_bounds(&self) -> (Vec2, Vec2) {
        bounds(self.segments.iter().flat_map(|s| [s.from, s.to]))
    }

    /// 계산된 경로의 화면 범위. 경로가 없으면 `None`.
    pub fn route_bounds(&self) -> Option<(Vec2, Vec2)> {
        let route = self.route_views();
        (route.len() >= 2).then(|| bounds(route))
    }

    /// 찍은 점들의 화면 좌표.
    pub fn point_views(&self) -> Vec<Vec2> {
        self.points
            .iter()
            .map(|&[lat, lon]| self.to_view(LatLon { lat, lon }))
            .collect()
    }

    /// 화면 좌표 위치에 점을 끝에 더하고 경로를 다시 계산한다.
    pub fn add_point(&mut self, view: Vec2) {
        self.points.push(self.from_view(view));
        self.recompute();
    }

    /// 화면 좌표 `view`에서 `radius_m` 안의 가장 가까운 점을 지우고 경로를 다시 계산한다. 지웠으면 `true`.
    pub fn remove_near(&mut self, view: Vec2, radius_m: f32) -> bool {
        let nearest = self
            .point_views()
            .into_iter()
            .enumerate()
            .map(|(index, point)| (index, point.distance(view)))
            .filter(|(_, distance)| *distance <= radius_m)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((index, _)) = nearest else {
            return false;
        };
        self.points.remove(index);
        self.recompute();
        true
    }

    /// 마지막 점을 지우고 경로를 다시 계산한다.
    pub fn remove_last(&mut self) {
        if self.points.pop().is_some() {
            self.recompute();
        }
    }

    /// 찍은 점을 [위도, 경도] 목록으로 바꾸고 경로를 다시 계산한다(미리 찍은 점 불러오기용).
    pub fn set_points(&mut self, points: &[[f64; 2]]) {
        self.points = points.to_vec();
        self.recompute();
    }

    /// 모든 점을 지운다.
    pub fn clear(&mut self) {
        self.points.clear();
        self.planned = None;
    }

    /// 점이 2개 이상이면 경로를 다시 계산한다.
    fn recompute(&mut self) {
        self.planned = (self.points.len() >= 2).then(|| {
            route_plan::plan_route_detailed(
                &self.ways,
                self.connectivity.as_ref(),
                "",
                &self.points,
                &self.config,
            )
        });
    }

    /// 계산된 경로 노드의 화면 좌표. 계산되지 않았으면 비어 있다.
    pub fn route_views(&self) -> Vec<Vec2> {
        match &self.planned {
            Some(Ok(planned)) => planned.path.iter().map(|p| self.to_view(*p)).collect(),
            _ => Vec::new(),
        }
    }

    /// 화면 안내 문구: 점 개수, 경로 길이·폭·출처, 경고(좁은 폭·임시 폭), 오류(돌 수 없는 꺾임·거부된 교차로 등).
    pub fn status_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Points: {}", self.points.len())];
        lines.push(match &self.connectivity {
            Some(connectivity) => format!(
                "Road data: Geofabrik extract, boundary connections checked ({} incomplete node(s) near the edge)",
                connectivity.incomplete.len()
            ),
            None => "Road data: no boundary connectivity record (Overpass snapshot)".to_owned(),
        });
        match &self.planned {
            None => lines.push("Click at least 2 points on the roads (start, end)".to_owned()),
            Some(Ok(planned)) => {
                let file = &planned.file;
                lines.push(format!(
                    "Route: {:.0} m, {} intersection(s)",
                    planned.length_m,
                    planned.course.junctions.len()
                ));
                lines.push(format!(
                    "Width: {:.1} m ({})",
                    file.width_m, file.width_source
                ));
                if file.width_source != WidthSource::Tag.as_str() {
                    lines.push(
                        "Note: width is a temporary value, not from an OSM width tag".to_owned(),
                    );
                }
                if file.width_m < NARROW_WIDTH_M {
                    lines.push(format!(
                        "Warning: narrow road ({:.1} m < {NARROW_WIDTH_M} m)",
                        file.width_m
                    ));
                }
                lines.push("Press Enter or the button to start driving".to_owned());
            }
            Some(Err(error)) => lines.push(describe_error(error)),
        }
        lines
    }

    /// 계산된 경로를 `dir/name`에 경로 파일로 저장하고 경로를 돌려준다. 스냅샷 경로는 저장 폴더 기준 상대 경로다.
    pub fn save(&self, dir: &Path, name: &str) -> Result<PathBuf, String> {
        let Some(Ok(planned)) = &self.planned else {
            return Err("no valid route to save".to_owned());
        };
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let mut file = planned.file.clone();
        file.snapshot = relative_path(dir, &self.snapshot_path)?;
        let text = route_plan::route_file_toml(&file)?;
        let path = dir.join(name);
        std::fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(path)
    }
}

/// 점들의 (최소, 최대).
fn bounds(points: impl IntoIterator<Item = Vec2>) -> (Vec2, Vec2) {
    points.into_iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

/// 경로 계획 오류를 화면 문구로 바꾼다. 꺾임·교차로 거부는 종류를 앞에 붙인다.
pub fn describe_error(error: &str) -> String {
    let kind = if error.contains("too sharp") || error.contains("short for the road width") {
        "Turn the vehicle cannot make"
    } else if error.contains("intersection")
        || error.contains("junction")
        || error.contains("one-way")
    {
        "Intersection rejected"
    } else if error.contains("overlap") {
        "Roads overlap"
    } else {
        "Route error"
    };
    format!("{kind}: {error}")
}

/// `from_dir`에서 `to_file`로 가는 상대 경로(`/` 구분)를 만든다. 드라이브가 다르면 절대 경로를 쓴다.
pub fn relative_path(from_dir: &Path, to_file: &Path) -> Result<String, String> {
    let canonical = |path: &Path| {
        path.canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()))
    };
    let (from, to) = (canonical(from_dir)?, canonical(to_file)?);
    let from_parts: Vec<Component> = from.components().collect();
    let to_parts: Vec<Component> = to.components().collect();
    if from_parts.first() != to_parts.first() {
        return Ok(to.to_string_lossy().replace('\\', "/"));
    }
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from_parts.len() - common];
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map;
    use crate::world::osm_graph::{fixtures, parse_overpass_graph};

    /// 저장소의 실제 스냅샷 경로.
    fn snapshot_path() -> PathBuf {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("assets/maps/yeosu-expo/graph-2026-11-06.osm.json")
    }

    /// 실제 스냅샷으로 상태를 만든다.
    fn yeosu() -> PickerModel {
        let path = snapshot_path();
        let ways = parse_overpass_graph(&std::fs::read_to_string(&path).unwrap()).unwrap();
        PickerModel::new(path, ways, RoadGraphConfig::default()).unwrap()
    }

    /// 화면 좌표 ↔ 위경도 변환이 서로 역변환인지 확인한다.
    #[test]
    fn view_coordinates_round_trip() {
        let model = yeosu();
        let point = LatLon {
            lat: 34.75264,
            lon: 127.74712,
        };
        let view = model.to_view(point);
        let [lat, lon] = model.from_view(view);
        assert!((lat - point.lat).abs() < 1e-7 && (lon - point.lon).abs() < 1e-7);
        assert!(model.segments.iter().any(|s| s.usable));
        assert!(model.segments.iter().any(|s| !s.usable));
    }

    /// 점 찍기·지우기: 두 점이면 경로가 계산되고 안내 문구가 나오며, 지우면 다시 계산하고, 멀리서는 지우지 않는다.
    #[test]
    fn points_drive_route_planning() {
        let mut model = yeosu();
        let start = model.to_view(LatLon {
            lat: 34.75264,
            lon: 127.74712,
        });
        let end = model.to_view(LatLon {
            lat: 34.74972,
            lon: 127.74816,
        });
        model.add_point(start);
        assert!(model.planned.is_none());
        model.add_point(end);
        let Some(Ok(planned)) = &model.planned else {
            panic!("{:?}", model.planned);
        };
        assert!(
            (300.0..800.0).contains(&planned.length_m),
            "{}",
            planned.length_m
        );
        for junction in [5_143_358_021, 5_143_358_070, 5_239_572_002] {
            assert!(planned.course.junctions.contains(&junction));
        }
        let route = model.route_views();
        assert!(route.first().unwrap().distance(start) < 30.0);
        assert!(route.last().unwrap().distance(end) < 30.0);
        let lines = model.status_lines().join("\n");
        assert!(
            lines.contains("Route:") && lines.contains("temporary"),
            "{lines}"
        );
        // 같은 점이면 저장소 예제 경로 파일과 같은 노드 순서다.
        let example: route_plan::RouteFile = toml::from_str(
            &std::fs::read_to_string(
                Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                    .join("assets/maps/yeosu-expo/route-junctions.toml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(planned.file.nodes, example.nodes);
        assert!(!model.remove_near(start + Vec2::new(50.0, 0.0), 10.0));
        assert!(model.remove_near(start + Vec2::new(2.0, 0.0), 10.0));
        assert_eq!(model.points.len(), 1);
        assert!(model.planned.is_none());
        model.remove_last();
        assert!(model.points.is_empty());
    }

    /// 저장한 경로 파일을 `--course map:` 로더로 다시 열면 같은 코스가 나온다(PLAN 4-5 완료 기준).
    #[test]
    fn saved_route_reopens_as_same_course() {
        let mut model = yeosu();
        for point in [[34.75264, 127.74712], [34.74972, 127.74816]] {
            let view = model.to_view(LatLon {
                lat: point[0],
                lon: point[1],
            });
            model.add_point(view);
        }
        let dir = std::env::temp_dir().join(format!("sds-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = model.save(&dir, "route.toml").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("graph-2026-11-06.osm.json"), "{text}");
        assert!(!text.contains('\\'), "{text}");
        let loaded = map::load_map_course(&path).unwrap();
        let Some(Ok(planned)) = &model.planned else {
            panic!();
        };
        assert_eq!(loaded, planned.course.course);
        assert_eq!(map::load_map_course(&path).unwrap(), loaded);
        let _ = std::fs::remove_dir_all(&dir);
        let empty = PickerModel {
            planned: None,
            ..model
        };
        assert!(empty.save(&dir, "x.toml").is_err());
    }

    /// 경고·오류 문구: 좁은 폭 경고, 돌 수 없는 꺾임, 거부된 교차로를 구분해 보여 준다.
    #[test]
    fn warnings_name_the_problem() {
        let config = RoadGraphConfig {
            default_width_m: [("service".to_owned(), 2.5)].into_iter().collect(),
            allowed_highways: vec!["service".to_owned()],
            ..RoadGraphConfig::default()
        };
        let straight: Vec<fixtures::FixtureWay> =
            vec![(1, "service", &[], vec![(1, 0.0, 0.0), (2, 100.0, 0.0)])];
        let ways = parse_overpass_graph(&fixtures::overpass_json(&straight)).unwrap();
        let mut narrow = PickerModel::new(PathBuf::from("f.json"), ways, config).unwrap();
        let origin = narrow.to_view(fixtures::offset(0.0, 0.0));
        narrow.add_point(origin + Vec2::new(10.0, 0.0));
        narrow.add_point(origin + Vec2::new(90.0, 0.0));
        assert!(
            narrow.status_lines().join("\n").contains("narrow road"),
            "{:?}",
            narrow.status_lines()
        );
        let ways =
            parse_overpass_graph(&fixtures::overpass_json(&fixtures::narrow_arms())).unwrap();
        let mut junction =
            PickerModel::new(PathBuf::from("f.json"), ways, RoadGraphConfig::default()).unwrap();
        let origin = junction.to_view(fixtures::offset(0.0, 0.0));
        junction.add_point(origin + Vec2::new(-40.0, 0.0));
        junction.add_point(origin + Vec2::new(40.0, 0.0));
        let lines = junction.status_lines().join("\n");
        assert!(lines.contains("Intersection rejected"), "{lines}");
        assert!(describe_error("bend is too sharp").starts_with("Turn the vehicle cannot make"));
        assert!(describe_error("no road path").starts_with("Route error"));
    }

    /// 상대 경로: 같은 드라이브면 `..`로 올라가 `/`로 잇는다.
    #[test]
    fn relative_path_walks_up_and_down() {
        let root = std::env::temp_dir().join(format!("sds-rel-{}", std::process::id()));
        let (a, b) = (root.join("cache/routes"), root.join("assets/maps"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("g.json"), "{}").unwrap();
        assert_eq!(
            relative_path(&a, &b.join("g.json")).unwrap(),
            "../../assets/maps/g.json"
        );
        assert!(relative_path(&a, &b.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
