# 5-2~5-5 시나리오 확인: 메뉴에 시나리오 A·B·C가 있고, B의 원뿔 두 개가 코스 충돌 경계(obstacle)로 더해져 그려지며,
# C에서 10 m를 가면 보행자 대체 객체가 오른쪽 보도(4, -26)에 나타나 걷고(화면 노드가 따라감), 제동하면 반응 시간이 기록되고,
# F8로 끝내면 결과 JSON이 저장되어 사건 기록·입력 열·임시 점수 안내가 들어 있고 끝 안내에 기록 요약이 나오는지 본다.
# 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/scenario_record_test.gd
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
const SCENARIOS := ["scenario-a-basic.toml", "scenario-b-space.toml", "scenario-c-hazard.toml"]
const SCENARIO_B := "scenario:assets/scenarios/scenario-b-space.toml"
const SCENARIO_C := "scenario:assets/scenarios/scenario-c-hazard.toml"
## 결과 파일을 저장소 `logs/` 대신 둘 시험 폴더.
const RESULT_DIR := "user://scenario-record-test"
## 각 동작에서 기다릴 최대 물리 틱 수.
const MAX_TICKS := 64 * 30
## 시나리오 C 주행 속도(m/s)와 등장 뒤 제동까지 기다릴 틱 수(0.5 s).
const CRUISE_MPS := 3.5
const REACTION_TICKS := 32

var failures := PackedStringArray()


## 확장을 확인하고 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("SCENARIO RECORD FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 메뉴 목록, 시나리오 B 장애물, 시나리오 C 보행자·기록·결과 파일을 차례로 확인한다.
func _run() -> void:
	_clear_results()
	await _check_menu()
	await _check_obstacles()
	await _check_hazard_and_result()
	_clear_results()
	if failures.is_empty():
		print("SCENARIO RECORD OK: 메뉴 A·B·C · B 원뿔 2개 · C 보행자 등장·이동 · 제동 반응 기록 · 결과 JSON · 끝 안내 요약")
		quit(0)
	else:
		for message in failures:
			printerr("SCENARIO RECORD FAIL: " + message)
		quit(1)


## 메뉴 코스 목록에 시나리오 A·B·C가 있는지 본다.
func _check_menu() -> void:
	var main: Node = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	var names := Array(main.screen.course_names)
	for file in SCENARIOS:
		_check(names.has("scenario:assets/scenarios/" + file), "메뉴에 %s가 없음: %s" % [file, str(names)])
	main.free()


## 시나리오 B를 열면 training 코스 충돌 경계에 원뿔 두 개(obstacle)가 더해지는지 본다.
func _check_obstacles() -> void:
	var plain := _open("training")
	var with_cones := _open(SCENARIO_B)
	await process_frame
	var count := func(drive: Node) -> int:
		var colliders: Array = (drive.vehicle.call("course_data") as Dictionary).get("colliders", [])
		return colliders.filter(func(collider): return collider.kind == "obstacle").size()
	_check(with_cones.failure.is_empty(), "시나리오 B 실패: " + with_cones.failure)
	_check(count.call(with_cones) == count.call(plain) + 2, "시나리오 B 원뿔 수: %d (training %d)" % [count.call(with_cones), count.call(plain)])
	plain.free()
	with_cones.free()


## 시나리오 C에서 보행자 등장·이동·제동 반응·결과 파일·끝 안내를 확인한다.
func _check_hazard_and_result() -> void:
	var drive := _open(SCENARIO_C)
	_check(drive.failure.is_empty(), "시나리오 C 실패: " + drive.failure)
	if not drive.failure.is_empty():
		return
	var vehicle: Node3D = drive.vehicle
	vehicle.set("read_input", false)
	vehicle.set("result_dir", RESULT_DIR)
	_check(vehicle.call("scenario_control", "start"), "시작되지 않음")
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	var spawned_at := -1
	for i in MAX_TICKS:
		var speed := float(vehicle.call("speed_mps"))
		vehicle.call("set_manual_input", 0.6 if speed < CRUISE_MPS else 0.0, 0.0, 0.0)
		await physics_frame
		if not (vehicle.call("scenario_hazards") as Array).is_empty():
			spawned_at = int(vehicle.call("tick_count"))
			break
	_check(spawned_at > 0, "보행자 대체 객체가 나타나지 않음")
	if spawned_at < 0:
		return
	var hazard: Dictionary = (vehicle.call("scenario_hazards") as Array)[0]
	_check(hazard.id == "pedestrian" and (hazard.position as Vector3).distance_to(Vector3(4.0, 0.0, -26.0)) < 1e-4, "등장 자리: " + str(hazard))
	await process_frame
	var node: Node3D = drive.hazard_nodes.get("pedestrian")
	_check(node != null and node.visible, "보행자 표시 노드가 없음")
	for i in REACTION_TICKS:
		await physics_frame
	await process_frame
	var walked: Dictionary = (vehicle.call("scenario_hazards") as Array)[0]
	_check(float((walked.position as Vector3).x) < 4.0 - 0.5, "보행자가 걷지 않음: " + str(walked))
	# process_frame 시그널은 노드 _process보다 먼저 나가므로 표시 노드는 한 틱(약 2 cm) 늦을 수 있다.
	_check(node != null and node.position.distance_to(walked.position) < 0.1, "표시 노드가 보행자 자리를 따라가지 않음: %s / %s" % [str(node.position if node else null), str(walked.position)])
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for i in MAX_TICKS:
		await physics_frame
		if str((vehicle.call("scenario_status") as Dictionary).get("guide", "")).contains("지나간 뒤"):
			break
	var status: Dictionary = vehicle.call("scenario_status")
	_check(int(status.collisions) == 0, "보행자와 충돌함: " + str(status))
	_check(str(status.guide).contains("지나간 뒤"), "멈춤 단계가 끝나지 않음: " + str(status))

	_press(KEY_F8)
	await process_frame
	await process_frame
	status = vehicle.call("scenario_status")
	_check(str(status.outcome) == "aborted", "F8로 중단되지 않음: " + str(status))
	var path := str(status.get("result_path", ""))
	_check(not path.is_empty() and FileAccess.file_exists(path), "결과 파일이 없음: " + path)
	var result: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(path)) if FileAccess.file_exists(path) else {}
	_check(str(result.get("format", "")) == "scenario-result-v1" and str(result.get("outcome", "")) == "aborted" and not bool(result.get("completed", true)), "결과 파일 머리: " + str(result.keys()))
	_check(str(result.get("session_id", "")).ends_with("-scenario-c-hazard"), "세션 ID: " + str(result.get("session_id", "")))
	var kinds := (result.get("records", []) as Array).map(func(event): return str(event.kind))
	for kind in ["step_done", "hazard_appeared", "brake_start", "finished"]:
		_check(kinds.has(kind), "결과 파일 기록에 %s가 없음: %s" % [kind, str(kinds)])
	var reaction := float((result.get("summary", {}) as Dictionary).get("reaction_s", [-1.0])[0])
	_check(absf(reaction - REACTION_TICKS / 64.0) < 0.05, "제동 반응 시간: %f" % reaction)
	_check(not (result.get("input_track", []) as Array).is_empty(), "입력 열이 비어 있음")
	_check(str((result.get("score", {}) as Dictionary).get("notice", "")).contains("임시 기준"), "점수 안내 없음")
	await process_frame
	var text: String = drive.scenario_label.text
	_check(text.contains("제동 반응 시간") and text.contains("임시 점수") and text.contains("결과 파일"), "끝 안내에 기록 요약이 없음: " + text)

	_press(KEY_F7)
	await process_frame
	await process_frame
	status = vehicle.call("scenario_status")
	_check((status.result_lines as PackedStringArray).is_empty() and str(status.result_path).is_empty(), "재시작 뒤 결과 요약이 남음")
	_check((vehicle.call("scenario_hazards") as Array).is_empty(), "재시작 뒤 보행자가 남음")
	await process_frame
	_check(not node.visible, "재시작 뒤 보행자 표시 노드가 보임")
	drive.free()


## 주행 화면을 만들어 `course` 코스를 연다.
func _open(course: String) -> Node:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start(course)
	return drive


## 시험 결과 폴더의 파일을 지운다.
func _clear_results() -> void:
	var dir := DirAccess.open(RESULT_DIR)
	if dir == null:
		return
	for file in dir.get_files():
		dir.remove(file)


## 기능 키 하나를 눌렀다 뗀 입력 이벤트를 보낸다.
func _press(key: Key) -> void:
	for pressed in [true, false]:
		var event := InputEventKey.new()
		event.physical_keycode = key
		event.keycode = key
		event.pressed = pressed
		root.push_input(event)


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
