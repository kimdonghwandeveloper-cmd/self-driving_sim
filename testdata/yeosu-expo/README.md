# 여수 엑스포해양공원 일대 OSM 조사 자료

> © OpenStreetMap contributors. 이 폴더의 데이터는 [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/)을 따른다.
> 저작권·라이선스 상세: https://www.openstreetmap.org/copyright
> 출처 표기와 라이선스 방침은 [`docs/map-data.md`](../../docs/map-data.md)에 있다.

> **예외:** `places-synthetic.osm.json`은 PLAN 4-9 시험용 **합성 자료**이며 실제 OSM 자료가 아니다(건물 2개·정류장 1개를 예제 경로 양 끝 옆에 임의로 둠).

`survey-2026-11-06.osm.json`은 **통로 후보 조사용**이다. 주행 코스로 고정한 자료는 `course-way-532526349.osm.json`과 `course.toml`이다.
선택 근거와 기록은 `docs/map-data.md`의 "상태" 절에 있다.

| 항목 | 값 |
| --- | --- |
| 파일 | `survey-2026-11-06.osm.json` (Overpass API JSON 응답 원본, 수정하지 않음) |
| 추출 시각 | 2026-11-06, `timestamp_osm_base` 2026-11-06T01:24:34Z |
| 서버 | `https://overpass-api.de/api/interpreter` |
| bbox (남, 서, 북, 동) | 34.7480, 127.7460, 34.7590, 127.7600 (약 1.3 × 1.2 km, 조사 범위일 뿐 코스 범위 아님) |
| 런타임 사용 | 없음. 시뮬레이터는 실행 중에 네트워크로 내려받지 않는다. |

질의문(Overpass QL):

```
[out:json][timeout:60];
(
  way["highway"](34.7480,127.7460,34.7590,127.7600);
  nwr["name"](34.7480,127.7460,34.7590,127.7600)["leisure"];
  nwr["name"](34.7480,127.7460,34.7590,127.7600)["tourism"];
);
out tags geom;
```

## Phase 4 선행 자료: 노드 ID 스냅샷

`graph-2026-11-06.osm.json`은 Phase 4(굽은 길·교차로·도로 그래프) **오프라인 검증용**이다. 같은 bbox에서 `highway` way만
`out body geom`으로 받아 노드 ID·태그·좌표를 모두 보존했다. 주행 코스로 고정한 자료가 아니다.

| 항목 | 값 |
| --- | --- |
| 파일 | `graph-2026-11-06.osm.json` (Overpass API JSON 응답 원본, 수정하지 않음, 125 898 바이트) |
| 추출 시각 | 2026-11-06, `timestamp_osm_base` 2026-11-06T07:02:21Z |
| 서버 | `https://overpass-api.de/api/interpreter` (User-Agent `self-driving-sim-dev/0.1`, 개발자가 한 번 받음) |
| bbox (남, 서, 북, 동) | 34.7480, 127.7460, 34.7590, 127.7600 (위 조사 자료와 같음, 대한민국 범위 안) |
| 내용 | way 146개, 모두 노드 ID 배열 있음, 빠진 노드 없음 |
| 런타임 사용 | 없음. 테스트에서만 읽는다. |

질의문(Overpass QL):

```
[out:json][timeout:60];
way["highway"](34.7480,127.7460,34.7590,127.7600);
out body geom;
```

검증에 쓰는 실제 사례(노드 ID 기준, 허용 종류 service·pedestrian·residential·unclassified·tertiary만 셈)는
[`docs/map-data.md`](../../docs/map-data.md)의 "Phase 4 선행 자료" 절에 있다.

## 경로 파일 예제 (Phase 4-3)

`route-junctions.toml`은 위 노드 ID 스냅샷과 찍은 점 두 개로 계획한 **경로 파일**이다. 실제 운행 경로가 아니다.
메뉴에는 나오지 않고 자동 시험이 `map:testdata/yeosu-expo/route-junctions.toml`로 연다. 저장소 테스트가 같은 점으로 다시 계획한 결과와
이 파일이 같은지 확인한다. 형식은 [`docs/map-data.md`](../../docs/map-data.md)의 "도로 그래프와 경로 탐색" 절에 있다.
