mod app;

use std::path::PathBuf;

use self_driving_sim::picker::{PickerSource, parse_points};
use self_driving_sim::world::CourseSelection;
use self_driving_sim::world::map_fetch::{self, Bbox, FetchJob, FetchPolicy, FetchSettings};

/// 명령줄 인자를 읽어 앱을 실행한다.
///
/// - `--smoke-test`: 약 5초 후 자동 종료.
/// - `--capture-views <폴더>`: 네 방향 시점과 외부 디버그 시점을 PNG로 저장한 뒤 종료.
/// - `--window <폭>x<높이>`: 창 크기(기본 1280x720). 기준 사진 비교 캡처에 쓴다.
/// - `--course <training|calibration|map:<선택 파일>>`: 주행 코스(기본 training).
///   모르는 이름이면 경고 후 기본 코스. 지도 코스를 읽지 못해도 경고 후 기본 코스로 실행한다.
/// - `--pick-route <yeosu|latlon:<위도>,<경도>|<스냅샷 파일>>`: 경로 선택 화면으로 시작한다(Phase 4-5).
///   "주행 시작"으로 경로 파일을 `cache/routes/`에 저장하고 그 코스로 주행한다. `latlon:`은 주변 1.5 km를 받는다.
///   값이 잘못되면 오류 종료한다. `--pick-points "<위도>,<경도>;..."`로 점을 미리 찍어 둘 수 있고,
///   `--auto-start`를 주면 지도를 불러오는 대로 주행을 시작한다(화면 전환 자동 검증용).
/// - `--fetch-map <남,서,북,동>`: 창 없이 Overpass에서 지도 그래프를 받아 `cache/maps/`에 저장하고 끝낸다
///   (Phase 4-4 개발용). `--offline`을 함께 주면 네트워크를 쓰지 않고 캐시만 확인한다.
fn main() -> bevy::app::AppExit {
    let args: Vec<String> = std::env::args().collect();
    let value_after = |flag: &str| {
        args.iter()
            .position(|argument| argument == flag)
            .and_then(|index| args.get(index + 1))
    };
    if args.iter().any(|argument| argument == "--fetch-map") {
        let offline = args.iter().any(|argument| argument == "--offline");
        return match value_after("--fetch-map") {
            Some(bbox) => fetch_map_command(bbox, offline),
            None => {
                eprintln!("--fetch-map needs a bbox: south,west,north,east");
                bevy::app::AppExit::error()
            }
        };
    }
    let window = value_after("--window").and_then(|size| {
        let parsed = app::parse_window_size(size);
        if parsed.is_none() {
            eprintln!("ignoring invalid --window value `{size}` (expected e.g. 1498x700)");
        }
        parsed
    });
    let course = value_after("--course")
        .map(|name| {
            CourseSelection::parse(name).unwrap_or_else(|| {
                eprintln!(
                    "unknown --course `{name}` (expected training, calibration or map:<file>); using training"
                );
                CourseSelection::default()
            })
        })
        .unwrap_or_default();
    let pick_route = match value_after("--pick-route").map(|text| PickerSource::parse(text)) {
        Some(Ok(source)) => Some(source),
        Some(Err(error)) => {
            eprintln!("invalid --pick-route: {error}");
            return bevy::app::AppExit::error();
        }
        None if args.iter().any(|argument| argument == "--pick-route") => {
            eprintln!("--pick-route needs yeosu, latlon:<lat>,<lon> or a snapshot path");
            return bevy::app::AppExit::error();
        }
        None => None,
    };
    let pick_points = match value_after("--pick-points").map(|text| parse_points(text)) {
        Some(Ok(points)) => points,
        Some(Err(error)) => {
            eprintln!("invalid --pick-points: {error}");
            return bevy::app::AppExit::error();
        }
        None => Vec::new(),
    };
    app::run(app::RunOptions {
        smoke_test: args.iter().any(|argument| argument == "--smoke-test"),
        capture_dir: value_after("--capture-views").map(PathBuf::from),
        window,
        course,
        pick_route,
        pick_points,
        auto_start: args.iter().any(|argument| argument == "--auto-start"),
    })
}

/// `--fetch-map` 명령: bbox 스냅샷을 백그라운드 작업으로 구해(받기 또는 캐시) 결과를 출력한다.
///
/// 성공하면 캐시 폴더와 기록을 표준 출력에, 실패하면 이유를 표준 오류에 쓰고 오류 종료 코드를 돌려준다.
fn fetch_map_command(bbox_text: &str, offline: bool) -> bevy::app::AppExit {
    let result = Bbox::parse(bbox_text).and_then(|bbox| {
        let policy = if offline {
            FetchPolicy::CacheOnly
        } else {
            FetchPolicy::Refresh
        };
        FetchJob::start(
            PathBuf::from(map_fetch::CACHE_DIR),
            bbox,
            policy,
            FetchSettings::default(),
        )?
        .wait()
    });
    match result {
        Ok(obtained) => {
            if let Some(warning) = &obtained.warning {
                eprintln!("warning: {warning}");
            }
            let meta = &obtained.cached.meta;
            println!(
                "{} map snapshot: {}",
                if obtained.downloaded {
                    "downloaded"
                } else {
                    "cached"
                },
                obtained.cached.dir.display()
            );
            println!(
                "fetched_at {} / timestamp_osm_base {} / {} bytes",
                meta.fetched_at, meta.timestamp_osm_base, meta.bytes
            );
            bevy::app::AppExit::Success
        }
        Err(error) => {
            eprintln!("map fetch failed: {error}");
            bevy::app::AppExit::error()
        }
    }
}
