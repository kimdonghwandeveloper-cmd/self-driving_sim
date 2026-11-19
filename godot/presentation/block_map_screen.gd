extends "res://presentation/map_screen.gd"
## 기존 지도·검색·타일을 재사용하고 선택/미리보기/저장만 블록 엔진으로 통일한다.

var candidate_items: Array = []
var candidate_list := OptionButton.new()
var selection_list := ItemList.new()
var selected_blocks: Array = []
var focus_line := PackedVector2Array()
var preview_button := Button.new()
var span := OptionButton.new()
var failure_lines: Array = []
var failure_blocks: Array = []
var details_toggle := CheckButton.new()
var sidebar_scroll := ScrollContainer.new()
var technical_feedback := ""
var role_buttons: Array[Button] = []

## 기존 지도 서비스는 유지하고 도구를 좁은 사이드바와 고정 주행 버튼으로 정리한다.
func _services(canvas: CanvasLayer) -> void:
	super._services(canvas)
	for child in toolbar.get_children(): child.hide()
	for button in endpoint_buttons: button.hide()
	endpoint_label.hide()
	restricted_roads.hide()
	overwrite.hide()
	toolbar_panel.remove_child(toolbar)
	var body := VBoxContainer.new()
	body.add_theme_constant_override("separation", 10)
	toolbar_panel.add_child(body)
	sidebar_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	sidebar_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	body.add_child(sidebar_scroll)
	sidebar_scroll.add_child(toolbar)
	toolbar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	toolbar.add_theme_constant_override("separation", 7)
	var title := Label.new()
	title.text = "주행 경로 만들기"
	title.add_theme_font_size_override("font_size", 20)
	toolbar.add_child(title)
	var search_row := HBoxContainer.new()
	toolbar.add_child(search_row)
	_move_control(place_search, search_row)
	place_search.custom_minimum_size.x = 0
	place_search.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	place_search.placeholder_text = "지역·장소 검색"
	_add_button(search_row, "검색", func(): _search_submitted(place_search.text))
	_move_control(place_results, toolbar)
	place_results.custom_minimum_size.x = 0
	place_results.fit_to_longest_item = false
	place_results.clip_text = true
	var navigation := HBoxContainer.new()
	toolbar.add_child(navigation)
	_move_control(cities, navigation)
	cities.fit_to_longest_item = false
	cities.clip_text = true
	cities.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_add_button(navigation, "전체", _fit_country)
	_add_button(navigation, "지역", _fit_region)
	toolbar.add_child(HSeparator.new())
	var instruction := Label.new()
	instruction.text = "도로 클릭 → 방향 확인 → 출발·도착 지정"
	instruction.add_theme_font_size_override("font_size", 13)
	toolbar.add_child(instruction)
	candidate_list.fit_to_longest_item = false
	candidate_list.clip_text = true
	candidate_list.item_selected.connect(_focus_candidate)
	toolbar.add_child(candidate_list)
	var roles := HBoxContainer.new()
	toolbar.add_child(roles)
	for role in [["출발로 지정", "start"], ["도착으로 지정", "goal"]]:
		var button := _add_button(roles, role[0], _confirm_block.bind(role[1]))
		button.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		role_buttons.append(button)
	selection_list.custom_minimum_size.y = 78
	selection_list.tooltip_text = "선택한 출발·경유·도착. 진행 방향은 지도 화살표로 확인하세요."
	toolbar.add_child(selection_list)
	details_toggle.text = "더 보기 · 경유 / 저장 / 예제"
	details_toggle.toggled.connect(func(value: bool): advanced.visible = value)
	toolbar.add_child(details_toggle)
	toolbar.move_child(advanced, toolbar.get_child_count() - 1)
	advanced.visible = false
	coordinates.custom_minimum_size.x = 0
	route_name.custom_minimum_size.x = 0
	var extra := HFlowContainer.new()
	advanced.add_child(extra)
	_add_button(extra, "경유 추가", _confirm_block.bind("via"))
	_add_button(extra, "마지막 경유 취소", _remove_last)
	span.add_item("지역 1.5 km", 1500)
	span.add_item("지역 5 km", 5000)
	span.add_item("지역 10 km", 10000)
	span.tooltip_text = "새 경로를 만들기 전에 열 지역의 크기를 선택합니다."
	advanced.add_child(span)
	for preset in [["엑스포 내부 예제 · 98 m", "expo-internal"], ["엑스포 주변 곡선 예제 · 1 km", "expo-curved-joint"], ["강남 예제 · 253 m", "gangnam-working-area"]]:
		var button := _add_button(advanced, preset[0], _load_snapshot.bind("testdata/block-real/%s.osm.json" % preset[1]))
		button.tooltip_text = "검증한 실제 OSM 표본입니다. 해당 지역 전체가 아닙니다."
	var note := Label.new()
	note.text = "연습용 · 경로 최대 10 km\n일부 교차로는 도로 생성이 제한됩니다."
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.add_theme_font_size_override("font_size", 12)
	advanced.add_child(note)
	var actions := HBoxContainer.new()
	body.add_child(actions)
	preview_button.text = "경로 확인"
	preview_button.pressed.connect(_preview_blocks)
	actions.add_child(preview_button)
	_move_control(drive_button, actions)
	drive_button.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	drive_button.modulate = Color("84e6cd")
	_add_button(actions, "새 경로", _clear_points)
	_move_control(status, body)
	status.max_lines_visible = 4
	status.custom_minimum_size.y = 74
	status.add_theme_font_size_override("font_size", 13)
	candidate_list.add_item("지도의 도로를 클릭하세요")
	candidate_list.disabled = true
	_layout_toolbar()

## 재사용할 검색·저장 위젯의 연결은 유지하고 새 레이아웃으로 이동한다.
func _move_control(control: Control, parent: Node) -> void:
	control.reparent(parent)
	control.show()

## 같은 높이와 글자 크기를 가진 지도 작업 버튼을 생성한다.
func _add_button(parent: Node, text: String, action: Callable) -> Button:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size.y = 30
	button.add_theme_font_size_override("font_size", 14)
	button.pressed.connect(action)
	parent.add_child(button)
	return button

## 작은 창에서는 패널 안만 스크롤하고 지도와 주행 버튼은 화면에 유지한다.
func _layout_toolbar() -> void:
	var view := get_viewport_rect().size
	toolbar_panel.position = Vector2(16, 64)
	toolbar_panel.size = Vector2(minf(360.0, view.x * 0.42), maxf(260.0, view.y - 96.0))

## 클릭은 후보를 조회하며 Shift 클릭도 임의로 도로 선택을 삭제하지 않는다.
func _select_map_position(screen_position: Vector2, _remove: bool) -> void:
	var at: Vector2 = get_canvas_transform().affine_inverse() * screen_position
	if _ensure_region(at): _show_candidates(at)

## 가까운 평행 도로와 양방향을 모두 보여 주고 첫 후보는 강조만 한다.
func _show_candidates(at: Vector2) -> void:
	candidate_items = picker.call("block_candidates", at, clampf(12.0 / camera.zoom.x, 2.0, 300.0))
	candidate_list.clear()
	for item in candidate_items:
		var direction := "정방향 →" if item.direction == "forward" else "역방향 ←"
		candidate_list.add_item("%s · %s · %.0f m" % [item.name, direction, item.length_m])
		candidate_list.set_item_tooltip(candidate_list.item_count - 1, "폭 %.1f m · %s→%s · way %s" % [item.width_m, item.from_node, item.to_node, item.way_id])
	candidate_list.disabled = candidate_items.is_empty()
	if candidate_items.is_empty():
		candidate_list.add_item("클릭 반경에 블록 없음 · 확대 후 도로를 클릭하세요")
		focus_line.clear()
	else:
		candidate_list.select(0)
		_focus_candidate(0)
	feedback = "후보를 골라 화살표를 확인한 뒤 출발·경유·도착으로 확정하세요."
	overlay_layer.queue_redraw()

## 후보를 바꿔도 확정 슬롯은 유지하고 도로 강조·방향 화살표만 갱신한다.
func _focus_candidate(index: int) -> void:
	focus_line = candidate_items[index].line if index >= 0 and index < candidate_items.size() else PackedVector2Array()
	overlay_layer.queue_redraw()

## 사용자가 확인한 방향을 역할 슬롯으로 확정한다.
func _confirm_block(role: String) -> void:
	var index := candidate_list.selected
	if index < 0 or index >= candidate_items.size():
		feedback = "도로 후보를 먼저 선택하세요."
		return
	var item: Dictionary = candidate_items[index]
	var result: Dictionary = picker.call("choose_block", item.id, item.direction, role)
	_apply_selection_result(result, "연결 확인 완료 · 미리보기로 도로 형상을 확인하세요.")
	_refresh_route()

## 확정된 경유만 한 칸 옮기거나 삭제하고 화면을 다시 만든다.
func _edit_via(offset: int) -> void:
	var selected := selection_list.get_selected_items()
	if selected.is_empty() or selected_blocks[selected[0]].role != "via":
		feedback = "목록에서 경유 항목을 고르세요."
		return
	var result: Dictionary = picker.call("edit_block_via", selected_blocks[selected[0]].index, offset)
	_apply_selection_result(result, "경유 연결 확인 완료 · 미리보기를 다시 실행하세요.")
	_refresh_route()

## 계산 실패를 그대로 표시하고 자동 우회나 기존 성공 결과 사용을 하지 않는다.
func _preview_blocks() -> void:
	var result: Dictionary = picker.call("preview_blocks")
	_apply_selection_result(result, "경로 검증 완료 · 저장하거나 주행할 수 있습니다.")
	_refresh_route()

## 실패한 확정은 반영하지 않고 문제의 두 블록을 붉게 표시하며 성공 시 강조를 지운다.
func _apply_selection_result(result: Dictionary, success_message: String) -> void:
	failure_lines.clear()
	failure_blocks = result.get("highlights", [])
	for item in failure_blocks:
		failure_lines.append(item.line)
	technical_feedback = "" if result.ok else str(result.error)
	feedback = success_message if result.ok else _friendly_error(technical_feedback)
	overlay_layer.queue_redraw()

## 내부 진단은 툴팁에 보존하고 기본 화면에는 사용자가 판단할 수 있는 짧은 설명을 표시한다.
func _friendly_error(raw: String) -> String:
	if raw.contains("block too short for joint cut"):
		return "짧은 도로의 교차로 접속부를 만들지 못했습니다. 다른 구간이나 검증된 예제로 확인해 주세요."
	if raw.contains("self-intersect") or raw.contains("self intersect"):
		return "교차로 도로 면이 겹쳐 생성하지 못했습니다. 다른 구간이나 검증된 예제로 확인해 주세요."
	if raw.contains("BlockRouteFailure"):
		return "이 구간의 도로 형상을 만들지 못했습니다. 다른 구간을 선택해 주세요. 상세 진단은 안내에 마우스를 올리면 볼 수 있습니다."
	return raw

## 새 지역을 열면 선택을 지우므로 기존 선택이 있는 동안에는 지역 교체를 명시적으로 막는다.
func _ensure_region(at: Vector2) -> bool:
	if opened_region.has_point(at): return true
	if not selected_blocks.is_empty():
		feedback = "현재 선택을 보존했습니다. 더 넓은 지역은 '새 경로' 후 지역 크기를 선택해 여세요."
		return false
	return _open_region_at(at)

## 선택을 보존해야 할 때 자동 지역 교체를 하지 않는다.
func _auto_open(delta: float) -> void:
	if selected_blocks.is_empty(): super._auto_open(delta)

## 고른 크기의 전국 자료를 원본 스냅샷으로 열어 블록을 구성한다.
func _open_region_at(view: Vector2) -> bool:
	if picker == null: return false
	if not selected_blocks.is_empty():
		feedback = "새 지역을 열려면 먼저 '새 경로'를 누르세요."
		return false
	var result: Dictionary = picker.call("open_block_region", picker.call("view_to_latlon", view), float(span.get_selected_id()))
	feedback = "" if result.ok else str(result.error)
	if result.ok: _clear_candidates()
	_refresh_route()
	return result.ok

## 파일을 열 때도 같은 블록 엔진을 쓰고 기존 점 경로 계산은 호출하지 않는다.
func _load_snapshot(path: String) -> void:
	if not selected_blocks.is_empty():
		feedback = "선택을 보존했습니다. '새 경로' 후 스냅샷을 여세요."
		return
	var result: Dictionary = picker.call("load_block_snapshot", path)
	feedback = "블록 스냅샷을 열었습니다." if result.ok else str(result.error)
	if result.ok:
		_clear_candidates()
		_refresh_route()
		_fit_region()

## 검색 결과는 지도를 이동하고 도로 후보만 보여 준다. 시설 중심을 자동 스냅하지 않는다.
func _pick_area_result(chosen: Dictionary) -> void:
	var view: Vector2 = chosen.view
	camera.position = view
	camera.zoom = Vector2.ONE * 0.5
	camera.force_update_scroll()
	if _ensure_region(view): _show_candidates(view)

## 지역 장소를 고르면 그 윤곽 가운데의 도로 후보를 보여 주고 확정은 사용자에게 맡긴다.
func _add_place(index: int) -> void:
	if index < 0 or index >= place_outlines.size(): return
	var outline: PackedVector2Array = place_outlines[index]
	if outline.is_empty(): return
	var center := Vector2.ZERO
	for point in outline: center += point
	_show_candidates(center / outline.size())

## 새 경로는 블록 선택과 후보·미리보기를 함께 지운다.
func _clear_points() -> void:
	if picker == null: return
	picker.call("clear_blocks")
	_clear_candidates()
	_refresh_route()

## 기존 취소 버튼은 마지막 경유 삭제 또는 새 경로 안내로 처리한다.
func _remove_last() -> void:
	var vias := selected_blocks.filter(func(item): return item.role == "via")
	if vias.is_empty():
		feedback = "출발·도착은 다른 후보로 교체하거나 '새 경로'로 초기화하세요."
	else:
		var result: Dictionary = picker.call("edit_block_via", vias[-1].index, 0)
		_apply_selection_result(result, "마지막 경유를 삭제했습니다. 미리보기를 다시 실행하세요.")
		_refresh_route()

## 후보 선택 상태만 지우고 확정 목록은 손대지 않는다.
func _clear_candidates() -> void:
	candidate_items.clear()
	candidate_list.clear()
	candidate_list.add_item("도로를 클릭해 블록과 방향 후보 보기")
	candidate_list.disabled = true
	focus_line.clear()
	failure_lines.clear()
	failure_blocks.clear()
	feedback = ""
	technical_feedback = ""

## 미리보기한 블록 경로를 새 파일로 저장한다. 덮어쓰기는 지원하지 않는다.
func _save_route() -> void:
	if not failure_lines.is_empty():
		feedback = "실패한 선택을 올바른 도로·방향으로 다시 확정한 뒤 저장하세요."
		return
	var result: Dictionary = picker.call("save_blocks", route_name.text)
	feedback = "저장: " + str(result.path) if result.ok else str(result.error)

## 검증한 결과를 고유 이름으로 저장해 기존 공통 지도 주행 화면으로 넘긴다.
func _drive_route() -> void:
	if not failure_lines.is_empty() or not picker.call("block_route").ok:
		feedback = "먼저 경로 미리보기를 통과해야 합니다."
		return
	var name := "block-drive-%d-%d" % [OS.get_process_id(), Time.get_ticks_usec()]
	var saved: Dictionary = picker.call("save_blocks", name)
	if saved.ok: drive_requested.emit("map:" + str(saved.path))
	else: feedback = str(saved.error)

## 선택과 독립적인 도로망 전체를 그리고 확정 목록과 검증된 안내선을 표시한다.
func _refresh_route() -> void:
	if picker == null: return
	opened_region = picker.call("loaded_region")
	place_outlines = picker.call("place_outlines")
	_rebuild_places()
	var road_points := PackedVector2Array()
	var flags := PackedByteArray()
	for block in picker.call("block_items"):
		for i in range(block.line.size() - 1):
			road_points.append(block.line[i])
			road_points.append(block.line[i+1])
			flags.append(0 if block.directions.is_empty() else 1)
	selected_blocks = picker.call("block_selections")
	selection_list.clear()
	var selected_points := PackedVector2Array()
	for item in selected_blocks:
		var role := "출발" if item.role == "start" else ("도착" if item.role == "goal" else "경유 %d" % (int(item.index)+1))
		selection_list.add_item("%s · %s · %s" % [role, item.name, "→" if item.direction == "forward" else "←"])
		selection_list.set_item_tooltip(selection_list.item_count - 1, "%s→%s · way %s" % [item.from_node, item.to_node, item.way_id])
		if failure_blocks.any(func(b): return b.id == item.id):
			selection_list.set_item_custom_fg_color(selection_list.item_count - 1, Color("ff7474"))
		selected_points.append(item.line[0])
	var route: Dictionary = picker.call("block_route")
	display_geometry(road_points, flags, selected_points, route.line)

## 기존 점 기반 상태 갱신을 부르지 않고 블록 미리보기 성공 여부로 저장·주행 버튼을 제어한다.
func _process(delta: float) -> void:
	if picker != null:
		for event in picker.call("poll"):
			if event.type == "search_results" and search_request != 0 and int(event.request_id) == search_request: _show_area_results(event)
		_auto_open(delta)
		var route: Dictionary = picker.call("block_route")
		save_button.disabled = not route.ok or not failure_lines.is_empty()
		drive_button.disabled = not route.ok or not failure_lines.is_empty()
		var summary := "선택 연결 검사 실패 · 기존 선택 유지" if not failure_lines.is_empty() else ("검증된 경로 %.0f m" % float(route.length_m) if route.ok else "도로 후보·방향 확인 → 역할 확정 → 경로 미리보기")
		status.text = summary + "\n" + feedback
		status.tooltip_text = technical_feedback if not technical_feedback.is_empty() else feedback
		for button in role_buttons: button.disabled = candidate_items.is_empty()
		var diagnostics: PackedStringArray = picker.call("block_diagnostics")
		if not diagnostics.is_empty():
			status.text += "\n연결 제한 %d건 · 상세 내용은 마우스를 올려 확인" % diagnostics.size()
			status.tooltip_text += "\n" + "\n".join(diagnostics)
	if tiles != null: _update_tiles()
	if camera.zoom.x != overlay_zoom: overlay_layer.queue_redraw()
	queue_redraw()

## 확정 블록과 현재 후보를 굵게 그리고 진행 화살표를 표시한다.
func _draw_overlay() -> void:
	super._draw_overlay()
	var scale_m := maxf(camera.zoom.x, 0.0005)
	for item in selected_blocks:
		overlay_layer.draw_polyline(item.line, Color("e9b95f"), 5.0 / scale_m, true)
	if focus_line.size() < 2:
		_draw_failure_lines(scale_m)
		return
	overlay_layer.draw_polyline(focus_line, Color("58e9cb"), 7.0 / scale_m, true)
	var i := maxi(0, (focus_line.size() - 2) / 2)
	var direction := (focus_line[i+1] - focus_line[i]).normalized()
	var center := focus_line[i].lerp(focus_line[i+1], 0.5)
	var tip := center + direction * 9.0 / scale_m
	var side := direction.orthogonal() * 6.0 / scale_m
	overlay_layer.draw_line(tip, center - direction * 5.0 / scale_m + side, Color.WHITE, 2.0 / scale_m)
	overlay_layer.draw_line(tip, center - direction * 5.0 / scale_m - side, Color.WHITE, 2.0 / scale_m)
	_draw_failure_lines(scale_m)

## 후보 강조 위에 실패한 연결 양끝을 붉게 덧그려 다른 도로와 구분한다.
func _draw_failure_lines(scale_m: float) -> void:
	for line in failure_lines:
		if line.size() >= 2: overlay_layer.draw_polyline(line, Color("ff5252"), 9.0 / scale_m, true)

## 왼쪽 사이드바를 제외한 지도 영역 가운데로 도로와 경로를 맞춘다.
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
