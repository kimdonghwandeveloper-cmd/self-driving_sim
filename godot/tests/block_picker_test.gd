extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 새 화면에서 후보·방향 확정과 미리보기 저장 및 공통 주행 연결을 검사한다.
func _init() -> void:
	call_deferred("_run")

## 실제 Rust 서비스로 스냅샷을 열고 블록 후보와 저장을 검증한다.
func _run() -> void:
	var screen := Screen.new()
	root.add_child(screen)
	await process_frame
	var opened: Dictionary = screen.picker.call("load_block_snapshot", "testdata/block-picker/graph.osm.json")
	_check(opened.get("ok", false), "snapshot: " + str(opened))
	if opened.get("ok", false):
		screen._refresh_route()
		var blocks: Array = screen.picker.call("block_items")
		_check(blocks.size() >= 3, "blocks missing")
		var start: Dictionary = blocks.filter(func(b): return b.name == "start")[0]
		var goal: Dictionary = blocks.filter(func(b): return b.name == "goal")[0]
		_check_selection_feedback(screen, blocks, start, goal)
		var start_direction: String = start.directions[0]
		var goal_direction: String = goal.directions[0]
		screen._show_candidates(start.line[0].lerp(start.line[-1], 0.5))
		_check(not screen.candidate_items.is_empty(), "candidate list empty")
		_check(screen.picker.call("block_selections").is_empty(), "query silently selected road")
		_choose(screen, start, start_direction, "start")
		_choose(screen, goal, goal_direction, "goal")
		_check(not screen.picker.call("block_route").ok, "stale preview active")
		screen._preview_blocks()
		var preview: Dictionary = screen.picker.call("block_route")
		_check(preview.ok and preview.line.size() >= 2, "preview: " + str(preview.error))
		if preview.ok:
			var name := "block-ui-test-%d" % Time.get_ticks_usec()
			var saved: Dictionary = screen.picker.call("save_blocks", name)
			_check(saved.ok, "save: " + str(saved))
			if saved.ok:
				var built: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
				_check(built.ok and built.strips.size() > 1, "replay: " + str(built.get("error", "")))
				var duplicate: Dictionary = screen.picker.call("save_blocks", name)
				_check(not duplicate.ok, "existing route overwritten")
				_check(not screen.picker.call("save_blocks", "../escape").ok, "invalid filename accepted")
				var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
				vehicle.set("read_input", false)
				vehicle.set("course", "map:" + str(saved.path))
				root.add_child(vehicle)
				await process_frame
				_check(vehicle.call("is_ready"), "vehicle replay not ready")
				if vehicle.call("is_ready"):
					vehicle.call("send_command", "engine_toggle")
					vehicle.call("send_command", "gear_drive")
					vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
					for tick in range(128): await physics_frame
					_check(float(vehicle.call("speed_mps")) > 0.2, "saved block route vehicle did not move")
				vehicle.free()
				DirAccess.remove_absolute(saved.path)
		var before: Array = screen.picker.call("block_selections")
		_check(not screen.picker.call("load_block_snapshot", "cache/no-such-block-snapshot.json").ok, "missing snapshot accepted")
		_check(screen.picker.call("block_selections") == before, "failed load discarded selection")
		_check(not screen.picker.call("choose_block", start.id, "invalid", "start").ok, "invalid direction accepted")
		_check(not screen.picker.call("edit_block_via", 0, -9223372036854775808).ok, "invalid edit accepted")
		_choose(screen, start, start_direction, "via")
		_check(screen.picker.call("block_selections") == before, "duplicate edit changed selection")
		_check(screen.picker.call("block_route").ok, "rejected edit destroyed existing valid preview")
		screen._process(0.0)
		_check(screen.drive_button.disabled, "drive enabled while failed edit highlighted")
		screen._preview_blocks()
		_check(screen.picker.call("block_route").ok, "preserved selection failed preview")
		_check(screen.failure_lines.is_empty(), "preview did not clear failure highlights")
	screen.free()
	var main: Node = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	main._show_map()
	_check(main.screen.has_method("_preview_blocks"), "main menu still opens old point picker")
	main.free()
	if failures.is_empty():
		print("BLOCK PICKER OK")
		quit(0)
	else:
		for message in failures: printerr("BLOCK PICKER FAIL: " + message)
		quit(1)

## 실제 후보 상자에서 특정 도로와 방향을 골라 확정 버튼의 처리 경로를 실행한다.
func _choose(screen: Node, block: Dictionary, direction: String, role: String) -> void:
	screen._show_candidates(block.line[0].lerp(block.line[-1], 0.5))
	for index in screen.candidate_items.size():
		var candidate: Dictionary = screen.candidate_items[index]
		if candidate.id == block.id and candidate.direction == direction:
			screen.candidate_list.select(index)
			screen._focus_candidate(index)
			screen._confirm_block(role)
			return
	_check(false, "directional candidate missing: " + role)

## 실패한 기대 동작을 모아 마지막에 함께 출력한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)

## 실패한 확정은 이전 목록을 유지하고 두 도로 강조와 방향·연결별 안내를 즉시 반환한다.
func _check_selection_feedback(screen: Node, blocks: Array, start: Dictionary, goal: Dictionary) -> void:
	_choose(screen, start, "forward", "start")
	var before: Array = screen.picker.call("block_selections")
	_choose(screen, goal, "backward", "goal")
	_check(screen.picker.call("block_selections") == before, "unreachable direction was committed")
	_check(str(screen.feedback).contains("방향"), "direction error not translated")
	var lines = screen.get("failure_lines")
	_check(lines != null and lines.size() == 2, "failed pair not highlighted")
	var parallel: Dictionary = blocks.filter(func(b): return b.name == "parallel")[0]
	_choose(screen, parallel, "forward", "goal")
	_check(screen.picker.call("block_selections") == before, "disconnected block was committed")
	_check(str(screen.feedback).contains("연결"), "connection error not distinguished")
	_choose(screen, goal, "forward", "goal")
	lines = screen.get("failure_lines")
	_check(lines != null and lines.is_empty(), "successful edit kept stale failure highlight")
	screen._clear_points()
