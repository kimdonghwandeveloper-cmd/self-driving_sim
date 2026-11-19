extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 기본 지도 도구의 크기와 접힘 상태 및 사용자 오류 안내를 먼저 검사한다.
func _init() -> void:
	call_deferred("_run")

## 작은 창에서도 지도 영역이 남고 진단 문자열이 기본 화면에 노출되지 않는지 확인한다.
func _run() -> void:
	root.content_scale_size = Vector2i.ZERO
	var screen := Screen.new()
	root.add_child(screen)
	await process_frame
	for size in [Vector2i(1280, 720), Vector2i(960, 540), Vector2i(800, 600)]:
		root.size = size
		for frame in 4: await process_frame
		var panel: Rect2 = screen.toolbar_panel.get_global_rect()
		_check(panel.size.x <= size.x * 0.45, "panel hides most of map: " + str(size))
		_check(Rect2(Vector2.ZERO, Vector2(size)).encloses(panel), "panel leaves viewport: " + str(panel) + " viewport " + str(size))
	_check(not screen.advanced.is_visible_in_tree(), "advanced controls visible by default")
	for button in screen.toolbar.find_children("*", "Button", true, false):
		_check(not (button.is_visible_in_tree() and button.text in ["경유 ↑", "경유 ↓"]), "reorder controls clutter default UI")
	screen._apply_selection_result({"ok": false, "error": 'block route: BlockRouteFailure { stage: Network, node_id: Some(806285558), message: "block too short for joint cut" }'}, "")
	screen._process(0.0)
	_check(screen.feedback.contains("짧은 도로"), "short block error has no user explanation")
	_check(not screen.status.text.contains("BlockRouteFailure"), "internal diagnostic leaked to default status")
	_check(screen.drive_button.disabled, "drive enabled after preview failure")
	var toggle = screen.get("details_toggle")
	if toggle != null:
		toggle.button_pressed = true
		for frame in 4: await process_frame
		_check(screen.advanced.is_visible_in_tree(), "advanced tools cannot be opened")
		_check(Rect2(Vector2.ZERO, Vector2(root.size)).encloses(screen.toolbar_panel.get_global_rect()), "expanded tools leave viewport: " + str(screen.toolbar_panel.get_global_rect()))
	else:
		_check(false, "details toggle missing")
	var capture := ""
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture="): capture = arg.trim_prefix("--capture=")
	if not capture.is_empty() and DisplayServer.get_name() != "headless":
		if toggle != null: toggle.button_pressed = false
		root.size = Vector2i(1280, 720)
		screen._clear_points()
		screen._load_snapshot("testdata/block-real/expo-internal.osm.json")
		for frame in 6: await process_frame
		await RenderingServer.frame_post_draw
		_check(root.get_texture().get_image().save_png(capture) == OK, "capture failed")
	screen.free()
	if failures.is_empty():
		print("BLOCK MAP UI OK")
		quit(0)
	else:
		for message in failures: printerr("BLOCK MAP UI FAIL: " + message)
		quit(1)

## 기대 동작 실패를 누적해 검사 종료 때 출력한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
