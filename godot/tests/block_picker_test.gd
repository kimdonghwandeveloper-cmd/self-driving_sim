extends SceneTree
var failures: Array[String] = []

## 기존 블록 경로 파일과 새 자유 주행 파일의 API 회귀 검사를 예약한다.
func _init() -> void:
 call_deferred("_run")

## 이전 저장 경로 API는 유지하고 새 영역 API만 방향·역할 없이 동작하는지 검사한다.
func _run() -> void:
 var picker: Object = ClassDB.instantiate("SimRoutePicker")
 picker.set("network", false)
 _check(picker.call("load_block_snapshot", "testdata/block-picker/graph.osm.json").ok, "load snapshot")
 var blocks: Array = picker.call("block_items")
 var start: Dictionary = blocks.filter(func(b): return b.name == "start")[0]
 var goal: Dictionary = blocks.filter(func(b): return b.name == "goal")[0]
 _check(picker.call("choose_block", start.id, start.directions[0], "start").ok, "legacy start")
 _check(picker.call("choose_block", goal.id, goal.directions[0], "goal").ok, "legacy goal")
 _check(picker.call("preview_blocks").ok, "legacy route preview")
 _check(not picker.call("choose_block", start.id, "invalid", "start").ok, "invalid direction")
 var area: Dictionary = picker.call("preview_road_area", start.id, 500.0)
 _check(area.ok and not area.lines.is_empty(), "area preview")
 _check(not picker.call("save_road_area", "../escape").ok, "invalid area name")
 picker.call("clear_road_area")
 _check(not picker.call("save_road_area", "not-selected").ok, "stale area saved")
 picker = null
 var main: Node = load("res://main.tscn").instantiate()
 root.add_child(main)
 await process_frame
 main._show_map()
 _check(main.screen.has_method("_set_pin"), "menu must open the pin route UI")
 main.free()
 await process_frame
 if failures.is_empty():
  print("BLOCK PICKER OK")
  quit(0)
 else:
  for message in failures: printerr(message)
  quit(1)

## 실패 사유를 누적한다.
func _check(ok: bool, message: String) -> void:
 if not ok: failures.append(message)
