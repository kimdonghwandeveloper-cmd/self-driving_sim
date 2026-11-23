//! Mapzen Skadi HGT 타일을 지역 도로 프로필로 잘라낸다.
use std::{collections::BTreeMap, io::Read, path::Path, sync::Mutex, time::Duration};

use flate2::read::GzDecoder;
use glam::Vec2;

use super::{
    elevation::{ElevationFile, RoadElevation},
    map::LatLon,
    map_fetch::write_atomic,
    road_snapshot::sha256_hex,
    route_plan::{in_korea, unlocal},
};

/// 공급원과 디코딩 규칙을 캐시 경로에 고정한다.
pub const CACHE_DIR: &str = "cache/elevation/mapzen-skadi-v1";
const ARCSECONDS: i32 = 3600;
const SIDE: usize = ARCSECONDS as usize + 1;
const RAW_BYTES: usize = SIDE * SIDE * 2;
const MAX_GZIP_BYTES: usize = RAW_BYTES;
const MARGIN_M: f32 = 35.0;
static CACHE_LOCK: Mutex<()> = Mutex::new(());

/// 캐시에 없는 타일을 공개 HTTPS 저장소에서 받는다.
pub fn load_profile(cache: &Path, origin: LatLon, guide: &[Vec2]) -> Result<RoadElevation, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .into();
    load_profile_with(cache, origin, guide, |name| {
        let url = format!(
            "https://s3.amazonaws.com/elevation-tiles-prod/skadi/{}/{name}.hgt.gz",
            &name[..3]
        );
        agent
            .get(url)
            .call()
            .map_err(|e| format!("{name}: {e}"))?
            .body_mut()
            .with_config()
            .limit(MAX_GZIP_BYTES as u64)
            .read_to_vec()
            .map_err(|e| format!("{name}: {e}"))
    })
}

/// 경로 주변 표본만 모은다. 동일 경로는 잘라 둔 프로필을 바로 재사용한다.
pub fn load_profile_with(
    cache: &Path,
    origin: LatLon,
    guide: &[Vec2],
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<RoadElevation, String> {
    if !in_korea(origin) || guide.len() < 2 || guide.iter().any(|point| !point.is_finite()) {
        return Err("invalid Korean elevation route".into());
    }
    let (min, max) = guide
        .iter()
        .fold((guide[0], guide[0]), |(min, max), &point| {
            (min.min(point), max.max(point))
        });
    let nw = unlocal(
        origin,
        [f64::from(min.x - MARGIN_M), f64::from(-min.y + MARGIN_M)],
    );
    let se = unlocal(
        origin,
        [f64::from(max.x + MARGIN_M), f64::from(-max.y - MARGIN_M)],
    );
    let arc = f64::from(ARCSECONDS);
    let north = (nw.lat * arc).ceil() as i32;
    let south = (se.lat * arc).floor() as i32;
    let west = (nw.lon * arc).floor() as i32;
    let east = (se.lon * arc).ceil() as i32;
    if !(2..=512).contains(&(north - south + 1)) || !(2..=512).contains(&(east - west + 1)) {
        return Err("elevation crop exceeds supported size".into());
    }
    let id = sha256_hex(format!("{:?}/{north}/{south}/{west}/{east}", origin).as_bytes());
    let frozen = cache.join(format!("{id}.toml"));
    let _guard = CACHE_LOCK.lock().map_err(|e| e.to_string())?;
    if let Ok(text) = std::fs::read_to_string(&frozen)
        && let Ok(profile) = RoadElevation::parse(&text)
    {
        return Ok(profile);
    }
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let mut tiles = BTreeMap::new();
    let mut samples = Vec::new();
    for lat in (south..=north).rev() {
        let mut row = Vec::new();
        for lon in west..=east {
            let key = (lat.div_euclid(ARCSECONDS), lon.div_euclid(ARCSECONDS));
            let tile = match tiles.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(read_tile(cache, key, &mut fetch)?)
                }
            };
            let y = (ARCSECONDS - lat.rem_euclid(ARCSECONDS)) as usize;
            let x = lon.rem_euclid(ARCSECONDS) as usize;
            let index = (y * SIDE + x) * 2;
            let height = i16::from_be_bytes([tile[index], tile[index + 1]]);
            row.push(f32::from(height));
        }
        samples.push(row);
    }
    let grid = ElevationFile {
        origin: [origin.lat, origin.lon],
        north_west: [f64::from(north) / arc, f64::from(west) / arc],
        samples_m: samples,
    };
    let text = toml::to_string(&grid).map_err(|e| e.to_string())?;
    let profile = RoadElevation::from_grid(grid)?;
    write_atomic(&frozen, text.as_bytes())?;
    Ok(profile)
}

/// 다운로드 전에 검증된 압축 파일을 재사용한다. 손상된 캐시는 다시 받는다.
fn read_tile(
    cache: &Path,
    (lat, lon): (i32, i32),
    fetch: &mut impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let name = format!("N{lat:02}E{lon:03}");
    let path = cache.join(format!("{name}.hgt.gz"));
    if let Ok(file) = std::fs::File::open(&path)
        && let Ok(raw) = decode(file)
    {
        return Ok(raw);
    }
    let bytes = fetch(&name)?;
    let raw = decode(bytes.as_slice()).map_err(|error| format!("{name}: {error}"))?;
    write_atomic(&path, &bytes)?;
    Ok(raw)
}

/// 압축을 풀되 3601×3601개의 16비트 표본을 넘지 못하게 한다.
fn decode(input: impl Read) -> Result<Vec<u8>, String> {
    let mut raw = Vec::with_capacity(RAW_BYTES);
    GzDecoder::new(input.take(MAX_GZIP_BYTES as u64))
        .take(RAW_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() != RAW_BYTES {
        return Err(format!("invalid Skadi tile size: {}", raw.len()));
    }
    Ok(raw)
}
