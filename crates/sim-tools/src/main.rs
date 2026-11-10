//! 창 없는 개발 명령(GD-5에서 Bevy 실행 파일로부터 옮김). 네트워크는 `--fetch-map`만 쓴다.
//!
//! - `--fetch-map <남,서,북,동>`: Overpass에서 지도 그래프를 받아 `cache/maps/`에 저장하고 끝낸다
//!   (Phase 4-4 개발용). `--offline`을 함께 주면 네트워크를 쓰지 않고 캐시만 확인한다.
//! - `--extract-pbf <PBF 파일> --bbox <남,서,북,동> [--margin <m>]`: 로컬 Geofabrik PBF에서 지역 도로를
//!   추출해 `cache/extracts/`에 새 세대로 저장하고, 단계별 시간·최대 메모리를 출력한다(Phase 4-5G, 네트워크 없음).
//! - `--audit-extract <세대 폴더> --pbf <PBF 파일>`: 세대 폴더를 원본 PBF와 보존 규칙으로 전수 대조한다.
//! - `--compare-overpass <Overpass 스냅샷> --extract <세대 폴더> --pbf <PBF 파일> [--pick-points ...]`: 날짜가 다른
//!   기존 Overpass 자료와의 차이를 분류해 출력한다(설명되지 않은 차이는 데이터 동등성 미확인).
//!
//! 상대 경로(`cache/`, `config/`)는 실행한 폴더 기준이므로 저장소 최상위에서 실행한다.

use std::path::PathBuf;
use std::process::ExitCode;

use sim_core::world::map_fetch::{self, Bbox, FetchJob, FetchPolicy, FetchSettings};
use sim_core::world::{pbf_audit, pbf_compare, pbf_extract, route_plan};

/// 명령줄 인자를 읽어 개발 명령 하나를 실행한다. 명령이 없거나 인자가 모자라면 사용법을 출력하고 오류 종료한다.
fn main() -> ExitCode {
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
                ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|argument| argument == "--extract-pbf") {
        return match (value_after("--extract-pbf"), value_after("--bbox")) {
            (Some(pbf), Some(bbox)) => extract_pbf_command(pbf, bbox, value_after("--margin")),
            _ => {
                eprintln!("--extract-pbf needs a PBF file and --bbox south,west,north,east");
                ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|argument| argument == "--audit-extract") {
        return match (value_after("--audit-extract"), value_after("--pbf")) {
            (Some(dir), Some(pbf)) => audit_extract_command(dir, pbf),
            _ => {
                eprintln!("--audit-extract needs a generation folder and --pbf <file>");
                ExitCode::FAILURE
            }
        };
    }
    if args.iter().any(|argument| argument == "--compare-overpass") {
        return match (
            value_after("--compare-overpass"),
            value_after("--extract"),
            value_after("--pbf"),
        ) {
            (Some(old), Some(dir), Some(pbf)) => {
                compare_overpass_command(old, dir, pbf, value_after("--pick-points"))
            }
            _ => {
                eprintln!(
                    "--compare-overpass needs <overpass graph> --extract <folder> --pbf <file>"
                );
                ExitCode::FAILURE
            }
        };
    }
    eprintln!("{USAGE}");
    ExitCode::FAILURE
}

/// 명령을 주지 않았을 때 출력하는 사용법.
const USAGE: &str = "usage: sim-tools --fetch-map <south,west,north,east> [--offline]
       sim-tools --extract-pbf <pbf> --bbox <south,west,north,east> [--margin <m>]
       sim-tools --audit-extract <generation folder> --pbf <pbf>
       sim-tools --compare-overpass <overpass graph> --extract <folder> --pbf <pbf> [--pick-points <lat,lon;...>]";

/// `위도,경도;위도,경도;...` 형식의 점 목록을 읽는다.
pub fn parse_points(text: &str) -> Result<Vec<[f64; 2]>, String> {
    text.split(';')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            let values: Vec<f64> = part
                .split(',')
                .map(|value| value.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`{part}` must be <lat>,<lon>"))?;
            match values[..] {
                [lat, lon] if lat.is_finite() && lon.is_finite() => Ok([lat, lon]),
                _ => Err(format!("`{part}` must be <lat>,<lon>")),
            }
        })
        .collect()
}

/// `--fetch-map` 명령: bbox 스냅샷을 백그라운드 작업으로 구해(받기 또는 캐시) 결과를 출력한다.
///
/// 성공하면 캐시 폴더와 기록을 표준 출력에, 실패하면 이유를 표준 오류에 쓰고 오류 종료 코드를 돌려준다.
fn fetch_map_command(bbox_text: &str, offline: bool) -> ExitCode {
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
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("map fetch failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 도로 그래프 설정의 허용 도로 목록을 읽는다. 설정을 읽지 못하면 기본값을 쓰고 알린다.
fn allowed_highways() -> Vec<String> {
    std::fs::read_to_string(route_plan::ROAD_GRAPH_CONFIG_PATH)
        .map_err(|error| error.to_string())
        .and_then(|text| route_plan::parse_road_graph_config(&text))
        .map(|config| config.allowed_highways)
        .unwrap_or_else(|error| {
            eprintln!("warning: road graph config unavailable ({error}); using defaults");
            route_plan::RoadGraphConfig::default().allowed_highways
        })
}

/// `--extract-pbf` 명령: 로컬 PBF에서 지역을 추출해 세대 폴더와 측정값을 출력한다.
fn extract_pbf_command(pbf: &str, bbox_text: &str, margin: Option<&String>) -> ExitCode {
    let margin = match margin.map(|text| text.parse::<f64>()) {
        None => pbf_extract::DEFAULT_MARGIN_M,
        Some(Ok(value)) => value,
        Some(Err(_)) => {
            eprintln!("--margin must be a number of metres");
            return ExitCode::FAILURE;
        }
    };
    let result = Bbox::parse(bbox_text).and_then(|bbox| {
        pbf_extract::extract_pbf_file(
            std::path::Path::new(pbf),
            &bbox,
            margin,
            &allowed_highways(),
            std::path::Path::new(pbf_extract::EXTRACT_DIR),
        )
    });
    match result {
        Ok(report) => {
            println!("extract generation: {}", report.dir.display());
            println!(
                "national allowed roads: {} ways, {} nodes ({} without coordinates)",
                report.national_ways, report.national_nodes, report.missing_coords
            );
            println!(
                "region: {} ways, {} nodes checked, {} incomplete boundary node(s), margin {margin} m",
                report.ways, report.checked_nodes, report.incomplete_nodes
            );
            for (step, seconds) in &report.timings {
                println!("time {step}: {seconds:.2} s");
            }
            match report.peak_memory_bytes {
                Some(bytes) => println!("peak working set: {:.1} MiB", bytes as f64 / 1_048_576.0),
                None => println!("peak working set: not measured on this OS"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("PBF extract failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `--audit-extract` 명령: 세대 폴더를 원본 PBF와 대조하고 문제를 출력한다. 문제가 있으면 오류 종료한다.
fn audit_extract_command(dir: &str, pbf: &str) -> ExitCode {
    let path = std::path::PathBuf::from(pbf);
    let open = || {
        std::fs::File::open(&path)
            .map(std::io::BufReader::new)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))
    };
    let checked = pbf_extract::hash_file(&path).and_then(|source| {
        let (meta, _) =
            sim_core::world::road_snapshot::load_extract_generation(std::path::Path::new(dir))?;
        if meta.geofabrik.pbf_sha256 != source.sha256 {
            return Err(
                "the PBF is not the file this generation was extracted from (SHA-256 differs)"
                    .to_owned(),
            );
        }
        pbf_audit::audit_generation(open, std::path::Path::new(dir))
    });
    match checked {
        Ok(report) => {
            println!(
                "expected ways {}, snapshot ways {}, checked nodes {}",
                report.expected_ways, report.snapshot_ways, report.checked_nodes
            );
            for problem in &report.problems {
                println!("problem: {problem}");
            }
            println!("problems: {}", report.problem_count);
            if report.problem_count == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("audit failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `--compare-overpass` 명령: 기존 Overpass 자료와 추출 세대의 차이를 분류해 출력한다.
fn compare_overpass_command(old: &str, dir: &str, pbf: &str, points: Option<&String>) -> ExitCode {
    let points = match points.map(|text| parse_points(text)) {
        None => Vec::new(),
        Some(Ok(points)) => points,
        Some(Err(error)) => {
            eprintln!("invalid --pick-points: {error}");
            return ExitCode::FAILURE;
        }
    };
    let path = std::path::PathBuf::from(pbf);
    let open = || {
        std::fs::File::open(&path)
            .map(std::io::BufReader::new)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))
    };
    match pbf_compare::compare_with_overpass(
        std::path::Path::new(old),
        std::path::Path::new(dir),
        open,
        &points,
    ) {
        Ok(report) => {
            println!(
                "overpass base time: {}",
                report.base_time.as_deref().unwrap_or("unknown")
            );
            println!(
                "allowed-road ways: overpass {}, extract {}",
                report.ways.0, report.ways.1
            );
            for (label, count) in report.counts() {
                println!("{label}: {count}");
            }
            for difference in &report.differences {
                println!(
                    "- [{}] {} {} {}",
                    difference.class.label(),
                    difference.kind,
                    difference.id,
                    difference.note
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("compare failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 점 목록 인자: 세미콜론으로 나눈 위도·경도 쌍, 잘못된 값은 오류.
    #[test]
    fn points_are_parsed() {
        assert_eq!(
            parse_points("34.75,127.74; 34.74,127.75;").unwrap(),
            vec![[34.75, 127.74], [34.74, 127.75]]
        );
        assert!(parse_points("34.75").is_err());
        assert!(parse_points("a,b").is_err());
        assert!(parse_points("NaN,1").is_err());
    }
}
