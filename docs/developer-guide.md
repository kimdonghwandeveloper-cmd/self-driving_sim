# 개발·지도 자료·시험 안내

[README로 돌아가기](../README.md) · [개발 환경 기록](development.md)

명령은 Windows PowerShell에서 작업 워크트리 최상위를 기준으로 실행합니다.

## 준비

- Rustup: `rust-toolchain.toml`이 Rust **1.99.1**, rustfmt, Clippy를 선택합니다.
- Windows: Visual Studio Build Tools의 **Desktop development with C++** 및 Windows SDK가 필요합니다(루트 `.vsconfig`).
- Godot **4.7.2**(표준판, .NET 아님). 콘솔 실행 파일로 헤드리스 시험을 돌립니다.
- 그래픽 드라이버와 데스크톱 세션이 필요합니다.
- RustRover에서 루트 `Cargo.toml`(작업 공간)을 Cargo 프로젝트로 연결합니다.

```powershell
rustup show
cargo fetch --locked
cargo build --locked -p sim-godot
```

그다음 Godot 에디터로 `godot/project.godot`을 열고 실행(F5)합니다.
`godot/sim_godot.gdextension`은 **같은 워크트리의** `target/debug/sim_godot.dll`을 읽습니다. 워크트리마다 `target/`이 따로이므로,
Godot으로 열 워크트리에서 먼저 위 빌드를 해야 합니다. 없으면 `GDExtension dynamic library not found`가 나고 조작·지도 화면이 멈춥니다.
Rust 코드를 바꾼 뒤에는 다시 빌드하고 에디터를 다시 시작합니다(에디터가 열려 있으면 DLL이 잠겨 빌드가 실패할 수 있습니다).

### WSL에서 작업하는 경우

이 저장소의 기본 실행 대상은 Windows MSVC입니다. WSL 터미널에서 Windows에
설치된 Rust를 사용할 때는 `rustup.exe`, `cargo.exe`, `rustc.exe`처럼 확장자를 붙입니다.
WSL Linux용 Rust와 그래픽 환경은 별도로 설치·검증하지 않았습니다.

## 개발 명령

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

`cargo test`는 작업 공간의 세 crate(`sim-core`, `sim-godot`, `sim-tools`) 시험을 모두 돌립니다. 화면 동작은 아래 Godot 시험으로 확인합니다.
`ureq`(=3.5.1)는 지도 받기·타일용 HTTP 클라이언트입니다. 기본 기능(rustls + ring 암호, webpki 루트 인증서, gzip)을
쓰며, cmake·NASM이 필요한 aws-lc-rs는 쓰지 않습니다.

### 전국 지도 자료 만들기 (PLAN 4-11)

지도 화면은 `data/korea/`의 전국 지도 자료에서 지역을 엽니다. 이 폴더는 저장소에 넣지 않으며(약 116 MB), **배포할 때 실행 파일과
함께 넣습니다**(실행 폴더 기준 `data/korea/`). 만드는 방법(저장소 최상위, 네트워크는 원본 받기에만 씀):

1. Geofabrik에서 대한민국 원본을 받습니다: <https://download.geofabrik.de/asia/south-korea-latest.osm.pbf> (2026-11-11자 약 278 MB)
2. 변환합니다(이 PC에서 약 36초, 최대 메모리 약 2 GB). 기존 `data/korea/`는 다 만든 뒤에 바뀝니다.

```powershell
cargo run --release --locked -p sim-tools -- --build-korea-data <south-korea-latest.osm.pbf>
cargo run --release --locked -p sim-tools -- --korea-region 37.4979,127.0276   # 확인: 강남역 주변 지역을 꺼내 개수·시간 출력
```

자료를 새로 만들면 지역 스냅샷은 새 판 폴더(`cache/regions/<판>/`)에 쌓이고, 이전에 저장한 경로는 이전 스냅샷으로 그대로 재생됩니다.
형식과 넣는 것·빼는 것은 [지도 자료 안내](map-data.md)의 "4-11 전국 지도 자료"를 참고하세요.

지역 보정(4-11a) 접속점을 조사할 때는 확인 좌표 주변 도로의 OSM ID를 뽑습니다(파일을 쓰지 않음).
점은 `;`로 여러 개 줄 수 있고(모두 점들의 가운데 1.5 km 지역 안), 반경 기본값은 30 m입니다.

```powershell
cargo run --release --locked -p sim-tools -- --osm-anchors "34.7535,127.7480;34.7505,127.7455" --radius 50
```

맨 위에 기준 자료(원본 PBF SHA-256·자료 시각·도로 묶음 SHA-256)를, 점마다 가까운 way 순으로
`osm_way`·`between_nodes`(가장 가까운 선분), 가장 가까운 `osm_node`와 거리, 막힌 끝 여부, 허용 도로 묶음 번호를 출력합니다.
두 점의 묶음 번호가 다르면 그 지역 안에서는 보정 전 서로 이어지지 않습니다. 결과는 조사표(`docs/map-patches/survey-template.md`) 2절에 옮깁니다.

### 지도 받기 (Phase 4-4, 개발용)

사용자가 요청할 때만 Overpass API에서 bbox(남,서,북,동) 안의 도로를 받아 `cache/maps/`에 저장합니다.
`cache/`는 저장소에서 제외합니다. 창은 열리지 않습니다. 대한민국 범위 밖이거나 한 변이 2 km를 넘거나
응답이 20 MB를 넘으면 오류로 끝납니다. `--offline`을 주면 네트워크를 쓰지 않고 캐시만 확인합니다.
`sim-tools`의 상대 경로(`cache/`, `config/`)는 실행한 폴더 기준이므로 저장소 최상위에서 실행합니다.

```powershell
cargo run --locked -p sim-tools -- --fetch-map 34.7480,127.7460,34.7590,127.7600
cargo run --locked -p sim-tools -- --fetch-map 34.7480,127.7460,34.7590,127.7600 --offline
```

### Geofabrik 지역 추출 (Phase 4-5G G-1, 개발용)

로컬에 있는 Geofabrik `.osm.pbf`에서 bbox(남,서,북,동) 주변 도로만 잘라 `cache/extracts/`에 저장합니다.
네트워크를 쓰지 않고 창도 열리지 않습니다. 전국 파일이면 허용 도로 전체를 메모리에 올리므로 시간이 걸립니다.
결과 세대 폴더는 지도 화면의 "스냅샷 폴더 열기"로 열 수 있고, 경계에서 연결이 끊긴 도로를 지나는 경로는 거부됩니다.
자세한 규칙은 [지도 자료 안내](map-data.md)의 "Geofabrik 지역 추출"을 참고하세요.

```powershell
cargo run --locked -p sim-tools -- --extract-pbf <south-korea.osm.pbf> --bbox 34.7480,127.7460,34.7590,127.7600
cargo run --locked -p sim-tools -- --audit-extract <세대 폴더> --pbf <south-korea.osm.pbf>
cargo run --locked -p sim-tools -- --compare-overpass testdata/yeosu-expo/graph-2026-11-06.osm.json --extract <세대 폴더> --pbf <south-korea.osm.pbf>
```

시나리오 결과 파일(Phase 5-5)은 파일 안의 시나리오 내용과 입력 열을 다시 돌려 같은 결과·기록이 나오는지 확인할 수 있습니다
(같은 `config/vehicle.toml`이 필요합니다). 같으면 `replay matches`, 다르면 처음 다른 기록을 출력하고 오류로 끝납니다.

```powershell
cargo run --locked -p sim-tools -- --check-result logs/scenarios/<세션 ID>.json
```

## 다른 워크트리의 전국 자료 재사용

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-korea-data.ps1 -SourceData ../self-driving_sim-develop/data/korea
```

원본 자료의 SHA-256을 확인해 복사하며 기존 자료는 덮어쓰지 않습니다.

## 3D 건물 자료와 저장 경로 재생

전국 자료는 태그 없는 `korea-data-v1`과 건물 높이·층수 태그를 보존하는 `korea-data-v2`를 읽습니다.
자료 준비 스크립트도 두 형식의 파일 해시를 검사합니다. 기존 v1 자료에 없던 태그는 자동 복원되지 않으므로
높이 태그가 필요하면 원본 PBF에서 v2 자료를 다시 만들어야 합니다.

지역을 열면 도로 스냅샷 `이름.osm.json`과 같은 판본 폴더에 `이름.places.osm.json`을 저장합니다.
저장 경로는 자신이 가리키는 도로 스냅샷의 동반 파일만 읽으며, 다른 지역·CWD·디렉터리 파일 목록에서 건물을 고르지 않습니다.
미리보기에도 같은 스냅샷 경로를 전달합니다. 동반 파일이 없는 옛 스냅샷은 건물 없이 재생하고, 손상된 동반 파일은 오류로 처리합니다.
고정 OSM 지도는 코스 폴더의 `places.osm.json`(없으면 기존 `places-synthetic.osm.json`)을 사용합니다.

캐시 판본은 **자료 형식 + 도로 SHA-256 + 장소 SHA-256**을 함께 반영합니다. 건물 자료만 바뀌어도 새 폴더를 만들고
옛 경로가 참조하는 스냅샷은 보존합니다. 같은 판본의 기존 동반 파일이 예상 내용과 다르면 덮어쓰지 않고 오류를 반환합니다.

## Godot 시험과 첫 import 검사


메뉴·주행·지도·저장/재생을 검사합니다. 블록 경로의 실제 지역 시험은 아래 명령을 함께 실행합니다.

### 블록 경로 회귀 시험

```powershell
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/block_map_ui_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/block_picker_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/block_real_test.gd
```

실제 지역 고정 자료·시험 범위와 한계는 [짧은 블록·교차로 검토 기록](short-block-junctions-review.md)을 참고하세요.

### 기존 코스 및 화면 시험

Windows에서 확장을 빌드한 뒤 다음 명령으로 초기화하고 시험합니다(워크트리 최상위).

```powershell
cargo.exe build --locked -p sim-godot
& '<Godot 콘솔 실행 파일>' --headless --path godot --import --frame-delay 1000
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/smoke_test.gd
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/presentation_smoke.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/drive_replay_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/map_course_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/menu_flow_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/ui_interaction_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/compact_hud_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/osm_route_drive_test.gd
# 전국 자료 준비 후 8개 도시의 실제 자료에서 짧은 경로 생성·저장·차량 이동까지 확인:
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/osm_route_drive_test.gd -- --full-data
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/route_save_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/place_pick_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/region_open_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/goal_reset_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/calibration_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/scenario_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/suspension_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/map_patch_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/yeosu_custom_roads_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/scenario_record_test.gd
```

- `presentation_smoke.gd`는 정지 미리보기 장면·네 시점·지도 화면·메뉴 재진입을 검사합니다. `-- --capture-dir=<폴더>`를 주고 창 모드로 돌리면
  시점별 화면을 PNG로 저장합니다(Bevy 판 `--capture-views` 대신).
- `drive_replay_test.gd`는 정해진 입력 열을 `SimVehicle`의 실제 물리 틱으로 재생한 위치·속도·기어가
  Rust 단독 재생(`SimReplay.run`)과 같은지 비교합니다(GD-2). 같은 열의 기대 표본은 `sim-core` 시험에도 고정되어 있습니다.
- `map_course_test.gd`는 시험 자료의 여수 경로 코스(`map:testdata/yeosu-expo/route-junctions.toml`)를 만들고 몇 초 주행하며, 메뉴 목록에는 없는지 봅니다(GD-3, 4-11).
- `menu_flow_test.gd`는 메인 메뉴 → 코스 선택 → Rust 차량 주행 → Esc 메뉴 복귀를(시험 경로를 저장 경로 자리에 잠시 복사), `route_save_test.gd`는
  오프라인으로 경로를 찍어 저장하고 그 저장 경로로 차량이 준비되는지 확인합니다(GD-4, 시험 파일은 끝에 지움).
- `place_pick_test.gd`는 지도 화면에서 여수 스냅샷과 합성 장소 자료를 열어 건물 윤곽 표시, 이름 검색으로 출발 건물, 건물 클릭으로
  도착 건물을 고르고 경로가 계산되며 출발·도착 이름이 안내에 나오는지 확인합니다(4-9, 네트워크 없음).
- `region_open_test.gd`는 작은 전국 지도 자료 조각(`testdata/korea-yeosu`)으로 클릭한 곳 지역 열기, 도로·건물 표시, 경로 계산·저장(지역 스냅샷을
  가리킴)·다시 코스 만들기, 점이 있을 때 지역 밖 클릭 안내, 멈춘 화면 자동 열기, 자료 없음 안내를 확인합니다(4-11).
- `goal_reset_test.gd`는 지도 코스의 경로 끝 목표 표시, 주행 후 리셋 시 경로 시작 자세 복원, 없는 지도 파일을 열 때
  개발 코스(training) 대체를 확인합니다(4-6). 출력의 `ERROR: ... 없는-코스.toml` 한 줄은 의도된 실패 경로입니다.
- `calibration_test.gd`는 보정 코스의 측정 상자, 가속·제동 시험 1회차 결과, 리셋 후 결과 유지와 training에서의 상자 숨김을 확인합니다(2-5).
- `scenario_test.gd`는 사용자 메뉴에서 제외된 시험 전용 예시(`testdata/scenarios/sample-stop-line.toml`)를 직접 열어 시작 전 정지, F5 시작, 단계 4개·메시지 2개, F6 일시정지(차량·시간 멈춤), 성공,
  F7 재시작, F8 종료와 없는 시나리오 파일의 실패 화면을 확인합니다(5-1). 출력의 `ERROR: ... 없는-시나리오.toml` 한 줄은 의도된 실패 경로입니다.
- `suspension_test.gd`는 보정 코스에서 제동 때 앞 숙임, 오른쪽 회전 때 왼쪽(바깥) 기울기가 차체 메시에 전달되고, 바퀴 부품은 기울지 않으며,
  입석 카메라는 `body_follow`(0.5)만큼만 따라가고 외부 시점은 따라가지 않는지 확인합니다(2-8).
- `map_patch_test.gd`는 합성 지역 보정으로 끊긴 OSM 도로를 연결해 경로 선택·안내·저장·재생·차량 통과를 확인하고,
  보정 파일 누락·변조 오류와 다른 코스에서 보정 안내가 사라지는지도 검사합니다(4-11a, 네트워크 없음).
- `yeosu_custom_roads_test.gd`는 메뉴에서 "여수 엑스포 · 내 맵 도로"(사용자 제작 맵의 도로 중심선 코스)를 열어 도로 면·연석·배경,
  도로 이탈 판정과 몇 초 주행을 확인합니다(4-11b).
- `scenario_record_test.gd`는 메뉴의 시나리오 A·B·C, B의 원뿔 두 개(충돌 경계), C의 보행자 대체 객체 등장 자리·이동·표시 노드,
  제동 반응 기록, F8로 끝낸 뒤 결과 JSON(사건 기록·입력 열·임시 점수 안내)과 끝 안내의 기록 요약, F7 뒤 요약 비움을 확인합니다(5-2~5-5).
  결과 파일은 `user://scenario-record-test`에 쓰고 끝에 지웁니다.

프레임 시간은 실제 창에서 잽니다(헤드리스 아님, 측정만 하고 실패로 끝내지 않음).
지도 화면은 `godot/tests/map_frame_probe.gd`로 잽니다(전국 지도 자료 필요, 기본 강남역 1.5 km 지역을 열고 가만히·끌기·확대 프레임 시간 출력):
`& '<Godot 콘솔 실행 파일>' --path godot --script res://tests/map_frame_probe.gd -- --data=<전국 자료 폴더 절대 경로>`

```powershell
& '<Godot 콘솔 실행 파일>' --path godot --resolution 1280x720 --script res://tests/frame_time_probe.gd -- --seconds=15
```

Godot 4.7.2와 godot-rust 0.5.5 조합에서는 캐시 없는 기본 `--import`가 종료 시 접근 위반으로 실패했습니다.
`--frame-delay 1000`으로 첫 import의 정상 종료를 확인했으며, 엔진 자체를 수정한 것은 아닙니다.
Godot [이슈 #111048](https://github.com/godotengine/godot/issues/111048)에 문서 생성과 종료 정리의 경합 및 프레임 지연 우회가 보고되어 있습니다. 로컬 크래시의 심볼 스택은 아직 확보하지 않아 같은 원인으로 확정하지 않습니다.

회귀 검사는 매번 캐시가 없는 별도 복사본으로 첫 import와 Rust 함수 호출을 검사합니다.
기본 3회이며 첫 실패에서 중단합니다. 충돌 후 재실행으로 성공 처리하지 않습니다.

```powershell
powershell.exe -NoProfile -File tools/test-godot-cold-import.ps1 -Godot '<Godot 콘솔 실행 파일>' -ScratchRoot 'D:\CodexBuilds\self-driving-sim\scratchpad'
```

`-FrameDelay 0 -Runs 1`은 우회 없는 재현 비교용입니다. 이 조합에서 실패하는 것은 알려진 제약입니다.
검사 복사본과 로그는 출력된 경로에 남습니다. 실제 에디터 조작 및 일반 GUI 첫 실행은 별도 확인이 필요합니다.

`Cargo.lock`은 커밋하여 의존성 해석을 고정합니다. `logs/`와 `target/`은 버전 관리에서 제외합니다.

## Rust 지도 제작 도구

Blender 수동 GLB를 입력으로 사용하는 좌표 변환·도로 추출·이미지 지면·캐노피 생성은 [Rust 지도 제작 안내](rust-map-tools.md)를 참고하세요. Python 설치는 필요하지 않습니다.
