//! Rust로 생성하는 표시 전용 도로 GLB. 기존 배경 파일과 충돌 자료는 변경하지 않는다.
use serde_json::{Value, json};

/// 도로 스냅샷의 중심선과 폭으로 삼각형 띠를 만들고 glTF 2.0 바이너리를 생성한다.
pub fn build(snapshot: &Value, origin: [f64; 2], widths: &[f64]) -> Result<Vec<u8>, String> {
    let roads = snapshot["elements"].as_array().ok_or("도로 목록 누락")?;
    if roads.len() != widths.len() {
        return Err("도로 폭 개수 불일치".into());
    }
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    for (road, width) in roads.iter().zip(widths) {
        let points = road["geometry"].as_array().ok_or("도로 좌표 누락")?;
        let mut centers = Vec::new();
        for p in points {
            let lat = p["lat"].as_f64().ok_or("위도 오류")?;
            let lon = p["lon"].as_f64().ok_or("경도 오류")?;
            centers.push([
                (lon - origin[1]).to_radians() * 6_371_008.8 * origin[0].to_radians().cos(),
                -(lat - origin[0]).to_radians() * 6_371_008.8,
            ]);
        }
        if centers.len() < 2 || !(0.5..=30.0).contains(width) {
            return Err("도로 형상 오류".into());
        }
        let mut vertices = Vec::new();
        for i in 0..centers.len() {
            let point = centers[i];
            let normalize = |a: [f64; 2], b: [f64; 2]| -> Result<[f64; 2], String> {
                let d = [b[0] - a[0], b[1] - a[1]];
                let length = d[0].hypot(d[1]);
                if length < 1e-6 {
                    return Err("퇴화 도로 방향".into());
                }
                Ok([d[0] / length, d[1] / length])
            };
            let incoming = if i == 0 {
                normalize(point, centers[1])?
            } else {
                normalize(centers[i - 1], point)?
            };
            let outgoing = if i + 1 == centers.len() {
                incoming
            } else {
                normalize(point, centers[i + 1])?
            };
            let n = [-incoming[1], incoming[0]];
            let m = [-outgoing[1], outgoing[0]];
            let sum = [n[0] + m[0], n[1] + m[1]];
            let length = sum[0].hypot(sum[1]);
            if length < 1e-6 {
                return Err("되돌림 도로".into());
            }
            let mid = [sum[0] / length, sum[1] / length];
            let scale = width / 2.0 / (mid[0] * m[0] + mid[1] * m[1]).max(0.5);
            let offset = [mid[0] * scale, mid[1] * scale];
            for sign in [-1.0, 1.0] {
                vertices.push([
                    (centers[i][0] + sign * offset[0]) as f32,
                    0.03,
                    (centers[i][1] + sign * offset[1]) as f32,
                ]);
            }
        }
        let start = bin.len();
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in &vertices {
            for axis in 0..3 {
                if !v[axis].is_finite() {
                    return Err("비유한 메시".into());
                }
                min[axis] = min[axis].min(v[axis]);
                max[axis] = max[axis].max(v[axis]);
                bin.extend_from_slice(&v[axis].to_le_bytes());
            }
        }
        views.push(
            json!({"buffer":0,"byteOffset":start,"byteLength":bin.len()-start,"target":34962}),
        );
        let position = accessors.len();
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5126,"count":vertices.len(),"type":"VEC3","min":min,"max":max}));
        let start = bin.len();
        for i in 0..centers.len() - 1 {
            let j = (2 * i) as u32;
            for index in [j, j + 1, j + 2, j + 1, j + 3, j + 2] {
                bin.extend_from_slice(&index.to_le_bytes());
            }
        }
        views.push(
            json!({"buffer":0,"byteOffset":start,"byteLength":bin.len()-start,"target":34963}),
        );
        let indices = accessors.len();
        accessors.push(json!({"bufferView":views.len()-1,"componentType":5125,"count":(centers.len()-1)*6,"type":"SCALAR"}));
        nodes.push(json!({"name":format!("RustRoad_{}",road["id"]),"mesh":meshes.len()}));
        meshes.push(json!({"primitives":[{"attributes":{"POSITION":position},"indices":indices,"material":if road["tags"]["sim_surface"]=="gallery"{1}else{0}}]}));
    }
    let document = json!({"asset":{"version":"2.0","generator":"sim-tools Rust road preview"},"scene":0,"scenes":[{"nodes":(0..nodes.len()).collect::<Vec<_>>()}],"nodes":nodes,"meshes":meshes,"materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.135,0.145,0.155,1.0],"metallicFactor":0.0,"roughnessFactor":0.82}},{"pbrMetallicRoughness":{"baseColorFactor":[0.24,0.25,0.26,1.0],"metallicFactor":0.0,"roughnessFactor":0.82}}],"accessors":accessors,"bufferViews":views,"buffers":[{"byteLength":bin.len()}]});
    let mut text = serde_json::to_vec(&document).map_err(|e| e.to_string())?;
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let length = 12 + 8 + text.len() + 8 + bin.len();
    let length = u32::try_from(length).map_err(|_| "GLB 크기 초과")?;
    let mut out = Vec::new();
    for word in [0x46546c67, 2, length, text.len() as u32, 0x4e4f534a] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend(text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004e4942u32.to_le_bytes());
    out.extend(bin);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// GLB 청크 길이와 좌표계·상향 삼각형 및 퇴화 입력 거부를 확인한다.
    #[test]
    fn writes_valid_upward_strip() {
        let s = json!({"elements":[{"id":1,"geometry":[{"lat":34.0,"lon":127.0},{"lat":34.001,"lon":127.0}]}]});
        let bytes = build(&s, [34.0, 127.0], &[6.0]).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(doc["accessors"][0]["count"], 4);
        assert_eq!(doc["accessors"][1]["count"], 6);
        assert!(build(&s, [34.0, 127.0], &[]).is_err());
        let start = 28 + len;
        let read = |i: usize| {
            f32::from_le_bytes(bytes[start + i * 4..start + i * 4 + 4].try_into().unwrap())
        };
        let ax = read(3) - read(0);
        let az = read(5) - read(2);
        let bx = read(6) - read(0);
        let bz = read(8) - read(2);
        assert!(az * bx - ax * bz > 0.0, "윗면 삼각형 방향");
    }
}
