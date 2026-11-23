extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 실제 엑스포 내부 도로의 방향별 선택·저장·재생 시험을 예약한다.
func _init() -> void:
	call_deferred("_run")

## 남쪽 선택 시 북쪽 역 주변을 포함하지 않고 미리보기와 생성 코스가 같은 블록을 사용하는지 검사한다.
func _run() -> void:
	var screen := Screen.new()
	root.add_child(screen)
	await process_frame
	screen._load_snapshot("testdata/junction-regressions/expo-region.osm.json")
	await process_frame
	var at: Vector2 = screen.picker.call("latlon_to_view", PackedFloat64Array([34.7505655,127.7489806]))
	var candidates: Array = screen.picker.call("road_area_candidates", at, 20.0)
	_check(not candidates.is_empty(), "expo internal road candidate missing")
	if not candidates.is_empty():
		var road: Dictionary = candidates[0]
		var reverse_south: bool = road.line[-1].y < road.line[0].y
		screen._select_area(str(road.id), reverse_south)
		_check(screen.preview_ok, "south preview failed: " + screen.technical_feedback)
		var south: Array = screen.area_lines.duplicate()
		var north_limit: Vector2 = screen.picker.call("latlon_to_view", PackedFloat64Array([34.7535,127.749]))
		for line in south:
			for point in line: _check(point.y > north_limit.y, "south selection included station-side road")
		if screen.preview_ok:
			var saved: Dictionary = screen.picker.call("save_road_area", "expo-directional-%d" % Time.get_ticks_usec())
			_check(saved.ok, str(saved.error))
			if saved.ok:
				var text := FileAccess.get_file_as_string(saved.path)
				_check(text.contains("connected-forward-v1") and text.contains("selected_blocks"), "stored selection policy missing")
				var course: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
				_check(course.ok and course.goal.is_empty(), "directional replay failed")
				var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
				vehicle.set("read_input", false)
				vehicle.set("course", "map:" + str(saved.path))
				root.add_child(vehicle)
				_check(vehicle.call("is_ready"), "directional vehicle failed")
				vehicle.call("send_command", "engine_toggle")
				vehicle.call("send_command", "gear_drive")
				vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
				for tick in 128: await physics_frame
				_check(float(vehicle.call("speed_mps")) > 0.2, "directional vehicle did not move")
				await process_frame
				vehicle.free()
				DirAccess.remove_absolute(saved.path)
		for arg in OS.get_cmdline_user_args():
			if arg.begins_with("--capture=") and DisplayServer.get_name() != "headless":
				for frame in 120: await process_frame
				await RenderingServer.frame_post_draw
				_check(root.get_texture().get_image().save_png(arg.trim_prefix("--capture=")) == OK, "capture failed")
		screen._select_area(str(road.id), not reverse_south)
		_check(screen.preview_ok, "north preview failed: " + screen.technical_feedback)
		_check(not south.is_empty() and south != screen.area_lines, "direction must change the generated road selection")
		print("EXPO DIRECTIONAL: south %d roads / north %d roads" % [south.size(),screen.area_lines.size()])
	screen.free()
	await process_frame
	if failures.is_empty():
		print("DIRECTIONAL GENERATION OK")
		quit(0)
	else:
		for message in failures: printerr(message)
		quit(1)

## 실패 사유를 누적하고 성공 출력과 별개로 종료 코드를 결정한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
