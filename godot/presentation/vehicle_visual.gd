extends Node3D
## 사진 기준 Bevy 최소 실내를 단순 도형으로 옮긴다. 모든 치수는 미확인 임시값이다.

const Meshes = preload("res://presentation/course_mesh.gd")
## 사용자 제작 셔틀 외관(임시). 충돌 차체(폭 1.8 m, 길이 4.2 m)와 크기가 달라 외관 표시에만 쓴다.
const SHUTTLE_PATH := "res://assets/vehicle/autonomous_shuttle_topology_v4.glb"
## 모델은 Z축 위·앞쪽 -Y로 저장돼 있어 차량 로컬(+Y 위, -Z 앞)로 돌린다: X = -x, Y = z, Z = y.
const SHUTTLE_ROTATION_DEG := Vector3(-90, 180, 0)
## 모델 원점은 두 차축 가운데(축간 2.54 m)라 뒷바퀴 축이 차량 원점에 오도록 앞으로 1.27 m 옮긴다.
const SHUTTLE_OFFSET := Vector3(0, 0, -1.27)
## 외부 카메라만 보는 외관 시각 레이어.
const EXTERIOR_LAYER := 2

## 후륜축 원점에 외부 차체와 최소 실내를 조립한다.
func _ready() -> void:
	if not _add_shuttle():
		_box(self, Vector3(1.8, 2.2, 4.2), Vector3(0, 1.1, -1.4), Color("c9cbd0"), EXTERIOR_LAYER)
	_box(self, Vector3(1.7, 0.05, 4.1), Vector3(0, 0.325, -1.4), Color("202123"))
	for side in [-1.0, 1.0]:
		_box(self, Vector3(0.05, 0.8, 4.1), Vector3(side * 0.875, 0.75, -1.4), Color("4c4c4f"))
	for yaw in [0.0, PI]:
		var end := Node3D.new()
		end.position.z = -1.4
		end.rotation.y = yaw
		add_child(end)
		_window_end(end)

## 셔틀 모델이 있으면 외관 레이어에 붙이고 `true`를 돌려준다. 파일이 없거나 읽지 못하면 `false`(상자 외관을 쓴다).
func _add_shuttle() -> bool:
	if not ResourceLoader.exists(SHUTTLE_PATH):
		return false
	var scene := load(SHUTTLE_PATH) as PackedScene
	if scene == null:
		push_warning("셔틀 외관을 읽지 못해 상자 외관을 씁니다: " + SHUTTLE_PATH)
		return false
	var model := scene.instantiate() as Node3D
	model.name = "ShuttleExterior"
	model.rotation_degrees = SHUTTLE_ROTATION_DEG
	model.position = SHUTTLE_OFFSET
	add_child(model)
	_to_exterior_layer(model)
	return true

## 모델 안의 모든 표시 노드를 외관 레이어로 옮기고 그림자를 끈다(입석 카메라에 보이지 않게).
func _to_exterior_layer(node: Node) -> void:
	if node is VisualInstance3D:
		node.layers = EXTERIOR_LAYER
	if node is GeometryInstance3D:
		node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	for child in node.get_children():
		_to_exterior_layer(child)

## 창 아래 선반·끝벽·기운 기둥·원형 개구부를 한쪽 끝에 배치한다.
func _window_end(parent: Node3D) -> void:
	_box(parent, Vector3(1.66, 1.03, 0.05), Vector3(0, 0.865, -2.0), Color("4c4c4f"))
	_box(parent, Vector3(1.66, 0.12, 0.6), Vector3(0, 1.32, -1.7), Color("666666"))
	_box(parent, Vector3(1.66, 0.04, 0.03), Vector3(0, 1.4, -2.0), Color("080808"))
	for side in [-1.0, 1.0]:
		var bottom := Vector2(side * 0.53, 1.33)
		var top := Vector2(side * 0.76, 2.12)
		var along := (top - bottom).normalized()
		var outward := Vector2(side * along.y, -side * along.x)
		var center := (bottom + top) * 0.5 + outward * 0.07
		var pillar := _box(parent, Vector3(0.14, bottom.distance_to(top), 0.4), Vector3(center.x, center.y, -1.8), Color("19191c"))
		pillar.rotation.z = atan2(-along.x, along.y)
		_box(parent, Vector3(0.16, 0.9, 0.3), Vector3(side * 0.8, 1.4, -1.8), Color("6b6b6b"))
		_disc(parent, Vector3(side * 0.62, 1.385, -1.6), 0.08)
	for x in [-0.3, 0.0, 0.3]:
		_disc(parent, Vector3(x, 1.385, -1.92), 0.035)

## 무광 상자를 추가하고 외부 차체만 별도 시각 레이어에 둔다.
func _box(parent: Node3D, size: Vector3, center: Vector3, color: Color, layer: int = 1) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	var box := BoxMesh.new()
	box.size = size
	node.mesh = box
	node.position = center
	node.layers = layer
	if layer == EXTERIOR_LAYER:
		node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	node.material_override = Meshes.material(color)
	parent.add_child(node)
	return node

## 선반 위 스피커와 통풍구를 얇은 원판으로 표시한다.
func _disc(parent: Node3D, center: Vector3, radius: float) -> void:
	var node := MeshInstance3D.new()
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = 0.01
	node.mesh = mesh
	node.position = center
	node.material_override = Meshes.material(Color("101012"))
	parent.add_child(node)