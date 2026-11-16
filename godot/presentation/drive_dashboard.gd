extends Control
## 작은 계기판·코스 미니맵과 접이식 상세 정보. 차량 입력·물리는 변경하지 않는다.

const MiniMap = preload("res://presentation/course_minimap.gd")
const Dial = preload("res://presentation/speed_dial.gd")
var speed := Label.new()
var gears := Label.new()
var state := Label.new()
var metrics := Label.new()
var panel := PanelContainer.new()
var minimap := MiniMap.new()
var large_map := MiniMap.new()
var dial := Dial.new()
var details := PanelContainer.new()
var map_panel := PanelContainer.new()
var info_column := VBoxContainer.new()
var detail_text := Label.new()
var alert := Label.new()
var alert_panel := PanelContainer.new()
var attribution := Label.new()
var toggle_info := Button.new()
var toggle_map := Button.new()
var toggle_hud := Button.new()
var _visibility_tween: Tween


## 하단 중앙 계기판과 우측 접이식 지도·정보창을 배치한다.
func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(panel)
	panel.add_theme_stylebox_override("panel", _style())
	panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 18)
	panel.add_child(row)
	dial.custom_minimum_size = Vector2(118, 108)
	dial.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(dial)
	dial.add_child(speed)
	speed.set_anchors_and_offsets_preset(Control.PRESET_CENTER)
	speed.offset_left = -52
	speed.offset_right = 52
	speed.offset_top = -12
	speed.offset_bottom = 16
	speed.text = "0 km/h"
	speed.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	speed.add_theme_font_size_override("font_size", 22)
	minimap.custom_minimum_size = Vector2(150, 108)
	minimap.clip_contents = true
	row.add_child(minimap)
	var status_column := VBoxContainer.new()
	status_column.custom_minimum_size.x = 140
	status_column.add_theme_constant_override("separation", 6)
	row.add_child(status_column)
	gears.add_theme_font_size_override("font_size", 28)
	gears.add_theme_color_override("font_color", Color("72e5ce"))
	status_column.add_child(gears)
	state.add_theme_font_size_override("font_size", 12)
	state.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	state.max_lines_visible = 2
	status_column.add_child(state)
	var buttons := HBoxContainer.new()
	status_column.add_child(buttons)
	_button(buttons, toggle_map, "지도", _toggle_map)
	_button(buttons, toggle_info, "정보", _toggle_info)
	add_child(toggle_hud)
	toggle_hud.text = "계기판 숨김"
	toggle_hud.focus_mode = Control.FOCUS_NONE
	toggle_hud.pressed.connect(_toggle_hud)
	add_child(details)
	details.add_theme_stylebox_override("panel", _style())
	var scroll := ScrollContainer.new()
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	details.add_child(scroll)
	info_column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	info_column.add_theme_constant_override("separation", 12)
	scroll.add_child(info_column)
	for label in [metrics, detail_text]:
		label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		label.add_theme_font_size_override("font_size", 13)
		info_column.add_child(label)
	details.hide()
	add_child(map_panel)
	map_panel.add_theme_stylebox_override("panel", _style())
	var map_column := VBoxContainer.new()
	map_panel.add_child(map_column)
	var map_title := Label.new()
	map_title.text = "코스 지도 · 도로 / 차량 / 목표"
	map_title.add_theme_font_size_override("font_size", 13)
	map_column.add_child(map_title)
	large_map.size_flags_vertical = Control.SIZE_EXPAND_FILL
	large_map.clip_contents = true
	map_column.add_child(large_map)
	map_panel.hide()
	add_child(alert_panel)
	alert_panel.add_theme_stylebox_override("panel", _style(Color("452b25ee")))
	alert.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	alert.add_theme_font_size_override("font_size", 14)
	alert.add_theme_color_override("font_color", Color("ffcfaa"))
	alert_panel.add_child(alert)
	alert_panel.hide()
	add_child(attribution)
	attribution.add_theme_font_size_override("font_size", 10)
	attribution.add_theme_color_override("font_color", Color("c1ccd5"))
	attribution.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	attribution.clip_text = true
	resized.connect(_layout)
	_layout()


## 최대 폭 520px·높이 132px로 도로 중앙을 비우고 펼침 창은 화면 가장자리에 둔다.
func _layout() -> void:
	panel.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM)
	panel.offset_left = -260
	panel.offset_right = 260
	panel.offset_top = -152
	panel.offset_bottom = -20
	for popup in [details, map_panel]:
		popup.set_anchors_and_offsets_preset(Control.PRESET_TOP_RIGHT)
		popup.offset_left = -360
		popup.offset_right = -20
		popup.offset_top = 64
		popup.offset_bottom = minf(350, size.y - 170)
	alert_panel.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
	alert_panel.offset_left = -260
	alert_panel.offset_right = 260
	alert_panel.offset_top = 62
	toggle_hud.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	toggle_hud.offset_left = -124
	toggle_hud.offset_right = -20
	toggle_hud.offset_top = -50
	toggle_hud.offset_bottom = -22
	attribution.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE)
	attribution.offset_left = 16
	attribution.offset_right = -16
	attribution.offset_top = -17
	attribution.offset_bottom = -1


## Rust 코스 형상을 두 지도에 전달한다. 배경 타일이나 네트워크를 사용하지 않는다.
func configure_course(data: Dictionary) -> void:
	minimap.configure(data)
	large_map.configure(data)


## 실제 차량의 위치·방향을 두 지도에 같은 값으로 전달한다.
func update_vehicle(position_3d: Vector3, heading_rad: float) -> void:
	minimap.update_vehicle(position_3d, heading_rad)
	large_map.update_vehicle(position_3d, heading_rad)


## 전체 출처 안내는 정보창에 두고 저작권 표기는 접어도 계속 표시한다.
func add_notices(label: Label) -> void:
	if label.get_parent() == null:
		info_column.add_child(label)
	label.visible = true
	label.tooltip_text = label.text
	var text := label.text
	if text.contains("OpenStreetMap"):
		attribution.text = "© OpenStreetMap contributors · 자세한 출처: 정보"
	else:
		attribution.text = text.get_slice("\n", 0)
	attribution.tooltip_text = text


## 정상 상태는 간단히 표시하고 안전 경고는 정보창·계기판을 숨겨도 별도로 보여 준다.
func update_state(kmh: float, gear: String, description: String, values: Dictionary, elapsed_s: float, flags: Dictionary = {}) -> void:
	speed.text = "%.0f km/h" % absf(kmh)
	dial.set_speed(kmh)
	gears.text = "기어 " + gear
	state.text = ("시동 켜짐" if flags.get("engine_on", false) else "시동 꺼짐") + " · " + str(flags.get("view", "전방"))
	var warning := str(flags.get("alert", ""))
	alert.text = warning
	alert_panel.visible = not warning.is_empty()
	if not warning.is_empty():
		state.text = warning
	detail_text.text = description
	var seconds := int(elapsed_s)
	metrics.text = "시뮬레이션 시간 %02d:%02d\n" % [seconds / 60, seconds % 60]
	if values.get("active", false):
		metrics.text += "주행 거리 %.1f m · 충돌 %d회\nCTE %.2f m\n목표까지 직선 거리 %.1f m%s" % [
			values.distance_m, values.impact_count, values.cte_m, values.remaining_m,
			" · 목표 도달" if values.goal_reached else ""]
	else:
		metrics.text += "경로 평가 미지원 코스\nCTE·누적 거리·충돌 합계: —"


## 초기화 실패는 접힌 창에 숨기지 않고 고정 경고와 상세 정보로 보여 준다.
func show_alert(message: String) -> void:
	alert.text = message
	state.text = message
	alert_panel.show()


## 초기화 실패는 접힌 창에 숨기지 않고 고정 경고와 상세 정보로 보여 준다.
func show_failure(message: String) -> void:
	speed.text = "—"
	gears.text = "주행 중지"
	state.text = message
	detail_text.text = message
	metrics.text = "메뉴로 돌아가 다른 코스를 선택하세요."
	alert.text = message
	alert_panel.show()
	details.show()


## 정보 버튼으로만 긴 수치·출처·설명을 펼친다.
func _toggle_info() -> void:
	map_panel.hide()
	details.visible = not details.visible


## 지도 버튼으로 코스 전체를 더 크게 확인한다.
func _toggle_map() -> void:
	details.hide()
	map_panel.visible = not map_panel.visible


## 계기판만 부드럽게 감추거나 되돌린다. 안전 경고에는 영향을 주지 않는다.
func _toggle_hud() -> void:
	if _visibility_tween:
		_visibility_tween.kill()
	var hide_panel := panel.visible and panel.modulate.a > 0.5
	if not hide_panel:
		panel.show()
	_visibility_tween = create_tween()
	_visibility_tween.tween_property(panel, "modulate:a", 0.0 if hide_panel else 1.0, 0.18)
	if hide_panel:
		_visibility_tween.tween_callback(panel.hide)
	toggle_hud.text = "계기판 표시" if hide_panel else "계기판 숨김"


## 주행 키와 충돌하지 않는 포커스 없는 버튼을 만든다.
func _button(parent: Control, button: Button, text: String, action: Callable) -> void:
	button.text = text
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(action)
	parent.add_child(button)


## 작은 패널에 공통으로 적용할 반투명 바탕을 만든다.
func _style(color: Color = Color("101d29ee")) -> StyleBoxFlat:
	var style := StyleBoxFlat.new()
	style.bg_color = color
	style.border_color = Color("345164")
	style.set_border_width_all(1)
	style.set_corner_radius_all(14)
	style.set_content_margin_all(10)
	return style
