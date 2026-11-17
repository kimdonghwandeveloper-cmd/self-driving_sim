//! 3D 건물 절차적 생성 (PLAN 4-12).
//!
//! 경로 양옆 150m 안의 건물 윤곽을 평평한 지붕 상자(로우폴리)로 세운다.
//! 높이는 `height` → `building:levels` × 층 높이(기본 3.0m) → 건물 종류별 기본값 순서로 정하고,
//! 기본값을 쓴 건물이 있으면 "실제 높이 아님"을 표시한다 (한국 OSM: 층수 약 16%, 높이 약 6%만 있음).
//! 도로 면(폭 + 여유)과 겹치는 건물은 빼거나 제외한다.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec2;

use crate::vehicle::GroundRect;

use super::course::BuildingDef;
use super::map::{LatLon, project};
use super::places::{self, Place};

/// 건물 층당 기본 높이 (m).
pub const DEFAULT_LEVEL_HEIGHT_M: f32 = 3.0;

/// 경로 중심선으로부터 건물을 포함할 기본 복도 거리 (m).
pub const MAX_BUILDING_CORRIDOR_DISTANCE_M: f32 = 150.0;

/// 도로 면과의 최소 이격 거리 (m). 도로 면과 겹치는 건물을 제외할 때 여유를 둔다.
pub const ROAD_CLEARANCE_MARGIN_M: f32 = 0.5;

/// 피트(ft) 단위를 미터(m)로 변환하는 계수.
const FEET_TO_METERS: f32 = 0.3048;

/// 문자열에서 `height` 태그의 미터 단위 높이를 파싱한다.
pub fn parse_height_tag(raw: &str) -> Option<f32> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix("meters").or_else(|| s.strip_suffix('m')) {
        let val = num.trim().parse::<f32>().ok()?;
        return if val > 0.0 && val <= 1000.0 {
            Some(val)
        } else {
            None
        };
    }
    if let Some(num) = s.strip_suffix("feet").or_else(|| s.strip_suffix("ft")) {
        let val = num.trim().parse::<f32>().ok()?;
        let meters = val * FEET_TO_METERS;
        return if meters > 0.0 && meters <= 1000.0 {
            Some(meters)
        } else {
            None
        };
    }
    let val = s.parse::<f32>().ok()?;
    if val > 0.0 && val <= 1000.0 {
        Some(val)
    } else {
        None
    }
}

/// 문자열에서 `building:levels` 태그의 층수를 파싱한다.
pub fn parse_levels_tag(raw: &str) -> Option<f32> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let val = s.parse::<f32>().ok()?;
    if val > 0.0 && val <= 200.0 {
        Some(val)
    } else {
        None
    }
}

/// 건물 태그로부터 기본 높이(m)를 종류별로 산정한다.
fn default_height_by_kind(kind: &str, tags: &BTreeMap<String, String>) -> f32 {
    let building_tag = tags.get("building").map(|s| s.as_str()).unwrap_or("");
    let kind_lower = kind.to_lowercase();
    let b_lower = building_tag.to_lowercase();

    if b_lower == "apartments" || kind_lower.contains("apartments") {
        18.0
    } else if b_lower == "commercial"
        || b_lower == "retail"
        || b_lower == "office"
        || b_lower == "hotel"
        || b_lower == "supermarket"
        || kind_lower.contains("commercial")
        || kind_lower.contains("office")
        || kind_lower.contains("shop")
    {
        12.0
    } else if b_lower == "school"
        || b_lower == "university"
        || b_lower == "hospital"
        || b_lower == "civic"
        || b_lower == "public"
        || kind_lower.contains("school")
        || kind_lower.contains("hospital")
    {
        10.0
    } else if b_lower == "industrial"
        || b_lower == "warehouse"
        || b_lower == "hangar"
        || kind_lower.contains("industrial")
    {
        8.0
    } else if b_lower == "house"
        || b_lower == "residential"
        || b_lower == "detached"
        || b_lower == "terrace"
        || kind_lower.contains("residential")
        || kind_lower.contains("house")
    {
        6.0
    } else if b_lower == "garage"
        || b_lower == "garages"
        || b_lower == "shed"
        || b_lower == "kiosk"
        || kind_lower.contains("garage")
    {
        3.0
    } else {
        6.0
    }
}

/// OSM 태그와 종류로부터 건물의 최종 높이(m)와 기본값 추정 여부를 결정한다.
pub fn determine_building_height(tags: &BTreeMap<String, String>, kind: &str) -> (f32, bool) {
    if let Some(h) = tags.get("height").and_then(|s| parse_height_tag(s)) {
        return (h, false);
    }
    if let Some(levels) = tags
        .get("building:levels")
        .and_then(|s| parse_levels_tag(s))
    {
        return (levels * DEFAULT_LEVEL_HEIGHT_M, false);
    }
    (default_height_by_kind(kind, tags), true)
}

/// 2D 다각형의 부호 있는 면적(Shoelace formula)을 계산한다.
pub fn signed_polygon_area(vertices: &[Vec2]) -> f32 {
    let n = vertices.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
        sum += a.x * b.y - b.x * a.y;
    }
    sum * 0.5
}

/// 점 p가 2D 삼각형 (a, b, c) 내부에 있는지 검사한다.
pub fn point_in_triangle_2d(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let cross_ab = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let cross_bc = (c.x - b.x) * (p.y - b.y) - (c.y - b.y) * (p.x - b.x);
    let cross_ca = (a.x - c.x) * (p.y - c.y) - (a.y - c.y) * (p.x - c.x);

    let has_neg = cross_ab < -1e-5 || cross_bc < -1e-5 || cross_ca < -1e-5;
    let has_pos = cross_ab > 1e-5 || cross_bc > 1e-5 || cross_ca > 1e-5;

    !(has_neg && has_pos)
}

/// 2D 단순 다각형을 귀 잘라내기(Ear-clipping) 알고리즘으로 삼각분할하여 인덱스 목록을 반환한다.
pub fn triangulate_polygon_2d(vertices: &[Vec2]) -> Vec<[usize; 3]> {
    let n = vertices.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    let mut indices: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);

    let area = signed_polygon_area(vertices);
    let ccw = area > 0.0;

    let mut iterations = 0;
    let max_iterations = n * 4;

    while indices.len() > 3 && iterations < max_iterations {
        iterations += 1;
        let mut ear_found = false;
        let len = indices.len();

        for i in 0..len {
            let prev = indices[(i + len - 1) % len];
            let curr = indices[i];
            let next = indices[(i + 1) % len];

            let a = vertices[prev];
            let b = vertices[curr];
            let c = vertices[next];

            let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            let is_convex = if ccw { cross > 1e-6 } else { cross < -1e-6 };
            if !is_convex {
                continue;
            }

            let mut has_point_inside = false;
            for &p_idx in &indices {
                if p_idx == prev || p_idx == curr || p_idx == next {
                    continue;
                }
                if point_in_triangle_2d(vertices[p_idx], a, b, c) {
                    has_point_inside = true;
                    break;
                }
            }

            if !has_point_inside {
                triangles.push([prev, curr, next]);
                indices.remove(i);
                ear_found = true;
                break;
            }
        }

        if !ear_found {
            break;
        }
    }

    if indices.len() == 3 {
        triangles.push([indices[0], indices[1], indices[2]]);
    } else if triangles.is_empty() {
        // 볼록 다각형 단순 팬 방식 폴백
        for i in 1..n - 1 {
            triangles.push([0, i, i + 1]);
        }
    }

    triangles
}

/// 점 p에서 선분 a-b까지의 최단 거리(m)를 계산한다.
pub fn distance_point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let projection = a + ab * t;
    (p - projection).length()
}

/// 점 p에서 도로 중심선 목록까지의 최단 거리(m)를 계산한다.
pub fn distance_to_centerlines(p: Vec2, centerlines: &[Vec<Vec2>]) -> f32 {
    let mut min_dist = f32::INFINITY;
    for line in centerlines {
        for window in line.windows(2) {
            let dist = distance_point_to_segment(p, window[0], window[1]);
            if dist < min_dist {
                min_dist = dist;
            }
        }
    }
    min_dist
}

/// 건물 윤곽이 도로 영역(GroundRect)과 겹치는지 검사한다.
pub fn polygon_overlaps_ground_rect(footprint: &[Vec2], rect: &GroundRect, clearance: f32) -> bool {
    // 1. AABB 1차 빠른 거부 검사
    let min_x = footprint.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x = footprint
        .iter()
        .map(|p| p.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = footprint.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_z = footprint
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max);

    let rect_radius = rect.half_size.length() + clearance;
    let dx = rect.center.x - (min_x + max_x) * 0.5;
    let dz = rect.center.y - (min_z + max_z) * 0.5;
    let dist_sq = dx * dx + dz * dz;
    let max_extent = ((max_x - min_x).hypot(max_z - min_z) * 0.5) + rect_radius;
    if dist_sq > max_extent * max_extent {
        return false;
    }

    // 2. 직사각형 로컬 좌표계로 건물 꼭짓점 변환 후 포함 여부 검사
    let (sin, cos) = (-rect.heading_rad).sin_cos();
    for &p in footprint {
        let rel = p - rect.center;
        let local_x = rel.x * cos - rel.y * sin;
        let local_z = rel.x * sin + rel.y * cos;
        if local_x.abs() <= rect.half_size.x + clearance
            && local_z.abs() <= rect.half_size.y + clearance
        {
            return true;
        }
    }

    // 3. 직사각형의 네 꼭짓점이 건물 다각형 내부에 있는지 검사
    let half_w = rect.half_size.x + clearance;
    let half_h = rect.half_size.y + clearance;
    let corners_local = [
        Vec2::new(-half_w, -half_h),
        Vec2::new(half_w, -half_h),
        Vec2::new(half_w, half_h),
        Vec2::new(-half_w, half_h),
    ];
    let (world_sin, world_cos) = rect.heading_rad.sin_cos();
    for c in &corners_local {
        let world_pt = Vec2::new(
            rect.center.x + c.x * world_cos - c.y * world_sin,
            rect.center.y + c.x * world_sin + c.y * world_cos,
        );
        if point_inside_polygon(world_pt, footprint) {
            return true;
        }
    }

    false
}

/// 점 p가 단순 다각형 내부에 있는지 검사한다 (Ray-casting).
fn point_inside_polygon(p: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[j]);
        if ((a.y > p.y) != (b.y > p.y)) && (p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 장소 목록에서 경로 양옆 복도(150m) 내의 건물을 추출하고 도로 겹침을 방지하여 건물 목록을 만든다.
pub fn build_course_buildings(
    places: &[Place],
    origin: LatLon,
    road_centerlines: &[Vec<Vec2>],
    road_surfaces: &[GroundRect],
    max_corridor_dist_m: f32,
) -> (Vec<BuildingDef>, bool) {
    let mut buildings = Vec::new();
    let mut has_estimated = false;

    for place in places {
        // 윤곽이 없는 점 장소는 3D 건물로 생성하지 않음
        if place.outline.len() < 3 {
            continue;
        }

        // 지면 로컬 (x, z) 좌표로 투영
        let footprint: Vec<Vec2> = place.outline.iter().map(|&p| project(origin, p)).collect();

        // 중심점 계산
        let count = footprint.len() as f32;
        let center = Vec2::new(
            footprint.iter().map(|p| p.x).sum::<f32>() / count,
            footprint.iter().map(|p| p.y).sum::<f32>() / count,
        );

        // 경로 중심선과의 복도 거리 검사
        if !road_centerlines.is_empty() {
            let dist = distance_to_centerlines(center, road_centerlines);
            if dist > max_corridor_dist_m {
                continue;
            }
        }

        // 도로 면과의 중첩 검사
        let overlaps = road_surfaces
            .iter()
            .any(|rect| polygon_overlaps_ground_rect(&footprint, rect, ROAD_CLEARANCE_MARGIN_M));
        if overlaps {
            continue;
        }

        // 높이 산정
        let (height_m, is_est) = determine_building_height(&place.tags, &place.kind);
        if is_est {
            has_estimated = true;
        }

        buildings.push(BuildingDef {
            id: place.id.clone(),
            name: place.name.clone(),
            kind: place.kind.clone(),
            height_m,
            is_estimated: is_est,
            footprint,
        });
    }

    // 일관된 순서를 위해 ID로 정렬
    buildings.sort_by(|a, b| a.id.cmp(&b.id));
    (buildings, has_estimated)
}

/// 기준 디렉터리 주변에서 동반 장소/건물 파일(`places.osm.json`, `places-synthetic.osm.json` 등)을 찾아 건물을 생성한다.
pub fn load_companion_buildings(
    base_dir: &Path,
    origin: LatLon,
    road_centerlines: &[Vec<Vec2>],
    road_surfaces: &[GroundRect],
) -> (Vec<BuildingDef>, bool) {
    let candidate_names = [
        "places.osm.json",
        "places-synthetic.osm.json",
        "places.pack",
        "../places.osm.json",
        "../places-synthetic.osm.json",
    ];

    for name in &candidate_names {
        let path = base_dir.join(name);
        if !path.exists() {
            continue;
        }
        let Ok(places) = places::load_places(&path) else {
            continue;
        };
        if places.is_empty() {
            continue;
        }
        return build_course_buildings(
            &places,
            origin,
            road_centerlines,
            road_surfaces,
            MAX_BUILDING_CORRIDOR_DISTANCE_M,
        );
    }

    (Vec::new(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_height_tag_variations() {
        assert_eq!(parse_height_tag("15"), Some(15.0));
        assert_eq!(parse_height_tag("15.5m"), Some(15.5));
        assert_eq!(parse_height_tag(" 12.0 meters "), Some(12.0));
        assert_eq!(parse_height_tag("50ft"), Some(50.0 * 0.3048));
        assert_eq!(parse_height_tag("0"), None);
        assert_eq!(parse_height_tag("-5"), None);
        assert_eq!(parse_height_tag("invalid"), None);
    }

    #[test]
    fn parse_levels_tag_variations() {
        assert_eq!(parse_levels_tag("4"), Some(4.0));
        assert_eq!(parse_levels_tag(" 3.5 "), Some(3.5));
        assert_eq!(parse_levels_tag("0"), None);
        assert_eq!(parse_levels_tag("-2"), None);
        assert_eq!(parse_levels_tag("abc"), None);
    }

    #[test]
    fn building_height_priority_rule() {
        let mut tags = BTreeMap::new();
        // 1. 아무것도 없을 때 기본값 (종류별)
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 18.0);
        assert!(est);

        // 2. levels가 들어왔을 때
        tags.insert("building:levels".to_owned(), "5".to_owned());
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 15.0);
        assert!(!est);

        // 3. height가 들어왔을 때 (최우선)
        tags.insert("height".to_owned(), "22.5m".to_owned());
        let (h, est) = determine_building_height(&tags, "apartments");
        assert_eq!(h, 22.5);
        assert!(!est);
    }

    #[test]
    fn polygon_triangulation_convex_and_concave() {
        // 사각형
        let quad = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let tris = triangulate_polygon_2d(&quad);
        assert_eq!(tris.len(), 2);

        // L자형 오목 다각형 (6개 꼭짓점)
        let l_shape = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 5.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        let tris_l = triangulate_polygon_2d(&l_shape);
        assert_eq!(tris_l.len(), 4);
    }

    #[test]
    fn road_overlap_and_corridor_filtering() {
        let origin = LatLon {
            lat: 34.75,
            lon: 127.75,
        };
        let mut tags = BTreeMap::new();
        tags.insert("building".to_owned(), "residential".to_owned());

        // 1. 도로와 겹치는 건물 (x=0, z=-20 근처)
        let overlapping_place = Place {
            id: "way/101".to_owned(),
            name: "중첩 건물".to_owned(),
            kind: "building".to_owned(),
            tags: tags.clone(),
            outline: vec![
                LatLon {
                    lat: 34.75,
                    lon: 127.75,
                },
                LatLon {
                    lat: 34.75,
                    lon: 127.7501,
                },
                LatLon {
                    lat: 34.7501,
                    lon: 127.7501,
                },
                LatLon {
                    lat: 34.7501,
                    lon: 127.75,
                },
            ],
            center: origin,
            entrances: Vec::new(),
        };

        let road_surface = GroundRect::axis_aligned(Vec2::ZERO, Vec2::new(10.0, 50.0));
        let road_centerlines = vec![vec![Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)]];

        let (buildings, _) = build_course_buildings(
            &[overlapping_place],
            origin,
            &road_centerlines,
            &[road_surface],
            150.0,
        );

        // 원점의 도로는 겹치므로 제외되어야 함
        assert!(buildings.is_empty());
    }
}
