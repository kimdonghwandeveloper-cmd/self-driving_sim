extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
const CourseMesh = preload("res://presentation/course_mesh.gd")
var failures: Array[String] = []

## 전체 실제 지역의 원본 도로 선택·저장·재생·메시·차량 이동 시험을 시작한다.
func _init() -> void:
	call_deferred("_run")

## 기존 저장 파일 API의 호환성은 방향·경유 순서를 고정해 별도로 검사한다.
func _run() -> void:
	for sample in [["expo", [[1286142779, false]], 350, 1016.3], ["gangnam", [[129731721, true], [504227065, false]], 973, 252.9]]:
		var screen := Screen.new()
		root.add_child(screen)
		await process_frame
		var path := "testdata/junction-regressions/%s-region.osm.json" % sample[0]
		screen._load_snapshot(path)
		var blocks: Array = screen.picker.call("block_items")
		_check(blocks.size() == sample[2], "source blocks missing: " + str(sample[0]))
		var file := ProjectSettings.globalize_path("res://").path_join("../" + path).simplify_path()
		var source: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(file))
		var selected: Array = []
		for spec in sample[1]: selected.append_array(_source_blocks(blocks, source, int(spec[0]), bool(spec[1])))
		_check(selected.size() >= 2, "source route not mapped")
		for i in selected.size():
			_choose(screen, selected[i], "start" if i == 0 else ("goal" if i == selected.size()-1 else "via"))
		screen.picker.call("preview_blocks")
		var route: Dictionary = screen.picker.call("block_route")
		_check(route.ok, "%s preview: %s" % [sample[0], route.error])
		if route.ok:
			_check(absf(float(route.length_m) - float(sample[3])) < 0.2, "source way was skipped or detoured")
			var saved: Dictionary = screen.picker.call("save_blocks", "junction-region-%s-%d" % [sample[0], Time.get_ticks_usec()])
			_check(saved.ok, str(saved))
			if saved.ok:
				var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
				_check(data.ok and data.strips.size() >= blocks.size(), "replay lost buffered block surfaces")
				var mesh := CourseMesh.new()
				root.add_child(mesh)
				mesh.build(data)
				_check(mesh.get_child_count() > 0, "full region mesh missing")
				mesh.free()
				var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
				vehicle.set("read_input", false)
				vehicle.set("course", "map:" + str(saved.path))
				root.add_child(vehicle)
				await process_frame
				_check(vehicle.call("is_ready"), "full region vehicle not ready")
				if vehicle.call("is_ready"):
					vehicle.call("send_command", "engine_toggle")
					vehicle.call("send_command", "gear_drive")
					vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
					for tick in 128: await physics_frame
					_check(float(vehicle.call("speed_mps")) > 0.2, "full region vehicle did not move")
				vehicle.free()
				var text := FileAccess.get_file_as_string(saved.path)
				_check(text.contains("road-network-v4") and text.contains("block-guide-v2"), "new replay versions missing")
				var old_path: String = str(saved.path) + ".old.toml"
				var old := FileAccess.open(old_path, FileAccess.WRITE)
				old.store_string(text.replace("road-network-v4", "road-network-v3"))
				old.close()
				_check(not ClassDB.class_call_static("SimCourse", "build", "map:" + old_path).ok, "old geometry silently replayed with new rules")
				DirAccess.remove_absolute(old_path)
				DirAccess.remove_absolute(saved.path)
				print("JUNCTION REGION: %s · %d blocks · %.1f m · selected/saved/replayed/meshed/driven" % [sample[0], blocks.size(), route.length_m])
		screen.free()
	if failures.is_empty():
		print("JUNCTION REGION OK")
		quit(0)
	else:
		for message in failures: printerr("JUNCTION REGION FAIL: " + message)
		quit(1)

## 원본 way 노드 순서를 따르는 블록·방향을 결정한다. 경로 실패 뒤 후보를 바꾸지 않는다.
func _source_blocks(blocks: Array, source: Dictionary, way: int, reversed: bool) -> Array:
	var nodes: Array[int] = []
	for item in source.elements:
		if item.type == "way" and int(item.id) == way:
			for node in item.nodes: nodes.append(int(node))
	var selected: Array = []
	for block in blocks:
		if int(block.way_id) != way: continue
		var a := nodes.find(int(block.from_node))
		var b := nodes.find(int(block.to_node))
		if a < 0 or b < 0: continue
		var entry: Dictionary = block.duplicate()
		entry.order = mini(a, b)
		if (a > b) != reversed: entry.direction = "backward" if block.direction == "forward" else "forward"
		selected.append(entry)
	selected.sort_custom(func(a, b): return a.order < b.order)
	if reversed: selected.reverse()
	return selected

## 기존 블록 API에서 고정한 원본 후보와 방향을 찾아 저장 호환성을 검증한다.
func _choose(screen: Node, item: Dictionary, role: String) -> void:
	var line: PackedVector2Array = item.line
	for c in screen.picker.call("block_candidates", line[0].lerp(line[1], 0.5), 20.0):
		if c.id == item.id and c.direction == item.direction:
			var result: Dictionary = screen.picker.call("choose_block", item.id, item.direction, role)
			_check(result.ok, str(result.error))
			return
	_check(false, "exact source candidate missing")

## 기대 동작 실패를 모아 검사 종료 시 보고한다.
func _check(ok: bool, message: String) -> void:
	if not ok: failures.append(message)
