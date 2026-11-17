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
