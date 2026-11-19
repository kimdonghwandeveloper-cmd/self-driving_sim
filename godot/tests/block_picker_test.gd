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
		var start_direction: String = start.directions[0]
		var goal_direction: String = goal.directions[0]
		screen._show_candidates(start.line[0].lerp(start.line[-1], 0.5))
		_check(not screen.candidate_items.is_empty(), "candidate list empty")
		_check(screen.picker.call("block_selections").is_empty(), "query silently selected road")
		_check(screen.picker.call("choose_block", start.id, start_direction, "start").ok, "start selection")
		_check(screen.picker.call("choose_block", goal.id, goal_direction, "goal").ok, "goal selection")
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
				DirAccess.remove_absolute(saved.path)
		_check(not screen.picker.call("choose_block", start.id, "invalid", "start").ok, "invalid direction accepted")
	screen.free()
	if failures.is_empty():
		print("BLOCK PICKER OK")
		quit(0)
	else:
		for message in failures: printerr("BLOCK PICKER FAIL: " + message)
		quit(1)

## 실패한 기대 동작을 모아 마지막에 함께 출력한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
