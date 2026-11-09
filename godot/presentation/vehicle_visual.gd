extends Node3D
## 사진 기준 Bevy 최소 실내를 단순 도형으로 옮긴다. 모든 치수는 미확인 임시값이다.

const Meshes = preload("res://presentation/course_mesh.gd")

## 후륜축 원점에 외부 차체와 최소 실내를 조립한다.
func _ready() -> void:
	_box(self, Vector3(1.8, 2.2, 4.2), Vector3(0, 1.1, -1.4), Color("c9cbd0"), 2)
	_box(self, Vector3(1.7, 0.05, 4.1), Vector3(0, 0.325, -1.4), Color("202123"))
	for side in [-1.0, 1.0]:
		_box(self, Vector3(0.05, 0.8, 4.1), Vector3(side * 0.875, 0.75, -1.4), Color("4c4c4f"))
	for yaw in [0.0, PI]:
		var end := Node3D.new()
		end.position.z = -1.4
		end.rotation.y = yaw
		add_child(end)
		_window_end(end)

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
	if layer == 2:
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