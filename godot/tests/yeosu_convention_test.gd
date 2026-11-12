extends SceneTree
const Drive = preload("res://presentation/drive_screen.gd")
const Main = preload("res://main.tscn")

## Open the menu before testing each course through its normal entry.
func _initialize() -> void:
	call_deferred("_run")

## Verify each traced road can be opened, has colliders, and can be driven.
func _run() -> void:
	if not ClassDB.class_exists("SimVehicle"):
		_fail("Rust extension missing")
		return
	var app := Main.instantiate()
	root.add_child(app)
	for key: String in Drive.CONVENTION_COURSES:
		app._show_menu()
		var course := Drive.CONVENTION_PREFIX + key + ".toml"
		var index: int = app.screen.course_names.find(course)
		if index < 0:
			_fail(key + " missing from menu")
			return
		app.screen.courses.select(index)
		app.screen._open_course()
		var drive: Node3D = app.screen
		if not drive.failure.is_empty() or not drive.fallback_notice.is_empty():
			_fail(key + ": " + drive.failure + drive.fallback_notice)
			return
		var bg: Node3D = drive.scene_content.get_node_or_null("YeosuBackground")
		if bg == null or bg.find_child("Convention_" + key, true, false) == null or not bg.find_children("*", "CollisionObject3D", true, false).is_empty():
			_fail(key + " background missing or has collision")
			return
		var vehicle: Node3D = drive.vehicle
		var course_data: Dictionary = vehicle.call("course_data")
		if course_data.colliders.is_empty() or course_data.strips.is_empty():
			_fail(key + " course geometry missing")
			return
		vehicle.set("read_input", false)
		vehicle.call("send_command", "engine_toggle")
		vehicle.call("send_command", "gear_drive")
		vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
		var start := vehicle.position
		for i in range(192):
			await physics_frame
		if vehicle.position.distance_to(start) < 0.4:
			_fail(key + " vehicle did not move")
			return
		print("CONVENTION ROAD OK: ", key)
	print("CONVENTION ALL SIX OK")
	quit(0)

## Report a failed course and stop.
func _fail(reason: String) -> void:
	printerr("CONVENTION FAIL: " + reason)
	quit(1)
