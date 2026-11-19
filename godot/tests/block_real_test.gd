extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 실제 OSM 두 표본을 UI로 선택한 뒤 저장·재생·차량 이동을 검사한다.
func _init() -> void:
	call_deferred("_run")

## 본문 검사는 개발 코스나 다른 경로로 대체하지 않고 명시한 블록 ID/방향을 사용한다.
func _run() -> void:
	root.size = Vector2i(1280, 720)
	for sample in [
		["expo-internal", 532526365, 13388325415, 8664556115, 532526365, 13388325415, 8664556115],
		["expo-curved-joint", 1286142779, 772406248, 794175423, 1286142779, 794175423, 12468412566],
		["gangnam-working-area", 129731721, 4944067989, 1906756396, 504227065, 1906756396, 3089780844]
	]:
		var screen := Screen.new()
		root.add_child(screen)
		await process_frame
		screen._load_snapshot("testdata/block-real/%s.osm.json" % sample[0])
		await process_frame
		_check(screen.opened_region.has_area(), str(sample[0]) + ": region missing")
		_check(screen.toolbar_panel.get_global_rect().size.x < root.size.x * 0.45, "sidebar leaves no map area")
		_choose(screen, sample[1], sample[2], sample[3], "start")
		_choose(screen, sample[4], sample[5], sample[6], "goal")
		if sample[0] == "gangnam-working-area":
			_check(not screen.picker.call("block_diagnostics").is_empty(), "quarantined restriction warning missing")
		screen._preview_blocks()
		var route: Dictionary = screen.picker.call("block_route")
		_check(route.ok, str(sample[0]) + ": preview: " + str(route.error))
		if route.ok:
			var name := "real-block-test-%d" % Time.get_ticks_usec()
			var saved: Dictionary = screen.picker.call("save_blocks", name)
			_check(saved.ok, str(saved.get("error", "")))
			if saved.ok:
				var text := FileAccess.get_file_as_string(saved.path)
				_check(text.contains('block-route-v1') and text.contains('osm-road-blocks-v2') and text.contains('road-network-v3'), "saved version contract missing")
				var course: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
				_check(course.ok, "saved real map replay failed")
				if sample[0] == "gangnam-working-area" and course.ok:
					_check("\n".join(course.notices).contains("차단"), "saved course lost restriction warning")
				var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
				vehicle.set("read_input", false)
				vehicle.set("course", "map:" + str(saved.path))
				root.add_child(vehicle)
				await process_frame
				_check(vehicle.call("is_ready"), "actual map vehicle not ready")
				if vehicle.call("is_ready"):
					_check(vehicle.call("course_data").start == course.start, "vehicle replay start differs")
					vehicle.call("send_command", "engine_toggle")
					vehicle.call("send_command", "gear_drive")
					vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
					for tick in range(128): await physics_frame
					_check(float(vehicle.call("speed_mps")) > 0.2, "actual map vehicle did not move")
				vehicle.free()
				DirAccess.remove_absolute(saved.path)
			print("REAL BLOCK UI: %s · %.1f m · selected/saved/replayed/driven" % [sample[0], route.length_m])
		screen.free()
	if failures.is_empty():
		print("REAL BLOCK UI OK")
		quit(0)
	else:
		for message in failures: printerr("REAL BLOCK UI FAIL: " + message)
		quit(1)

## 원본 way와 양 끝 노드로 후보를 특정하고 실제 후보 상자·확정 버튼 로직으로 선택한다.
func _choose(screen: Node, way: int, start: int, end: int, role: String) -> void:
	for block in screen.picker.call("block_items"):
		if int(block.way_id) != way: continue
		var direction: String = block.direction
		if int(block.from_node) == start and int(block.to_node) == end: pass
		elif int(block.to_node) == start and int(block.from_node) == end:
			direction = "backward" if direction == "forward" else "forward"
		else: continue
		var index := maxi(0, (block.line.size() - 2) / 2)
		screen._show_candidates(block.line[index].lerp(block.line[index+1], 0.5))
		for candidate in screen.candidate_items.size():
			var item: Dictionary = screen.candidate_items[candidate]
			if item.id == block.id and item.direction == direction:
				screen.candidate_list.select(candidate)
				screen._focus_candidate(candidate)
				screen._confirm_block(role)
				return
	_check(false, "exact source traversal not found: %s %s->%s" % [way, start, end])

## 실패 항목을 모아 잘못된 성공 출력 없이 종료한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
