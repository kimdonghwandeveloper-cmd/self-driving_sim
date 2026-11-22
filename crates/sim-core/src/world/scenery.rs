//! 지도에 기록된 닫힌 경계를 주행 주변의 시각 전용 지면으로 바꾼다.
use super::{building, map, places::Place};
use glam::Vec2;
use std::collections::BTreeMap;

/// 지면 재질별로 모아 둔 삼각형 꼭짓점.
pub type Scenery = BTreeMap<&'static str, Vec<Vec2>>;

/// 지도 태그를 지면 재질로 분류한다.
pub fn surface_kind(tags: &BTreeMap<String, String>) -> Option<&'static str> {
    let tag = |key| tags.get(key).map(String::as_str);
    if matches!(tag("building"), Some(value) if value != "no") {
        return None;
    }
    if tag("amenity") == Some("parking")
        && matches!(
            tag("parking"),
            None | Some("surface" | "lane" | "street_side")
        )
    {
        return Some("parking");
    }
    if tag("natural") == Some("water") || tag("landuse") == Some("reservoir") {
        return Some("water");
    }
    if matches!(tag("leisure"), Some("park" | "garden"))
        || matches!(tag("landuse"), Some("grass" | "forest" | "meadow"))
        || tag("natural") == Some("wood")
    {
        return Some("green");
    }
    None
}

/// 경로 주변에 있는 단순 다각형을 코스 좌표로 투영하고 재질별로 삼각분할한다.
pub fn build(places: &[Place], origin: map::LatLon, guide: &[Vec2]) -> Scenery {
    let mut layers = Scenery::new();
    if guide.is_empty() {
        return layers;
    }
    let low = guide
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min)
        - Vec2::splat(250.0);
    let high = guide
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max)
        + Vec2::splat(250.0);
    for place in places {
        let Some(kind) = surface_kind(&place.tags) else {
            continue;
        };
        if !(3..=512).contains(&place.outline.len()) {
            continue;
        }
        let polygon: Vec<_> = place
            .outline
            .iter()
            .map(|&p| map::project(origin, p))
            .collect();
        let min = polygon.iter().copied().reduce(Vec2::min).unwrap();
        let max = polygon.iter().copied().reduce(Vec2::max).unwrap();
        if min.cmpgt(high).any() || max.cmplt(low).any() {
            continue;
        }
        let triangles = building::triangulate_polygon_2d(&polygon);
        if triangles.len() != polygon.len() - 2 {
            continue;
        }
        layers
            .entry(kind)
            .or_default()
            .extend(triangles.into_iter().flatten().map(|i| polygon[i]));
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::osm_graph::fixtures::{ORIGIN, offset};

    /// 이름 없는 주차장도 원본 경계의 면적과 코스 좌표를 보존한다.
    #[test]
    fn unnamed_parking_projects_and_triangulates() {
        let ring = [
            offset(10., 10.),
            offset(30., 10.),
            offset(30., 20.),
            offset(10., 20.),
            offset(10., 10.),
        ];
        let geometry: Vec<_> = ring
            .iter()
            .map(|p| serde_json::json!({"lat":p.lat,"lon":p.lon}))
            .collect();
        let json = serde_json::json!({"elements":[{"type":"way","id":1,"tags":{"amenity":"parking"},"geometry":geometry}]});
        let places = super::super::places::parse_places(&json.to_string()).unwrap();
        let layers = build(&places, ORIGIN, &[Vec2::ZERO]);
        let vertices = &layers["parking"];
        assert_eq!(vertices.len(), 6);
        let area: f32 = vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| (t[1] - t[0]).perp_dot(t[2] - t[0]).abs() * 0.5)
            .sum();
        assert!((area - 200.).abs() < 0.01);
        assert!(build(&places, ORIGIN, &[Vec2::splat(2000.)]).is_empty());
    }
}
