# 여수 엑스포해양공원 일대 OSM 조사 자료

> © OpenStreetMap contributors. 이 폴더의 데이터는 [ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/)을 따른다.
> 출처 표기와 라이선스 방침은 [`docs/map-data.md`](../../../docs/map-data.md)에 있다.

이 자료는 **통로 후보 조사용**이다. 주행 코스용으로 선택한 구역 데이터가 아니다.
대상 통로는 아직 정하지 않았다(`docs/map-data.md` 참고).

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
