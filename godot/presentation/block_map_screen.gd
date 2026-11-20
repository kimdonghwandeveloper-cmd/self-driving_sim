extends "res://presentation/map_screen.gd"
## 도로 하나를 선택해 주변 영역 전체를 자유 주행용으로 생성한다.

var area_lines: Array = []
var selected_road := ""
var extent := OptionButton.new()
var selection_label := Label.new()
var sidebar_scroll := ScrollContainer.new()
var technical_feedback := ""
var preview_ok := false
var draw_origin := Vector2.ZERO

## 화면 없는 검증에서는 지도 캔버스를 숨기고 컨트롤·서비스만 실행한다.
## Godot dummy 렌더러의 큰 지도 캔버스 종료 충돌을 피하며 실제 화면은 OpenGL 시험으로 확인한다.
func _ready() -> void:
	super._ready()
	if DisplayServer.get_name() == "headless":
		hide()
		tiles = null

## 기존 검색·지도 서비스를 유지하고 역할·방향 도구를 영역 선택으로 교체한다.
func _services(canvas: CanvasLayer) -> void:
	super._services(canvas)
	var header := PanelContainer.new()
	header.position = Vector2(230, 8)
	header.size = Vector2(500, 44)
	var header_style := StyleBoxFlat.new()
	header_style.bg_color = Color.WHITE
	header_style.set_content_margin_all(10)
	header_style.set_corner_radius_all(8)
	header.add_theme_stylebox_override("panel", header_style)
	canvas.add_child(header)
	var header_label := Label.new()
	header_label.text = "지도에서 도로 선택  /  자유 주행"
	header_label.add_theme_color_override("font_color", Color("27364b"))
	header.add_child(header_label)
	for child in toolbar.get_children(): child.hide()
	var theme := Theme.new()
	for kind in ["Label", "Button", "LineEdit", "OptionButton", "CheckButton", "LinkButton"]:
		for state in ["font_color", "font_hover_color", "font_focus_color", "font_pressed_color"]:
			theme.set_color(state, kind, Color("27364b"))
		theme.set_font_size("font_size", kind, 15)
		theme.set_color("font_disabled_color", kind, Color("7d8c9e"))
	for kind in ["Button", "OptionButton", "LineEdit"]:
		for state in ["normal", "hover", "pressed", "focus", "disabled"]:
			var box := StyleBoxFlat.new()
			box.bg_color = Color("eef4ff") if state in ["hover", "pressed"] else Color.WHITE
			box.border_color = Color("1262ef") if state == "focus" else Color("dce3ec")
			box.set_border_width_all(1)
			box.set_corner_radius_all(8)
			box.set_content_margin_all(10)
			theme.set_stylebox(state, kind, box)
	toolbar_panel.theme = theme
	var panel := StyleBoxFlat.new()
	panel.bg_color = Color.WHITE
	panel.set_content_margin_all(16)
	panel.set_corner_radius_all(12)
	panel.shadow_color = Color(0.1, 0.2, 0.3, 0.12)
	panel.shadow_size = 8
	toolbar_panel.add_theme_stylebox_override("panel", panel)
	toolbar_panel.remove_child(toolbar)
	var body := VBoxContainer.new()
	body.add_theme_constant_override("separation", 10)
	toolbar_panel.add_child(body)
	sidebar_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sidebar_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	body.add_child(sidebar_scroll)
	sidebar_scroll.add_child(toolbar)
	toolbar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	toolbar.add_theme_constant_override("separation", 8)
	_add_label(toolbar, "어디를 달려볼까요?", 24)
	_add_label(toolbar, "지도에서 원하는 도로를 눌러 주세요.\n주변 도로를 생성해 자유롭게 주행합니다.", 14)
	_move(place_search, toolbar)
	place_search.custom_minimum_size.x = 0
	place_search.placeholder_text = "장소 또는 도로 검색"
	place_search.add_theme_color_override("font_placeholder_color", Color("687c91"))
	_move(place_results, toolbar)
	place_results.custom_minimum_size.x = 0
	place_results.fit_to_longest_item = false
	place_results.clip_text = true
	place_results.visible = not place_results.disabled
	_move(cities, toolbar)
	cities.fit_to_longest_item = false
	_add_button(toolbar, "현재 지역에 맞추기", _fit_region)
	toolbar.add_child(HSeparator.new())
	selection_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	selection_label.text = "선택한 도로 없음"
	toolbar.add_child(selection_label)
	_add_label(toolbar, "도로 생성 범위", 16)
	for item in [["주변 500 m", 500], ["주변 1 km", 1000], ["주변 3 km", 3000], ["주변 5 km", 5000], ["주변 10 km", 10000]]:
		extent.add_item(item[0], item[1])
	extent.item_selected.connect(_extent_changed)
	toolbar.add_child(extent)
	_add_label(toolbar, "경계에 걸친 도로는 교차로를 보존하기 위해\n끝까지 생성될 수 있습니다.", 12)
	_add_button(toolbar, "선택 초기화", _clear_points)
	var details := CheckButton.new()
	details.text = "자료 및 저장"
	details.toggled.connect(func(value: bool): advanced.visible = value)
	toolbar.add_child(details)
	advanced.reparent(toolbar)
	advanced.hide()
	for child in advanced.get_children(): child.hide()
	_move(route_name, advanced)
	route_name.custom_minimum_size.x = 0
	route_name.placeholder_text = "저장 이름 (영문·숫자)"
	_move(save_button, advanced)
	save_button.text = "도로 영역 저장"
	_add_button(advanced, "OSM 표본 파일 열기", _open_snapshot)
	_add_button(advanced, "엑스포 내부 도로 예제", _load_snapshot.bind("testdata/block-real/expo-internal.osm.json"))
	_add_button(advanced, "전체 지도", _fit_country)
	_move(status, body)
	status.custom_minimum_size = Vector2(0, 66)
	status.max_lines_visible = 4
	status.add_theme_font_size_override("font_size", 13)
	_move(drive_button, body)
	drive_button.text = "도로 생성 후 주행"
	drive_button.custom_minimum_size.y = 52
	for state in ["normal", "hover", "pressed", "disabled"]:
		var box := StyleBoxFlat.new()
		box.bg_color = Color("b8c9df") if state == "disabled" else (Color("084bd3") if state == "hover" else Color("1262ef"))
		box.set_corner_radius_all(9)
		drive_button.add_theme_stylebox_override(state, box)
	for state in ["font_color", "font_hover_color", "font_pressed_color", "font_focus_color", "font_disabled_color"]:
		drive_button.add_theme_color_override(state, Color.WHITE)
	_layout_toolbar()
	_refresh_route()

## 결과가 없는 검색 상자는 숨겨 기본 선택 화면의 높이를 줄인다.
func _fill_place_results(found: Array) -> void:
	super._fill_place_results(found)
	place_results.visible = not place_results.disabled

## 설명을 패널 폭에 맞게 자동 줄바꿈한다.
func _add_label(parent: Node, text: String, size: int) -> void:
	var label := Label.new()
	label.text = text
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.add_theme_font_size_override("font_size", size)
	parent.add_child(label)

## 기존 서비스 컨트롤을 새 패널로 옮긴다.
func _move(control: Control, parent: Node) -> void:
	control.reparent(parent)
	control.show()

## 같은 클릭 높이와 포인터를 가진 버튼을 만든다.
func _add_button(parent: Node, text: String, action: Callable) -> void:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size.y = 38
	button.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	button.pressed.connect(action)
	parent.add_child(button)

## 지도와 주행 버튼은 보존하고 도구 내용만 스크롤한다.
func _layout_toolbar() -> void:
	var view := get_viewport_rect().size
	toolbar_panel.position = Vector2(16, 64)
	toolbar_panel.size = Vector2(minf(350.0, view.x * 0.42), maxf(260.0, view.y - 88.0))

## 클릭한 위치의 가장 가까운 도로를 방향 구분 없이 선택한다.
func _select_map_position(screen_position: Vector2, _remove: bool) -> void:
	var at: Vector2 = get_canvas_transform().affine_inverse() * screen_position
	if not _ensure_region(at): return
	var candidates: Array = picker.call("block_candidates", at, clampf(12.0 / camera.zoom.x, 2.0, 300.0))
	if candidates.is_empty():
		_clear_points()
		feedback = "이 위치에는 도로가 없습니다. 도로 선을 눌러 주세요."
		return
	_select_area(str(candidates[0].id))

## 도로와 범위만 전달하고 목표·방향·경로는 지정하지 않는다.
func _select_area(id: String) -> void:
	selected_road = id
	preview_ok = false
	area_lines.clear()
	drive_button.disabled = true
	var result: Dictionary = picker.call("preview_road_area", id, float(extent.get_selected_id()))
	technical_feedback = str(result.error)
	if result.ok:
		area_lines = result.lines
		preview_ok = true
		var title := "선택한 도로"
		for item in picker.call("block_items"):
			if str(item.id) == id:
				title = str(item.name)
				if title in ["service", "pedestrian"]: title = "시설 내부 도로"
		selection_label.text = title + " 주변"
		feedback = "도로 %d개 · 건물 %d개\n목표 지점 없이 자유롭게 주행하세요." % [result.road_count, result.get("building_count", 0)]
	else:
		selection_label.text = "도로 생성 실패"
		feedback = "이 영역을 생성하지 못했습니다.\n다른 도로나 작은 범위를 선택해 주세요."
	drive_button.disabled = not preview_ok
	save_button.disabled = not preview_ok
	overlay_layer.queue_redraw()

## 범위 변경 시 같은 도로로 다시 검증한다.
func _extent_changed(_index: int) -> void:
	if selected_road.is_empty(): return
	if not data_ok:
		_select_area(selected_road)
		return
	for item in picker.call("block_items"):
		if str(item.id) == selected_road:
			var view: Vector2 = item.line[item.line.size() / 2]
			if _open_region_at(view):
				var candidates: Array = picker.call("block_candidates", view, 10.0)
				if not candidates.is_empty(): _select_area(str(candidates[0].id))
			return

## 선택 중에는 지도 이동만으로 원본을 바꾸지 않는다.
func _auto_open(delta: float) -> void:
	if selected_road.is_empty(): super._auto_open(delta)

## 현재 지역 밖을 누르면 새 지역을 연다.
func _ensure_region(at: Vector2) -> bool:
	if opened_region.has_point(at): return true
	if not data_ok and picker != null and not picker.call("block_items").is_empty(): return true
	return _open_region_at(at)

## 영역 주위 여유를 포함해 전국 자료에서 도로를 연다.
func _open_region_at(view: Vector2) -> bool:
	if picker == null: return false
	_clear_points()
	var result: Dictionary = picker.call("open_block_region", picker.call("view_to_latlon", view), minf(10000.0, maxf(1500.0, float(extent.get_selected_id()) + 1000.0)))
	feedback = "도로를 눌러 영역을 선택하세요." if result.ok else "지역 자료를 열지 못했습니다."
	technical_feedback = str(result.error)
	_refresh_route()
	return result.ok

## 표본 파일도 동일한 영역 생성기를 쓴다.
func _load_snapshot(path: String) -> void:
	_clear_points()
	var result: Dictionary = picker.call("load_block_snapshot", path)
	feedback = "도로를 눌러 영역을 선택하세요." if result.ok else "지도 자료를 열지 못했습니다."
	technical_feedback = str(result.error)
	if result.ok:
		_refresh_route()
		_fit_region()

## 검색은 지도 이동만 하고 출발·도착 지점을 만들지 않는다.
func _pick_area_result(chosen: Dictionary) -> void:
	var view: Vector2 = chosen.view
	camera.position = view
	camera.zoom = Vector2.ONE * 0.5
	camera.force_update_scroll()
	_open_region_at(view)

## 장소 선택 시 시설 주변 도로를 사용자가 고르도록 안내한다.
func _add_place(index: int) -> void:
	if index < 0 or index >= place_outlines.size(): return
	var outline: PackedVector2Array = place_outlines[index]
	if outline.is_empty(): return
	var center := Vector2.ZERO
	for point in outline: center += point
	camera.position = center / outline.size()
	feedback = "시설 주변의 주행할 도로를 눌러 주세요."

## 선택과 이전 미리보기를 함께 폐기한다.
func _clear_points() -> void:
	selected_road = ""
	area_lines.clear()
	preview_ok = false
	if picker != null: picker.call("clear_road_area")
	drive_button.disabled = true
	save_button.disabled = true
	selection_label.text = "선택한 도로 없음"
	feedback = "지도에서 원하는 도로를 눌러 주세요."
	technical_feedback = ""
	overlay_layer.queue_redraw()

## 검증한 영역 파일을 사용자 이름으로 저장한다.
func _save_route() -> void:
	if not preview_ok: return
	var saved: Dictionary = picker.call("save_road_area", route_name.text)
	feedback = "도로 영역을 저장했습니다." if saved.ok else "저장하지 못했습니다. 이름을 확인해 주세요."
	technical_feedback = str(saved.error)

## 영역을 저장한 후 목표 없는 공통 주행 화면으로 전환한다.
func _drive_route() -> void:
	if not preview_ok: return
	var saved: Dictionary = picker.call("save_road_area", "area-drive-%d-%d" % [OS.get_process_id(), Time.get_ticks_usec()])
	if saved.ok: drive_requested.emit("map:" + str(saved.path))
	else:
		preview_ok = false
		drive_button.disabled = true
		feedback = "자료가 바뀌었거나 저장하지 못했습니다. 도로를 다시 선택해 주세요."
		technical_feedback = str(saved.error)

## 경로 통행 방향과 무관한 전체 원본 도로를 표시한다.
func _refresh_route() -> void:
	if picker == null: return
	opened_region = picker.call("loaded_region")
	place_outlines = picker.call("place_outlines")
	_rebuild_places()
	var segments := PackedVector2Array()
	var flags := PackedByteArray()
	for block in picker.call("block_items"):
		for i in range(block.line.size() - 1):
			if block.line[i].distance_squared_to(block.line[i+1]) < 0.01: continue
			segments.append(block.line[i])
			segments.append(block.line[i+1])
			flags.append(1)
	display_geometry(segments, flags, PackedVector2Array(), PackedVector2Array())
	if not segments.is_empty(): draw_origin = segments[0]

## 경로 탐색 없이 검색·타일·상태 안내만 갱신한다.
func _process(delta: float) -> void:
	if picker != null:
		for event in picker.call("poll"):
			if event.type == "search_results" and search_request != 0 and int(event.request_id) == search_request: _show_area_results(event)
		_auto_open(delta)
	status.text = feedback
	status.tooltip_text = technical_feedback
	if tiles != null: _update_tiles()
	if camera.zoom.x != overlay_zoom: overlay_layer.queue_redraw()
	queue_redraw()

## 흰 도로·회색 건물과 생성할 도로 전체를 파란색으로 표시한다.
func _draw_overlay() -> void:
	overlay_zoom = camera.zoom.x
	var scale_m := maxf(camera.zoom.x, 0.0005)
	overlay_layer.draw_set_transform(draw_origin)
	if not place_edges.is_empty(): overlay_layer.draw_multiline(_local_line(place_edges), Color("bbc6d2"), 1.0 / scale_m)
	if not road_open.is_empty():
		overlay_layer.draw_multiline(_local_line(road_open), Color.WHITE, 2.0 / scale_m)
		overlay_layer.draw_multiline(_local_line(road_open), Color("aebdcb"), 2.0 / scale_m)
	for line in area_lines:
		var visible_line := PackedVector2Array()
		for point in line:
			if visible_line.is_empty() or point.distance_squared_to(visible_line[-1]) >= 0.01: visible_line.append(point)
		if visible_line.size() > 1:
			var local := _local_line(visible_line)
			for i in range(local.size() - 1):
				overlay_layer.draw_line(local[i], local[i+1], Color("1262ef"), 5.0 / scale_m)
	overlay_layer.draw_set_transform(Vector2.ZERO)

## 큰 메르카토르 좌표를 지역 좌표로 옮겨 굵은 선 메시의 부동소수점 손상을 막는다.
func _local_line(line: PackedVector2Array) -> PackedVector2Array:
	var local := PackedVector2Array()
	for point in line: local.append(point - draw_origin)
	return local

## 큰 지도 좌표의 사각형도 카메라 기준으로 옮겨 렌더러의 정밀도 손상을 피한다.
func _draw() -> void:
	var scale_m := maxf(camera.zoom.x, 0.0005)
	var span := get_viewport_rect().size / scale_m
	draw_set_transform(camera.position)
	draw_rect(Rect2(-span, span * 2.0), Color("eef2f6"))
	for tile in tile_layers:
		if tile.get("texture") is Texture2D:
			var rect: Rect2 = tile.rect
			rect.position -= camera.position
			draw_texture_rect(tile.texture, rect, false)
	for polygon in mask_polygons:
		var local := PackedVector2Array()
		for point in polygon: local.append(point - camera.position)
		draw_colored_polygon(local, MASK_COLOR)
	draw_set_transform(Vector2.ZERO)

## 사이드바를 제외한 지도 공간에 지역을 맞춘다.
func _fit_rect(bounds: Rect2) -> void:
	if bounds.size.x <= 0.0 or bounds.size.y <= 0.0: return
	var view := get_viewport_rect().size
	var left := toolbar_panel.get_global_rect().end.x + 24.0
	var available := Vector2(view.x - left - 24.0, view.y - 100.0)
	if available.x <= 0.0 or available.y <= 0.0: return
	var value := clampf(minf(available.x / bounds.size.x, available.y / bounds.size.y), 0.0005, 8.0)
	camera.zoom = Vector2.ONE * value
	camera.position = bounds.get_center() - Vector2(left * 0.5 / value, 20.0 / value)
	_clamp_camera()
	last_camera = camera.position
	last_zoom = camera.zoom.x
	still_time = AUTO_OPEN_DELAY_S
