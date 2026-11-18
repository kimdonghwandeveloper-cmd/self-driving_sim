extends SceneTree
## OSM 지역 이동·실제 클릭·저장·주행을 검사한다. --full-data는 준비된 전국 자료의 8개 도시도 확인한다.

var failures := PackedStringArray()
var capture_dir := ""
var main: Node

## 캡처 인자를 읽고 첫 프레임 이후 시험을 시작한다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture-dir="):
			capture_dir = arg.trim_prefix("--capture-dir=")
	call_deferred("_run")

## 작은 자료로 재현 가능한 실제 클릭 흐름을 검사하고 요청 시 전국 자료도 검사한다.
func _run() -> void:
	main = load("res://main.tscn").instantiate()
	root.add_child(main)
	root.size = Vector2i(1280, 720)
	for frame in 4:
		await process_frame
	await _click(main.screen.map_button.get_global_rect().get_center())
	var map: Node2D = main.screen
	_check(map.has_method("_drive_route"), "OSM 지도 진입 실패")
	if not map.has_method("_drive_route"):
		_finish()
		return
	map.picker.call("use_data_dir", "testdata/korea-yeosu")
	map._check_data()
	map.coordinates.text = "34.75264,127.74712"
	map._jump_coordinate_button()
	var requested_region: Rect2 = map.opened_region
	for frame in 45:
		await process_frame
	_check(map.opened_region == requested_region, "좌표 이동 후 멈췄는데 다른 지역이 자동으로 열림")
	for window_size in [Vector2i(800, 600), Vector2i(1280, 720)]:
		root.size = window_size
		for frame in 4:
			await process_frame
		_check(map.get_viewport_rect().encloses(map.toolbar_panel.get_global_rect()), "지도 도구 줄이 창 밖으로 나감")
	map._fit_region()
	await process_frame
	_check(map.data_ok and map.roads.size() > 100, "자료 또는 지역 열기 실패")
	_check(map.drive_button.disabled, "선택 전 주행 허용")
	var region: Rect2 = map.opened_region
	map._jump_coordinates("잘못된 좌표")
	_check(map.feedback.contains("숫자") and map.opened_region == region, "잘못된 좌표가 지역을 바꿈")
	for pos in [[34.75264, 127.74712], [34.74972, 127.74816]]:
		var view: Vector2 = map.picker.call("latlon_to_view", PackedFloat64Array(pos))
		map.camera.force_update_scroll()
		await _click(map.get_canvas_transform() * view)
	_check(map.points.size() == 2, "실제 도로 클릭으로 출발·도착을 선택하지 못함")
	_check(not map.drive_button.disabled, "선택 경로 주행 버튼 비활성")
	map._jump_coordinates("37.4979,127.0276")
	_check(map.feedback.contains("새 경로") and map.points.size() == 2, "지역 이동이 선택을 삭제함")
	map._fit_route()
	for frame in 4:
		await process_frame
	await _capture("osm-route-selected")
	await _click(map.drive_button.get_global_rect().get_center())
	await _check_drive_and_return()
	if "--full-data" in OS.get_cmdline_user_args():
		await _check_nationwide()
	_finish()

## 준비된 전국 자료에서 도시별 실제 지역을 열고 주행 가능한 짧은 경로를 찾아 같은 주행 화면으로 연다.
func _check_nationwide() -> void:
	for title in ["여수 엑스포", "서울 강남", "부산 시청", "대구 시청", "인천 시청", "광주 시청", "대전 시청", "제주 시청"]:
		main._show_map()
		var map: Node2D = main.screen
		_check(map.data_ok, "전국 자료 없음: " + map.data_error)
		if not map.data_ok:
			return
		var city: Vector2 = map.CITY_PRESETS[title]
		map._jump_coordinates("%.6f,%.6f" % [city.x, city.y])
		_check(map.opened_region.has_area() and map.roads.size() > 0, "도시 지역 열기 실패: " + title)
		var selected := _find_short_route(map)
		_check(selected, "주행 가능한 표본 경로 없음: " + title)
		if not selected:
			continue
		map._refresh_route()
		print("NATIONWIDE SAMPLE: %s · %d road segments · %.1f m" % [title, map.roads.size() / 2, map.picker.call("route").length_m])
		map._drive_route()
		await _check_drive_and_return()

## 도로 선분 내부의 두 점을 양방향으로 시험해 유효한 짧은 코스 하나를 고른다.
func _find_short_route(map: Node2D) -> bool:
	var segments: PackedVector2Array = map.picker.call("road_segments")
	var flags: PackedByteArray = map.picker.call("road_usable")
	var attempts := 0
	for i in flags.size():
		var a := segments[i * 2]
		var b := segments[i * 2 + 1]
		if flags[i] == 0 or a.distance_to(b) < 60.0 or a.distance_to(b) > 500.0:
			continue
		var start := a.lerp(b, 0.2)
		var end := a.lerp(b, 0.8)
		if not map.opened_region.has_point(start) or not map.opened_region.has_point(end):
			continue
		for pair in [[start, end], [end, start]]:
			map.picker.call("clear_points")
			map.picker.call("add_point", pair[0])
			map.picker.call("add_point", pair[1])
			if map.picker.call("route").ok:
				return true
		attempts += 1
		if attempts >= 80:
			break
	return false

## 저장된 OSM 코스가 대체 없이 열리고 실제 차량이 움직이며 재로드 결과가 같은지 확인한다.
func _check_drive_and_return() -> void:
	await process_frame
	var drive: Node = main.screen
	_check(drive.has_method("start"), "주행 화면 전환 실패")
	if not drive.has_method("start"):
		return
	_check(drive.failure.is_empty() and drive.fallback_notice.is_empty(), "선택 경로 로드 실패")
	var name := str(drive.vehicle.get("course"))
	_check(name.begins_with("map:cache/routes/osm-drive-"), "선택 OSM 코스를 사용하지 않음")
	var data: Dictionary = drive.vehicle.call("course_data")
	var loaded: Dictionary = ClassDB.class_call_static("SimCourse", "build", name)
	_check(loaded.get("ok", false) and loaded.get("start") == data.get("start"), "저장 경로 재로드 불일치")
	var initial: Vector3 = drive.vehicle.position
	drive.vehicle.set("read_input", false)
	drive.vehicle.call("send_command", "engine_toggle")
	drive.vehicle.call("send_command", "gear_drive")
	for tick in 96:
		drive.vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
		await physics_frame
	_check(drive.vehicle.position.distance_to(initial) > 0.1, "차량이 움직이지 않음")
	main._show_menu()
	await process_frame
	_check(name in main.screen.course_names, "저장 경로가 메뉴에 없음")
	if name.begins_with("map:cache/routes/osm-drive-"):
		DirAccess.remove_absolute(ProjectSettings.globalize_path("res://..").path_join(name.trim_prefix("map:")))

## 실제 마우스 이벤트를 보내 지도·버튼 입력 경로를 함께 검사한다.
func _click(point: Vector2) -> void:
	var motion := InputEventMouseMotion.new()
	motion.position = point
	root.push_input(motion)
	for pressed in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = MOUSE_BUTTON_LEFT
		event.position = point
		event.global_position = point
		event.pressed = pressed
		root.push_input(event)
		await process_frame

## 요청한 렌더링 시험에서 지도·선택 경로·도구 줄을 함께 캡처한다.
func _capture(label: String) -> void:
	if capture_dir.is_empty():
		return
	await RenderingServer.frame_post_draw
	DirAccess.make_dir_recursive_absolute(capture_dir)
	_check(root.get_texture().get_image().save_png(capture_dir.path_join(label + ".png")) == OK, "캡처 실패")

## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)

## 모든 화면을 정리하고 검증 결과를 출력한다.
func _finish() -> void:
	main.free()
	if failures.is_empty():
		print("OSM ROUTE DRIVE OK: 지역 이동 · 클릭 · 저장 · 주행 · 재로드")
		quit(0)
	else:
		for message in failures:
			printerr("OSM ROUTE DRIVE FAIL: " + message)
		quit(1)
