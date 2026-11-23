extends SceneTree
const Screen = preload("res://presentation/block_map_screen.gd")
var failures: Array[String] = []

## 실제 엑스포·강남 도로 영역 선택과 주행 검사를 예약한다.
func _init() -> void:
 call_deferred("_run")

## 도착 지정 없이 영역을 생성해 저장·공통 로더·차량 이동까지 검증한다.
func _run() -> void:
 for sample in [["expo-internal",532526365], ["expo-curved-joint",1286142779], ["gangnam-working-area",129731721]]:
  var screen := Screen.new()
  root.add_child(screen)
  await process_frame
  screen._load_snapshot("testdata/block-real/%s.osm.json" % sample[0])
  var items: Array = screen.picker.call("block_items")
  var matches: Array = items.filter(func(b): return int(b.way_id) == sample[1])
  _check(not matches.is_empty(), str(sample[0])+": road missing")
  if not matches.is_empty():
   screen._select_area(str(matches[0].id))
   _check(screen.preview_ok, str(sample[0])+": "+screen.technical_feedback)
   if screen.preview_ok:
    var saved: Dictionary = screen.picker.call("save_road_area", "real-area-%d" % Time.get_ticks_usec())
    _check(saved.ok, str(saved.error))
    if saved.ok:
     var course: Dictionary = ClassDB.class_call_static("SimCourse","build","map:"+str(saved.path))
     _check(course.ok and course.goal.is_empty(), "goal-less area replay")
     var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
     vehicle.set("read_input",false)
     vehicle.set("course","map:"+str(saved.path))
     root.add_child(vehicle)
     await process_frame
     _check(vehicle.call("is_ready"), "vehicle initialization")
     vehicle.call("send_command","engine_toggle")
     vehicle.call("send_command","gear_drive")
     vehicle.call("set_manual_input",0.4,0.0,0.0)
     for tick in 128: await physics_frame
     _check(float(vehicle.call("speed_mps")) > 0.2, "vehicle did not move")
     await process_frame
     vehicle.free()
     DirAccess.remove_absolute(saved.path)
     print("REAL ROAD AREA: "+str(sample[0])+" selected/saved/replayed/driven")
  screen.free()
  await process_frame
 if failures.is_empty():
  print("REAL BLOCK UI OK")
  quit(0)
 else:
  for message in failures: printerr(message)
  quit(1)

## 실패 사유를 누적한다.
func _check(ok: bool, message: String) -> void:
 if not ok: failures.append(message)
