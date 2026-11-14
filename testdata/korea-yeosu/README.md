# 시험용 전국 지도 자료 조각 (여수 엑스포 주변)

`godot/tests/region_open_test.gd`가 쓰는 작은 전국 지도 자료(PLAN 4-11 형식 `korea-data-v1`)입니다. 실제 배포용 자료(`data/korea/`)와 형식이 같고,
범위만 여수 엑스포해양공원 주변 약 2 km(`34.743,127.737,34.759,127.757`)로 잘랐습니다.

- 원본: Geofabrik `south-korea-latest.osm.pbf` (2026-11-11자, 원본 이름·SHA-256은 `manifest.toml`)
- 만든 명령(저장소 최상위): `sim-tools --build-korea-data <PBF> --out testdata/korea-yeosu --clip 34.743,127.737,34.759,127.757`
- 형식이 바뀌면(`korea-data-v1` → 다음 판) 같은 명령으로 다시 만듭니다.

지도 자료 © OpenStreetMap contributors, [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/).
시험 전용이며 메뉴·코스 목록에는 나오지 않습니다.
