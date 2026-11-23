extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
const CANDIDATES := [
	["서울 남산", Vector2(37.55261, 126.98399), Vector2(37.55160, 126.98524)],
	["서울 남산", Vector2(37.55378, 126.98295), Vector2(37.55095, 126.98182)],
	["서울 해방촌", Vector2(37.54405, 126.98537), Vector2(37.54644, 126.98533)],
	["부산 산복도로", Vector2(35.11810, 129.03450), Vector2(35.12020, 129.03692)],
]
var course_name := ""

## 전국 OSM에서 실제 핀 경로를 골라 고도 타일과 주행을 검증한다.
func _initialize() -> void:
	call_deferred("_run")

## 핀 지도에서 경로를 저장하고 같은 주행 화면에서 고도 적용과 캐시 재생을 확인한다.
func _run() -> void:
	for candidate in CANDIDATES:
		var screen = load("res://presentation/map_screen.tscn").instantiate()
		root.add_child(screen)
		await process_frame
		course_name = ""
		screen.drive_requested.connect(_capture_course)
		screen._clear_points()
		for slot in range(2):
			var point: Vector2 = candidate[slot + 1]
			var view: Vector2 = screen.picker.call("latlon_to_view", PackedFloat64Array([point.x, point.y]))
			if not screen._set_pin(slot, view):
				break
		var route: Dictionary = screen.picker.call("route")
		if route.get("ok", false):
			screen._drive_route()
		screen.free()
		if course_name.is_empty():
			print("ELEVATION ROUTE SKIP: ", candidate[0], " ", route.get("error", ""))
			continue
		var drive := Drive.new()
		root.add_child(drive)
		drive.start(course_name)
		var vehicle: Node3D = drive.vehicle
		var data: Dictionary = vehicle.call("course_data")
		if not data.get("has_elevation", false):
			print("ELEVATION ROUTE SKIP: ", candidate[0], " ", data.get("elevation_notice", drive.failure))
			drive.free()
			_remove_route()
			continue
		var goal: Vector3 = data.goal.position
		var start := vehicle.position
		var initial := float(vehicle.call("road_height", start.x, start.z))
		var end := float(vehicle.call("road_height", goal.x, goal.z))
		if absf(end - initial) < 0.5:
			drive.free()
			_remove_route()
			continue
		vehicle.set("read_input", false)
		await physics_frame
		await physics_frame
		vehicle.call("send_command", "engine_toggle")
		vehicle.call("send_command", "gear_drive")
		vehicle.call("set_manual_input", 0.55, 0.0, 0.0)
		var moved_height := 0.0
		var aligned := true
		for i in range(384):
			await physics_frame
			var here := float(vehicle.call("road_height", vehicle.position.x, vehicle.position.z))
			aligned = aligned and is_equal_approx(vehicle.position.y, here)
			moved_height = maxf(moved_height, absf(here - initial))
		var ok := aligned and start.distance_to(vehicle.position) > 3.0 and moved_height > 0.02
		ok = _check_terrain(drive) and ok
		drive.free()
		var replay: Node3D = ClassDB.instantiate("SimVehicle")
		replay.set("course", course_name)
		root.add_child(replay)
		ok = ok and replay.call("course_data").get("has_elevation", false)
		ok = ok and is_equal_approx(float(replay.call("road_height", goal.x, goal.z)), end)
		replay.free()
		_remove_route()
		print("NATIONWIDE ELEVATION ", "OK" if ok else "FAIL", ": ", candidate[0], " ", route.length_m,
			" m, endpoint delta ", end - initial, " m, driven height change ", moved_height, " m")
		if ok:
			quit(0)
			return
	printerr("NATIONWIDE ELEVATION FAIL: no verified non-Yeosu route")
	quit(1)

## 실제 생성된 도로·지붕 삼각형과 지면 표면의 간격을 검사한다.
func _check_terrain(drive: Node) -> bool:
	var course_mesh: Node = drive.scene_content.get_child(0)
	var terrain = course_mesh._terrain
	var roofs := course_mesh.get_node_or_null("Buildings_Roofs") as MeshInstance3D
	if terrain == null or roofs == null or course_mesh.get_node_or_null("Terrain") == null:
		return false
	var road_min := INF
	var road_count := 0
	var roof_min := INF
	for node in course_mesh.get_children():
		if str(node.name).begins_with("Scenery_"):
			return false
		if node.has_meta("road_surface"):
			road_min = minf(road_min, _clearance(node, terrain))
			road_count += 1
	roof_min = _clearance(roofs, terrain)
	print("TERRAIN CLEARANCE: minimum sampled road ", road_min, " m, roof ", roof_min, " m")
	return road_count > 0 and road_min >= 0.09 and roof_min > 0.0

## 각 메시 삼각형의 꼭짓점과 내부 중심에서 지면으로부터 높이를 잰다.
func _clearance(mesh_instance: MeshInstance3D, terrain: RefCounted) -> float:
	var vertices: PackedVector3Array = mesh_instance.mesh.surface_get_arrays(0)[Mesh.ARRAY_VERTEX]
	var result := INF
	for i in range(0, vertices.size(), 3):
		var a := vertices[i]
		var b := vertices[i + 1]
		var c := vertices[i + 2]
		for p: Vector3 in [a, b, c, (a + b + c) / 3.0]:
			result = minf(result, p.y - terrain.height_at(p.x, p.z))
	return result

## 지도에서 요청한 저장 코스를 수신한다.
func _capture_course(path: String) -> void:
	course_name = path

## 시험 실행에서 생성한 저장 경로만 정리한다.
func _remove_route() -> void:
	var path := ProjectSettings.globalize_path("res://../" + course_name.trim_prefix("map:"))
	DirAccess.remove_absolute(path)
