# 지도 화면 프레임 시간 측정(4-11 렉 조사, 측정만 하고 실패로 끝내지 않음). 실제 창에서 돌린다(헤드리스 아님).
# 전국 지도 자료로 지역 하나(기본 강남역)를 열고, 지역 열기·표시 갱신·경로·안내 문구 호출 시간과 가만히 둔 화면의 프레임 시간을 출력한다.
# 실행(워크트리 최상위): godot --path godot --script res://tests/map_frame_probe.gd -- [--data=<전국 자료 폴더>] [--at=<위도>,<경도>]
extends SceneTree

const Map = preload("res://presentation/map_screen.gd")
## 가만히 둔 화면을 잴 프레임 수.
const FRAMES := 180

var map: Node2D


## 인자를 읽고 측정을 시작한다.
func _init() -> void:
	call_deferred("_run")


## 명령줄 `--name=값` 인자를 읽는다. 없으면 기본값.
func _arg(name: String, fallback: String) -> String:
	for argument in OS.get_cmdline_user_args():
		if argument.begins_with("--%s=" % name):
			return argument.split("=", true, 1)[1]
	return fallback


## 지역을 열고 단계별 시간과 프레임 시간을 잰다.
func _run() -> void:
	map = Map.new()
	root.add_child(map)
	await process_frame
	var picker: Object = map.picker
	var data := _arg("data", "")
	if data != "":
		picker.call("use_data_dir", data)
	map._check_data()
	if not map.data_ok:
		print("MAP PROBE: 자료 없음 " + map.data_error)
		quit(0)
		return
	var parts := _arg("at", "37.4979,127.0276").split(",")
	var at: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([float(parts[0]), float(parts[1])]))
	map.camera.zoom = Vector2.ONE * (map.get_viewport_rect().size.x / 2000.0)
	map.camera.position = at
	var started := Time.get_ticks_usec()
	var latlon: PackedFloat64Array = picker.call("view_to_latlon", at)
	var opened: Dictionary = picker.call("open_region", latlon)
	var open_ms := (Time.get_ticks_usec() - started) / 1000.0
	started = Time.get_ticks_usec()
	map._refresh_route()
	var refresh_ms := (Time.get_ticks_usec() - started) / 1000.0
	started = Time.get_ticks_usec()
	map._refresh_route()
	var refresh_again_ms := (Time.get_ticks_usec() - started) / 1000.0
	started = Time.get_ticks_usec()
	for i in range(10):
		picker.call("route")
	var route_ms := (Time.get_ticks_usec() - started) / 10000.0
	started = Time.get_ticks_usec()
	for i in range(10):
		picker.call("status_lines")
	var status_ms := (Time.get_ticks_usec() - started) / 10000.0
	print("MAP PROBE: open_region %s / %.1f ms, 첫 표시 갱신 %.1f ms, 다시 갱신 %.1f ms, route() %.2f ms, status_lines() %.2f ms" % [str(opened.get("ok", false)), open_ms, refresh_ms, refresh_again_ms, route_ms, status_ms])
	print("MAP PROBE: 도로 선분 %d, 건물·장소 %d" % [map.roads.size() / 2, map.place_outlines.size()])
	await _measure("가만히 둔 화면", Vector2.ZERO, 1.0)
	await _measure("끌어 이동(프레임마다 3 m)", Vector2(3, 0), 1.0)
	await _measure("확대·축소(프레임마다 배율 바뀜)", Vector2.ZERO, 1.003)
	map.free()
	quit(0)


## `FRAMES` 프레임 동안 프레임마다 카메라를 `step`만큼 옮기고 배율에 `zoom_factor`를 곱하며 프레임 시간을 잰다.
func _measure(label: String, step: Vector2, zoom_factor: float) -> void:
	var times: Array[float] = []
	var last := Time.get_ticks_usec()
	for i in range(FRAMES):
		map.camera.position += step
		map.camera.zoom *= zoom_factor
		await process_frame
		var now := Time.get_ticks_usec()
		times.append((now - last) / 1000.0)
		last = now
	times.sort()
	var total := 0.0
	for value in times:
		total += value
	print("MAP PROBE: %s %d프레임 평균 %.2f ms, p95 %.2f ms, 최대 %.2f ms" % [label, FRAMES, total / FRAMES, times[int(FRAMES * 0.95)], times[FRAMES - 1]])
