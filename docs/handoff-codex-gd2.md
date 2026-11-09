# Codex 개발 투입 인계: GD-2 Godot 쪽 (2일 목표)

기록일: 2026-11-08, Claude 작성. 사용자 결정: 일정을 줄이기 위해 Codex도 검토뿐 아니라 개발을 맡는다.
이 문서는 `AGENTS.md`를 대체하지 않는다. 충돌하면 `AGENTS.md`와 사용자 지시가 우선한다.
**작업 전 반드시 확인:** `git status --short`, `git worktree list`, `git log --all --oneline -15`.

## 1. 목표 (2일, 2026-11-08 사용자 지시로 수정)

사용자 지시: **시나리오 A와 여수 경로 주행은 제외**하고, 시스템 구현과 **경로 선택·지도 화면 개선**을 우선한다.
구현 전에 재생 시험의 틱 안 적용 순서와 코스 생성 실패 처리를 계약에 명확히 정한다(→ `docs/design/godot-interface.md` v0.1 5·6절).

| 순서 | 공동 결과물 |
| --- | --- |
| 0 | 인터페이스 계약 v0.1 검토 → v1 고정(구현 시작 조건) |
| 1 | 시스템: Godot에서 개발 코스(training·calibration) 키보드·게임패드 주행, 입석 시점 4방향, 최소 HUD, 실패 처리, 재생 시험 |
| 2 | 경로 선택·지도 화면: 전국 배경 지도 → 지역 도로 받기 → 점 찍기 → 경로·경고 확인 → 저장(계약 7절). 개선 항목은 사용자 확인 후 |
| 3 | 메뉴: 개발 코스 주행·지도 화면·종료 |

제외: 시나리오 A·B·C, 여수(저장 경로) 주행, GD-3 그래픽 고도화, 경사, 기록 저장, 패키징, Bevy 정리.

## 2. 역할과 파일 소유권

| | Claude | Codex |
| --- | --- | --- |
| 수정 영역 | `crates/sim-core/`, `crates/sim-godot/`, 루트 `Cargo.toml`·`Cargo.lock`, `README.md`, `PLAN.md` | `godot/` 전체(장면 `.tscn`, `.gd`, 재질, `project.godot`) |
| 시스템 | GD-1b 형상 이동 마무리, `SimVehicle`·`SimCourse`·`SimReplay` 구현, 실패 처리 | 주행 장면, 코스 메시 조립, 차체·최소 실내, 입석 카메라 리그(4방향), 조명, HUD, `godot/tests/drive_replay_test.gd` |
| 경로·지도 | GD-1c(지도·경로·스냅샷·타일 모듈 sim-core 이동), `SimRoutePicker`·`SimTiles` 구현 | 지도 화면(Camera2D 이동·확대, 타일·도로·점·경로 그리기, 상태·경고 표시, 출처 표기), 메뉴 |
| 검토 | Codex 브랜치 교차 검토 | Claude 브랜치 교차 검토 |

- 상대 영역 파일은 수정하지 않는다. 필요하면 요청 문서(아래 5절)로 요청한다.
- `docs/design/godot-interface.md`(인터페이스 계약)는 둘 다 고칠 수 있지만, 이름·인자·순서 규칙 변경은 상대 확인 후에만 한다.
- `config/*.toml`은 둘 다 읽기만 한다. 값 변경은 사용자 확인 후.

## 3. 인터페이스

`docs/design/godot-interface.md`(v0.1) 참고. 요약:

- `SimVehicle`(Node3D): 입력·상태 머신·운동 모델·충돌을 Rust가 처리하고 매 물리 틱 자기 transform을 갱신한다.
  Godot 쪽은 이 노드 **자식**으로 차체·실내·카메라를 붙인다. HUD는 읽기 함수와 시그널을 쓴다.
  코스 메시는 반드시 `SimVehicle.course_data()`(충돌 판정과 같은 데이터)로 만든다.
- 틱 안 순서(5-1절): 간격 고정(1/64 s) → 장치 입력 → 명령(대기열 → 장치) → 연속 입력 → 운동·충돌 → 자세 반영 → 시그널.
  `send_command`는 다음 틱에 처리된다.
- 실패 처리(6절): 코스 실패는 `is_ready()=false` + `course_failed` + 정지, 설정 오류는 기본값으로 계속 + `config_warnings()`.
- 경로 선택·지도(7절): 계산·받기·캐시는 Rust(`SimRoutePicker`, `SimTiles`), 그리기·조작은 Godot. 타일은 Godot `HTTPRequest`로 받지 않는다.
- `project.godot`에 `physics/common/physics_ticks_per_second = 64`를 넣는다(Bevy 판과 같은 고정 간격).

Claude 구현이 오기 전에는 `ClassDB.class_exists("SimVehicle")`로 확인하고, 없으면 GDScript 임시 노드로 장면을 먼저 만든다.
임시 노드는 계약과 같은 함수 이름을 쓰고, Rust 구현이 오면 교체만 하면 되게 한다.

## 4. 브랜치·검증·병합

- 브랜치: Claude `feature/gd2-rust`, Codex `feature/gd2-godot`. 둘 다 최신 `develop`(= main) 기준.
- Codex 작업 워크트리는 Claude의 develop 워크트리와 **다른 폴더**를 쓴다(같은 브랜치 동시 체크아웃 불가).
  예: `git worktree add ../self-driving_sim-codex -b feature/gd2-godot develop`. 폴더를 만들기 전에 사용자 확인.
- 커밋 메시지는 한글. 작업 단위로 커밋한다.
- 사용자 승인된 절차 변경(승인 전이면 기존 `AGENTS.md` 절차를 따른다):
  검토는 하루 1~2번 묶어서 하고, 병합은 사용자 승인 후 메인 리포지토리에서 `--no-ff`로만 한다. push는 사용자 요청 시에만.
- Godot 쪽 검증:
  - `godot --headless --path godot --script res://tests/smoke_test.gd`가 exit 0.
  - 캐시 없는 첫 import는 `--frame-delay 1000`이 필요하다(README "Godot 연결과 첫 import 검사", `tools/test-godot-cold-import.ps1`).
    기본 `--import` 바로 종료는 Godot 4.7.2에서 접근 위반이 난다(godot#111048 계열, 원인 미확정).
  - 장면을 바꾸면 에디터에서 열어 오류가 없는지와, 사람이 확인할 항목(조작감·시점)을 따로 적는다.
- Godot 실행 파일: `C:\Users\kimdo\Downloads\Godot_v4.7.2-stable_win64.exe\Godot_v4.7.2-stable_win64_console.exe`.

## 5. 서로 요청·보고하는 방법

- 요청·질문: `docs/design/godot-interface.md` 8절 "아직 정하지 않은 것"에 항목을 추가하고 커밋하거나,
  사용자를 통해 전달한다. 상대 영역 파일을 직접 고치지 않는다.
- 문제가 생기면 멋대로 판단해 우회하지 말고 보고한다(사용자 규칙).
- 완료 처리는 사용자가 한다. "완료"로 단정하지 말고 통과한 검사와 남은 확인 항목을 나눠 적는다.

## 6. 메모리·빌드 주의 (중요)

- 전체 메모리 19.9 GB. RustRover·Godot 에디터와 함께 Rust 빌드 2개가 동시에 돌면 Claude Code가 작업을 강제 종료한다(2026-11-08 세 번 발생).
- **Rust 빌드는 Claude 쪽에서만 한다.** Codex는 `cargo build -p sim-godot`로 만든 DLL을 쓴다.
  Codex가 Rust 빌드가 꼭 필요하면 `-j 1`로 하고, Claude 빌드와 겹치지 않게 사용자에게 알린다.
- dev 프로필 디버그 정보 축소안: `feature/dev-profile-debuginfo`(e0a10d1), 측정·병합 전.
- Godot 작업은 빌드 부담이 작아 동시 진행에 문제가 없다.

## 7. 현재 Git 상태 (작성 시점 스냅샷)

| 브랜치 | 커밋 | 상태 |
| --- | --- | --- |
| `main` | a2e5676 | GD-0·Phase G 계획 병합 완료, origin보다 6커밋 앞(미 push) |
| `feature/gd-1a-sim-core-vehicle` | b89a207 | 차량 로직 sim-core 분리. Codex 검토 요청 `gd-1a-1794139428685352200` 대기 |
| `feature/gd-1b-sim-core-geometry` | (미커밋) | 코스·교차로·도로 형상 이동, develop 워크트리 stash@{0}에 보관 |
| `feature/dev-profile-debuginfo` | e0a10d1 | 빌드 메모리 절감 설정, 측정 미완 |
| `docs/gd2-parallel-plan` | 이 문서 | 인터페이스 계약 초안(v0.1)과 이 인계 문서. Codex는 문서·Git 상태만 확인했고 작업 중단 상태 유지(2026-11-08) |

## 8. Codex가 처음 할 일

1. 이 문서와 `docs/design/godot-interface.md`(v0.1)를 읽고, Godot 쪽으로 쓰기 어렵거나 순서·실패 규칙이 모호한 부분을 계약 8절에 적는다.
   v1 고정 전에는 구현을 시작하지 않는다.
2. 대기 중인 GD-1a 검토(`gd-1a-1794139428685352200`)를 마친다.
3. 사용자 확인 후 Codex 워크트리·브랜치를 만들고 1일차 Godot 장면 작업을 시작한다.
