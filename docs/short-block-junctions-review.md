# 짧은 블록·교차로 생성 수정 검토

상태: 기능 브랜치 `feature/short-block-junctions`에서 자동 검증. main/develop 병합·push·정식 업데이트 배포는 하지 않음. 사용자 수동 주행·시연 검수는 대기.
기준: 미병합 UI 릴리스 `873e6e3` 위에서 후속 작업. 기존 develop 워크트리의 `drive_screen.gd` 미커밋 서식 변경과 임포트 파일은 보존·커밋 제외.

## 테스트 먼저

- `1341730`에 실제 원본 좌표/ID/태그를 가진 엑스포·강남 회귀 자료와 기대 시험을 먼저 커밋했다.
- 기존 코드에서 4 m 연결 블록 및 엑스포 node 806285558은 `block too short for joint cut`, 강남 node 414684605는 경계 자기 교차로 실패했다.
- 추가로 인접 T자/3° 분기, 짧은 S자 마이터 중첩, 접선이 원본 노드를 지나지 않는 안내선도 실패를 재현하고 수정했다.
- 십자형 노면 합집합의 면적 76 m²와 모서리 빈 공간을 검사한다. 볼록 껍질로 빈 공간을 메우지 않는다. 90개 회전·입력 순서 및 비유한/퇴화 입력 거부를 검사한다.

## 구현

- 거의 직선인 폭 전환은 먼 가장자리 교점 대신 두 원본 단면을 잇는 테이퍼로 만든다.
- 양끝 교차로가 같은 블록을 모두 잘라 쓰지 않도록 호 길이 예산을 나눈다.
- 곡선 입구는 실제 단면과 접선에 맞춘다. 필렛이 입구와 겹치면 도로 단면/반폭 원판의 방사 외곽을 계산하고 동일한 자기 교차·삼각형 검증을 통과시킨다.
- 짧은 S자에서 마이터가 자기 교차하면 원본 선분별 폭을 가진 띠와 둥근 이음 면으로 표현한다. 중심선·블록 ID·회전 제한은 변경하지 않는다. 급격한 되돌림·퇴화 입력은 계속 진단한다.
- 공통 원본 노드/블록을 가진 노면 안에 덮인 내부 연석만 분할·제거한다. 좌표만 겹친 다른 도로/입체 교차의 연석은 제거하지 않는다. 새 고도 모델을 도입한 것은 아니다.
- 안내선은 실제 입구 접선 교점을 사용한다. 원호 끝은 다음 도로 시작과 부동소수점 잔차 없이 결합한다. 최소 회전 반경·차체 검사는 유지한다.
- 저장 지문은 모든 띠/이음 면과 교차로 외곽을 포함한다. `road-network-v4`/`block-guide-v2`로 구분하며 옛 블록 파일은 재생성한다. `route-v1` 동작은 유지한다.
- 대규모 자동 주행 시험도 실제 세션과 같은 `ObstacleGrid` 검색을 쓴다. 기존 격자/전체 장애물 적분 동등성 시험을 유지하고 모든 시험 연석을 충돌 대상으로 둔다.

## 자동 검증 결과

- fmt / clippy 모든 target·feature, 경고 0.
- Rust 484개: sim-core 465, 통합 4, sim-godot 12, sim-tools 3.
- Godot 28종(전국 자료 포함). 전체 엑스포 350블록/200접속부, 강남 973블록/571접속부를 유지한 선택·저장·재생·메시·차량 이동.
- 패키징 경로/필수 파일 안전성 110개 통과.
- 검토용 독립 실행본 `self-driving_sim-test/dist/short-junction-review-20261120/Simulator.exe` 제작 및 독립 검사 9개 통과. 원본 소스 없이 전체 지역 시험도 통과했다. 판본은 로컬 검토용 0, 자동 업데이트 비활성이다.

| 고정 원본 경로 | 길이 | 3 / 8 m/s | 최대 중심선 이탈 |
| --- | ---: | --- | ---: |
| 엑스포 지역 충민로 | 1,016.3 m | 모두 목표 도달, 충돌 0 | 0.051 m 이하 |
| 강남 지역 | 252.9 m | 모두 목표 도달, 충돌 0 | 1.597 m 이하 |
| 오류 node 806285558 직접 통과 | 250.7 m | 모두 목표 도달, 충돌 0 | 0.058 m 이하 |
| 오류 node 414684605 직접 통과 | 240.0 m | 모두 목표 도달, 충돌 0 | 0.062 m 이하 |

Godot 차량 시험은 시작 준비와 128틱 이동까지, 완주·충돌 0은 Rust 공통 운동·충돌·목표 평가기로 확인했다. 시험 자동 추종 결과는 실차·모든 경로·모든 속도의 안전성 보장이 아니다.

## 수동 확인 순서

1. 새 검토용 실행본 또는 test 워크트리 Godot 프로젝트를 실행한다.
2. 경로 지도 → 새 경로 → 더 보기 → ‘엑스포 주변 지역 표본 · 350블록’ 또는 ‘강남 지역 표본 · 973블록’을 연다.
3. 긴 도로 두 개를 출발·도착으로 지정하고 경로 확인 → 주행 시작을 누른다.
4. 저장 후 메뉴에서 재진입하고 교차로 면·연석·주행 감각을 확인한다. 이전 `road-network-v3`/`block-guide-v1` 블록 파일은 경로 확인·저장으로 다시 만든다. 기존 `route-v1` 파일은 유지된다.
5. 극히 짧은 도로에 차량 시작/목표 자세를 놓을 수 없는 경우와 좁은 도로·실제 최소 반경 미달은 계속 거부한다. 특정 전국 임의 구간과 성능/입체 도로 수동 검수는 별도다.

## 변경 파일과 주요 위치

| 파일 | 주요 위치 | 내용 |
| --- | --- | --- |
| crates/sim-core/src/world/road_network.rs | 18, 42, 79, 371, 462, 534 | 판본, 노면 조각, 절단 예산, 테이퍼/접속 외곽 |
| crates/sim-core/src/world/road_network_surface.rs | 1 | 방사 합집합과 연결된 노면 내부 연석 절단·검증 |
| crates/sim-core/src/world/block_route.rs | 21, 233, 336, 562, 579 | 노면 조각 연결, 실제 접선 안내선·끝점 결합 |
| crates/sim-core/src/world/block_route_file.rs | 471, 478 | 새 결과 지문·전체 노면 결속 |
| crates/sim-core/src/world/test_drive.rs | 9, 68 | 실제 세션과 같은 격자 충돌 검색 |
| crates/sim-core/src/world/road_network_tests.rs | 413, 647 | 퇴화 입력 진단·짧은 블록/실제 전체 지역 회귀 |
| crates/sim-core/src/world/road_network_region_tests.rs | 1 | 원본 way 고정 저장·재생·목표 도달 4사례 |
| crates/sim-core/src/world/block_route_tests.rs | 353, 506 | 진단 보존·실제 접선·정확한 끝점·짧은 목표 배치 |
| godot/presentation/block_map_screen.gd | 89 | 전체 고정 지역 표본 열기와 안내 |
| godot/tests/block_real_test.gd | 37 | 새 도로망 판본 검증 |
| godot/tests/junction_region_test.gd, .gd.uid | 1 | 전체 지역 UI·메시·재생/차량 이동·옛 판본 거부 |
| scripts/package_release.ps1 | 29, 127, 169 | 필수 표본·검토 안내·전체 지역 독립 검사 |
| testdata/junction-regressions/README.md | 1 | 원본 경로·SHA-256·ODbL 출처 |
| testdata/junction-regressions/expo-joint.osm.json, gangnam-joint.osm.json | 1 | 대상 접속부 원본 way 자료 |
| testdata/junction-regressions/expo-region.osm.json, gangnam-region.osm.json | 1 | 기존 지역 스냅샷 전체 보존 |
| PLAN.md | 683 | 이력/현행 판본, 자동 검증과 수동 검수 분리 |
| docs/short-block-junctions-review.md | 1 | 검토 범위·결과·잔여 검수 |

검토용 실행본의 런타임 코드는 `03557e0` 기준으로 검증했다. 이후 검토 문서 갱신은 런타임에 영향을 주지 않는다. 사용자 확인 전까지 정식 v2·main/develop은 그대로 유지한다.
