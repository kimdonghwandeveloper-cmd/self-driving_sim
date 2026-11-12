# 긴 경로 측정(4-11 2단계, 측정만 하고 실패로 끝내지 않음). 전국 지도 자료로 두 위치 사이 경로를 계산해 저장하고, 계산 시간·길이·
# 코스 만들기 시간·코스 크기(도로 띠·연석·교차로 면 수)를 출력한다. 주행 화면 프레임 시간은 저장한 경로로 frame_time_probe.gd를 돌려 잰다.
# 실행(워크트리 최상위):
#   godot --headless --path godot --script res://tests/long_route_probe.gd -- --data=<전국 자료 폴더> --from=<위도>,<경도> --to=<위도>,<경도> [--name=<저장 이름>]
extends SceneTree


## 인자를 읽고 측정을 시작한다.
func _init() -> void:
	call_deferred("_run")


## 명령줄 `--name=값` 인자를 읽는다. 없으면 기본값.
func _arg(name: String, fallback: String) -> String:
	for argument in OS.get_cmdline_user_args():
		if argument.begins_with("--%s=" % name):
			return argument.split("=", true, 1)[1]
	return fallback


## `위도,경도` 글자를 [위도, 경도]로 바꾼다.
func _latlon(text: String) -> PackedFloat64Array:
	var parts := text.split(",")
	return PackedFloat64Array([float(parts[0]), float(parts[1])])


## 경로를 계산·저장하고 코스를 만들어 시간과 크기를 출력한다.
func _run() -> void:
	var picker: Object = ClassDB.instantiate("SimRoutePicker")
	picker.call("use_data_dir", _arg("data", ""))
	var name := _arg("name", "long-route-probe")
	var start := {"place": -1, "latlon": _latlon(_arg("from", "37.4979,127.0276")), "label": "출발"}
	var end := {"place": -1, "latlon": _latlon(_arg("to", "37.5088,127.0631")), "label": "도착"}
	var started := Time.get_ticks_usec()
	var planned: Dictionary = picker.call("plan_endpoints", start, end)
	var plan_ms := (Time.get_ticks_usec() - started) / 1000.0
	var route: Dictionary = picker.call("route")
	if not planned.ok or not route.ok:
		print("LONG ROUTE PROBE: 경로 실패 %.0f ms: %s %s" % [plan_ms, planned.error, route.error])
		quit(0)
		return
	# --jitter=<m>: 계산된 경로선을 사람이 손으로 그린 선처럼 좌우로 흔들어(사인파, 진폭 m) 선 그리기 경로로 다시 만들어 본다.
	var jitter := float(_arg("jitter", "-1"))
	if jitter >= 0.0:
		var line: PackedVector2Array = route.line
		var drawn := PackedVector2Array()
		var walked := 0.0
		for i in range(line.size()):
			if i > 0:
				walked += line[i].distance_to(line[i - 1])
			var along: Vector2 = (line[mini(i + 1, line.size() - 1)] - line[maxi(i - 1, 0)]).normalized()
			drawn.append(line[i] + Vector2(-along.y, along.x) * jitter * sin(walked / 37.0))
		started = Time.get_ticks_usec()
		var drew: Dictionary = picker.call("plan_line", drawn)
		var draw_ms := (Time.get_ticks_usec() - started) / 1000.0
		var redrawn: Dictionary = picker.call("route")
		print("LONG ROUTE PROBE: 선 그리기(흔들림 %.0f m) %s %.0f ms, 건너뜀 %d, 경로 %s %.0f m (원래 %.0f m) %s %s" % [jitter, str(drew.ok), draw_ms, int(drew.skipped), str(redrawn.ok), redrawn.length_m, route.length_m, drew.error, redrawn.error])
		quit(0)
		return
	var saved: Dictionary = picker.call("save", name, true)
	started = Time.get_ticks_usec()
	var built: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:cache/routes/%s.toml" % name)
	var build_ms := (Time.get_ticks_usec() - started) / 1000.0
	print("LONG ROUTE PROBE: 경로 %.0f m, 계산 %.0f ms, 저장 %s" % [route.length_m, plan_ms, str(saved.get("path", saved.get("error", "")))])
	if built.get("ok", false):
		print("LONG ROUTE PROBE: 코스 만들기 %.0f ms, 도로 띠 %d, 연석·벽 %d, 교차로 면 %d" % [build_ms, built.strips.size(), built.colliders.size(), built.areas.size()])
	else:
		print("LONG ROUTE PROBE: 코스 만들기 실패: " + str(built.get("error", "")))
	quit(0)
