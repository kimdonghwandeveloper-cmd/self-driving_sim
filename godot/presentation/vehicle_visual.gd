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
## 셔틀 모델에서 바퀴로 보는 부품 이름 머리. 차체가 기울어도 이 부품은 노면에 남는다(2-8).
const WHEEL_PREFIXES := ["tire", "tread", "wheel_hub", "wheel_lug", "hub_ridge"]
## 차체가 기울 때 도는 점(차량 로컬, m). 두 차축 가운데, 롤 중심 높이로 본 임시값.
const BODY_PIVOT := Vector3(0, 0.4, -1.4)

## 서스펜션(2-8)에 따라 기우는 차체(외관·실내). 바퀴 부품은 넣지 않는다.
var body := Node3D.new()
## 기울지 않는 바퀴 부품.
var wheels := Node3D.new()

## 후륜축 원점에 외부 차체와 최소 실내를 조립한다. 차체는 `body`, 셔틀 바퀴 부품은 `wheels` 아래에 둔다.
func _ready() -> void:
	body.name = "Body"
	wheels.name = "Wheels"
	add_child(body)
	add_child(wheels)
	if not _add_shuttle():
		_box(body, Vector3(1.8, 2.2, 4.2), Vector3(0, 1.1, -1.4), Color("c9cbd0"), EXTERIOR_LAYER)
	_box(body, Vector3(1.7, 0.05, 4.1), Vector3(0, 0.325, -1.4), Color("202123"))
	for side in [-1.0, 1.0]:
		_box(body, Vector3(0.05, 0.8, 4.1), Vector3(side * 0.875, 0.75, -1.4), Color("4c4c4f"))
	for yaw in [0.0, PI]:
		var end := Node3D.new()
		end.position.z = -1.4
		end.rotation.y = yaw
		body.add_child(end)
		_window_end(end)

## 차체 움직임 사전(`SimVehicle.body_pose`, 계약 v1.9)을 `follow` 비율(0..=1)만큼 적용한 변환을 만든다.
## 피치(앞이 내려가면 양수)는 X축, 롤(오른쪽이 내려가면 양수)은 Z축으로 [constant BODY_PIVOT]을 중심으로 돌리고 상하만큼 올린다.
static func pose_transform(pose: Dictionary, follow: float) -> Transform3D:
	var pitch := float(pose.get("pitch_rad", 0.0)) * follow
	var roll := float(pose.get("roll_rad", 0.0)) * follow
	var heave := float(pose.get("heave_m", 0.0)) * follow
	var turn := Basis(Vector3.RIGHT, -pitch) * Basis(Vector3.BACK, -roll)
	return Transform3D(Basis.IDENTITY, BODY_PIVOT + Vector3.UP * heave) * Transform3D(turn, Vector3.ZERO) * Transform3D(Basis.IDENTITY, -BODY_PIVOT)

## 차체만 기울인다. 바퀴 부품은 그대로 둔다.
func set_body_pose(pose: Dictionary) -> void:
	body.transform = pose_transform(pose, 1.0)

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
	body.add_child(model)
	_to_exterior_layer(model)
	_split_wheels(model)
	return true

## 모델에서 바퀴 부품([constant WHEEL_PREFIXES])을 `wheels` 아래로 옮긴다. 옮긴 뒤에도 보이는 자리는 같다.
func _split_wheels(model: Node3D) -> void:
	var parts: Array[Node3D] = []
	_collect_wheels(model, parts)
	for part in parts:
		part.reparent(wheels, true)

## `node` 아래에서 이름이 바퀴 부품 머리로 시작하는 표시 노드를 모은다.
func _collect_wheels(node: Node, parts: Array[Node3D]) -> void:
	for child in node.get_children():
		if child is Node3D and WHEEL_PREFIXES.any(func(prefix): return str(child.name).begins_with(prefix)):
			parts.append(child)
		else:
			_collect_wheels(child, parts)

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