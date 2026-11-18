extends SceneTree
## 재생성한 전국 팩에서 엑스포 내부 서비스 도로와 주행 코스 생성을 확인한다.

var failures := PackedStringArray()

## 자료가 준비된 검증 환경에서만 실제 OSM 자료 시험을 실행한다.
func _init() -> void:
	if not "--full-data" in OS.get_cmdline_user_args():
		print("EXPO INTERNAL SKIP: --full-data와 재생성한 전국 팩 필요")
		quit(0)
		return
	call_deferred("_run")

## 엑스포 지역 원본 스냅샷에서 복구한 통로를 세고 실제 전체 그래프에서 주행 가능한 표본을 찾는다.
func _run() -> void:
	var picker: Object = ClassDB.instantiate("SimRoutePicker")
	picker.set("network", false)
	var result: Dictionary = picker.call("open_region", PackedFloat64Array([34.7505, 127.7475]))
	_check(result.ok, "엑스포 지역 열기 실패: " + str(result.error))
	if result.ok:
		var source: String = picker.call("source_snapshot")
		var graph: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(source))
		var internal := []
		for element: Dictionary in graph.get("elements", []):
			if element.get("type") != "way":
				continue
			var tags: Dictionary = element.get("tags", {})
			if tags.get("service", "") in ["parking_aisle", "driveway", "drive-through"]:
				var center := Vector2.ZERO
				for point: Dictionary in element.geometry:
					center += Vector2(point.lat, point.lon) / element.geometry.size()
				if center.x > 34.747 and center.x < 34.754 and center.y > 127.744 and center.y < 127.751:
					internal.append(element)
		_check(not internal.is_empty(), "주차장 통로·진입로가 여전히 팩에서 누락됨")
		print("EXPO INTERNAL SOURCE: ", source, " · facility services: ", internal.size())
		for argument in OS.get_cmdline_user_args():
			if argument.begins_with("--previous-data="):
				_check_previous(internal, argument.trim_prefix("--previous-data="))
		picker.call("set_restricted_roads", true)
		var selected := false
		for way: Dictionary in internal:
			var geometry: Array = way.geometry
			for i in range(geometry.size() - 1):
				var a: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([geometry[i].lat, geometry[i].lon]))
				var b: Vector2 = picker.call("latlon_to_view", PackedFloat64Array([geometry[i + 1].lat, geometry[i + 1].lon]))
				if a.distance_to(b) < 70.0:
					continue
				for pair in [[a.lerp(b, 0.15), a.lerp(b, 0.85)], [a.lerp(b, 0.85), a.lerp(b, 0.15)]]:
					picker.call("clear_points")
					picker.call("add_point", pair[0])
					picker.call("add_point", pair[1])
					var route: Dictionary = picker.call("route")
					if route.ok:
						var name := "expo-internal-regression-%d" % OS.get_process_id()
						var saved: Dictionary = picker.call("save", name, false)
						_check(saved.ok, "내부 도로 표본 저장 실패")
						if saved.ok:
							var course: Dictionary = ClassDB.class_call_static("SimCourse", "build", "map:" + str(saved.path))
							_check(course.get("ok", false), "내부 도로 Godot 코스 생성 실패")
							var vehicle: Node3D = ClassDB.instantiate("SimVehicle")
							vehicle.set("course", "map:" + str(saved.path))
							vehicle.set("read_input", false)
							root.add_child(vehicle)
							await physics_frame
							var start := vehicle.position
							vehicle.call("send_command", "engine_toggle")
							vehicle.call("send_command", "gear_drive")
							var goal: Vector3 = course.goal.position
							var touched := false
							for tick in 4096:
								var speed := float(vehicle.call("speed_mps"))
								vehicle.call("set_manual_input", clampf((3.0 - speed) * 0.5, 0.0, 0.5), 0.0, 0.0)
								await physics_frame
								touched = touched or bool(vehicle.call("in_contact"))
								if vehicle.position.distance_to(goal) < 5.0:
									break
							_check(vehicle.position.distance_to(start) > 30.0 and vehicle.position.distance_to(goal) < 5.0, "복구한 내부 도로에서 목표 근처까지 도달하지 못함")
							_check(not touched, "내부 도로 주행 중 연석 또는 벽 접촉")
							vehicle.free()
							DirAccess.remove_absolute(str(saved.path))
						print("EXPO INTERNAL ROUTE: way ", int(way.id), " · ", way.tags.service, " · ", route.length_m, " m")
						selected = true
						break
				if selected:
					break
			if selected:
				break
		_check(selected, "복구된 내부 통로에서 주행 가능한 표본을 찾지 못함")
	picker = null
	if failures.is_empty():
		print("EXPO INTERNAL OK: 원본 PBF 기반 내부 통로 보존 · 선택 · 저장 · 코스 재생")
		quit(0)
	else:
		for message in failures:
			printerr("EXPO INTERNAL FAIL: " + message)
		quit(1)

## 같은 원본의 이전 팩과 대조해 새로 보존한 내부 통로 ID가 과거 자료에는 없음을 검사한다.
func _check_previous(internal: Array, previous_dir: String) -> void:
	var old: Object = ClassDB.instantiate("SimRoutePicker")
	old.set("network", false)
	old.call("use_data_dir", previous_dir)
	var opened: Dictionary = old.call("open_region", PackedFloat64Array([34.7505, 127.7475]))
	_check(opened.ok, "이전 팩 비교를 위한 지역 열기 실패")
	if opened.ok:
		var path: String = old.call("source_snapshot")
		var graph: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(path))
		var ids := {}
		for element: Dictionary in graph.get("elements", []):
			if element.get("type") == "way":
				ids[int(element.id)] = true
		for way: Dictionary in internal:
			_check(not ids.has(int(way.id)), "복구 통로가 이전 팩에도 이미 존재함: " + str(way.id))
		print("EXPO PREVIOUS COMPARISON: ", internal.size(), " facility services absent from previous pack")
	old = null

## 실패 항목을 모아 검증 결과로 출력한다.
func _check(ok: bool, message: String) -> void:
	if not ok:
		failures.append(message)
