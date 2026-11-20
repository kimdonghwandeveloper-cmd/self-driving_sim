extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 도로 클릭·방향 선택·저장 후 실제 차량 방향의 계약 검사를 예약한다.
func _init() -> void:
	call_deferred("_run")

## 팝업은 확정 전 주행을 막고 hover는 강조만 하며 반대 선택은 재생 방향도 뒤집어야 한다.
func _run() -> void:
	root.content_scale_size = Vector2i.ZERO
	var screen := Screen.new()
	root.add_child(screen)
	await process_frame
	if not screen.has_method("_choose_start_direction"):
		_check(false, "direction popup selection missing")
	else:
		screen._load_snapshot("testdata/block-real/expo-internal.osm.json")
		var first: Dictionary = screen.picker.call("road_area_candidates", screen.picker.call("block_items")[0].line[0], 10.0)[0]
		var at: Vector2 = first.line[0].lerp(first.line[1], 0.5)
		var click: Vector2 = screen.get_canvas_transform() * at
		screen._select_map_position(click, false)
		_check(screen.direction_popup.visible, "road click opens direction popup")
		_check(screen.drive_button.disabled and not screen.preview_ok, "direction confirmation required")
		_check(screen.direction_popup.get_theme_stylebox("panel").corner_radius_top_left >= 10, "rounded popup")
		_check(screen.direction_buttons.size() == 2, "two arrow choices")
		_check(screen.direction_vectors[0].dot(screen.direction_vectors[1]) < -0.99, "opposite road-aligned arrows")
		var segment: Vector2 = first.line[1] - first.line[0]
		_check(absf(segment.normalized().dot(screen.direction_vectors[0])) > 0.99, "arrows follow road angle")
		screen.direction_buttons[1].mouse_entered.emit()
		_check(screen.drive_button.disabled and not screen.preview_ok, "hover does not confirm")
		await process_frame
		await _press_direction(screen.direction_buttons[0])
		_check(not screen.direction_popup.visible and screen.preview_ok, "arrow click confirms direction")
		var forward := _saved_course(screen, "forward")
		screen._select_map_position(click, false)
		_check(screen.drive_button.disabled, "reopening invalidates previous preview")
		_check(not screen.picker.call("save_road_area", "stale-direction").ok, "stale backend preview cleared")
		await process_frame
		await _press_direction(screen.direction_buttons[1])
		var reverse := _saved_course(screen, "reverse")
		if not forward.is_empty() and not reverse.is_empty():
			_check(cos(float(forward.start.heading_rad) - float(reverse.start.heading_rad)) < -0.99, "saved vehicle actually reverses")
			_check(forward.strips == reverse.strips and forward.areas == reverse.areas, "direction does not regenerate a different road area")
		for size in [Vector2i(1280,720), Vector2i(800,600)]:
			root.size = size
			for frame in 3: await process_frame
			screen._select_map_position(screen.get_canvas_transform() * at, false)
			_check(Rect2(Vector2.ZERO, Vector2(size)).encloses(screen.direction_popup.get_global_rect()), "popup stays inside viewport")
			screen._cancel_direction_popup()
			_check(not screen.direction_popup.visible and screen.drive_button.disabled, "cancel keeps driving disabled")
		screen._select_map_position(screen.get_canvas_transform() * at, false)
		var escape := InputEventKey.new()
		escape.physical_keycode = KEY_ESCAPE
		escape.pressed = true
		screen._input(escape)
		_check(not screen.direction_popup.visible, "Esc closes popup")
		screen._select_map_position(screen.get_canvas_transform() * at, false)
		var had_data: bool = screen.data_ok
		screen.data_ok = false
		screen._select_map_position(screen.get_canvas_transform() * (at + Vector2(10000, 10000)), false)
		screen.data_ok = had_data
		_check(not screen.direction_popup.visible and screen.drive_button.disabled, "blank map cancels popup")
		screen._select_map_position(screen.get_canvas_transform() * at, false)
		for arg in OS.get_cmdline_user_args():
			if arg.begins_with("--capture=") and DisplayServer.get_name() != "headless":
				root.size = Vector2i(1280,720)
				for frame in 120: await process_frame
				await RenderingServer.frame_post_draw
				_check(root.get_texture().get_image().save_png(arg.trim_prefix("--capture=")) == OK, "capture failed")
	screen.free()
	await process_frame
	if failures.is_empty():
		print("ROAD DIRECTION POPUP OK")
		quit(0)
	else:
		for message in failures: printerr("ROAD DIRECTION POPUP FAIL: " + message)
		quit(1)

## 선택 방향이 저장 파일과 공통 코스 로더까지 전달되는지 검사한다.
func _saved_course(screen: Node, suffix: String) -> Dictionary:
	var saved: Dictionary = screen.picker.call("save_road_area", "direction-test-%s-%d" % [suffix, Time.get_ticks_usec()])
	_check(saved.ok, "save direction: " + str(saved.error))
	if not saved.ok: return {}
	var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
	_check(data.ok, "replay direction")
	var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", "map:" + str(saved.path))
	root.add_child(vehicle)
	_check(vehicle.call("is_ready"), "vehicle initializes with saved direction")
	if data.ok: _check(vehicle.call("course_data").start == data.start, "vehicle uses saved start pose")
	vehicle.free()
	DirAccess.remove_absolute(saved.path)
	return data if data.ok else {}

## 헤드리스는 숨긴 지도 컨트롤의 신호를 검사하고 실제 렌더러에서는 포인터 입력을 검사한다.
func _press_direction(button: Button) -> void:
	if DisplayServer.get_name() == "headless":
		button.pressed.emit()
		return
	_check(button.is_visible_in_tree(), "arrow button visible for pointer input")
	await _click(button.get_global_rect().get_center())

## 실제 포인터 입력으로 화살표 버튼의 입력 처리와 팝업 차단 영역을 함께 검사한다.
func _click(at: Vector2) -> void:
	var move := InputEventMouseMotion.new()
	move.position = at
	Input.parse_input_event(move)
	await process_frame
	for pressed in [true, false]:
		var event := InputEventMouseButton.new()
		event.position = at
		event.button_index = MOUSE_BUTTON_LEFT
		event.pressed = pressed
		Input.parse_input_event(event)
		await process_frame

## 실패를 모아 테스트 프로세스의 종료 코드에 반영한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
