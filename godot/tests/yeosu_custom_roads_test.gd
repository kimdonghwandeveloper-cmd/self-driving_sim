extends SceneTree
## 메뉴에서 "여수 엑스포 · 내 맵 도로" 코스를 열어 맵 도로 면·연석·배경이 함께 동작하는지 확인한다.

const Main = preload("res://main.tscn")
const Drive = preload("res://presentation/drive_screen.gd")

## 메뉴 경유 열기, 도로 면·연석 합친 메시, 배경, 도로 이탈 판정, 주행을 검사한다.
func _initialize() -> void:
	call_deferred("_run")

## 실제 차량에 몇 초간 가속을 보내 움직이는지와 장면 구성을 확인한다.
func _run() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		_finish(false, "Rust 확장을 불러오지 못했습니다.")
		return
	var app := Main.instantiate()
	root.add_child(app)
	var menu: Control = app.screen
	var index: int = menu.course_names.find(Drive.YEOSU_CUSTOM_ROADS_COURSE)
	if index < 0:
		_finish(false, "내 맵 도로 코스가 메뉴에 없습니다.")
		return
	menu.courses.select(index)
	menu._open_course()
	var drive: Node3D = app.screen
	if not drive.failure.is_empty() or not drive.fallback_notice.is_empty():
		_finish(false, "코스 초기화 실패: " + drive.failure + drive.fallback_notice)
		return
	if drive.scene_content.get_node_or_null("YeosuBackground") == null:
		_finish(false, "여수 배경이 없습니다.")
		return
	var vehicle: Node3D = drive.vehicle
	var data: Dictionary = vehicle.call("course_data")
	if data.areas.size() < 500 or data.colliders.size() < 500:
		_finish(false, "도로 면 또는 연석 수가 너무 적습니다: %d / %d" % [data.areas.size(), data.colliders.size()])
		return
	if drive.scene_content.find_child("Colliders_curb", true, false) == null:
		_finish(false, "연석 합친 메시가 없습니다.")
		return
	if not drive.on_road(vehicle.position) or drive.on_road(Vector3(5000.0, 0.0, 5000.0)):
		_finish(false, "도로 이탈 판정이 맞지 않습니다(시작점은 도로 위, 먼 곳은 도로 밖이어야 함).")
		return
	vehicle.set("read_input", false)
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
	var start := vehicle.position
	for i in range(192):
		await physics_frame
	_finish(vehicle.position.distance_to(start) >= 1.0 and float(vehicle.call("speed_mps")) >= 0.5,
		"내 맵 도로 코스에서 차량이 가속해 움직이지 않았습니다.")

## 검증 결과와 종료 코드를 남긴다.
func _finish(ok: bool, message: String) -> void:
	if ok:
		print("YEOSU CUSTOM ROADS OK: menu, road polygons, curbs, background, off-road check, driving")
	else:
		printerr("YEOSU CUSTOM ROADS FAIL: " + message)
	quit(0 if ok else 1)
