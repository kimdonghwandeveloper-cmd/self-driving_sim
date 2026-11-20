use serde_json::{Value, json};
/// GLB 2.0의 경계와 내장 버퍼 길이를 검사해 JSON과 BIN을 분리한다.
pub fn decode(bytes: &[u8]) -> Result<(Value, Vec<u8>), String> {
    if bytes.len() < 28
        || &bytes[..4] != b"glTF"
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
    {
        return Err("GLB 헤더 오류".into());
    }
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if !n.is_multiple_of(4) || 20 + n + 8 > bytes.len() || &bytes[16..20] != b"JSON" {
        return Err("GLB JSON 청크 오류".into());
    }
    let doc: Value = serde_json::from_slice(&bytes[20..20 + n]).map_err(|e| e.to_string())?;
    let m = u32::from_le_bytes(bytes[20 + n..24 + n].try_into().unwrap()) as usize;
    if &bytes[24 + n..28 + n] != b"BIN\0" || 28 + n + m != bytes.len() {
        return Err("GLB BIN 청크 오류".into());
    }
    if doc["buffers"].as_array().ok_or("버퍼 누락")?.len() != 1
        || doc["buffers"][0].get("uri").is_some()
    {
        return Err("내장 단일 버퍼만 지원".into());
    }
    Ok((doc, bytes[28 + n..].to_vec()))
}
/// GLB 문서와 버퍼를 네 바이트 정렬하여 직렬화한다.
pub fn encode(doc: &mut Value, bin: &mut Vec<u8>) -> Result<Vec<u8>, String> {
    doc["buffers"] = json!([{"byteLength":bin.len()}]);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut text = serde_json::to_vec(doc).map_err(|e| e.to_string())?;
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let len = u32::try_from(28 + text.len() + bin.len()).map_err(|_| "GLB 크기 초과")?;
    let mut result = Vec::new();
    for v in [0x46546c67, 2, len, text.len() as u32, 0x4e4f534a] {
        result.extend_from_slice(&v.to_le_bytes());
    }
    result.extend(text);
    result.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    result.extend_from_slice(b"BIN\0");
    result.extend_from_slice(bin);
    Ok(result)
}
/// 기존 정합 배경을 유지하면서 Rust 생성 표시 레이어를 내장 버퍼에 합친다.
pub fn merge(base: &[u8], layer: &[u8]) -> Result<Vec<u8>, String> {
    let (mut a, mut bin) = decode(base)?;
    let (mut b, data) = decode(layer)?;
    if b.get("animations").is_some() || b.get("skins").is_some() {
        return Err("표시 레이어 애니메이션 미지원".into());
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let offset = bin.len();
    bin.extend(data);
    let count = |key: &str| a[key].as_array().map_or(0, Vec::len);
    let views = count("bufferViews");
    let access = count("accessors");
    let mats = count("materials");
    let meshes = count("meshes");
    let nodes = count("nodes");
    let images = count("images");
    let textures = count("textures");
    let samplers = count("samplers");
    for view in b["bufferViews"].as_array_mut().ok_or("레이어 뷰 누락")? {
        view["byteOffset"] = json!(view["byteOffset"].as_u64().unwrap_or(0) + offset as u64);
    }
    for v in b["accessors"].as_array_mut().ok_or("레이어 접근자 누락")? {
        v["bufferView"] = json!(v["bufferView"].as_u64().ok_or("접근자 뷰 누락")? + views as u64);
    }
    for v in b["meshes"].as_array_mut().ok_or("레이어 메시 누락")? {
        for p in v["primitives"].as_array_mut().ok_or("프리미티브 누락")? {
            for (_, id) in p["attributes"].as_object_mut().ok_or("속성 누락")? {
                *id = json!(id.as_u64().ok_or("속성 번호 오류")? + access as u64);
            }
            p["indices"] = json!(p["indices"].as_u64().ok_or("인덱스 누락")? + access as u64);
            p["material"] = json!(p["material"].as_u64().ok_or("재질 누락")? + mats as u64);
        }
    }
    for v in b["nodes"].as_array_mut().ok_or("레이어 노드 누락")? {
        v["mesh"] = json!(v["mesh"].as_u64().ok_or("노드 메시 누락")? + meshes as u64);
    }
    if let Some(items) = b["images"].as_array_mut() {
        for v in items {
            v["bufferView"] =
                json!(v["bufferView"].as_u64().ok_or("이미지 뷰 누락")? + views as u64);
        }
    }
    if let Some(items) = b["textures"].as_array_mut() {
        for v in items {
            v["source"] = json!(v["source"].as_u64().ok_or("이미지 소스 누락")? + images as u64);
            if let Some(id) = v.get_mut("sampler") {
                *id = json!(id.as_u64().ok_or("샘플러 오류")? + samplers as u64);
            }
        }
    }
    for v in b["materials"].as_array_mut().ok_or("재질 누락")? {
        if let Some(id) = v.pointer_mut("/pbrMetallicRoughness/baseColorTexture/index") {
            *id = json!(id.as_u64().ok_or("텍스처 번호 오류")? + textures as u64);
        }
    }
    let mut roots = a["scenes"][0]["nodes"]
        .as_array()
        .ok_or("장면 노드 누락")?
        .clone();
    for id in b["scenes"][0]["nodes"]
        .as_array()
        .ok_or("레이어 장면 누락")?
    {
        roots.push(json!(id.as_u64().ok_or("노드 번호 오류")? + nodes as u64));
    }
    a["scenes"][0]["nodes"] = json!(roots);
    for key in [
        "bufferViews",
        "accessors",
        "meshes",
        "nodes",
        "images",
        "textures",
        "materials",
        "samplers",
    ] {
        if let Some(items) = b[key].as_array() {
            if a.get(key).is_none() {
                a[key] = json!([]);
            }
            a[key]
                .as_array_mut()
                .ok_or("배경 배열 오류")?
                .extend(items.iter().cloned());
        }
    }
    encode(&mut a, &mut bin)
}
