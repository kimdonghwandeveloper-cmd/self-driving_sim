//! 지도 데이터 받기와 캐시(Phase 4-4).
//!
//! 사용자가 요청할 때만 Overpass API로 bbox 안의 `highway` way를 `out body geom`으로 받는다. 받은 원본은 수정하지 않고
//! 저장소가 추적하지 않는 캐시 폴더([`CACHE_DIR`])에 질의·받은 시각·`timestamp_osm_base`와 함께 저장하며,
//! 코스는 저장된 스냅샷만으로 다시 만든다(오프라인 재현).
//!
//! - 요청 범위: bbox는 대한민국 근사 사각형([`route_plan::KOREA_LAT`]·[`route_plan::KOREA_LON`]) 안이어야 하고
//!   한 변이 [`MAX_BBOX_SIDE_M`] 이하여야 한다. 응답은 [`MAX_RESPONSE_BYTES`] 이하여야 한다.
//! - 요청: 응답 시간 제한, 동시 요청 1개([`FetchJob`]), 429/504 응답 시 대기 후 재시도, 개인 정보가 없는 일반
//!   User-Agent([`USER_AGENT`])를 쓴다.
//! - 실패: 네트워크 실패·시간 초과·상한 초과·범위 밖 요청은 모두 오류 문자열로 돌려준다(앱을 멈추지 않는다).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::map::LatLon;
use super::osm_graph::parse_overpass_graph;
use super::route_plan;

/// 기본 Overpass API 주소.
pub const OVERPASS_ENDPOINT: &str = "https://overpass-api.de/api/interpreter";
/// 요청에 쓰는 일반 User-Agent. 개인 정보(이메일 등)를 넣지 않는다.
pub const USER_AGENT: &str = "self-driving-sim-dev/0.1";
/// 한 번에 받는 범위의 한 변 상한(m). PLAN 4 공통 계약의 개발상 임시값 2 × 2 km.
pub const MAX_BBOX_SIDE_M: f64 = 2000.0;
/// 응답 크기 상한(바이트). PLAN 4 공통 계약의 20 MB.
pub const MAX_RESPONSE_BYTES: u64 = 20 * 1024 * 1024;
/// 캐시 폴더(실행 폴더 기준). `.gitignore`의 `/cache/`로 저장소에서 뺀다.
pub const CACHE_DIR: &str = "cache/maps";
/// 캐시 폴더 안의 원본 응답 파일 이름.
pub const SNAPSHOT_FILE: &str = "graph.osm.json";
/// 캐시 폴더 안의 받은 기록 파일 이름.
pub const META_FILE: &str = "meta.toml";
/// 재시도 대기 시간 상한(s). 서버가 더 긴 `Retry-After`를 보내도 이만큼만 기다린다.
const MAX_RETRY_WAIT_S: u64 = 60;
/// 지구 평균 반지름(m). [`super::map::project`]와 같은 값.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// 받을 범위(deg). Overpass 순서대로 남·서·북·동.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    /// 남쪽 위도.
    pub south: f64,
    /// 서쪽 경도.
    pub west: f64,
    /// 북쪽 위도.
    pub north: f64,
    /// 동쪽 경도.
    pub east: f64,
}

impl Bbox {
    /// `남,서,북,동` 문자열을 읽고 [`Bbox::validate`]로 검사한다.
    pub fn parse(text: &str) -> Result<Self, String> {
        let values: Vec<f64> = text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("bbox `{text}` must be 4 numbers: south,west,north,east"))?;
        let [south, west, north, east] = values[..] else {
            return Err(format!(
                "bbox `{text}` must be 4 numbers: south,west,north,east"
            ));
        };
        let bbox = Self {
            south,
            west,
            north,
            east,
        };
        bbox.validate()?;
        Ok(bbox)
    }

    /// 유한값, 남 < 북·서 < 동, 네 모서리가 대한민국 범위 안, 한 변이 [`MAX_BBOX_SIDE_M`] 이하인지 검사한다.
    pub fn validate(&self) -> Result<(), String> {
        let values = [self.south, self.west, self.north, self.east];
        if !values.iter().all(|value| value.is_finite()) {
            return Err(format!("bbox {values:?} has a non-finite value"));
        }
        if !(self.south < self.north && self.west < self.east) {
            return Err(format!(
                "bbox {values:?} must have south < north and west < east"
            ));
        }
        for (lat, lon) in [(self.south, self.west), (self.north, self.east)] {
            if !route_plan::in_korea(LatLon { lat, lon }) {
                return Err(format!(
                    "bbox {values:?} is outside the South Korea area (lat {:?}, lon {:?})",
                    route_plan::KOREA_LAT,
                    route_plan::KOREA_LON
                ));
            }
        }
        let (east_west, north_south) = self.size_m();
        if east_west > MAX_BBOX_SIDE_M || north_south > MAX_BBOX_SIDE_M {
            return Err(format!(
                "bbox is {east_west:.0} x {north_south:.0} m (east-west by north-south); limit is {MAX_BBOX_SIDE_M} m per side"
            ));
        }
        Ok(())
    }

    /// 범위의 크기(동서 m, 남북 m). 동서 길이는 가운데 위도에서 잰다.
    pub fn size_m(&self) -> (f64, f64) {
        let middle = ((self.south + self.north) / 2.0).to_radians();
        (
            EARTH_RADIUS_M * (self.east - self.west).to_radians() * middle.cos(),
            EARTH_RADIUS_M * (self.north - self.south).to_radians(),
        )
    }

    /// Overpass 질의문(`way[highway](bbox); out body geom;`). `timeout_s`는 서버 쪽 시간 제한이다.
    pub fn query(&self, timeout_s: u64) -> String {
        format!(
            "[out:json][timeout:{timeout_s}];\nway[\"highway\"]({:.7},{:.7},{:.7},{:.7});\nout body geom;",
            self.south, self.west, self.north, self.east
        )
    }

    /// 캐시 폴더 이름(소수 7자리 좌표). 같은 범위는 같은 이름이다.
    pub fn cache_name(&self) -> String {
        format!(
            "bbox_{:.7}_{:.7}_{:.7}_{:.7}",
            self.south, self.west, self.north, self.east
        )
    }
}

/// 받기 설정.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchSettings {
    /// Overpass API 주소.
    pub endpoint: String,
    /// 요청 하나의 전체 시간 제한(연결·응답 포함).
    pub timeout: Duration,
    /// 응답 크기 상한(바이트).
    pub max_bytes: u64,
    /// 시도 횟수(첫 요청 포함). 429/504일 때만 다시 시도한다.
    pub attempts: u32,
    /// 서버가 `Retry-After`를 주지 않을 때의 기본 대기 시간. 시도마다 배수로 늘린다.
    pub retry_wait: Duration,
}

impl Default for FetchSettings {
    /// 기본값: 공개 Overpass 주소, 90초 제한, 20 MB, 3회 시도, 10초 대기(임시값).
    fn default() -> Self {
        Self {
            endpoint: OVERPASS_ENDPOINT.to_owned(),
            timeout: Duration::from_secs(90),
            max_bytes: MAX_RESPONSE_BYTES,
            attempts: 3,
            retry_wait: Duration::from_secs(10),
        }
    }
}

/// HTTP 응답 중 필요한 부분.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// 상태 코드.
    pub status: u16,
    /// `Retry-After` 헤더(초). 없거나 읽을 수 없으면 `None`.
    pub retry_after_s: Option<u64>,
    /// 응답 본문.
    pub body: Vec<u8>,
}

/// 질의를 보내는 수단. 실제 네트워크는 [`UreqTransport`], 시험은 가짜 구현을 쓴다.
pub trait Transport {
    /// `endpoint`로 `data=<질의>` 양식을 POST하고 응답을 돌려준다. 시간 초과·크기 초과·연결 실패는 `Err`다.
    fn post_query(
        &self,
        endpoint: &str,
        query: &str,
        timeout: Duration,
        max_bytes: u64,
    ) -> Result<Reply, String>;
}

/// `ureq`로 실제 HTTP 요청을 보낸다.
pub struct UreqTransport;

impl Transport for UreqTransport {
    /// 상태 코드를 오류로 바꾸지 않고 돌려받아 재시도 판단은 [`fetch_snapshot`]이 한다.
    fn post_query(
        &self,
        endpoint: &str,
        query: &str,
        timeout: Duration,
        max_bytes: u64,
    ) -> Result<Reply, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .post(endpoint)
            .header("User-Agent", USER_AGENT)
            .send_form([("data", query)])
            .map_err(|error| describe_http_error(&error))?;
        let status = response.status().as_u16();
        let retry_after_s = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        let body = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|error| describe_http_error(&error))?;
        Ok(Reply {
            status,
            retry_after_s,
            body,
        })
    }
}

/// `ureq` 오류를 사용자에게 보일 문장으로 바꾼다.
fn describe_http_error(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Timeout(_) => format!("map download timed out ({error})"),
        ureq::Error::BodyExceedsLimit(limit) => {
            format!("map response is larger than the {limit} byte limit")
        }
        _ => format!("map download failed: {error}"),
    }
}

/// 받은 스냅샷(원본 응답과 받은 기록).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// 받은 기록.
    pub meta: SnapshotMeta,
    /// 원본 응답 본문(수정하지 않음).
    pub body: Vec<u8>,
}

/// 받은 기록(`meta.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotMeta {
    /// 받은 범위.
    pub bbox: Bbox,
    /// 보낸 Overpass 질의문.
    pub query: String,
    /// 요청한 주소.
    pub endpoint: String,
    /// 보낸 User-Agent.
    pub user_agent: String,
    /// 받은 시각(UTC, RFC 3339).
    pub fetched_at: String,
    /// 응답의 `osm3s.timestamp_osm_base`.
    pub timestamp_osm_base: String,
    /// 원본 응답 크기(바이트).
    pub bytes: u64,
}

/// Overpass 응답 중 기록에 필요한 부분.
#[derive(Debug, Deserialize)]
struct ResponseHeader {
    /// 서버 정보.
    osm3s: Option<Osm3s>,
    /// 서버가 질의 실행 중 문제를 알릴 때 쓰는 문구(시간 초과·메모리 부족 등).
    remark: Option<String>,
}

/// `osm3s` 항목.
#[derive(Debug, Deserialize)]
struct Osm3s {
    /// 자료 기준 시각.
    timestamp_osm_base: Option<String>,
}

/// 응답 본문이 노드 ID를 보존한 그래프 자료인지 검사하고 `timestamp_osm_base`를 꺼낸다.
///
/// `remark`(서버 쪽 실행 오류)가 있으면 일부만 담긴 응답일 수 있으므로 거부한다.
pub fn check_response(body: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(body).map_err(|_| "map response is not UTF-8".to_owned())?;
    let header: ResponseHeader = serde_json::from_str(text)
        .map_err(|error| format!("map response is not Overpass JSON: {error}"))?;
    if let Some(remark) = header.remark {
        return Err(format!(
            "Overpass reported a problem (the response may be partial): {remark}"
        ));
    }
    parse_overpass_graph(text)?;
    header
        .osm3s
        .and_then(|osm3s| osm3s.timestamp_osm_base)
        .ok_or_else(|| "map response has no osm3s.timestamp_osm_base".to_owned())
}

/// bbox 범위를 받아 검사한 스냅샷을 만든다. 429/504면 `Retry-After`(없으면 기본 대기 × 시도 횟수)만큼
/// `sleep`으로 기다렸다가 다시 시도한다. 그 밖의 상태 코드·전송 오류는 바로 오류다.
pub fn fetch_snapshot(
    bbox: &Bbox,
    settings: &FetchSettings,
    transport: &dyn Transport,
    sleep: &dyn Fn(Duration),
) -> Result<Snapshot, String> {
    bbox.validate()?;
    let query = bbox.query(settings.timeout.as_secs().max(1));
    let attempts = settings.attempts.max(1);
    for attempt in 1..=attempts {
        let reply = transport.post_query(
            &settings.endpoint,
            &query,
            settings.timeout,
            settings.max_bytes,
        )?;
        match reply.status {
            200 => {
                if reply.body.len() as u64 > settings.max_bytes {
                    return Err(format!(
                        "map response is larger than the {} byte limit",
                        settings.max_bytes
                    ));
                }
                let timestamp_osm_base = check_response(&reply.body)?;
                return Ok(Snapshot {
                    meta: SnapshotMeta {
                        bbox: *bbox,
                        query,
                        endpoint: settings.endpoint.clone(),
                        user_agent: USER_AGENT.to_owned(),
                        fetched_at: utc_now(),
                        timestamp_osm_base,
                        bytes: reply.body.len() as u64,
                    },
                    body: reply.body,
                });
            }
            429 | 504 if attempt < attempts => {
                let wait = reply
                    .retry_after_s
                    .map(Duration::from_secs)
                    .unwrap_or(settings.retry_wait * attempt)
                    .min(Duration::from_secs(MAX_RETRY_WAIT_S));
                sleep(wait);
            }
            429 | 504 => {
                return Err(format!(
                    "Overpass is busy (HTTP {}) after {attempts} attempts; try again later",
                    reply.status
                ));
            }
            status => return Err(format!("Overpass answered HTTP {status}")),
        }
    }
    unreachable!("the loop returns on the last attempt")
}

/// 지금 시각을 UTC RFC 3339 문자열(초 단위)로 만든다.
fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    rfc3339(seconds)
}

/// 유닉스 시각(초)을 `YYYY-MM-DDThh:mm:ssZ`로 바꾼다(그레고리력, 1970년 이후).
fn rfc3339(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // 1970-01-01부터의 날 수 → 연월일 (H. Hinnant의 civil_from_days).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// 파일을 임시 이름으로 쓴 뒤 바꿔 달아, 중간에 실패해도 쓰다 만 파일이 남지 않게 한다.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("partial");
    let result = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    result.map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("cannot write {}: {error}", path.display())
    })
}

/// 스냅샷을 `cache_root/<bbox 이름>/`에 원본 응답과 받은 기록으로 저장하고 그 폴더를 돌려준다.
///
/// 원본을 먼저 쓰고 기록을 나중에 쓴다. 기록이 있는 폴더만 캐시로 인정하므로 중간 실패는 캐시로 보이지 않는다.
pub fn save_snapshot(cache_root: &Path, snapshot: &Snapshot) -> Result<PathBuf, String> {
    let dir = cache_root.join(snapshot.meta.bbox.cache_name());
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    let meta = toml::to_string(&snapshot.meta)
        .map_err(|error| format!("cannot write snapshot record: {error}"))?;
    let _ = std::fs::remove_file(dir.join(META_FILE));
    write_atomic(&dir.join(SNAPSHOT_FILE), &snapshot.body)?;
    write_atomic(&dir.join(META_FILE), meta.as_bytes())?;
    Ok(dir)
}

/// 캐시에 저장된 스냅샷.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    /// 캐시 폴더.
    pub dir: PathBuf,
    /// 받은 기록.
    pub meta: SnapshotMeta,
}

/// `cache_root`에서 bbox의 캐시를 찾는다. 없으면 `Ok(None)`, 있는데 기록·원본이 맞지 않으면 `Err`다.
pub fn load_cached(cache_root: &Path, bbox: &Bbox) -> Result<Option<Cached>, String> {
    let dir = cache_root.join(bbox.cache_name());
    let meta_path = dir.join(META_FILE);
    if !meta_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&meta_path)
        .map_err(|error| format!("cannot read {}: {error}", meta_path.display()))?;
    let meta: SnapshotMeta = toml::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", meta_path.display()))?;
    if meta.bbox != *bbox {
        return Err(format!(
            "{} was recorded for a different bbox",
            meta_path.display()
        ));
    }
    let body_path = dir.join(SNAPSHOT_FILE);
    let body = std::fs::read(&body_path)
        .map_err(|error| format!("cannot read {}: {error}", body_path.display()))?;
    if body.len() as u64 != meta.bytes {
        return Err(format!(
            "{} is {} bytes but the record says {}",
            body_path.display(),
            body.len(),
            meta.bytes
        ));
    }
    check_response(&body)?;
    Ok(Some(Cached { dir, meta }))
}

/// 스냅샷을 구하는 방법.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchPolicy {
    /// 네트워크를 쓰지 않고 캐시만 쓴다(오프라인).
    CacheOnly,
    /// 네트워크로 새로 받는다. 실패하면 있던 캐시를 경고와 함께 쓴다.
    Refresh,
}

/// 스냅샷을 구한 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct Obtained {
    /// 쓸 캐시.
    pub cached: Cached,
    /// 이번에 네트워크로 받았는지.
    pub downloaded: bool,
    /// 받기에 실패해 이전 캐시를 쓸 때의 경고.
    pub warning: Option<String>,
}

/// 정책에 따라 bbox 스냅샷을 구한다. 캐시도 없고 받기도 실패하면 오류다.
pub fn obtain_snapshot(
    cache_root: &Path,
    bbox: &Bbox,
    policy: FetchPolicy,
    settings: &FetchSettings,
    transport: &dyn Transport,
    sleep: &dyn Fn(Duration),
) -> Result<Obtained, String> {
    bbox.validate()?;
    let cached = load_cached(cache_root, bbox);
    if policy == FetchPolicy::CacheOnly {
        return match cached? {
            Some(cached) => Ok(Obtained {
                cached,
                downloaded: false,
                warning: None,
            }),
            None => Err(format!(
                "no cached map for bbox {} (offline: nothing downloaded)",
                bbox.cache_name()
            )),
        };
    }
    match fetch_snapshot(bbox, settings, transport, sleep)
        .and_then(|snapshot| save_snapshot(cache_root, &snapshot))
    {
        Ok(_) => Ok(Obtained {
            cached: load_cached(cache_root, bbox)?
                .ok_or("the saved map snapshot could not be read back")?,
            downloaded: true,
            warning: None,
        }),
        Err(error) => match cached {
            Ok(Some(cached)) => Ok(Obtained {
                cached,
                downloaded: false,
                warning: Some(format!("{error}; using the cached snapshot")),
            }),
            _ => Err(error),
        },
    }
}

/// 진행 중인 받기 작업이 있는지. 동시 요청을 1개로 제한한다.
static FETCH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// 작업 스레드가 끝날 때(패닉 포함) 진행 표시를 내린다.
struct InFlightGuard;

impl Drop for InFlightGuard {
    /// 진행 표시를 내린다.
    fn drop(&mut self) {
        FETCH_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

/// 백그라운드 받기 작업. 화면 갱신을 막지 않도록 별도 스레드에서 [`obtain_snapshot`]을 실행한다.
pub struct FetchJob {
    /// 결과 수신 통로.
    receiver: Receiver<Result<Obtained, String>>,
}

impl FetchJob {
    /// 실제 네트워크([`UreqTransport`])로 받기 작업을 시작한다. 이미 진행 중이면 오류다.
    pub fn start(
        cache_root: PathBuf,
        bbox: Bbox,
        policy: FetchPolicy,
        settings: FetchSettings,
    ) -> Result<Self, String> {
        Self::start_with(cache_root, bbox, policy, settings, Box::new(UreqTransport))
    }

    /// 지정한 전송 수단으로 받기 작업을 시작한다. 이미 진행 중이면 오류다.
    pub fn start_with(
        cache_root: PathBuf,
        bbox: Bbox,
        policy: FetchPolicy,
        settings: FetchSettings,
        transport: Box<dyn Transport + Send>,
    ) -> Result<Self, String> {
        if FETCH_IN_FLIGHT
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("a map download is already in progress".to_owned());
        }
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("map-fetch".to_owned())
            .spawn(move || {
                let _guard = InFlightGuard;
                let result = obtain_snapshot(
                    &cache_root,
                    &bbox,
                    policy,
                    &settings,
                    transport.as_ref(),
                    &std::thread::sleep,
                );
                let _ = sender.send(result);
            });
        if let Err(error) = spawned {
            FETCH_IN_FLIGHT.store(false, Ordering::SeqCst);
            return Err(format!("cannot start the map download: {error}"));
        }
        Ok(Self { receiver })
    }

    /// 결과가 나왔으면 돌려주고, 아직이면 `None`. 기다리지 않는다.
    pub fn try_result(&self) -> Option<Result<Obtained, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("the map download stopped unexpectedly".to_owned()))
            }
        }
    }

    /// 결과가 나올 때까지 기다린다(화면 없는 명령줄 받기용).
    pub fn wait(self) -> Result<Obtained, String> {
        self.receiver
            .recv()
            .unwrap_or_else(|_| Err("the map download stopped unexpectedly".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;

    use super::*;
    use crate::world::osm_graph::fixtures;
    use crate::world::route_plan::{RoadGraphConfig, plan_route};

    /// 여수 조사 범위(약 1.28 × 1.22 km, 대한민국 범위 안).
    const YEOSU: &str = "34.7480,127.7460,34.7590,127.7600";

    /// `osm3s` 머리말을 붙인 합성 그래프 응답.
    fn fixture_body() -> Vec<u8> {
        fixtures::overpass_json(&fixtures::three_way())
            .replacen(
                "{\"elements\"",
                "{\"osm3s\":{\"timestamp_osm_base\":\"2026-11-07T00:00:00Z\"},\"elements\"",
                1,
            )
            .into_bytes()
    }

    /// 저장소의 실제 노드 ID 스냅샷 원본.
    fn real_body() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/maps/yeosu-expo/graph-2026-11-06.osm.json"),
        )
        .unwrap()
    }

    /// 정해 둔 응답을 차례로 돌려주고 보낸 질의를 기록하는 가짜 전송 수단.
    struct Scripted {
        /// 남은 응답(앞에서부터).
        replies: RefCell<Vec<Result<Reply, String>>>,
        /// 받은 질의.
        queries: RefCell<Vec<String>>,
    }

    impl Scripted {
        /// 응답 목록으로 만든다.
        fn new(replies: Vec<Result<Reply, String>>) -> Self {
            Self {
                replies: RefCell::new(replies),
                queries: RefCell::new(Vec::new()),
            }
        }
    }

    impl Transport for Scripted {
        /// 다음 정해 둔 응답을 돌려준다.
        fn post_query(&self, _: &str, query: &str, _: Duration, _: u64) -> Result<Reply, String> {
            self.queries.borrow_mut().push(query.to_owned());
            self.replies.borrow_mut().remove(0)
        }
    }

    /// 백그라운드 작업 시험용: 스레드로 보낼 수 있는, 응답 하나만 돌려주는 전송 수단.
    struct Once(std::sync::Mutex<Option<Result<Reply, String>>>);

    impl Transport for Once {
        /// 정해 둔 응답을 한 번 돌려준다. 두 번째부터는 오류다.
        fn post_query(&self, _: &str, _: &str, _: Duration, _: u64) -> Result<Reply, String> {
            self.0
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Err("no more replies".to_owned()))
        }
    }

    /// 상태 코드와 본문으로 응답을 만든다.
    fn reply(status: u16, retry_after_s: Option<u64>, body: Vec<u8>) -> Result<Reply, String> {
        Ok(Reply {
            status,
            retry_after_s,
            body,
        })
    }

    /// 시험마다 비어 있는 캐시 폴더를 만든다.
    fn temp_cache(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sds-map-fetch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 요청 하나를 받아 기록을 보내고, `delay` 뒤 `response`를 쓰고 닫는 시험용 HTTP 서버.
    fn local_server(response: Vec<u8>, delay: Duration) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/api/interpreter", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                head.push_str(&line);
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let _ = sender.send(format!("{head}\r\n{}", String::from_utf8_lossy(&body)));
            std::thread::sleep(delay);
            let _ = stream.write_all(&response);
        });
        (address, receiver)
    }

    /// 상태 줄, 추가 헤더, 본문으로 HTTP/1.1 응답 바이트를 만든다.
    fn http_response(status: &str, extra: &str, body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    /// bbox 읽기·검사: 정상 범위, 형식 오류, 순서 오류, 대한민국 범위 밖, 2 km 초과, 유한하지 않은 값.
    #[test]
    fn bbox_is_parsed_and_validated() {
        let bbox = Bbox::parse(YEOSU).unwrap();
        let (east_west, north_south) = bbox.size_m();
        assert!((1200.0..1350.0).contains(&east_west), "{east_west}");
        assert!((1150.0..1300.0).contains(&north_south), "{north_south}");
        assert_eq!(
            bbox.query(90),
            "[out:json][timeout:90];\nway[\"highway\"](34.7480000,127.7460000,34.7590000,127.7600000);\nout body geom;"
        );
        assert_eq!(
            bbox.cache_name(),
            "bbox_34.7480000_127.7460000_34.7590000_127.7600000"
        );
        for (text, expected) in [
            ("34.748,127.746,34.759", "4 numbers"),
            ("a,b,c,d", "4 numbers"),
            ("34.759,127.746,34.748,127.760", "south < north"),
            ("35.0,140.0,35.01,140.01", "outside"),
            ("34.70,127.70,34.73,127.73", "limit"),
            ("NaN,127.7,34.7,127.8", "non-finite"),
        ] {
            let error = Bbox::parse(text).unwrap_err();
            assert!(error.contains(expected), "{text}: {error}");
        }
    }

    /// 받기: 정상 응답의 기록, 429/504 후 재시도(`Retry-After`·기본 대기·대기 상한), 계속 바쁘면 실패, 그 밖의
    /// 상태·전송 오류는 바로 실패, 노드 ID 없는 응답·`remark` 응답·JSON 아님 거부, 범위 밖이면 요청하지 않음.
    #[test]
    fn fetch_retries_busy_server_and_checks_responses() {
        let bbox = Bbox::parse(YEOSU).unwrap();
        let settings = FetchSettings::default();
        let waits = RefCell::new(Vec::new());
        let sleep = |wait: Duration| waits.borrow_mut().push(wait);
        let ok = Scripted::new(vec![
            reply(429, Some(2), Vec::new()),
            reply(504, None, Vec::new()),
            reply(200, None, fixture_body()),
        ]);
        let snapshot = fetch_snapshot(&bbox, &settings, &ok, &sleep).unwrap();
        assert_eq!(snapshot.body, fixture_body());
        assert_eq!(snapshot.meta.timestamp_osm_base, "2026-11-07T00:00:00Z");
        assert_eq!(snapshot.meta.user_agent, USER_AGENT);
        assert_eq!(snapshot.meta.query, bbox.query(90));
        assert_eq!(ok.queries.borrow().len(), 3);
        assert_eq!(
            *waits.borrow(),
            vec![Duration::from_secs(2), Duration::from_secs(20)]
        );
        let busy = Scripted::new(vec![
            reply(504, None, Vec::new()),
            reply(429, Some(3600), Vec::new()),
            reply(429, None, Vec::new()),
        ]);
        let error = fetch_snapshot(&bbox, &settings, &busy, &sleep).unwrap_err();
        assert!(error.contains("busy"), "{error}");
        assert_eq!(waits.borrow()[3], Duration::from_secs(MAX_RETRY_WAIT_S));
        let no_ids = String::from_utf8(fixture_body())
            .unwrap()
            .replace("\"nodes\"", "\"node_list\"")
            .into_bytes();
        let remark = br#"{"osm3s":{"timestamp_osm_base":"x"},"remark":"runtime error: timeout","elements":[]}"#;
        for (name, response, expected) in [
            ("status", reply(400, None, Vec::new()), "HTTP 400"),
            (
                "transport",
                Err("map download timed out".to_owned()),
                "timed out",
            ),
            ("no ids", reply(200, None, no_ids), "node IDs"),
            ("remark", reply(200, None, remark.to_vec()), "partial"),
            (
                "html",
                reply(200, None, b"<html>".to_vec()),
                "not Overpass JSON",
            ),
        ] {
            let transport = Scripted::new(vec![response]);
            let error = fetch_snapshot(&bbox, &settings, &transport, &sleep).unwrap_err();
            assert!(error.contains(expected), "{name}: {error}");
        }
        let outside = Bbox {
            south: 40.0,
            west: 127.0,
            north: 40.01,
            east: 127.01,
        };
        let never = Scripted::new(Vec::new());
        assert!(fetch_snapshot(&outside, &settings, &never, &sleep).is_err());
        assert!(never.queries.borrow().is_empty());
    }

    /// 실제 `ureq` 전송: 로컬 시험 서버로 양식 POST·User-Agent를 보내고, `Retry-After`를 읽으며,
    /// 크기 상한·시간 초과·연결 실패를 오류로 돌려준다(외부 네트워크 없음).
    #[test]
    fn ureq_transport_sends_form_and_enforces_limits() {
        let body = fixture_body();
        let (address, requests) = local_server(http_response("200 OK", "", &body), Duration::ZERO);
        let reply = UreqTransport
            .post_query(&address, "[out:json];", Duration::from_secs(10), 1 << 20)
            .unwrap();
        assert_eq!((reply.status, reply.body), (200, body.clone()));
        let request = requests.recv().unwrap();
        assert!(request.starts_with("POST /api/interpreter"), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains(&format!("user-agent: {USER_AGENT}")),
            "{request}"
        );
        assert!(request.ends_with("data=%5Bout%3Ajson%5D%3B"), "{request}");
        let (address, _) = local_server(http_response("200 OK", "", &body), Duration::ZERO);
        let error = UreqTransport
            .post_query(&address, "q", Duration::from_secs(10), 64)
            .unwrap_err();
        assert!(error.contains("limit"), "{error}");
        let (address, _) = local_server(http_response("200 OK", "", &body), Duration::from_secs(3));
        let started = std::time::Instant::now();
        let error = UreqTransport
            .post_query(&address, "q", Duration::from_millis(300), 1 << 20)
            .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        let (address, _) = local_server(
            http_response("429 Too Many Requests", "Retry-After: 7\r\n", b""),
            Duration::ZERO,
        );
        let reply = UreqTransport
            .post_query(&address, "q", Duration::from_secs(10), 1 << 20)
            .unwrap();
        assert_eq!((reply.status, reply.retry_after_s), (429, Some(7)));
        // 아무도 듣지 않는 포트(바로 닫은 리스너의 포트)로 연결하면 실패한다.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/x", closed.local_addr().unwrap());
        drop(closed);
        let error = UreqTransport
            .post_query(&address, "q", Duration::from_secs(5), 1 << 20)
            .unwrap_err();
        assert!(error.contains("failed"), "{error}");
    }

    /// 캐시: 원본을 바이트 그대로 저장하고, 오프라인에서는 캐시만 쓰며, 받기에 실패하면 이전 캐시를 경고와 함께
    /// 쓰고, 캐시가 없으면 오류다. 기록과 원본이 맞지 않는 캐시는 거부한다.
    #[test]
    fn cache_supports_offline_reuse() {
        let root = temp_cache("cache");
        let bbox = Bbox::parse(YEOSU).unwrap();
        let settings = FetchSettings::default();
        let sleep = |_: Duration| {};
        let offline = obtain_snapshot(
            &root,
            &bbox,
            FetchPolicy::CacheOnly,
            &settings,
            &Scripted::new(Vec::new()),
            &sleep,
        )
        .unwrap_err();
        assert!(offline.contains("offline"), "{offline}");
        let failing = Scripted::new(vec![Err("map download failed: no route".to_owned())]);
        let refresh = FetchPolicy::Refresh;
        assert!(obtain_snapshot(&root, &bbox, refresh, &settings, &failing, &sleep).is_err());
        let good = Scripted::new(vec![reply(200, None, real_body())]);
        let fresh = obtain_snapshot(&root, &bbox, refresh, &settings, &good, &sleep).unwrap();
        assert!(fresh.downloaded && fresh.warning.is_none());
        assert_eq!(fresh.cached.meta.timestamp_osm_base, "2026-11-06T07:02:21Z");
        assert_eq!(
            std::fs::read(fresh.cached.dir.join(SNAPSHOT_FILE)).unwrap(),
            real_body()
        );
        let failing = Scripted::new(vec![Err("map download timed out".to_owned())]);
        let stale = obtain_snapshot(&root, &bbox, refresh, &settings, &failing, &sleep).unwrap();
        assert!(!stale.downloaded);
        assert!(stale.warning.unwrap().contains("cached"));
        let never = Scripted::new(Vec::new());
        let cache_only = FetchPolicy::CacheOnly;
        let reused = obtain_snapshot(&root, &bbox, cache_only, &settings, &never, &sleep).unwrap();
        assert_eq!(reused.cached, fresh.cached);
        assert!(never.queries.borrow().is_empty());
        // 원본이 바뀐(잘린) 캐시는 거부한다.
        std::fs::write(fresh.cached.dir.join(SNAPSHOT_FILE), b"{}").unwrap();
        assert!(load_cached(&root, &bbox).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 오프라인 재현: 캐시에 저장한 실제 스냅샷만으로 경로를 계획하고, 같은 결과가 나오는지 확인한다.
    #[test]
    fn cached_snapshot_reproduces_route_offline() {
        let root = temp_cache("reproduce");
        let bbox = Bbox::parse(YEOSU).unwrap();
        let transport = Scripted::new(vec![reply(200, None, real_body())]);
        let obtained = obtain_snapshot(
            &root,
            &bbox,
            FetchPolicy::Refresh,
            &FetchSettings::default(),
            &transport,
            &|_| {},
        )
        .unwrap();
        let json = std::fs::read_to_string(obtained.cached.dir.join(SNAPSHOT_FILE)).unwrap();
        let ways = parse_overpass_graph(&json).unwrap();
        let points = [[34.75264, 127.74712], [34.74972, 127.74816]];
        let config = RoadGraphConfig::default();
        let first = plan_route(&ways, SNAPSHOT_FILE, &points, &config).unwrap();
        let again = plan_route(&ways, SNAPSHOT_FILE, &points, &config).unwrap();
        assert_eq!(first, again);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 백그라운드 작업: 결과가 통로로 오고, 진행 중에는 두 번째 작업을 거부하며, 끝나면 다시 시작할 수 있다.
    #[test]
    fn fetch_job_runs_in_background_one_at_a_time() {
        /// 신호를 받을 때까지 기다렸다가 실제 스냅샷을 돌려주는 전송 수단.
        struct Gated(std::sync::Mutex<Receiver<()>>);
        impl Transport for Gated {
            /// 신호가 올 때까지 기다린 뒤 응답한다.
            fn post_query(&self, _: &str, _: &str, _: Duration, _: u64) -> Result<Reply, String> {
                let _ = self.0.lock().unwrap().recv();
                reply(200, None, real_body())
            }
        }
        let root = temp_cache("job");
        let bbox = Bbox::parse(YEOSU).unwrap();
        let (open, gate) = mpsc::channel();
        let job = FetchJob::start_with(
            root.clone(),
            bbox,
            FetchPolicy::Refresh,
            FetchSettings::default(),
            Box::new(Gated(std::sync::Mutex::new(gate))),
        )
        .unwrap();
        assert!(job.try_result().is_none());
        let second = FetchJob::start_with(
            root.clone(),
            bbox,
            FetchPolicy::CacheOnly,
            FetchSettings::default(),
            Box::new(Once(std::sync::Mutex::new(None))),
        );
        assert!(second.is_err_and(|error| error.contains("already")));
        open.send(()).unwrap();
        assert!(job.wait().unwrap().downloaded);
        // 진행 표시는 작업 스레드가 끝날 때 내려가므로 잠시 기다린다.
        let mut again = None;
        for _ in 0..200 {
            match FetchJob::start_with(
                root.clone(),
                bbox,
                FetchPolicy::CacheOnly,
                FetchSettings::default(),
                Box::new(Once(std::sync::Mutex::new(None))),
            ) {
                Ok(job) => {
                    again = Some(job);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let reused = again.unwrap().wait().unwrap();
        assert!(!reused.downloaded);
        assert!(reused.cached.dir.ends_with(bbox.cache_name()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 받은 시각 문자열: 유닉스 시각을 UTC RFC 3339로 바꾼다.
    #[test]
    fn rfc3339_formats_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_794_009_656), "2026-11-07T00:00:56Z");
    }
}
