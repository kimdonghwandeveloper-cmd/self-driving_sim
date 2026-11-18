extends SceneTree
## 약 10 km 합성 도로의 제한 옵션·저장·Godot 코스 생성과 전국 자료의 지역 확장을 검사한다.

const Map = preload("res://presentation/map_screen.gd")
var failures := PackedStringArray()
var paths := PackedStringArray()

## 첫 프레임 뒤 실제 지도 UI와 확장을 초기화해 시험한다.
func _init() -> void:
	call_deferred("_run")

## 임시 합성 도로를 열어 제한 도로 옵션과 장거리 코스·저장 판본을 확인한다.
func _run() -> void:
	var map := Map.new()
	root.add_child(map)
	await process_frame
	var snapshot := ProjectSettings.globalize_path("res://../cache/routes/long-route-test-%d.osm.json" % OS.get_process_id())
	DirAccess.make_dir_recursive_absolute(snapshot.get_base_dir())
	paths.append(snapshot)
	var ids := []
	var geometry := []
	for i in 21:
		ids.append(i + 1)
		geometry.append({"lat": 34.75, "lon": 127.75 + rad_to_deg(i * 500.0 / (6371008.8 * cos(deg_to_rad(34.75))))})
	var file := FileAccess.open(snapshot, FileAccess.WRITE)
	file.store_string(JSON.stringify({"generator": "synthetic 10km regression fixture, not actual road", "elements": [{"type": "way", "id": 190, "nodes": ids, "geometry": geometry, "tags": {"highway": "service", "access": "private", "oneway": "yes"}}]}))
	file.close()
	map._load_snapshot(snapshot)
	var picker: Object = map.picker
	var a: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([34.75, 127.7501095]))
	var b: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([34.75, float(geometry[20].lon) - 0.0001095]))
	picker.call("add_point", a)
	picker.call("add_point", b)
	_check(not picker.call("route").ok, "기본 모드에서 제한 도로를 허용함")
	map.restricted_roads.button_pressed = true
	map._refresh_route()
	var route: Dictionary = picker.call("route")
	_check(route.ok and route.length_m > 9900.0, "10 km 코스 생성 실패: " + str(route.error))
	if route.ok:
		var saved: Dictionary = picker.call("save", "long-route-regression-%d" % OS.get_process_id(), false)
		_check(saved.ok, "장거리 경로 저장 실패")
		if saved.ok:
			paths.append(str(saved.path))
			var text := FileAccess.get_file_as_string(str(saved.path))
			_check(text.contains("include_restricted = true") and text.contains("max_route_length_m = 10000"), "저장 경로 정책 누락")
			var course: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
			_check(course.get("ok", false), "장거리 Godot 코스 로드 실패: " + str(course.get("error", "")))
			_check(str(course.get("notices", [])).contains("실제 출입 허가 아님"), "제한 도로 안내 누락")
	map.restricted_roads.button_pressed = false
	_check(not picker.call("route").ok, "옵션 해제 후에도 제한 도로 경로가 유지됨")
	picker.call("clear_points")
	map.restricted_roads.button_pressed = true
	picker.call("add_point", b)
	picker.call("add_point", a)
	_check(not picker.call("route").ok, "제한 옵션이 일방통행 규칙까지 해제함")
	if "--full-data" in OS.get_cmdline_user_args():
		_check_expansion(map)
	map.free()
	for path in paths:
		DirAccess.remove_absolute(path)
	if failures.is_empty():
		print("LONG ROUTE OK: 약 10 km · 제한 도로 옵션 · 저장·재생 · 일방통행 · 지역 확장")
		quit(0)
	else:
		for message in failures:
			printerr("LONG ROUTE FAIL: " + message)
		quit(1)

## 전국 팩에서 출발과 이름을 보존하며 8 km 떨어진 지역까지 확장하고 과도한 범위는 원상 보존한다.
func _check_expansion(map: Node2D) -> void:
	var picker: Object = map.picker
	var opened: Dictionary = picker.call("open_region", PackedFloat64Array([34.7526, 127.7471]))
	_check(opened.ok, "지역 확장 시험의 출발 지역 열기 실패")
	if not opened.ok:
		return
	var a: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([34.7526, 127.7471]))
	picker.call("add_point", a)
	var before: PackedVector2Array = picker.call("points")
	var result: Dictionary = picker.call("expand_route_region", PackedFloat64Array([34.8236, 127.7471]), true)
	_check(result.ok and picker.call("points") == before, "8 km 지역 확장이 출발을 보존하지 않음: " + str(result.error))
	var region: Rect2 = picker.call("loaded_region")
	_check(region.size.y > 9000.0, "지역 확장이 여전히 2 km에 제한됨")
	result = picker.call("expand_route_region", PackedFloat64Array([36.0, 127.7471]), true)
	_check(not result.ok and picker.call("points") == before and picker.call("loaded_region") == region, "과도한 지역 확장 실패가 기존 상태를 바꿈")

## 실패를 모아 시험 종료 시 보고한다.
func _check(ok: bool, message: String) -> void:
	if not ok:
		failures.append(message)
