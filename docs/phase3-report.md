# Phase 3 통합 검증·인계 보고 (3D)

작성: 2026-11-06, Claude. 이 문서는 **보고**이며 PLAN 체크박스 완료 표시나 main 병합 승인이 아니다.
PLAN의 Phase 3 체크박스는 사용자 검토 전까지 바꾸지 않았다.

## 1. 브랜치와 커밋

| 단계 | 브랜치 | 커밋 | 상태 |
| --- | --- | --- | --- |
| 3-1 카메라·최소 실내 (3A) | `feature/phase3-camera` | `91ed1ad`, `d97b555` | 사용자 시야 확인 후 main 병합(`1475f80`). 브랜치 삭제는 사용자 확인 대기 |
| 3-2 개발용 테스트 코스 (3B) | `feature/phase3-test-course` | `f20d31e` | Codex continue. main 미병합 |
| 3-3 OSM 조사·출처 방침 | `feature/phase3-map` (3-2 위에 쌓음) | `0fddec8`, `d2d45e3` | Codex continue(조사 산출물만). 대상 통로 미확정 |
| 3-4 범용 OSM 변환기 | `feature/phase3-map` | `33488e4`, `687eaf5` | Codex continue(범용 변환기만). 실제 지도 코스 없음 |
| 3D 이 보고서 | `feature/phase3-map` | 이 커밋 | 검증 요청 예정 |

`feature/phase3-map`은 `feature/phase3-test-course`를 포함한다. main 대비 변경 파일은 이 보고서를 빼면 14개, 포함하면 15개다(원본 OSM JSON 4 248줄 포함).

## 2. 변경 파일 (main `1475f80` 대비, 이 보고서 제외)

| 파일 | 내용 |
| --- | --- |
| `src/world/course.rs` (신규) | `CourseDef`(이름·안내·시작 자세·노면·표시선·충돌 경계), `training`·`calibration` 코스, 테스트 7개 |
| `src/world/mod.rs` (신규) | `ActiveCourse`, `CourseSelection`(`training`/`calibration`/`map:<파일>`, 실패 시 training), `box_on_rect`, `spawn_course`, 안내 패널, 테스트 3개 |
| `src/world/map.rs` (신규) | 위경도→지역 좌표 투영, Overpass JSON 읽기, way 연결, 누락 노드·범위·크기·꺾임 검사, 임시 폭 도로 생성, 선택 파일 로더, 테스트 12개 |
| `src/scene.rs` | 고정 장애물 상수를 지우고 선택 코스를 생성 |
| `src/calibration.rs` | 시작 위치 복귀가 선택 코스의 시작 자세를 복원 |
| `src/app.rs`, `src/main.rs` | `--course` 옵션, 코스 시작 자세로 차량 초기화, 지도 실패 경고 |
| `src/lib.rs` | `pub mod world` |
| `Cargo.toml`, `Cargo.lock` | `serde_json = "=1.0.154"` 직접 의존성(잠금 파일에 이미 있던 버전) |
| `README.md` | 개발용 테스트 코스 절과 `map:` 선택 |
| `docs/map-data.md` (신규) | 공원 경계 가정, 직선 통로 후보 7개, 가상 코스 표시, ODbL 출처 표기 방침, 변환기 규칙 |
| `assets/maps/yeosu-expo/` (신규) | Overpass 응답 원본(조사용)과 출처·질의·bbox 기록 |

## 3. 자동 검증 결과

검증 신호 도구(`feature/verification-signals`의 verification-gate)로 Codex가 요청 커밋을 SHA 고정으로 검사했다.

| 요청 ID | 커밋 | 결과 | 비고 |
| --- | --- | --- | --- |
| `3-2-1793927583823721800` | `f20d31e` | continue | fmt/clippy/155 tests/smoke, calibration·잘못된 코스 이름 대체 실행, 정지 4방향 캡처 |
| `3-3-1793928473702516700` | `0fddec8` | changes-required | 출처 표기를 로그로 대신할 수 있게 쓴 문장 지적 |
| `3-3-1793928662222950900` | `d2d45e3` | continue | 출처 표기 수정 확인, 후보 7개 수치 재계산 일치 |
| `3-4-1793929311243998700` | `33488e4` | changes-required | 누락 노드를 직선으로 이음, 500 × 300 m·원점 거리 검사 없음(실행 재현) |
| `3-4-1793929942244606900` | `687eaf5` | changes-required | test 워크트리 `target` 일부가 빌드 중 사라져 smoke 빌드 실패(환경 문제로 분류) |
| `3-4-1793930061449482200` | `687eaf5` | continue | 별도 리뷰 워크트리에서 fmt/clippy/168 tests/smoke, 재현 입력 대체 실행, 정상 합성 지도 캡처 |

3-1(카메라)은 검증 도구가 생기기 전에 test 워크트리에서 fmt/clippy/147 tests/smoke를 확인했고, 사용자가 시야를 검토한 뒤 병합했다.

화면 캡처는 Git 공용 메타데이터 폴더(`.git`) 안의 `verification-signals/`에 있다. Git이 추적하지 않는 공유 자료라서 커밋이나 클론에는 포함되지 않는다.
- `3-2-1793927583823721800/captures/`: training 코스 정지 상태 4방향·외부·HUD
- `3-4-1793930061449482200/captures-normal/`: 합성 지도 코스와 출처 안내 패널

## 4. 실행하지 않은 검증 (미완료)

- **이동 중 수동 검증:** D 직진, 정지선 정지, 장애물 회피, 좁은 통로 통과, R 후진, 좌우 회전을 하면서 네 시점을 전환하는 검증. 창 가림, 지면 관통, 카메라 지연, HUD 방향 혼동을 사람이 확인해야 한다. 지금까지의 캡처는 모두 정지 상태다.
- **게임패드 검증:** 실제 장치로 확인해야 한다. Phase 1-2 미검증 항목과 같다.
- **실제 공원 지도 코스:** 대상 통로가 정해지지 않아 만들지 않았다. 변환기는 합성 데이터로만 시험했다.

## 5. 사용자 결정이 필요한 사항

1. OSM `여수세계박람회장`(way 529495025)이 PLAN의 대상 공원이 맞는지.
2. 교육 코스로 쓸 통로(way ID)와 시작·끝 지점, 그리고 최종 bbox(500 × 300 m 이하). 후보는 `docs/map-data.md`에 있다.
3. `feature/phase3-test-course`, `feature/phase3-map`의 main 병합 여부(AGENTS.md에 따라 사용자 승인 필요).
4. 병합이 끝난 `feature/phase3-camera` 브랜치의 삭제 여부.
5. PLAN Phase 3 체크박스 중 완료로 표시할 항목.

## 6. PLAN 항목별 현재 상태 (체크 표시하지 않음)

| 항목 | 상태 |
| --- | --- |
| 3-1 입석 조작자 카메라 | 구현·병합 완료. 이동 중 수동 검증만 남음 |
| 3-2 개발용 평면 테스트맵 | 구현·Codex 검증 완료. 이동 중 수동 주행 확인 남음 |
| 3-3 OSM 범위 확정 | 조사·출처 방침 완료. **대상 통로·범위 미확정** |
| 3-4 OSM → Bevy 변환 | 범용 변환기 완료. **실제 대상 코스 미생성**. 주변 건물(선택 항목) 미구현 |
| 3-5 지도 실패 대비 | 지도 파일이 없거나 잘못되면 training 코스로 실행하도록 구현함. 수동 정적 중심선은 대상 통로를 정한 뒤 만들 수 있음 |

## 7. 사진 기반 가정 (실측 아님)

- 카메라 눈 위치 (0, 1.65, −2.75) m, pitch −3°, 수직 FOV 62°는 `61.jpg` 구도에 맞춘 보정값이다.
- 흰 램프 쪽을 전방으로 본 것은 임시 방향 가정이다. 후방 실내는 전방을 180° 돌린 것이다.
- 코스 치수(차로 6 m, 좁은 통로 3.0 m, 지도 임시 폭)는 모두 훈련용 임시값이다.
