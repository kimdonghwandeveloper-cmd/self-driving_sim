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
var texture_order: Array[String] = []
var receive := Button.new()
var save_button := Button.new()
var route_name := LineEdit.new()
var overwrite := CheckBox.new()

## 전국 범위를 담을 배율로 시작하고 출처 및 비활성 기능을 표시한다.
func _ready() -> void:
	camera.position = Vector2.ZERO
	camera.zoom = Vector2.ONE * 0.001
	add_child(camera)
	camera.make_current()
	get_viewport().size_changed.connect(queue_redraw)
	var canvas := CanvasLayer.new()
	add_child(canvas)
	status.position = Vector2(24, 84)
	status.text = "지도 미리보기 · Rust 지도 서비스 미연결\n우클릭 끌기: 이동 / 휠: 확대·축소\n지역 받기·점 편집·경로 계산·저장은 연결 대기 중입니다.\n회색 칸은 실제 지도 타일이 아닙니다."
	canvas.add_child(status)
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
			if event.shift_pressed:
				picker.call("remove_near", get_global_mouse_position(), 12.0 / camera.zoom.x)
			else:
				picker.call("add_point", get_global_mouse_position())
			_refresh_route()
		elif event.pressed and event.button_index in [MOUSE_BUTTON_WHEEL_UP, MOUSE_BUTTON_WHEEL_DOWN]:
			var before := get_global_mouse_position()
			var factor := 1.25 if event.button_index == MOUSE_BUTTON_WHEEL_UP else 0.8
			camera.zoom = Vector2.ONE * clampf(camera.zoom.x * factor, 0.0005, 8.0)
			camera.force_update_scroll()
			camera.position += before - get_global_mouse_position()
			queue_redraw()
	elif event is InputEventMouseMotion and dragging:
		camera.position -= event.relative / camera.zoom.x
		queue_redraw()

## 포커스 상실 시 지도 끌기를 끝낸다.
func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		dragging = false

## 현재 뷰의 회색 칸, 타일, 도로, 경로, 선택 점을 순서대로 표시한다.
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
	for i in range(roads.size() / 2):
		var allowed := i < usable.size() and usable[i] == 1
		draw_line(roads[i * 2], roads[i * 2 + 1], Color("cbd5dd") if allowed else Color("737e88"), 2.0 / scale_m, true)
	if route_line.size() > 1:
		draw_polyline(route_line, Color("58c4ef"), 4.0 / scale_m, true)
	for i in range(points.size()):
		draw_circle(points[i], 6.0 / scale_m, Color("f3bf63") if i == 0 else Color("9ee5c2"))
## v1 ???? ??? ???? ??? ??????? ?? ??? ???.
func _services(canvas: CanvasLayer) -> void:
	var offline := DisplayServer.get_name() == "headless" or "--offline" in OS.get_cmdline_user_args()
	if ClassDB.class_exists("SimRoutePicker"):
		picker = ClassDB.instantiate("SimRoutePicker")
		picker.set("network", not offline)
	if ClassDB.class_exists("SimTiles"):
		tiles = ClassDB.instantiate("SimTiles")
		tiles.set("offline", offline)
	var controls := HBoxContainer.new()
	controls.position = Vector2(24, 220)
	canvas.add_child(controls)
	receive.text = "? ?? ?? ??"
	receive.pressed.connect(_request_region)
	controls.add_child(receive)
	var cancel := Button.new()
	cancel.text = "?? ??"
	cancel.pressed.connect(_cancel_region)
	controls.add_child(cancel)
	var undo := Button.new()
	undo.text = "??? ? ??"
	undo.pressed.connect(_remove_last)
	controls.add_child(undo)
	var clear := Button.new()
	clear.text = "? ?? ??"
	clear.pressed.connect(_clear_points)
	controls.add_child(clear)
	route_name.placeholder_text = "?? ?? (??????_- )"
	route_name.custom_minimum_size.x = 210
	controls.add_child(route_name)
	overwrite.text = "????"
	controls.add_child(overwrite)
	save_button.text = "?? ??"
	save_button.pressed.connect(_save_route)
	controls.add_child(save_button)
	for button in [cancel, undo, clear]:
		button.disabled = picker == null

## ?? ?? ??? Rust?? ???? ?? ???? ?? ??? ???.
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

## ??? ??? ?? ??? ??? ?? ??? ?? ??? ???.
func _cancel_region() -> void:
	if picker != null:
		picker.call("cancel_region")
	active_request = 0
	feedback = "?? ??? ??????."

## ??? ? ??? ?? ??? Rust? ???.
func _remove_last() -> void:
	if picker != null:
		picker.call("remove_last")
		_refresh_route()

## ?? ?? ??? ?? ??? ????.
func _clear_points() -> void:
	if picker != null:
		picker.call("clear_points")
		_refresh_route()

## ?? ??? ???? ??? ???? ???? ????.
func _save_route() -> void:
	if picker == null:
		return
	var result: Dictionary = picker.call("save", route_name.text, overwrite.button_pressed)
	feedback = "??: " + str(result.path) if result.get("ok", false) else str(result.error)

## ?? ?? ?? ?? ??? ? ?? ???? ?? ??? ???.
func _refresh_route() -> void:
	var result: Dictionary = picker.call("route")
	display_geometry(picker.call("road_segments"), picker.call("road_usable"), picker.call("points"), result.line)

## ?? ???? ?? ??? ???? ????? ??? Rust? ??.
func _process(_delta: float) -> void:
	if picker != null:
		for event in picker.call("poll"):
			if active_request == 0 or int(event.request_id) != active_request:
				continue
			feedback = str(event.message)
			if event.type in ["region_loaded", "failed"]:
				active_request = 0
				_refresh_route()
		var latlon: PackedFloat64Array = picker.call("view_to_latlon", camera.position)
		receive.disabled = active_request != 0 or latlon.size() != 2
		var result: Dictionary = picker.call("route")
		save_button.disabled = not bool(result.ok)
		status.text = "??? ??: ?? / ?: ?? / ??: ? ?? / Shift+??: ? ??\n" + "\n".join(picker.call("status_lines")) + "\n" + feedback
	else:
		receive.disabled = true
		save_button.disabled = true
	if tiles != null:
		_update_tiles()
	queue_redraw()

## ??? ??? ???? PNG? ????? ???? ?? 512? ????.
func _update_tiles() -> void:
	var span := get_viewport_rect().size / camera.zoom.x
	var visible := Rect2(camera.position - span * 0.5, span)
	tiles.call("request_visible", visible.position, visible.end, 1.0 / camera.zoom.x)
	for tile in tiles.call("poll"):
		if tile.status != "ok":
			feedback = "?? ??: " + str(tile.error)
			continue
		var image := Image.new()
		if image.load_png_from_buffer(tile.png) != OK:
			tiles.call("report_bad", tile.x, tile.y, tile.z)
			continue
		var key := "%d/%d/%d" % [tile.z, tile.x, tile.y]
		textures[key] = {"texture": ImageTexture.create_from_image(image), "rect": tile.rect, "x": tile.x, "y": tile.y, "z": tile.z}
		texture_order.erase(key)
		texture_order.append(key)
	for key in textures:
		if visible.intersects(textures[key].rect):
			texture_order.erase(key)
			texture_order.append(key)
	while texture_order.size() > 512:
		var key: String = texture_order.pop_front()
		var tile: Dictionary = textures[key]
		tiles.call("forget_displayed", tile.x, tile.y, tile.z)
		textures.erase(key)
	tile_layers = []
	for tile in textures.values():
		if visible.intersects(tile.rect):
			tile_layers.append(tile)
	if int(tiles.call("paused_until_s")) > int(Time.get_unix_time_from_system()):
		status.text += "\n?? ?? ?? ?? ??"

## ?? ?? ? ??? ???? ???? GPU ??? ??? ????.
func _exit_tree() -> void:
	if picker != null:
		picker.call("cancel_region")
	picker = null
	tiles = null
	textures.clear()
	tile_layers.clear()
	texture_order.clear()
