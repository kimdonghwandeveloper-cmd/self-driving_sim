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
var active_request := 0
var feedback := ""
var textures: Dictionary = {}
## 타일 표시 프레임 번호. 텍스처마다 마지막으로 화면에 보인 번호를 기록해 오래된 것부터 버린다.
var tile_frame := 0
var receive := Button.new()
var save_button := Button.new()
var route_name := LineEdit.new()
var overwrite := CheckBox.new()
var snapshot_dialog := FileDialog.new()
## 스냅샷 세대 폴더 선택 창. 고른 폴더는 Rust가 그 안의 graph.osm.json으로 바꿔 연다.
var snapshot_folder_dialog := FileDialog.new()
## 디스크 캐시에 받아 둔 지역 범위(지도 좌표)와 그 목록 선택 상자.
var cached: Array[Rect2] = []
var cache_choices := OptionButton.new()
## 지금 열린 지역 범위(지도 좌표). 열린 것이 없으면 크기 0.
var opened_region := Rect2()
## 건물·장소(PLAN 4-9): 받기 버튼, 이름 검색 칸, 검색 결과 상자, 그릴 윤곽(점 장소는 점 하나).
var places_button := Button.new()
var place_search := LineEdit.new()
var place_results := OptionButton.new()
var place_outlines: Array = []
## 검색 결과 상자 항목별 내용 {source: "place"(받은 건물·장소)|"area"(전국 검색), index, view}. 안내문·구분선은 null.
var result_items: Array = []
## 받은 건물·장소 검색 결과와 마지막 전국 검색 결과(4-10).
var local_results: Array = []
var area_results: Array = []
## 기다리는 전국 검색 요청 번호(0이면 없음). 지역 받기 요청과 따로 센다.
var search_request := 0
## 전국 검색 결과로 옮길 때 화면에 담을 범위(m). 지역 받기 범위(1.5 km)와 맞춘다.
const SEARCH_VIEW_M := 1500.0
## 건물 윤곽·점 장소 색.
const PLACE_COLOR := Color("b48ead")
## Godot 쪽 지도 텍스처 보관 상한(계약 7-3 권장값).
const MAX_TEXTURES := 512
## 맞춤 보기에서 버튼·안내 영역으로 빼는 화면 크기(px). 최소 배율도 같은 영역 기준이다.
const FIT_MARGIN := Vector2(80, 380)
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
	get_viewport().size_changed.connect(queue_redraw)
	var canvas := CanvasLayer.new()
	add_child(canvas)
	status.position = Vector2(24, 152)
	status.text = "지도 미리보기 · Rust 지도 서비스 미연결\n우클릭 끌기: 이동 / 휠: 확대·축소\n지역 받기·점 편집·경로 계산·저장은 연결 대기 중입니다.\n회색 칸은 실제 지도 타일이 아닙니다."
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

## 도로·점·경로의 표시 스냅샷을 받아 복사한다. 계산이나 편집은 하지 않는다.
func display_geometry(segments: PackedVector2Array, flags: PackedByteArray, selected: PackedVector2Array, line: PackedVector2Array) -> void:
	roads = segments.duplicate()
	usable = flags.duplicate()
	points = selected.duplicate()
	route_line = line.duplicate()
	queue_redraw()

## 디코딩된 타일과 지도상의 사각형을 표시할 자리를 제공한다. 다운로드는 하지 않는다.
func display_tiles(layers: Array) -> void:
	tile_layers = layers.duplicate()
	queue_redraw()

## 마우스 기준 위치를 고정하며 확대하고 우클릭 끌기로 이동한다.
func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_RIGHT:
			dragging = event.pressed
		elif event.pressed and event.button_index == MOUSE_BUTTON_LEFT and picker != null:
			var at := get_global_mouse_position()
			if event.shift_pressed:
				picker.call("remove_near", at, 12.0 / camera.zoom.x)
			else:
				# 건물·장소를 누르면 그 진입 지점(가까운 도로 위 점)을, 아니면 누른 자리를 점으로 더한다.
				var place := int(picker.call("place_at", at))
				if place >= 0:
					_add_place(place)
				else:
					picker.call("add_point", at)
			_refresh_route()
		elif event.pressed and event.button_index in [MOUSE_BUTTON_WHEEL_UP, MOUSE_BUTTON_WHEEL_DOWN]:
			var before := get_global_mouse_position()
			var factor := 1.25 if event.button_index == MOUSE_BUTTON_WHEEL_UP else 0.8
			camera.zoom = Vector2.ONE * clampf(camera.zoom.x * factor, _min_zoom(), 8.0)
			camera.force_update_scroll()
			camera.position += before - get_global_mouse_position()
			_clamp_camera()
			queue_redraw()
	elif event is InputEventMouseMotion and dragging:
		camera.position -= event.relative / camera.zoom.x
		_clamp_camera()
		queue_redraw()

## 포커스 상실 시 지도 끌기를 끝낸다.
func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		dragging = false

## 현재 뷰의 회색 칸, 타일, 받은 지역 테두리, 받기 기준 십자선, 도로, 경로, 선택 점을 순서대로 표시한다.
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
	for bounds in cached:
		draw_rect(bounds, Color("657f99"), false, 1.0 / scale_m)
	if opened_region.has_area():
		draw_rect(opened_region, Color("79d9b1"), false, 2.0 / scale_m)
	# "이 지역 도로 받기"가 기준으로 쓰는 화면 가운데(카메라 위치).
	var center := camera.position
	draw_line(center - Vector2(10, 0) / scale_m, center + Vector2(10, 0) / scale_m, Color.WHITE, 1.0 / scale_m)
	draw_line(center - Vector2(0, 10) / scale_m, center + Vector2(0, 10) / scale_m, Color.WHITE, 1.0 / scale_m)
	for outline in place_outlines:
		if outline.size() == 1:
			draw_circle(outline[0], 4.0 / scale_m, PLACE_COLOR)
		elif outline.size() > 2:
			var closed: PackedVector2Array = outline.duplicate()
			closed.append(outline[0])
			draw_colored_polygon(outline, Color(PLACE_COLOR, 0.18))
			draw_polyline(closed, PLACE_COLOR, 1.0 / scale_m, true)
	for i in range(roads.size() / 2):
		var allowed := i < usable.size() and usable[i] == 1
		draw_line(roads[i * 2], roads[i * 2 + 1], Color("cbd5dd") if allowed else Color("737e88"), 2.0 / scale_m, true)
	if route_line.size() > 1:
		draw_polyline(route_line, Color("58c4ef"), 4.0 / scale_m, true)
	for i in range(points.size()):
		draw_circle(points[i], 6.0 / scale_m, Color("f3bf63") if i == 0 else Color("9ee5c2"))

## Rust 지도 서비스(SimRoutePicker·SimTiles)를 만들고 받기·편집·저장·맞춤 보기 버튼을 화면에 붙인다. 헤드리스·--offline이면 네트워크를 끈다.
func _services(canvas: CanvasLayer) -> void:
	var offline := DisplayServer.get_name() == "headless" or "--offline" in OS.get_cmdline_user_args()
	if ClassDB.class_exists("SimRoutePicker"):
		picker = ClassDB.instantiate("SimRoutePicker")
		picker.set("network", not offline)
	if ClassDB.class_exists("SimTiles"):
		tiles = ClassDB.instantiate("SimTiles")
		tiles.set("offline", offline)
		if tiles.has_method("country_outline"):
			_set_country(tiles.call("country_outline"), tiles.call("country_bounds"))
	var controls := HBoxContainer.new()
	controls.position = Vector2(24, 64)
	canvas.add_child(controls)
	receive.text = "이 지역 도로 받기"
	receive.pressed.connect(_request_region)
	controls.add_child(receive)
	var cancel := Button.new()
	cancel.text = "받기 취소"
	cancel.pressed.connect(_cancel_region)
	controls.add_child(cancel)
	var undo := Button.new()
	undo.text = "마지막 점 삭제"
	undo.pressed.connect(_remove_last)
	controls.add_child(undo)
	var clear := Button.new()
	clear.text = "점 모두 삭제"
	clear.pressed.connect(_clear_points)
	controls.add_child(clear)
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
	cache_choices.custom_minimum_size.x = 180
	cache_choices.item_selected.connect(_select_cached_region)
	navigation.add_child(cache_choices)
	places_button.text = "건물·장소 받기"
	places_button.pressed.connect(_request_places)
	navigation.add_child(places_button)
	place_search.placeholder_text = "장소 이름 검색"
	place_search.custom_minimum_size.x = 180
	# 한글 IME는 Enter 때 조합 중인 마지막 글자가 아직 text에 없을 수 있어, 입력할 때마다 찾고 Enter·버튼은 조합 글자까지 합쳐 찾는다.
	place_search.text_changed.connect(_search_typed)
	place_search.text_submitted.connect(_search_submitted)
	navigation.add_child(place_search)
	var search_button := Button.new()
	search_button.text = "검색"
	search_button.pressed.connect(func(): _search_submitted(place_search.text))
	navigation.add_child(search_button)
	place_results.custom_minimum_size.x = 220
	place_results.item_selected.connect(_select_place_result)
	navigation.add_child(place_results)
	_fill_place_results([])
	snapshot_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	snapshot_dialog.access = FileDialog.ACCESS_FILESYSTEM
	snapshot_dialog.filters = PackedStringArray(["*.json ; OSM snapshot"])
	snapshot_dialog.file_selected.connect(_load_snapshot)
	add_child(snapshot_dialog)
	snapshot_folder_dialog.file_mode = FileDialog.FILE_MODE_OPEN_DIR
	snapshot_folder_dialog.access = FileDialog.ACCESS_FILESYSTEM
	snapshot_folder_dialog.dir_selected.connect(_load_snapshot)
	add_child(snapshot_folder_dialog)
	for button in [cancel, undo, clear]:
		button.disabled = picker == null
	_reload_cached_regions()

## 화면 가운데 위경도로 지역 도로 받기를 요청하고 요청 번호를 기억한다. 받는 중이거나 좌표가 무효면 하지 않는다.
func _request_region() -> void:
	if picker == null or active_request != 0:
		return
	var latlon: PackedFloat64Array = picker.call("view_to_latlon", camera.position)
	if latlon.size() != 2:
		return
	var result: Dictionary = picker.call("request_region", latlon)
	feedback = str(result.get("error", ""))
	if result.get("ok", false):
		active_request = int(result.request_id)

## 열린 지역의 건물·장소 받기를 요청하고 요청 번호를 기억한다. 받는 중이면 하지 않는다.
func _request_places() -> void:
	if picker == null or active_request != 0:
		return
	var result: Dictionary = picker.call("request_places")
	feedback = str(result.get("error", ""))
	if result.get("ok", false):
		active_request = int(result.request_id)

## 검색어로 장소를 찾아 결과 상자를 채운다.
func _search_places(text: String) -> void:
	if picker == null:
		return
	var found: Array = picker.call("search_places", text)
	_fill_place_results(found)
	feedback = "검색 결과 %d곳" % found.size() if not found.is_empty() else "'%s' 이름의 장소가 없습니다(건물·장소를 받았는지 확인)" % text

## 입력이 바뀔 때마다(한글은 글자 조합이 끝날 때마다) 받은 건물·장소 안에서만 찾는다. 전국 검색은 하지 않는다
## (Nominatim 이용 규칙: 입력 중 자동완성 금지). 지난 전국 결과는 지운다. 빈 칸이면 결과 상자만 비운다.
func _search_typed(text: String) -> void:
	area_results = []
	if text.strip_edges().is_empty():
		_fill_place_results([])
		feedback = ""
	else:
		_search_places(text)

## Enter·검색 버튼: 아직 조합 중인 한글 글자(IME)를 붙여 받은 건물·장소에서 찾고, 전국 검색도 요청한다.
func _search_submitted(text: String) -> void:
	var composing := DisplayServer.ime_get_text() if place_search.has_focus() else ""
	var query := (text + composing).strip_edges()
	if query.is_empty() or picker == null:
		return
	area_results = []
	_search_places(query)
	var result: Dictionary = picker.call("search_nationwide", query)
	if result.get("ok", false):
		search_request = int(result.request_id)
		feedback = "전국 검색 중... (OpenStreetMap Nominatim)"
	else:
		feedback = str(result.error)

## 전국 검색 결과 이벤트를 받아 결과 상자 아래쪽에 붙인다.
func _show_area_results(event: Dictionary) -> void:
	search_request = 0
	area_results = event.get("results", [])
	_fill_place_results(local_results)
	feedback = str(event.message)
	if not area_results.is_empty():
		place_results.show_popup()

## 결과 상자를 안내문 한 줄, 받은 건물·장소 결과(이름 · 종류), 전국 검색 결과(구분선 아래, 이름 — 주소)로 다시 만든다.
func _fill_place_results(found: Array) -> void:
	local_results = found
	place_results.clear()
	result_items.clear()
	place_results.add_item("검색 결과에서 고르기")
	result_items.append(null)
	for item in found:
		place_results.add_item("%s · %s" % [item.name, item.kind] if str(item.kind) != "" else str(item.name))
		result_items.append({"source": "place", "index": int(item.index)})
	if not area_results.is_empty():
		place_results.add_separator("전국 검색")
		result_items.append(null)
		for item in area_results:
			place_results.add_item("%s — %s" % [item.name, item.address])
			result_items.append({"source": "area", "index": int(item.index), "view": item.view})
	place_results.disabled = result_items.size() <= 1

## 고른 검색 결과를 처리한다. 받은 건물·장소는 점으로 더하고, 전국 결과는 _pick_area_result에 맡긴다.
func _select_place_result(item: int) -> void:
	place_results.select(0)
	if item < 1 or item >= result_items.size() or result_items[item] == null:
		return
	var chosen: Dictionary = result_items[item]
	if chosen.source == "area":
		_pick_area_result(chosen)
		return
	_add_place(int(chosen.index))
	_refresh_route()

## 전국 검색 결과: 열린 지역 안이면 가까운 도로 위 점으로 더하고, 아니면 그 위치로 지도를 옮겨 도로 받기를 안내한다.
func _pick_area_result(chosen: Dictionary) -> void:
	var view: Vector2 = chosen.view
	if opened_region.has_point(view):
		var result: Dictionary = picker.call("add_search_hit", int(chosen.index))
		feedback = "" if result.get("ok", false) else str(result.error)
		_refresh_route()
		return
	var available := get_viewport_rect().size - FIT_MARGIN
	if available.x > 0.0 and available.y > 0.0:
		camera.zoom = Vector2.ONE * clampf(minf(available.x, available.y) / SEARCH_VIEW_M, _min_zoom(), 8.0)
	camera.position = view
	_clamp_camera()
	feedback = "검색한 곳으로 옮겼습니다. '이 지역 도로 받기'를 누른 뒤 같은 결과를 다시 고르면 출발·도착 점으로 더합니다."
	queue_redraw()

## 장소 번호의 진입 지점을 점으로 더하고, 실패하면 이유를 안내에 남긴다.
func _add_place(index: int) -> void:
	var result: Dictionary = picker.call("add_place", index)
	feedback = "" if result.get("ok", false) else str(result.error)

## 받는 중인 지역 요청을 취소하고 그 결과를 더 기다리지 않는다.
func _cancel_region() -> void:
	if picker != null:
		picker.call("cancel_region")
	active_request = 0
	feedback = "지역 받기를 취소했습니다."

## 마지막으로 찍은 점을 지우고 도로·경로 표시를 다시 받는다.
func _remove_last() -> void:
	if picker != null:
		picker.call("remove_last")
		_refresh_route()

## 찍은 점을 모두 지우고 표시를 다시 받는다.
func _clear_points() -> void:
	if picker != null:
		picker.call("clear_points")
		_refresh_route()

## 입력한 이름과 덮어쓰기 선택으로 경로를 저장하고 결과(경로 또는 오류)를 안내에 남긴다.
func _save_route() -> void:
	if picker == null:
		return
	var result: Dictionary = picker.call("save", route_name.text, overwrite.button_pressed)
	feedback = "저장: " + str(result.path) if result.get("ok", false) else str(result.error)

## Rust에서 열린 지역 범위·도로 선분·사용 가능 여부·건물 윤곽·점·경로선을 받아 그리기 자료를 바꾼다.
func _refresh_route() -> void:
	opened_region = picker.call("loaded_region")
	place_outlines = picker.call("place_outlines")
	var result: Dictionary = picker.call("route")
	display_geometry(picker.call("road_segments"), picker.call("road_usable"), picker.call("points"), result.line)

## 매 프레임 받기 이벤트를 처리하고 버튼 상태·안내를 갱신하며 배경 타일을 요청한다.
func _process(_delta: float) -> void:
	if picker != null:
		for event in picker.call("poll"):
			if event.type == "search_results":
				if search_request != 0 and int(event.request_id) == search_request:
					_show_area_results(event)
				continue
			if active_request == 0 or int(event.request_id) != active_request:
				continue
			feedback = str(event.message)
			if event.type in ["region_loaded", "places_loaded", "failed"]:
				active_request = 0
				_refresh_route()
				if event.type == "region_loaded":
					_reload_cached_regions()
					_fit_region()
		var latlon: PackedFloat64Array = picker.call("view_to_latlon", camera.position)
		receive.disabled = active_request != 0 or latlon.size() != 2
		places_button.disabled = active_request != 0 or not opened_region.has_area()
		cache_choices.disabled = active_request != 0 or cached.is_empty()
		var result: Dictionary = picker.call("route")
		save_button.disabled = not bool(result.ok)
		status.text = "우클릭: 이동 / 휠: 확대 / 클릭: 점 추가(건물을 누르면 그 건물) / Shift+클릭: 점 삭제\n" + "\n".join(picker.call("status_lines")) + "\n" + feedback
	else:
		receive.disabled = true
		places_button.disabled = true
		save_button.disabled = true
	if tiles != null:
		_update_tiles()
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

## 화면을 떠날 때 지역 받기를 취소하고 서비스 객체와 텍스처를 놓는다.
func _exit_tree() -> void:
	if picker != null:
		picker.call("cancel_region")
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
	camera.position = bounds.get_center() - Vector2(0, 150.0 / zoom_value)
	_clamp_camera()

## 로컬 스냅샷(graph*.osm.json) 파일 선택 창을 연다. 받는 중에는 열지 않는다.
func _open_snapshot() -> void:
	if picker != null and active_request == 0:
		snapshot_dialog.popup_centered_ratio(0.75)

## 고른 스냅샷을 열고 성공하면 표시를 갱신해 그 지역에 맞춘다.
func _load_snapshot(path: String) -> void:
	if picker == null or active_request != 0:
		return
	var result: Dictionary = picker.call("load_snapshot", path)
	feedback = "스냅샷을 열었습니다." if result.get("ok", false) else str(result.error)
	if result.get("ok", false):
		_refresh_route()
		_fit_region()

## 스냅샷 세대 폴더 선택 창을 연다. 받는 중에는 열지 않는다.
func _open_snapshot_folder() -> void:
	if picker != null and active_request == 0:
		snapshot_folder_dialog.popup_centered_ratio(0.75)

## 디스크 캐시의 지역 목록을 다시 읽어 선택 상자를 만든다. 네트워크는 쓰지 않는다.
func _reload_cached_regions() -> void:
	cache_choices.clear()
	cache_choices.add_item("받아 둔 지역 열기")
	cached.clear()
	if picker != null:
		for bounds in picker.call("cached_regions"):
			cached.append(bounds)
			cache_choices.add_item("받아 둔 지역 %d" % cached.size())
	cache_choices.disabled = cached.is_empty()

## 고른 캐시 지역의 가운데로 지역 열기를 요청한다. 가운데가 받은 범위 안이라 Rust가 캐시만으로 연다.
func _select_cached_region(index: int) -> void:
	cache_choices.select(0)
	if picker == null or active_request != 0 or index < 1 or index > cached.size():
		return
	var latlon: PackedFloat64Array = picker.call("view_to_latlon", cached[index - 1].get_center())
	if latlon.size() != 2:
		return
	var result: Dictionary = picker.call("request_region", latlon)
	feedback = str(result.get("error", ""))
	if result.get("ok", false):
		active_request = int(result.request_id)

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

## 대한민국 전체가 화면에 들어오는 배율보다 더 축소하지 못하게 하는 최소 배율. 경계가 없으면 기존 한계.
func _min_zoom() -> float:
	if not country_bounds.has_area():
		return 0.0005
	var available := get_viewport_rect().size - FIT_MARGIN
	return clampf(minf(available.x / country_bounds.size.x, available.y / country_bounds.size.y), 0.0005, 8.0)

## 카메라 가운데를 대한민국 범위 안으로, 배율을 최소 배율 이상으로 묶는다.
func _clamp_camera() -> void:
	if not country_bounds.has_area():
		return
	if camera.zoom.x < _min_zoom():
		camera.zoom = Vector2.ONE * _min_zoom()
	camera.position = camera.position.clamp(country_bounds.position, country_bounds.end)
