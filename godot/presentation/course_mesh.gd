extends Node3D
## Rust 코스 Dictionary를 표시용 메시로 조립한다. 물리 충돌체는 생성하지 않는다.
## 노면·연석은 절차 생성 잡음 무늬를 월드 좌표로 입혀(UV 없음) 아스팔트·콘크리트 질감을 낸다. 외부 애셋은 쓰지 않는다.

## 노면 무늬 한 장이 덮는 길이(m).
const ASPHALT_TILE_M := 6.0
## 연석·벽 무늬 한 장이 덮는 길이(m).
const CONCRETE_TILE_M := 2.0
const COLORS = {
	"road": Color("626a6d"), "edge_line": Color("f4e6ad"),
	"stop_line": Color("f5f4e8"), "curb": Color("c4c6bf"),
	"wall": Color("9eaaa7"), "barrier": Color("dbb35b"), "obstacle": Color("d37e57")
}
## 경로 끝 목표 표시: 체크무늬 띠 칸 수(가로, 세로)와 칸 크기(m), 안내 글자 높이(m).
const GOAL_CELLS := Vector2i(6, 2)
const GOAL_CELL_M := 0.6
const GOAL_LABEL_Y_M := 3.0

## 이전 메시를 제거하고 전달받은 코스 스냅샷만 그린다.
func build(data: Dictionary) -> void:
	for child in get_children():
		remove_child(child)
		child.queue_free()
	if not data.get("ok", false):
		return
	var asphalt := textured("road", Color("5b6164"), Color("72787b"), ASPHALT_TILE_M)
	var concrete := textured("curb", Color("b3b5ae"), Color("d2d3cc"), CONCRETE_TILE_M)
	var paint := {}
	for kind in ["edge_line", "stop_line"]:
		paint[kind] = material(COLORS[kind])
		paint[kind].roughness = 0.55
	for rect in data.get("surfaces", []):
		_box(rect, 0.012, 0.012, COLORS.road, asphalt)
	for strip in data.get("strips", []):
		var left: PackedVector3Array = strip.left
		var right: PackedVector3Array = strip.right
		var vertices := PackedVector3Array()
		for i in range(mini(left.size(), right.size()) - 1):
			vertices.append_array(PackedVector3Array([left[i], right[i], left[i + 1], right[i], right[i + 1], left[i + 1]]))
		var is_line: bool = strip.kind == "edge_line"
		_triangles(vertices, 0.016 if is_line else 0.012, paint.edge_line if is_line else asphalt)
	# 면은 모두 메시 하나로 합친다(도로 면 코스는 면이 천 개가 넘는다).
	var area_vertices := PackedVector3Array()
	for area in data.get("areas", []):
		var boundary: PackedVector3Array = area.boundary
		for i in range(boundary.size()):
			area_vertices.append_array(PackedVector3Array([area.center, boundary[i], boundary[(i + 1) % boundary.size()]]))
	_triangles(area_vertices, 0.012, asphalt)
	for marking in data.get("markings", []):
		_box(marking.rect, 0.004, 0.016, COLORS.stop_line, paint.get(marking.kind, paint.stop_line))
	_colliders(data.get("colliders", []), concrete)
	_buildings(data.get("buildings", []))
	var goal: Dictionary = data.get("goal", {})
	if not goal.is_empty():
		_goal_marker(goal)

## 3D 절차적 건물을 벽면과 지붕 메시로 배칭하여 생성한다 (PLAN 4-12).
func _buildings(buildings: Array) -> void:
	if buildings.is_empty():
		return
	var wall_tool := SurfaceTool.new()
	var roof_tool := SurfaceTool.new()
	wall_tool.begin(Mesh.PRIMITIVE_TRIANGLES)
	roof_tool.begin(Mesh.PRIMITIVE_TRIANGLES)

	var wall_count := 0
	var roof_count := 0

	for building in buildings:
		var height: float = building.get("height_m", 6.0)
		var footprint: PackedVector3Array = building.get("footprint", [])
		# 윤곽 방향을 통일하여 외벽 앞면과 법선이 바깥을 향하게 한다.
		var area := 0.0
		for i in footprint.size():
			var a := footprint[i]
			var b := footprint[(i + 1) % footprint.size()]
			area += a.x * b.z - b.x * a.z
		if area < 0.0:
			footprint.reverse()
		var n := footprint.size()
		if n < 3:
			continue

		# 1. 벽면 (수직 쿼드 및 UV 좌표 매핑)
		for i in range(n):
			var a: Vector3 = footprint[i]
			var b: Vector3 = footprint[(i + 1) % n]
			var a_top := a + Vector3(0, height, 0)
			var b_top := b + Vector3(0, height, 0)

			var edge := b - a
			var wall_len := edge.length()
			var normal := Vector3(edge.z, 0, -edge.x).normalized()

			# 두 개의 삼각형으로 벽면 사각형 구성
			var uvs := [
				Vector2(0, 0),
				Vector2(wall_len / 4.0, 0),
				Vector2(wall_len / 4.0, height / 3.0),
				Vector2(0, 0),
				Vector2(wall_len / 4.0, height / 3.0),
				Vector2(0, height / 3.0)
			]
			var wall_verts := [a, b, b_top, a, b_top, a_top]
			for idx in range(6):
				wall_tool.set_normal(normal)
				wall_tool.set_uv(uvs[idx])
				wall_tool.add_vertex(wall_verts[idx])
				wall_count += 1

		# 2. 지붕 (다각형 삼각분할)
		var roof_tris := _triangulate_roof(footprint, height)
		for v: Vector3 in roof_tris:
			roof_tool.set_normal(Vector3.UP)
			roof_tool.set_uv(Vector2(v.x / 10.0, v.z / 10.0))
			roof_tool.add_vertex(v)
			roof_count += 1

	if wall_count > 0:
		var wall_node := MeshInstance3D.new()
		wall_node.name = "Buildings_Walls"
		wall_node.mesh = wall_tool.commit()
		wall_node.material_override = _building_wall_material()
		add_child(wall_node)

	if roof_count > 0:
		var roof_node := MeshInstance3D.new()
		roof_node.name = "Buildings_Roofs"
		roof_node.mesh = roof_tool.commit()
		roof_node.material_override = material(Color("5f6368"))
		add_child(roof_node)

## 3D 건물 외벽용 콘크리트 무광 재질을 생성한다.
func _building_wall_material() -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color("b4b8b6")
	mat.roughness = 0.85
	mat.metallic = 0.05
	mat.specular_mode = StandardMaterial3D.SPECULAR_SCHLICK_GGX
	mat.shading_mode = StandardMaterial3D.SHADING_MODE_PER_PIXEL
	return mat

## 지붕 다각형을 2D 귀 잘라내기(Ear-clipping)로 삼각분할하여 꼭짓점 목록을 반환한다.
func _triangulate_roof(footprint: PackedVector3Array, height: float) -> PackedVector3Array:
	var n := footprint.size()
	if n < 3:
		return PackedVector3Array()
	var poly2d := PackedVector2Array()
	for p in footprint:
		poly2d.append(Vector2(p.x, p.z))
	var indices := Geometry2D.triangulate_polygon(poly2d)
	var result := PackedVector3Array()
	if indices.is_empty():
		for i in range(1, n - 1):
			result.append(Vector3(footprint[0].x, height, footprint[0].z))
			result.append(Vector3(footprint[i].x, height, footprint[i].z))
			result.append(Vector3(footprint[i + 1].x, height, footprint[i + 1].z))
		return result

	for idx in indices:
		var p: Vector3 = footprint[idx]
		result.append(Vector3(p.x, height, p.z))
	return result

## 충돌 경계를 종류마다 메시 하나로 합쳐 표시한다(4-6). 지도 경로 코스는 연석 상자가 수백 개라 상자마다
## 노드를 두면 그리기 호출이 그만큼 늘어난다. 연석은 `concrete` 재질, 나머지는 종류 색 무광 재질이다.
func _colliders(colliders: Array, concrete: Material) -> void:
	var tools := {}
	for collider in colliders:
		var height: float = collider.height_m
		var rect: Dictionary = collider.rect
		var box := BoxMesh.new()
		box.size = Vector3(rect.size.x, height, rect.size.y)
		if not tools.has(collider.kind):
			tools[collider.kind] = SurfaceTool.new()
			tools[collider.kind].begin(Mesh.PRIMITIVE_TRIANGLES)
		var place := Transform3D(Basis(Vector3.UP, rect.heading_rad), rect.center + Vector3(0, height * 0.5, 0))
		tools[collider.kind].append_from(box, 0, place)
	for kind in tools:
		var node := MeshInstance3D.new()
		node.name = "Colliders_" + kind
		node.mesh = tools[kind].commit()
		node.material_override = concrete if kind == "curb" else material(COLORS.get(kind, COLORS.obstacle))
		add_child(node)

## 경로 끝 목표 자세(계약 3절 `goal`)에 진행 방향을 가로지르는 체크무늬 띠와 "도착" 글자를 둔다. 충돌 판정은 없다.
func _goal_marker(goal: Dictionary) -> void:
	var marker := Node3D.new()
	marker.name = "GoalMarker"
	marker.position = goal.position
	marker.rotation.y = goal.heading_rad
	add_child(marker)
	var cells := {true: material(Color("f5f4e8")), false: material(Color("2b2e30"))}
	var mesh := BoxMesh.new()
	mesh.size = Vector3(GOAL_CELL_M, 0.004, GOAL_CELL_M)
	for column in GOAL_CELLS.x:
		for row in GOAL_CELLS.y:
			var cell := MeshInstance3D.new()
			cell.mesh = mesh
			cell.material_override = cells[(column + row) % 2 == 0]
			cell.position = Vector3((column - (GOAL_CELLS.x - 1) * 0.5) * GOAL_CELL_M, 0.018, (row - (GOAL_CELLS.y - 1) * 0.5) * GOAL_CELL_M)
			marker.add_child(cell)
	var label := Label3D.new()
	label.text = "도착"
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.font_size = 96
	label.pixel_size = 0.01
	label.outline_size = 18
	label.modulate = Color("7ee08a")
	label.position = Vector3(0, GOAL_LABEL_Y_M, 0)
	marker.add_child(label)

## 지면 직사각형을 주어진 전체 높이의 상자로 표시한다. `surface`가 없으면 `color` 무광 재질을 쓴다.
func _box(rect: Dictionary, height: float, center_y: float, color: Color, surface: Material = null) -> void:
	var mesh := BoxMesh.new()
	mesh.size = Vector3(rect.size.x, height, rect.size.y)
	var node := MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = surface if surface != null else material(color)
	node.position = rect.center + Vector3(0, center_y, 0)
	node.rotation.y = rect.heading_rad
	add_child(node)

## 지면 삼각형을 위쪽 법선의 양면 메시로 표시한다.
func _triangles(vertices: PackedVector3Array, y: float, surface_material: Material) -> void:
	if vertices.is_empty():
		return
	var surface := SurfaceTool.new()
	surface.begin(Mesh.PRIMITIVE_TRIANGLES)
	var count := 0
	for i in range(0, vertices.size(), 3):
		var a := Vector3(vertices[i].x, y, vertices[i].z)
		var b := Vector3(vertices[i + 1].x, y, vertices[i + 1].z)
		var c := Vector3(vertices[i + 2].x, y, vertices[i + 2].z)
		var normal := (b - a).cross(c - a)
		if normal.length_squared() == 0.0:
			continue
		# Godot 앞면은 위에서 볼 때 시계 방향(법선 아래)이다. 앞면이 위를 보게 맞춰야 뒷면 조명(뒤집힌 법선)으로 어두워지지 않는다.
		var ordered := [a, b, c] if normal.y < 0.0 else [a, c, b]
		for point in ordered:
			surface.set_normal(Vector3.UP)
			surface.add_vertex(point)
			count += 1
	if count == 0:
		return
	var node := MeshInstance3D.new()
	node.mesh = surface.commit()
	node.material_override = surface_material
	add_child(node)

## 단순 무광 재질을 만든다. 지면은 위·아래 양쪽에서 보이게 한다.
static func material(color: Color) -> StandardMaterial3D:
	var result := StandardMaterial3D.new()
	result.albedo_color = color
	result.roughness = 0.9
	result.cull_mode = BaseMaterial3D.CULL_DISABLED
	return result

## 두 색 사이를 오가는 잡음 무늬 재질을 만든다. 월드 좌표 투영(triplanar)이라 메시에 UV가 없어도 되고,
## `tile_m`마다 무늬가 반복된다. 같은 `seed_name`이면 같은 무늬다.
static func textured(seed_name: String, dark: Color, light: Color, tile_m: float) -> StandardMaterial3D:
	var noise := FastNoiseLite.new()
	noise.seed = hash(seed_name)
	noise.noise_type = FastNoiseLite.TYPE_SIMPLEX_SMOOTH
	noise.frequency = 0.08
	noise.fractal_octaves = 4
	var ramp := Gradient.new()
	ramp.set_color(0, dark)
	ramp.set_color(1, light)
	var texture := NoiseTexture2D.new()
	texture.width = 256
	texture.height = 256
	texture.seamless = true
	texture.noise = noise
	texture.color_ramp = ramp
	var result := material(Color.WHITE)
	result.albedo_texture = texture
	result.uv1_triplanar = true
	result.uv1_world_triplanar = true
	result.uv1_scale = Vector3.ONE / tile_m
	result.roughness = 0.95
	return result

## 코스 사전의 노면·띠·면·경계가 차지하는 수평 범위(x, z)를 구한다. 비어 있으면 원점 크기 0.
static func ground_bounds(data: Dictionary) -> Rect2:
	var points := PackedVector3Array()
	for rect in data.get("surfaces", []):
		points.append(rect.center)
	for strip in data.get("strips", []):
		points.append_array(strip.left)
		points.append_array(strip.right)
	for area in data.get("areas", []):
		points.append_array(area.boundary)
	for collider in data.get("colliders", []):
		points.append(collider.rect.center)
	if points.is_empty():
		return Rect2()
	var bounds := Rect2(Vector2(points[0].x, points[0].z), Vector2.ZERO)
	for point in points:
		bounds = bounds.expand(Vector2(point.x, point.z))
	return bounds
