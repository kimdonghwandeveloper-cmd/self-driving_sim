extends SceneTree
## Lanelet2 메뉴 선택·형상·출처·연결부 주행·목표·재로드와 오류 안내를 검사한다.

const Drive = preload("res://presentation/drive_screen.gd")
const COURSE := "map:assets/maps/lanelet2-demo/course.toml"
const BAD_PATH := "cache/routes/lanelet2-godot-invalid-test.toml"
var failures := PackedStringArray()


## Rust 확장이 준비된 경우에만 시험을 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("LANELET2 FAIL: Rust 확장 없음")
		quit(1)
		return
	call_deferred("_run")


## 메뉴에서 예제를 열고 연결부 주행·다시 열기·잘못된 파일 처리를 검사한다.
func _run() -> void:
	var main: Node = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	var menu: Node = main.screen
	var index: int = Array(menu.course_names).find(COURSE)
	_check(index >= 0, "메뉴에 Lanelet2 예제 없음")
	if index >= 0:
		_check(menu.courses.get_item_text(index).contains("합성"), "메뉴의 합성 표시 없음")
		menu.courses.select(index)
		menu.call("_open_course")
		var drive: Node = main.screen
		_check(drive.failure.is_empty() and drive.fallback_notice.is_empty(), "코스 열기 실패")
		if drive.failure.is_empty() and drive.fallback_notice.is_empty():
			await _check_drive(drive)
	main.free()
	_check_reloading()
	await _check_errors()
	if failures.is_empty():
		print("LANELET2 OK: 메뉴 · 형상 · 출처 · 차선 연결 주행 · 목표 · 재로드 · 오류 안내")
		quit(0)
	else:
		for message in failures:
			printerr("LANELET2 FAIL: " + message)
		quit(1)


## 실제 Rust 차량을 두 차선 연결부 너머 목표 근처까지 주행하고 정지·리셋한다.
func _check_drive(drive: Node) -> void:
	var vehicle: Node3D = drive.vehicle
	var data: Dictionary = vehicle.call("course_data")
	_check(data.name == "lanelet2-synthetic-demo", "다른 코스로 열림")
	_check(data.strips.size() == 2, "연결 차선 면 2개가 아님")
	_check(drive.scene_content.find_child("GoalMarker", true, false) != null, "목표 표시 없음")
	_check(drive.scene_content.find_children("*", "MeshInstance3D", true, false).size() > 0, "노면 메시 없음")
	_check(drive.notice.text.contains("합성") and drive.notice.text.contains("실제 지도"), "합성 자료 안내 없음")
	_check(drive.notice.text.contains("self-driving_sim"), "출처 표시 없음")
	vehicle.set("read_input", false)
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	var goal: Vector3 = data.goal.position
	for tick in 2048:
		var speed := float(vehicle.call("speed_mps"))
		vehicle.call("set_manual_input", clampf((3.0 - speed) * 0.5, 0.0, 0.5), 0.0, 0.0)
		await physics_frame
		if vehicle.position.distance_to(goal) < 5.0:
			break
	_check(vehicle.position.distance_to(data.start.position) > 35.0, "차선 연결부를 통과하지 못함")
	_check(vehicle.position.distance_to(goal) < 5.0, "목표 근처에 도달하지 못함")
	_check(not bool(vehicle.call("in_contact")), "연석 또는 벽에 닿음")
	vehicle.call("set_manual_input", 0.0, 1.0, 0.0)
	for tick in 640:
		await physics_frame
		if absf(float(vehicle.call("speed_mps"))) < 1e-4:
			break
	vehicle.call("send_command", "reset_course")
	await physics_frame
	_check(vehicle.position.distance_to(data.start.position) < 1e-3, "리셋 시작 위치 불일치")


## 같은 코스를 두 번 열면 시작·목표·도로 면·충돌 형상이 같은지 검사한다.
func _check_reloading() -> void:
	var first: Dictionary = ClassDB.class_call_static("SimCourse", "build", COURSE)
	var second: Dictionary = ClassDB.class_call_static("SimCourse", "build", COURSE)
	_check(first.get("ok", false) and second.get("ok", false), "재로드 실패")
	for key in ["start", "goal", "strips", "colliders"]:
		_check(first.get(key) == second.get(key), "재로드 결과 불일치: " + key)


## 없는 OSM·잘못된 ID에서 원인이 나오고 주행 화면에 대체 안내가 남는지 검사한다.
func _check_errors() -> void:
	var absolute := ProjectSettings.globalize_path("res://").path_join("../" + BAD_PATH).simplify_path()
	if FileAccess.file_exists(absolute):
		_check(false, "시험 파일이 이미 있어 덮어쓰지 않음: " + absolute)
		return
	DirAccess.make_dir_recursive_absolute(absolute.get_base_dir())
	var template := 'format = "lanelet2-course-v1"\nname = "invalid"\nosm = "%s"\norigin = [34.0, 127.0]\nstart_lanelet = %d\ngoal_lanelet = 302\n'
	for entry in [["missing-lanelet2.osm", 301, "cannot read osm"], ["../../assets/maps/lanelet2-demo/synthetic.osm", 9999, "9999"]]:
		var file := FileAccess.open(absolute, FileAccess.WRITE)
		file.store_string(template % [entry[0], entry[1]])
		file.close()
		var result: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + BAD_PATH)
		_check(not result.get("ok", true), "잘못된 코스가 성공함")
		_check(str(result.get("error", "")).contains(str(entry[2])), "오류 원인 누락")
		var drive := Drive.new()
		root.add_child(drive)
		drive.start("map:" + BAD_PATH)
		await process_frame
		_check(drive.failure.is_empty() and not drive.fallback_notice.is_empty(), "대체 코스 또는 안내 실패")
		_check(drive.warnings_label.text.contains(str(entry[2])), "실패 사유가 화면에 없음")
		drive.free()
	DirAccess.remove_absolute(absolute)


## 실패 항목을 모아 마지막에 함께 보고한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
