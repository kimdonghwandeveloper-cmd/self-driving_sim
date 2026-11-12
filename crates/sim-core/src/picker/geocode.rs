//! 전국 장소 검색(PLAN 4-10): OSM 공식 검색 서버 Nominatim에 지명·장소 이름을 묻는다.
//!
//! 이용 규칙(<https://operations.osmfoundation.org/policies/nominatim/>)을 지킨다.
//!
//! - 사용자가 Enter·검색 버튼을 누를 때만 묻는다(입력 중 자동완성 금지). 화면 쪽이 지킨다.
//! - 요청 간격은 [`MIN_REQUEST_INTERVAL`] 이상이다(초당 1회 이하). 프로그램 전체가 [`NOMINATIM_LIMIT`] 하나를 쓴다.
//! - 앱 이름 User-Agent([`map_fetch::USER_AGENT`])를 보낸다.
//! - 같은 검색어는 저장한 결과([`GEOCODE_CACHE_DIR`])를 다시 쓴다. 다시 묻고 싶으면 그 폴더를 지운다.
//! - 출처 표기는 지도 화면의 "© OpenStreetMap contributors"가 한다.
//!
//! 결과는 대한민국 근사 경계([`region::KOREA_OUTLINE`]) 안의 것만 남긴다.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::region::KOREA_OUTLINE;
use crate::world::map::LatLon;
use crate::world::map_fetch::{self, Reply, USER_AGENT};

/// Nominatim 검색 주소.
pub const NOMINATIM_ENDPOINT: &str = "https://nominatim.openstreetmap.org/search";
/// 검색 결과 저장 폴더(저장소 루트 기준, 저장소가 추적하지 않음).
pub const GEOCODE_CACHE_DIR: &str = "cache/geocode";
/// 요청 사이 최소 간격. 이용 규칙의 초당 1회에 여유를 둔 값(임시값).
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(1100);
/// 한 번에 받을 결과 수 상한(임시값).
pub const MAX_RESULTS: usize = 10;
/// 검색어 길이 상한(글자).
pub const MAX_QUERY_CHARS: usize = 200;
/// 응답 크기 상한(바이트).
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
/// 응답 시간 제한.
const TIMEOUT: Duration = Duration::from_secs(15);

/// 검색 결과 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct GeocodeHit {
    /// 이름(`name`). 없으면 주소 첫 부분.
    pub name: String,
    /// 전체 주소(`display_name`).
    pub address: String,
    /// 종류(`category=type`, 예: `railway=station`).
    pub kind: String,
    /// 위치.
    pub at: LatLon,
}

/// Nominatim `jsonv2` 응답 항목 중 쓰는 부분.
#[derive(Debug, Deserialize)]
struct RawHit {
    lat: String,
    lon: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    category: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// 저장한 검색 결과(검색어와 받은 원본 응답).
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    query: String,
    fetched_at: String,
    body: String,
}

/// 검색어를 보내는 수단. 실제 네트워크는 [`UreqGet`], 시험은 가짜 구현을 쓴다.
pub trait GeocodeTransport {
    /// `endpoint`에 `params`를 질의 문자열로 붙여 GET하고 응답을 돌려준다. 연결 실패·시간 초과는 `Err`다.
    fn get(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        timeout: Duration,
        max_bytes: u64,
    ) -> Result<Reply, String>;
}

/// `ureq`로 실제 GET 요청을 보낸다.
pub struct UreqGet;

impl GeocodeTransport for UreqGet {
    /// 앱 이름 User-Agent와 한국어 우선 `Accept-Language`를 붙여 보낸다. 상태 코드는 오류로 바꾸지 않는다.
    fn get(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        timeout: Duration,
        max_bytes: u64,
    ) -> Result<Reply, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent
            .get(endpoint)
            .header("User-Agent", USER_AGENT)
            .header("Accept-Language", "ko,en");
        for (key, value) in params {
            request = request.query(*key, *value);
        }
        let mut response = request
            .call()
            .map_err(|error| format!("place search failed: {error}"))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|error| format!("place search failed: {error}"))?;
        Ok(Reply {
            status,
            retry_after_s: None,
            body,
        })
    }
}

/// 요청 간격 지키기. 마지막 요청 시각을 기억한다.
pub struct RateLimiter {
    last: Mutex<Option<Instant>>,
}

/// 프로그램 전체가 함께 쓰는 Nominatim 요청 간격.
pub static NOMINATIM_LIMIT: RateLimiter = RateLimiter::new();

impl RateLimiter {
    /// 아직 요청한 적이 없는 간격 지키기를 만든다.
    pub const fn new() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }

    /// 마지막 요청에서 `interval`이 지날 때까지 `sleep`으로 기다린 뒤 지금을 마지막 요청 시각으로 적는다.
    pub fn wait_turn(&self, interval: Duration, sleep: &dyn Fn(Duration)) {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < interval {
                sleep(interval - elapsed);
            }
        }
        *last = Some(Instant::now());
    }
}

impl Default for RateLimiter {
    /// [`RateLimiter::new`]와 같다.
    fn default() -> Self {
        Self::new()
    }
}

/// 검색어 앞뒤 공백을 지우고 가운데 공백을 하나로 줄인다. 비었거나 너무 길면 오류다.
pub fn normalize_query(text: &str) -> Result<String, String> {
    let query = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.is_empty() {
        return Err("empty search text".to_owned());
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(format!(
            "search text is longer than {MAX_QUERY_CHARS} characters"
        ));
    }
    Ok(query)
}

/// Nominatim에 보낼 질의 인자(대한민국만, `jsonv2`, 결과 상한).
pub fn query_params(query: &str) -> Vec<(&'static str, String)> {
    vec![
        ("q", query.to_owned()),
        ("format", "jsonv2".to_owned()),
        ("countrycodes", "kr".to_owned()),
        ("limit", MAX_RESULTS.to_string()),
    ]
}

/// 위경도가 대한민국 근사 경계 안인지 짝홀 규칙으로 판정한다.
pub fn in_korea(at: LatLon) -> bool {
    let mut inside = false;
    for (index, a) in KOREA_OUTLINE.iter().enumerate() {
        let b = KOREA_OUTLINE[(index + 1) % KOREA_OUTLINE.len()];
        if (a[0] > at.lat) != (b[0] > at.lat) {
            let lon = a[1] + (at.lat - a[0]) / (b[0] - a[0]) * (b[1] - a[1]);
            if at.lon < lon {
                inside = !inside;
            }
        }
    }
    inside
}

/// 응답 본문(`jsonv2` 배열)을 읽어 대한민국 근사 경계 안의 결과만 순서대로 돌려준다.
pub fn parse_results(body: &str) -> Result<Vec<GeocodeHit>, String> {
    let raw: Vec<RawHit> = serde_json::from_str(body)
        .map_err(|error| format!("cannot read the place search reply: {error}"))?;
    let mut hits = Vec::new();
    for item in raw {
        let (Ok(lat), Ok(lon)) = (item.lat.parse::<f64>(), item.lon.parse::<f64>()) else {
            continue;
        };
        let at = LatLon { lat, lon };
        if !(lat.is_finite() && lon.is_finite() && in_korea(at)) {
            continue;
        }
        let name = if item.name.trim().is_empty() {
            item.display_name
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        } else {
            item.name.trim().to_owned()
        };
        let kind = if item.category.is_empty() {
            item.kind
        } else {
            format!("{}={}", item.category, item.kind)
        };
        hits.push(GeocodeHit {
            name,
            address: item.display_name,
            kind,
            at,
        });
    }
    Ok(hits)
}

/// 검색어의 저장 파일 경로(검색어 FNV-1a 해시 이름). 같은 이름의 다른 검색어는 파일 안 검색어로 가린다.
pub fn cache_path(cache_root: &Path, query: &str) -> PathBuf {
    let hash = query.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    cache_root.join(format!("{hash:016x}.json"))
}

/// 저장한 결과가 있으면 읽는다. 없거나 다른 검색어의 파일이면 `None`.
fn load_cached(cache_root: &Path, query: &str) -> Option<Vec<GeocodeHit>> {
    let text = std::fs::read_to_string(cache_path(cache_root, query)).ok()?;
    let entry: CacheEntry = serde_json::from_str(&text).ok()?;
    (entry.query == query)
        .then(|| parse_results(&entry.body).ok())
        .flatten()
}

/// 검색어로 장소를 찾는다. 저장한 결과가 있으면 그것을 쓰고, 없으면 `network`일 때만 간격을 지켜 서버에 묻고
/// 원본 응답을 저장한다. 결과가 0개여도 성공이다.
pub fn geocode(
    cache_root: &Path,
    text: &str,
    network: bool,
    transport: &dyn GeocodeTransport,
    limiter: &RateLimiter,
    sleep: &dyn Fn(Duration),
) -> Result<Vec<GeocodeHit>, String> {
    let query = normalize_query(text)?;
    if let Some(hits) = load_cached(cache_root, &query) {
        return Ok(hits);
    }
    if !network {
        return Err("this search is not saved and the network is off".to_owned());
    }
    limiter.wait_turn(MIN_REQUEST_INTERVAL, sleep);
    let params = query_params(&query);
    let pairs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let reply = transport.get(NOMINATIM_ENDPOINT, &pairs, TIMEOUT, MAX_RESPONSE_BYTES)?;
    if reply.status != 200 {
        return Err(format!("the place search server answered {}", reply.status));
    }
    let body = String::from_utf8(reply.body)
        .map_err(|_| "the place search reply is not UTF-8".to_owned())?;
    let hits = parse_results(&body)?;
    let entry = CacheEntry {
        query: query.clone(),
        fetched_at: map_fetch::utc_now(),
        body,
    };
    std::fs::create_dir_all(cache_root)
        .map_err(|error| format!("cannot create {}: {error}", cache_root.display()))?;
    let json = serde_json::to_string(&entry).map_err(|error| error.to_string())?;
    map_fetch::write_atomic(&cache_path(cache_root, &query), json.as_bytes())?;
    Ok(hits)
}

/// 백그라운드 검색 작업. 화면 갱신을 막지 않도록 별도 스레드에서 [`geocode`]를 실행한다.
pub struct GeocodeJob {
    /// 결과 수신 통로. 여러 스레드에서 공유하는 노드에 넣을 수 있도록 잠금으로 감싼다.
    receiver: Mutex<Receiver<Result<Vec<GeocodeHit>, String>>>,
}

impl GeocodeJob {
    /// 공용 요청 간격([`NOMINATIM_LIMIT`])으로 검색 작업을 시작한다.
    pub fn start(
        cache_root: PathBuf,
        text: String,
        network: bool,
        transport: Box<dyn GeocodeTransport + Send>,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("place-search".to_owned())
            .spawn(move || {
                let result = geocode(
                    &cache_root,
                    &text,
                    network,
                    transport.as_ref(),
                    &NOMINATIM_LIMIT,
                    &std::thread::sleep,
                );
                let _ = sender.send(result);
            })
            .map_err(|error| format!("cannot start the place search: {error}"))?;
        Ok(Self {
            receiver: Mutex::new(receiver),
        })
    }

    /// 결과가 나왔으면 돌려주고, 아직이면 `None`. 기다리지 않는다.
    pub fn try_result(&self) -> Option<Result<Vec<GeocodeHit>, String>> {
        let receiver = self
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("the place search stopped unexpectedly".to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// 서울역(한국), 평양역(북한), 대마도(일본) 세 결과가 든 가짜 응답.
    const REPLY: &str = r#"[
        {"lat":"37.5547","lon":"126.9707","name":"서울역","display_name":"서울역, 한강대로, 중구, 서울","category":"railway","type":"station"},
        {"lat":"39.0461","lon":"125.7495","name":"평양역","display_name":"평양역, 평양","category":"railway","type":"station"},
        {"lat":"34.2","lon":"129.29","name":"","display_name":"対馬市, 長崎県, 日本","category":"boundary","type":"administrative"},
        {"lat":"35.1151","lon":"129.0414","name":"","display_name":"부산역, 중앙대로, 동구, 부산","category":"railway","type":"station"}
    ]"#;

    /// 요청을 기록하고 정해 둔 응답을 돌려주는 가짜 전송.
    struct Fake {
        status: u16,
        calls: RefCell<Vec<Vec<(String, String)>>>,
    }

    impl GeocodeTransport for Fake {
        /// 받은 인자를 기록하고 [`REPLY`]를 돌려준다.
        fn get(
            &self,
            endpoint: &str,
            params: &[(&str, &str)],
            _: Duration,
            _: u64,
        ) -> Result<Reply, String> {
            assert_eq!(endpoint, NOMINATIM_ENDPOINT);
            self.calls.borrow_mut().push(
                params
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            );
            Ok(Reply {
                status: self.status,
                retry_after_s: None,
                body: REPLY.as_bytes().to_vec(),
            })
        }
    }

    /// 시험마다 비어 있는 임시 저장 폴더.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sim-geocode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    /// 결과 읽기: 한국 밖(평양·대마도)은 빼고, 이름이 없으면 주소 첫 부분을 이름으로 쓴다.
    #[test]
    fn results_outside_korea_are_dropped() {
        let hits = parse_results(REPLY).unwrap();
        let names: Vec<_> = hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["서울역", "부산역"]);
        assert_eq!(hits[0].kind, "railway=station");
        assert!(hits[1].address.starts_with("부산역, "));
        assert!(parse_results("{}").is_err());
    }

    /// 검색어 정리: 공백을 줄이고, 빈 검색어·너무 긴 검색어는 거부한다.
    #[test]
    fn queries_are_normalized() {
        assert_eq!(
            normalize_query("  여수   엑스포역 ").unwrap(),
            "여수 엑스포역"
        );
        assert!(normalize_query("   ").is_err());
        assert!(normalize_query(&"가".repeat(MAX_QUERY_CHARS + 1)).is_err());
    }

    /// 처음에는 대한민국만·`jsonv2`로 묻고 저장하며, 같은 검색어는 서버에 다시 묻지 않는다. 오프라인이면 저장분만 쓴다.
    #[test]
    fn searches_are_saved_and_reused() {
        let root = temp_root("reuse");
        let fake = Fake {
            status: 200,
            calls: RefCell::new(Vec::new()),
        };
        let limiter = RateLimiter::new();
        let sleep = |_: Duration| {};
        let first = geocode(&root, " 서울역 ", true, &fake, &limiter, &sleep).unwrap();
        assert_eq!(first.len(), 2);
        let calls = fake.calls.borrow().clone();
        assert_eq!(calls.len(), 1);
        for expected in [
            ("q", "서울역"),
            ("format", "jsonv2"),
            ("countrycodes", "kr"),
        ] {
            assert!(
                calls[0].contains(&(expected.0.to_owned(), expected.1.to_owned())),
                "{expected:?}"
            );
        }
        let again = geocode(&root, "서울역", true, &fake, &limiter, &sleep).unwrap();
        assert_eq!(again, first);
        assert_eq!(
            fake.calls.borrow().len(),
            1,
            "saved search must not ask again"
        );
        let offline = geocode(&root, "서울역", false, &fake, &limiter, &sleep).unwrap();
        assert_eq!(offline, first);
        let missing = geocode(&root, "부산역", false, &fake, &limiter, &sleep).unwrap_err();
        assert!(missing.contains("network is off"), "{missing}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 서버가 200이 아닌 상태로 답하면 실패하고 저장하지 않는다.
    #[test]
    fn server_errors_are_not_saved() {
        let root = temp_root("error");
        let fake = Fake {
            status: 429,
            calls: RefCell::new(Vec::new()),
        };
        let error =
            geocode(&root, "서울역", true, &fake, &RateLimiter::new(), &|_| {}).unwrap_err();
        assert!(error.contains("429"), "{error}");
        assert!(!cache_path(&root, "서울역").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 요청 간격: 두 번째 요청은 최소 간격에서 지난 시간을 뺀 만큼 기다린다.
    #[test]
    fn requests_keep_the_minimum_interval() {
        let limiter = RateLimiter::new();
        let waited = Cell::new(Duration::ZERO);
        let sleep = |d: Duration| waited.set(waited.get() + d);
        limiter.wait_turn(MIN_REQUEST_INTERVAL, &sleep);
        assert_eq!(waited.get(), Duration::ZERO, "first request does not wait");
        limiter.wait_turn(MIN_REQUEST_INTERVAL, &sleep);
        assert!(waited.get() > MIN_REQUEST_INTERVAL - Duration::from_millis(200));
        assert!(waited.get() <= MIN_REQUEST_INTERVAL);
    }
}
