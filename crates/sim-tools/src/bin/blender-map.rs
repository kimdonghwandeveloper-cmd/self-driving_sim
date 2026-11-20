#[path = "convention-map/container.rs"]
#[allow(dead_code)]
mod container;
// 레이어 병합 기능도 같은 컨테이너 모듈에서 공유한다.
#[path = "convention-map/source.rs"]
mod source;
use std::{fs, process::ExitCode};
/// 수동 GLB를 코스 좌표로 변환하고 새 폴더에 GLB와 도로 자료를 함께 쓴다.
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().collect();
    let result = (|| -> Result<(), String> {
        if args.len() != 3 {
            return Err("usage: blender-map <manual-export.glb> <new-output-directory>".into());
        }
        let input = fs::read(&args[1]).map_err(|e| e.to_string())?;
        let (glb, roads) = source::convert(&input, [34.7509700, 127.7493575])?;
        fs::create_dir(&args[2]).map_err(|e| e.to_string())?;
        let output = std::path::Path::new(&args[2]);
        fs::write(output.join("background.glb"), glb).map_err(|e| e.to_string())?;
        fs::write(
            output.join("custom-roads.json"),
            serde_json::to_vec_pretty(&roads).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
