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

## 기존 지도 서비스 위에 블록 후보·확정·경유 편집 제어를 붙인다.
func _services(canvas: CanvasLayer) -> void:
	super._services(canvas)
	for button in toolbar.find_children("*", "Button", true, false):
		if button.text == "도착 취소": button.text = "마지막 경유 취소"
	for label in toolbar.find_children("*", "Label", true, false):
		if label.text.contains("지역 자동 연결"): label.text = "전국 OSM · 경로 최대 10 km · 블록/방향 선택"
	for button in endpoint_buttons: button.hide()
	endpoint_label.hide()
	restricted_roads.hide()
	overwrite.hide()
	var row := HFlowContainer.new()
	toolbar.add_child(row)
	candidate_list.custom_minimum_size.x = 360
	candidate_list.fit_to_longest_item = false
	candidate_list.clip_text = true
	candidate_list.item_selected.connect(_focus_candidate)
	row.add_child(candidate_list)
	for role in [["출발로 확정", "start"], ["경유 추가", "via"], ["도착으로 확정", "goal"]]:
		var button := Button.new()
		button.text = role[0]
		button.pressed.connect(_confirm_block.bind(role[1]))
		row.add_child(button)
	preview_button.text = "경로 미리보기"
	preview_button.pressed.connect(_preview_blocks)
	row.add_child(preview_button)
	span.add_item("지역 1.5 km", 1500)
	span.add_item("지역 5 km", 5000)
	span.add_item("지역 10 km", 10000)
	span.tooltip_text = "선택 전에 열 지역 크기를 고르세요. 경로 길이는 최대 10 km입니다."
	row.add_child(span)
	for preset in [["엑스포 내부 실제 표본", "expo-internal"], ["엑스포 곡선 실제 표본", "expo-curved-joint"], ["강남 실제 표본", "gangnam-working-area"]]:
		var button := Button.new()
		button.text = preset[0]
		button.tooltip_text = "오늘 검증용으로 범위를 고정한 실제 OSM 도로 표본입니다. 지역 전체가 아닙니다."
		button.pressed.connect(_load_snapshot.bind("testdata/block-real/%s.osm.json" % preset[1]))
		row.add_child(button)
	selection_list.custom_minimum_size.y = 66
	selection_list.tooltip_text = "출발·경유·도착 목록. 경유를 고르고 순서 이동 또는 삭제하세요."
	toolbar.add_child(selection_list)
	var edit := HFlowContainer.new()
	toolbar.add_child(edit)
	for item in [["경유 ↑", -1], ["경유 ↓", 1], ["경유 삭제", 0]]:
		var button := Button.new()
		button.text = item[0]
		button.pressed.connect(_edit_via.bind(item[1]))
		edit.add_child(button)
	var note := Label.new()
	note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	note.custom_minimum_size.x = 260
	note.text = "도로 블록·방향 선택 · 제한 통로 포함(연습용) · 지역 보정은 기존 점 경로 전용"
	edit.add_child(note)
	candidate_list.add_item("도로를 클릭해 블록과 방향 후보 보기")
	candidate_list.disabled = true

## 클릭은 후보를 조회하며 Shift 클릭도 임의로 도로 선택을 삭제하지 않는다.
func _select_map_position(screen_position: Vector2, _remove: bool) -> void:
	var at: Vector2 = get_canvas_transform().affine_inverse() * screen_position
	if _ensure_region(at): _show_candidates(at)

## 가까운 평행 도로와 양방향을 모두 보여 주고 첫 후보는 강조만 한다.
func _show_candidates(at: Vector2) -> void:
	candidate_items = picker.call("block_candidates", at, clampf(12.0 / camera.zoom.x, 2.0, 300.0))
	candidate_list.clear()
	for item in candidate_items:
		candidate_list.add_item("%s · %.0f m · 폭 %.1f m · %s→%s · way %s" % [item.name, item.length_m, item.width_m, item.from_node, item.to_node, item.way_id])
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
	feedback = success_message if result.ok else str(result.error)
	overlay_layer.queue_redraw()

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
		picker.call("edit_block_via", vias[-1].index, 0)
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
		selection_list.add_item("%s · %s · %s→%s · way %s" % [role, item.name, item.from_node, item.to_node, item.way_id])
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
		status.text = ("검증된 경로 %.0f m" % float(route.length_m) if route.ok else "도로 후보·방향 확인 → 역할 확정 → 경로 미리보기") + "\n" + feedback
		status.tooltip_text = feedback
		var diagnostics: PackedStringArray = picker.call("block_diagnostics")
		if not diagnostics.is_empty():
			status.text += "\n회전 제한 참조 불일치 %d건 · 해당 진입 연결 차단(자세한 내용은 마우스를 올려 확인)" % diagnostics.size()
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
	_draw_failure_lines(scale_m)
	if focus_line.size() < 2: return
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

## 실제 도구 줄 아래의 남은 지도 영역에 표본 전체가 보이게 맞춘다.
func _fit_rect(bounds: Rect2) -> void:
	if bounds.size.x <= 0.0 or bounds.size.y <= 0.0: return
	var view := get_viewport_rect().size
	var top := maxf(180.0, toolbar_panel.get_global_rect().end.y + 20.0)
	var available := Vector2(view.x - 60.0, view.y - top - 30.0)
	if available.x <= 0.0 or available.y <= 0.0: return
	var value := clampf(minf(available.x / bounds.size.x, available.y / bounds.size.y), 0.0005, 8.0)
	camera.zoom = Vector2.ONE * value
	camera.position = bounds.get_center() - Vector2(0, top * 0.5 / value)
	_clamp_camera()
	last_camera = camera.position
	last_zoom = camera.zoom.x
	still_time = AUTO_OPEN_DELAY_S
