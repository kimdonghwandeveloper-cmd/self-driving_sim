# Rust 지도 제작 도구

Blender UI에서 원본을 GLB로 내보낸 뒤 Rust 도구로 가공한다. `.blend` 직접 파싱·자동 저장은 하지 않는다. 기존 지도/주행 에셋과 미커밋 원본은 덮어쓰지 않는다. GDScript·PowerShell의 Rust 전환은 이 지도 제작 도구 대체와 별도 범위다.

## Blender 수동 내보내기

- 원본 `여수맵/2104_autosave.blend`를 열고 glTF 2.0 → GLB를 선택한다.
- Custom Properties를 켜고 애니메이션·압축은 끈다. 원본 메시·UV·인덱스 순서를 보존하며 추가 모디파이어나 메시 최적화를 적용하지 않는다.
- 장면의 `SRID=EPSG:3857`, `crs x`, `crs y` 속성을 GLB에 포함한다.
- `Ways`, `Areas`, `EXPORT_OSM_MAPNIK_WM`을 포함한다. 표시용 scale/translation은 Rust가 적용하지 않고 메시 원본 위치를 변환한다. 회전·계층·압축·외부 버퍼와 도로 다중 프리미티브는 오류로 거부한다.

이번 실제 내보내기:

- GLB: `D:/CodexBuilds/self-driving-sim/rust-map-preview/2104_autosave.glb`
- SHA-256: `3dc216ab2feaa50810a16d1845a529879fe83951cc3529754e6f13c985991299`
- `crs x=14221057.273994608`, `crs y=4129870.144414883`. Blender 숫자 필드 표시값은 반올림되므로 GLB extras의 값을 사용한다.
- 원본 `.blend`는 저장·변경하지 않았다.

## 원본 GLB 정합 및 도로 추출

```powershell
cargo.exe run --locked -p sim-tools --bin blender-map -- <manual-export.glb> <new-output-directory>
```

새 출력 폴더에 `background.glb`, `custom-roads.json`을 생성한다. 원점은 기존 여수 코스의 `[34.7509700, 127.7493575]`이며 다른 원점 도구로 일반화한 것은 아니다. 배포 번들에서는 `bin/blender-map.exe`를 같은 두 인자로 실행할 수 있다.

- 내장 단일 GLB 2.0 버퍼를 검사하고 EPSG:3857 → 위경도 → 지역 미터(x 동쪽, z 남쪽)로 변환한다.
- 높이 오프셋은 도로 -0.04m, 구역 -0.06m, 지도 -0.08m로 유지한다.
- 연속 두 삼각형으로 원래 사각형 띠를 복원하며 좌표가 같다는 이유로 다른 도로 정점을 합치지 않는다.
- 수평 지면 조건을 통과한 띠만 추출한다. 제외한 면 수는 GLB 장면 extras의 `excluded_non_ground_or_degenerate_quads`로 기록한다.
- GLB UV 이음새의 양 끝 단면 중심·폭이 1.5mm 이내면 폐곡선으로 정규화하고 중복 끝점을 제거한다. 이는 원본 정점 ID 복원이 아니라 기하학적 폐곡선 정규화다.
- 실제 자료의 유효 도로 133개를 기존 결과와 3mm 이내로 비교했다. 이전 자료의 길이 0인 띠 6개는 제외한다. 기존 파일을 수정하거나 런타임 도로 정책을 바꾼 것은 아니다.

## 컨벤션 통로·이미지·캐노피

```powershell
cargo.exe run --locked -p sim-tools --bin convention-map -- <trace.json> <new-output-directory> <reference.png> <aligned-background.glb>
```

- 여섯 코스 TOML, `traced-roads.json`, `convention-background.glb`를 생성한다.
- PNG와 정합 배경은 선택 인자다. PNG를 생략하면 `convention-roads.glb`에 도로만 만든다.
- 두 픽셀 기준점으로 축척·회전·이동을 정합하고, 기존 마이터 제한을 적용한 폭을 가진 도로와 중앙 갤러리 재질을 만든다.
- PNG 내용 영역 크기로 UV 여백을 제외하고 높이 0.005m의 표시 지면에 이미지를 내장한다. 손상되거나 지나치게 큰 PNG는 거부한다.
- 캐노피 가로 빔은 4m 간격·폭 0.28m·높이 7.5m, 두 측면 빔은 폭 0.36m·높이 7.4m로 기존 치수를 유지한다. 주행을 가로막는 기둥이나 충돌체는 추가하지 않는다.
- 정합 배경을 지정하면 재질·이미지·접근자·메시 인덱스를 조정해 기존 배경과 새 레이어를 병합한다. 기존 컨벤션 완성 GLB를 배경으로 넣으면 장식이 중복되므로 `blender-map`의 정합 배경을 사용한다.
- 출력 폴더가 이미 있으면 거부한다. 검증 결과는 새 폴더에서 검토한 뒤 별도 승인으로 에셋에 반영한다.

## 검증

```powershell
cargo.exe test --locked --all-features
$env:SIMULATOR_MANUAL_GLB = '<manual-export.glb>'
cargo.exe test --locked -p sim-tools --bin blender-map manual_export_matches_existing_roads -- --ignored --nocapture
& '<Godot console exe>' --headless --path godot --script res://tests/rust_map_tools_test.gd -- <converted-background.glb> <convention-background.glb>
```

수동 내보내기 비교 시험은 입력이 없는 일반 CI에서는 ignored이며, 이번 실제 파일을 지정하여 별도로 실행한다. Godot 시험은 새 GLB 로드·여섯 통로/캐노피/이미지 평면의 기존 AABB 비교·텍스처 존재·앞면 방향을 검사한다. 전체 픽셀 일치나 새 배경의 사람 시야 검수까지 보장하지 않는다.

지도와 원본 이미지의 이용 조건은 유지한다. © OpenStreetMap contributors · ODbL. 원본 PNG·수동 GLB는 사용자 로컬 자료이며 새로 Git에 포함하지 않는다.