# 2-8 차체 서스펜션 움직임 확인: Rust가 계산한 차체 피치·롤(`body_pose`)이 화면에 전달되는지 본다.
# 제동하면 앞이 숙여지고, 오른쪽으로 돌면 바깥(왼쪽)으로 기울며, 차체만 기울고 바퀴 부품은 그대로이고,
# 입석 카메라는 `body_follow`만큼만 따라가며 외부 시점에서는 따라가지 않는지 본다. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/suspension_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
## 가속 틱 수(약 3초)와 제동·회전 틱 수.
const ACCEL_TICKS := 192
const BRAKE_TICKS := 16
const TURN_TICKS := 128

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("SUSPENSION FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 보정 코스(넓은 빈 바닥)에서 제동·회전 때 차체·카메라 기울기를 확인하고 결과를 낸다.
func _run() -> void:
	await _check_body_motion()
	if failures.is_empty():
		print("SUSPENSION OK: 제동 앞 숙임 · 회전 바깥 기울기 · 차체만 기욺(바퀴 그대로) · 카메라 따라가기 비율 · 외부 시점 고정")
		quit(0)
	else:
		for message in failures:
			printerr("SUSPENSION FAIL: " + message)
		quit(1)


## 가속 뒤 제동해 앞 숙임을, 그다음 오른쪽으로 돌며 왼쪽 기울기를 보고 화면 노드에 같은 값이 들어갔는지 본다.
func _check_body_motion() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("calibration")
	_check(drive.failure.is_empty(), "보정 코스가 열리지 않음: " + drive.failure)
	var vehicle: Node3D = drive.vehicle
	vehicle.set("read_input", false)
	await process_frame
	var level: Dictionary = vehicle.call("body_pose")
	_check(is_zero_approx(float(level.pitch_rad)) and is_zero_approx(float(level.roll_rad)), "출발 전 차체가 기울어 있음: %s" % level)
	_check(drive.visual.wheels.get_child_count() > 0 or not ResourceLoader.exists(drive.visual.SHUTTLE_PATH), "셔틀 바퀴 부품을 나누지 못함")
	_check(is_equal_approx(drive.body_follow, 0.5), "카메라 따라가기 비율이 설정값(0.5)이 아님: %f" % drive.body_follow)

	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 1.0, 0.0, 0.0)
	for i in ACCEL_TICKS:
		await physics_frame
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for i in BRAKE_TICKS:
		await physics_frame
	await process_frame
	var braking: Dictionary = vehicle.call("body_pose")
	_check(float(braking.pitch_rad) > 0.003, "제동 중 앞이 숙여지지 않음: %s" % braking)
	var nose: Vector3 = drive.visual.body.transform.basis * Vector3.FORWARD
	_check(nose.y < -0.003, "차체 메시 앞이 내려가지 않음: %s" % nose)
	_check(drive.visual.wheels.transform == Transform3D.IDENTITY, "바퀴 부품이 차체와 함께 움직임")
	var eye: Vector3 = drive.camera_mount.transform.basis * Vector3.FORWARD
	_check(absf(eye.y + sin(asin(-nose.y) * 0.5)) < 1e-4, "카메라가 기울기의 절반만큼 따라가지 않음: %f / %f" % [eye.y, nose.y])

	vehicle.call("set_manual_input", 0.4, 0.0, 1.0)
	for i in TURN_TICKS:
		await physics_frame
	await process_frame
	var turning: Dictionary = vehicle.call("body_pose")
	_check(float(turning.roll_rad) < -0.003, "오른쪽 회전 중 왼쪽(바깥)으로 기울지 않음: %s" % turning)
	var right: Vector3 = drive.visual.body.transform.basis * Vector3.RIGHT
	_check(right.y > 0.003, "차체 메시 오른쪽이 들리지 않음: %s" % right)

	drive.rig.set_external(true)
	await process_frame
	_check(drive.camera_mount.transform == Transform3D.IDENTITY, "외부 시점 카메라가 차체를 따라 기욺")
	drive.free()


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
