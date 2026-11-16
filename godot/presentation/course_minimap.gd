extends Control
## 코스 형상·차량·목표를 북쪽 위로 그리는 오프라인 미니맵. 경로 계산은 하지 않는다.

var lines: Array[PackedVector2Array] = []
var bounds := Rect2(Vector2(-10, -10), Vector2(20, 20))
var vehicle_position := Vector2.ZERO
var heading := 0.0
var goal := Vector2.ZERO
var has_goal := false


## 마우스 입력은 상위 버튼에 맡기고 크기가 달라지면 다시 그린다.
func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	resized.connect(queue_redraw)


## 도로 띠·교차로·사각 노면에서 표시 선을 한 번 추출한다.
func configure(data: Dictionary) -> void:
	lines.clear()
	for strip in data.get("strips", []):
		if strip.get("kind", "road") != "road":
			continue
		for side in ["left", "right"]:
			var line := PackedVector2Array()
			for point: Vector3 in strip[side]:
				line.append(Vector2(point.x, point.z))
			if line.size() > 1:
				lines.append(line)
	for area in data.get("areas", []):
		var line := PackedVector2Array()
		for point: Vector3 in area.boundary:
			line.append(Vector2(point.x, point.z))
		if line.size() > 2:
			line.append(line[0])
			lines.append(line)
	for rect in data.get("surfaces", []):
		var line := PackedVector2Array()
		var center: Vector3 = rect.center
		var half: Vector2 = rect.size * 0.5
		for corner in [Vector2(-half.x, -half.y), Vector2(half.x, -half.y), Vector2(half.x, half.y), Vector2(-half.x, half.y), Vector2(-half.x, -half.y)]:
			line.append(Vector2(center.x, center.z) + corner.rotated(-float(rect.heading_rad)))
		lines.append(line)
	var start: Vector3 = data.get("start", {}).get("position", Vector3.ZERO)
	vehicle_position = Vector2(start.x, start.z)
	bounds = Rect2(vehicle_position, Vector2.ZERO)
	for line in lines:
		for point in line:
			bounds = bounds.expand(point)
	var target: Dictionary = data.get("goal", {})
	has_goal = not target.is_empty()
	if has_goal:
		var point: Vector3 = target.position
		goal = Vector2(point.x, point.z)
		bounds = bounds.expand(goal)
	bounds = bounds.grow(3.0)
	queue_redraw()


## 실제 차량 자세가 달라질 때만 표시를 갱신한다.
func update_vehicle(position_3d: Vector3, heading_rad: float) -> void:
	var point := Vector2(position_3d.x, position_3d.z)
	if point != vehicle_position or heading_rad != heading:
		vehicle_position = point
		heading = heading_rad
		queue_redraw()


## 코스 전체가 작은 지도 안에 들어오는 등방 배율로 좌표를 바꾼다.
func map_point(point: Vector2) -> Vector2:
	var usable := (size - Vector2(24, 32)).max(Vector2.ONE)
	var scale_m := minf(usable.x / maxf(bounds.size.x, 1.0), usable.y / maxf(bounds.size.y, 1.0))
	return (point - bounds.get_center()) * scale_m + size * 0.5 + Vector2(0, 6)


## 도로 윤곽, 목표와 진행 방향 화살표를 그린다. 배경 지도 타일은 사용하지 않는다.
func _draw() -> void:
	draw_style_box(_background(), Rect2(Vector2.ZERO, size))
	for line in lines:
		var screen := PackedVector2Array()
		for point in line:
			screen.append(map_point(point))
		draw_polyline(screen, Color("8aa5b8"), 1.5, true)
	if has_goal:
		draw_circle(map_point(goal), 4.0, Color("f5cf7b"))
	var point := map_point(vehicle_position).clamp(Vector2(10, 24), size - Vector2(10, 10))
	var arrow := PackedVector2Array()
	for vertex in [Vector2(0, -7), Vector2(5, 5), Vector2(0, 2), Vector2(-5, 5)]:
		arrow.append(point + vertex.rotated(-heading))
	draw_colored_polygon(arrow, Color("72e5ce"))
	draw_string(ThemeDB.fallback_font, Vector2(8, 15), "코스 · 북 ↑", HORIZONTAL_ALIGNMENT_LEFT, -1, 10, Color("adc1d0"))


## 미니맵 바탕에만 쓰는 작은 반투명 패널 스타일을 만든다.
func _background() -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = Color("0c1722")
	style.set_corner_radius_all(8)
	return style
