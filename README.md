# 자율주행 셔틀 조작 훈련 시뮬레이터

[PLAN.md](PLAN.md)의 Phase 4(경로 지정 OSM 코스 생성) 진행 중인 개발 버전입니다.
키보드나 Xbox 유형 게임패드로 셔틀을 운전하며, 차량 상태 머신(시동·기어·비상 정지)과
운동 모델(가속·제동·관성·조향, 최고 속도 40 km/h·출력 한계·저항·횡가속도 상한), 고정 장애물·연석 충돌을 확인할 수 있습니다.
화면은 입석 조작자 1인칭 시점(전·후·좌·우 전환)입니다([입석 조작자 시점](#입석-조작자-시점-phase-3-1-임시값)).
HUD에 속도·가속도·바퀴 조향각·방향·위치, 장애물 접촉 여부·충돌 횟수, 마지막 명령의 수락/거부 사유가 표시되고,
명령 로그에도 그때의 속도가 함께 남습니다.
HUD 아래쪽에는 차량 파라미터 보정용 회전반경·예상 정지거리와 제동·가속 시험 측정 결과가 표시됩니다([보정 도구](#차량-파라미터-보정-도구-임시값)).
인자 없이 실행하면 메인 화면이 열리고, 전국 배경 지도에서 지역 도로를 받아 경로를 찍으면 굽은 길·교차로가 있는
가상 교육 코스를 만들어 주행할 수 있습니다([메인 화면](#메인-화면-phase-4-5a), [경로 선택 화면](#경로-선택-화면-phase-4-5-4-5b-전국-배경-지도)).
교육 시나리오와 기록 파일 저장(Phase 5)은 아직 구현하지 않았습니다.
실제 차량의 성능이나 안전성을 검증하는 도구가 아닙니다.

### 진행 상황 (2026-11-07, main 기준)

[PLAN.md](PLAN.md)의 체크박스와 같은 기준입니다. 자동 시험과 Codex 검토를 통과하고 main에 병합된 항목만 완료로 봅니다.

| 단계 | 상태 |
| --- | --- |
| Phase 1~3 (기반·운동 모델·카메라·여수 코스) | 대부분 완료. 게임패드 실기 확인, 수동 시야 확인 등은 남음 |
| 2-6 중속 운동 모델, 4-1 굽은 길, 4-2 교차로, 4-3 경로 탐색, 4-4 지도 받기 | 완료(수동 주행·게임패드 확인은 남음) |
| 4-5 경로 선택 화면 | 상태 추가·지역 선택·주행 시작 완료. 마우스 조작은 사람 확인 전 |
| 4-5a 메인 화면, 4-5b 전국 배경 지도 | main에 병합, 자동 시험 통과. Codex 재검증 철회, 사람의 화면 확인 전 |
| 4-5G G-1 Geofabrik 지역 추출 | 코드와 fixture 시험 완료. 실제 한국 PBF 검증·측정 전 |
| 4-6 이후, Phase 5~6 | 시작 전 |

## 개발용 키보드 조작 (임시 매핑)

| 동작 | 키 |
| --- | --- |
| 가속 / 제동 | `W` / `S` (보조: `↑` / `↓`, 동시 입력 시 제동 우선) |
| 조향 좌 / 우 | `A` / `D` (보조: `←` / `→`) |
| 기어 P / N / D / R | `I` / `K` / `J` / `L` (동시에 두 기어 키를 누르면 거부) |
| 시동 ON/OFF | `Enter` |
| 비상 정지 / 해제 | `Space` / `Backspace` |
| 시작 위치 복귀 (정지 상태에서만) | `R` |
| 시점 전방 / 후방 / 좌측 / 우측 | `1` / `2` / `3` / `4` |
| 개발용 외부 디버그 카메라 켜기/끄기 | `C` |

## 입석 조작자 시점 (Phase 3-1, 임시값)

- 기본·리셋 시점은 사용자 지정 기준 사진 `Car_photo/KakaoTalk_261030_131310_61.jpg`의 구도
  (넓은 창, 양옆의 두꺼운 어두운 내장재, 화면 아래 약 1/4의 회색 선반과 원형 개구부 3개)를 단순 도형으로 재현합니다.
  사진은 런타임 텍스처로 쓰지 않으며, 실내 형상·눈 위치·FOV는 사진 구도에 맞춘 **실측 전 임시값**입니다.
- 네 방향은 같은 눈 위치에서 시선만 90°/180° 돌립니다. 기어·조향·가속·차량 자세에 영향을 주지 않고,
  후방을 봐도 조향은 반전되지 않으며 R에 들어가도 시점은 자동으로 바뀌지 않습니다.
  여러 방향 키를 동시에 누르면 현재 방향을 유지하고, 시작 위치 복귀(`R`)가 적용되면 전방으로 돌아갑니다.
- HUD 둘째 줄 `View: REAR window | Gear: D | Moving: FORWARD`처럼 바라보는 방향과 실제 부호 있는 속도로 판정한
  이동 방향(N 관성 이동 포함)을 함께 표시합니다. 전방이 아닌 시점에는 `(not facing vehicle front)`가 붙습니다.
- 외관 차체는 별도 렌더 레이어에 있어 1인칭 시점에서는 보이지 않습니다. `C`로 켜는 외부 카메라는 개발 확인용이며 훈련 시점이 아닙니다.
- 후방 창 주변은 전방 구성을 180° 돌린 것입니다(양방향 외형 가정, 미확인). 게임패드 시점 버튼은 아직 지정하지 않았습니다.
- `config/camera.toml`의 `[operator_view]`에서 눈 위치(`eye_m`, 차량 로컬 m), `pitch_deg`, 수직 FOV(`vertical_fov_deg`, 30..=110),
  `near_clip_m`(0.01..=0.5)을 바꿉니다. 파일이 없거나 값이 잘못되면(NaN·무한대·범위 밖 포함) 경고 로그 후 기본값을 씁니다.

## 개발용 테스트 코스 (Phase 3-2, 임시값)

`--course <이름>`으로 코스를 고릅니다(기본 `training`). 모르는 이름이면 경고 후 `training`으로 실행합니다.
지도(OSM) 자료 없이 실행되며, 모든 치수는 **훈련용 임시값**이고 실제 도로·공원 통로가 아닙니다.

| 코스 | 구성 (시작: 원점, 전방 -Z) |
| --- | --- |
| `training` | 폭 6 m 직선 차로(z=+6 ~ -66), 양옆 연석(충돌 경계), 가장자리 흰 선(표시만), 정지선 z=-20, 차로 오른쪽 고정 장애물 z=-32, 좁은 통로 z=-42 ~ -54(폭 3.0 m = 차체 폭 1.8 m + 양옆 0.6 m), 앞·뒤 끝 벽 |
| `calibration` | Phase 2-5에서 쓰던 넓은 평지와 장애물 4개(회전반경·제동 시험용) |
| `map:<선택 파일>` | OSM 중심선에서 만든 지도 코스. 여수 가상 교육 코스는 `map:assets/maps/yeosu-expo/course.toml`(way 532526349, 약 292 m, 임시 폭 4 m; 실제 운행 경로 아님). 굽은 길 확인용은 `map:assets/maps/yeosu-expo/curved-course.toml`(way 529495041, 약 321 m). 교차로를 지나는 경로 파일 예제는 `map:assets/maps/yeosu-expo/route-junctions.toml`(Phase 4-3, 교차로 4곳, 임시 폭 4 m). 형식과 제약은 `docs/map-data.md` 참고. 읽지 못하면 경고 후 `training`으로 실행 |

- 화면 상자와 충돌 판정은 `crates/sim-core/src/world/course.rs`의 같은 정의에서 만들어서 어긋나지 않습니다. 흰 선은 시각 표시일 뿐이고 실제 경계는 연석입니다.
- 시작 위치 복귀(`R`)는 선택한 코스의 시작 자세로 되돌립니다.

```powershell
cargo run --locked -- --course calibration
```

## 게임패드 조작 (임시 매핑)

| 동작 | 입력 |
| --- | --- |
| 가속 / 제동 | 왼쪽 스틱 위 / 아래 (D=전진, R=후진 방향으로 가속, 아래는 D/R 모두 제동) |
| 조향 좌 / 우 | 오른쪽 스틱 좌 / 우 |
| 기어 P / R / N / D | `Y` / `B` / `A` / `X` (동시에 두 기어 버튼을 누르면 거부) |
| 시동 ON/OFF | `Start` |
| 비상 정지 / 해제 | `RB` / `Back` |

`config/input.toml`의 `[gamepad]`에서 버튼과 데드존을 바꿀 수 있습니다.
기본 배치는 PLAN.md 0-1절과 같습니다. `layout = "triggers"`로 바꾸면 왼쪽 스틱 좌우=조향, `RT`=가속, `LT`=제동을 씁니다.
시작 위치 복귀는 키보드에서만 합니다. 게임패드 여러 대가 연결되면 한 대만 사용합니다.
새로 선택된 게임패드(대체 게임패드, 재연결 포함)는 모든 입력을 놓아 중립이 된 뒤에 활성화되며,
그 전에는 HUD에 `release all inputs to activate`가 표시되고 비상 정지 입력만 받습니다.
HUD에 게임패드 연결 상태, 지금 조향·가속에 쓰는 장치(`Active input`), 게임패드 조작 안내가 표시됩니다.

키보드와 게임패드를 함께 쓸 때의 규칙은 다음과 같습니다.

- 조향·가속은 한 장치의 값만 씁니다. 지금 장치의 입력이 없어진 상태에서 다른 장치를 조작하면 장치가 바뀌고,
  두 장치를 동시에 조작하면 지금 장치를 유지합니다.
- 제동은 어느 장치든 더 강한 값을 적용하고, 제동이 있으면 가속은 0입니다.
- 비상 정지는 어느 장치든 우선합니다. 한 장치의 비상 정지가 눌려 있는 동안에는 다른 장치의 해제·시동·기어 입력도 무시합니다.
- 두 장치의 시동 입력이 같은 프레임에 겹치면 한 번만 적용하고, 서로 다른 기어를 요청하면 거부합니다.
- 게임패드 연결이 끊기면 그 프레임부터 그 게임패드의 입력이 사라집니다. 게임패드로 조작 중이고 차량이 움직이고 있었다면
  비상 정지로 멈추며, 이후 `Backspace` 또는 다시 연결한 게임패드로 해제합니다.
  키보드로 조작 중이었거나 정지 상태였다면 차량 상태를 바꾸지 않습니다.

## 설정과 상태 전이 규칙

매핑은 `config/input.toml`, 정지 판정 속도와 운동 모델 값(최고 속도, 가속·제동·관성·비상 감속도,
질량·구동 출력·구름/공기 저항, 축간거리, 최대 조향각, 조향 속도, 조향 감도, 조향 중립 복귀 방식·복귀 속도,
언더스티어 계수·횡가속도 상한)은 `config/vehicle.toml`에서 바꿉니다.
전진 최고 속도 40 km/h는 실차 사양이고, 나머지 값은 실차 검증 전 임시값입니다.
설정 파일이 없거나 잘못되면 경고 로그를 남기고 기본값을 사용합니다.
관성 감속도(`coast_decel_mps2`)와 저항 값은 0을 허용하며, 모두 0이면 가속 입력이 없을 때 감속 없이 계속 굴러갑니다.
제동·관성·비상·저항 감속도는 서로 더해지는 성분입니다(기본값·저속에서 가속 없이 최대 제동 약 3.3, 비상 정지 약 5.3,
둘 다 약 8.3 m/s². 속도가 높으면 공기 저항이 조금 더해집니다).
고속에서는 출력 한계로 가속이 줄고, 언더스티어 식과 횡가속도 상한 때문에 같은 조향각에서도 덜 돕니다.
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
  회전 중 차체 모서리가 검사 지점 사이에서만 장애물을 스치는 경우도 경로를 보수적으로 나눠 검사합니다.
  다만 최소 검사 구간(0.1 mm)에 따른 근사와 부동소수점 오차가 있어, 절대적인 관통 방지를 보장하지는 않습니다.
  장애물과 겹친 채 시작하면(배치 오류 등) 그 장애물에서 빠져나가는 이동만 허용하고 더 깊이 들어가는 이동은 막으며,
  다른 장애물은 평소처럼 막습니다.
- 속도 값이 NaN/무한대이면 정지로 보지 않아, 정지가 필요한 전이를 모두 거부합니다(HUD에 `INVALID` 표시).

## 차량 파라미터 보정 도구 (임시값)

`config/vehicle.toml` 값을 바꾼 뒤 앱을 다시 실행해 같은 코스에서 가속·제동·조향 반응을 비교하는 용도입니다.
표시되는 수치는 모두 시뮬레이터 임시값으로 계산한 결과이며 실차 성능과 같다고 보지 않습니다.

- `Turn radius (rear axle)`: 현재 바퀴 각도의 후륜축 중심 회전반경(축간거리 / tan 조향각). 직진이면 `straight`.
  `Min at full lock`은 최대 조향각의 최소 회전반경입니다.
- `Full-brake stop estimate`: 지금 속도에서 가속 없이 최대 제동할 때의 예상 정지거리 v²/(2a).
  a는 최대 제동 + 관성 감속(비상 정지 중이면 비상 감속도 더함)이며, 고정 주기 적분 결과와 한 단계 이동량 정도 차이가 날 수 있습니다.
- 제동 시험: 움직이는 중 가속 없이 제동 키를 누르거나 비상 정지하면 시작해, 속도가 0이 되면
  `Last stop`에 정지거리·시작 속도·시간을 남깁니다. 멈추는 순간까지 제동(또는 비상 정지)을 유지하지 않았거나
  (정지 직전에 풀어 관성으로 멈춘 경우 포함) 장애물에 막혀 멈추면 결과를 남기지 않습니다.
- 가속 시험: 정지 상태에서 가속하면 시작해, 최고 속도에 닿거나(`top speed`) 가속을 떼면(`released`)
  `Last accel`에 도달 속도·시간·거리를 남깁니다. 장애물에 막혀 이동이 잘리면(이미 부딪힌 벽을 계속 미는 경우 포함) 결과를 남기지 않습니다.
- 결과를 버리는 기준은 실제 이동 막힘입니다. 장애물 2 cm 안에 있을 뿐 막히지 않은 시험(`Obstacle contact: YES`)은 정상 결과로 남깁니다.
- `R`: 정지 상태에서 차량을 시작 자세(원점, 전방 -Z, 바퀴 정렬)로 되돌립니다. 시동·기어·비상 정지 상태와
  끝난 시험 결과는 유지하고, 진행 중인 시험과 장애물 접촉 기록만 초기화합니다. 움직이는 중에는 거부합니다.
  비상 정지 키를 누르고 있는 동안에는 다른 명령처럼 무시합니다.
- 설정은 앱 시작 때 읽으므로 값을 바꾼 뒤에는 앱을 다시 실행합니다(실행 중 재적용은 없음).

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
게임패드 입력은 Bevy의 게임패드 API를 사용합니다. `3d`/`ui` 기능에 포함된 `default_platform`이 `bevy_gilrs`를 켜므로
별도의 기능 설정이나 gilrs 직접 의존성은 없습니다.
`ureq`(=3.5.1)는 Phase 4-4 지도 받기용 HTTP 클라이언트입니다. 기본 기능(rustls + ring 암호, webpki 루트 인증서, gzip)을
쓰며, cmake·NASM이 필요한 aws-lc-rs는 쓰지 않습니다. ring은 `cc`로 C/어셈블리를 빌드하는데, `cc`는 이미
Bevy 의존성(blake3)이 쓰고 있어 새 빌드 도구가 필요하지 않습니다.

### 메인 화면 (Phase 4-5a)

인자 없이 `cargo run --locked`로 실행하면 메인 화면에서 시작합니다. 개발 코스(training·calibration) 주행, 저장된 경로
파일(`cache/routes/`, 최근 8개) 주행, 맵 생성하기(전국 지도 경로 선택), 종료를 고릅니다. 주행·경로 선택 화면에서
`Esc`를 3초 안에 두 번 누르면 메인 화면으로 돌아옵니다. 다시 주행하면 차량 상태·위치·기록·시점이 처음으로 돌아갑니다.
`--course`, `--pick-route`, `--smoke-test`, `--capture-views`를 주면 메인 화면을 건너뜁니다.

### 경로 선택 화면 (Phase 4-5, 4-5b 전국 배경 지도)

메인 화면의 "Create map route" 또는 `--pick-route`로 들어갑니다. 배경에 OSM 표준 지도 타일을 깔고(화면에 보이는 타일만
받고 `cache/tiles/`에 캐시), 받은 도로 그래프를 그 위에 선으로 그립니다. 타일은 보기 전용이며 코스는 받은 도로 자료로만
만듭니다. 타일을 받지 못하면 회색으로 비워 두고 나머지 기능은 그대로 됩니다. 출처 표기는 오른쪽 아래에 늘 보입니다.
`--smoke-test`·`--capture-views` 실행에서는 타일을 받지 않습니다(OSM 타일 정책).

| 조작 | 입력 |
| --- | --- |
| 화면 이동 / 확대·축소 | 왼쪽 끌기 / 휠 |
| 이 지역 도로 받기(화면 가운데 1.5 km, 이미 받은 노란 테두리 안이면 캐시만) | `L` 또는 버튼 |
| 맞춤 보기: 전국 / 받은 지역 / 경로 | `K` / `G` / `F` 또는 버튼 |
| 점 추가(시작 → 경유 → 끝) | 왼쪽 클릭(도로를 받은 뒤) |
| 점 지우기 | 오른쪽 클릭(가까운 점), `Backspace`(마지막 점), `C`(모두) |
| 주행 시작 | `Enter` 또는 버튼 |
| 메인 화면 | `Esc` 두 번 |

경로 탐색에 쓰는 도로는 진한 파랑, 그 밖은 회색입니다. 화면에는 경로 길이, 교차로 수, 폭과 그 출처, 경고가 표시됩니다.
경고는 임시 폭, 3 m 미만의 좁은 폭, 돌 수 없는 꺾임, 거부된 교차로 등입니다. 주행을 시작하면 경로 파일을
`cache/routes/route-<UTC 시각>.toml`에 저장하고 그 코스로 주행합니다. 저장한 파일은 메인 화면이나
`--course map:cache/routes/<파일>`로 다시 열 수 있고, 같은 코스가 나옵니다.
`--pick-route`의 지역은 `yeosu`(저장된 여수 스냅샷), 스냅샷 파일 경로, `latlon:<위도>,<경도>`(주변 1.5 km를 Overpass에서
받기) 중 하나입니다. 장소 이름 검색은 없습니다.

```powershell
cargo run --locked -- --pick-route yeosu
```

`--pick-points "34.75264,127.74712;34.74972,127.74816"`로 점을 미리 찍어 둘 수 있습니다. `--auto-start`를 함께 주면
지도를 불러오는 대로 주행을 시작합니다. 이 옵션은 화면 전환 자동 검증용입니다.

### 지도 받기 (Phase 4-4, 개발용)

사용자가 요청할 때만 Overpass API에서 bbox(남,서,북,동) 안의 도로를 받아 `cache/maps/`에 저장합니다.
`cache/`는 저장소에서 제외합니다. 창은 열리지 않습니다. 대한민국 범위 밖이거나 한 변이 2 km를 넘거나
응답이 20 MB를 넘으면 오류로 끝납니다. `--offline`을 주면 네트워크를 쓰지 않고 캐시만 확인합니다.

```powershell
cargo run --locked -- --fetch-map 34.7480,127.7460,34.7590,127.7600
cargo run --locked -- --fetch-map 34.7480,127.7460,34.7590,127.7600 --offline
```

### Geofabrik 지역 추출 (Phase 4-5G G-1, 개발용)

로컬에 있는 Geofabrik `.osm.pbf`에서 bbox(남,서,북,동) 주변 도로만 잘라 `cache/extracts/`에 저장합니다.
네트워크를 쓰지 않고 창도 열리지 않습니다. 전국 파일이면 허용 도로 전체를 메모리에 올리므로 시간이 걸립니다.
결과 폴더의 `graph.osm.json`은 `--pick-route <파일>`로 열 수 있고, 경계에서 연결이 끊긴 도로를 지나는 경로는 거부됩니다.
자세한 규칙은 `docs/map-data.md`의 "Geofabrik 지역 추출"을 참고하세요.

```powershell
cargo run --locked -- --extract-pbf <south-korea.osm.pbf> --bbox 34.7480,127.7460,34.7590,127.7600
cargo run --locked -- --audit-extract <세대 폴더> --pbf <south-korea.osm.pbf>
cargo run --locked -- --compare-overpass assets/maps/yeosu-expo/graph-2026-11-06.osm.json --extract <세대 폴더> --pbf <south-korea.osm.pbf>
```

창과 렌더러를 자동 실행하고 약 5초 후 종료하려면 다음 명령을 사용합니다.
이 검증은 GPU 초기화와 실행 오류 확인용이며 화면의 시각적 검수를 대체하지 않습니다.

```powershell
cargo run --locked -- --smoke-test
```

기준 사진과 비교할 시점 캡처(HUD 없는 전·후·좌·우·외부 디버그 + HUD 있는 전방)를 PNG로 저장하고 종료하려면
다음 명령을 사용합니다. `--window`로 창 크기를 정하며, `1498x700`은 `61.jpg`와 같은 약 2.14:1 화면비입니다.
(Windows 화면 배율에 따라 저장 이미지의 픽셀 크기는 달라질 수 있습니다.)

```powershell
cargo run --locked -- --capture-views target/view-captures --window 1498x700
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
- `src/input/`: 키보드·게임패드 입력과 매핑, 장치 입력 통합, 동시 기어 요청 판정
- `src/calibration.rs`: 차량 파라미터 보정 도구(회전반경·정지거리 계산, 제동·가속 시험 측정, 시작 위치 복귀)
- `src/vehicle/`: 차량 상태 머신(`state.rs`), 운동 모델(`dynamics.rs`), 충돌 판정(`collision.rs`), Bevy 연결(`controller.rs`)
- `src/training/`: 교육 기록(현재는 충돌 이벤트만 메모리에 보관)
- `src/scene.rs`: 개발용 3D 장면(바닥, 장애물, 차량 도형)
- `src/camera/`: 입석 조작자 시점(네 방향 전환, 실내 내장재)
- `src/menu.rs`: 메인 화면(코스 선택, `Esc` 두 번 복귀)
- `src/picker/`: 경로 선택 화면(`mod.rs`), 화면 좌표·경로 계산(`model.rs`), OSM 배경 타일 받기·캐시(`tiles.rs`)
- `src/world/`: 코스(`course.rs`), OSM 지도 코스(`map.rs`), 굽은 길 형상(`road_geom.rs`), 교차로(`junction.rs`),
  도로 그래프(`osm_graph.rs`), 경로(`route.rs`, `route_plan.rs`), Overpass 받기(`map_fetch.rs`),
  Geofabrik 추출·감사·비교(`pbf_extract.rs`, `pbf_audit.rs`, `pbf_compare.rs`), 스냅샷 읽기(`road_snapshot.rs`), 시험 주행(`test_drive.rs`)
- `src/ui/`: 개발용 HUD
- `config/{vehicle,input,road_graph}.toml`: 시뮬레이터 임시 설정값(`road_graph.toml`은 경로 탐색 허용 도로·폭 기본값)
- `assets/{models,textures,maps}/`: 추후 에셋 경로
- [docs/development.md](docs/development.md): 선정 버전, 개발 PC, 검증 결과
- [docs/vehicle-assumptions.md](docs/vehicle-assumptions.md): 실차 사양 가정과 미확인 위험(사륜 조향·특수 구동 등)

나머지 설정 파일과 도메인 모듈은 해당 기능을 구현할 때 추가합니다.
엔진 설정은 [Bevy 공식 가이드](https://bevy.org/learn/quick-start/getting-started/setup/)와
[0.19.1 기능 목록](https://docs.rs/crate/bevy/0.19.1/features)을 참고했습니다.

## Godot 연결과 첫 import 검사

Windows에서 확장을 빌드한 뒤 다음 명령으로 초기화합니다.

```powershell
cargo.exe build --locked -p sim-godot
& '<Godot 콘솔 실행 파일>' --headless --path godot --import --frame-delay 1000
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/smoke_test.gd
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/presentation_smoke.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/drive_replay_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/map_course_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/menu_flow_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/route_save_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/goal_reset_test.gd
```

`map_course_test.gd`는 저장소의 여수 경로 코스(`map:assets/maps/yeosu-expo/route-junctions.toml`)를 Godot에서 만들고 몇 초 주행합니다(GD-3).
`menu_flow_test.gd`는 메인 메뉴 → 코스 선택 → Rust 차량 주행 → Esc 메뉴 복귀를, `route_save_test.gd`는 오프라인으로 경로를 찍어 저장하고
그 저장 경로로 차량이 준비되는지 확인합니다(GD-4, 시험 파일은 끝에 지움).
`goal_reset_test.gd`는 지도 코스의 경로 끝 목표 표시, 주행 후 리셋 시 경로 시작 자세 복원, 없는 지도 파일을 열 때
개발 코스(training) 대체를 확인합니다(4-6).
프레임 시간은 실제 창에서 잽니다(헤드리스 아님, 측정만 하고 실패로 끝내지 않음).

```powershell
& '<Godot 콘솔 실행 파일>' --path godot --resolution 1280x720 --script res://tests/frame_time_probe.gd -- --seconds=15
```

Godot 메인 메뉴의 코스 목록에서 개발 코스, 저장소 지도 코스, 경로 선택 화면이 저장한 경로(`cache/routes`)를 고를 수 있습니다.

`drive_replay_test.gd`는 정해진 입력 열을 `SimVehicle`의 실제 물리 틱으로 재생한 위치·속도·기어가
Rust 단독 재생(`SimReplay.run`)과 같은지 비교합니다(GD-2). 같은 열의 기대 표본은 `sim-core` 시험에도 고정되어 있습니다.

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
