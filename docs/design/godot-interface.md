# Godot ↔ Rust 인터페이스 계약 (초안 v0.1)

작성: 2026-11-08, Claude 초안. **Codex 확인 전 초안**이며, 확인 후 v1로 고정한다. 구현은 v1 고정 후 시작한다.
목적: Claude(Rust, `crates/`)와 Codex(Godot, `godot/`)가 동시에 개발할 때 서로 기다리지 않도록
Godot에 노출할 클래스·메서드·시그널·좌표 규칙·실패 처리를 먼저 정한다.

| 판 | 변경 |
| --- | --- |
| v0 | 첫 초안(차량·코스·재생 시험) |
| v0.1 | 사용자 지시 반영: 시나리오 A·여수 경로 제외, 경로 선택·지도 화면 추가(7절). 재생 시험의 틱 안 적용 순서(5절), 코스·설정 실패 처리(6절) 명확화 |

변경 규칙:

- 이 문서를 먼저 고치고 커밋한 뒤 코드를 바꾼다. 문서와 코드가 다르면 문서가 기준이다.
- 이름·인자·반환형·순서 규칙을 바꾸는 변경은 상대 에이전트 확인 후에만 한다. 항목 추가는 자유롭게 하되 문서에 적는다.
- 아직 구현되지 않은 항목은 "구현: 미"로 표시한다. Godot 쪽은 `ClassDB.class_exists()`로 확인하고 없으면 임시값을 쓴다.

## 1. 공통 규칙

| 항목 | 규칙 |
| --- | --- |
| 단위 | 길이 m, 속도 m/s(표시용 km/h 별도 제공), 각도: 함수 이름에 `_deg`/`_rad`를 붙여 구분 |
| 주행 좌표 | Rust 지면 (x, z) → Godot `Vector3(x, 0, z)`. 높이는 Godot에서 정한다(경사 미지원) |
| 방향 | heading 0이면 전방이 -Z. Godot `rotation.y = heading_rad`와 같다(Node3D 기본 전방 -Z와 일치) |
| 조향 부호 | 오른쪽이 양수(입력 `steering`, 바퀴 조향각 모두). Rust `Motion::steer_angle_rad`와 같다 |
| 차량 원점 | 후륜축 중심. 차체 중심은 전방으로 1.4 m(`VEHICLE_FOOTPRINT`, 임시값) |
| 고정 간격 | `DT = 1/64 s`(Bevy FixedUpdate 64 Hz와 같음). `project.godot`의 `physics/common/physics_ticks_per_second = 64` |
| 설정 파일 | 저장소 루트 `config/*.toml`을 그대로 읽는다. 경로는 `res://` 기준 `../config` |
| 실패 보고 | 6절 규칙을 따른다. 결과를 돌려주는 함수는 `{ "ok": bool, "error": String, ... }` 형태를 쓴다 |

## 2. `SimVehicle` (Rust, `Node3D` 상속) — 구현: 미 (Claude)

차량 하나. 입력 읽기·상태 머신·운동 모델·충돌을 Rust가 모두 처리하고, 매 물리 틱 끝에 자기 `transform`을 갱신한다.
Godot 쪽은 이 노드의 **자식**으로 차체 메시·실내·카메라를 붙인다(차량 로컬 좌표 = 이 노드 좌표).

### 내보내는 속성 (`#[export]`)

| 이름 | 형 | 기본값 | 설명 |
| --- | --- | --- | --- |
| `course` | `String` | `"training"` | `"training"`, `"calibration"`. `_ready`에서 **한 번만** 읽는다. 바꾸려면 노드를 새로 만든다 |
| `config_dir` | `String` | `""` | 비우면 `res://../config`. 테스트에서 다른 설정 폴더를 쓸 때만 지정 |
| `read_input` | `bool` | `true` | `false`면 Godot 입력을 읽지 않고 `set_manual_input`/`send_command`만 쓴다(헤드리스 시험용) |

### 상태 확인

| 함수 | 반환 | 설명 |
| --- | --- | --- |
| `is_ready()` | `bool` | 코스를 만들고 시작 자세에 놓았으면 `true`. `false`면 6절 "코스 실패" 상태 |
| `last_error()` | `String` | 코스 실패 사유. 정상이면 빈 문자열 |
| `config_warnings()` | `PackedStringArray` | 설정 파일을 읽다가 기본값으로 바꾼 항목의 경고문(화면 표시용). 없으면 빈 배열 |
| `course_data()` | `Dictionary` | 이 차량이 **실제로 충돌 판정에 쓰는** 코스 데이터(3절 형식). Godot 메시는 반드시 이것으로 만든다 |
| `tick_count()` | `int` | `_ready` 뒤 처리한 물리 틱 수(재생 시험용) |

### 읽기 함수 (HUD·카메라용)

| 함수 | 반환 | 설명 |
| --- | --- | --- |
| `speed_mps()` | `float` | 부호 있는 속도(후진 음수) |
| `speed_kmh()` | `float` | 표시용 절댓값 km/h |
| `gear()` | `String` | `"P"`, `"R"`, `"N"`, `"D"` |
| `engine_on()` | `bool` | 시동 상태 |
| `emergency_active()` | `bool` | 비상 정지 중 |
| `steer_angle_deg()` | `float` | 현재 바퀴 조향각(오른쪽 양수) |
| `in_contact()` | `bool` | 장애물·연석과 접촉해 정지한 상태 |
| `view()` | `String` | `"front"`, `"rear"`, `"left"`, `"right"` (입석 시점 방향) |
| `debug_camera()` | `bool` | 외부 디버그 카메라 켜짐 |
| `input_source()` | `String` | `"keyboard"`, `"gamepad"`, `"manual"`, `"none"` (마지막으로 입력이 들어온 곳) |
| `course_notices()` | `PackedStringArray` | 화면에 계속 띄울 안내(임시값·출처 표기) |

### 명령 함수

| 함수 | 설명 |
| --- | --- |
| `send_command(name: String) -> bool` | `"gear_park"`, `"gear_reverse"`, `"gear_neutral"`, `"gear_drive"`, `"engine_toggle"`, `"emergency_stop"`, `"emergency_release"`, `"reset_course"`. **즉시 처리하지 않고 다음 틱 대기열에 넣는다**(5절). 이름이 틀리면 `false`와 `godot_error!`, 맞으면 `true`(받아들임일 뿐 성공 여부는 시그널로 안다) |
| `set_manual_input(throttle, brake, steering)` | `read_input = false`일 때 쓰는 연속 입력(0..1, 0..1, -1..1). 다시 부를 때까지 **값이 유지**된다. 범위 밖·NaN은 Rust `VehicleInput::sanitized` 규칙으로 자른다 |

### 시그널

한 틱 안에서 발생한 시그널은 5절 순서대로 그 틱 끝에 보낸다.

| 시그널 | 인자 | 발생 시점 |
| --- | --- | --- |
| `command_applied` | `command: String` | 대기열 명령이 상태 머신에서 성공 |
| `command_rejected` | `command: String, reason: String` | 상태 머신이 거부(예: 주행 중 P) |
| `gear_changed` | `gear: String` | 기어가 실제로 바뀜 |
| `engine_changed` | `on: bool` | 시동이 실제로 바뀜 |
| `emergency_changed` | `active: bool` | 비상 정지 걸림/해제 |
| `course_reset` | 없음 | 시작 자세 복귀 |
| `collided` | 없음 | 접촉이 시작된 틱에 1회 |
| `view_changed` | `view: String` | 시점 키 입력 |
| `debug_camera_toggled` | `on: bool` | 디버그 카메라 키 입력 |
| `course_failed` | `error: String` | 6절 코스 실패. `_ready` 중 1회(연결이 늦을 수 있으니 `is_ready()`도 확인) |

### 카메라 설정

`operator_view() -> Dictionary`: `config/camera.toml` 값. 읽기 실패 항목은 기본값으로 채우고 `config_warnings()`에 적는다.

```
{ "eye_m": Vector3, "pitch_deg": float, "vertical_fov_deg": float, "near_clip_m": float }
```

네 방향 시점은 같은 눈 위치에서 yaw만 바꾼다: front 0, left +π/2, rear π, right -π/2(+Y축 기준, Bevy 판 `ViewDirection`과 같음).
Godot 카메라 리그가 `view_changed`를 받아 yaw를 바꾼다. 시점 전환은 주행 입력에 영향을 주지 않는다.

## 3. `SimCourse` (Rust, `RefCounted`, 정적 함수) — 구현: 미 (Claude)

`SimCourse.build(name: String) -> Dictionary`: 코스 형상 데이터. 메시 조립·재질은 Godot 쪽에서 한다.
메뉴 미리보기 등 차량 없이 형상만 필요할 때 쓴다. 주행 장면은 `SimVehicle.course_data()`를 쓴다(같은 형식).
충돌 판정은 Rust가 하므로 Godot 쪽에 물리 충돌체를 만들지 않는다.

```
성공: {
  "ok": true, "error": "",
  "name": String,
  "notices": PackedStringArray,
  "start": { "position": Vector3, "heading_rad": float },
  "surfaces":  [ Rect, ... ],                       # 포장 노면(직사각형)
  "strips":    [ { "kind": "road"|"edge_line",
                   "left": PackedVector3Array, "right": PackedVector3Array }, ... ],
  "areas":     [ { "center": Vector3, "boundary": PackedVector3Array }, ... ],  # 교차로 면(반시계, 부채꼴)
  "markings":  [ { "kind": "edge_line"|"stop_line", "rect": Rect }, ... ],
  "colliders": [ { "kind": "curb"|"wall"|"barrier"|"obstacle", "rect": Rect, "height_m": float }, ... ]
}
실패: { "ok": false, "error": String }              # 다른 키는 없다
Rect = { "center": Vector3, "size": Vector2(폭 x, 길이 z), "heading_rad": float }
```

- 모든 `Vector3`의 y는 0이다. 표시 높이는 Godot 쪽에서 정한다. Bevy 판 값은 `src/world/mod.rs`의
  `SURFACE_Y_M`(0.012), `MARKING_Y_M`(0.016), `PAINT_THICKNESS_M`(0.004) 참고.
- `strips`의 `left[i]`와 `right[i]`가 마주 본다. 점 개수가 같다. 빈 배열인 키도 항상 넣는다.

## 4. `SimBridge` (기존, 정적 함수)

`version() -> String`, `kmh_to_mps(kmh: float) -> float`. 연결 확인용으로 유지한다.

## 5. 틱 처리 순서와 재생 시험

### 5-1. 물리 틱 하나의 처리 순서 (`SimVehicle._physics_process`)

`is_ready() == false`면 아무것도 하지 않는다. 아니면 매 틱 다음 순서로 한다.

1. **간격 확인:** Godot이 준 `delta`는 쓰지 않고 항상 `DT = 1/64`로 계산한다.
   `Engine.physics_ticks_per_second != 64`이면 첫 틱에 `godot_warn!` 1회(계산은 그대로 `DT`).
2. **장치 입력 읽기**(`read_input = true`일 때만): 키보드·게임패드 상태를 읽어 이번 틱의 연속 입력과
   "이번 틱에 새로 눌린" 명령을 만든다. 같은 틱에 여러 명령이 눌리면 `config/input.toml` 동작 순서
   (gear_park, gear_reverse, gear_neutral, gear_drive, engine_toggle, emergency_stop, emergency_release, reset_course) 순으로 넣는다.
   시점·디버그 카메라 키는 여기서 처리하고 주행 계산에 영향을 주지 않는다.
3. **명령 처리:** `send_command`로 들어온 대기열 명령을 **들어온 순서대로** 먼저, 그다음 2단계 장치 명령을 처리한다.
   각 명령은 이번 틱 시작 시점의 속도로 상태 머신이 판정한다. `reset_course`는 정지 상태에서만 성공하며,
   성공하면 자세·속도·조향각을 시작값으로 되돌린다. 처리한 명령은 대기열에서 지운다.
4. **연속 입력 확정:** `read_input = true`면 2단계 값, 아니면 마지막 `set_manual_input` 값(처음은 모두 0).
   시동 OFF·P·비상 정지 때의 입력 무시 규칙은 Rust 상태 머신이 그대로 적용한다.
5. **운동·충돌:** 3·4단계 결과로 Rust `step_with_obstacles`를 `DT`만큼 한 번 부른다.
   Bevy 판과 같은 함수·같은 파라미터를 쓴다.
6. **자세 반영:** `position = Vector3(x, 0, z)`, `rotation = Vector3(0, heading_rad, 0)`.
7. **시그널 보내기:** 이번 틱 안에서 생긴 것을 `command_applied`/`command_rejected`(3단계 순서) →
   `gear_changed` → `engine_changed` → `emergency_changed` → `course_reset` → `collided` → `view_changed` → `debug_camera_toggled` 순으로 보낸다.
8. `tick_count`를 1 늘린다.

즉 `send_command`를 부른 직후에는 상태가 바뀌지 않고, **다음 물리 틱의 3단계**에서 바뀐다.

### 5-2. 재생 입력 열 형식

```
[ { "ticks": 64, "command": "engine_toggle" },
  { "ticks": 1,  "command": "gear_drive" },
  { "ticks": 320, "throttle": 0.5, "brake": 0.0, "steering": 0.0 },
  { "ticks": 64, "brake": 1.0 } ]
```

- 항목마다: (a) `command`가 있으면 그 항목의 **첫 틱 3단계**에서 한 번 처리한다. (b) `throttle`/`brake`/`steering` 중
  있는 키만 바꾸고 없는 키는 이전 값을 유지한다(처음은 0). (c) 그 상태로 `ticks`번 틱을 돈다. `ticks`는 1 이상.
- 한 항목에 `command`와 연속 입력이 함께 있으면 둘 다 첫 틱부터 적용된다(3단계 → 4단계 순서).
- 알 수 없는 키, `ticks < 1`, 범위 밖 숫자는 재생 전체를 실패로 본다(`{ "ok": false, "error": ... }`).

### 5-3. 기대값과 비교

- `SimReplay.run(course: String, entries: Array) -> Dictionary` (Rust 정적 함수, 구현: 미 Claude):
  Godot 노드 없이 5-1의 3~5단계를 같은 순서로 돌린 결과. `{ "ok", "error", "samples": [ Sample, ... ] }`.
  `Sample = { "tick": int, "position": Vector3, "heading_rad": float, "speed_mps": float, "gear": String }`, 항목이 끝날 때마다 하나.
- `godot/tests/drive_replay_test.gd` (Codex): `SimVehicle`을 `read_input = false`로 장면에 넣고, 5-2 열을 실제 물리 틱으로 재생해
  같은 형식의 표본을 만든 뒤 `SimReplay.run` 결과와 비교한다. 허용 오차는 위치 1e-4 m, heading 1e-5 rad, 속도 1e-4 m/s로 시작한다
  (같은 Rust 함수를 같은 순서로 부르므로 원칙적으로 같아야 한다. 다르면 틱 수·순서 오류로 본다).
- 명령 처리를 위해 항목 첫 틱 직전(이전 틱의 `_physics_process` 뒤)에 `send_command`를 부른다. 헤드리스 실행은
  `--headless --fixed-fps 64`로 한다. 실패하면 첫 불일치 표본을 출력하고 exit 1.
- Rust 쪽은 같은 열의 기대 표본을 `sim-core` 테스트로 고정해, Bevy 판 운동 모델 회귀도 함께 잡는다.

## 6. 실패 처리

| 상황 | Rust 동작 | Godot 쪽이 할 일 |
| --- | --- | --- |
| 알 수 없는 코스 이름 | `SimCourse.build`: `{ok:false}`. `SimVehicle`: `is_ready()=false`, `last_error()` 설정, `course_failed` 1회, `_physics_process` 정지, 위치는 원점 | 오류 문구를 보여 주고 메뉴로 돌아가는 버튼만 둔다. 메시를 만들지 않는다 |
| 코스 생성 중 형상 오류(경로 코스 등 이후 단계) | 위와 같음. 오류 문구는 Rust `describe_error` 규칙(사람이 읽는 한국어) | 위와 같음 |
| `config/*.toml` 파일 없음·문법 오류·값 범위 오류 | Bevy 판과 같이 해당 파일(입력 매핑은 장치별) 기본값 사용, `godot_warn!`, `config_warnings()`에 추가. **주행은 계속한다** | 주행 화면 구석에 경고 표시(닫을 수 있게) |
| `config_dir` 폴더 없음 | 모든 설정 기본값 + 경고 1개 | 위와 같음 |
| 잘못된 `send_command` 이름 | `false`, `godot_error!`. 대기열에 넣지 않음 | 개발 오류이므로 고친다 |
| 상태 머신 거부 | `command_rejected(command, reason)` | HUD에 잠깐 표시 |
| Rust 확장 미로드(`SimVehicle` 클래스 없음) | — | 장면이 `ClassDB.class_exists`로 확인하고 "cargo build -p sim-godot 후 에디터 재시작" 안내 |
| Rust 쪽 패닉 | gdext가 Godot 오류로 바꾼다. 차량은 그 틱부터 `is_ready()=false`로 두고 `course_failed("내부 오류: ...")` | 코스 실패와 같게 처리 |

## 7. 경로 선택·지도 화면 (초안, Codex 확인 필요) — 구현: 미

사용자 지시(2026-11-08): 시나리오·여수 경로 주행보다 **경로 선택과 지도 화면 개선을 우선**한다.
Bevy 판 `src/picker/`(4-5·4-5b)의 계산·캐시·받기 부분은 Bevy와 거의 무관(`Vec2`만 사용)하므로 `sim-core`로 옮기고,
화면·조작은 Godot에서 새로 만든다. 선행 작업: GD-1c(지도·경로·스냅샷·타일 모듈 sim-core 이동, Claude).

### 7-1. 좌표

- 지도 화면 좌표: Bevy 판 `tiles::to_view`와 같은 웹 메르카토르 미터, 원점 `VIEW_ORIGIN`(36.0, 127.8), **x = 동쪽, y = 남쪽**.
  Godot 2D(`Node2D`, y 아래 방향)와 축 방향이 같아 그대로 `Vector2`로 쓴다. 단위는 m이고 확대·이동은 Godot `Camera2D`가 한다.
- 위경도는 `Vector2(lat, lon)`이 아니라 `PackedFloat64Array [lat, lon]`으로 주고받는다(f32 정밀도 손실 방지).

### 7-2. `SimRoutePicker` (Rust, `RefCounted` 인스턴스)

| 함수 | 반환 | 설명 |
| --- | --- | --- |
| `load_snapshot(path: String)` | `{ok, error}` | 스냅샷(지역 도로) 열기. 실패 시 이전 상태 유지 |
| `cached_regions()` | `Array[Rect2]` | 이미 받은 지역 범위(지도 좌표) |
| `request_region(center_latlon: PackedFloat64Array)` | `{ok, error}` | 화면 가운데 1.5 × 1.5 km 받기 시작(이미 받은 범위면 캐시만). 백그라운드 처리 |
| `poll()` | `Array[Dictionary]` | 진행 이벤트: `{type: "progress"|"region_loaded"|"failed", message: String}`. 매 프레임 호출 |
| `road_segments()` | `PackedVector2Array` | 그릴 도로 선분(짝수 개: from, to 반복), 지도 좌표 |
| `road_usable()` | `PackedByteArray` | 선분별 경로 탐색 가능 여부(1/0) |
| `add_point(view: Vector2)` / `remove_near(view, radius_m)` / `remove_last()` / `clear_points()` | `bool` 또는 없음 | 점 편집. 점이 바뀌면 경로를 다시 계산한다 |
| `points()` | `PackedVector2Array` | 찍은 점(지도 좌표, 시작 → 경유 → 끝) |
| `route()` | `{ok, error, line: PackedVector2Array, length_m: float, warnings: PackedStringArray}` | 마지막 계산 결과. 점 2개 미만이면 `ok=false, error=""` |
| `status_lines()` | `PackedStringArray` | Bevy 판 상태 문구(지역·점 수·경고) |
| `save(name: String)` | `{ok, error, path: String}` | 경로 파일 저장(Bevy 판과 같은 형식, 같은 폴더) |

### 7-3. `SimTiles` (Rust, `RefCounted` 인스턴스) — 배경 지도

- OSM 타일 정책은 Bevy 판 `tiles.rs`와 같다(고유 User-Agent `self-driving-sim-dev/0.1`, 캐시 헤더, 동시 2개, 403·429 일시 중지,
  미리받기 금지, 자동 시험에서 받지 않음). **받기·캐시는 Rust만 하고 Godot `HTTPRequest`로 타일을 받지 않는다.**
- `request_visible(min: Vector2, max: Vector2, scale: float)`: 보이는 타일 요청. `poll() -> Array[{x, y, z, rect: Rect2, png: PackedByteArray}]`로
  받은 타일을 돌려준다. Godot은 `Image.load_png_from_buffer`로 텍스처를 만든다. 실패한 칸은 회색으로 둔다.
- 출처 표기 `© OpenStreetMap contributors`는 Godot 화면 오른쪽 아래에 항상 둔다.
- `offline: bool` 속성이 `true`면 캐시만 쓴다(헤드리스 시험은 항상 `true`).

### 7-4. 화면 흐름 (Godot, Codex)

메인 메뉴 → 지도 화면(전국 보기 → 확대 → "이 지역 도로 받기" → 점 찍기 → 경로·길이·경고 확인 → 저장) → 메뉴.
"선택한 경로로 주행"은 이번 범위에서 제외(여수 경로 주행 제외 지시). 조작은 Bevy 판(4-5 모듈 주석)을 기준으로 하되 개선은 사용자 확인 후 정한다.

## 8. 아직 정하지 않은 것

- **지도 화면 "개선"의 구체 항목**(사용자 확인 필요): 예) 조작감(끌기·휠 확대), 도로·경로 표시 모양, 경고 표시, 지역 받기 진행 표시, 화면 배치.
- 저장한 경로로 주행 시작을 언제 다시 범위에 넣을지.
- 게임패드 축 이름 매핑(Bevy `GamepadAxis` → Godot `JoyAxis`)과 데드존 처리. 초안은 **Rust가 `Input` 싱글턴을 직접 읽어** Bevy 판과 같은 규칙을 쓴다.
- 키 이름 변환표(Bevy `KeyCode` 이름 → Godot `Key`). Claude가 `sim-godot`에 둔다.
- `SimRoutePicker`의 지역 받기(Overpass/Geofabrik 스냅샷) 중 네트워크가 필요한 부분의 실행 조건. 네트워크 받기는 사용자 요청 때만 한다.

### Codex 검토: v1 고정 전에 합의할 항목 (2026-11-08)

아래는 질문과 권장안이며 **확정된 계약 변경이 아니다**. 1~7절의 이름·인자·순서는 그대로 두었다.
시스템 구현에는 1~5번, 지도 구현에는 6~9번을 먼저 합의해야 한다.

1. **틱 경계와 재생 시험의 관측 시점** (5-1·5-3)
   - 시그널은 7단계이고 tick_count 증가는 8단계다. 시그널 콜백에서 읽는 카운트는 직전 틱 값인지 명시해야 한다.
     콜백 안에서 send_command/set_manual_input을 부르면 현재 틱을 바꾸지 않고 다음 틱에만 반영되는가?
   - Godot 시험이 입력을 넣고 표본을 읽는 순서를 노드 등록 순서나 단순 대기에 의존하면 한 틱 차이가 날 수 있다.
     권장: 시험 드라이버와 차량의 처리 순서 또는 틱 완료 관측 방법을 계약에 고정하고,
     정확히 N틱 뒤 카운트와 상태를 함께 읽는다. 이를 위한 통지/API가 필요하면 Claude 확인 후 추가한다.
   - SimReplay의 초기 상태·설정 경로·파라미터가 SimVehicle과 어떻게 같아지는지 정해야 한다.
     기본 config만 비교할지, config_dir을 지정한 재생도 지원할지와 tick의 시작값(0), 누적 표본 번호를 명시한다.

2. **여러 명령의 성공 및 상태 시그널 의미** (2·5-1)
   - 한 틱에 D → R처럼 여러 명령이 성공하면 gear_changed는 중간 값마다 보내는가, 최종 값만 보내는가?
     시작·종료 기어가 같아도 중간 변경이 있었다면 시그널을 보내는가?
   - command_applied/rejected는 대기열 명령에만 해당하는지, 장치 명령과 reset_course에도 해당하는지 정해야 한다.
     권장: 명령 결과는 각 명령마다 순서대로, 상태 시그널은 틱 시작·종료 값 비교로 최종 상태만 통지한다.
     장치 명령도 동일한 결과 규칙을 적용하고, reset의 성공·실패 사유도 명시한다.
   - 실패 상태(is_ready=false)에서 send_command가 true를 반환하면 영원히 처리되지 않을 수 있다.
     초기화 전/실패 후 명령 수락 여부, 중복·상한 초과 대기열 처리도 정해야 한다.

3. **코스 초기화·입력 장치 전환·화면 이탈** (2·5·6)
   - reset_course가 기어·시동·비상 정지·접촉·수동 입력·남은 명령·시점·디버그 카메라·tick_count 중 무엇을 초기화하는지 표가 필요하다.
     같은 틱에 reset 뒤 가속 입력이 있으면 곧바로 이동하는지, reset 틱만 입력을 비우는지도 정한다.
   - read_input은 실행 중 바꿀 수 있는가? 가능하면 이전 수동 입력과 눌림 이력을 어떻게 처리하는가?
   - 키보드와 게임패드 동시 입력 우선순위, 장치 선택, 패드 분리·창 포커스 상실 때 연속 입력과 시동 상태를
     Bevy 판의 어느 규칙에 맞출지 지정해야 한다. HUD의 input_source 변경 기준도 같이 정한다.
   - 메뉴/지도 전환 시 기존 차량의 입력·물리 처리를 언제 정지하고 노드를 제거하는지 정한다.
     권장: 화면 이탈 이후에는 주행 입력·틱·잔여 명령이 다음 주행 세션으로 넘어가지 않게 한다.

4. **잘못된 재생 값과 Rust 패닉의 보장 범위** (5-2·6)
   - ticks의 정수 여부·최대/합계 상한, NaN/Infinity·문자열·잘못된 명령·빈 입력 열의 처리가 빠져 있다.
     권장: 첫 틱 실행 전에 전체 열을 검사하고, 실패 결과에 항목 번호·키·사유를 포함한다.
     상태 머신의 정상 거부는 재생 형식 오류와 구분해, 재생을 계속할지 실패할지 정한다.
   - gdext가 패닉을 Godot 오류로 바꾸는 것만으로 is_ready=false 설정과 course_failed 통지가 보장되는지는
     확인되지 않았다. Rust 진입부에서 복구 가능한 패닉을 처리하는 범위와, 처리 불가능한 경우의 한계를 명시해야 한다.
     Godot 쪽에서 패닉 후에도 정상 상태를 계속 표시하는 계약이 되지 않도록 한다.

5. **코스 형상의 좌표·유효성 계약** (3)
   - areas의 반시계는 (x,z) 평면을 위에서 본 기준인지 명시하고, boundary 끝점 중복 여부와 center가
     부채꼴 삼각분할에 항상 유효한 점인지 보장해야 한다. strips는 최소 점 수와 중복·퇴화 선분 처리도 필요하다.
   - Rect 회전은 Node3D와 같은 +Y yaw인지, size가 전체 폭·길이인지 명시한다.
     권장: 잘못된 형상은 Rust에서 실패시키고 Godot이 임의로 고쳐 충돌 형상과 달라지게 하지 않는다.
   - course_data()가 반환한 컨테이너는 화면 코드가 수정해도 Rust 충돌 데이터에 영향을 주지 않는 스냅샷인지 확인한다.

6. **지도 좌표 정밀도와 배율** (7-1·7-3)
   - 위경도는 f64지만 넓은 전국 범위를 Vector2로 옮긴 뒤 확대하면 정밀도가 부족할 수 있다.
     권장: 전국 보기와 최대 확대에서 오차를 확인하고, 필요하면 화면 중심 기준 좌표로 변환하는 경계를 합의한다.
     임의로 API 형식을 바꾸지는 않는다.
   - request_visible의 scale이 화면 px/지도 m인지 그 역수인지, UI 배율·창 크기와 어떤 관계인지 명시한다.
     min/max 축 순서, 허용 확대 범위, 타일 줌 선택 기준과 rect의 좌표계도 고정한다.

7. **지역 요청의 수명·취소·결과 교체** (7-2)
   - 받는 중 재요청/스냅샷 열기/화면 이탈 시 이전 작업을 취소하는지, 뒤늦은 결과를 무시하는지 정해야 한다.
     권장: 요청을 식별할 수 있게 하고 이전 요청 결과가 최신 화면 상태를 덮어쓰지 않게 한다.
     취소 방식과 이벤트의 요청 식별 필드는 Claude와 합의 후 계약에 추가한다.
   - 실패 시 기존 지역·점·경로가 보존되는지, 성공 시 지역 교체/합치기 및 점·경로 초기화 여부를 지정한다.
     캐시 적중의 범위 판정과 cached_regions가 디스크 캐시인지 현재 로드된 지역인지도 정한다.
   - 네트워크 요청 허용은 지역 받기 버튼으로 한정한다. 지도 표시/점 편집/저장으로 지역 자료를 자동으로 받지 않는다.
     Overpass/Geofabrik 선택 방식, 오프라인 모드와 오류·재시도 상태를 화면에서 어떻게 알 수 있는지 정한다.

8. **점 편집·경로·저장의 결과 형식** (7-2)
   - bool 또는 없음으로 적힌 편집 함수들의 반환형과 실패 사유를 각각 확정해야 한다.
     points가 클릭 원위치인지 도로 스냅 후 위치인지, remove_near가 여러 점 중 무엇을 지우는지도 정한다.
   - 경로 재계산은 동기인지 비동기인지, 실패 후 이전 경로를 계속 노출하는지 정한다.
     권장: 현재 점 목록과 맞지 않는 이전 경로를 유효한 결과로 저장하지 않는다.
   - save의 파일명 검증·덮어쓰기·저장 폴더·오류 결과 키를 고정한다. 2점 미만과 실패 결과에서
     route/save의 모든 키를 유지하는지, 성공 때만 있는 키가 무엇인지도 명시한다.

9. **타일 결과·실패·캐시와 화면 수명** (7-3)
   - 매 프레임 request_visible을 불러도 중복 다운로드가 없는지, poll이 완료 이벤트를 한 번만 반환하는지,
     캐시 적중 타일도 같은 방식으로 전달하는지 정한다. 화면 이탈·재진입 시 소유자와 취소 규칙도 필요하다.
   - 현재 poll 형식에는 실패·403/429 대기 상태가 없다. 권장: UI가 상태를 알 수 있는 결과/API를 합의하고,
     해당 상태에서 프레임마다 다시 요청하지 않게 한다. PNG 디코딩 실패를 Rust 캐시에 전달할 방법도 정한다.
   - 지도 텍스처의 화면 측 보유 상한/퇴출 기준, 디스크 캐시와 분리된 수명, offline=true를 설정할 시점과
     자동 시험에서 지역 요청까지 차단하는 방법을 고정한다. 타일 실패가 점 편집·계산·저장을 막아서는 안 된다.
