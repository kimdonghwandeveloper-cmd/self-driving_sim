extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 영역 선택 화면의 작은 창 배치 검사를 예약한다.
func _init() -> void:
 call_deferred("_run")

## 해상도를 바꿔도 패널·주행 버튼이 창 안에 있고 개발 진단이 노출되지 않는지 검사한다.
func _run() -> void:
 root.content_scale_size = Vector2i.ZERO
 var screen := Screen.new()
 root.add_child(screen)
 await process_frame
 for size in [Vector2i(1280,720), Vector2i(960,540), Vector2i(800,600)]:
  root.size = size
  for frame in 4: await process_frame
  var panel: Rect2 = screen.toolbar_panel.get_global_rect()
  _check(panel.size.x <= size.x * 0.45, "sidebar hides map")
  _check(Rect2(Vector2.ZERO, Vector2(size)).encloses(panel), "sidebar outside viewport")
  _check(panel.encloses(screen.drive_button.get_global_rect()), "drive action outside sidebar")
 _check(not screen.advanced.is_visible_in_tree(), "advanced controls visible")
 _check(not screen.status.text.contains("BlockRouteFailure"), "internal error shown")
 _check(screen.drive_button.disabled, "drive enabled before selection")
 screen._load_snapshot("testdata/block-real/expo-internal.osm.json")
 screen._select_area("missing-road")
 _check(screen.drive_button.disabled and screen.area_lines.is_empty(), "failed area retains old preview")
 screen.free()
 await process_frame
 if failures.is_empty():
  print("BLOCK MAP UI OK")
  quit(0)
 else:
  for message in failures: printerr(message)
  quit(1)

## 실패 사유를 누적한다.
func _check(ok: bool, message: String) -> void:
 if not ok: failures.append(message)
