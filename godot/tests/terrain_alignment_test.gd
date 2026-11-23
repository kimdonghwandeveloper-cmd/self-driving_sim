extends SceneTree

const CourseMesh = preload("res://presentation/course_mesh.gd")

## 합성 언덕 위의 건물과 도로가 동일 높이 기준을 쓰는지 확인한다.
func _initialize() -> void:
	call_deferred("_run")

## 도로 아래에 지면이 있고, 건물 지붕이 기초에서 지정 높이만큼 올라가는지 검사한다.
func _run() -> void:
	var mesh := CourseMesh.new()
	root.add_child(mesh)
	var footprint := PackedVector3Array([Vector3(6, 0, 2), Vector3(8, 0, 2), Vector3(8, 0, 4), Vector3(6, 0, 4)])
	mesh.build({
		"ok": true,
		"strips": [{"kind": "road", "left": PackedVector3Array([Vector3(-4, 0, -8), Vector3(-4, 0, 8)]),
			"right": PackedVector3Array([Vector3(0, 0, -8), Vector3(0, 0, 8)])}],
		"scenery": {"parking": PackedVector3Array([Vector3(-10, 0, -10), Vector3(-10, 0, 10), Vector3(10, 0, 0)])},
		"buildings": [{"footprint": footprint, "height_m": 5.0}],
	}, _hill)
	var terrain := mesh.get_node_or_null("Terrain") as MeshInstance3D
	var walls := mesh.get_node_or_null("Buildings_Walls") as MeshInstance3D
	var roofs := mesh.get_node_or_null("Buildings_Roofs") as MeshInstance3D
	var ok := terrain != null and walls != null and roofs != null
	ok = ok and mesh.get_node_or_null("Scenery_parking") == null
	ok = ok and walls.get_aabb().position.y > 17.0 and roofs.get_aabb().position.y > 23.0
	print("TERRAIN ALIGNMENT ", "OK" if ok else "FAIL")
	mesh.free()
	quit(0 if ok else 1)

## 평지 기준에서 충분히 올라간 연속 지형을 만든다.
func _hill(x: float, z: float) -> float:
	return 20.0 + (x + z) * 0.1
