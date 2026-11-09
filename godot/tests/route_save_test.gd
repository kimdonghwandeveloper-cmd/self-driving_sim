# GD-4 경로 선택 → 저장 → 저장 경로 주행 확인(오프라인). 네트워크를 쓰지 않는다. 성공 0 / 실패 1.
# 저장소의 여수 스냅샷을 열어 도로 위에 점 두 개를 찍고, 경로를 cache/routes/<시험 이름>.toml로 저장한 뒤
# 메뉴 코스 목록(SimCourse.course_files)에 나타나고 그 코스로 SimVehicle이 준비되는지 본다. 시험 파일은 끝에 지운다.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/route_save_test.gd
extends SceneTree

## 저장소에 들어 있는 여수 노드 ID 스냅샷.
const SNAPSHOT := "assets/maps/yeosu-expo/graph-2026-11-06.osm.json"

var failures := PackedStringArray()
## 이번 실행에서만 쓰는 저장 이름(같은 이름 파일이 남아 있어도 겹치지 않게 프로세스 번호를 붙인다).
var route_name := "gd4-route-save-test-%d" % OS.get_process_id()
var saved_path := ""
var vehicle: Node3D


## 스냅샷을 열고 점을 찍어 저장한 뒤, 목록과 코스 생성을 확인하고 차량 준비를 기다린다.
func _init() -> void:
	if not ClassDB.class_exists("SimRoutePicker") or not ClassDB.class_exists("SimVehicle"):
		printerr("ROUTE SAVE FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	var picker: Object = ClassDB.instantiate("SimRoutePicker")
	picker.set("network", false)
	var opened: Dictionary = picker.call("load_snapshot", SNAPSHOT)
	_check(opened.ok, "스냅샷 열기 실패: " + str(opened.error))
	if not opened.ok:
		_finish()
		return
	var ends := _longest_usable_segment(picker)
	# 교차로 노드 위는 시작점으로 거부되므로 선분 안쪽(1/4, 3/4)에 찍는다.
	picker.call("add_point", ends[0].lerp(ends[1], 0.25))
	picker.call("add_point", ends[0].lerp(ends[1], 0.75))
	var route: Dictionary = picker.call("route")
	_check(route.ok and route.length_m > 0.0, "경로 계산 실패: " + str(route.error))
	var status := "\n".join(picker.call("status_lines"))
	_check(status.contains("찍은 점: 2개") and status.contains("경로:"), "경로 선택 안내가 한국어가 아님: " + status)
	var saved: Dictionary = picker.call("save", route_name)
	_check(saved.ok, "저장 실패: " + str(saved.error))
	if not saved.ok:
		_finish()
		return
	saved_path = saved.path
	var again: Dictionary = picker.call("save", route_name)
	_check(not again.ok, "같은 이름 덮어쓰기가 거부되지 않음")
	var course := "map:cache/routes/%s.toml" % route_name
	var listed := false
	for file in ClassDB.class_call_static("SimCourse", "course_files"):
		if file.course == course:
			listed = file.saved
	_check(listed, "저장한 경로가 메뉴 코스 목록에 저장 경로로 없음: " + course)
	var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", course)
	_check(data.get("ok", false), "저장한 경로로 코스를 만들지 못함: " + str(data.get("error", "")))
	vehicle = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", course)
	vehicle.ready.connect(_on_ready)
	root.add_child(vehicle)


## 저장 경로 코스로 차량이 준비됐는지 보고 끝낸다.
func _on_ready() -> void:
	_check(vehicle.call("is_ready"), "저장 경로 코스로 차량 준비 실패: " + str(vehicle.call("last_error")))
	vehicle.queue_free()
	_finish()


## 주행 가능한 도로 선분 중 가장 긴 것의 양 끝(지도 좌표)을 돌려준다.
func _longest_usable_segment(picker: Object) -> Array[Vector2]:
	var segments: PackedVector2Array = picker.call("road_segments")
	var usable: PackedByteArray = picker.call("road_usable")
	var best: Array[Vector2] = [Vector2.ZERO, Vector2.ZERO]
	var best_length := 0.0
	for i in usable.size():
		var length := segments[i * 2].distance_to(segments[i * 2 + 1])
		if usable[i] == 1 and length > best_length:
			best_length = length
			best = [segments[i * 2], segments[i * 2 + 1]]
	return best


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)


## 시험 파일을 지우고 결과를 출력한다.
func _finish() -> void:
	if not saved_path.is_empty() and FileAccess.file_exists(saved_path):
		_check(DirAccess.remove_absolute(saved_path) == OK, "시험 파일을 지우지 못함: " + saved_path)
	if failures.is_empty():
		print("ROUTE SAVE OK: 스냅샷 → 점 2개 → 저장 → 코스 목록 → 저장 경로 차량 준비")
		quit(0)
	else:
		for message in failures:
			printerr("ROUTE SAVE FAIL: " + message)
		quit(1)
