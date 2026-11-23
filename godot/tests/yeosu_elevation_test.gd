extends SceneTree
const Drive = preload("res://presentation/drive_screen.gd")

## 실제 여수 고도 자료를 주행 화면에서 검증한다.
func _initialize() -> void:
	call_deferred("_run")

## 평지 코스와 같은 입력을 적용하여 높이와 속도가 달라지는지 확인한다.
func _run() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start(Drive.YEOSU_ELEVATION_COURSE)
	if not drive.failure.is_empty() or not drive.fallback_notice.is_empty():
		printerr("YEOSU ELEVATION FAIL: ", drive.failure, " ", drive.fallback_notice)
		drive.free()
		quit(1)
		return
	var elevated: Node3D = drive.vehicle
	var flat: Node3D = ClassDB.instantiate("SimVehicle")
	flat.set("course", Drive.YEOSU_COURSE)
	root.add_child(flat)
	var data: Dictionary = elevated.call("course_data")
	var ok := bool(data.get("has_elevation", false)) and bool(flat.call("is_ready"))
	ok = ok and elevated.position.y > 0.1 and is_zero_approx(flat.position.y)
	var curbs := drive.scene_content.find_child("Colliders_curb", true, false) as MeshInstance3D
	ok = ok and curbs != null and curbs.get_aabb().size.y > 1.0
	var road: MeshInstance3D
	for node in drive.scene_content.find_children("*", "MeshInstance3D", true, false):
		if node.get_aabb().size.y > 1.0 and node.name != "Colliders_curb":
			road = node
			break
	ok = ok and road != null
	var start := elevated.position
	var max_speed_delta := 0.0
	for vehicle in [elevated, flat]:
		vehicle.set("read_input", false)
		vehicle.call("send_command", "engine_toggle")
		vehicle.call("send_command", "gear_drive")
		vehicle.call("set_manual_input", 0.25, 0.0, 0.0)
	for i in range(384):
		await physics_frame
		var height := float(elevated.call("road_height", elevated.position.x, elevated.position.z))
		ok = ok and is_equal_approx(elevated.position.y, height)
		max_speed_delta = maxf(max_speed_delta, absf(elevated.call("speed_mps") - flat.call("speed_mps")))
	ok = ok and start.distance_to(elevated.position) > 5.0
	ok = ok and absf(start.y - elevated.position.y) > 0.05 and max_speed_delta > 0.02
	print("YEOSU ELEVATION ", "OK" if ok else "FAIL", ": start height ", start.y,
		" m, end height ", elevated.position.y, " m, max speed delta ", max_speed_delta, " m/s")
	drive.free()
	flat.free()
	quit(0 if ok else 1)
