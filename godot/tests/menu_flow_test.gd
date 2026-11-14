# GD-4 화면 흐름 확인: 메인 메뉴 → 코스 선택(개발 코스·지도 코스) → Rust 차량 주행 → Esc로 메뉴 복귀. 성공 0 / 실패 1.
# presentation_smoke.gd는 정지 미리보기만 보므로, 여기서는 Rust SimVehicle이 실제로 틱을 돌고 HUD가 한국어로 갱신되는지까지 본다.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/menu_flow_test.gd
extends SceneTree

## 메뉴에 저장 경로로 보일 시험 경로 파일(시험 중에만 두고 지운다). 여수 고정 코스는 메뉴에서 빠졌으므로(PLAN 4-11)
## 시험 자료 경로를 저장 경로 자리(`cache/routes`)에 복사해 쓴다.
const SAVED_NAME := "menu-flow-test.toml"
## 메뉴에서 고를 코스(개발 코스 하나, 저장 경로 하나).
const COURSES := ["training", "map:cache/routes/" + SAVED_NAME]
## 주행 화면에서 기다릴 물리 틱 수.
const WAIT_TICKS := 32

var failures := PackedStringArray()
var main: Node


## 확장 확인 뒤 메인 장면을 띄우고 흐름 검사를 시작한다.
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		printerr("MENU FLOW FAIL: Rust 확장이 등록되지 않았습니다.")
		quit(1)
		return
	call_deferred("_run")


## 저장소 최상위 기준 경로를 절대 경로로 바꾼다.
func _repo_path(relative: String) -> String:
	return ProjectSettings.globalize_path("res://").path_join("..").path_join(relative).simplify_path()


## 시험 자료 여수 경로를 저장 경로 폴더에 복사한다(스냅샷은 시험 자료를 가리키게 고친다).
func _write_saved_route() -> void:
	var text := FileAccess.get_file_as_string(_repo_path("testdata/yeosu-expo/route-junctions.toml"))
	text = text.replace("snapshot = \"graph-2026-11-06.osm.json\"", "snapshot = \"../../testdata/yeosu-expo/graph-2026-11-06.osm.json\"")
	DirAccess.make_dir_recursive_absolute(_repo_path("cache/routes"))
	var file := FileAccess.open(_repo_path("cache/routes/" + SAVED_NAME), FileAccess.WRITE)
	file.store_string(text)
	file.close()


## 코스마다 메뉴에서 골라 주행 화면을 열고 Esc로 돌아온다.
func _run() -> void:
	_write_saved_route()
	main = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	for course in COURSES:
		await _drive_and_return(course)
	_check(main.screen.has_signal("quit_requested"), "메뉴에 종료 요청 시그널 없음")
	_check(not Array(main.screen.course_names).any(func(name): return str(name).contains("yeosu-expo")), "여수 고정 코스가 메뉴에 남음")
	main.free()
	DirAccess.remove_absolute(_repo_path("cache/routes/" + SAVED_NAME))
	if failures.is_empty():
		print("MENU FLOW OK: 메뉴 → %s 주행 → Esc 메뉴 복귀" % " / ".join(COURSES))
		quit(0)
	else:
		for message in failures:
			printerr("MENU FLOW FAIL: " + message)
		quit(1)


## 메뉴 목록에서 `course`를 골라 열고, 차량 틱·HUD를 확인한 뒤 Esc 키 이벤트로 메뉴에 돌아온다.
func _drive_and_return(course: String) -> void:
	var menu: Node = main.screen
	_check(menu.has_signal("drive_requested") and not main.back_button.visible, "메뉴 화면이 아님(" + course + ")")
	var index: int = Array(menu.course_names).find(course)
	_check(index >= 0, "메뉴 코스 목록에 없음: " + course)
	if index < 0:
		return
	menu.courses.select(index)
	menu.call("_open_course")
	var drive: Node = main.screen
	_check(drive.has_method("start") and main.back_button.visible, "주행 화면으로 바뀌지 않음(" + course + ")")
	_check(drive.failure.is_empty(), "주행 화면 실패(" + course + "): " + drive.failure)
	_check(not drive.is_preview, "Rust 차량이 아니라 미리보기로 열림(" + course + ")")
	if not drive.failure.is_empty():
		return
	var start_tick: int = drive.vehicle.call("tick_count")
	for i in WAIT_TICKS:
		await physics_frame
	await process_frame
	_check(int(drive.vehicle.call("tick_count")) > start_tick, "차량 틱이 돌지 않음(" + course + ")")
	_check(drive.hud.text.contains("기어 P") and drive.hud.text.contains("시동 꺼짐"), "HUD 한국어 표시 아님: " + drive.hud.text)
	var escape := InputEventKey.new()
	escape.physical_keycode = KEY_ESCAPE
	escape.keycode = KEY_ESCAPE
	escape.pressed = true
	root.push_input(escape)
	await process_frame
	_check(main.screen.has_signal("drive_requested") and not main.back_button.visible, "Esc로 메뉴에 돌아오지 않음(" + course + ")")
	_check(not is_instance_valid(drive) or drive.get_parent() == null, "이전 주행 화면이 트리에 남음(" + course + ")")


## 실패 항목을 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)
