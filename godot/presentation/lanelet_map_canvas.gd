extends Control
## 지역 좌표의 차선·방향·선택 경로를 표시하고 화면 좌표로 차선을 고른다.

signal lane_clicked(id: int)
var lanes: Array = []
var selected_start := 0
var selected_goal := 0
var preview: Dictionary = {}
var bounds := Rect2()
var zoom := 1.0
var pan := Vector2.ZERO
var dragging := false

## 지도만 입력을 받고 크기가 바뀌면 같은 좌표 변환으로 다시 그린다.
func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	clip_contents = true
	resized.connect(queue_redraw)

## 전체 차선 범위를 계산하고 이전 지도 선택·확대 상태를 폐기한다.
func configure(data: Array) -> void:
	lanes = data
	selected_start = 0
	selected_goal = 0
	preview = {}
	fit()

## 현재 차선 전체를 화면에 맞추고 드래그 이동을 초기화한다.
func fit() -> void:
	var first := true
	for lane in lanes:
		for key in ["left", "right"]:
			for point in lane[key]:
				if first:
					bounds = Rect2(point, Vector2.ZERO)
					first = false
				else:
					bounds = bounds.expand(point)
	bounds = bounds.grow(4.0)
	zoom = 1.0
	pan = Vector2.ZERO
	queue_redraw()

## 동일 축척으로 x=동쪽, y=남쪽 좌표를 화면에 투영한다.
func map_point(point: Vector2) -> Vector2:
	var available := (size - Vector2(48, 48)).max(Vector2.ONE)
	var scale_factor := minf(available.x / maxf(bounds.size.x, 1.0), available.y / maxf(bounds.size.y, 1.0)) * zoom
	return size * 0.5 + pan + (point - bounds.get_center()) * scale_factor

## 지면 점 배열을 현재 확대·이동 상태의 화면 점으로 옮긴다.
func screen_points(points: PackedVector2Array) -> PackedVector2Array:
	var result := PackedVector2Array()
	for point in points:
		result.append(map_point(point))
	return result

## 경계 내부 차선을 우선 선택하고 빈 곳은 선택하지 않는다. 겹치면 중심선 거리가 가까운 차선을 고른다.
func pick(point: Vector2) -> int:
	var best := 0
	var best_distance := INF
	for lane in lanes:
		var polygon := screen_points(lane.left)
		var right := screen_points(lane.right)
		right.reverse()
		polygon.append_array(right)
		if not Geometry2D.is_point_in_polygon(point, polygon):
			continue
		var center := screen_points(lane.center)
		for i in range(center.size() - 1):
			var distance := point.distance_to(Geometry2D.get_closest_point_to_segment(point, center[i], center[i + 1]))
			if distance < best_distance:
				best_distance = distance
				best = int(lane.id)
	return best

## 휠로 확대하고 가운데·오른쪽 버튼으로 이동하며 왼쪽 클릭으로 차선을 선택한다.
func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index in [MOUSE_BUTTON_MIDDLE, MOUSE_BUTTON_RIGHT]:
			dragging = event.pressed
		elif event.pressed and event.button_index in [MOUSE_BUTTON_WHEEL_UP, MOUSE_BUTTON_WHEEL_DOWN]:
			var old := zoom
			zoom = clampf(zoom * (1.2 if event.button_index == MOUSE_BUTTON_WHEEL_UP else 1.0 / 1.2), 0.5, 12.0)
			pan = event.position - size * 0.5 - (event.position - size * 0.5 - pan) * (zoom / old)
		elif event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
			lane_clicked.emit(pick(event.position))
		queue_redraw()
		accept_event()
	elif event is InputEventMouseMotion and dragging:
		pan += event.relative
		queue_redraw()
		accept_event()

## 노면·경계·진행 방향 화살표와 검증된 안내선을 구분해 그린다.
func _draw() -> void:
	draw_rect(Rect2(Vector2.ZERO, size), Color("0b1724"))
	for lane in lanes:
		var left := screen_points(lane.left)
		var right := screen_points(lane.right)
		var polygon := left.duplicate()
		var reverse := right.duplicate()
		reverse.reverse()
		polygon.append_array(reverse)
		var color := Color("294354")
		if preview.get("ids", PackedInt64Array()).has(int(lane.id)):
			color = Color("28665d")
		if int(lane.id) == selected_start:
			color = Color("276b91")
		if int(lane.id) == selected_goal:
			color = Color("8a6b30")
		draw_colored_polygon(polygon, color)
		draw_polyline(left, Color("87a2ae"), 1.5, true)
		draw_polyline(right, Color("87a2ae"), 1.5, true)
		var center := screen_points(lane.center)
		var mid := maxi(0, center.size() / 2 - 1)
		var direction := (center[mid + 1] - center[mid]).normalized()
		var tip := center[mid] + direction * 8
		draw_line(center[mid] - direction * 6, tip, Color("a6c4ce"), 2, true)
		draw_line(tip, tip - direction.rotated(0.6) * 7, Color("a6c4ce"), 2, true)
		draw_line(tip, tip - direction.rotated(-0.6) * 7, Color("a6c4ce"), 2, true)
		draw_string(ThemeDB.fallback_font, center[mid] + Vector2(10, -6), str(lane.id), HORIZONTAL_ALIGNMENT_LEFT, -1, 13, Color.WHITE)
	if preview.get("ok", false):
		draw_polyline(screen_points(preview.guide), Color("65e4ce"), 3, true)
		_marker(preview.start, Color("69c8ff"), "출발")
		_marker(preview.goal, Color("ffd27a"), "도착")
	draw_string(ThemeDB.fallback_font, Vector2(16, 24), "북 ↑  · 휠 확대 · 오른쪽 드래그 이동", HORIZONTAL_ALIGNMENT_LEFT, -1, 14, Color("adc1d0"))

## 실제 코스 시작·목표 자세를 차선 클릭 위치와 구분해 표시한다.
func _marker(point: Vector2, color: Color, title: String) -> void:
	var position_on_map := map_point(point)
	draw_circle(position_on_map, 6, color)
	draw_string(ThemeDB.fallback_font, position_on_map + Vector2(10, 18), title, HORIZONTAL_ALIGNMENT_LEFT, -1, 15, color)
