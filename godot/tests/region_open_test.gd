# 4-11 전국 지도 자료로 지역 열기 확인(오프라인). 네트워크를 쓰지 않는다. 성공 0 / 실패 1.
# 작은 시험 자료(testdata/korea-yeosu: 실제 전국 자료에서 여수 엑스포 주변 2 km만 잘라 만든 것)를 쓴다.
# 받기 버튼 없이 클릭한 곳의 지역이 열리는지, 도로·건물이 오는지, 점 두 개로 경로를 계산해 저장하면 경로 파일이 지역 스냅샷
# (cache/regions)을 가리키고 다시 코스로 만들어지는지, 점이 있을 때 지역 밖 클릭은 안내만 하는지, 확대한 화면이 멈추면
# 그 가운데 지역이 자동으로 열리는지, 자료가 없을 때 만드는 방법을 안내하는지 본다.
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


## 지도 화면에서 시험 자료로 지역을 열고 점·경로·저장·자동 열기·자료 없음 안내를 확인한다.
func _run() -> void:
	var map: Node2D = Map.new()
	root.add_child(map)
	await process_frame
	var picker: Object = map.picker
	picker.call("use_data_dir", DATA)
	map._check_data()
	_check(map.data_ok, "시험 자료를 열지 못함: " + map.data_error)

	# 점이 없으면 클릭한 곳 지역을 연다.
	var start: Vector2 = picker.call("latlon_to_view", PackedFloat64Array(START))
	var finish: Vector2 = picker.call("latlon_to_view", PackedFloat64Array(END))
	_check(map._ensure_region(start), "클릭한 곳 지역 열기 실패: " + map.feedback)
	_check(map.opened_region.has_point(start), "열린 지역이 클릭한 곳을 담지 않음")
	_check(map.roads.size() > 100, "도로 선분이 너무 적음: %d" % (map.roads.size() / 2))
	_check(map.place_outlines.size() > 100, "건물·장소가 너무 적음: %d" % map.place_outlines.size())
	picker.call("add_point", start)
	_check(map._ensure_region(finish), "같은 지역 안 도착 클릭이 거부됨")
	picker.call("add_point", finish)
	map._refresh_route()
	var route: Dictionary = picker.call("route")
	_check(route.ok, "경로 계산 실패: " + str(route.error))

	# 저장한 경로 파일은 지역 스냅샷을 가리키고 다시 코스로 만들어진다.
	if route.ok:
		var saved: Dictionary = picker.call("save", SAVED, true)
		_check(saved.ok, "경로 저장 실패: " + str(saved.error))
		if saved.ok:
			var text := FileAccess.get_file_as_string(str(saved.path))
			_check(text.contains("cache/regions/"), "경로 파일이 지역 스냅샷을 가리키지 않음: " + text)
			var built: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:cache/routes/%s.toml" % SAVED)
			_check(built.get("ok", false), "저장 경로로 코스 만들기 실패: " + str(built.get("error", "")))
			DirAccess.remove_absolute(str(saved.path))

	# 점이 있을 때 지역 밖 클릭은 안내만 한다.
	_check(not map._ensure_region(start + Vector2(5000, 0)) and map.feedback.contains("점 모두 삭제"), "지역 밖 클릭 안내: " + map.feedback)

	# 점을 지우고 확대한 화면을 다른 곳에 멈추면 그 가운데 지역이 자동으로 열린다.
	map._clear_points()
	var moved := start + Vector2(600, -400)
	map.camera.zoom = Vector2.ONE * (map.get_viewport_rect().size.x / 2000.0)
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


## 지도 화면을 지우고 결과를 출력한 뒤 끝낸다.
func _finish(map: Node) -> void:
	map.free()
	if failures.is_empty():
		print("REGION OPEN OK: 클릭한 곳 지역 열기 · 도로·건물 · 경로 계산·저장·다시 코스 · 지역 밖 안내 · 멈춘 화면 자동 열기 · 자료 없음 안내")
		quit(0)
	else:
		for message in failures:
			printerr("REGION OPEN FAIL: " + message)
		quit(1)


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
