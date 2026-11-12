# 4-11 전국 지도 자료로 지역 열기와 경로 고르기(1·2단계) 확인(오프라인). 네트워크를 쓰지 않는다. 성공 0 / 실패 1.
# 작은 시험 자료(testdata/korea-yeosu: 실제 전국 자료에서 여수 엑스포 주변 2 km만 잘라 만든 것)를 쓴다.
# 받기 버튼 없이 클릭한 곳의 지역이 열리는지, 도로·건물이 오는지, 클릭 두 번(출발·도착)으로 경로를 계산해 저장하면 경로 파일이
# 지역 스냅샷(cache/regions)을 가리키고 다시 코스로 만들어지는지, 같은 경로를 선으로 그려도 경로가 되는지, 도로를 벗어난 선은
# 안내만 하고 경로를 두는지, 확대한 화면이 멈추면 그 가운데 지역이 자동으로 열리는지, 자료가 없을 때 만드는 방법을 안내하는지 본다.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/region_open_test.gd
extends SceneTree

const Map = preload("res://presentation/map_screen.gd")
## 시험 전국 자료 폴더(저장소 최상위 기준).
const DATA := "testdata/korea-yeosu"
## 여수 시험 경로의 출발·도착(testdata/yeosu-expo/route-junctions.toml과 같은 점).
const START := [34.75264, 127.74712]
const END := [34.74972, 127.74816]
## 시험이 저장했다가 지우는 경로 이름.
const SAVED := "region-open-test"

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimRoutePicker"):
		printerr("REGION OPEN FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 지도 화면에서 시험 자료로 지역을 열고 출발·도착·저장·선 그리기·자동 열기·자료 없음 안내를 확인한다.
func _run() -> void:
	var map: Node2D = Map.new()
	root.add_child(map)
	await process_frame
	var picker: Object = map.picker
	picker.call("use_data_dir", DATA)
	map._check_data()
	_check(map.data_ok, "시험 자료를 열지 못함: " + map.data_error)
	map.camera.zoom = Vector2.ONE * (map.get_viewport_rect().size.x / 2000.0)

	# 경로가 없으면 클릭한 곳 지역을 열고 그곳이 출발이 된다. 두 번째 클릭이 도착이고 경로가 계산된다.
	var start: Vector2 = picker.call("latlon_to_view", PackedFloat64Array(START))
	var finish: Vector2 = picker.call("latlon_to_view", PackedFloat64Array(END))
	map._click(start)
	_check(map.opened_region.has_point(start), "클릭한 곳 지역이 열리지 않음: " + map.feedback)
	_check(map.roads.size() > 100, "도로 선분이 너무 적음: %d" % (map.roads.size() / 2))
	_check(map.place_outlines.size() > 100, "건물·장소가 너무 적음: %d" % map.place_outlines.size())
	_check(not map.start_choice.is_empty() and map.end_choice.is_empty(), "첫 클릭이 출발이 아님")
	map._click(finish)
	var route: Dictionary = picker.call("route")
	_check(route.ok, "클릭 두 번 경로 계산 실패: " + str(route.error) + " / " + map.feedback)
	var clicked_line: PackedVector2Array = map.route_line.duplicate()

	# 저장한 경로 파일은 지역 스냅샷을 가리키고 다시 코스로 만들어진다.
	if route.ok:
		var saved: Dictionary = picker.call("save", SAVED, true)
		_check(saved.ok, "경로 저장 실패: " + str(saved.error))
		if saved.ok:
			var text := FileAccess.get_file_as_string(str(saved.path))
			_check(text.contains("snapshot = \"../regions/"), "경로 파일이 지역 스냅샷을 가리키지 않음: " + text)
			var built: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:cache/routes/%s.toml" % SAVED)
			_check(built.get("ok", false), "저장 경로로 코스 만들기 실패: " + str(built.get("error", "")))
			DirAccess.remove_absolute(str(saved.path))

	# 같은 길을 선으로 그리면 그 선을 따라 경로가 된다.
	map._clear_route()
	_check(map.points.is_empty() and map.start_box.edit.text == "", "경로 지우기가 점·칸을 비우지 않음")
	map.draw_toggle.button_pressed = true
	_draw_line(map, clicked_line)
	route = picker.call("route")
	_check(route.ok, "선 그리기 경로 계산 실패: " + str(route.error) + " / " + map.feedback)
	_check(map.start_box.edit.text == "(그린 선의 처음)", "선 그리기 뒤 출발 칸 안내: " + map.start_box.edit.text)
	_check(map.points.size() > 2, "그린 선이 점 여러 개로 나뉘지 않음: %d" % map.points.size())

	# 도로에서 멀리 떨어진 선은 안내만 하고 기존 경로를 둔다.
	var far := PackedVector2Array([start + Vector2(0, -3000), start + Vector2(300, -3000), start + Vector2(600, -3000)])
	_draw_line(map, far)
	_check(map.feedback.contains("도로") and bool(picker.call("route").ok), "도로 밖 선 안내: " + map.feedback)
	map.draw_toggle.button_pressed = false

	# 경로를 지우고 확대한 화면을 다른 곳에 멈추면 그 가운데 지역이 자동으로 열린다.
	map._clear_route()
	var moved := start + Vector2(600, -400)
	map.camera.position = moved
	for i in range(60):
		await process_frame
	_check(map.opened_region.get_center().distance_to(moved) < 5.0, "멈춘 화면 가운데 지역이 자동으로 열리지 않음")

	# 자료가 없으면 만드는 방법을 안내한다.
	var missing: Object = ClassDB.instantiate("SimRoutePicker")
	missing.call("use_data_dir", "testdata/없는-자료")
	var status: Dictionary = missing.call("data_status")
	_check(not status.ok and str(status.error).contains("README"), "자료 없음 안내: " + str(status.error))
	_finish(map)


## 선 그리기를 흉내 낸다: 첫 점에서 누르고, 점들을 따라 끌고, 마지막에 뗀다.
func _draw_line(map: Node2D, line: PackedVector2Array) -> void:
	if line.is_empty():
		_check(false, "그릴 선이 비어 있음")
		return
	map._start_line(line[0])
	for point in line:
		map._extend_line(point)
	map._finish_line()


## 지도 화면을 지우고 결과를 출력한 뒤 끝낸다.
func _finish(map: Node) -> void:
	map.free()
	if failures.is_empty():
		print("REGION OPEN OK: 클릭한 곳 지역 열기 · 도로·건물 · 클릭 두 번 출발·도착 경로·저장·다시 코스 · 선 그리기 경로 · 도로 밖 선 안내 · 멈춘 화면 자동 열기 · 자료 없음 안내")
		quit(0)
	else:
		for message in failures:
			printerr("REGION OPEN FAIL: " + message)
		quit(1)


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
