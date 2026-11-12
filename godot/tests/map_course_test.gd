# GD-3 지도 코스 확인(계약 v1.2 `map:<파일>`). 저장소의 여수 경로 코스를 목록에서 찾아 형상을 만들고,
# Rust 차량으로 몇 초 주행하며, 주행 화면이 메시·바닥·출처 표기를 만드는지 본다. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/map_course_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
const CourseMesh = preload("res://presentation/course_mesh.gd")
## 저장소에 들어 있는 여수 교차로 경로 코스(Bevy `--course map:...`과 같은 파일).
const ROUTE := "map:testdata/yeosu-expo/route-junctions.toml"
## 차량을 몰아 볼 틱 수(64 Hz로 4초).
const DRIVE_TICKS := 256

var failures := PackedStringArray()
var vehicle: Node3D
var ticks := 0


## 확장 확인 뒤 목록·형상·실패 사유를 검사하고 차량 주행을 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimCourse") or not ClassDB.class_exists("SimVehicle"):
		printerr("MAP COURSE FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	var listed := false
	for file in ClassDB.class_call_static("SimCourse", "course_files"):
		listed = listed or file.course == ROUTE
	_check(listed, "course_files에 " + ROUTE + " 없음")
	var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", ROUTE)
	_check(data.get("ok", false), "build 실패: " + str(data.get("error", "")))
	if data.get("ok", false):
		_check(not data.areas.is_empty(), "교차로 면 없음")
		_check(not data.strips.is_empty(), "도로 띠 없음")
		_check(not data.colliders.is_empty(), "연석 없음")
		_check("\n".join(data.notices).contains("OpenStreetMap"), "OSM 출처 표기 없음")
	var missing: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:assets/maps/없는-코스.toml")
	_check(not missing.ok and str(missing.error).begins_with("지도 코스를 열 수 없습니다"), "없는 파일 사유: " + str(missing))
	var empty: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:")
	_check(not empty.ok, "빈 경로가 거부되지 않음")
	vehicle = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", ROUTE)
	vehicle.connect("tick_finished", _on_tick)
	vehicle.ready.connect(_on_ready)
	root.add_child(vehicle)


## 차량이 준비되면 시동·D·가속을 넣는다.
func _on_ready() -> void:
	_check(vehicle.call("is_ready"), "차량 준비 실패: " + str(vehicle.call("last_error")))
	if not vehicle.call("is_ready"):
		_finish()
		return
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 0.4, 0.0, 0.0)


## 정해진 틱만큼 달린 뒤 움직였는지 보고 화면 검사로 넘어간다.
func _on_tick(_tick: int) -> void:
	ticks += 1
	if ticks == DRIVE_TICKS:
		_check(float(vehicle.call("speed_mps")) > 0.5, "지도 코스에서 차량이 움직이지 않음")
		vehicle.queue_free()
		call_deferred("_check_screen")


## 주행 화면을 같은 코스로 열어 실패 없이 메시·바닥·출처 안내가 생기는지 확인한다.
func _check_screen() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start(ROUTE)
	await process_frame
	_check(drive.failure.is_empty(), "주행 화면 실패: " + drive.failure)
	_check(drive.fallback_notice.is_empty(), "지도 코스 대신 개발 코스로 대체됨: " + drive.fallback_notice)
	if drive.failure.is_empty():
		var bounds: Rect2 = CourseMesh.ground_bounds(drive.vehicle.call("course_data"))
		var size: Vector2 = (drive.ground.mesh as PlaneMesh).size
		var center := Vector2(drive.ground.position.x, drive.ground.position.z)
		_check(Rect2(center - size * 0.5, size).encloses(bounds), "바닥이 코스 범위를 덮지 않음")
		_check(drive.notice.text.contains("OpenStreetMap"), "화면에 OSM 출처 표기 없음")
		_check_merged_colliders(drive)
	drive.free()
	_finish()


## 충돌 경계가 종류마다 메시 하나로 합쳐졌고(4-6), 합친 연석 메시 범위가 모든 연석 중심을 덮는지 본다.
func _check_merged_colliders(drive: Node) -> void:
	var colliders: Array = drive.vehicle.call("course_data").colliders
	var curbs := colliders.filter(func(collider): return collider.kind == "curb")
	var merged: MeshInstance3D = drive.scene_content.find_child("Colliders_curb", true, false)
	_check(curbs.size() > 100 and merged != null, "연석 합친 메시 없음(연석 %d개)" % curbs.size())
	if merged == null:
		return
	var nodes: Array[Node] = drive.scene_content.find_children("*", "MeshInstance3D", true, false)
	_check(nodes.size() < colliders.size(), "메시 노드가 충돌 경계 수보다 많음(%d / %d)" % [nodes.size(), colliders.size()])
	var box := merged.get_aabb().grow(0.01)
	for curb in curbs:
		var center: Vector3 = curb.rect.center
		if not box.has_point(center + Vector3(0, 0.01, 0)):
			_check(false, "합친 연석 메시가 연석 %s를 덮지 않음" % center)
			return

## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)


## 결과를 출력하고 종료 코드를 정한다.
func _finish() -> void:
	if failures.is_empty():
		print("MAP COURSE OK: %s 목록·형상·주행·화면" % ROUTE)
		quit(0)
	else:
		for message in failures:
			printerr("MAP COURSE FAIL: " + message)
		quit(1)
