# Phase 3 통합 검증·인계 보고 (3D)

작성: 2026-11-06, Claude(같은 날 여수 가상 교육 코스 적용 후 갱신). 이 문서는 **보고**이며 PLAN 체크박스 완료 표시나 main 병합 승인이 아니다.
PLAN 체크박스는 Codex가 검증된 개별 항목만 문서 브랜치 `docs/phase3-completion-status`(`eb4f2f6`, main 미병합)에 체크했다. 이 브랜치에서는 PLAN을 바꾸지 않았다.

## 1. 브랜치와 커밋

| 단계 | 브랜치 | 커밋 | 상태 |
| --- | --- | --- | --- |
| 3-1 카메라·최소 실내 (3A) | `feature/phase3-camera`(삭제됨) | `91ed1ad`, `d97b555` | 사용자 시야 확인 후 main 병합(`1475f80`). 병합된 브랜치는 Codex가 용량 정리 때 삭제 |
| 3-2 개발용 테스트 코스 (3B) | `feature/phase3-test-course` | `f20d31e` | Codex continue. main 미병합 |
| 3-3 OSM 조사·출처 방침 | `feature/phase3-map` (3-2 위에 쌓음) | `0fddec8`, `d2d45e3` | Codex continue(조사 산출물). 대상 통로는 아래 `ced298a`에서 선택 적용 |
| 3-4 범용 OSM 변환기 | `feature/phase3-map` | `33488e4`, `687eaf5` | Codex continue(범용 변환기만, 합성 데이터 시험). 실제 코스는 아래 `ced298a` |
| 3-3/3-4 여수 가상 교육 코스 | `feature/phase3-map` | `ced298a` | 선택안(way 532526349, 임시 폭 4 m) 적용과 코스 크기에 맞춘 바닥. Codex continue |
| 3D 이 보고서 | `feature/phase3-map` | `de6a60c`, `84215af`, 이번 갱신 커밋 | Codex continue(갱신 전 판). 이번 갱신은 검증 요청 예정 |

`feature/phase3-map`은 `feature/phase3-test-course`를 포함한다. main 대비 변경 파일은 이 보고서를 빼면 16개, 포함하면 17개다(조사용 OSM JSON 4 248줄과 코스 원본 JSON 포함).

## 2. 변경 파일 (main `1475f80` 대비, 이 보고서 제외)

| 파일 | 내용 |
| --- | --- |
| `src/world/course.rs` (신규) | `CourseDef`(이름·안내·시작 자세·노면·표시선·충돌 경계), `training`·`calibration` 코스, 테스트 7개 |
| `src/world/mod.rs` (신규) | `ActiveCourse`, `CourseSelection`(`training`/`calibration`/`map:<파일>`, 실패 시 training), `box_on_rect`, `spawn_course`, 안내 패널, 테스트 3개 |
| `src/world/map.rs` (신규) | 위경도→지역 좌표 투영, Overpass JSON 읽기, way 연결, 누락 노드·범위·크기·꺾임 검사, 임시 폭 도로 생성, 선택 파일 로더, 테스트 13개(저장소의 여수 코스 주행 테스트 포함) |
| `src/scene.rs` | 고정 장애물 상수를 지우고 선택 코스를 생성. 바닥·격자를 코스 전체를 덮는 크기·위치로 계산(`ground_square`, 테스트 1개) |
| `src/calibration.rs` | 시작 위치 복귀가 선택 코스의 시작 자세를 복원 |
| `src/app.rs`, `src/main.rs` | `--course` 옵션, 코스 시작 자세로 차량 초기화, 지도 실패 경고 |
| `src/lib.rs` | `pub mod world` |
| `Cargo.toml`, `Cargo.lock` | `serde_json = "=1.0.154"` 직접 의존성(잠금 파일에 이미 있던 버전) |
| `README.md` | 개발용 테스트 코스 절과 `map:` 선택 |
| `docs/map-data.md` (신규) | 공원 경계 가정, 직선 통로 후보 7개, 가상 코스 표시, ODbL 출처 표기 방침, 변환기 규칙 |
| `assets/maps/yeosu-expo/` (신규) | 조사용 Overpass 원본, 선택 way 532526349 원본, 선택 파일 `course.toml`, 출처·질의·bbox 기록 |

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
| `3D-1793932456712136800` | `de6a60c` | continue | 보고서 이력 대조 일치. 파일 수·캡처 위치 문구 P3 |
| `3D-1793932566531657200` | `84215af` | continue | P3 문구 수정 확인 |
| `3-4-1793935312136869300` | `ced298a` | continue | way 532526349 좌표·임시 폭 대조, fmt/clippy/170 tests/smoke, 여수 코스 10 colliders·정지 캡처, 파일 없음 대체 실행. 문서의 옛 상태 설명 P3 |

3-1(카메라)은 검증 도구가 생기기 전에 test 워크트리에서 fmt/clippy/147 tests/smoke를 확인했고, 사용자가 시야를 검토한 뒤 병합했다.

화면 캡처는 Git 공용 메타데이터 폴더(`.git`) 안의 `verification-signals/`에 있다. Git이 추적하지 않는 공유 자료라서 커밋이나 클론에는 포함되지 않는다.
- `3-2-1793927583823721800/captures/`: training 코스 정지 상태 4방향·외부·HUD
- `3-4-1793930061449482200/captures-normal/`: 합성 지도 코스와 출처 안내 패널
- `3-4-1793935312136869300/captures-yeosu/`: 여수 가상 교육 코스 정지 상태 4방향·외부·HUD

## 4. 실행하지 않은 검증 (미완료)

- **이동 중 수동 검증:** D 직진, 정지선 정지, 장애물 회피, 좁은 통로 통과, R 후진, 좌우 회전을 하면서 네 시점을 전환하는 검증. 창 가림, 지면 관통, 카메라 지연, HUD 방향 혼동을 사람이 확인해야 한다. 지금까지의 캡처는 모두 정지 상태다.
- **게임패드 검증:** 실제 장치로 확인해야 한다. Phase 1-2 미검증 항목과 같다.
- **여수 가상 교육 코스의 실제 적합성:** 코스는 만들었지만 OSM 중심선과 임시 폭 4 m만 반영했다. 실제 폭·노면·경사·운행 가능 여부와 이동 중 주행감은 확인하지 않았다.

## 5. 결정 사항과 남은 승인

결정됨 (2026-11-06, Codex 제안을 사용자가 전달):
- 대상 공원: OSM way 529495025 `여수세계박람회장`(EXPO Ocean Park)을 개발 대상으로 쓴다.
- 코스 통로: way 532526349(`highway=service`, 약 292 m), 임시 폭 4 m. 실제 셔틀 운행 경로라는 확인은 아니다.
- `feature/phase3-camera`: Codex가 삭제했다. 병합된 코드는 main에 있다.
- PLAN 체크박스: Codex가 검증된 개별 항목만 `docs/phase3-completion-status`에 체크했다(main 미병합).

남은 승인 (AGENTS.md에 따라 사용자 승인 필요):
1. `feature/phase3-map`(`feature/phase3-test-course` 포함)의 main 병합. Codex는 고정 코스 검증 후 병합을 권했다.
2. `docs/phase3-completion-status`의 main 병합.

## 6. PLAN 항목별 현재 상태 (이 브랜치에서 체크 표시하지 않음)

| 항목 | 상태 |
| --- | --- |
| 3-1 입석 조작자 카메라 | 구현·병합 완료. 이동 중 수동 검증만 남음 |
| 3-2 개발용 평면 테스트맵 | 구현·Codex 검증 완료. 이동 중 수동 주행 확인 남음 |
| 3-3 OSM 범위 확정 | 대상 통로·범위·원본 파일 고정 완료(way 532526349, bbox 약 177 × 233 m), 가상 교육 코스 표시와 ODbL 방침 문서화 |
| 3-4 OSM → Bevy 변환 | 변환기와 여수 가상 교육 코스 생성 완료. 이동 중 주행 확인 남음. 주변 건물(선택 항목) 미구현 |
| 3-5 지도 실패 대비 | 지도 파일이 없거나 잘못되면 training 코스로 실행. 별도의 수동 정적 중심선은 OSM 코스가 동작하므로 만들지 않음 |

## 7. 사진 기반 가정 (실측 아님)

- 카메라 눈 위치 (0, 1.65, −2.75) m, pitch −3°, 수직 FOV 62°는 `61.jpg` 구도에 맞춘 보정값이다.
- 흰 램프 쪽을 전방으로 본 것은 임시 방향 가정이다. 후방 실내는 전방을 180° 돌린 것이다.
- 코스 치수(차로 6 m, 좁은 통로 3.0 m, 지도 임시 폭)는 모두 훈련용 임시값이다.
