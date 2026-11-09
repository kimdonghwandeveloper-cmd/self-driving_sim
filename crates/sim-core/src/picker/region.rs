//! 경로 선택의 지역 범위·캐시 목록·모델 열기·저장 이름 규칙(엔진 무관, Phase GD-2d).
//!
//! Bevy 경로 선택 화면과 Godot `SimRoutePicker`가 같이 쓴다. 경로는 호출하는 쪽이 정한다
//! (Bevy는 실행 폴더 기준 상대 경로, Godot은 저장소 루트 기준 절대 경로).

use std::path::{Path, PathBuf};

use super::model::{NARROW_WIDTH_M, PickerModel};
use crate::world::map::LatLon;
use crate::world::map_fetch::{self, Bbox, FetchPolicy};
use crate::world::road_snapshot;
use crate::world::route_plan::{self, PlannedRoute, RoadGraphConfig, WidthSource};

/// 위경도 입력·"이 지역 도로 받기"로 받을 범위의 반 변(m). 한 변 1.5 km(PLAN 코스 범위 상한과 같음, 2 km 받기 상한 안).
pub const FETCH_HALF_SIDE_M: f64 = 750.0;
/// 경로 파일 저장 폴더(실행 폴더 기준, 저장소에서 제외되는 `cache/` 아래).
pub const ROUTE_DIR: &str = "cache/routes";

/// 위경도를 가운데로 하는 한 변 2 × [`FETCH_HALF_SIDE_M`] 범위를 만들고 검사한다.
pub fn bbox_around(lat: f64, lon: f64) -> Result<Bbox, String> {
    if !(lat.is_finite() && lon.is_finite() && lat.abs() < 85.0) {
        return Err(format!("invalid point [{lat}, {lon}]"));
    }
    let radius = 6_371_008.8_f64;
    let d_lat = (FETCH_HALF_SIDE_M / radius).to_degrees();
    let d_lon = (FETCH_HALF_SIDE_M / (radius * lat.to_radians().cos())).to_degrees();
    let bbox = Bbox {
        south: lat - d_lat,
        west: lon - d_lon,
        north: lat + d_lat,
        east: lon + d_lon,
    };
    bbox.validate()?;
    Ok(bbox)
}

/// 도로 그래프 설정 파일을 읽는다. 없거나 잘못되면 기본값과 경고문을 돌려준다.
pub fn load_road_graph_config(path: &Path) -> (RoadGraphConfig, Option<String>) {
    match std::fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| route_plan::parse_road_graph_config(&text))
    {
        Ok(config) => (config, None),
        Err(error) => (
            RoadGraphConfig::default(),
            Some(format!(
                "road graph config unavailable ({error}); using defaults"
            )),
        ),
    }
}

/// 스냅샷 파일과 도로 그래프 설정으로 경로 선택 상태를 만든다. Geofabrik 추출이면 연결 정보를 함께 읽어 검사에 쓴다.
/// 설정을 읽지 못해 기본값을 쓰면 그 경고문을 함께 돌려준다.
pub fn load_model(
    graph_path: &Path,
    config_path: &Path,
) -> Result<(PickerModel, Option<String>), String> {
    let snapshot = road_snapshot::load_road_snapshot(graph_path)?;
    let (config, warning) = load_road_graph_config(config_path);
    PickerModel::new(graph_path.to_path_buf(), snapshot.ways, config)
        .map(|model| (model.with_connectivity(snapshot.connectivity), warning))
}

/// 캐시 폴더에서 이미 받은 범위 목록을 읽는다(`bbox_<남>_<서>_<북>_<동>` 폴더 중 최신 세대가 있는 것).
pub fn cached_regions(root: &Path) -> Vec<Bbox> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut regions: Vec<Bbox> = entries
        .flatten()
        .filter(|entry| entry.path().join(map_fetch::LATEST_FILE).exists())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let values: Vec<f64> = name
                .strip_prefix("bbox_")?
                .split('_')
                .map(|part| part.parse::<f64>().ok())
                .collect::<Option<_>>()?;
            let [south, west, north, east] = values[..] else {
                return None;
            };
            let bbox = Bbox {
                south,
                west,
                north,
                east,
            };
            bbox.validate().ok().map(|()| bbox)
        })
        .collect();
    regions.sort_by_key(Bbox::cache_name);
    regions
}

/// "이 지역 도로 받기"의 범위와 정책: 화면 가운데가 이미 받은 범위 안이면 그 범위를 캐시만으로, 아니면 가운데
/// 주변 1.5 km를 새로 받는다.
pub fn region_to_load(center: LatLon, cached: &[Bbox]) -> Result<(Bbox, FetchPolicy), String> {
    if let Some(bbox) = cached.iter().find(|bbox| {
        (bbox.south..=bbox.north).contains(&center.lat)
            && (bbox.west..=bbox.east).contains(&center.lon)
    }) {
        return Ok((*bbox, FetchPolicy::CacheOnly));
    }
    Ok((bbox_around(center.lat, center.lon)?, FetchPolicy::Refresh))
}

/// 경로 저장 이름이 `[A-Za-z0-9_-]{1,64}`인지 확인한다(Godot 계약 7-2, 경로 구분자 금지).
pub fn valid_route_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// 계산된 경로의 경고 문구(임시 폭, 좁은 폭). [`PickerModel::status_lines`]와 같은 기준이다.
pub fn route_warnings(planned: &PlannedRoute) -> Vec<String> {
    let file = &planned.file;
    let mut warnings = Vec::new();
    if file.width_source != WidthSource::Tag.as_str() {
        warnings.push("Note: width is a temporary value, not from an OSM width tag".to_owned());
    }
    if file.width_m < NARROW_WIDTH_M {
        warnings.push(format!(
            "Warning: narrow road ({:.1} m < {NARROW_WIDTH_M} m)",
            file.width_m
        ));
    }
    warnings
}

/// 스냅샷 경로를 받는다: 폴더면 그 안의 `graph.osm.json`, 파일이면 그대로.
pub fn snapshot_graph_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(map_fetch::SNAPSHOT_FILE)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 캐시 폴더 이름에서 받은 범위를 읽고, 가운데가 안이면 캐시만·밖이면 새로 받기를 고르는지 확인한다.
    #[test]
    fn cached_regions_are_read_from_folder_names() {
        let root = std::env::temp_dir().join(format!("sds-cached-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bbox = bbox_around(34.75, 127.75).unwrap();
        let dir = root.join(bbox.cache_name());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(map_fetch::LATEST_FILE), "generation = \"x\"\n").unwrap();
        std::fs::create_dir_all(root.join("bbox_1_2_3_4")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let found = cached_regions(&root);
        assert_eq!(found.len(), 1);
        assert!(
            (found[0].south - bbox.south).abs() < 1e-6 && (found[0].east - bbox.east).abs() < 1e-6
        );
        // 받은 범위 안의 가운데는 캐시만, 밖은 새로 받기(1.5 km).
        let inside = LatLon {
            lat: 34.75,
            lon: 127.75,
        };
        assert!(matches!(
            region_to_load(inside, &found),
            Ok((_, FetchPolicy::CacheOnly))
        ));
        let outside = LatLon {
            lat: 37.5665,
            lon: 126.978,
        };
        assert!(matches!(
            region_to_load(outside, &found),
            Ok((_, FetchPolicy::Refresh))
        ));
        assert!(
            region_to_load(
                LatLon {
                    lat: 45.0,
                    lon: 140.0
                },
                &found
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 저장 이름 규칙: 영문·숫자·`_`·`-` 1~64자만 허용한다.
    #[test]
    fn route_names_are_checked() {
        assert!(valid_route_name("yeosu_route-1"));
        for bad in ["", "a/b", "..", "경로", "a b", &"x".repeat(65)] {
            assert!(!valid_route_name(bad), "{bad}");
        }
    }
}
