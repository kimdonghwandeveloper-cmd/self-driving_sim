# 자율주행 셔틀 조작 훈련 시뮬레이터

[PLAN.md](PLAN.md)의 Phase 2 진행 중인 개발 버전입니다.
현재는 5m 격자 바닥 위의 차량 도형을 키보드로 운전하며, 차량 상태 머신(시동·기어·비상 정지)과
저속 운동 모델(가속·제동·관성·조향), 고정 장애물 충돌을 확인할 수 있습니다.
카메라는 차량 뒤쪽 위에서 따라갑니다.
HUD에 속도·가속도·바퀴 조향각·방향·위치, 장애물 접촉 여부·충돌 횟수, 마지막 명령의 수락/거부 사유가 표시되고,
명령 로그에도 그때의 속도가 함께 남습니다.
게임패드, 지도, 교육 시나리오, 기록 파일 저장은 아직 구현하지 않았습니다.
실제 차량의 성능이나 안전성을 검증하는 도구가 아닙니다.

## 개발용 키보드 조작 (임시 매핑)

| 동작 | 키 |
| --- | --- |
| 가속 / 제동 | `W` / `S` (보조: `↑` / `↓`, 동시 입력 시 제동 우선) |
| 조향 좌 / 우 | `A` / `D` (보조: `←` / `→`) |
| 기어 P / N / D / R | `I` / `K` / `J` / `L` (동시에 두 기어 키를 누르면 거부) |
| 시동 ON/OFF | `Enter` |
| 비상 정지 / 해제 | `Space` / `Backspace` |

매핑은 `config/input.toml`, 정지 판정 속도와 운동 모델 값(최고 속도, 가속·제동·관성·비상 감속도,
축간거리, 최대 조향각, 조향 속도, 조향 감도, 조향 중립 복귀 방식·복귀 속도)은 `config/vehicle.toml`에서 바꿉니다.
설정 파일이 없거나 잘못되면 경고 로그를 남기고 기본값을 사용합니다.
관성 감속도(`coast_decel_mps2`)는 0을 허용하며, 0이면 가속 입력이 없을 때 감속 없이 계속 굴러갑니다.
제동·관성·비상 감속도는 서로 더해지는 성분입니다(기본값에서 가속 없이 최대 제동 3.3, 비상 정지 5.3, 둘 다 8.3 m/s²).
상태 전이 규칙(모두 임시)은 다음과 같습니다.

- 기어 변경은 일반 자동변속 차량 규칙을 따릅니다(실제 셔틀 절차 확인 전 임시).
  N은 언제든, P는 정지 상태에서만 허용합니다. D/R은 정지 상태이거나 지금 움직이는 방향과 같을 때만
  허용합니다(전진 중 N→D 허용, 전진 중 D→R·N→R 거부).
- 시동 ON/OFF와 비상 정지 해제는 정지 상태에서만 허용합니다.
- 시동 OFF 또는 비상 정지 중에는 기어 변경과 가속이 차단되며, P/N에서도 가속은 0입니다.
- 비상 정지 해제 후에는 Ready + P로 돌아갑니다.
- 비상 정지 키를 누르고 있는 동안에는 해제·시동·기어 입력을 무시합니다. 해제는 비상 정지 키를 뗀 뒤 따로 누릅니다.
- D/R 모두 가속 키는 기어 방향으로 가속하고, 제동·관성·비상 감속은 항상 속도를 0 쪽으로만 줄입니다.
  P에서는 정지 상태를 유지합니다.
- 조향은 후륜축 기준 Kinematic Bicycle Model(임시 기준 모델)로 계산하며, 정지 중 조향은 바퀴 각도만 바꿉니다.
  조향 입력 중에는 입력 위치가 목표 조향각입니다. 입력을 놓으면 `steer_return = "auto"`(기본)는
  `steer_return_rate_deg_s` 속도로 0도로 돌아가고, `"hold"`는 그 각도를 유지합니다(실차 동작 미확정이라 설정값).
- 시작 위치 주변에 시험용 장애물(주황색 상자: 정면 30 m 벽, 오른쪽 앞 기둥, 왼쪽 앞 상자, 뒤쪽 15 m 벽)이 있습니다.
  차체(길이 4.2 m, 폭 1.8 m 상자, 임시 치수)가 장애물에 닿으면 관통하지 않고 그 자리에서 멈춥니다(속도 0).
  벽을 따라 미끄러지지 않으며, 이동이 실제로 막힌 순간에 충돌 1회를 교육 기록에 남기고 `training event: collision` 경고 로그를 출력합니다.
  HUD의 `Obstacle contact`는 장애물과 2 cm 안이면 `YES`이지만, 가까이 멈추거나 스쳐 지나가기만 하면 충돌로 세지 않습니다.
  밀고 있는 동안에는 다시 세지 않고, 그 장애물에서 2 cm 밖으로 떨어졌다가 다시 막히면 새 충돌로 셉니다.
  중복 판정은 장애물마다 따로 하므로 좁은 통로에서 다른 벽에 붙어 있어도 재충돌을 놓치지 않습니다. 높이는 보지 않습니다.
  회전 중 차체 모서리가 검사 지점 사이에서만 장애물을 스치는 경우도 경로를 보수적으로 나눠 검사해 막습니다.
  장애물과 겹친 채 시작하면(배치 오류 등) 그 장애물에서 빠져나가는 이동만 허용하고 더 깊이 들어가는 이동은 막으며,
  다른 장애물은 평소처럼 막습니다.
- 속도 값이 NaN/무한대이면 정지로 보지 않아, 정지가 필요한 전이를 모두 거부합니다(HUD에 `INVALID` 표시).

## 준비

- Rustup: `rust-toolchain.toml`이 Rust **1.99.1**, rustfmt, Clippy를 선택합니다.
- Windows: Visual Studio Build Tools의 **Desktop development with C++** 및 Windows SDK가 필요합니다. 애플리케이션 구현 언어는 Rust입니다.
- 설치 구성은 루트 `.vsconfig`에 기록했습니다. Visual Studio Installer에서 가져온 뒤 권장 구성 요소를 포함하여 설치합니다.
- 그래픽 드라이버와 데스크톱 세션이 필요합니다.
- RustRover에서 루트 `Cargo.toml`을 Cargo 프로젝트로 연결합니다.

```powershell
rustup show
cargo fetch --locked
cargo run --locked
```

첫 실행은 엔진 의존성 컴파일 때문에 오래 걸립니다. 도형이 나타나면 창의 닫기 버튼으로 종료합니다.
실행 작업 디렉터리는 저장소 루트로 지정합니다.

### WSL에서 작업하는 경우

이 저장소의 기본 실행 대상은 Windows MSVC입니다. WSL 터미널에서 Windows에
설치된 Rust를 사용할 때는 `rustup.exe`, `cargo.exe`, `rustc.exe`처럼 확장자를
붙입니다. 예를 들어 `cargo.exe run --locked`로 Windows 창을 실행할 수 있습니다.
RustRover의 Windows 터미널에서는 위 PowerShell 명령을 그대로 사용합니다.
WSL Linux용 Rust와 그래픽 환경은 별도로 설치·검증하지 않았습니다.

## 개발 명령

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo run --locked
cargo run --locked --release
```

`physics` 기능은 Bevy/Rapier 빌드 호환성 확인용 선택 의존성입니다.
기본 실행에는 Rapier 플러그인을 등록하지 않습니다.
입력은 이후 Bevy의 게임패드 API를 사용하며 별도의 gilrs 직접 의존성을 추가하지 않았습니다.

창과 렌더러를 자동 실행하고 약 5초 후 종료하려면 다음 명령을 사용합니다.
이 검증은 GPU 초기화와 실행 오류 확인용이며 화면의 시각적 검수를 대체하지 않습니다.

```powershell
cargo run --locked -- --smoke-test
```

## 로그

Bevy의 기본 로그는 터미널에 출력됩니다. PowerShell에서 상세 로그와 파일 저장을 설정할 수 있습니다.

```powershell
$env:RUST_LOG = 'info,self_driving_sim=debug,wgpu=warn'
$env:RUST_BACKTRACE = '1'
New-Item -ItemType Directory -Force logs
cargo run --locked 2>&1 | Tee-Object -FilePath logs/debug.log
```

Release도 `cargo run --locked --release`에 같은 환경변수를 적용합니다.
`logs/`와 `target/`은 버전 관리에서 제외합니다. `Cargo.lock`은 커밋하여 의존성 해석을 고정합니다.

## 구성과 기준

- `src/main.rs`: 진입점과 자동 종료 검증 옵션
- `src/app.rs`: Bevy 앱 구성, 설정 로드, 개발환경 확인용 3D 장면
- `src/config.rs`: `config/*.toml` 로드와 검증
- `src/input/`: 키보드 입력과 키 매핑, 동시 기어 요청 판정
- `src/vehicle/`: 차량 상태 머신(`state.rs`), 운동 모델(`dynamics.rs`), 충돌 판정(`collision.rs`), Bevy 연결(`controller.rs`)
- `src/training/`: 교육 기록(현재는 충돌 이벤트만 메모리에 보관)
- `src/scene.rs`: 개발용 3D 장면(바닥, 장애물, 차량 도형, 따라가는 카메라)
- `src/ui/`: 개발용 HUD
- `config/{vehicle,input}.toml`: 시뮬레이터 임시 설정값
- `assets/{models,textures,maps}/`: 추후 에셋 경로
- [docs/development.md](docs/development.md): 선정 버전, 개발 PC, 검증 결과
- [docs/vehicle-assumptions.md](docs/vehicle-assumptions.md): 실차 사양 가정과 미확인 위험(사륜 조향·특수 구동 등)

나머지 설정 파일과 도메인 모듈은 해당 기능을 구현할 때 추가합니다.
엔진 설정은 [Bevy 공식 가이드](https://bevy.org/learn/quick-start/getting-started/setup/)와
[0.19.1 기능 목록](https://docs.rs/crate/bevy/0.19.1/features)을 참고했습니다.
