# Codex Godot 화면 뼈대 인계 (2026-11-09)

작업 브랜치: feature/gd2-godot. Rust·config·PLAN·README는 변경하지 않았다.

## 지금 표시되는 것

- main.tscn: 개발/보정 코스와 지도 화면을 바꾸는 임시 상단 탐색 버튼.
- presentation/drive_screen.tscn: 코스 메시, 최소 실내·외부 차체, 입석 카메라, 조명, HUD.
- SimVehicle 클래스가 없으면 preview/vehicle_preview.gd의 **정지 미리보기**를 쓴다.
  시험 형상은 preview/course_fixture.gd에서 직접 만든 자료이며 실제 training/calibration 코스가 아니다.
  이동·충돌·시동·기어 명령·리셋·재생을 흉내 내지 않는다.
- 임시 시야 조작: 1 전방 / 2 후방 / 3 좌측 / 4 우측 / C 외부 보기.
  정식 Rust 차량이 있으면 이 키를 GDScript에서 처리하지 않고 Rust 시그널만 받는다.
- preview의 operator_view()는 config/camera.toml의 해당 숫자만 읽는 미리보기 어댑터다.
  정식 설정 파싱·검증·경고는 Rust 구현을 사용한다.
- presentation/map_screen.tscn: Camera2D 우클릭 끌기·커서 기준 휠 확대, 회색 칸,
  도로·점·경로·타일 표시 레이어와 출처 링크. 실제 지도/지역 받기/편집/저장은 아직 연결하지 않았다.
  HTTPRequest나 외부 요청은 없다. 현재 확대 한계는 표시용 임시값이며 Rust 타일 배율 계약을 확정한 값이 아니다.

## Claude 연결 지점

- drive_screen.start()는 ClassDB로 SimVehicle을 생성하고 course를 설정한 뒤 트리에 추가한다.
  course_data()에서 받은 자료만 course_mesh.build()에 넘긴다. 정식 경로에서 시험 코스를 섞지 않는다.
- vehicle_visual.gd와 camera_rig.gd는 차량의 자식이다. 외부 차체는 레이어 2,
  실내/코스는 레이어 1. 입석 카메라는 1만, 외부 카메라는 1과 2를 본다.
- course_mesh.gd는 계약 3절의 surfaces/strips/areas/markings/colliders를 표시한다.
  Godot 물리 충돌체를 만들지 않는다. 부채꼴 유효성·좌표 규칙의 최종 검증은 Rust 계약 확정 후 연결한다.
- 카메라는 operator_view()와 view_changed/debug_camera_toggled를 사용한다.
- HUD는 읽기 함수와 config_warnings/course_notices만 사용한다.
- map_screen.display_geometry()는 표시용 배열, display_tiles()는 texture/rect 배열을 받는 내부 표시 함수다.
  Rust 공개 API를 새로 확정한 것이 아니다. 서비스 어댑터는 계약 답변 이후 작성한다.
- 틱/명령/리셋/재생·요청 수명·편집/저장 결과는 구현하지 않았다.
- 정식 메뉴와 오류 화면 흐름은 후속 작업이며 현재 상단 버튼은 화면 확인을 위한 탐색 도구다.

## 수행한 확인

Rust cargo 빌드 없음. 기존 test 워크트리의 sim_godot.dll을 Codex 워크트리의 무시되는 target/debug에 복사했다.

- 캐시 없는 Godot headless editor import: --frame-delay 1000, exit 0.
- tests/smoke_test.gd: 기존 SimBridge 버전/단위 변환 검사, exit 0.
- tests/presentation_smoke.gd: 코스 생성, 물리 노드 부재, 네 방향 카메라,
  외부/실내 레이어, 지도 표시, 화면 재진입, exit 0.
- 같은 화면 검사를 Compatibility 렌더러에서도 실행, exit 0 및 stderr 오류 없음.
  실제 렌더 이미지로 입석 창·HUD·지도 출처를 확인하고 조명 과노출을 조정했다.
  이미지는 TEMP/self-driving-sim-gd2-preview 아래 보관(저장소 미포함).
- 에디터 headless import 검사는 수행했으나 사람이 Godot 에디터를 클릭하며 확인한 것은 아니다.

## 사람이 확인할 것 / 남은 일

- 실제 창에서 1~4/C 시점, 지도 끌기·휠 확대, 창 크기 변경, 화면 전환과 경고 닫기.
- Rust 클래스 연결 후 실제 코스·키보드/게임패드 주행, 오류 화면, 재생 동등성.
- 계약 v1 및 지도 서비스 연결 뒤 지역 받기·점 편집·경로·저장·취소.
- 사진과 구도 비교/사용자 승인. 현재 형상·조명은 임시값이며 측정값이 아니다.

병합·push·PLAN 완료 처리는 하지 않았다.
