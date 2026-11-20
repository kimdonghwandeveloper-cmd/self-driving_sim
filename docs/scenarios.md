# 교육 시나리오 파일 형식 (Phase 5-1, 5-2~5-5 확장)

시나리오 하나는 `assets/scenarios/` 아래의 TOML 파일 하나입니다. 메인 메뉴의 코스 목록 끝에 `시나리오: <title>`로 나옵니다.
파일만 고치면 안내 문구·목표 위치·시간 제한 같은 값을 코드 변경 없이 바꿀 수 있습니다(파일은 시나리오를 열 때 다시 읽습니다).
읽기·검사는 `crates/sim-core/src/scenario/`, Godot 연결은 `docs/design/godot-interface.md` v1.4(2-1절)를 봅니다.

> 모든 기준값(정지 구역 반경, 시간 제한, 허용 충돌 수 등)은 **훈련용 임시값**입니다. 교육 담당자가 승인한 평가 기준이 아니며
> 공식 합격·불합격 판정에 쓰지 않습니다(PLAN 5-5, 6-3). 개발용 정지선 예시는 `testdata/scenarios/`로 분리하며 사용자 메뉴에는 표시하지 않습니다.

## 들어 있는 시나리오

| 파일 | 내용 | 기록·실패 |
| --- | --- | --- |
| `scenario-a-basic.toml` | 시나리오 A · 기본 조작(5-2): 시동 → D → 직진 → 정지선 앞 정지 → R → 8 m 후진 → 정지 → P → 시동 끄기 | 기어 변경 오류, 과도한 입력(과속 12 km/h), 정지 위치 오차. 시간 240 s |
| `scenario-b-space.toml` | 시나리오 B · 공간 판단(5-3): 원뿔 피하기 → 원뿔·장애물 사이 문(약 2.5 m) → 좁은 통로(3.0 m) → 끝 벽 앞 정차 | 접촉 위치·속도, 세 번째 접촉에서 실패. 시간 300 s |
| `scenario-c-hazard.toml` | 시나리오 C · 돌발 상황(5-4): 10 m 지나면 보행자 대체 객체가 z = -26을 1.3 m/s로 건넘 → 멈춤 → 다시 출발 → 정차 | 위험 등장(속도·거리), 제동 시작(반응 시간), 보행자 충돌 시 바로 실패. 시간 240 s |

## 구조

```toml
format = 1                       # 필수. 지금은 1만 읽는다
title = "예시 · 정지선 앞 정지"   # 필수. 메뉴·HUD 제목
briefing = "시작 전 안내 문구"    # 선택. 시작 전(F5 전)에 보인다
course = "training"              # 필수. training, calibration, map:<저장소 루트 기준 파일>

[start]                          # 선택. 시작·재시작 때마다 이 상태로 만든다
engine_on = false                # 기본 false
gear = "P"                       # "P", "R", "N", "D". 기본 "P". P가 아니면 engine_on = true 필요
position_m = [0.0, 0.0]          # 선택. 시작 후륜축 중심 (x, z) m. 없으면 코스 시작 자세
heading_deg = 0.0                # 선택(position_m과 함께). 0이면 -Z 방향, +Y축 기준 회전

[[steps]]                        # 한 개 이상. 적힌 순서대로 하나씩 끝낸다(목표)
id = "engine"                    # 파일 안에서 겹치지 않는 이름
guide = "시동을 켜세요."          # 이 단계 동안 HUD 안내
done = { kind = "engine_on" }    # 완료 조건(아래 표)

[[events]]                       # 선택. 조건이 처음 맞는 틱에 한 번 일어난다
id = "approach"
when = { kind = "travel", distance_m = 8.0 }
action = { kind = "message", text = "속도를 줄이세요.", duration_s = 4.0 }

[end]                            # 선택. 실패 조건. 모든 단계를 끝내면 성공
time_limit_s = 180.0             # 경과 시간 상한(s). 없으면 제한 없음
max_collisions = 0               # 허용 충돌 횟수. 넘으면 실패(0이면 첫 충돌에서 실패). 없으면 제한 없음

[[obstacles]]                    # 선택(5-3). 코스에 더할 회전 없는 상자. 다른 충돌 경계와 똑같이 막는다
position_m = [0.8, -18.0]        # 중심 (x, z) m
size_m = [1.0, 1.0]              # 크기 (x, z) m
height_m = 0.7                   # 표시 높이(m). 기본 0.8

[[hazards]]                      # 선택(5-4). 보행자 대체 객체. 이벤트 spawn_hazard로 등장한다
id = "pedestrian"
path_m = [[4.0, -26.0], [-4.0, -26.0]]  # 지나갈 점(두 개 이상). 첫 점에서 등장해 차례로 걷는다
speed_mps = 1.3                  # 걷는 속도
size_m = [0.6, 0.6]              # 바닥 크기(폭, 앞뒤). 기본 [0.6, 0.6]
height_m = 1.7                   # 표시 높이. 기본 1.7
remain_at_end = false            # 끝점에 남는지. 기본 false(사라짐)

[record]                         # 선택(5-2·5-4). 기록 기준. 아래는 기본값
speed_limit_kmh = 15.0           # 넘기 시작하면 과속
harsh_brake = 0.8                # 이 속도(harsh_brake_kmh) 이상에서 제동 입력이 이 값 이상이 되기 시작하면 급제동
harsh_brake_kmh = 10.0
harsh_steer = 0.8                # 이 속도(harsh_steer_kmh) 이상에서 조향 입력 크기가 이 값 이상이 되기 시작하면 급조향
harsh_steer_kmh = 10.0
reaction_brake = 0.1             # 위험 등장 뒤 제동 입력이 이 값 이상이면 제동 시작(반응 시간)

[score]                          # 선택(5-5). 임시 점수 기준. 아래는 기본값. 100에서 뺀다
pass_score = 70.0                # 성공했고 이 점수 이상이면 "임시 합격"
gear_error = 5.0                 # 기어 변경 오류 1회당
harsh_input = 5.0                # 과도한 입력(과속·급제동·급조향) 1회당
collision = 20.0                 # 충돌 1회당(보행자 포함)
emergency_stop = 10.0            # 비상 정지 1회당
stop_error_per_m = 10.0          # 정지 위치 오차 1 m당
reaction_limit_s = 1.5           # 제동 반응이 이 시간을 넘으면
slow_reaction_per_s = 10.0       # 넘은 1초당
```

모르는 키(오타 포함)가 있으면 파일 전체를 거부합니다. 좌표는 코스 지면 (x, z) m이고, 위치 조건은 **차체 중심** 기준입니다
(후륜축에서 전방 1.4 m, 차체 길이 4.2 m). training 코스는 시작 후륜축이 원점, 전방 -Z, 정지선이 z = -20입니다.

## 조건 (`done`, `when`)

| `kind` | 추가 값 | 맞는 때 |
| --- | --- | --- |
| `engine_on` | | 시동이 켜짐(비상 정지 중 아님) |
| `engine_off` | | 시동이 꺼짐 |
| `gear` | `gear = "D"` | 기어가 그 값 |
| `stopped` | | 정지 판정(`config/vehicle.toml`의 정지 임계값) |
| `speed_at_least` | `kmh` | 속도 크기가 그 값 이상 |
| `travel` | `distance_m` | 시나리오 시작 뒤 움직인 거리(전진·후진 합) 이상 |
| `reversed` | `distance_m` | 시나리오 시작 뒤 후진한 거리 이상 |
| `reach` | `position_m = [x, z]`, `radius_m` | 차체 중심이 그 원 안 |
| `stop_in` | `position_m = [x, z]`, `radius_m` | 차체 중심이 그 원 안에서 정지 |
| `stop_at_goal` | `radius_m` | 차체 중심이 코스 목표(지도 경로 끝, 4-6) 원 안에서 정지. 목표 없는 코스(training 등)에는 쓸 수 없다 |
| `elapsed` | `s` | 경과 시간(일시정지 제외) 이상 |
| `step_done` | `step = "<단계 id>"` | 그 단계를 끝냄. **이벤트(`when`)에만** 쓴다 |

조건은 매 틱(1/64 s) 운동 모델 다음에 판정합니다. 같은 틱에 여러 단계가 연달아 끝날 수 있습니다.

## 이벤트 동작 (`action`)

| `kind` | 값 | 하는 일 |
| --- | --- | --- |
| `message` | `text`, `duration_s` | HUD 시나리오 상자에 `duration_s`초(일시정지 제외) 동안 띄운다. 새 메시지가 오면 바꾼다 |
| `spawn_hazard` | `hazard` | `[[hazards]]`의 그 객체를 이 틱에 등장시킨다. 한 객체는 한 번만 등장시킬 수 있다 |

### 보행자 대체 객체(5-4)

- AI가 아니라 **결정적 스크립트**입니다. 등장 틱부터 `path_m`을 `speed_mps`로 걷고, 위치는 (경과 틱 × 1/64 s × 속도)로만 정해집니다.
  같은 입력이면 같은 틱·같은 자리에 나옵니다(시험 `scenario_c_hazard_repeats_identically`).
- 차량을 **막지 않습니다**. 차체와 겹치기 시작한 틱에 충돌 1회로 세고 기록합니다(`max_collisions = 0`이면 바로 실패).
- 화면에는 주황 상자(몸)와 공(머리)으로 그립니다. 실제 사람 모양·동작이 아닙니다.

## 실행과 조작

| 단계 | 차량 | 키(주행 화면) |
| --- | --- | --- |
| 시작 전 | 시작 조건 그대로 멈춤, 시간 0 | F5 시작 · F8 종료 |
| 진행 중 | 주행 | F6 일시정지 · F7 재시작 · F8 종료 |
| 일시정지 | 차량·시간·메시지 멈춤 | F5 계속 · F7 재시작 · F8 종료 |
| 끝(성공·실패·중단) | 끝난 순간 그대로 멈춤 | F7 다시 하기 · Esc 메뉴 |

- 같은 동작의 버튼도 시나리오 상자에 있습니다. 버튼은 키보드 포커스를 받지 않습니다(Enter·Space가 시동·비상 정지 키이기 때문).
- 기능 키는 `config/input.toml`의 주행 키와 겹치지 않게 골랐습니다. 게임패드에는 아직 시나리오 조작 버튼이 없습니다.
- 진행 중 리셋 키(R)는 기존처럼 차량만 시작 위치로 돌리고 시나리오 진행은 그대로 둡니다(기록에 `course_reset`이 남음). 처음부터 하려면 F7입니다.
- 끝나면(성공·실패·중단) 시나리오 상자에 기록 요약·임시 점수·결과 파일 경로가 나옵니다.

## 기록(5-2~5-5)

진행 중 틱마다 아래 사건을 일어난 순서대로 기록합니다(`tick`, `time_s` = 시작부터 경과 시간, 일시정지 제외).

| `kind` | 언제 | 값 |
| --- | --- | --- |
| `step_done` | 단계를 끝냄 | `step` |
| `gear_error` | 기어 변경이 거부됨(움직이는 중, 시동 OFF, 기어 키 동시 입력 등) | `command`, `reason` |
| `over_speed` | 속도가 `speed_limit_kmh`를 넘기 시작 | `speed_kmh` |
| `harsh_brake` | `harsh_brake_kmh` 이상에서 제동 입력이 `harsh_brake` 이상이 되기 시작 | `value`, `speed_kmh` |
| `harsh_steer` | `harsh_steer_kmh` 이상에서 조향 입력 크기가 `harsh_steer` 이상이 되기 시작 | `value`, `speed_kmh` |
| `emergency_stop` | 비상 정지를 켬 | `speed_kmh` |
| `collision` | 코스 장애물에 막히거나 보행자 대체 객체와 겹치기 시작 | `speed_kmh`, `x_m`, `z_m`(후륜축), `hazard`(보행자면 이름, 아니면 `null`) |
| `stop_position` | `stop_in`·`stop_at_goal` 단계를 끝냄 | `step`, `error_m`(차체 중심 ↔ 목표 중심), `x_m`, `z_m`(차체 중심) |
| `hazard_appeared` | 보행자 대체 객체 등장 | `hazard`, `x_m`, `z_m`(등장 자리), `speed_kmh`, `distance_m`(차체 중심까지) |
| `brake_start` | 등장 뒤 처음 제동 입력이 `reaction_brake` 이상 | `hazard`, `reaction_s`, `speed_kmh`, `distance_m` |
| `course_reset` | 진행 중 R로 시작 위치 복귀 | |
| `finished` | 끝 | `outcome` |

키보드는 가속·제동이 0 또는 1뿐이라 입력 크기만으로는 과도한 입력을 가를 수 없어, 과속·급제동·급조향을 모두 속도와 함께 봅니다.

## 결과 파일(5-5)

끝날 때마다 저장소 `logs/scenarios/<세션 ID>.json`에 저장합니다(git에 올리지 않음, 같은 이름이면 `-2`…). 형식 `scenario-result-v1`:

| 키 | 내용 |
| --- | --- |
| `session_id` | 시작 시각(UTC)과 파일 이름, 예: `20261111-093012-scenario-a-basic` |
| `scenario_path`, `scenario_title`, `scenario_text`, `course` | 연 시나리오와 그 파일 내용(재현용) |
| `started_at`, `ended_at` | ISO 8601 UTC. 시작하지 않고 끝내면 둘이 같다 |
| `outcome`, `completed` | `success`·`time_limit`·`collisions`·`aborted`, 성공 여부 |
| `elapsed_s`, `ticks`, `steps` | 경과 시간·틱 수, 단계마다 끝낸 시각(`done_s`, 못 끝냈으면 `null`) |
| `summary` | 기어 오류·과도한 입력·충돌(보행자)·비상 정지·복귀 횟수, 정지 위치 오차들, 반응 시간들, 제동 없음 수 |
| `score` | 임시 점수 `value`, `pass_score`, `passed`, 감점 목록 `deductions`, `notice`(임시 기준 안내) |
| `records` | 위 기록 사건 전체 |
| `input_track` | 재현용 입력 열 `[{ticks, throttle, brake, steering, commands}]`(같은 입력끼리 묶음, `commands`는 구간 첫 틱에 처리) |

제동하지 않은 위험의 반응 시간은 등장부터 끝까지로 칩니다. **점수는 임시 기준이며 공식 평가가 아닙니다.**

재현 확인(저장소 최상위, 같은 `config/vehicle.toml`이 필요): `cargo run --locked -p sim-tools -- --check-result logs/scenarios/<파일>.json`.
파일의 시나리오 내용과 입력 열을 다시 돌려 결과·기록이 같으면 `replay matches`, 다르면 처음 다른 기록을 출력합니다.

## 오류

파일을 읽지 못하거나 규칙을 어기면 주행 화면에 `시나리오 오류: <사유>`가 나오고 차량을 만들지 않습니다.
메뉴 목록에는 `<파일 이름> (오류)`로 보입니다. 검사 항목: 형식 번호, 빈 제목·코스, 시작 기어와 시동, 시작 좌표·방향 값,
단계 1개 이상·이름 겹침·빈 안내, 수치 범위(음수·0 반경·NaN), `step_done`이 가리키는 단계, 메시지 문구·시간, 시간 제한 값,
목표 조건과 코스 목표 유무, 장애물 크기·높이, 보행자 대체 객체 이름 겹침·점 수·속도·크기, `spawn_hazard`가 가리키는 객체와
두 번 등장, 기록 기준(입력 0..1, 속도 0 이상)·점수 기준(0 이상).
