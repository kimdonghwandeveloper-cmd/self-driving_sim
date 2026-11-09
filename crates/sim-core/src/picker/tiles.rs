//! 경로 선택 화면의 OSM 배경 지도 타일(Phase 4-5b). Bevy와 무관한 계산·캐시·받기 부분.
//!
//! - 화면 좌표: 웹 메르카토르(EPSG:3857) 미터를 [`VIEW_ORIGIN`] 기준으로 옮긴 평면이다. x = 동쪽, z = 남쪽
//!   (북쪽이 화면 위). 도로·점·경로·타일 모두 이 좌표로 그린다. 코스 계산은 계속 위경도와 지역 평면으로 한다.
//! - 타일은 **보기 전용**이다. 코스 형상·재현에 쓰지 않는다. 받지 못하면 그 칸은 빈 회색으로 둔다.
//! - OSM 타일 정책(<https://operations.osmfoundation.org/policies/tiles/>)을 따른다: 화면에 보이는 타일만 요청,
//!   미리받기·일괄 받기 없음, 고유 User-Agent, 캐시 헤더 준수(읽지 못하면 7일), 만료 후 조건부 요청,
//!   동시 요청 2개, 실패 시 간격을 늘려 재시도, 403·429는 전체 일시 중지. 자동 검증·시험에서는 받지 않는다.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::world::map::LatLon;
use crate::world::map_fetch;

/// 화면 좌표 원점(대한민국 가운데 부근). f32 화면 좌표의 정밀도를 지키려고 원점을 가까이 둔다.
pub const VIEW_ORIGIN: LatLon = LatLon {
    lat: 36.0,
    lon: 127.8,
};
/// 대한민국 전체 보기 범위(남, 서, 북, 동). 4-3 근사 사각형과 같다.
pub const KOREA_VIEW: [f64; 4] = [33.0, 124.5, 38.7, 132.0];
/// 기본 타일 주소. 설정에서 바꿀 수 있지만, 다른 제공자는 그 약관을 따로 확인해야 한다.
pub const DEFAULT_TILE_URL: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";
/// 타일 디스크 캐시 폴더(저장소 제외).
pub const TILE_CACHE_DIR: &str = "cache/tiles";
/// 타일 한 변(px).
pub const TILE_PX: u32 = 256;
/// 쓰는 확대 수준 범위. OSM 표준 타일은 19까지 있다.
pub const ZOOM_RANGE: (u8, u8) = (5, 19);
/// 응답 크기 상한(바이트, 임시값).
pub const MAX_TILE_BYTES: usize = 512 * 1024;
/// 캐시 헤더를 읽지 못할 때의 유효 기간(초). 정책의 최소 7일.
pub const DEFAULT_TTL_S: u64 = 7 * 24 * 3600;
/// 동시 요청 수(임시값).
pub const MAX_CONCURRENT: usize = 2;
/// 디스크 캐시 타일 수 상한(임시값, 대략 100 MB). 넘으면 오래 쓰지 않은 것부터 지운다.
pub const DISK_BUDGET_TILES: usize = 5000;
/// 웹 메르카토르 지구 반지름(m).
const MERC_RADIUS_M: f64 = 6_378_137.0;
/// z0 타일 한 장의 한 변(m).
const WORLD_M: f64 = 2.0 * std::f64::consts::PI * MERC_RADIUS_M;

/// 위경도를 웹 메르카토르 미터 [x, y]로 바꾼다(y는 북쪽 +).
pub fn mercator(point: LatLon) -> [f64; 2] {
    let lat = point.lat.clamp(-85.05112878, 85.05112878).to_radians();
    [
        MERC_RADIUS_M * point.lon.to_radians(),
        MERC_RADIUS_M * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln(),
    ]
}

/// 웹 메르카토르 미터를 위경도로 바꾼다.
pub fn unmercator([x, y]: [f64; 2]) -> LatLon {
    LatLon {
        lat: (2.0 * (y / MERC_RADIUS_M).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
        lon: (x / MERC_RADIUS_M).to_degrees(),
    }
}

/// 위경도를 화면 좌표(x 동쪽, y = 화면 z 남쪽)로 바꾼다.
pub fn to_view(point: LatLon) -> Vec2 {
    let [x, y] = mercator(point);
    let [ox, oy] = mercator(VIEW_ORIGIN);
    Vec2::new((x - ox) as f32, -(y - oy) as f32)
}

/// 화면 좌표를 위경도로 바꾼다([`to_view`]의 역변환).
pub fn from_view(view: Vec2) -> LatLon {
    let [ox, oy] = mercator(VIEW_ORIGIN);
    unmercator([ox + f64::from(view.x), oy - f64::from(view.y)])
}

/// 위경도 사각형(남, 서, 북, 동)의 화면 범위 (최소, 최대).
pub fn view_rect([south, west, north, east]: [f64; 4]) -> (Vec2, Vec2) {
    let a = to_view(LatLon {
        lat: south,
        lon: west,
    });
    let b = to_view(LatLon {
        lat: north,
        lon: east,
    });
    (a.min(b), a.max(b))
}

/// 타일 번호.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    /// 확대 수준.
    pub z: u8,
    /// 열.
    pub x: u32,
    /// 행(북쪽이 0).
    pub y: u32,
}

impl TileId {
    /// 타일 주소를 만든다.
    pub fn url(&self, template: &str) -> String {
        template
            .replace("{z}", &self.z.to_string())
            .replace("{x}", &self.x.to_string())
            .replace("{y}", &self.y.to_string())
    }

    /// 타일의 화면 범위 (최소, 최대).
    pub fn view_rect(&self) -> (Vec2, Vec2) {
        let size = WORLD_M / f64::from(1_u32 << self.z);
        let [ox, oy] = mercator(VIEW_ORIGIN);
        let left = -WORLD_M / 2.0 + f64::from(self.x) * size - ox;
        let top = WORLD_M / 2.0 - f64::from(self.y) * size - oy;
        (
            Vec2::new(left as f32, -top as f32),
            Vec2::new((left + size) as f32, (-top + size) as f32),
        )
    }

    /// 캐시 파일 경로(이미지, 기록).
    pub fn cache_paths(&self, root: &Path) -> (PathBuf, PathBuf) {
        let dir = root.join(self.z.to_string()).join(self.x.to_string());
        (
            dir.join(format!("{}.png", self.y)),
            dir.join(format!("{}.toml", self.y)),
        )
    }
}

/// 화면 1 px이 `scale` 미터(메르카토르)일 때 쓸 확대 수준.
pub fn zoom_for_scale(scale: f32) -> u8 {
    let z0 = WORLD_M / f64::from(TILE_PX);
    let z = (z0 / f64::from(scale.max(1e-3))).log2().round();
    z.clamp(f64::from(ZOOM_RANGE.0), f64::from(ZOOM_RANGE.1)) as u8
}

/// 화면 범위 (최소, 최대)를 덮는 확대 수준 `z`의 타일 목록. 대한민국 전체 보기 범위 밖은 요청하지 않는다.
/// 개수가 `limit`를 넘으면 가운데에 가까운 것부터 자른다.
pub fn visible_tiles(min: Vec2, max: Vec2, z: u8, limit: usize) -> Vec<TileId> {
    let (korea_min, korea_max) = view_rect(KOREA_VIEW);
    let (min, max) = (min.max(korea_min), max.min(korea_max));
    if min.x >= max.x || min.y >= max.y {
        return Vec::new();
    }
    let size = WORLD_M / f64::from(1_u32 << z);
    let [ox, oy] = mercator(VIEW_ORIGIN);
    let column = |view_x: f32| ((f64::from(view_x) + ox + WORLD_M / 2.0) / size).floor() as i64;
    let row = |view_y: f32| ((WORLD_M / 2.0 - (oy - f64::from(view_y))) / size).floor() as i64;
    let last = (1_i64 << z) - 1;
    let (x0, x1) = (column(min.x).clamp(0, last), column(max.x).clamp(0, last));
    let (y0, y1) = (row(min.y).clamp(0, last), row(max.y).clamp(0, last));
    let center = (min + max) / 2.0;
    let mut tiles: Vec<TileId> = (y0..=y1)
        .flat_map(|y| {
            (x0..=x1).map(move |x| TileId {
                z,
                x: x as u32,
                y: y as u32,
            })
        })
        .collect();
    tiles.sort_by(|a, b| {
        let da = (a.view_rect().0 + a.view_rect().1) / 2.0 - center;
        let db = (b.view_rect().0 + b.view_rect().1) / 2.0 - center;
        da.length_squared()
            .total_cmp(&db.length_squared())
            .then(a.cmp(b))
    });
    tiles.truncate(limit);
    tiles
}

/// 타일 캐시 기록(`<y>.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TileRecord {
    /// 만료 시각(유닉스 초).
    pub expires_at: u64,
    /// 서버 `ETag`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// 서버 `Last-Modified`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

/// HTTP 날짜(`Sun, 06 Nov 1994 08:49:37 GMT`, RFC 7231 IMF-fixdate)를 유닉스 초로 바꾼다. 형식이 다르면 `None`.
pub fn parse_http_date(text: &str) -> Option<u64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, "GMT"] = parts[..] else {
        return None;
    };
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|name| *name == month)? as i64
        + 1;
    let (day, year) = (day.parse::<i64>().ok()?, year.parse::<i64>().ok()?);
    let clock: Vec<i64> = time
        .split(':')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    let [hour, minute, second] = clock[..] else {
        return None;
    };
    // H. Hinnant의 days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

/// 응답 머리글로 만료 시각을 정한다. `Cache-Control`의 `max-age`(`no-store`·`no-cache`는 0초)를 먼저 보고,
/// 없으면 `Expires` 날짜, 둘 다 읽을 수 없으면 [`DEFAULT_TTL_S`]다.
pub fn expires_at(now: u64, cache_control: Option<&str>, expires: Option<&str>) -> u64 {
    let from_expires = || expires.and_then(parse_http_date).map(|time| time.max(now));
    let Some(value) = cache_control else {
        return from_expires().unwrap_or(now + DEFAULT_TTL_S);
    };
    let mut ttl = None;
    for part in value.split(',').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if lower == "no-store" || lower == "no-cache" {
            ttl = Some(0);
        } else if let Some(seconds) = lower.strip_prefix("max-age=")
            && let Ok(seconds) = seconds.trim().parse::<u64>()
        {
            ttl = Some(ttl.unwrap_or(seconds).min(seconds));
        }
    }
    match ttl {
        Some(ttl) => now + ttl,
        None => from_expires().unwrap_or(now + DEFAULT_TTL_S),
    }
}

/// PNG 서명과 IHDR의 너비·높이가 [`TILE_PX`]인지, 크기가 상한 이하인지 검사하고, 실제로 끝까지 디코딩해 본다
/// (잘리거나 CRC가 깨진 PNG를 정상 캐시로 저장하지 않게).
pub fn check_png(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_TILE_BYTES {
        return Err(format!(
            "tile is {} bytes (limit {MAX_TILE_BYTES})",
            bytes.len()
        ));
    }
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return Err("tile is not a PNG image".to_owned());
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if (width, height) != (TILE_PX, TILE_PX) {
        return Err(format!(
            "tile is {width}x{height}, expected {TILE_PX}x{TILE_PX}"
        ));
    }
    decode_png(bytes).map_err(|error| format!("tile PNG cannot be decoded: {error}"))
}

/// PNG를 끝까지 디코딩해 본다(잘림·CRC 오류 검출). 픽셀은 버린다.
fn decode_png(bytes: &[u8]) -> Result<(), png::DecodingError> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info()?;
    let size = reader
        .output_buffer_size()
        .ok_or(png::DecodingError::LimitsExceeded)?;
    let mut pixels = vec![0; size];
    reader.next_frame(&mut pixels)?;
    Ok(())
}

/// 지금 시각(유닉스 초).
pub fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// 캐시에서 타일을 읽는다. (이미지, 기록). 이미지가 PNG 검사를 통과하지 못하면(손상) 캐시 파일을 지우고 `None`.
pub fn read_cached(root: &Path, tile: TileId) -> Option<(Vec<u8>, TileRecord)> {
    let (image, record) = tile.cache_paths(root);
    let bytes = std::fs::read(&image).ok()?;
    if check_png(&bytes).is_err() {
        remove_cached(root, tile);
        return None;
    }
    let record = std::fs::read_to_string(&record)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    Some((bytes, record))
}

/// 타일 캐시 파일(이미지·기록)을 지운다.
pub fn remove_cached(root: &Path, tile: TileId) {
    let (image, record) = tile.cache_paths(root);
    let _ = std::fs::remove_file(image);
    let _ = std::fs::remove_file(record);
}

/// 타일과 기록을 캐시에 쓴다(임시 이름으로 쓴 뒤 바꿔 단다).
pub fn write_cached(
    root: &Path,
    tile: TileId,
    bytes: &[u8],
    record: &TileRecord,
) -> Result<(), String> {
    let (image, meta) = tile.cache_paths(root);
    if let Some(dir) = image.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    }
    map_fetch::write_atomic(&image, bytes)?;
    let text = toml::to_string(record).map_err(|error| error.to_string())?;
    map_fetch::write_atomic(&meta, text.as_bytes())
}

/// 디스크 캐시가 `budget`장을 넘으면 오래 쓰지 않은(수정 시각이 오래된) 타일부터 지운다. 지운 수를 돌려준다.
pub fn prune_cache(root: &Path, budget: usize) -> usize {
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "png") {
                let time = entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .unwrap_or(UNIX_EPOCH);
                files.push((time, path));
            }
        }
    }
    if files.len() <= budget {
        return 0;
    }
    files.sort();
    let extra = files.len() - budget;
    for (_, path) in files.iter().take(extra) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("toml"));
    }
    extra
}

/// 타일 서버 응답.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TileReply {
    /// HTTP 상태.
    pub status: u16,
    /// 본문.
    pub body: Vec<u8>,
    /// `Cache-Control`.
    pub cache_control: Option<String>,
    /// `ETag`.
    pub etag: Option<String>,
    /// `Last-Modified`.
    pub last_modified: Option<String>,
    /// `Expires`.
    pub expires: Option<String>,
}

/// 타일 전송 수단. 시험은 가짜 전송 수단만 쓴다(외부 요청 없음).
pub trait TileTransport: Send + Sync {
    /// `url`을 요청한다. `record`가 있으면 조건부 요청(`If-None-Match`/`If-Modified-Since`)으로 보낸다.
    fn get(
        &self,
        url: &str,
        user_agent: &str,
        record: Option<&TileRecord>,
    ) -> Result<TileReply, String>;
}

/// `ureq`로 타일을 받는 전송 수단.
pub struct UreqTileTransport;

impl TileTransport for UreqTileTransport {
    /// HTTPS GET을 보내고 상태·본문·캐시 머리글을 돌려준다. 본문은 [`MAX_TILE_BYTES`]까지만 읽는다.
    fn get(
        &self,
        url: &str,
        user_agent: &str,
        record: Option<&TileRecord>,
    ) -> Result<TileReply, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.get(url).header("User-Agent", user_agent);
        if let Some(record) = record {
            if let Some(etag) = &record.etag {
                request = request.header("If-None-Match", etag);
            }
            if let Some(modified) = &record.last_modified {
                request = request.header("If-Modified-Since", modified);
            }
        }
        let mut response = request
            .call()
            .map_err(|error| format!("tile request failed: {error}"))?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let (cache_control, etag, last_modified, expires) = (
            header("cache-control"),
            header("etag"),
            header("last-modified"),
            header("expires"),
        );
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_TILE_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|error| format!("tile body failed: {error}"))?;
        Ok(TileReply {
            status,
            body,
            cache_control,
            etag,
            last_modified,
            expires,
        })
    }
}

/// 타일 받기 설정.
#[derive(Debug, Clone, PartialEq)]
pub struct TileSettings {
    /// 네트워크로 받을지. 자동 검증(`--smoke-test`·`--capture-views`)과 시험에서는 `false`다.
    pub network: bool,
    /// 타일 주소 형식.
    pub url_template: String,
    /// User-Agent.
    pub user_agent: String,
    /// 디스크 캐시 폴더.
    pub cache_root: PathBuf,
}

impl Default for TileSettings {
    /// OSM 표준 타일, 공용 User-Agent, `cache/tiles`. 네트워크는 기본으로 켠다(앱이 자동 검증이면 끈다).
    fn default() -> Self {
        Self {
            network: true,
            url_template: DEFAULT_TILE_URL.to_owned(),
            user_agent: map_fetch::USER_AGENT.to_owned(),
            cache_root: PathBuf::from(TILE_CACHE_DIR),
        }
    }
}

/// 받기 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum TileOutcome {
    /// PNG 바이트(캐시 또는 새로 받음)와 만료 시각(유닉스 초). 만료되면 화면은 그대로 두고 다시 확인한다.
    Image {
        /// PNG 바이트.
        bytes: Vec<u8>,
        /// 만료 시각.
        expires_at: u64,
    },
    /// 받지 못함(회색 칸으로 둔다).
    Failed(String),
}

/// 작업자 공용 상태.
struct Shared {
    /// 대기 중인 요청(가장 최근 화면 기준).
    queue: VecDeque<TileId>,
    /// 지금 받는 중인 타일.
    in_flight: HashSet<TileId>,
    /// 끝낼지.
    stop: bool,
}

/// 백그라운드 타일 받기. 같은 타일 요청은 하나로 합치고, 화면이 바뀌면 오래된 대기 요청을 버린다.
pub struct TileFetcher {
    /// 공용 상태.
    shared: Arc<(Mutex<Shared>, std::sync::Condvar)>,
    /// 결과 수신.
    results: Mutex<Receiver<(TileId, TileOutcome)>>,
    /// 타일별 (실패 횟수, 다음 시도 가능 시각 초).
    backoff: HashMap<TileId, (u32, u64)>,
    /// 전체 일시 중지가 풀리는 시각(403·429 뒤).
    paused_until: Arc<Mutex<u64>>,
    /// 이미 꺼냈지만 아직 [`TileFetcher::poll`]로 넘기지 않은 결과.
    ready: Vec<(TileId, TileOutcome)>,
    /// 디스크 캐시 폴더.
    cache_root: PathBuf,
}

impl TileFetcher {
    /// 작업자 [`MAX_CONCURRENT`]개를 띄운다. 시작할 때 디스크 캐시 예산을 넘은 타일을 정리한다.
    pub fn start(settings: TileSettings, transport: Arc<dyn TileTransport>) -> Self {
        let shared = Arc::new((
            Mutex::new(Shared {
                queue: VecDeque::new(),
                in_flight: HashSet::new(),
                stop: false,
            }),
            std::sync::Condvar::new(),
        ));
        let paused_until = Arc::new(Mutex::new(0_u64));
        let (sender, receiver) = channel();
        let root = settings.cache_root.clone();
        let _ = std::thread::Builder::new()
            .name("tile-prune".to_owned())
            .spawn(move || prune_cache(&root, DISK_BUDGET_TILES));
        for index in 0..MAX_CONCURRENT {
            let (shared, settings, transport, sender, paused) = (
                Arc::clone(&shared),
                settings.clone(),
                Arc::clone(&transport),
                sender.clone(),
                Arc::clone(&paused_until),
            );
            let _ = std::thread::Builder::new()
                .name(format!("tile-{index}"))
                .spawn(move || worker(&shared, &settings, transport.as_ref(), &sender, &paused));
        }
        Self {
            shared,
            results: Mutex::new(receiver),
            backoff: HashMap::new(),
            paused_until,
            ready: Vec::new(),
            cache_root: settings.cache_root,
        }
    }

    /// 아직 시작하지 않은 대기 요청을 모두 버린다(화면을 나갈 때). 이미 전송 중인 요청은 끝나도록 둔다.
    pub fn cancel_pending(&mut self) {
        let (lock, _) = &*self.shared;
        lock.lock().expect("tile queue lock").queue.clear();
    }

    /// 받은 이미지를 화면에서 디코딩하지 못했을 때: 캐시를 지우고 실패로 셈해 다시 시도 간격을 늘린다.
    pub fn report_bad(&mut self, tile: TileId) {
        remove_cached(&self.cache_root, tile);
        let entry = self.backoff.entry(tile).or_insert((0, 0));
        entry.0 += 1;
        entry.1 = now_s() + (5_u64 << entry.0.min(6)).min(300);
    }

    /// 지금 화면에 필요한 타일 중 아직 없는 것을 요청한다. 대기열은 이 목록으로 바꾼다(오래된 요청 폐기).
    /// 실패 뒤 다시 시도 시각이 안 된 타일과 일시 중지 중의 요청은 넣지 않는다.
    pub fn request(&mut self, wanted: &[TileId]) {
        // 끝난 결과를 먼저 꺼내 실패 간격을 반영한다(실패 직후 같은 타일을 다시 넣지 않게).
        self.collect();
        let now = now_s();
        let paused = *self.paused_until.lock().expect("pause lock") > now;
        let (lock, wake) = &*self.shared;
        let mut shared = lock.lock().expect("tile queue lock");
        shared.queue.clear();
        for tile in wanted {
            let waiting = self.backoff.get(tile).is_some_and(|&(_, next)| next > now);
            if !waiting && !paused && !shared.in_flight.contains(tile) {
                shared.queue.push_back(*tile);
            }
        }
        wake.notify_all();
    }

    /// 403·429로 멈춘 받기가 다시 풀리는 유닉스 시각(초). 멈춘 적이 없으면 0이다(지난 시각일 수 있음).
    pub fn paused_until_s(&self) -> u64 {
        *self.paused_until.lock().expect("pause lock")
    }

    /// 끝난 결과를 모두 꺼낸다(기다리지 않음).
    pub fn poll(&mut self) -> Vec<(TileId, TileOutcome)> {
        self.collect();
        std::mem::take(&mut self.ready)
    }

    /// 결과 채널을 비워 [`Self::ready`]에 모으고, 실패한 타일은 다시 시도 간격을 늘린다(5 s × 2^n, 최대 5분).
    fn collect(&mut self) {
        let results: Vec<(TileId, TileOutcome)> = self
            .results
            .lock()
            .expect("tile result lock")
            .try_iter()
            .collect();
        let now = now_s();
        for (tile, outcome) in &results {
            match outcome {
                TileOutcome::Image { .. } => {
                    self.backoff.remove(tile);
                }
                TileOutcome::Failed(_) => {
                    let entry = self.backoff.entry(*tile).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 = now + (5_u64 << entry.0.min(6)).min(300);
                }
            }
        }
        self.ready.extend(results);
    }
}

impl Drop for TileFetcher {
    /// 작업자에게 끝내라고 알린다(받는 중인 요청은 끝난 뒤 버려진다).
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        if let Ok(mut shared) = lock.lock() {
            shared.stop = true;
            shared.queue.clear();
        }
        wake.notify_all();
    }
}

/// 작업자: 대기열에서 하나씩 꺼내 [`fetch_tile`]로 처리하고 결과를 보낸다.
fn worker(
    shared: &(Mutex<Shared>, std::sync::Condvar),
    settings: &TileSettings,
    transport: &dyn TileTransport,
    sender: &Sender<(TileId, TileOutcome)>,
    paused_until: &Mutex<u64>,
) {
    let (lock, wake) = shared;
    loop {
        let tile = {
            let mut state = lock.lock().expect("tile queue lock");
            loop {
                if state.stop {
                    return;
                }
                if let Some(tile) = state.queue.pop_front() {
                    state.in_flight.insert(tile);
                    break tile;
                }
                state = wake.wait(state).expect("tile queue wait");
            }
        };
        let outcome = fetch_tile(settings, transport, tile, paused_until);
        let paused = *paused_until.lock().expect("pause lock") > now_s();
        {
            let mut state = lock.lock().expect("tile queue lock");
            state.in_flight.remove(&tile);
            // 403·429로 멈췄으면 남은 대기 요청을 버린다(다른 작업자도 꺼내지 않게).
            if paused {
                state.queue.clear();
            }
        }
        if sender.send((tile, outcome)).is_err() {
            return;
        }
    }
}

/// 타일 하나를 처리한다: 유효한 캐시면 그대로, 만료됐으면 조건부 요청(304면 캐시 유지·만료 갱신),
/// 없으면 새로 받는다. 네트워크가 꺼져 있거나 403·429 뒤 일시 중지 중이면 전송하지 않고 캐시만 쓴다
/// (만료돼도 보여 준다). 403·429를 받으면 1분간 전체 중지한다.
pub fn fetch_tile(
    settings: &TileSettings,
    transport: &dyn TileTransport,
    tile: TileId,
    paused_until: &Mutex<u64>,
) -> TileOutcome {
    let now = now_s();
    let cached = read_cached(&settings.cache_root, tile);
    let paused = *paused_until.lock().expect("pause lock") > now;
    if let Some((bytes, record)) = &cached
        && (record.expires_at > now || !settings.network || paused)
    {
        return TileOutcome::Image {
            bytes: bytes.clone(),
            expires_at: record.expires_at.max(now + 60),
        };
    }
    if !settings.network {
        return TileOutcome::Failed("offline and not cached".to_owned());
    }
    if paused {
        return TileOutcome::Failed("tile server asked us to pause".to_owned());
    }
    // 실패해도 만료된 캐시를 계속 보여 주되, 1분 뒤 다시 확인한다.
    let stale = |cached: Option<(Vec<u8>, TileRecord)>, error: String| match cached {
        Some((bytes, _)) => TileOutcome::Image {
            bytes,
            expires_at: now + 60,
        },
        None => TileOutcome::Failed(error),
    };
    let url = tile.url(&settings.url_template);
    let reply = match transport.get(&url, &settings.user_agent, cached.as_ref().map(|(_, r)| r)) {
        Ok(reply) => reply,
        Err(error) => return stale(cached, error),
    };
    let expiry = expires_at(
        now,
        reply.cache_control.as_deref(),
        reply.expires.as_deref(),
    );
    match reply.status {
        200 => {
            if let Err(error) = check_png(&reply.body) {
                return stale(cached, error);
            }
            let record = TileRecord {
                expires_at: expiry,
                etag: reply.etag,
                last_modified: reply.last_modified,
            };
            let _ = write_cached(&settings.cache_root, tile, &reply.body, &record);
            TileOutcome::Image {
                bytes: reply.body,
                expires_at: expiry,
            }
        }
        304 => match cached {
            Some((bytes, old)) => {
                let record = TileRecord {
                    expires_at: expiry,
                    etag: reply.etag.or(old.etag),
                    last_modified: reply.last_modified.or(old.last_modified),
                };
                let _ = write_cached(&settings.cache_root, tile, &bytes, &record);
                TileOutcome::Image {
                    bytes,
                    expires_at: expiry,
                }
            }
            None => TileOutcome::Failed("304 without a cached tile".to_owned()),
        },
        status => {
            if status == 403 || status == 429 {
                *paused_until.lock().expect("pause lock") = now + 60;
            }
            stale(cached, format!("tile server answered HTTP {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC-32(PNG 청크용, 표 없이 비트 단위).
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// 실제로 디코딩되는 256×256 회색 RGB PNG(압축하지 않은 deflate 블록). 시험에서 직접 만든다.
    fn fake_png() -> Vec<u8> {
        let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        };
        let mut raw = Vec::new();
        for _ in 0..256 {
            raw.push(0);
            raw.extend(std::iter::repeat_n(200_u8, 256 * 3));
        }
        let mut zlib = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (index, block) in blocks.iter().enumerate() {
            zlib.push(u8::from(index + 1 == blocks.len()));
            let len = block.len() as u16;
            zlib.extend_from_slice(&len.to_le_bytes());
            zlib.extend_from_slice(&(!len).to_le_bytes());
            zlib.extend_from_slice(block);
        }
        let (mut a, mut b) = (1_u32, 0_u32);
        for &byte in &raw {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&256_u32.to_be_bytes());
        header.extend_from_slice(&256_u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &header);
        chunk(&mut png, b"IDAT", &zlib);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    /// 결과가 이미지이고 바이트가 `bytes`인지.
    fn is_image(outcome: &TileOutcome, bytes: &[u8]) -> bool {
        matches!(outcome, TileOutcome::Image { bytes: got, .. } if got == bytes)
    }

    /// 정해진 응답을 돌려주고 요청 기록을 남기는 가짜 전송 수단(외부 요청 없음).
    struct FakeTransport {
        replies: Mutex<VecDeque<TileReply>>,
        seen: Mutex<Vec<(String, Option<TileRecord>)>>,
    }

    impl TileTransport for FakeTransport {
        /// 다음 정해진 응답을 돌려준다.
        fn get(
            &self,
            url: &str,
            _: &str,
            record: Option<&TileRecord>,
        ) -> Result<TileReply, String> {
            self.seen
                .lock()
                .unwrap()
                .push((url.to_owned(), record.cloned()));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no reply".to_owned())
        }
    }

    /// 시험마다 비운 캐시 폴더.
    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sds-tiles-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 위경도 ↔ 메르카토르 ↔ 화면 좌표 왕복 오차가 작은지(여수, 서울, 제주, 독도 부근).
    #[test]
    fn projection_round_trip() {
        for (lat, lon) in [
            (34.75, 127.75),
            (37.5665, 126.978),
            (33.25, 126.56),
            (37.24, 131.87),
        ] {
            let point = LatLon { lat, lon };
            let back = from_view(to_view(point));
            // f32 화면 좌표(원점에서 수백 km)의 반올림 오차: 1e-6°(약 0.1 m) 이하.
            assert!(
                (back.lat - lat).abs() < 1e-6 && (back.lon - lon).abs() < 1e-6,
                "{back:?}"
            );
            let exact = unmercator(mercator(point));
            assert!((exact.lat - lat).abs() < 1e-9 && (exact.lon - lon).abs() < 1e-9);
        }
        // 북쪽이 화면 위(-y), 동쪽이 오른쪽(+x).
        let center = to_view(VIEW_ORIGIN);
        assert!(center.length() < 1e-3);
        assert!(
            to_view(LatLon {
                lat: 37.0,
                lon: 127.8
            })
            .y < 0.0
        );
        assert!(
            to_view(LatLon {
                lat: 36.0,
                lon: 128.0
            })
            .x > 0.0
        );
    }

    /// 알려진 타일 번호: 서울시청(37.5665, 126.978)은 z16에서 x=55883, y=25378(표준 슬리피 맵 공식 x=(lon+180)/360·2^z, y=(1−ln(tan φ+sec φ)/π)/2·2^z로 따로 계산).
    #[test]
    fn tile_numbers_match_slippy_map_formula() {
        let point = to_view(LatLon {
            lat: 37.5665,
            lon: 126.978,
        });
        let tiles = visible_tiles(point - Vec2::splat(1.0), point + Vec2::splat(1.0), 16, 10);
        assert_eq!(
            tiles,
            vec![TileId {
                z: 16,
                x: 55883,
                y: 25378
            }]
        );
        let (min, max) = tiles[0].view_rect();
        assert!(point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y);
        assert_eq!(
            tiles[0].url(DEFAULT_TILE_URL),
            "https://tile.openstreetmap.org/16/55883/25378.png"
        );
    }

    /// 확대 수준 선택과, 화면 범위의 타일 개수·상한·대한민국 밖 제외.
    #[test]
    fn zoom_and_visible_tile_limits() {
        assert_eq!(zoom_for_scale(0.6), 18);
        assert_eq!(zoom_for_scale(2446.0), 6);
        assert_eq!(zoom_for_scale(1.0e6), ZOOM_RANGE.0);
        assert_eq!(zoom_for_scale(0.01), ZOOM_RANGE.1);
        let (korea_min, korea_max) = view_rect(KOREA_VIEW);
        let all = visible_tiles(korea_min, korea_max, 6, 1000);
        assert!((2..=12).contains(&all.len()), "{}", all.len());
        assert_eq!(visible_tiles(korea_min, korea_max, 12, 30).len(), 30);
        let far = to_view(LatLon {
            lat: 45.0,
            lon: 140.0,
        });
        assert!(visible_tiles(far, far + Vec2::splat(1000.0), 12, 30).is_empty());
    }

    /// 캐시 머리글: max-age, no-cache, Expires 날짜, 읽을 수 없음(7일).
    #[test]
    fn cache_header_rules() {
        assert_eq!(expires_at(100, Some("public, max-age=3600"), None), 3700);
        assert_eq!(expires_at(100, Some("no-cache"), None), 100);
        assert_eq!(expires_at(100, Some("private"), None), 100 + DEFAULT_TTL_S);
        assert_eq!(expires_at(100, None, None), 100 + DEFAULT_TTL_S);
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("06 Nov 1994"), None);
        let now = 1_793_000_000;
        assert_eq!(
            expires_at(now, None, Some("Mon, 26 Oct 2026 08:33:20 GMT")),
            now + 3600
        );
        // max-age가 Expires보다 앞서고, 지난 Expires는 바로 만료다.
        assert_eq!(
            expires_at(
                now,
                Some("max-age=10"),
                Some("Mon, 26 Oct 2026 08:33:20 GMT")
            ),
            now + 10
        );
        assert_eq!(
            expires_at(now, None, Some("Sun, 06 Nov 1994 08:49:37 GMT")),
            now
        );
    }

    /// PNG 검사: 정상, 다른 치수, PNG 아님, 너무 큼.
    #[test]
    fn png_checks() {
        assert!(check_png(&fake_png()).is_ok());
        let mut small = fake_png();
        small[19] = 128;
        assert!(check_png(&small).is_err());
        assert!(check_png(b"<html>").is_err());
        let mut big = fake_png();
        big.resize(MAX_TILE_BYTES + 1, 0);
        assert!(check_png(&big).is_err());
        // 머리부는 맞지만 잘린 PNG와, 데이터 1바이트를 바꿔 CRC가 맞지 않는 PNG는 디코딩 검사에서 거부한다.
        let png = fake_png();
        assert!(check_png(&png[..png.len() / 2]).is_err(), "truncated");
        let mut corrupt = png.clone();
        corrupt[1000] ^= 0x55;
        assert!(check_png(&corrupt).is_err(), "bad CRC");
    }

    /// 손상된 캐시(잘림·CRC)는 지우고 다시 받으며, 200으로 받은 손상 PNG는 캐시에 저장하지 않는다.
    #[test]
    fn corrupt_tiles_are_not_cached() {
        let root = temp_root("corrupt");
        let png = fake_png();
        let tile = TileId {
            z: 11,
            x: 1750,
            y: 806,
        };
        write_cached(
            &root,
            tile,
            &png[..png.len() / 2],
            &TileRecord {
                expires_at: u64::MAX,
                ..TileRecord::default()
            },
        )
        .unwrap();
        assert!(read_cached(&root, tile).is_none());
        assert!(
            !tile.cache_paths(&root).0.exists(),
            "corrupt cache file removed"
        );
        let transport = FakeTransport {
            replies: Mutex::new(VecDeque::from(vec![
                TileReply {
                    status: 200,
                    body: png[..png.len() - 20].to_vec(),
                    ..TileReply::default()
                },
                TileReply {
                    status: 200,
                    body: png.clone(),
                    ..TileReply::default()
                },
            ])),
            seen: Mutex::new(Vec::new()),
        };
        let settings = TileSettings {
            cache_root: root.clone(),
            ..TileSettings::default()
        };
        let paused = Mutex::new(0);
        assert!(matches!(
            fetch_tile(&settings, &transport, tile, &paused),
            TileOutcome::Failed(_)
        ));
        assert!(
            !tile.cache_paths(&root).0.exists(),
            "bad download not cached"
        );
        assert!(is_image(
            &fetch_tile(&settings, &transport, tile, &paused),
            &png
        ));
        assert!(read_cached(&root, tile).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 새로 받기 → 캐시 재사용(요청 없음) → 만료 후 조건부 요청 304(기존 이미지 유지·만료 갱신) →
    /// 손상 캐시는 새로 받기 → 실패하면 회색, 403이면 일시 중지, 네트워크가 꺼지면 캐시만.
    #[test]
    fn fetch_cache_and_revalidation() {
        let root = temp_root("fetch");
        let png = fake_png();
        let transport = FakeTransport {
            replies: Mutex::new(VecDeque::from(vec![
                TileReply {
                    status: 200,
                    body: png.clone(),
                    cache_control: Some("max-age=0".to_owned()),
                    etag: Some("\"v1\"".to_owned()),
                    ..TileReply::default()
                },
                TileReply {
                    status: 304,
                    cache_control: Some("max-age=600".to_owned()),
                    ..TileReply::default()
                },
                TileReply {
                    status: 200,
                    body: png.clone(),
                    ..TileReply::default()
                },
                TileReply {
                    status: 403,
                    ..TileReply::default()
                },
            ])),
            seen: Mutex::new(Vec::new()),
        };
        let settings = TileSettings {
            cache_root: root.clone(),
            ..TileSettings::default()
        };
        let paused = Mutex::new(0);
        let tile = TileId {
            z: 10,
            x: 875,
            y: 403,
        };
        assert!(is_image(
            &fetch_tile(&settings, &transport, tile, &paused),
            &png
        ));
        // max-age=0이라 바로 만료: 다음 요청은 ETag를 붙인 조건부 요청이고 304면 기존 이미지를 쓴다.
        assert!(is_image(
            &fetch_tile(&settings, &transport, tile, &paused),
            &png
        ));
        let seen = transport.seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[1].1.as_ref().and_then(|r| r.etag.clone()).as_deref(),
            Some("\"v1\"")
        );
        let record = read_cached(&root, tile).unwrap().1;
        assert!(record.expires_at > now_s() + 500);
        // 유효한 캐시는 요청 없이 쓴다.
        assert!(is_image(
            &fetch_tile(&settings, &transport, tile, &paused),
            &png
        ));
        assert_eq!(transport.seen.lock().unwrap().len(), 2);
        // 손상된 캐시는 버리고 새로 받는다.
        std::fs::write(tile.cache_paths(&root).0, b"broken").unwrap();
        assert!(is_image(
            &fetch_tile(&settings, &transport, tile, &paused),
            &png
        ));
        // 캐시 없는 타일에 403: 실패(회색)이고 1분 일시 중지.
        let other = TileId {
            z: 10,
            x: 876,
            y: 403,
        };
        assert!(matches!(
            fetch_tile(&settings, &transport, other, &paused),
            TileOutcome::Failed(_)
        ));
        assert!(*paused.lock().unwrap() > now_s());
        // 네트워크를 끄면 요청하지 않는다.
        let offline = TileSettings {
            network: false,
            ..settings.clone()
        };
        let before = transport.seen.lock().unwrap().len();
        assert!(matches!(
            fetch_tile(
                &offline,
                &transport,
                TileId {
                    z: 10,
                    x: 877,
                    y: 403
                },
                &paused
            ),
            TileOutcome::Failed(_)
        ));
        assert_eq!(transport.seen.lock().unwrap().len(), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 디스크 예산을 넘으면 오래된 타일부터 지운다.
    #[test]
    fn disk_budget_prunes_oldest() {
        let root = temp_root("prune");
        for x in 0..5 {
            write_cached(
                &root,
                TileId { z: 3, x, y: 0 },
                &fake_png(),
                &TileRecord::default(),
            )
            .unwrap();
        }
        assert_eq!(prune_cache(&root, 3), 2);
        assert_eq!(prune_cache(&root, 3), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 주소가 `ok`면 PNG를, 아니면 오류를 돌려주는 가짜 전송 수단(작업자 순서와 무관).
    struct ByUrl {
        ok: String,
        seen: Mutex<Vec<String>>,
    }

    impl TileTransport for ByUrl {
        /// `ok` 주소만 성공한다.
        fn get(&self, url: &str, _: &str, _: Option<&TileRecord>) -> Result<TileReply, String> {
            self.seen.lock().unwrap().push(url.to_owned());
            if url == self.ok {
                Ok(TileReply {
                    status: 200,
                    body: fake_png(),
                    ..TileReply::default()
                })
            } else {
                Err("unreachable".to_owned())
            }
        }
    }

    /// 백그라운드 받기: 같은 타일을 여러 번 요청해도 결과가 돌아오고, 실패한 타일은 바로 다시 요청하지 않는다.
    #[test]
    fn fetcher_deduplicates_and_backs_off() {
        let root = temp_root("fetcher");
        let good = TileId {
            z: 9,
            x: 437,
            y: 201,
        };
        let bad = TileId {
            z: 9,
            x: 438,
            y: 201,
        };
        let transport = Arc::new(ByUrl {
            ok: good.url(DEFAULT_TILE_URL),
            seen: Mutex::new(Vec::new()),
        });
        let mut fetcher = TileFetcher::start(
            TileSettings {
                cache_root: root.clone(),
                ..TileSettings::default()
            },
            transport.clone(),
        );
        let mut done = HashMap::new();
        for _ in 0..300 {
            let missing: Vec<TileId> = [good, good, bad]
                .into_iter()
                .filter(|tile| !done.contains_key(tile))
                .collect();
            fetcher.request(&missing);
            for (tile, outcome) in fetcher.poll() {
                done.insert(tile, outcome);
            }
            if done.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(done.get(&good), Some(TileOutcome::Image { .. })));
        assert!(matches!(done.get(&bad), Some(TileOutcome::Failed(_))));
        // 실패 전에 겹쳐 들어간 요청의 늦은 결과를 비운다.
        std::thread::sleep(Duration::from_millis(100));
        fetcher.poll();
        let requests = transport.seen.lock().unwrap().len();
        fetcher.request(&[bad]);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            fetcher.poll().is_empty(),
            "failed tile is not retried right away"
        );
        assert_eq!(transport.seen.lock().unwrap().len(), requests);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 매번 정해진 상태로 답하고 호출 수를 세는 가짜 전송 수단. `delay`만큼 기다린 뒤 답한다.
    struct Counting {
        status: u16,
        delay: Duration,
        calls: Mutex<usize>,
    }

    impl TileTransport for Counting {
        /// 호출 수를 세고 `delay` 뒤 정해진 상태(200이면 정상 PNG)로 답한다.
        fn get(&self, _: &str, _: &str, _: Option<&TileRecord>) -> Result<TileReply, String> {
            *self.calls.lock().unwrap() += 1;
            std::thread::sleep(self.delay);
            Ok(TileReply {
                status: self.status,
                body: if self.status == 200 {
                    fake_png()
                } else {
                    Vec::new()
                },
                ..TileReply::default()
            })
        }
    }

    /// 화면 하나 분량(24장)을 요청했는데 429가 오면, 이미 전송 중이던 요청 말고는 더 보내지 않는다(작업자 수준).
    #[test]
    fn rate_limit_stops_queued_requests() {
        let root = temp_root("pause");
        let transport = Arc::new(Counting {
            status: 429,
            delay: Duration::from_millis(20),
            calls: Mutex::new(0),
        });
        let mut fetcher = TileFetcher::start(
            TileSettings {
                cache_root: root.clone(),
                ..TileSettings::default()
            },
            transport.clone(),
        );
        let tiles: Vec<TileId> = (0..24)
            .map(|x| TileId {
                z: 12,
                x: 3500 + x,
                y: 1600,
            })
            .collect();
        fetcher.request(&tiles);
        std::thread::sleep(Duration::from_millis(400));
        let calls = *transport.calls.lock().unwrap();
        assert!(calls <= MAX_CONCURRENT, "{calls} requests after 429");
        fetcher.request(&tiles);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            *transport.calls.lock().unwrap(),
            calls,
            "paused for a minute"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 화면을 나가며 대기 요청을 버리면, 이미 전송 중이던 요청 말고는 더 받지 않는다.
    #[test]
    fn cancel_pending_drops_queued_tiles() {
        let root = temp_root("cancel");
        let transport = Arc::new(Counting {
            status: 200,
            delay: Duration::from_millis(40),
            calls: Mutex::new(0),
        });
        let mut fetcher = TileFetcher::start(
            TileSettings {
                cache_root: root.clone(),
                ..TileSettings::default()
            },
            transport.clone(),
        );
        let tiles: Vec<TileId> = (0..20)
            .map(|x| TileId {
                z: 12,
                x: 3600 + x,
                y: 1600,
            })
            .collect();
        fetcher.request(&tiles);
        std::thread::sleep(Duration::from_millis(5));
        fetcher.cancel_pending();
        std::thread::sleep(Duration::from_millis(300));
        let calls = *transport.calls.lock().unwrap();
        assert!(calls <= MAX_CONCURRENT, "{calls} requests after cancel");
        let _ = std::fs::remove_dir_all(&root);
    }
}
