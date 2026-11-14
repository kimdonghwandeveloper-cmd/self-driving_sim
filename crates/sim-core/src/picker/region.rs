//! 경로 선택의 지역 범위·모델 열기·저장 이름 규칙(엔진 무관, Phase GD-2d).
//!
//! Godot `SimRoutePicker`가 쓴다. 경로는 호출하는 쪽이 정한다(Godot은 저장소 루트 기준 절대 경로).

use std::path::{Path, PathBuf};

use super::model::{NARROW_WIDTH_M, PickerModel};
use crate::world::map_fetch::{self, Bbox};
use crate::world::road_snapshot;
use crate::world::route_plan::{self, PlannedRoute, RoadGraphConfig, WidthSource};

/// 지역 열기(전국 자료, PLAN 4-11)의 범위 반 변(m). 한 변 1.5 km(PLAN 코스 범위 상한과 같음, 2 km 범위 상한 안).
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
    PickerModel::new(graph_path.to_path_buf(), snapshot.ways, config).map(|model| {
        let model = model
            .with_connectivity(snapshot.connectivity)
            .with_restrictions(snapshot.restrictions);
        (model, warning)
    })
}

/// 지도 화면에 보일 대한민국 영역의 **임시 근사 경계** [위도, 경도](시계 방향, 닫는 점 중복 없음).
///
/// 경로 선택 지도에서 주변 나라를 가리려는 표시용이며 실제 국경·영해선이 아니다(사용자 결정 2026-11-09).
/// 휴전선은 몇 개 점으로 꺾어 근사했고, 바다 쪽은 제주·서남해 섬·울릉도·독도를 넣고 쓰시마(일본)는 뺀다.
/// 해안·휴전선 근처는 수 km 어긋날 수 있다.
pub const KOREA_OUTLINE: [[f64; 2]; 22] = [
    [38.05, 124.55],
    [37.68, 125.55],
    [37.70, 126.10],
    [37.76, 126.66],
    [37.95, 126.95],
    [38.10, 127.10],
    [38.30, 127.55],
    [38.32, 128.05],
    [38.62, 128.33],
    [38.62, 128.60],
    [37.95, 130.40],
    [37.70, 132.00],
    [37.00, 132.00],
    [35.70, 129.80],
    [35.05, 129.32],
    [34.70, 128.85],
    [34.15, 128.30],
    [33.80, 127.50],
    [32.95, 126.90],
    [32.95, 125.90],
    [34.00, 124.90],
    [36.20, 124.55],
];

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

    /// 지역 범위는 가운데 기준 한 변 1.5 km이고, 잘못된 좌표는 오류다.
    #[test]
    fn region_bbox_is_1_5_km_around_center() {
        let bbox = bbox_around(34.75, 127.75).unwrap();
        let north_south_m = (bbox.north - bbox.south).to_radians() * 6_371_008.8;
        assert!((north_south_m - 2.0 * FETCH_HALF_SIDE_M).abs() < 1.0);
        assert!(((bbox.south + bbox.north) / 2.0 - 34.75).abs() < 1e-9);
        assert!(bbox_around(f64::NAN, 127.0).is_err());
        assert!(bbox_around(89.0, 127.0).is_err());
    }

    /// 점이 다각형 안에 있는지 짝홀 규칙으로 판정한다(시험용).
    fn inside(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
        let mut inside = false;
        for (index, a) in polygon.iter().enumerate() {
            let b = polygon[(index + 1) % polygon.len()];
            if (a[0] > point[0]) != (b[0] > point[0]) {
                let lon = a[1] + (point[0] - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
                if point[1] < lon {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// 근사 경계가 남한 주요 지점·섬은 넣고 북한·중국·일본 지점은 빼는지 확인한다.
    #[test]
    fn korea_outline_includes_south_korea_only() {
        let included = [
            ("Seoul", [37.5665, 126.978]),
            ("Busan", [35.1796, 129.0756]),
            ("Yeosu Expo", [34.751, 127.75]),
            ("Jeju", [33.4996, 126.5312]),
            ("Marado", [33.115, 126.267]),
            ("Baengnyeongdo", [37.96, 124.68]),
            ("Ulleungdo", [37.484, 130.905]),
            ("Dokdo", [37.241, 131.865]),
            ("Goseong", [38.38, 128.47]),
            ("Gageodo", [34.07, 125.12]),
        ];
        for (name, point) in included {
            assert!(inside(point, &KOREA_OUTLINE), "{name} must be inside");
        }
        let excluded = [
            ("Kaesong", [37.97, 126.55]),
            ("Pyongyang", [39.03, 125.75]),
            ("Wonsan", [39.15, 127.44]),
            ("Tsushima Izuhara", [34.20, 129.29]),
            ("Tsushima north", [34.65, 129.45]),
            ("Shanghai", [31.23, 121.47]),
            ("Fukuoka", [33.59, 130.40]),
        ];
        for (name, point) in excluded {
            assert!(!inside(point, &KOREA_OUTLINE), "{name} must be outside");
        }
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
