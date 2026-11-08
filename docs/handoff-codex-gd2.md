# Codex 개발 투입 인계: GD-2 Godot 쪽 (2일 목표)

기록일: 2026-11-08, Claude 작성. 사용자 결정: 일정을 줄이기 위해 Codex도 검토뿐 아니라 개발을 맡는다.
이 문서는 `AGENTS.md`를 대체하지 않는다. 충돌하면 `AGENTS.md`와 사용자 지시가 우선한다.
**작업 전 반드시 확인:** `git status --short`, `git worktree list`, `git log --all --oneline -15`.

## 1. 목표 (2일)

| 날짜 | 공동 결과물 |
| --- | --- |
| 1일차 | Godot에서 개발 코스(training·calibration)를 키보드·게임패드로 주행, 입석 시점 전·후·좌·우 전환 |
| 2일차 | 최소 HUD(속도·기어·시동·비상 정지·입력 장치), 간단한 메뉴(코스 선택·종료), 시나리오 A(기본 조작) 1개 |

미루는 것: GD-1 나머지(지도·경로·PBF 이동), GD-3 그래픽 고도화·여수 경로, 경사, 시나리오 B·C, 기록 저장, 패키징, Bevy 정리.
2일차 범위(시나리오 A와 여수 경로 중 무엇을 넣을지)는 사용자 확정 전이다.

## 2. 역할과 파일 소유권

| | Claude | Codex |
| --- | --- | --- |
| 수정 영역 | `crates/sim-core/`, `crates/sim-godot/`, 루트 `Cargo.toml`·`Cargo.lock`, `README.md`, `PLAN.md` | `godot/` 전체(장면 `.tscn`, `.gd`, 재질, `project.godot`) |
| 1일차 | GD-1b 형상 이동 마무리, `SimVehicle`·`SimCourse` 구현 | 주행 장면, 코스 메시 조립, 차체·최소 실내 형상, 입석 카메라 리그(4방향), 조명 |
| 2일차 | 시나리오 A 판정 로직(sim-core, Rust 테스트), 헤드리스 재생 기대값 | HUD, 메뉴, 시나리오 결과 화면, `godot/tests/drive_replay_test.gd` |
| 검토 | Codex 브랜치 교차 검토 | Claude 브랜치 교차 검토 |

- 상대 영역 파일은 수정하지 않는다. 필요하면 요청 문서(아래 5절)로 요청한다.
- `docs/design/godot-interface.md`(인터페이스 계약)는 둘 다 고칠 수 있지만, 이름·인자 변경은 상대 확인 후에만 한다.
- `config/*.toml`은 둘 다 읽기만 한다. 값 변경은 사용자 확인 후.

## 3. 인터페이스

`docs/design/godot-interface.md` 참고. 요약:

- `SimVehicle`(Node3D): 입력·상태 머신·운동 모델·충돌을 Rust가 처리하고 매 물리 틱 자기 transform을 갱신한다.
  Godot 쪽은 이 노드 **자식**으로 차체·실내·카메라를 붙인다. HUD는 `speed_kmh()`, `gear()` 등 읽기 함수와 시그널을 쓴다.
- `SimCourse.build(name) -> Dictionary`: 노면·띠·교차로 면·표시선·충돌 경계 데이터. 메시 조립과 재질은 Godot 쪽.
  Godot 물리 충돌체는 만들지 않는다(충돌은 Rust가 판정).
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

- 요청·질문: `docs/design/godot-interface.md` 6절 "아직 정하지 않은 것"에 항목을 추가하고 커밋하거나,
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
| `docs/gd2-parallel-plan` | 이 문서 | 인터페이스 계약 초안과 이 인계 문서 |

## 8. Codex가 처음 할 일

1. 이 문서와 `docs/design/godot-interface.md`를 읽고, 인터페이스 초안에서 Godot 쪽으로 쓰기 어려운 부분을 6절에 적는다.
2. 대기 중인 GD-1a 검토(`gd-1a-1794139428685352200`)를 마친다.
3. 사용자 확인 후 Codex 워크트리·브랜치를 만들고 1일차 Godot 장면 작업을 시작한다.
