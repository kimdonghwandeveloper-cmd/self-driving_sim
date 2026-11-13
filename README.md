# 자율주행 셔틀 조작 훈련 시뮬레이터

[PLAN.md](PLAN.md)를 따라 개발 중인 버전입니다. 화면은 Godot 4.7(`godot/`)이고, 엔진과 무관한 계산은 Rust(`crates/sim-core`)가 하며
Godot 확장(`crates/sim-godot`, godot-rust)으로 연결합니다. 처음에 만든 Bevy 앱은 GD-5(2026-11-10 사용자 결정)에서 지웠습니다.

키보드나 Xbox 유형 게임패드로 셔틀을 운전하며, 차량 상태 머신(시동·기어·비상 정지)과
운동 모델(가속·제동·관성·조향, 최고 속도 40 km/h·출력 한계·저항·횡가속도 상한), 고정 장애물·연석 충돌을 확인할 수 있습니다.
화면은 입석 조작자 1인칭 시점(전·후·좌·우 전환)입니다. HUD에 속도·기어·시동·비상 정지·입력 장치·접촉·시점·이동 방향과
거부된 명령의 사유가 한국어로 표시됩니다. 보정 코스(`calibration`)에서는 회전반경·예상 정지거리와 제동·가속 시험 측정이 함께 표시됩니다.
메인 메뉴에서 개발 코스·저장 경로를 골라 주행하고, 지도 화면에서 내장 전국 지도 자료로 지역을 바로 열어 경로를 찍어 저장할 수 있습니다.
교육 시나리오와 기록 파일 저장(Phase 5)은 아직 구현하지 않았습니다.
실제 차량의 성능이나 안전성을 검증하는 도구가 아닙니다.

### 진행 상황 (2026-11-10, main 기준)

[PLAN.md](PLAN.md)의 체크박스와 "진행 기록"이 기준입니다. 사람 확인 결과는 각 절의 진행 기록에 따로 적습니다.

| 단계 | 상태 |
| --- | --- |
| Phase 1~3 (기반·운동 모델·카메라·여수 코스) | 대부분 완료. 실제 게임패드 축 범위·드리프트 측정은 남음 |
| Phase 4 (굽은 길·교차로·경로 탐색·지도 받기·경로 선택 화면·통합) | 구현과 자동 시험 완료. 경사 도로(4-7)는 시작 전 |
| 4-8 자동 경로 · 4-9 건물·장소 고르기 (2026-11-11 추가) | 구현과 자동 시험 완료(main 병합). 사람 확인 전. 위아래로 겹치는 도로(4-7 추가)는 시작 전 |
| 4-10 전국 장소 검색·지도 바깥 바다색 (2026-11-12 추가) | 구현과 자동 시험 완료(`feature/place-search`). 실제 Nominatim 검색·사람 확인 전 |
| 4-11 전국 지도 자료 내장 (2026-11-12 추가) | 1단계(전국 자료·받기 단계 없애기) 구현과 자동 시험 완료(`feature/korea-data`). 사람 확인 전. 2단계(경로 길이 제한 풀기)·4-12 3D 건물·4-13 땅 쓰임 색은 시작 전 |
| Phase G (Godot 전환, GD-0~GD-4) | 완료. GD-2~GD-4 사람 확인 2026-11-10. GD-5 Bevy 앱 제거 |
| 4-5G G-1 Geofabrik 지역 추출 | 코드와 fixture 시험 완료. 실제 한국 PBF 검증·측정 전 |
| Phase 5~6 | 시작 전(6-1 일부 제외) |

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
| 메인 메뉴로 돌아가기 | `Esc` |

## 입석 조작자 시점 (Phase 3-1, 임시값)

- 기본·리셋 시점은 사용자 지정 기준 사진 `Car_photo/KakaoTalk_261030_131310_61.jpg`의 구도를 단순 도형으로 재현합니다.
  사진은 런타임 텍스처로 쓰지 않으며, 실내 형상·눈 위치·FOV는 사진 구도에 맞춘 **실측 전 임시값**입니다.
- 네 방향은 같은 눈 위치에서 시선만 90°/180° 돌립니다. 기어·조향·가속·차량 자세에 영향을 주지 않고,
  후방을 봐도 조향은 반전되지 않으며 R에 들어가도 시점은 자동으로 바뀌지 않습니다.
  여러 방향 키를 동시에 누르면 현재 방향을 유지하고, 시작 위치 복귀(`R`)가 적용되면 전방으로 돌아갑니다.
- HUD에 바라보는 방향과 실제 부호 있는 속도로 판정한 이동 방향(전진/후진/정지)을 함께 표시합니다.
  전방이 아닌 시점에서는 "현재 시선은 차량 전방과 다릅니다"가 붙습니다.
- 외관 차체는 1인칭 시점에서 보이지 않습니다. `C`로 켜는 외부 카메라는 개발 확인용이며 훈련 시점이 아닙니다(차체 외관은 임시 모델).
- 게임패드 시점 버튼은 아직 지정하지 않았습니다.
- `config/camera.toml`의 `[operator_view]`에서 눈 위치(`eye_m`, 차량 로컬 m), `pitch_deg`, 수직 FOV(`vertical_fov_deg`, 30..=110),
  `near_clip_m`(0.01..=0.5)을 바꿉니다. 파일이 없거나 값이 잘못되면(NaN·무한대·범위 밖 포함) 주행 화면 경고 후 기본값을 씁니다.

## 코스 (임시값)

메인 메뉴의 코스 목록에서 고릅니다. 모든 치수는 **훈련용 임시값**이고 실제 도로·공원 통로가 아닙니다.

| 코스 | 구성 (시작: 원점, 전방 -Z) |
| --- | --- |
| `training` | 폭 6 m 직선 차로(z=+6 ~ -66), 양옆 연석(충돌 경계), 가장자리 흰 선(표시만), 정지선 z=-20, 차로 오른쪽 고정 장애물 z=-32, 좁은 통로 z=-42 ~ -54(폭 3.0 m = 차체 폭 1.8 m + 양옆 0.6 m), 앞·뒤 끝 벽 |
| `calibration` | 넓은 평지와 장애물 4개(회전반경·제동 시험용, 아래 보정 측정 표시) |
| 여수 엑스포 · OSM 배경 | 여수 고정 교육 코스(way 532526349)에 Blender/Blosm으로 만든 OSM 배경을 겹쳐 주행합니다. 배경에는 충돌체가 없으며 기존 노면·연석·충돌 판정을 사용합니다. |
| 지도 코스 `map:<파일>` | OSM 중심선·경로 파일로 만든 가상 교육 코스. 메뉴에는 지도 화면이 저장한 경로(`cache/routes/`)가 나옵니다. 여수 고정 코스 자료(`testdata/yeosu-expo/`)는 위의 전용 메뉴 항목에서만 엽니다. 형식과 제약은 `docs/map-data.md` 참고. 읽지 못하면 사유를 띄우고 `training`으로 대체 |

- 화면 메시와 충돌 판정은 `crates/sim-core/src/world/`의 같은 정의에서 만들어서 어긋나지 않습니다. 흰 선은 시각 표시일 뿐이고 실제 경계는 연석입니다.
- 지도 코스에는 경로 끝 목표 표시(체크무늬 띠와 "도착")가 있습니다. 도착 판정·점수는 아직 없습니다(Phase 5).
- 시작 위치 복귀(`R`)는 선택한 코스의 시작 자세로 되돌립니다.

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
새로 선택된 게임패드(대체 게임패드, 재연결 포함)는 모든 입력을 놓아 중립이 된 뒤에 활성화되며, 그 전에는 비상 정지 입력만 받습니다.
HUD의 "입력" 항목에 지금 조향·가속에 쓰는 장치가 표시됩니다.

키보드와 게임패드를 함께 쓸 때의 규칙은 다음과 같습니다.

- 조향·가속은 한 장치의 값만 씁니다. 지금 장치의 입력이 없어진 상태에서 다른 장치를 조작하면 장치가 바뀌고,
  두 장치를 동시에 조작하면 지금 장치를 유지합니다.
- 제동은 어느 장치든 더 강한 값을 적용하고, 제동이 있으면 가속은 0입니다.
- 비상 정지는 어느 장치든 우선합니다. 한 장치의 비상 정지가 눌려 있는 동안에는 다른 장치의 해제·시동·기어 입력도 무시합니다.
- 두 장치의 시동 입력이 같은 틱에 겹치면 한 번만 적용하고, 서로 다른 기어를 요청하면 거부합니다.
- 게임패드 연결이 끊기면 그 틱부터 그 게임패드의 입력이 사라집니다. 게임패드로 조작 중이고 차량이 움직이고 있었다면
  비상 정지로 멈추며, 이후 `Backspace` 또는 다시 연결한 게임패드로 해제합니다.
  키보드로 조작 중이었거나 정지 상태였다면 차량 상태를 바꾸지 않습니다.

## 설정과 상태 전이 규칙

매핑은 `config/input.toml`, 정지 판정 속도와 운동 모델 값(최고 속도, 가속·제동·관성·비상 감속도,
질량·구동 출력·구름/공기 저항, 축간거리, 최대 조향각, 조향 속도, 조향 감도, 조향 중립 복귀 방식·복귀 속도,
언더스티어 계수·횡가속도 상한)은 `config/vehicle.toml`에서 바꿉니다.
전진 최고 속도 40 km/h는 실차 사양이고, 나머지 값은 실차 검증 전 임시값입니다.
설정 파일이 없거나 잘못되면 주행 화면에 경고를 띄우고 기본값을 사용합니다.
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
- 차체(길이 4.2 m, 폭 1.8 m 상자, 임시 치수)가 장애물·연석에 닿으면 관통하지 않고 그 자리에서 멈춥니다(속도 0).
  벽을 따라 미끄러지지 않으며, 이동이 실제로 막힌 순간에 `SimVehicle`이 `collided` 신호를 한 번 보냅니다.
  충돌 기록·횟수 집계는 Phase 5에서 다시 설계합니다(Bevy 판의 메모리 기록은 GD-5에서 함께 지움).
  HUD의 "접촉"은 장애물과 2 cm 안이면 "있음"이지만, 가까이 멈추거나 스쳐 지나가기만 하면 충돌로 세지 않습니다.
  밀고 있는 동안에는 다시 세지 않고, 그 장애물에서 2 cm 밖으로 떨어졌다가 다시 막히면 새 충돌로 셉니다.
  중복 판정은 장애물마다 따로 하므로 좁은 통로에서 다른 벽에 붙어 있어도 재충돌을 놓치지 않습니다. 높이는 보지 않습니다.
  회전 중 차체 모서리가 검사 지점 사이에서만 장애물을 스치는 경우도 경로를 보수적으로 나눠 검사합니다.
  다만 최소 검사 구간(0.1 mm)에 따른 근사와 부동소수점 오차가 있어, 절대적인 관통 방지를 보장하지는 않습니다.
  장애물과 겹친 채 시작하면(배치 오류 등) 그 장애물에서 빠져나가는 이동만 허용하고 더 깊이 들어가는 이동은 막으며,
  다른 장애물은 평소처럼 막습니다.
- 속도 값이 NaN/무한대이면 정지로 보지 않아, 정지가 필요한 전이를 모두 거부합니다.

## 보정 측정 (calibration 코스, 임시값)

`config/vehicle.toml` 값을 바꾼 뒤 다시 주행해 같은 코스에서 가속·제동·조향 반응을 비교하는 용도입니다.
`calibration` 코스의 HUD 아래 상자에 표시되며, 수치는 모두 시뮬레이터 임시값으로 계산한 결과이고 실차 성능과 같다고 보지 않습니다.
계산은 `crates/sim-core/src/vehicle/calibration.rs`, 한국어 줄은 `SimVehicle.calibration_lines()`입니다.

- 회전반경(후륜축): 지금 속도·바퀴 각도에서 운동 모델과 같은 곡률(언더스티어·횡가속도 상한 포함)로 계산한 반경. 직진이면 "직진".
  "최대 조향 최소 반경(저속)"은 최대 조향각의 기구학 반경(축간거리 / tan 조향각)입니다.
- 최대 제동 예상 정지거리: 지금 속도에서 가속 없이 최대 제동할 때의 v²/(2a).
  a는 최대 제동 + 관성 감속 + 구름 저항(비상 정지 중이면 비상 감속도 더함)이며, 고정 주기 적분 결과와 한 단계 이동량 정도 차이가 날 수 있습니다.
- 제동 시험: 움직이는 중 가속 없이 제동하거나 비상 정지하면 시작해, 속도가 0이 되면
  "마지막 제동 시험"에 회차·시작 속도·정지거리·시간을 남깁니다. 멈추는 순간까지 제동(또는 비상 정지)을 유지하지 않았거나
  (정지 직전에 풀어 관성으로 멈춘 경우 포함) 장애물에 막혀 멈추면 결과를 남기지 않습니다.
- 가속 시험: 정지 상태에서 가속하면 시작해, 최고 속도에 닿거나("최고 속도") 가속을 떼면("가속 뗌")
  "마지막 가속 시험"에 도달 속도·시간·거리를 남깁니다. 장애물에 막혀 이동이 잘리면 결과를 남기지 않습니다.
- 결과를 버리는 기준은 실제 이동 막힘입니다. 장애물 2 cm 안에 있을 뿐 막히지 않은 시험은 정상 결과로 남깁니다.
- `R`: 정지 상태에서 시작 자세로 되돌립니다. 시동·기어·비상 정지 상태와 끝난 시험 결과는 유지하고,
  진행 중인 시험과 장애물 접촉 기록만 초기화합니다. 움직이는 중에는 거부합니다.
- 설정은 주행 화면에 들어갈 때 읽습니다. 값을 바꾼 뒤에는 메뉴로 나갔다가 다시 들어갑니다.

## 지도 화면 (경로 선택, Phase 4-5·4-5b)

메인 메뉴에서 들어갑니다. 배경에 OSM 표준 지도 타일을 깔고(화면에 보이는 타일만 받고 `cache/tiles/`에 캐시),
그 위에 열린 지역의 도로·건물을 그립니다. 타일은 보기 전용이며 코스는 도로 자료로만 만듭니다.
타일을 받지 못하면 회색으로 비워 두고 나머지 기능은 그대로 됩니다. 출처 표기는 오른쪽 아래에 늘 보입니다. 헤드리스 시험에서는 타일을 받지 않습니다.

지역 도로·건물·장소는 **내장 전국 지도 자료**(`data/korea/`, 아래 "전국 지도 자료 만들기")에서 바로 엽니다(4-11). 받기 버튼이나
기다림 없이, 다음 경우에 그곳을 가운데로 하는 1.5 km 지역이 열립니다.

- 점이 없을 때 지도를 클릭하면 그곳 지역을 열고 바로 점을 찍습니다.
- 점이 없을 때 화면 가로 3 km 안쪽으로 확대하고 멈추면(0.4초) 화면 가운데 지역을 엽니다.
- 전국 검색 결과를 고르면 그곳으로 옮기고 지역을 열어 점으로 더합니다.

출발·도착은 아직 한 지역(1.5 km) 안에서 고릅니다. 점이 있을 때는 지역을 바꾸지 않으며, 다른 곳에서 시작하려면 "점 모두 삭제"를 누릅니다
(4-11 2단계에서 풀 예정).

| 조작 | 입력 |
| --- | --- |
| 화면 이동 / 확대·축소 | 오른쪽 끌기 / 휠 |
| 스냅샷 파일 열기 / 스냅샷 폴더 열기(개발용) | 버튼 |
| 맞춤 보기: 전국 / 열린 지역 / 경로 | 버튼 |
| 점 추가(시작 → 경유 → 끝) / 점 삭제 | 왼쪽 클릭(건물을 누르면 그 건물의 진입 지점) / `Shift` + 클릭, "마지막 점 삭제"·"점 모두 삭제" 버튼 |
| 장소 이름 검색 | 검색 칸에 입력하면 열린 지역의 건물·장소에서 바로 찾고, 결과 상자에서 고르면 점으로 더함 |
| 전국 장소 검색(4-10) | 검색 칸에서 `Enter` 또는 "검색" 버튼. 결과를 고르면 그곳으로 옮기고 지역을 열어 점으로 더함 |
| 경로 저장(같은 이름은 "덮어쓰기"를 켜야 함) | 이름 입력 후 "경로 저장" |
| 메인 메뉴 | `Esc` |

출발·도착 두 점만 찍으면 도로를 따라 경로를 만듭니다(4-8). 큰 도로·일방통행 방향·회전 제한을 따르고, 만들 수 없는 교차로는 피해 가거나
곧게 지나면 곁가지를 그리지 않고 잇습니다. 화면에는 다음이 표시됩니다.

- 경로 길이, 교차로 수, 단순화·우회한 교차로 수
- 회전 제한 수
- 출발·도착 건물 이름
- 폭과 그 출처
- 경고: 임시 폭, 3 m 미만의 좁은 폭, 돌 수 없는 꺾임, 거부된 교차로 등

한국 OSM은 건물 이름이 빈 곳이 많습니다. 열린 지역의 도로는 `cache/regions/<자료 판>/`에 스냅샷으로 남고,
저장한 경로는 `cache/routes/<이름>.toml`(그 스냅샷을 가리킴)이며, 메인 메뉴 코스 목록에서 골라 주행합니다.
전국 장소 검색은 OSM 공식 검색 서버 Nominatim을 씁니다(별도 서버 없음, 초당 1회 이하·Enter 때만 요청, 결과는 `cache/geocode/`에 저장).
지도에서 대한민국 밖은 바다색으로 가립니다. 가장 많이 축소해도 대한민국이 화면 가로를 꽉 채우며, 전국을 보려면 위아래로 끌어 이동합니다.

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
형식과 넣는 것·빼는 것은 `docs/map-data.md`의 "4-11 전국 지도 자료"를 참고하세요.

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
자세한 규칙은 `docs/map-data.md`의 "Geofabrik 지역 추출"을 참고하세요.

```powershell
cargo run --locked -p sim-tools -- --extract-pbf <south-korea.osm.pbf> --bbox 34.7480,127.7460,34.7590,127.7600
cargo run --locked -p sim-tools -- --audit-extract <세대 폴더> --pbf <south-korea.osm.pbf>
cargo run --locked -p sim-tools -- --compare-overpass testdata/yeosu-expo/graph-2026-11-06.osm.json --extract <세대 폴더> --pbf <south-korea.osm.pbf>
```

## 구성과 기준

- `Cargo.toml`: 작업 공간 설정(루트 패키지 없음)
- `crates/sim-core/`: 엔진과 무관한 로직
  - `vehicle/`: 상태 머신(`state.rs`), 운동 모델(`dynamics.rs`), 충돌 판정(`collision.rs`), 보정 측정(`calibration.rs`)
  - `session/`: 고정 간격 주행 세션(`DriveSession`)과 입력 재생(`replay.rs`)
  - `input/`: 키 매핑·게임패드 규칙·장치 입력 합치기·동시 기어 판정
  - `world/`: 코스(`course.rs`), OSM 지도 코스(`map.rs`), 굽은 길 형상(`road_geom.rs`), 교차로(`junction.rs`),
    도로 그래프(`osm_graph.rs`), 경로(`route.rs`, `route_plan.rs`), Overpass 받기(`map_fetch.rs`),
    Geofabrik 추출·감사·비교(`pbf_extract.rs`, `pbf_audit.rs`, `pbf_compare.rs`), 스냅샷 읽기(`road_snapshot.rs`),
    전국 지도 자료(`korea_data.rs`)
  - `picker/`: 경로 선택 모델·지역 범위·전국 장소 검색·OSM 배경 타일, `config.rs`·`camera.rs`: 설정 파싱과 시점
- `crates/sim-godot/`: Godot 확장(`SimVehicle`, `SimCourse`, `SimReplay`, `SimRoutePicker`, `SimTiles`, `SimScenario`). 계약은 `docs/design/godot-interface.md`
- `crates/sim-tools/`: 창 없는 개발 명령(전국 지도 자료 만들기, 지도 받기, PBF 추출·대조)
- `godot/`: Godot 프로젝트(메뉴·주행·지도 화면 `presentation/`, 헤드리스 시험 `tests/`). 구현 메모는 `godot/IMPLEMENTATION.md`
- `config/{vehicle,input,camera,road_graph}.toml`: 시뮬레이터 임시 설정값
- `assets/maps/`: 저장소에 넣는 지도 코스(현재 비어 있음). 여수 고정 코스는 `testdata/yeosu-expo/`에 있음
- `data/korea/`: 전국 지도 자료(저장소 제외, 배포에 포함)
- `testdata/`: 자동 시험 자료(여수 스냅샷·경로, 작은 전국 지도 자료 조각). 여수 고정 코스만 OSM 배경 전용 메뉴에서 재사용
- `assets/scenarios/`: 교육 시나리오 파일(5-1, 메뉴 목록 끝에 나옴). 형식은 [docs/scenarios.md](docs/scenarios.md)
- [docs/development.md](docs/development.md): 선정 버전, 개발 PC, 검증 결과
- [docs/vehicle-assumptions.md](docs/vehicle-assumptions.md): 실차 사양 가정과 미확인 위험(사륜 조향·특수 구동 등)

## Godot 시험과 첫 import 검사

Windows에서 확장을 빌드한 뒤 다음 명령으로 초기화하고 시험합니다(워크트리 최상위).

```powershell
cargo.exe build --locked -p sim-godot
& '<Godot 콘솔 실행 파일>' --headless --path godot --import --frame-delay 1000
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/smoke_test.gd
& '<Godot 콘솔 실행 파일>' --headless --path godot --script res://tests/presentation_smoke.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/drive_replay_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/map_course_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/menu_flow_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/route_save_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/place_pick_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/region_open_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/goal_reset_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/calibration_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/scenario_test.gd
& '<Godot 콘솔 실행 파일>' --headless --fixed-fps 64 --path godot --script res://tests/suspension_test.gd
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
- `scenario_test.gd`는 메뉴에서 예시 시나리오를 열어 시작 전 정지, F5 시작, 단계 4개·메시지 2개, F6 일시정지(차량·시간 멈춤), 성공,
  F7 재시작, F8 종료와 없는 시나리오 파일의 실패 화면을 확인합니다(5-1). 출력의 `ERROR: ... 없는-시나리오.toml` 한 줄은 의도된 실패 경로입니다.
- `suspension_test.gd`는 보정 코스에서 제동 때 앞 숙임, 오른쪽 회전 때 왼쪽(바깥) 기울기가 차체 메시에 전달되고, 바퀴 부품은 기울지 않으며,
  입석 카메라는 `body_follow`(0.5)만큼만 따라가고 외부 시점은 따라가지 않는지 확인합니다(2-8).

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
