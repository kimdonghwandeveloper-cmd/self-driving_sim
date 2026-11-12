extends SceneTree
## 실제 메뉴에서 여수 코스를 열어 배경과 기존 주행 세션이 함께 동작하는지 확인한다.

const Main = preload("res://main.tscn")
const Drive = preload("res://presentation/drive_screen.gd")

## 메뉴 경유 주행, Rust 코스 데이터 보존, 배경의 충돌체 부재와 화면 전환을 검사한다.
func _initialize() -> void:
	call_deferred("_run")

## 실제 차량에 몇 초간 가속을 보내고 다른 코스에는 배경이 없는지 확인한다.
func _run() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		_finish(false, "Rust 확장을 불러오지 못했습니다.")
		return
	var app := Main.instantiate()
	root.add_child(app)
	var menu: Control = app.screen
	var index: int = menu.course_names.find(Drive.YEOSU_COURSE)
	if index < 0:
		_finish(false, "여수 코스가 메뉴에 없습니다.")
		return
	menu.courses.select(index)
	menu._open_course()
	var drive: Node3D = app.screen
	if not drive.failure.is_empty() or not drive.fallback_notice.is_empty():
		_finish(false, "여수 코스 초기화 실패: " + drive.failure + drive.fallback_notice)
		return
	var background := drive.scene_content.get_node_or_null("YeosuBackground") as Node3D
	if background == null or background.transform != Transform3D.IDENTITY 			or background.find_children("*", "MeshInstance3D", true, false).is_empty() 			or not background.find_children("*", "CollisionObject3D", true, false).is_empty() 			or not background.find_children("*", "CollisionShape3D", true, false).is_empty():
		_finish(false, "배경 메시, 원점 또는 충돌체 조건이 맞지 않습니다.")
		return
	var vehicle: Node3D = drive.vehicle
	var original: Dictionary = ClassDB.class_call_static("SimCourse", "build", Drive.YEOSU_COURSE)
	var active: Dictionary = vehicle.call("course_data")
	if not original.get("ok", false) or original.colliders.is_empty() 			or original.colliders != active.colliders or original.strips != active.strips:
		_finish(false, "기존 코스의 연석 또는 노면 데이터가 달라졌습니다.")
		return
	vehicle.set("read_input", false)
	vehicle.call("send_command", "engine_toggle")
	vehicle.call("send_command", "gear_drive")
	vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
	var start := vehicle.position
	for i in range(192):
		await physics_frame
	if vehicle.position.distance_to(start) < 1.0 or float(vehicle.call("speed_mps")) < 0.5:
		_finish(false, "여수 코스에서 차량이 가속해 움직이지 않았습니다.")
		return
	app._show_menu()
	app._show_drive("training")
	_finish(app.screen.scene_content.get_node_or_null("YeosuBackground") == null,
		"여수 배경이 다른 코스에 남았습니다.")

## 검증 결과와 종료 코드를 남긴다.
func _finish(ok: bool, message: String) -> void:
	if ok:
		print("YEOSU BACKGROUND OK: menu, mesh, original course, driving, and isolation")
	else:
		printerr("YEOSU BACKGROUND FAIL: " + message)
	quit(0 if ok else 1)
