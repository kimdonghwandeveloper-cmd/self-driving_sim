extends "res://presentation/map_screen.gd"
## 두 핀으로 기존 경로 코스 생성기를 구동하는 지도 화면.

var pins: Array = [null, null]
var fields: Array[LineEdit] = []
var results: Array[OptionButton] = []
var choices: Array[Array] = [[], [], []]
var query_slot := 0
var query_text := ""
var route_ready := false
var alternatives: Array = []
var hovered_route := -1
var pin_mode := Label.new()
var hover_position := Vector2.INF
var hover_transform := Transform2D.IDENTITY

## 헤드리스 검증에서는 지도 렌더링과 타일 요청을 생략한다.
func _ready() -> void:
	super._ready()
	if DisplayServer.get_name() == "headless":
		hide()
		tiles = null

## 기존 지도 서비스 위에 출발·도착 검색과 지도 지정 도구만 배치한다.
func _services(canvas: CanvasLayer) -> void:
	super._services(canvas)
	for child in toolbar.get_children(): child.hide()
	var panel := StyleBoxFlat.new()
	panel.bg_color = Color.WHITE
	panel.set_corner_radius_all(14)
	panel.set_content_margin_all(16)
	toolbar_panel.add_theme_stylebox_override("panel", panel)
	var theme := Theme.new()
	for kind in ["Label", "Button", "OptionButton", "LineEdit"]:
		theme.set_color("font_color", kind, Color("25354d"))
		for state in ["normal", "hover", "pressed", "focus", "disabled"]:
			var box := StyleBoxFlat.new()
			box.bg_color = Color("eaf2ff") if state in ["hover", "pressed", "focus"] else Color("f6f8fc")
			box.set_corner_radius_all(8)
			box.set_content_margin_all(8)
			theme.set_stylebox(state, kind, box)
	toolbar_panel.theme = theme
	var title := Label.new()
	title.text = "주행 경로 선택"
	title.add_theme_font_size_override("font_size", 23)
	toolbar.add_child(title)
	for slot in [2, 0, 1]:
		var role := ["출발", "도착", "지역"][slot] as String
		var row := HBoxContainer.new()
		toolbar.add_child(row)
		var field := LineEdit.new()
		field.placeholder_text = role + " 장소·주소 검색"
		field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		field.add_theme_color_override("font_placeholder_color", Color("596a80"))
		row.add_child(field)
		fields.resize(3)
		fields[slot] = field
		_button(row, "검색", _submit_search.bind(slot))
		field.text_submitted.connect(func(_value: String): _submit_search(slot))
		field.text_changed.connect(func(_value: String): _clear_results(slot))
		var list := OptionButton.new()
		list.fit_to_longest_item = false
		list.clip_text = true
		list.hide()
		toolbar.add_child(list)
		results.resize(3)
		results[slot] = list
		list.item_selected.connect(_choose_result.bind(slot))
		if slot < 2:
			_button(toolbar, role + " 핀 지도에 찍기 (" + ("F" if slot == 0 else "G") + ")", _select_endpoint.bind(slot == 1))
	toolbar.add_child(pin_mode)
	for control in [cities, status, drive_button]:
		control.reparent(toolbar)
		control.show()
	cities.fit_to_longest_item = false
	_button(toolbar, "경로 맞춤", _fit_route)
	_button(toolbar, "핀 초기화", _clear_points)
	drive_button.text = "도로 생성 후 주행"
	drive_button.custom_minimum_size.y = 44
	var primary := StyleBoxFlat.new()
	primary.bg_color = Color("1262ef")
	primary.set_corner_radius_all(8)
	drive_button.add_theme_stylebox_override("normal", primary)
	drive_button.add_theme_color_override("font_color", Color.WHITE)
	_select_endpoint(false)
	_layout_toolbar()

## 사이드바 버튼을 지정한 동작에 연결한다.
func _button(parent: Control, label: String, action: Callable) -> void:
	var button := Button.new()
	button.text = label
	button.pressed.connect(action)
	parent.add_child(button)

## 지도 대부분은 입력에 사용할 수 있도록 비워 둔다.
func _layout_toolbar() -> void:
	toolbar_panel.position = Vector2(14, 57)
	toolbar_panel.size = Vector2(minf(336.0, get_viewport_rect().size.x * 0.43), 0)

## 입력 내용이 바뀌면 해당 칸의 이전 검색 결과를 지운다.
func _clear_results(slot: int) -> void:
	choices[slot].clear()
	results[slot].hide()
	if search_request != 0 and query_slot == slot: search_request = 0

## 해당 핀의 지역 검색과 전국 검색 결과를 같은 목록에 표시한다.
func _submit_search(slot: int) -> void:
	if picker == null: return
	var field := fields[slot]
	var composition := DisplayServer.ime_get_text() if field.has_focus() else ""
	var query := (field.text + composition).strip_edges()
	if query.is_empty(): return
	var local: Array = picker.call("search_places", query)
	_show_results(slot, local)
	var response: Dictionary = picker.call("search_nationwide", query)
	if response.ok:
		query_slot = slot
		query_text = field.text
		search_request = int(response.request_id)
	elif local.is_empty():
		feedback = str(response.error)

## 검색 결과의 소속 핀을 유지하며 목록을 갱신한다.
func _show_results(slot: int, found: Array) -> void:
	choices[slot] = found
	var list := results[slot]
	list.clear()
	list.add_item("검색 결과에서 고르기")
	for item in found:
		var name := str(item.name)
		var address := str(item.get("address", ""))
		list.add_item(name if address.is_empty() else name + " · " + address)
	list.select(0)
	list.visible = not found.is_empty()

## 현재 목록에서 선택한 항목만 적용한다.
func _choose_result(index: int, slot: int) -> void:
	if index > 0 and index <= choices[slot].size():
		_apply_search_result(slot, choices[slot][index - 1])

## 검색한 장소 또는 진입로를 해당 핀으로 적용한다.
func _apply_search_result(slot: int, item: Dictionary) -> void:
	if slot == 2:
		if item.has("view"):
			_go_to_search_region(item.view)
		elif int(item.index) < place_outlines.size():
			var outline: PackedVector2Array = place_outlines[int(item.index)]
			if not outline.is_empty(): _go_to_search_region(outline[0])
		return
	var at: Vector2
	if item.has("view"):
		at = item.view
	else:
		var added: Dictionary = picker.call("add_place", int(item.index))
		if not added.ok:
			feedback = str(added.error)
			return
		at = picker.call("points")[-1]
		picker.call("remove_last")
	if _set_pin(slot, at, str(item.name)):
		results[slot].hide()
		if route_ready: _fit_route()
		else: _fit_rect(Rect2(at - Vector2.ONE * 450.0, Vector2.ONE * 900.0))

## 지도 클릭은 활성 핀만 교체하며 다른 핀은 보존한다.
func _select_map_position(screen_position: Vector2, _remove: bool) -> void:
	var candidate := _alternative_at(screen_position)
	if candidate >= 0 and picker.call("choose_route_alternative", candidate):
		_refresh_route()
		alternatives = picker.call("route_alternatives")
		hovered_route = -1
		overlay_layer.queue_redraw()
		return
	var at := get_canvas_transform().affine_inverse() * screen_position
	var slot := int(selecting_destination)
	if _set_pin(slot, at) and slot == 0 and pins[1] == null: _select_endpoint(true)

## 자료가 필요한 경우 반대편 핀을 포함하는 조회 범위를 열어 두 지점을 함께 계산한다.
func _set_pin(slot: int, at: Vector2, label: String = "") -> bool:
	if picker == null: return false
	if not opened_region.has_point(at) and (data_ok or roads.is_empty()):
		var target: PackedFloat64Array = picker.call("view_to_latlon", at)
		var response: Dictionary
		if pins[1 - slot] == null:
			response = picker.call("open_region", target)
		else:
			response = picker.call("expand_route_region", target, slot == 1 or pins[0] == null)
		if not response.ok:
			feedback = str(response.error)
			return false
		for index in results.size(): _clear_results(index)
	pins[slot] = at
	_clear_results(slot)
	fields[slot].text = label if not label.is_empty() else "지도에서 지정"
	_recalculate()
	return true

## 계획기는 항상 출발·도착 순서로 호출하며 경로 실패 시 지도 핀은 유지한다.
func _recalculate() -> void:
	picker.call("clear_points")
	for pin in pins:
		if pin != null: picker.call("add_point", pin)
	_refresh_route()
	var route: Dictionary = picker.call("route")
	route_ready = route.ok and pins[0] != null and pins[1] != null
	picker.call("prepare_route_alternatives")
	alternatives = picker.call("route_alternatives")
	hovered_route = -1
	hover_position = Vector2.INF
	drive_button.disabled = not route_ready
	feedback = ""

## 핀과 검색 결과를 함께 초기화한다.
func _clear_points() -> void:
	pins = [null, null]
	for slot in fields.size():
		fields[slot].clear()
		_clear_results(slot)
	if picker != null: _recalculate()
	_select_endpoint(false)

## 지역 바로가기는 선택 중인 핀이나 계획기를 덮어쓰지 않는다.
func _open_region_at(view: Vector2) -> bool:
	if pins[0] != null or pins[1] != null:
		feedback = "현재 핀을 보존했습니다. 다른 지역을 열려면 핀을 초기화하세요."
		return false
	var opened := super._open_region_at(view)
	if opened:
		for slot in results.size(): _clear_results(slot)
	return opened

## 검색 요청을 보낸 핀에만 응답을 전달하고 경로 상태를 표시한다.
func _process(delta: float) -> void:
	if picker == null: return
	for event in picker.call("poll"):
		if event.type == "search_results" and search_request != 0 and int(event.request_id) == search_request:
			search_request = 0
			if fields[query_slot].text == query_text:
				var found: Array = picker.call("search_places", query_text)
				found.append_array(event.get("results", []))
				_show_results(query_slot, found)
				feedback = str(event.message)
	if pins[0] == null and pins[1] == null: _auto_open(delta)
	pin_mode.text = "도착 핀 모드 · G" if selecting_destination else "출발 핀 모드 · F"
	_update_route_hover()
	var route: Dictionary = picker.call("route")
	var hint := "지도에서 %s 핀을 찍거나 검색하세요." % ("도착" if selecting_destination else "출발")
	if route_ready:
		hint = "주행 경로 %.0f m" % float(route.length_m)
		if not alternatives.is_empty(): hint += "\n점선 경로를 클릭해 변경"
	elif pins[0] != null and pins[1] != null: hint = "연결 가능한 경로가 없습니다. 핀 위치를 조정하세요.\n" + str(route.error)
	var message := hint + ("\n" + feedback if not feedback.is_empty() else "")
	if status.text != message: status.text = message
	if tiles != null: _update_tiles()
	if camera.zoom.x != overlay_zoom: overlay_layer.queue_redraw()
	queue_redraw()

## 지도에 연결 경로와 역할별 핀을 표시하고 진입로까지의 오프셋을 선으로 보여 준다.
func _draw_overlay() -> void:
	overlay_zoom = camera.zoom.x
	var scale_m := maxf(camera.zoom.x, 0.0005)
	if not road_open.is_empty(): overlay_layer.draw_multiline(road_open, Color("aebdcb"), 1.0 / scale_m)
	for i in alternatives.size():
		var line: PackedVector2Array = alternatives[i].line
		for j in range(1, line.size()):
			overlay_layer.draw_dashed_line(line[j-1], line[j], Color("1262ef") if i == hovered_route else Color("5977a9"), (5.0 if i == hovered_route else 3.0) / scale_m, 8.0 / scale_m, false, true)
	if route_line.size() > 1: overlay_layer.draw_polyline(route_line, Color("1262ef"), 5.0 / scale_m, true)
	for slot in 2:
		if pins[slot] == null: continue
		var pin: Vector2 = pins[slot]
		var color := Color("1262ef") if slot == 0 else Color("cf465e")
		if route_ready and not route_line.is_empty():
			var snapped := route_line[0 if slot == 0 else -1]
			overlay_layer.draw_line(pin, snapped, color, 2.0 / scale_m)
			overlay_layer.draw_circle(snapped, 4.0 / scale_m, color)
		var center := pin + Vector2(0, -13.0 / scale_m)
		overlay_layer.draw_line(pin, center, color, 4.0 / scale_m)
		overlay_layer.draw_circle(center, 11.0 / scale_m, Color.WHITE)
		overlay_layer.draw_circle(center, 9.0 / scale_m, color)
		overlay_layer.draw_string(ThemeDB.fallback_font, center + Vector2(-4, 4) / scale_m, "A" if slot == 0 else "B", HORIZONTAL_ALIGNMENT_LEFT, -1, maxi(1, roundi(13.0 / scale_m)), Color.WHITE)

## 지도 화면에서 입력할 때 F·G로 핀 모드를 전환한다.
func _unhandled_key_input(event: InputEvent) -> void:
	if not event is InputEventKey or not event.pressed or event.echo or event.ctrl_pressed or event.alt_pressed or event.meta_pressed: return
	if get_viewport().gui_get_focus_owner() is LineEdit: return
	if event.physical_keycode in [KEY_F, KEY_G] or event.keycode in [KEY_F, KEY_G]:
		_select_endpoint(event.physical_keycode == KEY_G or event.keycode == KEY_G)
		get_viewport().set_input_as_handled()

## 지역 검색은 카메라만 옮기며 핀을 추가하거나 바꾸지 않는다.
func _go_to_search_region(at: Vector2) -> void:
	if pins[0] == null and pins[1] == null: _open_region_at(at)
	_fit_rect(Rect2(at - Vector2.ONE * 750.0, Vector2.ONE * 1500.0))
	results[2].hide()

## 현재 실선과 겹치는 곳에서는 대안 전환을 막고 점선 구간만 클릭 대상으로 잡는다.
func _alternative_at(at: Vector2) -> int:
	if alternatives.is_empty() or _screen_line_distance(route_line, at) <= 5.0: return -1
	var best := 9.0
	var selected := -1
	for i in alternatives.size():
		var distance := _screen_line_distance(alternatives[i].line, at)
		if distance < best:
			best = distance
			selected = i
	return selected

## 포인터 또는 지도 변환이 바뀔 때 대안 강조를 갱신한다.
func _update_route_hover() -> void:
	var at := get_viewport().get_mouse_position()
	var transform := get_canvas_transform()
	if dragging or left_down or toolbar_panel.get_global_rect().has_point(at):
		at = Vector2.INF
	if at == hover_position and transform == hover_transform: return
	hover_position = at
	hover_transform = transform
	var candidate := _alternative_at(at) if at.is_finite() else -1
	if candidate != hovered_route:
		hovered_route = candidate
		overlay_layer.queue_redraw()

## 화면 픽셀 기준으로 포인터와 각 선분 사이의 최단 거리를 구한다.
func _screen_line_distance(line: PackedVector2Array, at: Vector2) -> float:
	var transform := get_canvas_transform()
	var best := INF
	for i in range(1, line.size()):
		var closest := Geometry2D.get_closest_point_to_segment(at, transform * line[i-1], transform * line[i])
		best = minf(best, at.distance_to(closest))
	return best

## 경로가 사이드바 뒤에 가려지지 않도록 오른쪽 지도 공간에 맞춘다.
func _fit_rect(bounds: Rect2) -> void:
	var view := get_viewport_rect().size
	var left := toolbar_panel.get_global_rect().end.x + 12.0
	var available := view - Vector2(left + 20.0, 90.0)
	if not bounds.has_area() or available.x <= 0.0 or available.y <= 0.0: return
	var zoom_value := clampf(minf(available.x / bounds.size.x, available.y / bounds.size.y), 0.0005, 8.0)
	camera.zoom = Vector2.ONE * zoom_value
	camera.position = bounds.get_center() - Vector2(left * 0.5, 20) / zoom_value
	_clamp_camera()
	last_camera = camera.position
	last_zoom = zoom_value
	still_time = AUTO_OPEN_DELAY_S
