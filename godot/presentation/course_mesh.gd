extends Node3D
## Rust 코스 Dictionary를 표시용 메시로 조립한다. 물리 충돌체는 생성하지 않는다.

const COLORS = {
	"road": Color("626a6d"), "edge_line": Color("f4e6ad"),
	"stop_line": Color("f5f4e8"), "curb": Color("c4c6bf"),
	"wall": Color("9eaaa7"), "barrier": Color("dbb35b"), "obstacle": Color("d37e57")
}

## 이전 메시를 제거하고 전달받은 코스 스냅샷만 그린다.
func build(data: Dictionary) -> void:
	for child in get_children():
		remove_child(child)
		child.queue_free()
	if not data.get("ok", false):
		return
	for rect in data.get("surfaces", []):
		_box(rect, 0.012, 0.012, COLORS.road)
	for strip in data.get("strips", []):
		var left: PackedVector3Array = strip.left
		var right: PackedVector3Array = strip.right
		var vertices := PackedVector3Array()
		for i in range(mini(left.size(), right.size()) - 1):
			vertices.append_array(PackedVector3Array([left[i], right[i], left[i + 1], right[i], right[i + 1], left[i + 1]]))
		_triangles(vertices, 0.016 if strip.kind == "edge_line" else 0.012, COLORS.get(strip.kind, COLORS.road))
	for area in data.get("areas", []):
		var boundary: PackedVector3Array = area.boundary
		var vertices := PackedVector3Array()
		for i in range(boundary.size()):
			vertices.append_array(PackedVector3Array([area.center, boundary[i], boundary[(i + 1) % boundary.size()]]))
		_triangles(vertices, 0.012, COLORS.road)
	for marking in data.get("markings", []):
		_box(marking.rect, 0.004, 0.016, COLORS.get(marking.kind, COLORS.stop_line))
	for collider in data.get("colliders", []):
		var height: float = collider.height_m
		_box(collider.rect, height, height * 0.5, COLORS.get(collider.kind, COLORS.obstacle))

## 지면 직사각형을 주어진 전체 높이의 상자로 표시한다.
func _box(rect: Dictionary, height: float, center_y: float, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = Vector3(rect.size.x, height, rect.size.y)
	var node := MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = material(color)
	node.position = rect.center + Vector3(0, center_y, 0)
	node.rotation.y = rect.heading_rad
	add_child(node)

## 지면 삼각형을 위쪽 법선의 양면 메시로 표시한다.
func _triangles(vertices: PackedVector3Array, y: float, color: Color) -> void:
	if vertices.is_empty():
		return
	var surface := SurfaceTool.new()
	surface.begin(Mesh.PRIMITIVE_TRIANGLES)
	for point in vertices:
		surface.set_normal(Vector3.UP)
		surface.add_vertex(Vector3(point.x, y, point.z))
	var node := MeshInstance3D.new()
	node.mesh = surface.commit()
	node.material_override = material(color)
	add_child(node)

## 단순 무광 재질을 만든다. 지면은 위·아래 양쪽에서 보이게 한다.
static func material(color: Color) -> StandardMaterial3D:
	var result := StandardMaterial3D.new()
	result.albedo_color = color
	result.roughness = 0.9
	result.cull_mode = BaseMaterial3D.CULL_DISABLED
	return result