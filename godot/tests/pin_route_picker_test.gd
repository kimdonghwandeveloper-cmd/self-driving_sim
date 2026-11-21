extends SceneTree

## 기본 지도에서 목적지를 먼저 골라도 출발과 도착이 뒤바뀌지 않는지 검사한다.
func _initialize() -> void:
	call_deferred("_run")

## 검색 결과와 지도 클릭을 섞어 여수 경로를 만들고 저장 코스를 연다.
func _run() -> void:
	var screen = load("res://presentation/map_screen.tscn").instantiate()
	root.add_child(screen)
	await process_frame
	if not screen.has_method("_set_pin"):
		printerr("PIN ROUTE FAIL: default screen has no independent pins")
		quit(1)
		return
	screen.picker.call("use_data_dir", "testdata/korea-yeosu")
	screen._check_data()
	var start: Vector2 = screen.picker.call("latlon_to_view", PackedFloat64Array([34.75264, 127.74712]))
	var goal: Vector2 = screen.picker.call("latlon_to_view", PackedFloat64Array([34.74972, 127.74816]))
	if screen.fields.size() != 2 or screen.has_method("_choose_start_direction"):
		quit(1)
		return
	screen._show_results(1, [{"view": goal, "name": "여수 도착"}])
	screen.results[1].item_selected.emit(1)
	if not screen.drive_button.disabled or screen.pins[0] != null:
		quit(1)
		return
	screen._select_endpoint(false)
	screen._select_map_position(screen.get_canvas_transform() * start, false)
	var route: Dictionary = screen.picker.call("route")
	if not route.ok or screen.pins[1] != goal or screen.drive_button.disabled:
		printerr("PIN ROUTE FAIL: ", route.error)
		quit(1)
		return
	var saved: Dictionary = screen.picker.call("save", "pin-test-%d" % Time.get_ticks_usec(), false)
	if not saved.ok:
		printerr(saved.error)
		quit(1)
		return
	var course: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
	var path := ProjectSettings.globalize_path("res://").path_join("../" + str(saved.path)).simplify_path()
	DirAccess.remove_absolute(path)
	screen._set_pin(1, start)
	if not screen.drive_button.disabled or screen.pins[0] == null or screen.pins[1] == null:
		printerr("PIN ROUTE FAIL: invalid route must retain both pins and disable driving")
		quit(1)
		return
	screen._clear_points()
	screen._show_results(0, [{"view": start, "name": "여수 출발"}])
	screen.results[0].item_selected.emit(1)
	screen._show_results(1, [{"view": goal, "name": "여수 도착"}])
	screen.results[1].item_selected.emit(1)
	if not screen.route_ready:
		printerr("PIN ROUTE FAIL: both endpoints must support search")
		quit(1)
		return
	print("PIN ROUTE: ", route.length_m, " m")
	screen.free()
	await process_frame
	quit(0 if course.ok and not course.goal.is_empty() else 1)
