# GD-BUILDING 3D 건물 메시 절차적 생성 및 렌더링 무결성 시험 (PLAN 4-12).
# 코스 사전의 buildings 항목으로부터 벽면(Buildings_Walls)과 지붕(Buildings_Roofs) 메시가
# SurfaceTool 배칭을 통해 정상적으로 생성되고 AABB 영역이 유효한지 검증한다. 성공 0 / 실패 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/building_mesh_test.gd
extends SceneTree

const CourseMesh = preload("res://presentation/course_mesh.gd")

var failures := PackedStringArray()

func _init() -> void:
	print("--- 3D 건물 생성 검증 시험 시작 ---")
	_test_building_mesh_generation()
	_test_wall_winding()
	_test_real_course_buildings()
	_finish()

## 가상 건물 데이터를 포함하는 코스 사전으로 CourseMesh를 생성하고 메시 노드 및 형상을 검사한다.
func _test_building_mesh_generation() -> void:
	var mesh_node := CourseMesh.new()
	root.add_child(mesh_node)

	# 건물 2개 (직사각형 1개, 5각형 1개)
	var test_buildings := [
		{
			"id": "way/1001",
			"name": "테스트 아파트",
			"kind": "apartments",
			"height_m": 18.0,
			"is_estimated": true,
			"footprint": PackedVector3Array([
				Vector3(20, 0, -50),
				Vector3(40, 0, -50),
				Vector3(40, 0, -30),
				Vector3(20, 0, -30)
			])
		},
		{
			"id": "way/1002",
			"name": "테스트 상가",
			"kind": "commercial",
			"height_m": 12.0,
			"is_estimated": false,
			"footprint": PackedVector3Array([
				Vector3(-30, 0, -60),
				Vector3(-15, 0, -60),
				Vector3(-10, 0, -45),
				Vector3(-25, 0, -40),
				Vector3(-35, 0, -45)
			])
		}
	]

	var course_data := {
		"ok": true,
		"name": "building_test_course",
		"notices": PackedStringArray(["테스트"]),
		"surfaces": [],
		"strips": [],
		"areas": [],
		"markings": [],
		"colliders": [],
		"buildings": test_buildings
	}

	mesh_node.build(course_data)

	var walls_node: MeshInstance3D = mesh_node.find_child("Buildings_Walls", true, false)
	var roofs_node: MeshInstance3D = mesh_node.find_child("Buildings_Roofs", true, false)

	_check(walls_node != null, "Buildings_Walls 메시 노드가 생성되지 않음")
	_check(roofs_node != null, "Buildings_Roofs 메시 노드가 생성되지 않음")

	if walls_node != null and walls_node.mesh != null:
		var aabb: AABB = walls_node.mesh.get_aabb()
		_check(aabb.size.y >= 17.9, "벽면 메시 높이가 18m에 도달하지 않음 (실측: %f)" % aabb.size.y)
		_check(aabb.position.x <= -34.9 and (aabb.position.x + aabb.size.x) >= 39.9, "벽면 메시 수평 범위 불일치")

	if roofs_node != null and roofs_node.mesh != null:
		var aabb: AABB = roofs_node.mesh.get_aabb()
		_check(aabb.size.y <= 18.1, "지붕 메시 높이 이상")

	mesh_node.free()

## 시계·반시계 윤곽 모두에서 벽 앞면과 법선이 건물 밖을 향하는지 검사한다.
func _test_wall_winding() -> void:
	for reverse in [false, true]:
		var footprint := PackedVector3Array([Vector3(-2, 0, -2), Vector3(2, 0, -2), Vector3(2, 0, 2), Vector3(-2, 0, 2)])
		if reverse:
			footprint.reverse()
		var node := CourseMesh.new()
		root.add_child(node)
		node.build({"ok": true, "buildings": [{"footprint": footprint, "height_m": 6.0}]})
		var walls: MeshInstance3D = node.get_node("Buildings_Walls")
		var arrays := walls.mesh.surface_get_arrays(0)
		var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
		var normals: PackedVector3Array = arrays[Mesh.ARRAY_NORMAL]
		_check(vertices.size() == 24, "사각 건물의 네 벽이 모두 생성되어야 함")
		for i in range(0, vertices.size(), 3):
			var a := vertices[i]
			var b := vertices[i + 1]
			var c := vertices[i + 2]
			var center := (a + b + c) / 3.0
			var outward := Vector3(center.x, 0, center.z)
			_check((b - a).cross(c - a).dot(outward) < 0.0, "외벽 삼각형 앞면이 안쪽을 향함")
			_check(normals[i].dot(outward) > 0.0, "외벽 법선이 안쪽을 향함")
		node.free()

## 여수 OSM 코스의 건물 자료가 실제 메시로 생성되는지 검사한다.
func _test_real_course_buildings() -> void:
	var yeosu_course = SimCourse.build("map:testdata/yeosu-expo/course.toml")
	_check(yeosu_course.get("ok", false), "여수 엑스포 코스 로드 실패: " + str(yeosu_course.get("error", "")))
	var yeosu_buildings: Array = yeosu_course.get("buildings", [])
	_check(yeosu_buildings.size() > 0, "여수 엑스포 코스에 3D 건물이 생성되지 않음 (0개)")
	if yeosu_buildings.size() > 0:
		var mesh_node := CourseMesh.new()
		root.add_child(mesh_node)
		mesh_node.build(yeosu_course)
		var walls = mesh_node.find_child("Buildings_Walls", true, false)
		var roofs = mesh_node.find_child("Buildings_Roofs", true, false)
		_check(walls != null and roofs != null, "여수 엑스포 코스에서 3D 건물 메시 노드 생성 실패")
		mesh_node.free()

## 검사 실패 항목을 기록한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)

## 시험을 종료하고 결과를 출력한다.
func _finish() -> void:
	if failures.is_empty():
		print("BUILDING MESH OK: 3D 절차적 건물 벽면 및 지붕 메시 생성 정상")
		quit(0)
	else:
		for msg in failures:
			printerr("BUILDING MESH FAIL: " + msg)
		quit(1)
