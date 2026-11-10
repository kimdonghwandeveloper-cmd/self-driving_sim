# 5-1 시나리오 틀 확인: 메뉴에서 예시 시나리오(assets/scenarios/sample-stop-line.toml)를 열면 시작 전 안내에서 차량이 멈춰 있고,
# F5로 시작해 단계(시동 → D → 정지선 앞 정지 → P)를 끝내 성공하며, 일시정지 동안 차량·시간이 멈추고, 재시작·종료가 동작하는지 본다.
# 없는 시나리오 파일은 사유와 함께 실패 화면이 된다. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/scenario_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
const SCENARIO := "scenario:assets/scenarios/sample-stop-line.toml"
## 정지선 앞 정지 구역(차체 중심 z = -17.9 ± 1.5)에 들도록 제동을 시작할 후륜축 z(m)와 목표 속도(m/s).
const BRAKE_AT_Z := -15.5
const CRUISE_MPS := 1.8
## 일시정지를 시험할 후륜축 z(m). 8 m 이동 메시지가 이미 나온 뒤다.
const PAUSE_AT_Z := -10.0
## 각 동작에서 기다릴 최대 물리 틱 수.
const MAX_TICKS := 64 * 60

var failures := PackedStringArray()
var main: Node
## 받은 시나리오 시그널(순서대로 "step:<id>", "message:<글>", "phase:<이름>", "finished:<결과>").
var events := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle") or not ClassDB.class_exists("SimScenario"):
		printerr("SCENARIO FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 예시 시나리오 흐름과 없는 파일 실패를 차례로 확인하고 결과를 낸다.
func _run() -> void:
	main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	await _check_scenario_flow()
	main.free()
	await _check_missing_file()
	if failures.is_empty():
		print("SCENARIO OK: 메뉴 목록 · 시작 전 정지 · F5 시작 · 단계 4개 · 메시지 2개 · 일시정지 · 성공 · 재시작 · 종료 · 없는 파일 실패")
		quit(0)
	else:
		for message in failures:
			printerr("SCENARIO FAIL: " + message)
		quit(1)


## 메뉴에서 예시 시나리오를 열어 시작 전 → 진행 → 일시정지 → 계속 → 성공 → 재시작 → 종료를 확인한다.
func _check_scenario_flow() -> void:
	var menu: Node = main.screen
	var index: int = Array(menu.course_names).find(SCENARIO)
	_check(index >= 0, "메뉴에 예시 시나리오가 없음: " + str(menu.course_names))
	if index < 0:
		return
	menu.courses.select(index)
	menu.call("_open_course")
	var drive: Node = main.screen
	_check(drive.failure.is_empty(), "시나리오 화면 실패: " + drive.failure)
	if not drive.failure.is_empty():
		return
	var vehicle: Node3D = drive.vehicle
	for signal_name in ["scenario_step_done", "scenario_message", "scenario_phase_changed", "scenario_finished"]:
		vehicle.connect(signal_name, _record.bind(signal_name))
	vehicle.set("read_input", false)
	_check(drive.scenario_box.visible, "시나리오 안내 상자가 보이지 않음")

	for i in 16:
		await physics_frame
	await process_frame
	_check(_phase(vehicle) == "briefing", "시작 전 안내 단계가 아님: " + _phase(vehicle))
	_check(int(vehicle.call("tick_count")) == 0, "시작 전에 틱이 돎")
	_check(drive.scenario_label.text.contains("단계 1/4") and drive.scenario_label.text.contains("F5 시작"), "시작 전 안내 글자: " + drive.scenario_label.text)

	_press(KEY_F5)
	await process_frame
	_check(_phase(vehicle) == "running", "F5로 시작되지 않음: " + _phase(vehicle))
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	await physics_frame
	await physics_frame
	var status: Dictionary = vehicle.call("scenario_status")
	_check(int(status.step_index) == 2, "시동·D 단계가 끝나지 않음: " + str(status))

	# 정지선 쪽으로 천천히 간다(목표 속도 위면 가속을 뗀다).
	var paused_checked := false
	for i in MAX_TICKS:
		var speed := float(vehicle.call("speed_mps"))
		vehicle.call("set_manual_input", 0.3 if speed < CRUISE_MPS else 0.0, 0.0, 0.0)
		await physics_frame
		if not paused_checked and vehicle.position.z <= PAUSE_AT_Z:
			paused_checked = true
			await _check_pause(drive, vehicle)
		if vehicle.position.z <= BRAKE_AT_Z:
			break
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for i in MAX_TICKS:
		await physics_frame
		if int((vehicle.call("scenario_status") as Dictionary).step_index) >= 3:
			break
	status = vehicle.call("scenario_status")
	_check(int(status.step_index) == 3, "정지선 앞 정지 단계가 끝나지 않음(후륜축 z %.2f): %s" % [vehicle.position.z, str(status)])
	vehicle.call("send_command", "gear_park")
	await physics_frame
	await process_frame
	_check(_phase(vehicle) == "finished" and str((vehicle.call("scenario_status") as Dictionary).outcome) == "success", "성공으로 끝나지 않음: " + str(vehicle.call("scenario_status")))
	_check(drive.scenario_label.text.contains("성공") and drive.scenario_label.text.contains("단계 4/4 완료"), "끝 안내 글자: " + drive.scenario_label.text)
	var expected := PackedStringArray([
		"phase:running", "step:engine", "step:drive", "message:정지선이 가까워집니다. 미리 속도를 줄이세요.",
		"phase:paused", "phase:running", "step:stop-line", "message:정지선 앞에 멈췄습니다.",
		"step:park", "phase:finished", "finished:success"])
	_check(events == expected, "시그널 순서가 다름: " + str(events))
	var finished_tick := int(vehicle.call("tick_count"))
	for i in 8:
		await physics_frame
	_check(int(vehicle.call("tick_count")) == finished_tick, "끝난 뒤에도 틱이 돎")

	_press(KEY_F7)
	await process_frame
	status = vehicle.call("scenario_status")
	_check(_phase(vehicle) == "briefing" and int(status.step_index) == 0 and float(status.elapsed_s) == 0.0, "재시작 뒤 처음 상태가 아님: " + str(status))
	_check(int(vehicle.call("tick_count")) == 0 and vehicle.position.length() < 1e-6, "재시작 뒤 차량이 시작 자세가 아님: " + str(vehicle.position))
	_check(str(vehicle.call("gear")) == "P" and not bool(vehicle.call("engine_on")), "재시작 뒤 시작 조건(시동 OFF·P)이 아님")
	_press(KEY_F8)
	await process_frame
	_check(str((vehicle.call("scenario_status") as Dictionary).outcome) == "aborted", "F8로 중단되지 않음: " + str(vehicle.call("scenario_status")))
	_check(not bool(vehicle.call("scenario_control", "pause")), "끝난 뒤 일시정지를 받음")


## 진행 중 F6으로 일시정지해 차량·경과 시간이 멈추는지 보고 F5로 계속한다.
func _check_pause(drive: Node, vehicle: Node3D) -> void:
	_press(KEY_F6)
	await process_frame
	_check(_phase(vehicle) == "paused", "F6으로 일시정지되지 않음: " + _phase(vehicle))
	var position := vehicle.position
	var tick := int(vehicle.call("tick_count"))
	var elapsed := float((vehicle.call("scenario_status") as Dictionary).elapsed_s)
	vehicle.call("set_manual_input", 1.0, 0.0, 0.0)
	for i in 32:
		await physics_frame
	await process_frame
	_check(vehicle.position == position and int(vehicle.call("tick_count")) == tick, "일시정지 중 차량이 움직임")
	_check(float((vehicle.call("scenario_status") as Dictionary).elapsed_s) == elapsed, "일시정지 중 시간이 흐름")
	_check(drive.scenario_label.text.contains("일시정지") and drive.scenario_label.text.contains("F5 계속"), "일시정지 안내 글자: " + drive.scenario_label.text)
	vehicle.call("set_manual_input", 0.0, 0.0, 0.0)
	_press(KEY_F5)
	await process_frame
	_check(_phase(vehicle) == "running", "F5로 계속되지 않음: " + _phase(vehicle))


## 없는 시나리오 파일은 사유와 함께 실패 화면이 되는지 본다.
func _check_missing_file() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("scenario:assets/scenarios/없는-시나리오.toml")
	await process_frame
	_check(drive.failure.contains("시나리오 오류"), "없는 파일이 실패로 처리되지 않음: " + drive.failure)
	_check(not drive.scenario_box.visible, "실패 화면에 시나리오 상자가 보임")
	drive.free()


## 시나리오 실행 단계 이름을 읽는다.
func _phase(vehicle: Node3D) -> String:
	return str((vehicle.call("scenario_status") as Dictionary).get("phase", ""))


## 기능 키 하나를 눌렀다 뗀 입력 이벤트를 보낸다.
func _press(key: Key) -> void:
	for pressed in [true, false]:
		var event := InputEventKey.new()
		event.physical_keycode = key
		event.keycode = key
		event.pressed = pressed
		root.push_input(event)


## 시나리오 시그널을 "종류:값" 문자열로 기록한다(bind한 시그널 이름이 마지막 인자로 온다).
func _record(a: Variant, b: Variant = null, c: Variant = null) -> void:
	var args := [a, b, c].filter(func(value): return value != null)
	var signal_name := str(args.pop_back())
	match signal_name:
		"scenario_step_done":
			events.append("step:" + str(args[1]))
		"scenario_message":
			events.append("message:" + str(args[0]))
		"scenario_phase_changed":
			events.append("phase:" + str(args[0]))
		"scenario_finished":
			events.append("finished:" + str(args[0]))


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
