# 셔틀 조작 훈련 시뮬레이터

**전국 OSM 지도에서 도로를 선택하고, 가상 셔틀을 직접 운전하는 Windows 시뮬레이터입니다.**

Godot 4로 주행 화면을 그리고 Rust로 차량 운동·충돌·도로망·경로 재생을 계산합니다.
키보드와 Xbox 유형 게임패드를 지원하며, 기본 조작·공간 판단·돌발 상황 훈련 결과를 기록합니다.

[**다운로드**](https://github.com/kimdonghwandeveloper-cmd/self-driving_sim/releases/latest) ·
[첫 주행](#첫-주행) · [지도에서 경로 만들기](#지도에서-경로-만들기) ·
[개발 환경](#소스에서-실행하기) · [개발 계획](PLAN.md) ·
[문제 제보](https://github.com/kimdonghwandeveloper-cmd/self-driving_sim/issues)

> 현재는 시연·개발 단계입니다. 차량 수치와 평가 기준에는 임시값이 포함되어 있으며, 실제 차량 검증이나 인증 교육을 대체하지 않습니다.

## 주요 기능

| 기능 | 할 수 있는 일 |
| --- | --- |
| 전국 OSM 도로 | 도시 바로가기·좌표로 지역을 열고 최대 10 km 경로 생성 |
| 블록 기반 경로 선택 | 교차로 사이 도로와 진행 방향을 확인해 출발·경유·도착 지정 |
| 3D 주행 환경 | 도로·교차로·연석과 주변 건물 생성, 여수 사용자 제작 배경 코스 |
| 차량 조작 | 시동·P/N/D/R·가속·제동·조향·비상 정지, 연석·장애물 충돌 |
| 시점과 계기판 | 전·후·좌·우 1인칭, 외부 추종 시점, 속도·기어·경로 안내 |
| 교육 시나리오 | A 기본 조작 / B 공간 판단 / C 돌발 상황, 결과 JSON 저장 |
| 저장과 재생 | 선택한 경로와 지도 판본을 보존해 같은 코스 다시 열기 |
| Windows 배포 | 독립 실행 번들, 시작 시 서명·해시를 검증하는 업데이트 |

## 다운로드와 실행

1. [GitHub Releases](https://github.com/kimdonghwandeveloper-cmd/self-driving_sim/releases/latest)에서 `Simulator-Windows-v*.zip`을 받습니다.
2. 쓰기 가능한 폴더에 **ZIP 전체를 압축 해제**합니다.
3. 폴더 안의 **`Simulator.exe`**를 실행합니다.

배포본에는 Godot 런타임·Rust DLL·전국 지도 자료가 포함됩니다. Rust나 Godot을 별도로 설치할 필요는 없습니다.
EXE만 다른 폴더로 옮기면 실행되지 않습니다. Windows 10/11을 대상으로 하며, GPU·메모리별 최소 사양과 별도 PC 최종 검수는 아직 확정하지 않았습니다.

공개 배포본은 시작할 때 업데이트를 확인합니다. 업데이트가 실패하면 기존 버전을 실행하며, 주행 중에는 교체하지 않습니다.
개발용 패키지의 업데이트 설정은 공개 배포본과 다를 수 있습니다. 자세한 운영 방식은 [Windows 업데이트 안내](docs/windows-updates.md)를 참고하세요.

## 첫 주행

처음에는 지도 설정이 필요 없는 **`training`** 코스로 조작을 확인하세요.

1. 메인 메뉴에서 코스를 고르고 주행 화면에 들어갑니다.
2. `Enter`로 시동을 켜고 `J`로 **D**를 선택합니다.
3. `W`로 가속하고 `A` / `D`로 조향합니다. `S`로 제동합니다.
4. 정지한 뒤 `L`로 **R**을 선택하면 후진할 수 있습니다. 후진도 가속은 `W`, 제동은 `S`입니다.
5. `Esc`로 메뉴에 돌아갑니다.

### 키보드

| 동작 | 키 |
| --- | --- |
| 가속 / 제동 | `W` / `S` 또는 `↑` / `↓` |
| 좌 / 우 조향 | `A` / `D` 또는 `←` / `→` |
| 기어 P / N / D / R | `I` / `K` / `J` / `L` |
| 시동 켜기·끄기 | `Enter` |
| 비상 정지 / 해제 | `Space` / `Backspace` |
| 시작 위치로 복귀 | 정지 상태에서 `R` |
| 전방 / 후방 / 좌측 / 우측 시점 | `1` / `2` / `3` / `4` |
| 외부 추종 시점 | `C` |
| 메뉴로 돌아가기 | `Esc` |

시동·기어·비상 정지 상태에 따라 입력이 거부될 수 있으며 사유는 화면에 표시됩니다.
비상 정지 해제 후에는 P 상태로 돌아가므로 D 또는 R을 다시 선택하세요.

### 게임패드

기본 배치는 **왼쪽 스틱 위/아래 = 가속/제동**, **오른쪽 스틱 좌/우 = 조향**입니다.
기어는 `Y=P`, `B=R`, `A=N`, `X=D`, 시동은 `Start`, 비상 정지는 `RB`, 해제는 `Back`입니다.
연결 직후에는 모든 입력을 놓아 중립으로 만든 뒤 조작하세요. 매핑은 [config/input.toml](config/input.toml)에서 변경합니다.

## 지도에서 경로 만들기

1. 메뉴에서 **경로 지도**를 열고 도시 바로가기 또는 좌표로 원하는 지역을 엽니다.
2. 도로를 클릭하고 후보의 **도로 이름·진행 방향 화살표**를 확인합니다. 겹친 도로는 후보 목록에서 고릅니다.
3. **출발로 지정**을 누른 뒤 다른 도로를 골라 **도착으로 지정**합니다.
4. 필요하면 **더 보기 · 경유 / 저장 / 예제**에서 경유 도로를 추가합니다.
5. **경로 확인**으로 연결·차량 안내선을 확인하고 **주행 시작**을 누릅니다.

경로는 자동 저장되며 메뉴에서 다시 열 수 있습니다. 선택 연결이 실패하면 기존 선택을 유지하고 문제 도로를 강조합니다.
진행 방향이나 연결 상태를 확인해 다시 선택하세요. 형상 검사에 실패한 경로를 임의의 다른 길로 바꾸지 않습니다.

- **범위:** 전국 어디서나 지역 자료를 열어 최대 10 km 경로를 만듭니다. 전국 전체를 한 번에 3D로 생성하는 방식은 아닙니다.
- **내부 도로:** 원본 OSM에 없는 통로는 자동으로 생기지 않습니다. 자료에 없는 통로는 지역 보정이나 사용자 제작 코스로 보완합니다.
- **탐색 규칙:** 일방통행·회전 제한·차량 형상 검사는 유지됩니다. 제한 도로 포함 정책은 자료와 경로 설정에 따릅니다.
- **오프라인:** 내장 자료로 도로·경로 생성과 주행이 가능합니다. 배경 지도 타일과 전국 장소 이름 검색에는 인터넷이 필요합니다.
- **옛 저장 파일:** `road-network-v3` / `block-guide-v1` 블록 경로는 새 판본으로 다시 확인·저장해야 합니다. 기존 `route-v1`은 유지됩니다.

실제 엑스포·강남 표본의 검증 범위는 [도로·교차로 검토 기록](docs/short-block-junctions-review.md)에 있습니다.

## 코스와 교육 시나리오

| 항목 | 용도 |
| --- | --- |
| `training` | 직선·정지선·좁은 통로·고정 장애물에서 기본 조작 |
| `calibration` | 가속·제동·회전반경 측정과 차량 설정 비교 |
| 여수 엑스포 / 컨벤션 통로 | OSM 또는 사용자 제작 배경을 사용하는 고정 교육 코스 |
| 저장 경로 | 지도에서 만든 코스를 다시 열어 주행 |
| 시나리오 A / B / C | 기본 조작 / 공간 판단 / 돌발 상황 훈련 |

교육 시나리오는 화면 안내에 따라 진행하며 결과를 `logs/scenarios/`에 JSON으로 저장합니다.
점수는 임시 기준입니다. 일반 지도 자유 주행에 시나리오 점수가 자동 적용되지는 않습니다.
시나리오 구성은 [시나리오 안내](docs/scenarios.md), 수동 검수는 [시연 검수 가이드](docs/demonstration-inspection-guide.md)를 참고하세요.

## 소스에서 실행하기

Windows 개발 환경에 다음을 준비합니다.

- **Rust 1.99.1** 및 rustfmt·Clippy — [rust-toolchain.toml](rust-toolchain.toml)로 고정
- **Visual Studio Build Tools**의 C++ 데스크톱 개발 도구와 Windows SDK
- **Godot 4.7.2 표준판** — .NET 에디터 불필요

저장소 최상위에서 실행합니다.

```powershell
git clone https://github.com/kimdonghwandeveloper-cmd/self-driving_sim.git
cd self-driving_sim
cargo.exe fetch --locked
cargo.exe build --locked -p sim-godot
```

Godot에서 `godot/project.godot`을 열고 `F5`로 실행합니다. 기본 교육 코스는 전국 자료 없이도 사용할 수 있습니다.
지도 기능에는 Git에 포함되지 않는 `data/korea/`가 필요합니다. [자료 생성·재사용 방법](docs/developer-guide.md#전국-지도-자료-만들기-plan-4-11)을 따라 준비하세요.

> DLL은 **Godot을 여는 워크트리의** `target/debug/sim_godot.dll`을 사용합니다. 다른 워크트리에서 빌드한 것만으로는 연결되지 않습니다.

## 검증과 배포

```powershell
cargo.exe fmt --all -- --check
cargo.exe clippy --locked --all-targets --all-features -- -D warnings
cargo.exe test --locked --all-features
```

화면·주행 연결은 Rust 시험과 별도로 Godot 헤드리스 시험을 실행합니다.
명령과 첫 import 우회 방법은 [Godot 시험과 첫 import 검사](docs/developer-guide.md#godot-시험과-첫-import-검사)에 있습니다.

독립 번들을 만들 때는 전국 자료와 Godot import를 준비한 뒤 실행합니다.

```powershell
& ./scripts/package_release.ps1 -OutputDir dist/demo-reviewed -GodotPath '<Godot exe 절대 경로>'
& ./scripts/test-package-release.ps1
```

출력은 저장소 `dist/` 하위 전용 폴더로 제한하며, 필수 파일이 없으면 실패합니다.
게시·운영 서명은 [업데이트 안내](docs/windows-updates.md)를 따릅니다.

## 프로젝트 구조

| 경로 | 역할 |
| --- | --- |
| `crates/sim-core/` | 차량·충돌·도로망·경로·저장/재생 로직 |
| `crates/sim-godot/` | Rust와 Godot을 연결하는 GDExtension |
| `crates/sim-tools/` | 전국 자료 생성·지도 검사·기록 재현 CLI |
| `godot/` | 메뉴·지도·주행 화면과 화면 시험 |
| `config/` | 차량·입력·카메라·도로 설정 |
| `assets/`, `testdata/` | 교육 시나리오·지도 코스·시험 자료 |
| `scripts/` | 패키징·런처·업데이트·배포 검증 |
| `docs/` | 개발·설계·검수 문서 |

## 현재 한계와 다음 작업

블록 선택·저장·재생·주행 및 짧은 블록/교차로 회귀 시험은 구현되어 있습니다.
자동 시험 통과는 모든 실제 도로·속도에서의 주행 가능성이나 사용자 검수 완료를 뜻하지 않습니다.

- 차량·시야·건물 높이는 미검증 또는 추정값을 포함합니다.
- 도로 고도·경사·입체 교차와 땅 쓰임 색상, 건물 공간 분할 최적화는 후속 범위입니다.
- 사용자 조작 편의·성능 측정·독립 PC 실행·교육 담당자 검수는 남아 있습니다.
- Lanelet2 연동은 제거되었으며 현재 지도 경로 기능은 OSM 블록을 기준으로 합니다.

상세 계획과 단계별 검수 기록은 [PLAN.md](PLAN.md), 실차 관련 가정은 [차량 가정 문서](docs/vehicle-assumptions.md)를 참고하세요.

## 문서와 지도 출처

- [개발·지도 자료·시험 안내](docs/developer-guide.md)
- [지도 자료와 이용 조건](docs/map-data.md)
- [지역 보정 조사표](docs/map-patches/survey-template.md)
- [Rust ↔ Godot 인터페이스](docs/design/godot-interface.md)
- [개발 환경과 검증 기록](docs/development.md)

지도 자료는 **© OpenStreetMap contributors**입니다. [OSM 저작권·ODbL 안내](https://www.openstreetmap.org/copyright)를 확인하세요.
코드·지도·외부 에셋의 이용 조건은 각각 확인해야 하며, OSM 라이선스가 프로젝트 코드 전체의 라이선스를 뜻하지 않습니다.
