extends Node2D
## 네트워크 없는 지도 화면 뼈대. Rust 데이터가 오면 표시 레이어만 갱신한다.

var camera := Camera2D.new()
var roads := PackedVector2Array()
var usable := PackedByteArray()
var points := PackedVector2Array()
var route_line := PackedVector2Array()
var tile_layers: Array = []
var dragging := false
var status := Label.new()
var picker: Object
var tiles: Object
var feedback := ""
var textures: Dictionary = {}
## 타일 표시 프레임 번호. 텍스처마다 마지막으로 화면에 보인 번호를 기록해 오래된 것부터 버린다.
var tile_frame := 0
var save_button := Button.new()
var route_name := LineEdit.new()
var overwrite := CheckBox.new()
var snapshot_dialog := FileDialog.new()
## 스냅샷 세대 폴더 선택 창. 고른 폴더는 Rust가 그 안의 graph.osm.json으로 바꿔 연다.
var snapshot_folder_dialog := FileDialog.new()
## 지금 열린 지역 범위(지도 좌표). 열린 것이 없으면 크기 0.
var opened_region := Rect2()
## 전국 지도 자료(PLAN 4-11)가 있는지와 없을 때의 안내. 없으면 자동 지역 열기를 하지 않는다.
var data_ok := false
var data_error := ""
## 화면 가로가 이 길이(m)보다 좁게 확대하고 멈추면 화면 가운데 지역을 자동으로 연다(임시값).
const AUTO_OPEN_SPAN_M := 3000.0
## 카메라가 이 시간(초) 동안 움직이지 않으면 멈춘 것으로 본다.
const AUTO_OPEN_DELAY_S := 0.4
## 자동 지역 열기용 마지막 카메라 위치·배율과 멈춘 시간.
var last_camera := Vector2.INF
var last_zoom := 0.0
var still_time := 0.0
## 출발지·도착지 검색 칸(PLAN 4-11 2단계, 네이버 지도처럼 둘 다 검색해 정한다).
const SearchBox = preload("res://presentation/place_search_box.gd")
var start_box: HBoxContainer = SearchBox.new()
var end_box: HBoxContainer = SearchBox.new()
## 고른 출발·도착 {place, latlon, label, view, region}. 비어 있으면 아직 안 고름. 장소 번호는 고를 때 열린 지역(region)에서만 쓴다.
var start_choice: Dictionary = {}
var end_choice: Dictionary = {}
## 선으로 그리기(PLAN 4-11 2단계): 켜면 왼쪽 끌기로 선을 그리고, 놓으면 그 선을 따라 경로를 만든다.
var draw_toggle := CheckButton.new()
var drawing := false
var drawn := PackedVector2Array()
## 그릴 건물·장소 윤곽(점 장소는 점 하나, PLAN 4-9).
var place_outlines: Array = []
## 도로·건물·경로·점을 그리는 층(4-11 렉 수정). 카메라가 움직여도 다시 그리지 않고, 자료나 배율이 바뀔 때만 다시 그린다.
var overlay_layer := Node2D.new()
## 층을 마지막으로 그린 배율(선 굵기가 배율에 따라 바뀌므로 배율이 바뀌면 다시 그린다).
var overlay_zoom := 0.0
## 윤곽이 바뀔 때 한 번만 만드는 건물 그리기 자료: 면 삼각형(점·번호·색), 테두리 선분, 점 장소.
## 꼬이거나 겹친 실제 OSM 윤곽은 삼각형으로 나눌 수 없어 테두리만 그린다.
var place_fill_points := PackedVector2Array()
var place_fill_indices := PackedInt32Array()
var place_fill_colors := PackedColorArray()
var place_edges := PackedVector2Array()
var place_dots := PackedVector2Array()
## 도로 선분을 탐색 가능·불가로 나눠 둔 것(한 번에 그리기용).
var road_open := PackedVector2Array()
var road_closed := PackedVector2Array()
## 검색 결과로 옮길 때 화면에 담을 범위(m). 지역 열기 범위(1.5 km)와 맞춘다.
const SEARCH_VIEW_M := 1500.0
## 건물 윤곽·점 장소 색.
const PLACE_COLOR := Color("b48ead")
## Godot 쪽 지도 텍스처 보관 상한(계약 7-3 권장값).
const MAX_TEXTURES := 512
## 맞춤 보기에서 버튼·안내 영역으로 빼는 화면 크기(px). 최소 배율도 같은 영역 기준이다.
const FIT_MARGIN := Vector2(80, 420)
## 화면 위쪽 버튼·안내 영역 높이(px). 지도 북쪽 끝이 이 영역 아래까지 내려와 보이도록 이동 한계에 더한다.
const TOP_PANEL_PX := 340.0
## 대한민국 근사 경계 밖을 덮는 색. 주변 나라는 보이지 않게 하되(사용자 결정) 이질감이 없도록 OSM 지도 바다색을 쓴다(4-10).
const MASK_COLOR := Color("aad3df")
## 대한민국 근사 경계(지도 좌표). Rust SimTiles가 없으면 비어 있고 가림·제한을 하지 않는다.
var country_outline := PackedVector2Array()
## 경계를 감싸는 범위. 카메라 이동·축소 한계로 쓴다.
var country_bounds := Rect2()
## 경계 밖을 덮는 구멍 없는 다각형들(경계를 가운데 세로선으로 나눠 미리 계산).
var mask_polygons: Array[PackedVector2Array] = []

## 전국 범위를 담을 배율로 시작하고 출처 및 비활성 기능을 표시한다.
func _ready() -> void:
	camera.position = Vector2.ZERO
	camera.zoom = Vector2.ONE * 0.001
	add_child(camera)
	camera.make_current()
	add_child(overlay_layer)
	overlay_layer.draw.connect(_draw_overlay)
	get_viewport().size_changed.connect(queue_redraw)
	var canvas := CanvasLayer.new()
	add_child(canvas)
	status.position = Vector2(24, 196)
	status.text = "지도 미리보기 · Rust 지도 서비스 미연결\n우클릭 끌기: 이동 / 휠: 확대·축소\n지역 열기·점 편집·경로 계산·저장은 연결 대기 중입니다.\n회색 칸은 실제 지도 타일이 아닙니다."
	canvas.add_child(status)
	_services(canvas)
	_fit_country()
	var attribution := LinkButton.new()
	attribution.text = "© OpenStreetMap contributors"
	attribution.uri = "https://www.openstreetmap.org/copyright"
	attribution.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	attribution.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	attribution.grow_vertical = Control.GROW_DIRECTION_BEGIN
	attribution.offset_left = -280
	attribution.offset_top = -40
	attribution.offset_right = -16
	attribution.offset_bottom = -16
	var overlay := Control.new()
	overlay.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	overlay.mouse_filter = Control.MOUSE_FILTER_IGNORE
	canvas.add_child(overlay)
	overlay.add_child(attribution)
	queue_redraw()

## 도로·점·경로의 표시 스냅샷을 받아 복사하고, 도로 선분을 탐색 가능·불가로 나눠 그리기 층을 다시 그린다.
func display_geometry(segments: PackedVector2Array, flags: PackedByteArray, selected: PackedVector2Array, line: PackedVector2Array) -> void:
	if segments != roads or flags != usable:
		roads = segments.duplicate()
		usable = flags.duplicate()
		road_open.clear()
		road_closed.clear()
		for i in range(roads.size() / 2):
			var target := road_open if i < usable.size() and usable[i] == 1 else road_closed
			target.append(roads[i * 2])
			target.append(roads[i * 2 + 1])
	points = selected.duplicate()
	route_line = line.duplicate()
	overlay_layer.queue_redraw()
	queue_redraw()

## 건물 윤곽으로 그리기 자료(면 삼각형·테두리 선분·점 장소)를 한 번 만든다. 윤곽이 바뀔 때만 부른다.
func _rebuild_places() -> void:
	place_fill_points.clear()
	place_fill_indices.clear()
	place_edges.clear()
	place_dots.clear()
	for outline in place_outlines:
		if outline.size() == 1:
			place_dots.append(outline[0])
			continue
		if outline.size() < 3:
			continue
		for i in range(outline.size()):
			place_edges.append(outline[i])
			place_edges.append(outline[(i + 1) % outline.size()])
		var triangles := Geometry2D.triangulate_polygon(outline)
		var base := place_fill_points.size()
		place_fill_points.append_array(outline)
		for index in triangles:
			place_fill_indices.append(base + index)
	place_fill_colors.resize(place_fill_points.size())
	place_fill_colors.fill(Color(PLACE_COLOR, 0.18))
	overlay_layer.queue_redraw()

## 디코딩된 타일과 지도상의 사각형을 표시할 자리를 제공한다. 다운로드는 하지 않는다.
func display_tiles(layers: Array) -> void:
	tile_layers = layers.duplicate()
	queue_redraw()

## 마우스 기준 위치를 고정하며 확대하고 우클릭 끌기로 이동한다. 왼쪽 클릭은 출발·도착 고르기, "선으로 그리기"를 켜면
## 왼쪽 끌기로 선 그리기다.
func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_RIGHT:
			dragging = event.pressed
		elif event.button_index == MOUSE_BUTTON_LEFT and picker != null:
			var at := get_global_mouse_position()
			if draw_toggle.button_pressed:
				if event.pressed:
					_start_line(at)
				else:
					_finish_line()
			elif event.pressed:
				_click(at)
		elif event.pressed and event.button_index in [MOUSE_BUTTON_WHEEL_UP, MOUSE_BUTTON_WHEEL_DOWN]:
			var before := get_global_mouse_position()
			var factor := 1.25 if event.button_index == MOUSE_BUTTON_WHEEL_UP else 0.8
			camera.zoom = Vector2.ONE * clampf(camera.zoom.x * factor, _min_zoom(), 8.0)
			camera.force_update_scroll()
			camera.position += before - get_global_mouse_position()
			_clamp_camera()
			queue_redraw()
	elif event is InputEventMouseMotion and drawing:
		_extend_line(get_global_mouse_position())
	elif event is InputEventMouseMotion and dragging:
		camera.position -= event.relative / camera.zoom.x
		_clamp_camera()
		queue_redraw()

## 포커스 상실 시 지도 끌기를 끝낸다.
func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		dragging = false

## 현재 뷰의 회색 칸, 타일, 바깥 바다색, 열린 지역 테두리, 자동 열기 기준 십자선을 그린다(가볍다, 매 프레임).
## 도로·건물·경로·점은 _draw_overlay가 따로 그린다.
func _draw() -> void:
	var scale_m := maxf(camera.zoom.x, 0.0005)
	var span := get_viewport_rect().size / scale_m
	var origin := camera.position - span * 0.5
	var cell := 256.0 / scale_m
	var grid_origin := Vector2(floorf(origin.x / cell), floorf(origin.y / cell)) * cell
	for x in range(int(ceilf(span.x / cell)) + 2):
		for y in range(int(ceilf(span.y / cell)) + 2):
			var rect := Rect2(grid_origin + Vector2(x, y) * cell, Vector2.ONE * cell)
			draw_rect(rect, Color("343e48"))
			draw_rect(rect, Color("46515c"), false, 1.0 / scale_m)
	for tile in tile_layers:
		if tile.get("texture") is Texture2D:
			draw_texture_rect(tile.texture, tile.rect, false)
	# 근사 경계선은 바다 위에 가짜 선이 보이므로 그리지 않는다(4-10).
	for polygon in mask_polygons:
		draw_colored_polygon(polygon, MASK_COLOR)
	if opened_region.has_area():
		draw_rect(opened_region, Color("79d9b1"), false, 2.0 / scale_m)
	# 자동 지역 열기가 기준으로 쓰는 화면 가운데(카메라 위치).
	var center := camera.position
	draw_line(center - Vector2(10, 0) / scale_m, center + Vector2(10, 0) / scale_m, Color.WHITE, 1.0 / scale_m)
	draw_line(center - Vector2(0, 10) / scale_m, center + Vector2(0, 10) / scale_m, Color.WHITE, 1.0 / scale_m)

## 건물 면(미리 나눈 삼각형 한 번에)·테두리·점 장소, 도로, 경로, 선택 점을 그리기 층에 그린다. 자료·배율이 바뀔 때만 불린다.
func _draw_overlay() -> void:
	var scale_m := maxf(camera.zoom.x, 0.0005)
	overlay_zoom = camera.zoom.x
	if not place_fill_indices.is_empty():
		RenderingServer.canvas_item_add_triangle_array(overlay_layer.get_canvas_item(), place_fill_indices, place_fill_points, place_fill_colors)
	if not place_edges.is_empty():
		overlay_layer.draw_multiline(place_edges, PLACE_COLOR, 1.0 / scale_m)
	if not place_dots.is_empty():
		# 점 장소는 짧고 굵은 선분(작은 네모)으로 한 번에 그린다.
		var dots := PackedVector2Array()
		var half := Vector2(3.0 / scale_m, 0)
		for dot in place_dots:
			dots.append(dot - half)
			dots.append(dot + half)
		overlay_layer.draw_multiline(dots, PLACE_COLOR, 6.0 / scale_m)
	if not road_closed.is_empty():
		overlay_layer.draw_multiline(road_closed, Color("737e88"), 2.0 / scale_m)
	if not road_open.is_empty():
		overlay_layer.draw_multiline(road_open, Color("cbd5dd"), 2.0 / scale_m)
	if route_line.size() > 1:
		overlay_layer.draw_polyline(route_line, Color("58c4ef"), 4.0 / scale_m, true)
	# 출발(주황)·도착(초록)만 크게 표시한다. 그린 선의 중간 점은 경로선으로 보인다.
	if points.size() >= 1:
		overlay_layer.draw_circle(points[0], 6.0 / scale_m, Color("f3bf63"))
	if points.size() >= 2:
		overlay_layer.draw_circle(points[points.size() - 1], 6.0 / scale_m, Color("9ee5c2"))
	if drawn.size() > 1:
		overlay_layer.draw_polyline(drawn, Color("f39c63"), 3.0 / scale_m, true)

## Rust 지도 서비스(SimRoutePicker·SimTiles)를 만들고 편집·저장·맞춤 보기·검색 버튼을 화면에 붙인다. 헤드리스·--offline이면
## 네트워크(배경 지도·전국 검색)를 끈다. 지역 도로·건물은 내장 전국 자료에서 열므로 네트워크가 필요 없다(PLAN 4-11).
func _services(canvas: CanvasLayer) -> void:
	var offline := DisplayServer.get_name() == "headless" or "--offline" in OS.get_cmdline_user_args()
	if ClassDB.class_exists("SimRoutePicker"):
		picker = ClassDB.instantiate("SimRoutePicker")
		picker.set("network", not offline)
		_check_data()
	if ClassDB.class_exists("SimTiles"):
		tiles = ClassDB.instantiate("SimTiles")
		tiles.set("offline", offline)
		if tiles.has_method("country_outline"):
			_set_country(tiles.call("country_outline"), tiles.call("country_bounds"))
	var controls := HBoxContainer.new()
	controls.position = Vector2(24, 64)
	canvas.add_child(controls)
	var clear := Button.new()
	clear.text = "경로 지우기"
	clear.pressed.connect(_clear_route)
	controls.add_child(clear)
	draw_toggle.text = "선으로 그리기"
	draw_toggle.tooltip_text = "켜면 왼쪽 끌기로 도로를 따라 선을 그려 경로를 만듭니다"
	controls.add_child(draw_toggle)
	route_name.placeholder_text = "저장 이름 (영문·숫자·_-)"
	route_name.custom_minimum_size.x = 210
	controls.add_child(route_name)
	overwrite.text = "덮어쓰기"
	controls.add_child(overwrite)
	save_button.text = "경로 저장"
	save_button.pressed.connect(_save_route)
	controls.add_child(save_button)
	var navigation := HBoxContainer.new()
	navigation.position = Vector2(24, 108)
	canvas.add_child(navigation)
	for item in [["전체 지도", _fit_country], ["선택 지역", _fit_region], ["경로", _fit_route], ["스냅샷 파일 열기", _open_snapshot], ["스냅샷 폴더 열기", _open_snapshot_folder]]:
		var button := Button.new()
		button.text = item[0]
		button.pressed.connect(item[1])
		navigation.add_child(button)
	var ends := HBoxContainer.new()
	ends.position = Vector2(24, 152)
	canvas.add_child(ends)
	for pair in [["출발", start_box, "출발지 검색", "start"], ["도착", end_box, "도착지 검색", "end"]]:
		var label := Label.new()
		label.text = pair[0]
		ends.add_child(label)
		pair[1].setup(picker, pair[2])
		pair[1].picked.connect(_choose.bind(pair[3]))
		ends.add_child(pair[1])
	snapshot_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	snapshot_dialog.access = FileDialog.ACCESS_FILESYSTEM
	snapshot_dialog.filters = PackedStringArray(["*.json ; OSM snapshot"])
	snapshot_dialog.file_selected.connect(_load_snapshot)
	add_child(snapshot_dialog)
	snapshot_folder_dialog.file_mode = FileDialog.FILE_MODE_OPEN_DIR
	snapshot_folder_dialog.access = FileDialog.ACCESS_FILESYSTEM
	snapshot_folder_dialog.dir_selected.connect(_load_snapshot)
	add_child(snapshot_folder_dialog)
	clear.disabled = picker == null
	draw_toggle.disabled = picker == null

## 전국 지도 자료가 있는지 Rust에 묻고 결과를 기억한다. 없으면 만드는 방법을 안내에 남긴다.
func _check_data() -> void:
	var result: Dictionary = picker.call("data_status")
	data_ok = bool(result.get("ok", false))
	data_error = str(result.get("error", ""))

## 지도 좌표 `view`를 가운데로 하는 1.5 km 지역의 도로·건물을 전국 자료에서 바로 열고 표시를 갱신한다. 열었으면 true.
func _open_region_at(view: Vector2) -> bool:
	if picker == null:
		return false
	var latlon: PackedFloat64Array = picker.call("view_to_latlon", view)
	if latlon.size() != 2:
		return false
	var result: Dictionary = picker.call("open_region", latlon)
	feedback = "" if result.get("ok", false) else str(result.error)
	_refresh_route()
	return bool(result.get("ok", false))

## 확대한 화면이 멈추면 그 가운데 지역을 자동으로 연다. 점을 찍은 뒤에는 지역을 바꾸지 않는다(찍은 점 보존).
func _auto_open(delta: float) -> void:
	if picker == null or not data_ok or not points.is_empty() or drawing:
		return
	if camera.position != last_camera or camera.zoom.x != last_zoom:
		last_camera = camera.position
		last_zoom = camera.zoom.x
		still_time = 0.0
		return
	var before := still_time
	still_time += delta
	if before >= AUTO_OPEN_DELAY_S or still_time < AUTO_OPEN_DELAY_S:
		return
	if get_viewport_rect().size.x / camera.zoom.x > AUTO_OPEN_SPAN_M:
		return
	if opened_region.has_area() and opened_region.grow(-opened_region.size.x * 0.25).has_point(camera.position):
		return
	_open_region_at(camera.position)

## 지도 클릭으로 출발·도착을 고른다(비어 있는 쪽, 출발 먼저, 둘 다 있으면 도착). 열린 지역이 없고 경로도 없으면 그곳 지역을
## 먼저 연다. 건물·장소를 누르면 그 장소(진입 지점), 아니면 누른 곳(가까운 도로 위 점)이다.
func _click(at: Vector2) -> void:
	if not opened_region.has_point(at) and points.is_empty() and data_ok:
		_open_region_at(at)
	var place := int(picker.call("place_at", at))
	var label: String = str(picker.call("place_label", place)) if place >= 0 else "지도에서 고른 곳"
	var choice := {"place": place, "latlon": picker.call("view_to_latlon", at), "label": label, "view": at}
	var side := "start" if start_choice.is_empty() else "end"
	(start_box if side == "start" else end_box).show_label(label)
	_choose(choice, side)

## 출발(`side` = "start") 또는 도착을 정한다. 고를 때 열린 지역을 함께 기억하고(장소 번호는 그 지역에서만 쓴다),
## 화면 밖이면 지도를 옮긴다. 둘 다 정해지면 경로를 계산한다.
func _choose(choice: Dictionary, side: String) -> void:
	var chosen := choice.duplicate()
	chosen["region"] = opened_region
	if chosen.view == Vector2.INF and int(chosen.place) >= 0 and int(chosen.place) < place_outlines.size():
		chosen.view = _outline_center(place_outlines[int(chosen.place)])
	if side == "start":
		start_choice = chosen
	else:
		end_choice = chosen
	if chosen.view != Vector2.INF and not _visible_rect().has_point(chosen.view):
		_move_to(chosen.view)
	if start_choice.is_empty() or end_choice.is_empty():
		feedback = "도착지를 고르세요" if end_choice.is_empty() else "출발지를 고르세요"
		return
	_plan_endpoints()

## 고른 출발·도착으로 Rust에 경로를 계산시키고, 되면 경로가 보이게 화면을 맞춘다.
func _plan_endpoints() -> void:
	var result: Dictionary = picker.call("plan_endpoints", _end_dict(start_choice), _end_dict(end_choice))
	feedback = "" if result.get("ok", false) else str(result.error)
	_refresh_route()
	if result.get("ok", false):
		_fit_route()

## 고른 끝을 Rust에 넘길 사전으로 바꾼다. 장소 번호는 고를 때와 같은 지역이 열려 있을 때만 쓰고, 아니면 위치로 넘긴다.
func _end_dict(choice: Dictionary) -> Dictionary:
	if int(choice.place) >= 0 and choice.region == opened_region:
		return {"place": int(choice.place), "latlon": PackedFloat64Array(), "label": choice.label}
	var latlon: PackedFloat64Array = choice.latlon
	if latlon.size() != 2:
		latlon = picker.call("view_to_latlon", choice.view)
	return {"place": -1, "latlon": latlon, "label": choice.label}

## 윤곽 꼭짓점의 평균(점 장소는 그 점).
func _outline_center(outline: PackedVector2Array) -> Vector2:
	var center := Vector2.ZERO
	for point in outline:
		center += point / outline.size()
	return center

## 지금 화면에 보이는 지도 범위.
func _visible_rect() -> Rect2:
	var span := get_viewport_rect().size / camera.zoom.x
	return Rect2(camera.position - span * 0.5, span)

## 지도를 `view`로 옮기고 약 1.5 km가 보이게 확대한다.
func _move_to(view: Vector2) -> void:
	var available := get_viewport_rect().size - FIT_MARGIN
	if available.x > 0.0 and available.y > 0.0:
		camera.zoom = Vector2.ONE * clampf(minf(available.x, available.y) / SEARCH_VIEW_M, _min_zoom(), 8.0)
	camera.position = view
	_clamp_camera()
	queue_redraw()

## 선 그리기를 시작한다(왼쪽 누름).
func _start_line(at: Vector2) -> void:
	drawing = true
	drawn = PackedVector2Array([at])
	overlay_layer.queue_redraw()

## 그리는 선에 점을 더한다(화면 4 px보다 움직였을 때만).
func _extend_line(at: Vector2) -> void:
	if drawn.is_empty() or at.distance_to(drawn[drawn.size() - 1]) * camera.zoom.x >= 4.0:
		drawn.append(at)
		overlay_layer.queue_redraw()

## 선 그리기를 끝내고(왼쪽 뗌) 그 선을 따라 Rust에 경로를 계산시킨다. 출발·도착 칸에는 선의 처음·끝이라고 보인다.
func _finish_line() -> void:
	if not drawing:
		return
	drawing = false
	var line := drawn
	drawn = PackedVector2Array()
	if line.size() < 2:
		overlay_layer.queue_redraw()
		return
	var result: Dictionary = picker.call("plan_line", line)
	feedback = "" if result.get("ok", false) else str(result.error)
	if result.get("ok", false):
		start_choice = {}
		end_choice = {}
		start_box.show_label("(그린 선의 처음)")
		end_box.show_label("(그린 선의 끝)")
	_refresh_route()

## 경로와 고른 출발·도착을 모두 지운다.
func _clear_route() -> void:
	if picker == null:
		return
	picker.call("clear_points")
	start_choice = {}
	end_choice = {}
	start_box.show_label("")
	end_box.show_label("")
	feedback = ""
	_refresh_route()

## 입력한 이름과 덮어쓰기 선택으로 경로를 저장하고 결과(경로 또는 오류)를 안내에 남긴다.
func _save_route() -> void:
	if picker == null:
		return
	var result: Dictionary = picker.call("save", route_name.text, overwrite.button_pressed)
	feedback = "저장: " + str(result.path) if result.get("ok", false) else str(result.error)

## Rust에서 열린 지역 범위·도로 선분·사용 가능 여부·건물 윤곽·점·경로선을 받아 그리기 자료를 바꾼다.
## 건물 윤곽이 바뀌었을 때만 면을 칠할 수 있는지 다시 판정한다(매 프레임 판정하지 않는다).
func _refresh_route() -> void:
	opened_region = picker.call("loaded_region")
	var outlines: Array = picker.call("place_outlines")
	if outlines != place_outlines:
		place_outlines = outlines
		_rebuild_places()
	var result: Dictionary = picker.call("route")
	display_geometry(picker.call("road_segments"), picker.call("road_usable"), picker.call("points"), result.line)

## 매 프레임 전국 검색 결과를 처리하고, 멈춘 화면의 지역을 자동으로 열며, 버튼 상태·안내를 갱신하고 배경 타일을 요청한다.
func _process(delta: float) -> void:
	if picker != null:
		for event in picker.call("poll"):
			if event.type == "search_results" and not start_box.take_results(event):
				end_box.take_results(event)
		_auto_open(delta)
		var result: Dictionary = picker.call("route")
		save_button.disabled = not bool(result.ok)
		var hint := "" if data_ok else "\n" + data_error
		var searches := ""
		for pair in [["출발 검색", start_box], ["도착 검색", end_box]]:
			if pair[1].feedback != "":
				searches += "\n%s: %s" % [pair[0], pair[1].feedback]
		var how := "선 그리기: 왼쪽 끌기로 도로를 따라 그리세요" if draw_toggle.button_pressed else "클릭: 출발·도착 고르기(건물을 누르면 그 건물)"
		var text := "우클릭: 이동 / 휠: 확대(멈추면 그 지역을 엽니다) / " + how + "\n" + "\n".join(picker.call("status_lines")) + "\n" + feedback + searches + hint
		# 글자가 바뀔 때만 넣는다(같은 글자를 매 프레임 넣으면 글자 배치를 다시 한다).
		if status.text != text:
			status.text = text
	else:
		save_button.disabled = true
	if tiles != null:
		_update_tiles()
	if camera.zoom.x != overlay_zoom:
		overlay_layer.queue_redraw()
	queue_redraw()

## 보이는 범위의 타일을 요청하고 받은 PNG를 텍스처로 만든다. 디코딩 실패는 report_bad로 알린다.
func _update_tiles() -> void:
	var span := get_viewport_rect().size / camera.zoom.x
	var visible := Rect2(camera.position - span * 0.5, span)
	var wanted := visible.intersection(country_bounds) if country_bounds.has_area() else visible
	if wanted.has_area():
		tiles.call("request_visible", wanted.position, wanted.end, 1.0 / camera.zoom.x)
	for tile in tiles.call("poll"):
		if tile.status != "ok":
			feedback = "배경 지도: " + str(tile.error)
			continue
		var image := Image.new()
		if image.load_png_from_buffer(tile.png) != OK:
			tiles.call("report_bad", tile.x, tile.y, tile.z)
			continue
		var key := "%d/%d/%d" % [tile.z, tile.x, tile.y]
		textures[key] = {"texture": ImageTexture.create_from_image(image), "rect": tile.rect, "x": tile.x, "y": tile.y, "z": tile.z, "seen": tile_frame}
	tile_frame += 1
	tile_layers = []
	for tile in textures.values():
		if visible.intersects(tile.rect):
			tile.seen = tile_frame
			tile_layers.append(tile)
	# 낮은 확대 수준부터 그려 더 자세한 타일이 위에 오게 한다.
	tile_layers.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return a.z < b.z)
	if textures.size() > MAX_TEXTURES:
		_evict_textures()
	if int(tiles.call("paused_until_s")) > int(Time.get_unix_time_from_system()):
		status.text += "\n지도 서버 요청 일시 중지"

## 화면을 떠날 때 서비스 객체와 텍스처를 놓는다.
func _exit_tree() -> void:
	picker = null
	tiles = null
	textures.clear()
	tile_layers.clear()

## 전국이 보이는 처음 위치·배율로 돌아간다.
func _fit_country() -> void:
	if country_bounds.has_area():
		_fit_rect(country_bounds)
		return
	camera.position = Vector2.ZERO
	camera.zoom = Vector2.ONE * 0.001

## 지금 열린 지역 범위가 화면에 들어오게 맞춘다.
func _fit_region() -> void:
	if picker != null:
		_fit_rect(picker.call("loaded_region"))

## 계산된 경로선이 25 m 여백과 함께 화면에 들어오게 맞춘다.
func _fit_route() -> void:
	if route_line.is_empty():
		return
	var bounds := Rect2(route_line[0], Vector2.ZERO)
	for point in route_line:
		bounds = bounds.expand(point)
	_fit_rect(bounds.grow(25.0))

## 주어진 지도 범위가 버튼 영역을 뺀 화면에 들어오도록 배율과 가운데를 정한다.
func _fit_rect(bounds: Rect2) -> void:
	if bounds.size.x <= 0.0 or bounds.size.y <= 0.0:
		return
	var available := get_viewport_rect().size - FIT_MARGIN
	if available.x <= 0.0 or available.y <= 0.0:
		return
	var zoom_value := clampf(minf(available.x / bounds.size.x, available.y / bounds.size.y), 0.0005, 8.0)
	camera.zoom = Vector2.ONE * zoom_value
	camera.position = bounds.get_center() - Vector2(0, TOP_PANEL_PX * 0.5 / zoom_value)
	_clamp_camera()

## 로컬 스냅샷(graph*.osm.json) 파일 선택 창을 연다.
func _open_snapshot() -> void:
	if picker != null:
		snapshot_dialog.popup_centered_ratio(0.75)

## 고른 스냅샷을 열고 성공하면 표시를 갱신해 그 지역에 맞춘다.
func _load_snapshot(path: String) -> void:
	if picker == null:
		return
	var result: Dictionary = picker.call("load_snapshot", path)
	feedback = "스냅샷을 열었습니다." if result.get("ok", false) else str(result.error)
	if result.get("ok", false):
		_refresh_route()
		_fit_region()

## 스냅샷 세대 폴더 선택 창을 연다.
func _open_snapshot_folder() -> void:
	if picker != null:
		snapshot_folder_dialog.popup_centered_ratio(0.75)

## 텍스처가 상한을 넘으면 이번 프레임에 보이지 않은 것 중 가장 오래 안 보인 것부터 버리고 Rust에 재전달을 허용한다.
func _evict_textures() -> void:
	var hidden: Array = []
	for key in textures:
		if textures[key].seen != tile_frame:
			hidden.append([textures[key].seen, key])
	hidden.sort()
	var extra := textures.size() - MAX_TEXTURES
	for item in hidden.slice(0, extra):
		var tile: Dictionary = textures[item[1]]
		tiles.call("forget_displayed", tile.x, tile.y, tile.z)
		textures.erase(item[1])

## Rust가 준 대한민국 근사 경계를 기록하고, 그 밖을 덮을 구멍 없는 다각형을 만든다.
## 큰 사각형을 경계 가운데 세로선으로 둘로 나눠 각각에서 경계를 빼면 결과에 구멍이 생기지 않는다.
func _set_country(outline: PackedVector2Array, bounds: Rect2) -> void:
	if outline.size() < 3 or not bounds.has_area():
		return
	country_outline = outline
	country_bounds = bounds
	mask_polygons.clear()
	var margin := maxf(bounds.size.x, bounds.size.y) * 20.0
	var big := bounds.grow(margin)
	var middle := bounds.get_center().x
	for half in [Rect2(big.position, Vector2(middle - big.position.x, big.size.y)), Rect2(Vector2(middle, big.position.y), Vector2(big.end.x - middle, big.size.y))]:
		var corners := PackedVector2Array([half.position, Vector2(half.end.x, half.position.y), half.end, Vector2(half.position.x, half.end.y)])
		for piece in Geometry2D.clip_polygons(corners, outline):
			mask_polygons.append(piece)

## 대한민국 범위가 화면(위 버튼 영역 제외)을 가로·세로 모두 꽉 채우는 최소 배율. 경계가 없으면 기존 한계.
func _min_zoom() -> float:
	if not country_bounds.has_area():
		return 0.0005
	var screen := get_viewport_rect().size
	return clampf(maxf(screen.x / country_bounds.size.x, (screen.y - TOP_PANEL_PX) / country_bounds.size.y), 0.0005, 8.0)

## 배율을 최소 배율 이상으로, 화면 가장자리를 대한민국 범위 안으로 묶는다. 위쪽은 버튼 영역만큼 더 허용한다.
func _clamp_camera() -> void:
	if not country_bounds.has_area():
		return
	if camera.zoom.x < _min_zoom():
		camera.zoom = Vector2.ONE * _min_zoom()
	var half := get_viewport_rect().size * 0.5 / camera.zoom.x
	var low := country_bounds.position + half - Vector2(0, TOP_PANEL_PX / camera.zoom.x)
	var high := country_bounds.end - half
	camera.position = Vector2(_clamp_or_middle(camera.position.x, low.x, high.x), _clamp_or_middle(camera.position.y, low.y, high.y))

## value를 [low, high]로 묶는다. 화면이 범위보다 넓어 low > high이면 두 값의 가운데를 쓴다.
func _clamp_or_middle(value: float, low: float, high: float) -> float:
	return (low + high) * 0.5 if low > high else clampf(value, low, high)
