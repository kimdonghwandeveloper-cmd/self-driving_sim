extends Button
## 도로의 기울기를 그대로 보여 주는 굵은 방향 화살표 버튼.
var direction := Vector2.RIGHT

## 둥근 테두리와 마우스·키보드 강조 및 읽기 쉬운 설명을 구성한다.
func _ready() -> void:
	custom_minimum_size = Vector2(140, 92)
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	tooltip_text = "이 화살표 방향으로 출발"
	for state in ["normal", "hover", "pressed", "focus"]:
		var box := StyleBoxFlat.new()
		box.bg_color = Color.WHITE if state == "normal" else Color("edf4ff")
		box.border_color = Color("dce3ec") if state == "normal" else Color("1262ef")
		box.set_border_width_all(1 if state == "normal" else 2)
		box.set_corner_radius_all(12)
		add_theme_stylebox_override(state, box)
	var label := Label.new()
	label.text = "이쪽으로 출발"
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE)
	label.offset_top = -29
	label.offset_bottom = -8
	label.add_theme_font_size_override("font_size", 14)
	label.add_theme_color_override("font_color", Color("344256"))
	label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(label)
	for signal_name in ["mouse_entered", "mouse_exited", "focus_entered", "focus_exited", "resized"]:
		connect(signal_name, queue_redraw)

## 화살표를 도로 접선 각도로 회전시켜 위·아래·대각선도 같은 규칙으로 표시한다.
func _draw() -> void:
	var shape := PackedVector2Array([Vector2(-23,-6),Vector2(5,-6),Vector2(5,-15),Vector2(25,0),Vector2(5,15),Vector2(5,6),Vector2(-23,6)])
	var center := Vector2(size.x * 0.5, 32)
	for i in shape.size(): shape[i] = center + shape[i].rotated(direction.angle())
	draw_colored_polygon(shape, Color("1262ef") if is_hovered() or has_focus() else Color("344256"))
