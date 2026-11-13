# PLAN 4-11a 지역 보정의 Godot 경로 선택·저장·재생·주행·안내·오류 회귀 시험. 성공 0 / 실패 1.
# 실행: godot --headless --fixed-fps 64 --path godot --script res://tests/map_patch_test.gd
extends SceneTree

var failures := PackedStringArray()
var temp_root := ""
var patch_dir := ""
var snapshot_path := ""
var route_name := "map-patch-test-%d" % OS.get_process_id()
var saved_path := ""


## 비동기 시험 본체를 다음 프레임에 시작한다.
func _init() -> void:
	call_deferred("_run")


## 합성 스냅샷과 확인된 보정을 만들고 선택·저장·재생·주행·오류 표시를 차례로 검사한다.
func _run() -> void:
	if not ClassDB.class_exists("SimRoutePicker") or not ClassDB.class_exists("SimCourse") or not ClassDB.class_exists("SimVehicle"):
		_fail("Rust 확장이 등록되지 않았습니다")
		_finish()
		return
	temp_root = ProjectSettings.globalize_path("user://map-patch-test-%d" % OS.get_process_id())
	patch_dir = temp_root.path_join("patches")
	snapshot_path = temp_root.path_join("snapshot.json")
	DirAccess.make_dir_recursive_absolute(patch_dir)
	_write(snapshot_path, _snapshot_text())
	var patch_path := patch_dir.path_join("bridge.toml")
	var original := _patch_text()
	_write(patch_path, original)

	var without: Object = ClassDB.instantiate("SimRoutePicker")
	without.set("network", false)
	without.call("use_patch_dir", temp_root.path_join("empty"))
	_check(without.call("load_snapshot", snapshot_path).ok, "보정 전 스냅샷 열기 실패")
	_add_route_points(without)
	_check(not bool(without.call("route").ok), "보정 전인데 끊긴 OSM 경로가 생성됨")
	var plain_status := "\n".join(without.call("status_lines"))
	_check(plain_status.contains("지도 자료: OSM") and not plain_status.contains("OSM + 지역 보정"), "다른 코스에서 지역 보정 표시가 제거되지 않음: " + plain_status)

	var picker: Object = ClassDB.instantiate("SimRoutePicker")
	picker.set("network", false)
	picker.call("use_patch_dir", patch_dir)
	var opened: Dictionary = picker.call("load_snapshot", snapshot_path)
	_check(opened.ok, "보정 스냅샷 열기 실패: " + str(opened.error))
	_add_route_points(picker)
	var route: Dictionary = picker.call("route")
	_check(route.ok and float(route.length_m) > 150.0, "보정 적용 뒤 경로 생성 실패: " + str(route.error))
	var status := "\n".join(picker.call("status_lines"))
	_check(status.contains("지도 자료: OSM + 지역 보정"), "OSM + 지역 보정 안내 없음: " + status)
	_check(status.contains("합성 연결 통로") and status.contains("확인 상태: 확인됨"), "보정 이름·확인 상태 안내 없음: " + status)
	_check(status.contains("형상 정확도·실측 여부:") and status.contains("실측 아님"), "실측 여부 안내 없음: " + status)

	var saved: Dictionary = picker.call("save", route_name)
	_check(saved.ok, "보정 경로 저장 실패: " + str(saved.error))
	if not saved.ok:
		_finish()
		return
	saved_path = str(saved.path)
	var saved_text := FileAccess.get_file_as_string(saved_path)
	_check(saved_text.contains("[[map_patches]]") and saved_text.contains('id = "bridge"'), "저장 경로에 map_patches 참조가 없음")
	var course_name := "map:" + saved_path
	var built: Dictionary = ClassDB.class_call_static("SimCourse", "build_with_patch_dir", course_name, patch_dir)
	_check(built.get("ok", false), "보정 경로 재생 실패: " + str(built.get("error", "")))
	var notices := "\n".join(built.get("notices", PackedStringArray()))
	_check(notices.contains("지도 자료: OSM + 지역 보정") and notices.contains("합성 연결 통로"), "주행 안내에 지역 보정이 없음: " + notices)

	var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", course_name)
	vehicle.set("patch_dir", patch_dir)
	root.add_child(vehicle)
	await process_frame
	_check(vehicle.call("is_ready"), "보정 코스로 차량 준비 실패: " + str(vehicle.call("last_error")))
	var start := vehicle.position
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 0.7, 0.0, 0.0)
	for _frame in 1200:
		await physics_frame
		if vehicle.position.distance_to(start) > 70.0:
			break
	_check(vehicle.position.distance_to(start) > 70.0, "차량이 지역 보정 양쪽 접속점을 통과하지 못함: " + str(vehicle.position.distance_to(start)))
	vehicle.queue_free()
	await process_frame

	_write(patch_path, original.replace("unit test source", "unit tests source"))
	var tampered: Dictionary = ClassDB.class_call_static("SimCourse", "build_with_patch_dir", course_name, patch_dir)
	_check(not tampered.get("ok", false) and str(tampered.get("error", "")).contains("SHA-256"), "보정 변조 오류가 표시되지 않음: " + str(tampered))
	DirAccess.remove_absolute(patch_path)
	var missing: Dictionary = ClassDB.class_call_static("SimCourse", "build_with_patch_dir", course_name, patch_dir)
	_check(not missing.get("ok", false) and str(missing.get("error", "")).contains("missing map patch"), "보정 누락 오류가 표시되지 않음: " + str(missing))
	_finish()


## picker에 합성 경로의 출발·도착 위경도를 지도 좌표로 바꿔 추가한다.
func _add_route_points(picker: Object) -> void:
	picker.call("add_point", picker.call("latlon_to_view", PackedFloat64Array([35.0, 127.0002])))
	picker.call("add_point", picker.call("latlon_to_view", PackedFloat64Array([35.0, 127.0028])))


## 경로 시험용으로 서로 끊긴 OSM way 두 개의 스냅샷을 만든다.
func _snapshot_text() -> String:
	return '{"generator":"Overpass API test","elements":[{"type":"way","id":10,"nodes":[1,2],"geometry":[{"lat":35.0,"lon":127.0},{"lat":35.0,"lon":127.0005}],"tags":{"highway":"service"}},{"type":"way","id":20,"nodes":[3,4],"geometry":[{"lat":35.0,"lon":127.0008},{"lat":35.0,"lon":127.003}],"tags":{"highway":"service"}}]}'


## 끊긴 OSM way를 직선으로 잇는 배포 가능 상태의 합성 지역 보정 TOML을 만든다.
func _patch_text() -> String:
	return '''format = "map-patch-v1"
id = "bridge"
name = "합성 연결 통로"
revision = 1
bbox = [34.999, 126.999, 35.002, 127.004]
source_kind = "synthetic fixture"
source_description = "unit test source"
verified_at = "2026-11-13"
license = "test-only"
status = "verified"
geometry_accuracy = "합성 좌표 기반(실측 아님)"
access_assumption = "자동 시험 전용"
notice = "실제 도로가 아닌 자동 시험 자료"

[[anchors]]
id = "west"
osm_node = 2
lat = 35.0
lon = 127.0005
max_error_m = 1.0

[[anchors]]
id = "east"
osm_node = 3
lat = 35.0
lon = 127.0008
max_error_m = 1.0

[[nodes]]
id = "middle"
lat = 35.0
lon = 127.00065

[[ways]]
id = "connector"
nodes = ["anchor:west", "middle", "anchor:east"]
highway = "service"
oneway = false
width_m = 4.0
width_source = "synthetic"
'''


## UTF-8 시험 파일을 쓰고 실패하면 즉시 기록한다.
func _write(path: String, text: String) -> void:
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		_fail("시험 파일을 쓸 수 없음: " + path)
		return
	file.store_string(text)
	file.close()


## 조건이 거짓이면 실패 문구를 모은다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		_fail(message)


## 실패 문구 하나를 모은다.
func _fail(message: String) -> void:
	failures.append(message)


## 저장 경로와 임시 자료를 지우고 시험 결과를 출력한다.
func _finish() -> void:
	if not saved_path.is_empty() and FileAccess.file_exists(saved_path):
		DirAccess.remove_absolute(saved_path)
	if not temp_root.is_empty() and DirAccess.dir_exists_absolute(temp_root):
		_remove_tree(temp_root)
	if failures.is_empty():
		print("MAP PATCH OK: 선택 → 안내 → 저장 → 재생 → 차량 통과 → 누락·변조 오류")
		quit(0)
	else:
		for message in failures:
			printerr("MAP PATCH FAIL: " + message)
		quit(1)


## 시험이 만든 폴더 아래 파일과 하위 폴더만 재귀적으로 지운다.
func _remove_tree(path: String) -> void:
	var dir := DirAccess.open(path)
	if dir == null:
		return
	dir.list_dir_begin()
	var name := dir.get_next()
	while not name.is_empty():
		var child := path.path_join(name)
		if dir.current_is_dir():
			_remove_tree(child)
		else:
			DirAccess.remove_absolute(child)
		name = dir.get_next()
	dir.list_dir_end()
	DirAccess.remove_absolute(path)
