use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use glam::Vec2;
use sim_core::world::{elevation_tiles::load_profile_with, map::LatLon};

/// HGT 타일의 북쪽이 높은 합성 구배를 실제 gzip 형식으로 만든다.
fn tile() -> Vec<u8> {
    let mut raw = Vec::with_capacity(3601 * 3601 * 2);
    for row in 0..3601i16 {
        for _ in 0..3601 {
            raw.extend_from_slice(&(4000 - row).to_be_bytes());
        }
    }
    let mut zip = GzEncoder::new(Vec::new(), Compression::fast());
    zip.write_all(&raw).unwrap();
    zip.finish().unwrap()
}

/// 날짜변경선과 무관한 국내 경도 경계를 넘고 동일 경로에서 네트워크 없이 재사용한다.
#[test]
fn crosses_tile_boundary_and_reopens_from_cache() {
    let root = std::env::temp_dir().join(format!("sim-dem-test-{}", std::process::id()));
    let origin = LatLon {
        lat: 37.5,
        lon: 126.999,
    };
    let guide = [Vec2::ZERO, Vec2::new(200.0, -40.0)];
    let mut keys = Vec::new();
    let profile = load_profile_with(&root, origin, &guide, |name| {
        keys.push(name.to_owned());
        Ok(tile())
    })
    .unwrap();
    assert_eq!(keys, ["N37E126", "N37E127"]);
    assert!(profile.grade(Vec2::new(100.0, -20.0), -Vec2::Y) > 0.01);
    assert!(profile.grade(Vec2::new(100.0, -20.0), Vec2::Y) < -0.01);
    let reopened = load_profile_with(&root, origin, &guide, |_| panic!("cache miss")).unwrap();
    assert_eq!(profile.height(guide[1]), reopened.height(guide[1]));
    std::fs::remove_dir_all(root).unwrap();
}
