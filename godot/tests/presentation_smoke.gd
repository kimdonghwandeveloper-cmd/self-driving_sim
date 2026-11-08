extends SceneTree
## 정지 미리보기 장면과 코스 메시, 네 방향 카메라, 지도 화면의 오프라인 연결을 검사한다.

const Drive = preload("res://presentation/drive_screen.gd")
const Map = preload("res://presentation/map_screen.gd")
var failures := PackedStringArray()
var capture_dir := ""

## 노드 생성이 허용되는 프레임에서 장면 검사를 시작한다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture-dir="):
			capture_dir = arg.trim_prefix("--capture-dir=")
	call_deferred("_run")

## 장면을 실제 트리에 넣어 생성·시점·화면 재진입을 확인한다.
func _run() -> void:
	var drive := Drive.new()
	root.add_child(drive)
	drive.start("training", true)
	await process_frame
	_check(drive.failure.is_empty(), "drive setup")
	_check(drive.is_preview, "preview must be explicit")
	_check(not _has_physics(drive), "presentation must not create physics colliders")
	var directions := {"front": Vector3.FORWARD, "rear": Vector3.BACK, "left": Vector3.LEFT, "right": Vector3.RIGHT}
	for direction in directions:
		drive.rig.apply_view(direction)
		var forward: Vector3 = -drive.rig.camera.global_basis.z
		_check(forward.normalized().dot(directions[direction]) > 0.99, "camera " + direction)
		await _capture(direction)
	drive.rig.set_external(true)
	_check(drive.rig.camera.cull_mask == 3, "external body visibility")
	await _capture("external")
	drive.rig.set_external(false)
	_check(drive.rig.camera.cull_mask == 1, "interior body hidden")
	drive.free()
	var map := Map.new()
	root.add_child(map)
	map.display_geometry(PackedVector2Array([Vector2.ZERO, Vector2(10, 10)]), PackedByteArray([1]), PackedVector2Array([Vector2.ZERO]), PackedVector2Array())
	await process_frame
	_check(map.roads.size() == 2, "map display layer")
	await _capture("map")
	map.free()
	var main = load("res://main.tscn").instantiate()
	root.add_child(main)
	main.call("_show_map")
	main.call("_show_drive", "calibration")
	await process_frame
	_check(main.screen.failure.is_empty(), "screen re-entry")
	await _capture("main")
	main.free()
	if failures.is_empty():
		print("PRESENTATION SMOKE OK: course mesh, four views, HUD, map, re-entry; no driving claim")
		quit(0)
	else:
		for message in failures:
			printerr("PRESENTATION FAIL: " + message)
		quit(1)

## 표시 전용 트리에 물리 충돌 노드가 섞이지 않았는지 재귀 확인한다.
func _has_physics(node: Node) -> bool:
	if node is CollisionObject3D:
		return true
	for child in node.get_children():
		if _has_physics(child):
			return true
	return false

## 실패 항목을 모아 한 번에 보고한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)

## 선택적으로 실제 렌더링 결과를 저장한다. 헤드리스 검사에서는 실행하지 않는다.
func _capture(label: String) -> void:
	if capture_dir.is_empty():
		return
	await process_frame
	await RenderingServer.frame_post_draw
	DirAccess.make_dir_recursive_absolute(capture_dir)
	var image := root.get_texture().get_image()
	_check(image.save_png(capture_dir.path_join(label + ".png")) == OK, "capture " + label)