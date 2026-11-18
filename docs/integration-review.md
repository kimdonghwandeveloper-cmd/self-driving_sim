# 브랜치 통합 검토 (2026-11-18)

## 범위와 승인 상태

- 통합 브랜치: `integration/review-all-branches`. 기준: `refactor/remove-lanelet2` (`3dc312d`).
- 작업 전 `main=efd7991`, `develop=2dc4e1e`. 두 참조는 변경하지 않는다.
- main 병합·브랜치/워크트리 삭제는 사용자 검증 결과 승인 이후다. push는 수행하지 않는다.
- 표의 이력 포함은 모든 옛 기능의 활성화를 뜻하지 않는다. 아래 판단에 따라 최신 동작을 유지한 명시적 병합도 있다.

## 미커밋 변경 보존

- 공통 Git 디렉터리의 `integration-preservation/20261118-234047/manifest.json`에 원래 브랜치 SHA와 워크트리별 상태를 기록했다.
- 워크트리 16곳의 패치와 추적 변경 파일 21개 원본, 미추적 파일 33개를 별도 사본으로 보존하고 SHA-256을 대조했다.
- 중첩 등록 워크트리는 재귀 복사하지 않고 별도 항목으로 기록했다. ignored 캐시/빌드/원본 작업물은 기존 위치를 유지한다.
- 원래 파일을 stash/reset하거나 기존 워크트리를 제거하지 않았다. IDE/자동 import 파일은 제품 소스에 임의로 포함하지 않는다.

## 브랜치별 반영 판단

| 브랜치 | 원래 SHA | 처리 | 통합 이력 포함 |
| --- | --- | --- | --- |
| `backup/gear-rule-alignment-c3cb5b6` | `c3cb5b6` | 동등/후속 구현에 포함된 과거 시험 이력만 결합, 옛 src/ 앱을 재도입하지 않음 | 예 |
| `develop` | `2dc4e1e` | 이번 승인 전까지 참조 유지, 병합하지 않음 | 예 |
| `docs/car-photo-references` | `133722e` | 설계 이미지 추가, 기존 사진 8개 내용 유지 | 예 |
| `docs/plan-status-audit` | `c7a49fa` | 당시 추가 기록을 docs/history/에 보존, 현재 PLAN은 유지 | 예 |
| `feature/block-route-planning` | `862272e` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/block-route-replay` | `307d151` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/compact-driving-hud` | `3fd7ca8` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/demo-delivery-pipeline` | `724e341` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/lanelet2-route-picker` | `795598d` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/map-selection-and-building-faces` | `536f3c1` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/osm-10km-and-internal-roads` | `96a59a4` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/osm-3d-buildings` | `b572ece` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/osm-route-driving` | `0528add` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/restore-vehicle-defaults` | `d0fcd60` | 설정 숫자는 유지, 현행 소스 경로에 맞춘 가속도 설명만 반영 | 예 |
| `feature/road-block-model` | `be98752` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/road-network-geometry` | `0dd77ce` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/route-draw` | `93d2abb` | 선 그리기/5km 기본값/새 우회 정책은 제외, 공통 교차로 형상 계약 보완만 수동 통합 | 예 |
| `feature/simulator-ui-and-camera` | `278ae44` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `feature/verification-signals` | `0139b62` | 독립 검증 도구 통합, 옛 앱 실행을 현재 Godot 검사로 변경 | 예 |
| `feature/yeosu-convention-alignment` | `b7ce562` | 6개 고정 코스와 정렬 배경 통합, 기존 배경은 유지하고 별도 GLB 사용 | 예 |
| `feature/yeosu-convention-roads` | `02ef939` | 6개 고정 코스와 정렬 배경 통합, 기존 배경은 유지하고 별도 GLB 사용 | 예 |
| `fix/simulator-ui` | `fb95d55` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `main` | `efd7991` | 이번 승인 전까지 참조 유지, 병합하지 않음 | 예 |
| `refactor/remove-lanelet2` | `3dc312d` | 최신 refactor/remove-lanelet2 기준에 이미 포함된 결과 유지 | 예 |
| `test/merge-steering-calibration` | `f67fa76` | 동등/후속 구현에 포함된 과거 시험 이력만 결합, 옛 src/ 앱을 재도입하지 않음 | 예 |

## 유의할 동작

- Lanelet2 제거와 OSM 블록 엔진을 유지한다. 블록 선택 UI가 완성된 것으로 처리하지 않는다.
- 기존 `background.glb`는 바꾸지 않는다. 컨벤션 코스에만 `convention-background.glb`를 연결한다.
- 컨벤션 통로는 기존 이미지로 근사한 고정 교육 코스다. 실제 지도 정확도/접근 권한 또는 전국 블록 선택 연결 완료를 주장하지 않는다.
- 교차로 부채꼴의 중복/퇴화 정점을 정리하고 생성 단계에서 화면과 같은 형상 계약을 검사한다.
- 공통 교차로 규칙 변경으로 블록 도로망 판본은 `road-network-v2`다. 예전 블록 파일의 v1 결과를 묵시적으로 바꾸지 않고 판본 불일치로 거부한다. `route-v1` 지원은 유지한다.
- 검증 신호 도구는 독립 패키지다. Rust 전체 검사 외에도 별도 fmt/clippy/test를 수행한다. 사용자 승인 없이 실제 main 병합 승인을 만들지 않는다.

## 검증 결과

통합 변경에 대한 검증 진행 중. 최종 커밋과 실행 결과를 확인한 후 이 절을 갱신한다.
