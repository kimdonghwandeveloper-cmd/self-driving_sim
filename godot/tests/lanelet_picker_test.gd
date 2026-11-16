extends SceneTree
## 실제 클릭으로 출발·도착 선택, 잘못된 방향의 이전 경로 제거, 저장·주행 진입을 검증한다.

var failures := PackedStringArray()
var capture_dir := ""

## 선택적 화면 캡처 경로를 읽고 엔진 초기화 뒤 시험한다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--capture-dir="):
			capture_dir = arg.trim_prefix("--capture-dir=")
	call_deferred("_run")

## 실제 메뉴·지도·주행 화면을 순서대로 열어 새 코스가 대체 없이 사용되는지 확인한다.
func _run() -> void:
	var main: Node = load("res://main.tscn").instantiate()
	root.add_child(main)
	root.size = Vector2i(1280, 720)
	for frame in 4:
		await process_frame
	await _click(main.screen.lanelet_map_button.get_global_rect().get_center())
	var screen: Control = main.screen
	_check(screen.has_method("_pick"), "Lanelet2 메뉴 진입 실패")
	if not screen.has_method("_pick"):
		main.free()
		_finish()
		return
	var index: int = screen.paths.find("assets/maps/lanelet2-demo/course.toml")
	_check(index >= 0, "합성 지도 없음")
	if index < 0:
		main.free()
		_finish()
		return
	var intersection: int = screen.paths.find("assets/maps/lanelet2-real-demo/course.toml")
	_check(intersection >= 0, "교차로 지도 없음")
	if intersection >= 0:
		screen.maps.select(intersection)
		screen._open(intersection)
		for pair in [[1001, 1006], [1002, 1007]]:
			screen._reset()
			screen._pick(pair[0])
			screen._pick(pair[1])
			_check(not screen.start_button.disabled, "교차로 직진·우회전 선택 실패")
		await _capture("intersection-picker")
		screen._reset()
		screen._pick(1001)
		screen._pick(1007)
		_check(screen.start_button.disabled, "차로 변경 없는 잘못된 연결을 허용함")
	screen.maps.select(index)
	screen._open(index)
	for frame in 4:
		await process_frame
	for window_size in [Vector2i(800, 600), Vector2i(1280, 720)]:
		root.size = window_size
		for frame in 4:
			await process_frame
		_check(screen.get_global_rect().encloses(screen.canvas.get_global_rect()), "지도 영역이 창 밖")
		_check(screen.get_global_rect().encloses(screen.start_button.get_global_rect()), "주행 버튼이 창 밖")
	_check(screen.canvas.lanes.size() == 2, "전체 차선 2개 표시 실패")
	_check(screen.start_button.disabled, "선택 전 주행 허용")
	await _click_lane(screen, 301)
	_check(screen.canvas.selected_start == 301 and screen.mode == "goal", "출발 클릭 실패")
	await _click_lane(screen, 302)
	_check(not screen.start_button.disabled and screen.canvas.preview.get("ok", false), "경로 미리보기 실패")
	_check(screen.canvas.preview.get("ids") == PackedInt64Array([301, 302]), "다른 경로를 표시함")
	_check(screen.notices.text.contains("합성") and screen.notices.text.contains("self-driving_sim"), "출처 누락")
	await _capture("lanelet-picker")
	# 유효한 미리보기 이후 역방향 선택을 넣어 이전 경로로 시작할 수 없는지 본다.
	screen._select_start_mode()
	await _click_lane(screen, 302)
	await _click_lane(screen, 301)
	_check(screen.start_button.disabled and screen.canvas.preview.is_empty(), "실패 뒤 이전 경로가 남음")
	_check(screen.status.text.contains("경로를 만들 수 없습니다"), "실패 원인 안내 없음")
	screen._reset()
	await _click_lane(screen, 301)
	await _click_lane(screen, 302)
	# 확대·이동 후 같은 차선을 같은 변환으로 고를 수 있어야 한다.
	screen.canvas.zoom = 1.5
	screen.canvas.pan = Vector2(20, -10)
	_check(screen.canvas.pick(_lane_point(screen, 301)) == 301, "확대·이동 뒤 차선 판정 불일치")
	await _click(screen.start_button.get_global_rect().get_center())
	var drive: Node = main.screen
	_check(drive.has_method("start"), "주행 화면 전환 실패")
	if drive.has_method("start"):
		_check(drive.failure.is_empty() and drive.fallback_notice.is_empty(), "선택 경로 대신 대체 코스가 열림")
		var name := str(drive.vehicle.get("course"))
		_check(name.begins_with("map:cache/routes/lanelet2-selected-"), "선택 파일을 사용하지 않음")
		var data: Dictionary = drive.vehicle.call("course_data")
		_check(data.strips.size() == 2, "선택 경로 형상 불일치")
		var replay: Dictionary = ClassDB.class_call_static("SimCourse", "build", name)
		_check(replay.get("start") == data.start and replay.get("goal") == data.goal, "선택 파일 재로드 불일치")
		main._show_menu()
		await process_frame
		_check(name in main.screen.course_names, "저장 경로가 메뉴 목록에서 사라짐")
		if name.begins_with("map:cache/routes/lanelet2-selected-"):
			DirAccess.remove_absolute(ProjectSettings.globalize_path("res://..").path_join(name.trim_prefix("map:")))
	# 실패한 지도 열기가 이전 지도의 모델을 재사용하지 않아야 한다.
	var picker: RefCounted = ClassDB.instantiate("SimLaneletPicker")
	_check(picker.call("open", "assets/maps/lanelet2-demo/course.toml").ok, "모델 열기 실패")
	_check(not picker.call("open", "missing-lanelet-course.toml").ok, "없는 지도 열기 성공")
	_check(not picker.call("preview", 301, 302).ok, "지도 실패 뒤 이전 모델 유지")
	main.free()
	_finish()

## 화면에 보이는 차선 중앙의 좌표를 같은 지도 변환으로 구한다.
func _lane_point(screen: Control, id: int) -> Vector2:
	for lane in screen.canvas.lanes:
		if int(lane.id) == id:
			var center: PackedVector2Array = lane.center
			return screen.canvas.map_point(center[center.size() / 2])
	return Vector2(-100, -100)

## 실제 차선 내부 화면 위치에 마우스 이벤트를 보낸다.
func _click_lane(screen: Control, id: int) -> void:
	await _click(screen.canvas.global_position + _lane_point(screen, id))

## 뷰포트 입력으로 버튼·지도 클릭 전달 경로를 검사한다.
func _click(point: Vector2) -> void:
	for pressed in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = MOUSE_BUTTON_LEFT
		event.position = point
		event.global_position = point
		event.pressed = pressed
		root.push_input(event)
		await process_frame

## 렌더링 시험에서 지도 전체와 출처 영역을 함께 캡처한다.
func _capture(label: String) -> void:
	if capture_dir.is_empty():
		return
	await RenderingServer.frame_post_draw
	DirAccess.make_dir_recursive_absolute(capture_dir)
	_check(root.get_texture().get_image().save_png(capture_dir.path_join(label + ".png")) == OK, "지도 캡처 실패")

## 실패를 수집해 한 번에 출력한다.
func _check(condition: bool, message: String) -> void:
	if not condition:
		failures.append(message)

## 전체 결과와 종료 코드를 반환한다.
func _finish() -> void:
	if failures.is_empty():
		print("LANELET PICKER OK: 클릭 · 방향 거부 · 미리보기 · 저장 · 주행 · 재로드")
		quit(0)
	else:
		for message in failures:
			printerr("LANELET PICKER FAIL: " + message)
		quit(1)
