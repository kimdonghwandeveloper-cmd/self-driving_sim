# 여수 엑스포 OSM 배경

- © OpenStreetMap contributors · [ODbL 1.0 및 출처](https://www.openstreetmap.org/copyright). 바닥 지도 그림은 OSM 표준(Mapnik) 타일(줌 17)이다.
- 사용자가 Blender 5.3 + BlenderGIS로 만든 `.blend`(도로 `Ways`, 건물·구역 `Areas`, 지도 그림 `EXPORT_OSM_MAPNIK_WM`)를
  기존 제작 도구로 변환했다. 현재 재생성은 Rust `blender-map`을 사용한다. 원본 `.blend`는 저장소에 넣지 않았다(사용자 보관, `여수맵/2104_autosave.blend`).
  새 절차와 입력 계약은 [Rust 지도 제작 안내](../../../../docs/rust-map-tools.md)를 따른다.
- 원본의 Web Mercator(EPSG:3857) 좌표를 위경도로 되돌린 뒤 `testdata/yeosu-expo/course.toml`과 같은 원점(34.7509700, 127.7493575)의
  실제 미터로 바꿨다(원본 객체의 꾸밈용 배율·위치는 쓰지 않음). Godot +X는 동쪽, +Z는 남쪽이다. 배치 변환은 필요 없다.
- Rust 도구는 새 출력 폴더에 배경 GLB와 중심선·폭 자료(`custom-roads.json`, `road-lines-v1`)를 함께 쓴다. 기존 배포 에셋은 자동으로 덮어쓰지 않는다.
- 높이: 도로 -0.04 m, 건물·구역 밑면 -0.06 m, 지도 그림 -0.08 m(주행 노면 0 m와 바닥 -0.10 m 사이).
- 건물 높이와 도로 폭 일부는 근사값이며 지형은 평면이다. GLB는 표시용이고 주행 노면·충돌 형상은 기존 코스에서 제공한다.
