extends SceneTree
const CourseMesh = preload("res://presentation/course_mesh.gd")

## 여수 경로 좌표계에 합성 지면을 넣고 차량 데이터와 렌더링 메시까지 검사한다.
func _initialize() -> void:
	call_deferred("_run")

## 원본 여수 경로를 임시 복사하여 기존 저장 코스에는 손대지 않는다.
func _run() -> void:
	var source := ProjectSettings.globalize_path("res://../testdata/yeosu-expo")
	var stem := "scenery-test-%d" % OS.get_process_id()
	var dir := "user://" + stem
	DirAccess.make_dir_recursive_absolute(dir)
	var graph := "graph-2026-11-06.osm.json"
	var course_file := dir.path_join("course.toml")
	var graph_file := dir.path_join(graph)
	var places_file := dir.path_join(graph.replace(".osm.json", ".places.osm.json"))
	var route_text := FileAccess.get_file_as_string(source.path_join("route-junctions.toml"))
	_write(course_file, route_text.replace("stub_length_m = 10.0", "stub_length_m = 0.0"))
	_write(graph_file, FileAccess.get_file_as_string(source.path_join(graph)))
	var elements: Array = []
	for tags in [{"leisure":"park"}, {"natural":"water"}, {"amenity":"parking"}]:
		var lat := 34.7521 - elements.size() * 0.0002
		var a := {"lat": lat, "lon":127.7472}
		elements.append({"id": elements.size() + 1, "type":"way", "tags":tags, "geometry":[a, {"lat":lat, "lon":127.7474}, {"lat":lat+0.0001,"lon":127.7474}, {"lat":lat+0.0001,"lon":127.7472}, a]})
	_write(places_file, JSON.stringify({"elements":elements}))
	var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", "map:" + ProjectSettings.globalize_path(course_file))
	root.add_child(vehicle)
	var data: Dictionary = vehicle.call("course_data")
	var ok := bool(data.get("ok", false))
	var mesh := CourseMesh.new()
	root.add_child(mesh)
	if ok:
		mesh.build(data)
		for kind in ["green", "water", "parking"]:
			ok = ok and mesh.get_node_or_null("Scenery_" + kind) is MeshInstance3D
	mesh.free()
	vehicle.free()
	for path in [places_file, graph_file, course_file, dir]: DirAccess.remove_absolute(path)
	if ok: print("ROUTE SCENERY OK: green, water, parking")
	else: printerr("ROUTE SCENERY FAIL: ", data.get("error", "missing surface mesh"))
	quit(0 if ok else 1)

## 테스트용 자료를 닫힌 파일로 저장한다.
func _write(path: String, content: String) -> void:
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_string(content)
	file.close()
