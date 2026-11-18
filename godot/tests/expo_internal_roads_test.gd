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
							for tick in 96:
								vehicle.call("set_manual_input", 0.4, 0.0, 0.0)
								await physics_frame
							_check(vehicle.position.distance_to(start) > 0.1, "복구한 내부 도로에서 차량이 움직이지 않음")
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

## 실패 항목을 모아 검증 결과로 출력한다.
func _check(ok: bool, message: String) -> void:
	if not ok:
		failures.append(message)
