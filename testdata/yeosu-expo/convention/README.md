# 여수 컨벤션센터 통로

사용자가 제공한 항공 이미지의 포장 통로를 따라 만든 가상 교육 코스다. 빨간 지적 경계선은 도로 표식으로 쓰지 않았다. 이미지는 GLB의 시각 배경에 내장하지만 원본 PNG는 저장소에 포함하지 않는다.

- `trace.json`: 2048px 폭으로 표시한 이미지의 픽셀 좌표 및 OSM 교량 양 끝 기준점. 두 점으로 축척·회전·이동을 맞춘 근사 정합이다.
- `traced-roads.json`과 각 TOML: 이미지에서 추정한 중심선, 임시 폭 5~6 m. way ID는 프로젝트 내부 식별자다. 실제 차량 통행 여부나 지하·건물 실내 구조는 검증하지 않았다.
- 여섯 통로는 선택 가능한 별도 코스다. 다른 통로와 연결되어 보이는 곳에서도 주행 경계는 선택한 코스 기준이다.
- 기존 여수 GLB 원본을 Blender로 연 뒤 `build_background.py`를 실행해 재생성한다. `YEOSU_CONVENTION`은 이 디렉터리, `YEOSU_MAP_ASSET`은 `godot/assets/maps/yeosu-expo`, `YEOSU_OUTPUT`은 BLEND 출력 디렉터리, `YEOSU_REFERENCE_IMAGE`는 사용자가 제공한 원본 PNG다.
- 항공 이미지는 기존 지형보다 높고 도로보다 낮은 `0.005 m`의 표시 전용 평면이며 충돌체가 없다. 생성기는 기존 OSM 도로·보행로·철도 표시를 사진보다 위로 올리고 절차 도로를 `0.03 m`에 둔다. 절차 코스와 차량은 같은 위경도 원점에서 생성하고 시작점이 대응 도로 메시 위에 있는지 Godot 시험으로 검사한다.
- 원본 캡처의 오른쪽·아래 검은 여백은 `reference_content_size_px`로 제외하고 실제 지도 영역만 UV에 사용한다.
- 좌표 기준과 기존 지도 데이터: © OpenStreetMap contributors · https://www.openstreetmap.org/copyright.
