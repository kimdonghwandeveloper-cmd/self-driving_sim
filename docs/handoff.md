# 새 에이전트 환경 인수인계

기록일: 2026-11-10 (Asia/Seoul). 처음 작성 2026-10-29, 이번에 현재 상태로 다시 썼다.
이 문서는 대화·에이전트 메모리가 없는 새 환경을 위한 상태 기록이다.
아래 커밋과 워크트리는 작성 시점의 스냅샷이다. 작업 전에 실제 Git 상태를 다시 확인한다.

## 먼저 읽고 확인할 것

1. `AGENTS.md`: 개발·검증·병합 규칙. 이 문서는 해당 규칙을 대체하지 않는다.
2. 이 문서, `PLAN.md`(특히 Phase G와 각 절의 "점검"·"진행 기록"), `README.md`.
3. `docs/design/godot-interface.md`(Rust↔Godot 계약), `godot/IMPLEMENTATION.md`, `docs/map-data.md`,
   `docs/vehicle-assumptions.md`, `docs/development.md`.
4. `git status --short`, `git worktree list`, `git branch --no-merged main`으로 실제 상태 확인.

기존 PLAN 체크와 과거 테스트 보고만으로 새 변경의 완료를 단정하지 않는다.
PLAN의 체크는 구현과 자동 시험이 확인된 항목만 뜻하고, 사람 확인이 남은 것은 각 절의 "남은 것"에 따로 적는다.
사용자는 코드 검토와 수동 주행을 구분하며, 수동 주행은 직접 수행하겠다고 했다.

## 프로젝트 목적과 현재 범위

- 자율주행 셔틀 안전관리자 조작 절차 훈련용 시뮬레이터. 실차와 같은 조작감·제동거리·안전성을 검증하거나 인증하는 도구가 아니다.
- 구조(2026-11-08 사용자 결정, PLAN Phase G): 엔진과 무관한 로직은 `crates/sim-core`, 화면은 Godot 4.7(`godot/`)이며
  `crates/sim-godot`(godot-rust 0.5.5, cdylib)로 연결한다. 기존 Bevy 앱(루트 `src/`)은 비교용으로 남아 있고 검사는 계속 통과해야 한다.
  Bevy 앱 제거 여부는 GD-5에서 사용자에게 확인한다.
- 구현됨: 키보드·Xbox 유형 게임패드 입력, 시동·기어·비상 정지 상태 머신, 중속 운동 모델(2-6), 충돌, 입석 1인칭 네 시점,
  개발 코스, 여수 OSM 코스, 굽은 길·교차로·경로 탐색·지도 받기·캐시, 경로 선택 지도(전국 타일), 메인 메뉴, 경로 끝 목표 표시.
- 미구현: 교육 시나리오·평가·JSON 결과 저장(Phase 5), 경사 도로(4-7), 통합 검수·배포(Phase 6 중 6-2 이후),
  Geofabrik 전국 원자료의 실제 PBF 검증(G-1 일부)과 G-2~G-4.
- 사람 확인이 남은 것: GD-2~GD-4·4-6의 직접 주행·메뉴·지도 화면 확인, 실제 게임패드 확인, 실제 네트워크 타일·도로 받기.

## 작성 시점 Git 상태 (2026-11-10)

| 위치 / 브랜치 | 커밋 | 상태 |
| --- | --- | --- |
| main 워크트리 / `main` | `6335267` | Godot 지도 화면 보완 병합까지 반영. `origin`에는 push하지 않았다 |
| develop 워크트리 / `develop` | `6335267` | main과 같음. 이 문서는 여기서 만든 `docs/plan-status-2026-11-10`에서 작성 |
| test 워크트리 / detached HEAD | `271c5ee` | 마지막 검증 커밋 |
| plan-audit 워크트리 / `docs/plan-status-audit` | `c7a49fa` | 10-29 상태표. 낡아서 병합하지 않기로 함(사용자 동의 2026-11-10) |
| verification 워크트리 / `feature/verification-signals` | `0139b62` | Codex↔Claude 검증 요청 도구. 병합 여부 미정, 보관 중 |
| review·verification-test 워크트리 | detached | 과거 검토용. 정리 여부 미확인 |

경로: main은 `C:\Users\kimdo\RustroverProjects\self-driving_sim`, 형제 워크트리는 `../self-driving_sim-<이름>`이다.
환경에 따라 대소문자·마운트 경로가 달라질 수 있으므로 경로를 그대로 가정하지 않는다.
main에는 사용자의 `.idea/self-driving_sim.iml` 미커밋 변경과 추적하지 않는 `Car_photo/phase3-visual-guide-v1.png`가 있다. 건드리지 않는다.
develop 워크트리의 `godot/project.godot`·`.glb.import` 변경은 Godot 에디터가 만든 것(섹션 순서 등)이라 커밋하지 않는다.

정리된 브랜치(2026-11-10, 사용자 확인): `feature/map-ui-port`(병합 후 삭제), `feature/gd2-godot`(Codex 미검증 작업,
필요 기능만 다시 구현하고 병합 없이 삭제, 마지막 커밋 `dcc2fee`), Codex 워크트리 `self-driving_sim-codex`도 제거했다.

### 검증 명령과 Godot 확장 빌드

```text
cargo.exe fmt --all -- --check
cargo.exe clippy --locked --all-targets --all-features -- -D warnings
cargo.exe test --locked --all-features
cargo.exe run --locked -- --smoke-test
```

Godot 시험(워크트리 최상위, 명령 전체는 `README.md` "Godot 연결과 첫 import 검사"):
`cargo.exe build --locked -p sim-godot` → `--headless --path godot --import --frame-delay 1000` →
`godot/tests/`의 `smoke_test`·`presentation_smoke`·`drive_replay_test`·`map_course_test`·`menu_flow_test`·`route_save_test`·`goal_reset_test`.

- `godot/sim_godot.gdextension`은 **같은 워크트리의** `target/debug/sim_godot.dll`을 읽는다. 워크트리마다 `target/`이 따로라서,
  Godot으로 열 워크트리에서 먼저 `cargo.exe build -p sim-godot`를 해야 한다. 없으면 `GDExtension dynamic library not found`가 나고
  조작·지도 화면이 모두 멈춘다(2026-11-10 develop 워크트리에서 실제로 발생). 빌드 후 에디터를 다시 시작한다.
- `goal_reset_test`의 `ERROR: ... 없는-코스.toml` 한 줄은 의도된 실패 경로라 정상이다.
- Godot import는 `autonomous_shuttle_topology_v4.glb.import`를 바꿀 수 있다. 코드 변경이 아니므로 커밋하지 않는다.

툴체인과 의존성 버전은 `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`을 기준으로 확인한다.

## 유지해야 할 구현 규칙과 한계

### 기어와 입력 실행 순서

- 시동 Ready에서 N은 언제든, P는 정지 판정 시에만 허용.
- D/R은 정지 판정 시 또는 현재 진행 방향과 같을 때 허용.
  전진 중 N→D 허용, 전진 중 D→R·N→R 거부.
- 정지 판정은 설정 임계값을 사용한다. 시동 OFF·비상 정지에서는 구동 가속과 기어 변경 차단.
- Bevy 판: 키보드 읽기와 명령 처리는 `RunFixedMainLoop`의 `BeforeFixedMainLoop`,
  운동 계산은 `FixedUpdate`, 화면 표시는 `Update`다.
- Godot 판: `SimVehicle`이 `_physics_process`(64틱/초)에서 장치를 읽고 `sim-core` 주행 세션을 한 틱 진행한다.
  GDScript는 읽기 함수와 신호로 표시만 하고 주행·명령 규칙을 따로 구현하지 않는다(`docs/design/godot-interface.md`).
- Bevy 판 키보드 통합 테스트는 `KeyboardInput` 메시지 경로를 쓴다.
  `ButtonInput`을 직접 press하면 PreUpdate에서 just_pressed가 지워져 비상 정지 테스트가 거짓 통과할 수 있다.

### 충돌과 감속

- HUD의 contact는 장애물과 약 2cm 이내를 뜻하며, 충돌 기록은 실제 이동이 막힐 때 발생한다.
- 중복 방지는 장애물마다 따로 한다. 해당 장애물에서 2cm 밖으로 떨어진 뒤 재충돌하면 다시 기록한다.
- 현재 `ContactState.reported`는 장애물 목록의 순서 번호를 사용한다.
  **동적 장애물 추가·삭제·재정렬 기능 개발 전에 안정적인 ID 적용을 먼저 검토한다.**
  현재 고정 장면에서는 즉시 수정할 사유로 보지 않았다.
- 일반 경로의 보수 검사도 최소 구간 `1e-4 m`와 부동소수점 오차에 따른 근사다.
  “최대 0.05mm 절대 보장” 또는 모든 조건의 완전한 관통 방지로 표현하지 않는다.
- 출발 때 이미 겹친 장애물은 겹침이 더 깊어지는 이동을 막되,
  일반 장애물과 같은 구간 검사를 하지 않고 검사 지점에서 확인하는 제한이 있다.
- 제동·관성·비상 감속은 서로 더해지는 성분이다(2-6부터 구름·공기 저항 성분도 더해진다).
  기본값·가속 없음·저속에서 최대 제동 약 3.3, 비상 정지만 약 5.3, 둘 다 약 8.3m/s²다.

## 보류된 정리 작업

- 초기 겹침 장애물의 구간 경로 검사 보강.
- 기어 입력에서 고정 크기 목록을 Vec으로 만드는 임시 할당 정리.
- 테스트에서만 호출하는 `hud_text()`, 정지 임계값 getter의 공개 범위 정리.
  테스트 사용처가 있으므로 “사용되지 않는 코드”로 단정해 삭제하지 않는다.
- `TrainingLog::events()`는 기록 조회 API이고, Rapier 선택 의존성은 호환성 검사용이다.
  현재 실행 경로에서 쓰지 않는다는 이유만으로 불필요한 잔재로 처리하지 않는다.

보류 항목을 사용자 요청 없이 전부 신규 작업으로 시작하지 않는다.

## 워크트리 충돌 재발 방지와 승인 범위

- 과거 다른 세션이 사용하는 develop 워크트리에서 docs 브랜치를 체크아웃해 커밋이 섞인 일이 있었다.
  작업 전 점유 상태를 확인하고, 다른 세션이 사용 중인 워크트리의 브랜치를 바꾸지 않는다.
- 기존 `docs/gear-rule-alignment`의 문서 커밋은 `94afe82`이고 main에 반영됐다.
  섞였던 끝점 `c3cb5b6`는 `backup/gear-rule-alignment-c3cb5b6`에 보존한 이력이 있다.
- 새 환경에서는 작업 시작 전에 브랜치·워크트리·미커밋 변경을 확인한다.
- main 직접 개발 및 test 워크트리 코드 수정 금지. 개발·커밋은 별도 작업 브랜치에서 한다.
- main 병합은 검증 결과 보고와 사용자 승인 후 `--no-ff`로 수행한다.
- push와 브랜치 삭제는 각각 사용자 요청·확인 없이는 하지 않는다.
- 함수 작성 시 기능 설명 주석을 달고, 작업 보고에 수정 파일과 줄을 명시한다.

## 새 환경에 전달할 시작 지시문

> AGENTS.md, docs/handoff.md, PLAN.md, README.md를 읽고 실제 Git 상태와 워크트리를 확인하세요.
> 인수인계 스냅샷과 실제 상태가 다르면 먼저 보고하세요. Godot으로 열 워크트리에서는 먼저
> cargo.exe build -p sim-godot를 하세요. 기존 체크나 test 체크아웃만으로 완료를 단정하지 말고,
> 사람 확인이 남은 항목(PLAN 각 절의 "남은 것")을 완료로 처리하지 마세요.
> main 병합·push·브랜치 삭제를 새로 승인받은 것으로 해석하지 마세요.
