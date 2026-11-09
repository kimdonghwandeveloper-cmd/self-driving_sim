# 4-6 확인: 지도 코스 경로 끝 목표 표시, 주행 후 리셋 시 경로 시작 자세 복원, 지도 코스 실패 시 개발 코스 대체. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/goal_reset_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
## 저장소의 여수 교차로 경로 코스와, 없는 지도 파일(대체 확인용).
const ROUTE := "map:assets/maps/yeosu-expo/route-junctions.toml"
const MISSING := "map:assets/maps/없는-코스.toml"
## 리셋 전에 시작점에서 벗어날 거리(m)와 그때까지 가속할 최대 틱 수, 제동 후 정지를 기다릴 최대 틱 수.
const LEAVE_M := 3.0
const DRIVE_TICKS := 640
const STOP_TICKS := 640
## 시작 자세 비교 허용 오차(m, rad).
const POSE_TOLERANCE := 1e-3

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("GOAL RESET FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 목표 표시·리셋·대체를 차례로 확인하고 결과를 낸다.
func _run() -> void:
	await _check_goal_and_reset()
	await _check_fallback()
	_check_training_has_no_goal()
	if failures.is_empty():
		print("GOAL RESET OK: 목표 표시 · 주행 후 경로 시작 자세 리셋 · 지도 실패 시 training 대체")
		quit(0)
	else:
		for message in failures:
			printerr("GOAL RESET FAIL: " + message)
		quit(1)


## 지도 코스 주행 화면에 목표 표시가 계약 `goal` 자세에 있고, 달린 뒤 정지해 리셋하면 `start` 자세로 돌아오는지 본다.
func _check_goal_and_reset() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start(ROUTE)
	_check(drive.failure.is_empty() and drive.fallback_notice.is_empty(), "지도 코스가 열리지 않음: " + drive.failure + drive.fallback_notice)
	if not drive.failure.is_empty() or not drive.fallback_notice.is_empty():
		drive.free()
		return
	var vehicle: Node3D = drive.vehicle
	var data: Dictionary = vehicle.call("course_data")
	var goal: Dictionary = data.get("goal", {})
	var marker: Node3D = drive.scene_content.find_child("GoalMarker", true, false)
	_check(not goal.is_empty(), "지도 코스 사전에 goal 없음")
	_check(marker != null, "목표 표시 노드 없음")
	if marker != null and not goal.is_empty():
		_check(marker.position.distance_to(goal.position) < POSE_TOLERANCE, "목표 표시 위치가 goal과 다름")
		_check(absf(marker.rotation.y - float(goal.heading_rad)) < POSE_TOLERANCE, "목표 표시 방향이 goal과 다름")
		_check(Vector3(goal.position).distance_to(data.start.position) > 20.0, "목표가 시작점과 너무 가까움")
	vehicle.set("read_input", false)
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
	for i in DRIVE_TICKS:
		await physics_frame
		if vehicle.position.distance_to(data.start.position) > LEAVE_M:
			break
	var moved: float = vehicle.position.distance_to(data.start.position)
	_check(moved > LEAVE_M, "차량이 시작점을 벗어나지 않음(%.2f m)" % moved)
	_check(not bool(vehicle.call("in_contact")), "주행 중 연석·벽에 닿음")
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for i in STOP_TICKS:
		await physics_frame
		if absf(vehicle.call("speed_mps")) < 1e-4:
			break
	_check(absf(vehicle.call("speed_mps")) < 1e-4, "제동 후 정지하지 않음")
	vehicle.call("send_command", "reset_course")
	await physics_frame
	await physics_frame
	var start: Dictionary = data.start
	_check(vehicle.position.distance_to(start.position) < POSE_TOLERANCE, "리셋 후 위치가 경로 시작이 아님: %s" % vehicle.position)
	_check(absf(vehicle.rotation.y - float(start.heading_rad)) < POSE_TOLERANCE, "리셋 후 방향이 경로 시작이 아님")
	drive.free()


## 없는 지도 파일을 열면 실패 화면 대신 training으로 대체하고 사유를 경고에 띄우는지 본다.
func _check_fallback() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start(MISSING)
	await process_frame
	_check(drive.failure.is_empty(), "대체 코스도 실패: " + drive.failure)
	_check(not drive.fallback_notice.is_empty(), "대체 안내 없음")
	if drive.failure.is_empty():
		var data: Dictionary = drive.vehicle.call("course_data")
		_check(data.get("name", "") == "training", "대체 코스가 training이 아님: " + str(data.get("name", "")))
		_check(drive.warnings_label.text.contains("개발 코스(training)로 대체"), "경고에 대체 안내 없음: " + drive.warnings_label.text)
		_check(drive.warnings_label.text.contains("지도 코스를 열 수 없습니다"), "경고에 실패 사유 없음: " + drive.warnings_label.text)
		_check(drive.scene_content.find_child("GoalMarker", true, false) == null, "training에 목표 표시가 생김")
	var vehicles := drive.get_children().filter(func(child): return child.is_class("SimVehicle"))
	_check(vehicles.size() == 1, "실패한 지도 차량 노드가 남음(%d개)" % vehicles.size())
	drive.free()


## 개발 코스 형상 사전의 goal이 빈 사전인지 본다.
func _check_training_has_no_goal() -> void:
	var data: Dictionary = ClassDB.class_call_static("SimCourse", "build", "training")
	_check(data.has("goal") and Dictionary(data.goal).is_empty(), "training goal이 빈 사전이 아님")


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
