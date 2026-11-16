extends SceneTree
## 실제 마우스 입력·창 크기·안전 안내·평가값 바인딩과 탑다운 복귀를 검사한다.

const Menu = preload("res://presentation/menu_screen.gd")
const Drive = preload("res://presentation/drive_screen.gd")
var failures := PackedStringArray()
var capture_dir := ""


## 선택적인 렌더링 캡처 경로를 읽고 검사를 시작한다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture-dir="):
			capture_dir = arg.trim_prefix("--capture-dir=")
	call_deferred("_run")


## 메뉴 클릭과 모달 차단, 주행 상태, 실패 표시, 시점 왕복을 확인한다.
func _run() -> void:
	var main: Node = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	var menu: Control = main.screen
	for window_size in [Vector2i(1280, 720), Vector2i(960, 540), Vector2i(800, 600)]:
		root.size = window_size
		for frame in 4:
			await process_frame
		for button in [menu.start_button, menu.map_button, menu.controls_button, menu.settings_button, menu.courses]:
			_check(menu.get_global_rect().encloses(button.get_global_rect()), "메뉴 버튼이 화면 밖: " + button.text)
	root.size = Vector2i(1280, 720)
	await process_frame
	await _capture("menu")
	await _click(menu.controls_button)
	_check(menu._controls_dialog.visible, "도움말 클릭 실패")
	_check(menu._controls_dialog.dialog_text.contains("J: D") and menu._controls_dialog.dialog_text.contains("공통 제동"), "실제 기본 입력 안내 누락")
	menu._controls_dialog.hide()
	await process_frame
	await _click(menu.settings_button)
	_check(menu._settings_dialog.visible, "환경 안내 클릭 실패")
	menu._settings_dialog.hide()
	await process_frame
	await _click(menu.start_button)
	var drive: Node = main.screen
	_check(drive is Node3D and drive.has_method("start"), "시작 버튼 클릭 실패")
	if not drive.has_method("start"):
		main.free()
		_finish()
		return
	await process_frame
	_check(drive.failure.is_empty(), "training 초기화 실패")
	_check(drive.dashboard.state.is_visible_in_tree(), "상태 안내가 숨겨짐")
	_check(drive.dashboard.state.text.contains("시동 꺼짐"), "시동 상태 표시 누락")
	_check(drive.dashboard.metrics.text.contains("미지원"), "없는 평가값을 수치로 표시함")
	drive._command_rejected("gear_reverse", "manual", "주행 중 반대 기어 거부")
	await process_frame
	_check(drive.dashboard.state.text.contains("반대 기어 거부"), "명령 거부가 계기판에 표시되지 않음")
	drive.rig.set_external(true)
	await process_frame
	_check(drive.rig.camera.position.y > 8.0 and -drive.rig.camera.global_basis.z.y < -0.5, "탑다운 방향 오류")
	await _capture("topdown")
	drive.rig.set_external(false)
	_check(drive.rig.camera.cull_mask == 1 and drive.rig.camera.position == Vector3.ZERO, "실내 복귀 오류")
	await _capture("drive")
	main.call("_show_drive", "map:assets/maps/lanelet2-demo/course.toml")
	await process_frame
	drive = main.screen
	var values: Dictionary = drive.vehicle.call("evaluation_metrics")
	_check(values.get("active", false), "Lanelet2 평가값 미연결")
	_check(drive.dashboard.metrics.text.contains("CTE"), "CTE 표시 없음")
	drive.vehicle.set("read_input", false)
	drive.vehicle.call("send_command", "engine_toggle")
	drive.vehicle.call("send_command", "gear_drive")
	for tick in 96:
		drive.vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
		await physics_frame
	for frame in 2:
		await process_frame
	values = drive.vehicle.call("evaluation_metrics")
	_check(float(values.distance_m) > 0.1, "실제 주행 거리 갱신 없음")
	_check(drive.dashboard.speed.text != "0 km/h" and drive.dashboard.gears.text == "기어 D", "실제 속도·기어 갱신 없음")
	var displayed_distance: float = drive.dashboard.metrics.text.split("주행 거리 ")[1].split(" m")[0].to_float()
	# UI와 물리 틱의 한 프레임 차이 및 소수 첫째 자리 반올림을 허용한다.
	_check(absf(displayed_distance - float(values.distance_m)) < 0.2, "계기판 거리와 Rust 측정값 불일치")
	main.call("_show_drive", "scenario:assets/scenarios/ui-test-missing.toml")
	await process_frame
	drive = main.screen
	_check(not drive.failure.is_empty() and drive.dashboard.state.is_visible_in_tree(), "실패 상태가 숨겨짐")
	_check(drive.dashboard.state.text.contains("코스를 열 수 없습니다"), "실패 원인 표시 없음")
	main.free()
	for frame in 3:
		await process_frame
	_finish()


## 버튼의 실제 화면 위치로 누름과 뗌 이벤트를 보낸다.
func _click(button: Control) -> void:
	var position := button.get_global_rect().get_center()
	for pressed in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = MOUSE_BUTTON_LEFT
		event.position = position
		event.global_position = position
		event.pressed = pressed
		root.push_input(event)
		await process_frame


## 창 모드에서만 요청된 화면을 PNG로 저장한다.
func _capture(label: String) -> void:
	if capture_dir.is_empty():
		return
	await RenderingServer.frame_post_draw
	DirAccess.make_dir_recursive_absolute(capture_dir)
	_check(root.get_texture().get_image().save_png(capture_dir.path_join(label + ".png")) == OK, "캡처 실패")


## 실패를 모아 후속 검사도 계속 진행한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)


## 검증 결과를 출력하고 종료 코드를 반환한다.
func _finish() -> void:
	if failures.is_empty():
		print("UI INTERACTION OK: 창 크기 · 실제 클릭 · 도움말 · 상태 · 오류 · CTE · 탑다운 복귀")
		quit(0)
	else:
		for message in failures:
			printerr("UI INTERACTION FAIL: " + message)
		quit(1)
