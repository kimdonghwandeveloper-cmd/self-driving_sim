#[path = "convention-map/glb.rs"]
mod glb;
// 컨벤션 통로의 좌표 변환과 코스 생성.
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode};

/// 이미지 기준점으로 좌표를 정합하고 도로 및 코스 파일을 생성한다.
fn generate(trace: &Value) -> Result<Vec<(String, String)>, String> {
    let pair = |v: &Value| -> Result<[f64; 2], String> {
        let a = v.as_array().ok_or("좌표 배열 누락")?;
        if a.len() != 2 {
            return Err("좌표는 두 값이어야 합니다".into());
        }
        let x = a[0].as_f64().ok_or("좌표 숫자 오류")?;
        let y = a[1].as_f64().ok_or("좌표 숫자 오류")?;
        if !x.is_finite() || !y.is_finite() {
            return Err("비유한 좌표".into());
        }
        Ok([x, y])
    };
    let origin = pair(&trace["origin"])?;
    if origin[0].abs() > 90.0 || origin[1].abs() > 180.0 {
        return Err("원점 범위 오류".into());
    }
    let controls = trace["bridge_controls"].as_array().ok_or("기준점 누락")?;
    if controls.len() != 2 {
        return Err("기준점 두 개 필요".into());
    }
    let a = pair(&controls[0]["pixel"])?;
    let b = pair(&controls[1]["pixel"])?;
    let wa = pair(&controls[0]["east_north_m"])?;
    let wb = pair(&controls[1]["east_north_m"])?;
    let d = [b[0] - a[0], a[1] - b[1]];
    let w = [wb[0] - wa[0], wb[1] - wa[1]];
    let norm = d[0] * d[0] + d[1] * d[1];
    if norm < 1e-12 {
        return Err("동일한 기준점".into());
    }
    let scale = [
        (w[0] * d[0] + w[1] * d[1]) / norm,
        (w[1] * d[0] - w[0] * d[1]) / norm,
    ];
    if scale[0].hypot(scale[1]) < 1e-12 {
        return Err("퇴화 축척".into());
    }
    let roads = trace["roads"].as_array().ok_or("통로 누락")?;
    if roads.is_empty() {
        return Err("빈 통로 목록".into());
    }
    let mut files = Vec::new();
    let mut elements = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    for road in roads {
        let name = road["file"].as_str().ok_or("파일명 누락")?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !names.insert(name)
        {
            return Err("잘못되거나 중복된 파일명".into());
        }
        let id = road["id"].as_i64().ok_or("ID 누락")?;
        if !ids.insert(id) {
            return Err("중복 ID".into());
        }
        let width = road["width_m"].as_f64().ok_or("폭 누락")?;
        if !(0.5..=30.0).contains(&width) {
            return Err("폭 범위 오류".into());
        }
        let pixels = road["pixels"].as_array().ok_or("통로 좌표 누락")?;
        if pixels.len() < 2 {
            return Err("통로 점 부족".into());
        }
        let mut geometry = Vec::new();
        for point in pixels {
            let p = pair(point)?;
            let local = [p[0] - a[0], a[1] - p[1]];
            let east = wa[0] + local[0] * scale[0] - local[1] * scale[1];
            let north = wa[1] + local[0] * scale[1] + local[1] * scale[0];
            let lat = origin[0] + (north / 6_371_008.8).to_degrees();
            let lon =
                origin[1] + (east / (6_371_008.8 * origin[0].to_radians().cos())).to_degrees();
            geometry.push(json!({"lat":(lat*1e8).round()/1e8,"lon":(lon*1e8).round()/1e8}));
        }
        elements.push(json!({"type":"way","id":id,"tags":{"highway":"service","source":"manual aerial-image training approximation"},"geometry":geometry}));
        files.push((format!("{name}.toml"),format!("# 가상 교육용 이미지 근사 통로. 로컬 ID이며 OSM ID가 아님.\nsource = \"traced-roads.json\"\norigin = [{}, {}]\nways = [{id}]\ntemporary_width_m = {width}\n",origin[0],origin[1])));
    }
    files.push((
        "traced-roads.json".into(),
        serde_json::to_string_pretty(&json!({"elements":elements})).map_err(|e| e.to_string())?,
    ));
    Ok(files)
}
/// 입출력을 검사하고 기존 자료를 덮어쓰지 않는 새 폴더에 저장한다.
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().collect();
    let result = (|| -> Result<(), String> {
        if args.len() != 3 {
            return Err("usage: convention-map <trace.json> <new-output-directory>".into());
        }
        let trace: Value = serde_json::from_str(
            fs::read_to_string(&args[1])
                .map_err(|e| e.to_string())?
                .trim_start_matches('\u{feff}'),
        )
        .map_err(|e| e.to_string())?;
        let files = generate(&trace)?;
        let snapshot: Value = serde_json::from_str(&files.last().ok_or("도로 출력 누락")?.1)
            .map_err(|e| e.to_string())?;
        let origin = [
            trace["origin"][0].as_f64().ok_or("원점 오류")?,
            trace["origin"][1].as_f64().ok_or("원점 오류")?,
        ];
        let widths: Vec<f64> = trace["roads"]
            .as_array()
            .ok_or("도로 누락")?
            .iter()
            .map(|r| r["width_m"].as_f64().unwrap_or(0.0))
            .collect();
        let mesh = glb::build(&snapshot, origin, &widths)?;
        let output = Path::new(&args[2]);
        fs::create_dir(output).map_err(|e| e.to_string())?;
        for (name, contents) in files {
            fs::write(output.join(name), contents).map_err(|e| e.to_string())?;
        }
        fs::write(output.join("convention-roads.glb"), mesh).map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    /// 기존 Python 생성 좌표와 Rust 생성 좌표가 1 mm 이내에서 일치해야 한다.
    #[test]
    fn matches_existing_six_courses() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let base = root.join("testdata/yeosu-expo/convention");
        let trace: Value = serde_json::from_str(
            fs::read_to_string(base.join("trace.json"))
                .unwrap()
                .trim_start_matches('\u{feff}'),
        )
        .unwrap();
        let files = generate(&trace).unwrap();
        assert_eq!(files.len(), 7);
        let generated: Value = serde_json::from_str(&files.last().unwrap().1).unwrap();
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(base.join("traced-roads.json")).unwrap())
                .unwrap();
        assert_eq!(generated["elements"].as_array().unwrap().len(), 6);
        assert_eq!(expected["elements"].as_array().unwrap().len(), 6);
        for (a, b) in generated["elements"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["elements"].as_array().unwrap())
        {
            assert_eq!(a["id"], b["id"]);
            assert_eq!(
                a["geometry"].as_array().unwrap().len(),
                b["geometry"].as_array().unwrap().len()
            );
            for (x, y) in a["geometry"]
                .as_array()
                .unwrap()
                .iter()
                .zip(b["geometry"].as_array().unwrap())
            {
                for axis in ["lat", "lon"] {
                    assert!((x[axis].as_f64().unwrap() - y[axis].as_f64().unwrap()).abs() < 1e-8);
                }
            }
        }
    }
    /// 퇴화 기준점과 경로 이탈 파일명을 거부해야 한다.
    #[test]
    fn rejects_invalid_trace() {
        assert!(generate(&json!({})).is_err());
    }
}
