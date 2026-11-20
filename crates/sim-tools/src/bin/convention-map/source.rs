use super::container::{decode, encode};
// 수동 내보낸 GLB를 코스 원점으로 변환한다. Blender 표시용 위치·배율은 적용하지 않는다.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// 접근자 자료형·stride·범위를 확인하고 각 원소의 버퍼 위치를 반환한다.
fn positions(
    doc: &Value,
    bin: &[u8],
    id: usize,
    kind: &str,
    component: u64,
    size: usize,
) -> Result<Vec<usize>, String> {
    let a = &doc["accessors"][id];
    let view = &doc["bufferViews"][a["bufferView"].as_u64().ok_or("뷰 누락")? as usize];
    if a["type"] != kind
        || a["componentType"] != component
        || a.get("sparse").is_some()
        || view["buffer"] != 0
    {
        return Err("지원하지 않는 접근자".into());
    }
    let count = a["count"].as_u64().ok_or("개수 누락")? as usize;
    let base = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let offset = a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"].as_u64().unwrap_or(size as u64) as usize;
    let length = view["byteLength"].as_u64().ok_or("뷰 길이 누락")? as usize;
    if stride < size
        || base.checked_add(length).is_none_or(|end| end > bin.len())
        || count > bin.len() / size
    {
        return Err("접근자 범위 오류".into());
    }
    let mut out = Vec::new();
    for i in 0..count {
        let p = offset
            .checked_add(i.checked_mul(stride).ok_or("크기 초과")?)
            .ok_or("크기 초과")?;
        if p.checked_add(size).is_none_or(|end| end > length) {
            return Err("접근자 길이 오류".into());
        }
        out.push(base + p);
    }
    Ok(out)
}
/// float32 위치 접근자를 유한한 세 좌표로 읽는다.
fn vertices(doc: &Value, bin: &[u8], id: usize) -> Result<Vec<[f64; 3]>, String> {
    positions(doc, bin, id, "VEC3", 5126, 12)?
        .iter()
        .map(|p| {
            let mut v = [0.; 3];
            for k in 0..3 {
                v[k] = f32::from_le_bytes(bin[p + k * 4..p + k * 4 + 4].try_into().unwrap()) as f64;
                if !v[k].is_finite() {
                    return Err("비유한 정점".into());
                }
            }
            Ok(v)
        })
        .collect()
}
/// 삼각형 인덱스를 uint16 또는 uint32 접근자에서 읽는다.
fn indices(doc: &Value, bin: &[u8], id: usize) -> Result<Vec<usize>, String> {
    let c = doc["accessors"][id]["componentType"]
        .as_u64()
        .ok_or("인덱스 자료형 누락")?;
    let size = match c {
        5123 => 2,
        5125 => 4,
        _ => return Err("인덱스 자료형 오류".into()),
    };
    positions(doc, bin, id, "SCALAR", c, size)?
        .iter()
        .map(|p| {
            Ok(if size == 2 {
                u16::from_le_bytes(bin[*p..p + 2].try_into().unwrap()) as usize
            } else {
                u32::from_le_bytes(bin[*p..p + 4].try_into().unwrap()) as usize
            })
        })
        .collect()
}
/// 원본 정점 연결 관계를 보존하고 연속된 두 삼각형으로 사각형 띠를 복원한다.
fn road_lines(points: &[[f64; 3]], triangles: &[usize]) -> Result<Value, String> {
    let verts = points.to_vec();
    let remap: Vec<usize> = (0..points.len()).collect();
    if !triangles.len().is_multiple_of(6) {
        return Err("도로는 연속된 사각형 삼각분할이어야 함".into());
    }
    let mut quads = Vec::new();
    let mut degenerate = 0;
    for pair in triangles.as_chunks::<6>().0 {
        if pair.iter().any(|i| *i >= points.len()) {
            return Err("정점 인덱스 범위 오류".into());
        }
        let a = pair[..3].iter().map(|i| remap[*i]).collect::<Vec<_>>();
        let b = pair[3..].iter().map(|i| remap[*i]).collect::<Vec<_>>();
        let mut edges = BTreeMap::new();
        for face in [&a, &b] {
            for i in 0..3 {
                let edge = if face[i] < face[(i + 1) % 3] {
                    [face[i], face[(i + 1) % 3]]
                } else {
                    [face[(i + 1) % 3], face[i]]
                };
                *edges.entry(edge).or_insert(0) += 1;
            }
        }
        let boundary: Vec<_> = edges
            .into_iter()
            .filter_map(|(e, n)| (n == 1).then_some(e))
            .collect();
        if a.iter().copied().collect::<BTreeSet<_>>().len() < 3
            || b.iter().copied().collect::<BTreeSet<_>>().len() < 3
        {
            degenerate += 1;
            continue;
        }
        if boundary.len() != 4 {
            return Err(format!("도로 사각형 복원 실패: {pair:?}, {a:?}, {b:?}"));
        }
        let x = points[pair[0]];
        let y = points[pair[1]];
        let z = points[pair[2]];
        let u = [y[0] - x[0], y[1] - x[1], y[2] - x[2]];
        let v = [z[0] - x[0], z[1] - x[1], z[2] - x[2]];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        if length < 1e-9
            || normal[1].abs() / length < 0.5
            || pair.iter().any(|i| points[*i][1] + 0.04 >= 1.0)
        {
            degenerate += 1;
            continue;
        }
        quads.push(boundary);
    }
    let mut owners: BTreeMap<[usize; 2], Vec<usize>> = BTreeMap::new();
    for (i, q) in quads.iter().enumerate() {
        for e in q {
            owners.entry(*e).or_default().push(i);
        }
    }
    let mut neighbors = vec![Vec::new(); quads.len()];
    for (e, o) in &owners {
        if o.len() == 2 {
            neighbors[o[0]].push((o[1], *e));
            neighbors[o[1]].push((o[0], *e));
        } else if o.len() > 2 {
            return Err("비다양체 도로".into());
        }
    }
    if neighbors.iter().any(|n| n.len() > 2) {
        return Err("분기하는 사각형 띠".into());
    }
    let middle = |e: [usize; 2]| {
        [
            (verts[e[0]][0] + verts[e[1]][0]) / 2.,
            (verts[e[0]][2] + verts[e[1]][2]) / 2.,
        ]
    };
    let distance =
        |e: [usize; 2]| (verts[e[0]][0] - verts[e[1]][0]).hypot(verts[e[0]][2] - verts[e[1]][2]);
    let opposite = |q: usize, e: [usize; 2]| {
        quads[q]
            .iter()
            .find(|f| !f.contains(&e[0]) && !f.contains(&e[1]))
            .copied()
            .ok_or("맞은편 변 누락")
    };
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for first in 0..quads.len() {
        if seen.contains(&first) {
            continue;
        }
        let mut stack = vec![first];
        let mut component = Vec::new();
        seen.insert(first);
        while let Some(i) = stack.pop() {
            component.push(i);
            for (j, _) in &neighbors[i] {
                if seen.insert(*j) {
                    stack.push(*j);
                }
            }
        }
        let ends: Vec<_> = component
            .iter()
            .filter(|i| neighbors[**i].len() < 2)
            .copied()
            .collect();
        let mut closed = ends.is_empty();
        let start = *if closed { &component } else { &ends }
            .iter()
            .min()
            .ok_or("빈 띠")?;
        let mut cross = Vec::new();
        if neighbors[start].is_empty() {
            let a = quads[start][0];
            let b = opposite(start, a)?;
            let c = *quads[start]
                .iter()
                .find(|e| **e != a && **e != b)
                .ok_or("가로 변 누락")?;
            let d = opposite(start, c)?;
            cross = if distance(a) + distance(b) < distance(c) + distance(d) {
                vec![a, b]
            } else {
                vec![c, d]
            };
        } else {
            if !closed {
                cross.push(opposite(start, neighbors[start][0].1)?);
            }
            let mut prev = usize::MAX;
            let mut current = start;
            while let Some(&(next, e)) = neighbors[current].iter().find(|(j, _)| *j != prev) {
                cross.push(e);
                if next == start {
                    break;
                }
                prev = current;
                current = next;
                if neighbors[current].len() == 1 {
                    cross.push(opposite(current, e)?);
                    break;
                }
            }
        }
        // GLB의 UV 이음새가 끝 정점을 분리한 폐곡선은 같은 단면일 때만 고리를 복원한다.
        if !closed && cross.len() > 2 {
            let first = middle(cross[0]);
            let last = middle(*cross.last().unwrap());
            if (first[0] - last[0]).hypot(first[1] - last[1]) < 0.0015
                && (distance(cross[0]) - distance(*cross.last().unwrap())).abs() < 0.0015
            {
                closed = true;
                cross.pop();
            }
        }
        let mut widths: Vec<_> = cross.iter().map(|e| distance(*e)).collect();
        widths.sort_by(f64::total_cmp);
        if widths.is_empty() {
            return Err("빈 가로 변".into());
        }
        let round = |n: f64| (n * 1000.).round() / 1000.;
        lines.push(json!({"points":cross.into_iter().map(|e|middle(e).map(round)).collect::<Vec<_>>(),"width_m":round(widths[widths.len()/2]),"closed":closed}));
    }
    Ok(json!({"lines":lines,"degenerate_quads_skipped":degenerate}))
}
/// EPSG:3857 원본 GLB를 지역 미터로 변환하고 원본 사각형 도로 띠를 추출한다.
pub fn convert(bytes: &[u8], origin: [f64; 2]) -> Result<(Vec<u8>, Value), String> {
    let (mut doc, mut bin) = decode(bytes)?;
    let scene = doc["scene"].as_u64().unwrap_or(0) as usize;
    let extra = &doc["scenes"][scene]["extras"];
    if extra["SRID"] != "EPSG:3857" {
        return Err("EPSG:3857 장면 속성 필요".into());
    }
    let cx = extra["crs x"].as_f64().ok_or("crs x 누락")?;
    let cy = extra["crs y"].as_f64().ok_or("crs y 누락")?;
    let mut kept = Vec::new();
    let mut used = BTreeSet::new();
    let mut lines = None;
    let nodes = doc["nodes"].as_array().ok_or("노드 누락")?.clone();
    for (i, node) in nodes.iter().enumerate() {
        let name = node["name"].as_str().unwrap_or("");
        let offset = match name {
            "Ways" => -0.04,
            "Areas" => -0.06,
            "EXPORT_OSM_MAPNIK_WM" => -0.08,
            _ => continue,
        };
        if node.get("rotation").is_some()
            || node.get("matrix").is_some()
            || node.get("children").is_some()
        {
            return Err("회전 또는 계층 객체는 지원하지 않음".into());
        }
        let mesh = node["mesh"].as_u64().ok_or("메시 누락")? as usize;
        let primitives = doc["meshes"][mesh]["primitives"]
            .as_array()
            .ok_or("프리미티브 누락")?
            .clone();
        for primitive in primitives {
            if primitive.get("extensions").is_some() || primitive["mode"].as_u64().unwrap_or(4) != 4
            {
                return Err("압축·비삼각형 메시 미지원".into());
            }
            let id = primitive["attributes"]["POSITION"]
                .as_u64()
                .ok_or("위치 누락")? as usize;
            if !used.insert(id) {
                return Err("공유 위치 접근자 미지원".into());
            }
            let mut verts = vertices(&doc, &bin, id)?;
            let places = positions(&doc, &bin, id, "VEC3", 5126, 12)?;
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for (v, p) in verts.iter_mut().zip(places) {
                let lon = (cx + v[0]) / 6_378_137.;
                let lat = (2. * ((cy - v[2]) / 6_378_137.).exp().atan()
                    - std::f64::consts::FRAC_PI_2)
                    .to_degrees();
                let east =
                    6_371_008.8 * (lon - origin[1].to_radians()) * origin[0].to_radians().cos();
                let south = -6_371_008.8 * (lat - origin[0]).to_radians();
                *v = [east, v[1] + offset, south];
                for k in 0..3 {
                    if !v[k].is_finite() {
                        return Err("좌표 변환 오류".into());
                    }
                    min[k] = min[k].min(v[k]);
                    max[k] = max[k].max(v[k]);
                    bin[p + k * 4..p + k * 4 + 4].copy_from_slice(&(v[k] as f32).to_le_bytes());
                }
            }
            doc["accessors"][id]["min"] = json!(min);
            doc["accessors"][id]["max"] = json!(max);
            if name == "Ways" {
                if lines.is_some() {
                    return Err("도로 프리미티브는 하나여야 함".into());
                }
                let idx = indices(
                    &doc,
                    &bin,
                    primitive["indices"].as_u64().ok_or("인덱스 누락")? as usize,
                )?;
                lines = Some(road_lines(&verts, &idx)?);
            }
        }
        doc["nodes"][i]
            .as_object_mut()
            .ok_or("노드 오류")?
            .remove("translation");
        doc["nodes"][i].as_object_mut().unwrap().remove("scale");
        kept.push(i);
    }
    if kept.len() != 3 {
        return Err("Ways·Areas·배경 객체 세 개 필요".into());
    }
    doc["scenes"] = json!([{"nodes":kept,"extras":{"source_crs_x":cx,"source_crs_y":cy,"origin":origin,"excluded_non_ground_or_degenerate_quads":lines.as_ref().ok_or("도로 누락")?["degenerate_quads_skipped"]}}]);
    doc["scene"] = json!(0);
    let roads = json!({"format":"road-lines-v1","origin":origin,"source":"manual GLB export converted by Rust","lines":lines.as_ref().ok_or("도로 누락")?["lines"]});
    Ok((encode(&mut doc, &mut bin)?, roads))
}
#[cfg(test)]
mod tests {
    use super::*;
    /// 겹친 좌표라도 다른 원본 정점을 가진 띠를 합치지 않는다.
    #[test]
    fn coincident_roads_keep_original_topology() {
        let p = vec![
            [0., -0.04, 0.],
            [0., -0.04, -10.],
            [4., -0.04, 0.],
            [4., -0.04, -10.],
            [0., -0.04, 0.],
            [0., -0.04, -10.],
            [4., -0.04, 0.],
            [4., -0.04, -10.],
        ];
        let result = road_lines(&p, &[0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 7]).unwrap();
        assert_eq!(result["lines"].as_array().unwrap().len(), 2);
        assert!(road_lines(&p, &[99, 1, 2, 2, 1, 3]).is_err());
        assert!(road_lines(&p, &[0, 1, 2]).is_err());
    }
    /// 손상된 GLB와 잘못된 길이를 명시적으로 거부한다.
    #[test]
    fn malformed_container_is_rejected() {
        for bytes in [b"".as_slice(), b"glTF", &[0u8; 32]] {
            assert!(decode(bytes).is_err());
        }
    }
    /// 실제 수동 GLB를 기존 Python 결과의 유효 도로와 방향 무관하게 밀리미터 단위로 비교한다.
    #[test]
    #[ignore = "SIMULATOR_MANUAL_GLB 환경 변수의 수동 내보내기 자료가 필요함"]
    fn manual_export_matches_existing_roads() {
        let path = std::env::var("SIMULATOR_MANUAL_GLB").expect("manual GLB path");
        let bytes = std::fs::read(path).unwrap();
        let (output, result) = convert(&bytes, [34.7509700, 127.7493575]).unwrap();
        let (doc, _) = decode(&output).unwrap();
        assert_eq!(
            doc["scenes"][0]["extras"]["source_crs_x"],
            14221057.273994608
        );
        assert_eq!(
            doc["scenes"][0]["extras"]["source_crs_y"],
            4129870.144414883
        );
        let old: Value = serde_json::from_str(include_str!(
            "../../../../../testdata/yeosu-expo/custom-roads.json"
        ))
        .unwrap();
        let good: Vec<_> = old["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| {
                let p = l["points"].as_array().unwrap();
                p.windows(2).any(|v| v[0] != v[1])
            })
            .collect();
        let actual = result["lines"].as_array().unwrap();
        assert_eq!(good.len(), actual.len(), "유효 도로 개수");
        let mut used = BTreeSet::new();
        for expected in good {
            let mut normalized = expected["points"].as_array().unwrap().clone();
            let closed = expected["closed"] == true || normalized.first() == normalized.last();
            if normalized.first() == normalized.last() {
                normalized.pop();
            }
            let points = &normalized;
            let found = actual
                .iter()
                .enumerate()
                .find(|(i, line)| {
                    if used.contains(i)
                        || line["closed"] != closed
                        || (line["width_m"].as_f64().unwrap()
                            - expected["width_m"].as_f64().unwrap())
                        .abs()
                            > 0.003
                    {
                        return false;
                    }
                    let candidates = line["points"].as_array().unwrap();
                    if candidates.len() != points.len() {
                        return false;
                    }
                    let n = points.len();
                    (0..if closed { n } else { 1 }).any(|offset| {
                        [false, true].iter().any(|reverse| {
                            points.iter().enumerate().all(|(j, p)| {
                                let index = if *reverse {
                                    (offset + n - 1 - j) % n
                                } else {
                                    (offset + j) % n
                                };
                                let q = &candidates[index];
                                (p[0].as_f64().unwrap() - q[0].as_f64().unwrap())
                                    .hypot(p[1].as_f64().unwrap() - q[1].as_f64().unwrap())
                                    <= 0.003
                            })
                        })
                    })
                })
                .map(|(i, _)| i);
            assert!(found.is_some(), "일치하는 도로 누락: {expected}");
            used.insert(found.unwrap());
        }
        assert_eq!(used.len(), 133);
        println!("MANUAL GLB MATCH: 133 valid roads; source has 6 zero-length strips excluded");
    }
}
