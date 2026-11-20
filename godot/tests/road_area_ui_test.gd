extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 도로 하나 선택만으로 자유 주행을 시작하는 UI 계약을 검사한다.
func _init() -> void:
	call_deferred("_run")

## 흰색 패널·역할 버튼 제거·영역 강조·저장 재생·차량 진입을 실제 서비스로 검증한다.
func _run() -> void:
	var screen := Screen.new()
	root.add_child(screen)
	await process_frame
	var texts: Array[String] = []
	for button in screen.find_children("*", "Button", true, false): texts.append(button.text)
	_check(not texts.has("출발로 지정") and not texts.has("도착으로 지정"), "endpoint selection must be absent")
	_check(screen.drive_button.text == "도로 생성 후 주행", "free-drive action required")
	_check(screen.toolbar_panel.get_theme_stylebox("panel").bg_color.r > 0.9, "white sidebar required")
	_check(screen.drive_button.disabled, "cannot drive before selection")
	if screen.has_method("_select_area"):
		screen._load_snapshot("testdata/block-real/expo-internal.osm.json")
		var items: Array = screen.picker.call("block_items")
		_check(not items.is_empty(), "real OSM roads required")
		screen._select_area(str(items[0].id))
		_check(not screen.drive_button.disabled, "one road enables driving without endpoints")
		_check(not screen.area_lines.is_empty(), "generated roads highlighted")
		for arg in OS.get_cmdline_user_args():
			if arg.begins_with("--capture=") and DisplayServer.get_name() != "headless":
				for frame in 8: await process_frame
				await RenderingServer.frame_post_draw
				_check(root.get_texture().get_image().save_png(arg.trim_prefix("--capture=")) == OK, "screenshot failed")
		var courses: Array[String] = []
		screen.drive_requested.connect(func(course: String): courses.append(course))
		screen._drive_route()
		_check(courses.size() == 1, "drive emits saved course")
		if not courses.is_empty():
			var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", courses[0])
			_check(data.ok and data.goal.is_empty(), "reloaded free-drive course has no goal")
			var vehicle = ClassDB.instantiate("SimVehicle")
			vehicle.set("course", courses[0])
			vehicle.set("read_input", false)
			root.add_child(vehicle)
			await process_frame
			_check(vehicle.call("is_ready"), "vehicle enters selected area")
			vehicle.call("send_command", "engine_toggle")
			vehicle.call("send_command", "gear_drive")
			vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
			for tick in 128: await physics_frame
			_check(float(vehicle.call("speed_mps")) > 0.2, "free-drive vehicle moves without a route")
			await process_frame
			vehicle.free()
			DirAccess.remove_absolute(courses[0].trim_prefix("map:"))
		screen._clear_points()
		_check(screen.drive_button.disabled and screen.area_lines.is_empty(), "clear invalidates preview")
	else:
		_check(false, "road area selection missing")
	screen.free()
	await process_frame
	if failures.is_empty(): print("ROAD AREA UI OK")
	else:
		for message in failures: printerr("ROAD AREA UI FAIL: " + message)
	if failures.is_empty(): quit(0)
	else: quit(1)

## 실패 원인을 모아 테스트 프로세스를 실패로 종료한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
