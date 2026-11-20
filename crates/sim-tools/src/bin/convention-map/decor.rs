//! 이미지 지면과 갤러리 캐노피를 Rust GLB에 추가한다.
use super::container::{decode, encode};
use serde_json::{Value, json};
/// 픽셀 좌표를 두 기준점에 맞춘 동쪽·남쪽 미터로 변환한다.
fn world(trace: &Value, p: [f64; 2]) -> Result<[f64; 2], String> {
    let a = &trace["bridge_controls"][0];
    let b = &trace["bridge_controls"][1];
    let read = |v: &Value| v.as_f64().ok_or("기준점 숫자 오류");
    let ax = read(&a["pixel"][0])?;
    let ay = read(&a["pixel"][1])?;
    let dx = read(&b["pixel"][0])? - ax;
    let dy = ay - read(&b["pixel"][1])?;
    let wx = read(&b["east_north_m"][0])? - read(&a["east_north_m"][0])?;
    let wy = read(&b["east_north_m"][1])? - read(&a["east_north_m"][1])?;
    let n = dx * dx + dy * dy;
    if n < 1e-12 {
        return Err("퇴화 기준점".into());
    }
    let re = (wx * dx + wy * dy) / n;
    let im = (wy * dx - wx * dy) / n;
    let x = p[0] - ax;
    let y = ay - p[1];
    Ok([
        read(&a["east_north_m"][0])? + x * re - y * im,
        -(read(&a["east_north_m"][1])? + x * im + y * re),
    ])
}
/// 정점·삼각형·선택 UV를 내장 버퍼에 추가하고 장면 노드를 연결한다.
fn mesh(
    doc: &mut Value,
    bin: &mut Vec<u8>,
    name: &str,
    vertices: &[[f32; 3]],
    indices: &[u32],
    uv: Option<&[[f32; 2]]>,
    material: usize,
) -> Result<(), String> {
    let mut attributes = json!({});
    for (semantic, size, values) in [
        (
            "POSITION",
            3,
            vertices
                .iter()
                .flat_map(|v| v.iter().copied())
                .collect::<Vec<_>>(),
        ),
        (
            "TEXCOORD_0",
            2,
            uv.unwrap_or(&[])
                .iter()
                .flat_map(|v| v.iter().copied())
                .collect(),
        ),
    ] {
        if values.is_empty() {
            continue;
        }
        let offset = bin.len();
        for v in &values {
            if !v.is_finite() {
                return Err("비유한 표시 정점".into());
            }
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let view = doc["bufferViews"].as_array().ok_or("뷰 누락")?.len();
        doc["bufferViews"].as_array_mut().unwrap().push(
            json!({"buffer":0,"byteOffset":offset,"byteLength":values.len()*4,"target":34962}),
        );
        let accessor = doc["accessors"].as_array().unwrap().len();
        let mut record = json!({"bufferView":view,"componentType":5126,"count":values.len()/size,"type":if size==3{"VEC3"}else{"VEC2"}});
        if size == 3 {
            let min: [f32; 3] = std::array::from_fn(|i| {
                vertices.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min)
            });
            let max: [f32; 3] = std::array::from_fn(|i| {
                vertices
                    .iter()
                    .map(|p| p[i])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            record["min"] = json!(min);
            record["max"] = json!(max);
        }
        doc["accessors"].as_array_mut().unwrap().push(record);
        attributes[semantic] = json!(accessor);
    }
    let offset = bin.len();
    for i in indices {
        if *i as usize >= vertices.len() {
            return Err("표시 인덱스 범위 오류".into());
        }
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let view = doc["bufferViews"].as_array().unwrap().len();
    doc["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":offset,"byteLength":indices.len()*4,"target":34963}));
    let accessor = doc["accessors"].as_array().unwrap().len();
    doc["accessors"].as_array_mut().unwrap().push(
        json!({"bufferView":view,"componentType":5125,"count":indices.len(),"type":"SCALAR"}),
    );
    let id = doc["meshes"].as_array().unwrap().len();
    doc["meshes"].as_array_mut().unwrap().push(json!({"name":name,"primitives":[{"attributes":attributes,"indices":accessor,"material":material}]}));
    let node = doc["nodes"].as_array().unwrap().len();
    doc["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":name,"mesh":id}));
    doc["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!(node));
    Ok(())
}
/// 지면을 가로막는 기둥 없이 기존 치수의 사각 단면 캐노피 빔을 추가한다.
fn beam(
    vertices: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    a: [f64; 2],
    b: [f64; 2],
    width: f64,
    y: f64,
    height: f64,
) -> Result<(), String> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    if length < 1e-6 {
        return Err("퇴화 캐노피 선분".into());
    }
    let n = [-d[1] / length * width / 2., d[0] / length * width / 2.];
    let base = vertices.len() as u32;
    for yy in [y, y + height] {
        for p in [a, b] {
            for sign in [-1., 1.] {
                vertices.push([
                    (p[0] + sign * n[0]) as f32,
                    yy as f32,
                    (p[1] + sign * n[1]) as f32,
                ]);
            }
        }
    }
    for face in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        for i in [face[0], face[1], face[2], face[0], face[2], face[3]] {
            indices.push(base + i);
        }
    }
    Ok(())
}
/// 검증한 PNG를 내장하고 기존 치수·UV와 동일한 이미지 평면 및 캐노피를 생성한다.
pub fn decorate(bytes: &[u8], trace: &Value, png: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let dimensions = [reader.info().width as f64, reader.info().height as f64];
    let size = reader.output_buffer_size().ok_or("이미지 크기 초과")?;
    if size > 256 * 1024 * 1024 {
        return Err("이미지 크기 상한 초과".into());
    }
    reader
        .next_frame(&mut vec![0; size])
        .map_err(|e| e.to_string())?;
    let (mut doc, mut bin) = decode(bytes)?;
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let w = trace["reference_width_px"]
        .as_f64()
        .ok_or("미리보기 폭 누락")?;
    let cw = trace["reference_content_size_px"][0]
        .as_f64()
        .ok_or("내용 폭 누락")?;
    let ch = trace["reference_content_size_px"][1]
        .as_f64()
        .ok_or("내용 높이 누락")?;
    if w <= 0. || cw <= 0. || ch <= 0. || cw > dimensions[0] || ch > dimensions[1] {
        return Err("이미지 내용 영역 오류".into());
    }
    let h = ch * w / cw;
    let mut points = Vec::new();
    for p in [[0., 0.], [0., h], [w, h], [w, 0.]] {
        let p = world(trace, p)?;
        points.push([p[0] as f32, 0.005, p[1] as f32]);
    }
    let offset = bin.len();
    bin.extend_from_slice(png);
    let view = doc["bufferViews"].as_array().unwrap().len();
    doc["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer":0,"byteOffset":offset,"byteLength":png.len()}));
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    doc["images"] = json!([{"bufferView":view,"mimeType":"image/png"}]);
    doc["textures"] = json!([{"source":0}]);
    let material = doc["materials"].as_array().unwrap().len();
    doc["materials"].as_array_mut().unwrap().push(json!({"name":"Aerial reference","pbrMetallicRoughness":{"baseColorTexture":{"index":0},"metallicFactor":0.,"roughnessFactor":0.95}}));
    let right = (cw / dimensions[0]) as f32;
    let bottom = (ch / dimensions[1]) as f32;
    mesh(
        &mut doc,
        &mut bin,
        "Aerial_Context",
        &points,
        &[0, 1, 2, 0, 2, 3],
        Some(&[[0., 0.], [0., bottom], [right, bottom], [right, 0.]]),
        material,
    )?;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let canopy = trace["gallery_canopy"].as_array().ok_or("캐노피 누락")?;
    if canopy.len() < 2 {
        return Err("캐노피 점 부족".into());
    }
    for pair in canopy.windows(2) {
        let read = |p: &Value| -> Result<[f64; 2], String> {
            world(
                trace,
                [
                    p[0].as_f64().ok_or("캐노피 좌표 오류")?,
                    p[1].as_f64().ok_or("캐노피 좌표 오류")?,
                ],
            )
        };
        let a = read(&pair[0])?;
        let b = read(&pair[1])?;
        let d = [b[0] - a[0], b[1] - a[1]];
        let length = d[0].hypot(d[1]);
        if length < 1e-6 {
            return Err("퇴화 캐노피".into());
        }
        let axis = [d[0] / length, d[1] / length];
        let side = [-axis[1], axis[0]];
        for distance in (2..length as usize).step_by(4) {
            let center = [
                a[0] + axis[0] * distance as f64,
                a[1] + axis[1] * distance as f64,
            ];
            beam(
                &mut vertices,
                &mut indices,
                [center[0] - side[0] * 5.4, center[1] - side[1] * 5.4],
                [center[0] + side[0] * 5.4, center[1] + side[1] * 5.4],
                0.28,
                7.5,
                0.32,
            )?;
        }
        for o in [-5.4, 5.4] {
            beam(
                &mut vertices,
                &mut indices,
                [a[0] + side[0] * o, a[1] + side[1] * o],
                [b[0] + side[0] * o, b[1] + side[1] * o],
                0.36,
                7.4,
                0.5,
            )?;
        }
    }
    let material = doc["materials"].as_array().unwrap().len();
    doc["materials"].as_array_mut().unwrap().push(json!({"name":"Gallery roof white steel","pbrMetallicRoughness":{"baseColorFactor":[0.65,0.72,0.75,1.],"metallicFactor":0.,"roughnessFactor":0.82}}));
    mesh(
        &mut doc,
        &mut bin,
        "Convention_Gallery_Canopy",
        &vertices,
        &indices,
        None,
        material,
    )?;
    encode(&mut doc, &mut bin)
}
#[cfg(test)]
mod tests {
    use super::*;
    /// PNG 내장·UV 여백·지면 높이·캐노피 치수와 손상 입력 거부를 확인한다.
    #[test]
    fn embeds_reference_and_generates_canopy() {
        let snapshot = json!({"elements":[{"id":1,"geometry":[{"lat":34.0,"lon":127.0},{"lat":34.001,"lon":127.0}]}]});
        let base = super::super::glb::build(&snapshot, [34., 127.], &[6.]).unwrap();
        let trace = json!({"bridge_controls":[{"pixel":[0,0],"east_north_m":[0,0]},{"pixel":[10,0],"east_north_m":[10,0]}],"reference_width_px":4,"reference_content_size_px":[3,2],"gallery_canopy":[[0,0],[20,0]]});
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 4, 4);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255; 48]).unwrap();
        }
        let output = decorate(&base, &trace, &png).unwrap();
        let (doc, bin) = decode(&output).unwrap();
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 3);
        assert_eq!(doc["images"][0]["mimeType"], "image/png");
        let view = &doc["bufferViews"][doc["images"][0]["bufferView"].as_u64().unwrap() as usize];
        let start = view["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&bin[start..start + png.len()], png.as_slice());
        let canopy = doc["meshes"].as_array().unwrap().last().unwrap();
        let id = canopy["primitives"][0]["attributes"]["POSITION"]
            .as_u64()
            .unwrap() as usize;
        assert!((doc["accessors"][id]["max"][1].as_f64().unwrap() - 7.9).abs() < 1e-5);
        let combined = super::super::container::merge(&base, &output).unwrap();
        let (merged, _) = decode(&combined).unwrap();
        assert_eq!(merged["meshes"].as_array().unwrap().len(), 4);
        assert!(decorate(&base, &trace, b"broken PNG").is_err());
        let mut invalid = trace;
        invalid["reference_content_size_px"] = json!([100, 100]);
        assert!(decorate(&base, &invalid, &png).is_err());
    }
}
