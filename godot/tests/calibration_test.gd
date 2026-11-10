# 2-5 보정 측정(Bevy 앱에서 옮김) 확인: 보정 코스 주행 화면에 측정 상자가 보이고, 가속 → 최대 제동 정지 뒤
# 제동·가속 시험 1회차 결과가 한국어 줄로 나오며, 리셋 뒤에도 남는지 본다. 개발 코스(training)에서는 상자가 없다. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/calibration_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
## 가속 틱 수(약 2초)와 제동 후 정지를 기다릴 최대 틱 수.
const ACCEL_TICKS := 128
const STOP_TICKS := 640

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("CALIBRATION FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 보정 코스 측정과 training의 상자 숨김을 차례로 확인하고 결과를 낸다.
func _run() -> void:
	await _check_calibration_course()
	await _check_training_hides_box()
	if failures.is_empty():
		print("CALIBRATION OK: 보정 코스 측정 상자 · 가속/제동 시험 1회차 · 리셋 후 결과 유지 · training 상자 없음")
		quit(0)
	else:
		for message in failures:
			printerr("CALIBRATION FAIL: " + message)
		quit(1)


## 보정 코스에서 가속했다가 최대 제동으로 멈춘 뒤 측정 줄과 리셋 후 유지를 본다.
func _check_calibration_course() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("calibration")
	_check(drive.failure.is_empty(), "보정 코스가 열리지 않음: " + drive.failure)
	_check(drive.calibration_box.visible, "보정 코스에서 측정 상자가 보이지 않음")
	var vehicle: Node3D = drive.vehicle
	vehicle.set("read_input", false)
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 1.0, 0.0, 0.0)
	for i in ACCEL_TICKS:
		await physics_frame
	var lines := "\n".join(vehicle.call("calibration_lines"))
	_check(lines.contains("진행 중 시험: 가속"), "가속 중 진행 시험 표시 없음: " + lines)
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for i in STOP_TICKS:
		await physics_frame
		if absf(vehicle.call("speed_mps")) < 1e-4:
			break
	_check(absf(vehicle.call("speed_mps")) < 1e-4, "제동 후 정지하지 않음")
	await process_frame
	lines = "\n".join(vehicle.call("calibration_lines"))
	_check(lines.contains("마지막 제동 시험: 1회차") and lines.contains("(제동)"), "제동 시험 결과 없음: " + lines)
	_check(lines.contains("마지막 가속 시험: 1회차") and lines.contains("(가속 뗌)"), "가속 시험 결과 없음: " + lines)
	_check(drive.calibration_label.text.contains("마지막 제동 시험: 1회차"), "측정 상자 글자가 갱신되지 않음: " + drive.calibration_label.text)
	vehicle.call("send_command", "reset_course")
	await physics_frame
	lines = "\n".join(vehicle.call("calibration_lines"))
	_check(lines.contains("마지막 제동 시험: 1회차") and lines.contains("진행 중 시험: -"), "리셋 후 결과가 남지 않음: " + lines)
	drive.free()


## 개발 코스 training 주행 화면에는 측정 상자가 없는지 본다.
func _check_training_hides_box() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("training")
	await process_frame
	_check(drive.failure.is_empty(), "training 코스가 열리지 않음: " + drive.failure)
	_check(not drive.calibration_box.visible, "training 코스에서 측정 상자가 보임")
	drive.free()


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
