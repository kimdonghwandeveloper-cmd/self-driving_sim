# Godot ↔ Rust 인터페이스 계약 (GD-2, 초안 v0)

작성: 2026-11-08, Claude 초안. **Codex 확인 전 초안**이며, 확인 후 v1로 고정한다.
목적: Claude(Rust, `crates/`)와 Codex(Godot, `godot/`)가 동시에 개발할 때 서로 기다리지 않도록
Godot에 노출할 클래스·메서드·시그널·좌표 규칙을 먼저 정한다.

변경 규칙:

- 이 문서를 먼저 고치고 커밋한 뒤 코드를 바꾼다. 문서와 코드가 다르면 문서가 기준이다.
- 이름·인자·반환형을 바꾸는 변경은 상대 에이전트 확인 후에만 한다. 항목 추가는 자유롭게 하되 문서에 적는다.
- 아직 구현되지 않은 항목은 "구현: 미" 로 표시한다. Godot 쪽은 `ClassDB.class_exists()`로 확인하고 없으면 임시값을 쓴다.

## 1. 공통 규칙

| 항목 | 규칙 |
| --- | --- |
| 단위 | 길이 m, 속도 m/s(표시용 km/h 별도 제공), 각도: 함수 이름에 `_deg`/`_rad`를 붙여 구분 |
| 좌표 | Rust 지면 (x, z) → Godot `Vector3(x, 0, z)`. 높이는 Godot에서 정한다(경사 미지원) |
| 방향 | heading 0이면 전방이 -Z. Godot `rotation.y = heading_rad`와 같다(Node3D 기본 전방 -Z와 일치) |
| 차량 원점 | 후륜축 중심. 차체 중심은 전방으로 1.4 m(`VEHICLE_FOOTPRINT`, 임시값) |
| 시간 | 차량 갱신은 `_physics_process` 고정 간격. `project.godot`의 `physics/common/physics_ticks_per_second = 64`(Bevy FixedUpdate 64 Hz와 같게) |
| 설정 파일 | 저장소 루트 `config/*.toml`을 그대로 읽는다. 경로는 `res://` 기준 `../config` |
| 오류 | 설정 파일 오류는 Bevy 판과 같이 경고(`godot_warn!`) 후 기본값 사용. 치명 오류만 `godot_error!` |

## 2. `SimVehicle` (Rust, `Node3D` 상속) — 구현: 미 (Claude)

차량 하나. 입력 읽기·상태 머신·운동 모델·충돌을 Rust가 모두 처리하고, 매 물리 틱 끝에 자기 `transform`을 갱신한다.
Godot 쪽은 이 노드의 **자식**으로 차체 메시·실내·카메라를 붙인다(차량 로컬 좌표 = 이 노드 좌표).

### 내보내는 속성 (`#[export]`)

| 이름 | 형 | 기본값 | 설명 |
| --- | --- | --- | --- |
| `course` | `String` | `"training"` | `"training"` 또는 `"calibration"`. `_ready`에서 코스를 만들고 시작 자세에 놓는다 |
| `config_dir` | `String` | `""` | 비우면 `res://../config`. 테스트에서 다른 설정 폴더를 쓸 때만 지정 |
| `read_input` | `bool` | `true` | `false`면 Godot 입력을 읽지 않고 `set_manual_input`/`send_command`만 쓴다(헤드리스 시험용) |

### 읽기 함수 (`#[func]`, HUD·카메라용)

| 함수 | 반환 | 설명 |
| --- | --- | --- |
| `speed_mps()` | `float` | 부호 있는 속도(후진 음수) |
| `speed_kmh()` | `float` | 표시용 절댓값 km/h |
| `gear()` | `String` | `"P"`, `"R"`, `"N"`, `"D"` |
| `engine_on()` | `bool` | 시동 상태 |
| `emergency_active()` | `bool` | 비상 정지 중 |
| `steer_angle_deg()` | `float` | 현재 바퀴 조향각(오른쪽 양수, Rust `Motion::steer_angle_rad`와 같은 부호) |
| `in_contact()` | `bool` | 장애물·연석과 접촉해 정지한 상태 |
| `view()` | `String` | `"front"`, `"rear"`, `"left"`, `"right"` (입석 시점 방향) |
| `debug_camera()` | `bool` | 외부 디버그 카메라 켜짐 |
| `input_source()` | `String` | `"keyboard"`, `"gamepad"`, `"none"` (마지막으로 입력이 들어온 장치) |
| `course_notices()` | `PackedStringArray` | 화면에 계속 띄울 안내(임시값·출처 표기) |

### 명령 함수

| 함수 | 설명 |
| --- | --- |
| `reset_course()` | 정지 상태일 때만 시작 자세로 복귀(`reset_course` 키와 같음). 성공 여부 `bool` 반환 |
| `send_command(name: String)` | `"gear_park"`, `"gear_reverse"`, `"gear_neutral"`, `"gear_drive"`, `"engine_toggle"`, `"emergency_stop"`, `"emergency_release"`. 결과 `bool` |
| `set_manual_input(throttle, brake, steering)` | `read_input = false`일 때 쓰는 연속 입력(0..1, 0..1, -1..1, 조향 오른쪽 양수) |

### 시그널

| 시그널 | 인자 | 발생 시점 |
| --- | --- | --- |
| `gear_changed` | `gear: String` | 기어 변경 성공 |
| `engine_changed` | `on: bool` | 시동 변경 |
| `emergency_changed` | `active: bool` | 비상 정지 걸림/해제 |
| `command_rejected` | `command: String, reason: String` | 상태 머신이 거부(예: 주행 중 P) |
| `collided` | 없음 | 접촉 시작 순간 1회 |
| `view_changed` | `view: String` | 시점 키 입력 |
| `debug_camera_toggled` | `on: bool` | 디버그 카메라 키 입력 |
| `course_reset` | 없음 | 시작 자세 복귀 |

### 카메라 설정

`operator_view() -> Dictionary` (구현: 미): `config/camera.toml`의 값을 그대로 준다.

```
{ "eye_m": Vector3, "pitch_deg": float, "vertical_fov_deg": float, "near_clip_m": float }
```

네 방향 시점은 같은 눈 위치에서 yaw만 바꾼다: front 0°, left +90°, rear 180°, right -90°(+Y축 기준).
Godot 쪽 카메라 리그가 `view_changed`를 받아 yaw를 바꾼다. 시점 전환은 주행 입력에 영향을 주지 않는다.

## 3. `SimCourse` (Rust, `RefCounted`, 정적 함수) — 구현: 미 (Claude)

`SimCourse.build(name: String) -> Dictionary`: 코스 형상 데이터. 메시 조립·재질은 Godot 쪽에서 한다.
충돌 판정은 Rust `SimVehicle`이 같은 데이터로 하므로 Godot 쪽에 물리 충돌체를 만들 필요가 없다.

```
{
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
Rect = { "center": Vector3, "size": Vector2(폭 x, 길이 z), "heading_rad": float }
```

- 모든 `Vector3`의 y는 0이다. 표시 높이(노면 0.012 m, 표시선 0.016 m 등)는 Godot 쪽에서 정한다.
  Bevy 판 값은 `src/world/mod.rs`의 `SURFACE_Y_M`, `MARKING_Y_M`, `PAINT_THICKNESS_M` 참고.
- `strips`의 `left[i]`와 `right[i]`가 마주 본다. 점 개수가 같다.
- 알 수 없는 이름이면 빈 `Dictionary`를 돌려주고 `godot_error!`를 남긴다.

## 4. `SimBridge` (기존, 정적 함수)

`version() -> String`, `kmh_to_mps(kmh: float) -> float`. 연결 확인용으로 유지한다.

## 5. 헤드리스 시험 계약 (GD-2 자동 시험)

`godot/tests/drive_replay_test.gd`(Codex)가 `SimVehicle`을 `read_input = false`로 만들고,
정해진 명령·입력 열을 고정 틱 수만큼 넣은 뒤 `global_position`·`speed_mps()`를 출력한다.
Rust 쪽(Claude)은 같은 입력 열을 Rust 단독으로 재생한 기대값을 `sim-core` 테스트로 고정한다.
두 결과의 허용 오차는 위치 1e-3 m로 시작한다(f32 누적 차이를 보고 조정).

입력 열 형식(초안):

```
[ { "ticks": 64, "command": "engine_toggle" },
  { "ticks": 1,  "command": "gear_drive" },
  { "ticks": 320, "throttle": 0.5, "brake": 0.0, "steering": 0.0 } ]
```

## 6. 아직 정하지 않은 것

- 게임패드 축 이름 매핑(Bevy `GamepadAxis` → Godot `JoyAxis`)과 데드존 처리를 Rust가 할지(권장) Godot InputMap에 맡길지.
  초안은 **Rust가 `Input` 싱글턴을 직접 읽어** Bevy 판과 같은 규칙을 쓰는 쪽이다.
- 키 이름 변환표(Bevy `KeyCode` 이름 → Godot `Key`). Claude가 `sim-godot`에 둔다.
- 시나리오(Phase 5 A) 판정 결과를 넘기는 형식. 2일차에 추가한다.
