extends SceneTree
## 계기판 면적·접이식 지도·차량 위치·숨김 중 안전 경고와 출처를 검사한다.

const Drive = preload("res://presentation/drive_screen.gd")
var failures := PackedStringArray()
var capture_dir := ""


## 선택적 화면 캡처 경로를 읽고 검사를 시작한다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture-dir="):
			capture_dir = arg.trim_prefix("--capture-dir=")
	call_deferred("_run")


## 코스를 열어 평상시 최소 표시와 명시적인 지도·정보 펼침을 확인한다.
func _run() -> void:
	root.size = Vector2i(1280, 720)
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("map:assets/maps/lanelet2-demo/course.toml")
	await process_frame
	var dash: Control = drive.dashboard
	_check(drive.failure.is_empty() and drive.fallback_notice.is_empty(), "코스 열기 실패")
	_check(not dash.details.visible and not dash.map_panel.visible, "정보창이 기본으로 펼쳐짐")
	var screen := dash.get_global_rect()
	var panel: Rect2 = dash.panel.get_global_rect()
	_check(panel.get_area() / screen.get_area() < 0.12, "계기판이 화면 12% 이상 가림")
	_check(panel.position.y > screen.size.y * 0.7, "계기판이 도로 중앙을 가림")
	_check(not dash.minimap.lines.is_empty(), "코스 지도 없음")
	_check(dash.minimap.has_goal, "목표 위치 없음")
	_check(dash.attribution.is_visible_in_tree() and not dash.attribution.text.is_empty(), "출처 표시 없음")
	var position: Vector3 = drive.vehicle.position
	_check(dash.minimap.vehicle_position.distance_to(Vector2(position.x, position.z)) < 0.01, "미니맵 차량 위치 오류")
	await _capture("compact")
	dash.toggle_map.pressed.emit()
	await process_frame
	_check(dash.map_panel.visible and not dash.details.visible, "지도 펼침 실패")
	await _capture("map-expanded")
	dash.toggle_info.pressed.emit()
	await process_frame
	_check(dash.details.visible and not dash.map_panel.visible, "정보 펼침 실패")
	dash.toggle_info.pressed.emit()
	dash.toggle_hud.pressed.emit()
	await create_timer(0.3).timeout
	_check(not dash.panel.visible, "계기판 숨김 실패")
	drive._command_rejected("test", "manual", "주행 중 반대 기어 거부")
	await process_frame
	_check(dash.alert_panel.visible and dash.alert.text.contains("반대 기어 거부"), "숨김 중 안전 경고 누락")
	dash.toggle_hud.pressed.emit()
	await create_timer(0.3).timeout
	_check(dash.panel.visible and dash.panel.modulate.a > 0.99, "계기판 복구 실패")
	drive.rig.set_external(true)
	await process_frame
	await _capture("compact-topdown")
	drive.free()
	for frame in 3:
		await process_frame
	if failures.is_empty():
		print("COMPACT HUD OK: 면적 · 지도 · 차량 · 펼침 · 숨김 · 안전 경고 · 출처")
		quit(0)
	else:
		for message in failures:
			printerr("COMPACT HUD FAIL: " + message)
		quit(1)


## 창 모드에서만 실제 렌더링 결과를 저장한다.
func _capture(label: String) -> void:
	if capture_dir.is_empty():
		return
	await RenderingServer.frame_post_draw
	DirAccess.make_dir_recursive_absolute(capture_dir)
	_check(root.get_texture().get_image().save_png(capture_dir.path_join(label + ".png")) == OK, "캡처 실패")


## 실패 항목을 모아 나머지 검사를 계속한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
