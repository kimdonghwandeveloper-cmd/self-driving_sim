mod app;

use std::path::PathBuf;

/// 명령줄 인자를 읽어 앱을 실행한다.
///
/// - `--smoke-test`: 약 5초 후 자동 종료.
/// - `--capture-views <폴더>`: 네 방향 시점과 외부 디버그 시점을 PNG로 저장한 뒤 종료.
/// - `--window <폭>x<높이>`: 창 크기(기본 1280x720). 기준 사진 비교 캡처에 쓴다.
fn main() -> bevy::app::AppExit {
    let args: Vec<String> = std::env::args().collect();
    let value_after = |flag: &str| {
        args.iter()
            .position(|argument| argument == flag)
            .and_then(|index| args.get(index + 1))
    };
    let window = value_after("--window").and_then(|size| {
        let parsed = app::parse_window_size(size);
        if parsed.is_none() {
            eprintln!("ignoring invalid --window value `{size}` (expected e.g. 1498x700)");
        }
        parsed
    });
    app::run(app::RunOptions {
        smoke_test: args.iter().any(|argument| argument == "--smoke-test"),
        capture_dir: value_after("--capture-views").map(PathBuf::from),
        window,
    })
}
