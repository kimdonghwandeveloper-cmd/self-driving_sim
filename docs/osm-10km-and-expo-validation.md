# OSM 10 km 경로와 엑스포 내부 통로 검증

작업은 `feature/osm-10km-and-internal-roads`에 보존한다. main 병합 및 배포 승인과 사용자의 수동 검수는 별도다.

## 지원 범위

- 신규 OSM 경로 길이는 최대 10,000 m이며, 연석·곁가지 포함 코스 범위는 10,100 m다. 오프라인 팩 조회는 최대 12,000 m 범위의 조각을 합친다. 온라인 Overpass 요청은 기존 2 km 제한을 유지한다.
- 초기 지역은 1.5 km다. 출발을 찍은 뒤 지도를 이동하거나 다른 지역을 검색해 도착을 지정하면, 반대편 지점을 포함하는 범위를 다시 읽는다. 좌표·장소 이름·접근 옵션은 유지하고 실패하면 이전 모델로 돌아간다.
- `제한 도로 포함 · 연습용`은 `motor_vehicle`/`access` 제한 도로도 포함한다. 일방통행, 회전 제한, 허용 도로 종류, 차량 운동 모델은 유지한다. 실제 출입 허가나 차단기 개방을 뜻하지 않는다.
- 옵션은 `route-v1`의 선택 필드 `include_restricted`에 기록한다. 없는 기존 파일은 false다. 기존 저장 경로의 `max_route_length_m = 2000`도 그대로 재생한다.
- 이 문서는 당시 OSM 확대의 검증 기록이다. 당시 별도 HD 지도 코스 범위는 1.5 km였으나 해당 기능은 2026-11-18 사용자 결정으로 제거되었다. 교차로 형상 생성에 실패하는 모든 실제 도로가 이번 변경으로 해결되는 것은 아니다.

## 자료 출처와 갱신

원본은 [Geofabrik 한국 OSM 2026-11-11 PBF](https://download.geofabrik.de/asia/south-korea-261111.osm.pbf)이며 OSM/ODbL 표기를 유지한다. 다른 상용 지도에서 좌표를 추출하거나 내부 통로를 임의로 그리지 않았다.

- 원본 크기: 291,616,129 bytes.
- 원본 SHA-256: `ff3396a70e5b87a29de31d33bf366e7c6257f1c558cc282e6389f54a0c215243`.
- 이전 팩과 같은 원본이므로 변경 효과를 자료 시각 차이 없이 비교할 수 있다.

| 항목 | 이전 팩 | 재생성 팩 |
|---|---:|---:|
| 형식 | korea-data-v1 | korea-data-v2 |
| 도로 way | 1,117,848 | 1,226,142 |
| 회전 제한 | 22,764 | 23,395 |
| 장소 | 1,821,430 | 1,821,430 |
| 건물 | 1,379,296 | 1,379,296 |

새 도로 팩 SHA-256: `8f1ada330ab363485210cdcf402a72f016b43f3d7d710ca42ffb3c0743030f37`.
새 장소 팩 SHA-256: `a309ccc0cfe86eba0b85b21506c0021f99c7fae3df76a7ebab56759c32251a93`.

이 PC에서는 develop과 test의 `data/korea/`를 새 팩으로 교체했고, 이전 팩은 각 워크트리의 `cache/datasets/korea-before-internal-bebc3f9/`에 보존했다. main의 자료·소스에는 적용하지 않았다. 실행 중인 Godot는 자료를 메모리에 보관하므로 재실행해야 새 판본을 읽는다. 원본과 재생성 결과는 Git에 넣지 않는다.

재생성은 비어 있는 출력 폴더를 지정한다.

```powershell
sim-tools.exe --build-korea-data cache/source/south-korea-261111.osm.pbf --out cache/datasets/korea-regenerated-new
```

검증한 새 팩을 다른 워크트리로 복사할 때는 `scripts/prepare-korea-data.ps1 -SourceData <새 팩> -DestinationRepo <워크트리>`를 쓴다. 이 스크립트는 다른 판본이 이미 있으면 덮어쓰지 않으므로 먼저 기존 팩을 별도 폴더에 보존해야 한다. 새 팩 판본을 가리키는 캐시는 형식·도로 SHA·장소 SHA를 묶어 분리하고, 예전 경로가 가리키는 캐시는 유지한다.

## 자동 검증 항목

- Rust: 약 9.98 km 제한 도로 생성·TOML 저장·재생 동등성·실제 운동/충돌 모델 자동 주행, 옵션 해제 시 거부, 일방통행 역방향 거부, 상한 초과 거부.
- Rust: 여러 조각에 걸친 약 10.8 km 조회에서 way 중복 제거·회전 제한·건물 동반 파일·캐시 결정성. 온라인 2 km 상한과 오프라인 12 km 상한을 각각 검사.
- Godot `long_route_test.gd`: 약 10 km 코스 로드·옵션 저장/재생 안내·옵션 해제·방향 제한·8 km 지역 확장·실패 시 상태 보존.
- Godot `expo_internal_roads_test.gd --full-data`: 엑스포 주변 원본 스냅샷에서 이전 제외 대상 통로를 확인하고 시설 범위(위도 34.747~34.754, 경도 127.744~127.751) 안의 표본을 고른다. 이 범위는 조사·통행 허가가 검증된 시설 경계가 아닌 회귀 시험용 위치 필터다.
- 엑스포 표본: way `532526365`, `highway=service`, `service=parking_aisle`. 약 61.8 m 구간을 선택·저장·재생하며 차량 주행으로 검증한다. 주변 모든 통로의 연결·주행을 보장하는 시험은 아니다.
- 이전 팩 비교는 `--previous-data=cache/datasets/korea-before-internal-bebc3f9`를 더해 실행한다. 위치 필터 안의 26개 서비스 통로가 이전 팩에 없었는지 검사한다.

## 수동 확인

1. 새 DLL을 빌드한 테스트 프로젝트를 F5로 실행하고 OSM 지도로 들어간다.
2. 엑스포 지역에서 복구된 주차장 통로와 출발·도착 선택을 확인한다.
3. 제한 도로 포함 옵션을 켜고 끌 때 선택 가능 표시와 경로·안내가 함께 바뀌는지 확인한다.
4. 출발을 고른 뒤 다른 지역으로 이동해 도착을 지정한다. 경로 길이 표시와 주행, 저장 경로 재진입을 확인한다.
5. 실제 조작감·다양한 교차로·장거리 렌더링 성능은 사용자가 별도로 검수한다. 자료가 없는 길은 지역 보정 대상이며 자동으로 만들지 않는다.

## 이번 검증 결과

코드 기준 `503664d`에서 fmt·clippy(경고 0)·Rust 전체 시험 413개와 Godot 헤드리스 시험 24종이 통과했다. 새 전국 팩의 8개 도시 표본도 통과했다. OpenGL Compatibility 렌더링 환경에서 지도 클릭·저장·주행 시험도 통과했다. 실제 화면 캡처는 테스트 워크트리의 `cache/review-10km/osm-route-selected.png`에 있다. 다양한 실제 10 km 곡선·교차로에 대한 수동 검수나 성능 보장은 이 결과에 포함하지 않는다.

엑스포 표본 약 61.8 m는 실제 Godot 차량을 3 m/s 목표 속도로 목표 5 m 이내까지 주행했고 연석·벽 접촉이 없었다. 약 9.98 km 합성 코스는 Rust 운동 모델로 끝 부근까지 주행했으며 충돌이 없었다. 제한 옵션과 방향 규칙·저장·재생은 별도로 검사했다.

테스트 워크트리의 DLL은 갱신되어 바로 확인할 수 있다. develop 폴더의 DLL 빌드는 실행 중인 Godot의 파일 잠금으로 실패했으며, 실행 중인 사용자 앱은 종료하지 않았다. develop에서 확인하려면 그 폴더를 사용하는 Godot를 닫은 뒤 `cargo.exe build --locked -p sim-godot`로 DLL을 갱신해야 한다. develop 브랜치 자체는 병합 전 상태로 유지한다.

## 수정 파일·라인

아래는 develop 기준 변경의 시작 라인이다. 같은 파일의 여러 변경은 줄 번호를 함께 적었다.

| 파일 | 변경 라인 |
|---|---|
| `PLAN.md` | 20, 420, 661 |
| `README.md` | 187, 191, 310, 313, 315, 481, 485 |
| `config/road_graph.toml` | 19 |
| `crates/sim-core/src/picker/model.rs` | 90, 132, 314 |
| `crates/sim-core/src/picker/region.rs` | 35 |
| `crates/sim-core/src/world/korea_data.rs` | 816, 887, 1544 |
| 당시 별도 HD 코스 모듈 (현재 삭제됨) | 당시 15, 22, 437 |
| `crates/sim-core/src/world/map_fetch.rs` | 87, 116, 118 |
| `crates/sim-core/src/world/places.rs` | 436 |
| `crates/sim-core/src/world/route.rs` | 37 |
| `crates/sim-core/src/world/route_plan.rs` | 52, 81, 111, 115, 206, 320, 337, 358, 368, 491, 497, 791, 799, 840, 926, 1020, 1028, 1056, 1275, 1469, 1473, 1526, 1550, 1588 |
| `crates/sim-core/src/world/test_drive.rs` | 57 |
| `crates/sim-godot/src/korean.rs` | 40 |
| `crates/sim-godot/src/route_picker.rs` | 89, 153, 238, 651 |
| `docs/design/godot-interface.md` | 219, 361 |
| `docs/map-data.md` | 177, 198, 207, 212, 413 |
| `docs/osm-10km-and-expo-validation.md` | 1 |
| `godot/presentation/map_screen.gd` | 20, 322, 415, 444, 446, 450, 481, 502, 589, 604, 688, 691 |
| `godot/tests/expo_internal_roads_test.gd` | 1 |
| `godot/tests/long_route_test.gd` | 1 |
| `godot/tests/osm_route_drive_test.gd` | 61 |
| `godot/tests/place_pick_test.gd` | 104 |
| `godot/tests/region_open_test.gd` | 66 |
