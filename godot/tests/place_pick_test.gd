# 4-9 건물·장소로 출발·도착 고르기 확인(오프라인). 네트워크를 쓰지 않는다. 성공 0 / 실패 1.
# 지도 화면을 만들어 저장소의 여수 스냅샷과 합성 장소 자료(places-synthetic.osm.json, 실제 OSM 자료 아님)를 열고,
# 건물 윤곽이 그리기 자료로 오는지, 이름 검색 결과 상자에서 출발 건물을, 건물 클릭으로 도착 건물을 고르면
# 경로가 계산되고 안내에 출발·도착 이름이 한국어로 나오는지, 도로에서 먼 곳의 실패 안내가 나오는지 본다.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/place_pick_test.gd
extends SceneTree

const Map = preload("res://presentation/map_screen.gd")
## 저장소에 들어 있는 여수 노드 ID 스냅샷과 그 옆 합성 장소 자료.
const SNAPSHOT := "assets/maps/yeosu-expo/graph-2026-11-06.osm.json"
const PLACES := "assets/maps/yeosu-expo/places-synthetic.osm.json"

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimRoutePicker"):
		printerr("PLACE PICK FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 지도 화면에서 스냅샷·장소를 열고 검색·클릭으로 출발·도착을 고른다.
func _run() -> void:
	var map: Node2D = Map.new()
	root.add_child(map)
	await process_frame
	var picker: Object = map.picker
	_check(picker != null, "지도 화면에 SimRoutePicker가 없음")
	if picker == null:
		_finish(map)
		return
	map._load_snapshot(SNAPSHOT)
	var status := "\n".join(picker.call("status_lines"))
	_check(status.contains("건물·장소: 받지 않음"), "장소를 받기 전 안내가 없음: " + status)
	var opened: Dictionary = picker.call("load_places_file", PLACES)
	_check(opened.ok, "장소 파일 열기 실패: " + str(opened.error))
	map._refresh_route()
	_check(map.place_outlines.size() == 3, "그릴 장소 수: %d" % map.place_outlines.size())

	map._search_places("도서관")
	_check(map.place_results.item_count == 2 and not map.place_results.disabled, "검색 결과 상자: %d" % map.place_results.item_count)
	map._select_place_result(1)
	var labels: PackedStringArray = picker.call("point_labels")
	_check(labels.size() == 1 and labels[0] == "시험 도서관 (합성)", "검색으로 고른 출발 이름: " + str(labels))

	# 체육관 윤곽 가운데를 누른 것처럼 장소를 찾는다.
	var gym := -1
	for item in picker.call("search_places", "체육관"):
		gym = int(item.index)
	_check(gym >= 0, "체육관 검색 실패")
	var outline: PackedVector2Array = map.place_outlines[gym] if gym >= 0 else PackedVector2Array()
	var center := Vector2.ZERO
	for point in outline:
		center += point / outline.size()
	_check(int(picker.call("place_at", center)) == gym, "윤곽 가운데가 체육관을 가리키지 않음")
	map._add_place(gym)
	map._refresh_route()
	var route: Dictionary = picker.call("route")
	_check(route.ok, "건물 사이 경로 계산 실패: " + str(route.error))
	status = "\n".join(picker.call("status_lines"))
	for expected in ["건물·장소: 3곳", "출발: 시험 도서관 (합성)", "도착: 시험 체육관 (합성)"]:
		_check(status.contains(expected), "안내에 '%s'가 없음: %s" % [expected, status])
	_check(map.points.size() == 2 and map.route_line.size() > 1, "지도 화면 점·경로선이 갱신되지 않음")

	# 입력할 때마다 찾기(한글 조합이 끝날 때마다 text_changed가 온다)와 빈 칸.
	map.place_search.text = ""
	map.place_search.insert_text_at_caret("체육")
	_check(map.place_results.item_count == 2, "입력 중 검색 결과 상자: %d" % map.place_results.item_count)
	map._search_typed("")
	_check(map.place_results.disabled, "빈 칸인데 결과 상자가 남음")

	# 빈 곳 검색과 장소가 없는 곳 클릭.
	map._search_places("없는 이름")
	_check(map.place_results.disabled and map.feedback.contains("없습니다"), "빈 검색 결과 안내: " + map.feedback)
	_check(int(picker.call("place_at", center + Vector2(5000, 5000))) == -1, "빈 곳이 장소로 잡힘")

	# 전국 검색(4-10): 헤드리스는 네트워크가 꺼져 있어 저장한 결과가 없으면 실패 안내가 와야 한다.
	map._search_submitted("저장되지 않은 시험 검색어 4-10")
	_check(map.search_request != 0, "전국 검색 요청이 시작되지 않음: " + map.feedback)
	for i in range(200):
		await process_frame
		if map.search_request == 0:
			break
	_check(map.search_request == 0 and map.feedback.contains("네트워크"), "오프라인 전국 검색 실패 안내: " + map.feedback)
	# 열린 지역 밖 전국 결과를 고르면 그 위치로 지도를 옮기고 도로 받기를 안내한다.
	var away: Vector2 = map.opened_region.get_center() + Vector2(5000, 0)
	map._pick_area_result({"source": "area", "index": 0, "view": away})
	_check(map.camera.position.distance_to(away) < 1.0 and map.feedback.contains("옮겼습니다"), "전국 결과로 지도 옮기기: " + map.feedback)
	# 바깥은 바다색으로 덮는다.
	_check(map.MASK_COLOR == Color("aad3df"), "바깥 덮는 색이 바다색이 아님")
	_finish(map)


## 지도 화면을 지우고 결과를 출력한 뒤 끝낸다.
func _finish(map: Node) -> void:
	map.free()
	if failures.is_empty():
		print("PLACE PICK OK: 장소 윤곽 3개 · 이름 검색으로 출발 · 건물 클릭으로 도착 · 경로 계산 · 출발·도착 안내 · 전국 검색 오프라인 안내·지도 옮기기")
		quit(0)
	else:
		for message in failures:
			printerr("PLACE PICK FAIL: " + message)
		quit(1)


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
