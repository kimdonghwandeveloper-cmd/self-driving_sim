extends RefCounted
## 도로·건물·배경이 공유하는 코스 주변 지면 격자.

const CLEARANCE_M := 0.10
const BASE_COLOR := Color("7d906a")
var _bounds: Rect2
var _step: Vector2
var _cells: Vector2i
var _heights := PackedFloat32Array()

## 코스 범위에 맞춰 정점 수를 제한하고 고도 타일을 표본화한다.
func _init(bounds: Rect2, sample_height: Callable) -> void:
	_bounds = bounds
	_cells = Vector2i(clampi(ceili(bounds.size.x / 5.0), 1, 192), clampi(ceili(bounds.size.y / 5.0), 1, 192))
	_step = bounds.size / Vector2(_cells)
	_heights.resize((_cells.x + 1) * (_cells.y + 1))
	for z in range(_cells.y + 1):
		for x in range(_cells.x + 1):
			var p := _bounds.position + Vector2(x, z) * _step
			_heights[_index(x, z)] = float(sample_height.call(p.x, p.y))

## 겹칠 수 있는 지면 셀의 네 꼭짓점을 도로 삼각형 최저점 아래로 내린다.
func clear_road(a: Vector3, b: Vector3, c: Vector3) -> void:
	var first := Vector2(minf(a.x, minf(b.x, c.x)), minf(a.z, minf(b.z, c.z)))
	var last := Vector2(maxf(a.x, maxf(b.x, c.x)), maxf(a.z, maxf(b.z, c.z)))
	var start := _cell(first)
	var finish := _cell(last) + Vector2i.ONE
	var ceiling := minf(a.y, minf(b.y, c.y)) - CLEARANCE_M
	for z in range(start.y, finish.y + 1):
		for x in range(start.x, finish.x + 1):
			var index := _index(x, z)
			_heights[index] = minf(_heights[index], ceiling)

## 건물 기초를 실제 지면 메시와 같은 두 삼각형에서 보간한다.
func height_at(x: float, z: float) -> float:
	var position := Vector2(x, z)
	var cell := _cell(position)
	var t := ((position - _bounds.position) / _step - Vector2(cell)).clamp(Vector2.ZERO, Vector2.ONE)
	var a := _heights[_index(cell.x, cell.y)]
	var b := _heights[_index(cell.x + 1, cell.y)]
	var c := _heights[_index(cell.x, cell.y + 1)]
	var d := _heights[_index(cell.x + 1, cell.y + 1)]
	if t.x + t.y <= 1.0:
		return a + (b - a) * t.x + (c - a) * t.y
	return d + (c - d) * (1.0 - t.x) + (b - d) * (1.0 - t.y)

## 건물 아래의 지면 정점 최고점을 보수적으로 잡아 지붕이 묻히지 않게 한다.
func foundation_top(footprint: PackedVector3Array) -> float:
	var bounds := Rect2(Vector2(footprint[0].x, footprint[0].z), Vector2.ZERO)
	for point in footprint:
		bounds = bounds.expand(Vector2(point.x, point.z))
	var start := _cell(bounds.position)
	var finish := _cell(bounds.end) + Vector2i.ONE
	var top := -INF
	for z in range(start.y, finish.y + 1):
		for x in range(start.x, finish.x + 1):
			top = maxf(top, _heights[_index(x, z)])
	return top

## 별도 배경 면 대신 지면의 셀 색으로 공원·주차장·물을 표시한다.
func build_mesh(scenery: Dictionary) -> ArrayMesh:
	var colors := PackedColorArray()
	colors.resize(_cells.x * _cells.y)
	colors.fill(BASE_COLOR)
	for layer in [["green", Color("73a462")], ["parking", Color("777d84")], ["water", Color("579bad")]]:
		var vertices: PackedVector3Array = scenery.get(layer[0], PackedVector3Array())
		for i in range(0, vertices.size(), 3):
			var a := Vector2(vertices[i].x, vertices[i].z)
			var b := Vector2(vertices[i + 1].x, vertices[i + 1].z)
			var c := Vector2(vertices[i + 2].x, vertices[i + 2].z)
			var start := _cell(a.min(b).min(c))
			var finish := _cell(a.max(b).max(c))
			var triangle := PackedVector2Array([a, b, c])
			for z in range(start.y, finish.y + 1):
				for x in range(start.x, finish.x + 1):
					var center := _bounds.position + (Vector2(x, z) + Vector2(0.5, 0.5)) * _step
					if Geometry2D.is_point_in_polygon(center, triangle):
						colors[z * _cells.x + x] = layer[1]
	var surface := SurfaceTool.new()
	surface.begin(Mesh.PRIMITIVE_TRIANGLES)
	for z in range(_cells.y):
		for x in range(_cells.x):
			var a := _point(x, z)
			var b := _point(x + 1, z)
			var c := _point(x, z + 1)
			var d := _point(x + 1, z + 1)
			var color := colors[z * _cells.x + x]
			_emit_triangle(surface, a, b, c, color)
			_emit_triangle(surface, b, d, c, color)
	return surface.commit()

## 위쪽을 향하는 법선과 재질 색을 각 정점에 기록한다.
func _emit_triangle(surface: SurfaceTool, a: Vector3, b: Vector3, c: Vector3, color: Color) -> void:
	var normal := (c - a).cross(b - a).normalized()
	for vertex in [a, b, c]:
		surface.set_normal(normal)
		surface.set_color(color)
		surface.add_vertex(vertex)

## 좌표에서 지면 셀을 찾고 메시 밖은 가장자리로 제한한다.
func _cell(position: Vector2) -> Vector2i:
	var p := (position - _bounds.position) / _step
	return Vector2i(clampi(floori(p.x), 0, _cells.x - 1), clampi(floori(p.y), 0, _cells.y - 1))

## 행·열을 정점 배열의 인덱스로 바꾼다.
func _index(x: int, z: int) -> int:
	return z * (_cells.x + 1) + x

## 저장된 표고와 격자 좌표로 메시 정점을 반환한다.
func _point(x: int, z: int) -> Vector3:
	var p := _bounds.position + Vector2(x, z) * _step
	return Vector3(p.x, _heights[_index(x, z)], p.y)
