extends Control
## 전체 Lanelet2 지도에서 출발·도착 차선을 선택하고 검증된 경로만 주행으로 넘긴다.

const Canvas = preload("res://presentation/lanelet_map_canvas.gd")
signal drive_requested(course: String)
var picker: RefCounted
var canvas := Canvas.new()
var maps := OptionButton.new()
var paths := PackedStringArray()
var status := Label.new()
var notices := Label.new()
var selection := Label.new()
var start_button := Button.new()
var start_mode := Button.new()
var goal_mode := Button.new()
var mode := "start"

## 상단 지도 선택·작업 안내와 하단 출처를 두고 가운데 영역을 지도에 할당한다.
func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var background := ColorRect.new()
	background.color = Color("101d2c")
	background.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(background)
	background.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var margin := MarginContainer.new()
	add_child(margin)
	margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	for side in ["left", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 20)
	margin.add_theme_constant_override("margin_top", 66)
	var column := VBoxContainer.new()
	margin.add_child(column)
	column.add_theme_constant_override("separation", 10)
	var title := Label.new()
	title.text = "Lanelet2 · 출발·도착 차선 선택"
	title.add_theme_font_size_override("font_size", 24)
	column.add_child(title)
	maps.custom_minimum_size.y = 40
	maps.fit_to_longest_item = false
	column.add_child(maps)
	var actions := HFlowContainer.new()
	column.add_child(actions)
	_button(actions, start_mode, "① 출발 선택", _select_start_mode)
	_button(actions, goal_mode, "② 도착 선택", _select_goal_mode)
	_button(actions, Button.new(), "선택 초기화", _reset)
	_button(actions, Button.new(), "지도 맞춤", canvas.fit)
	_button(actions, start_button, "이 경로로 주행", _drive)
	start_button.disabled = true
	column.add_child(selection)
	status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	column.add_child(status)
	canvas.size_flags_vertical = Control.SIZE_EXPAND_FILL
	canvas.custom_minimum_size.y = 140
	column.add_child(canvas)
	canvas.lane_clicked.connect(_pick)
	var scroll := ScrollContainer.new()
	scroll.custom_minimum_size.y = 72
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	column.add_child(scroll)
	notices.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	notices.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	notices.add_theme_font_size_override("font_size", 13)
	scroll.add_child(notices)
	maps.item_selected.connect(_open)
	if not ClassDB.class_exists("SimLaneletPicker"):
		status.text = "Lanelet2 연결 모듈이 없습니다. sim-godot을 다시 빌드하세요."
		return
	picker = ClassDB.instantiate("SimLaneletPicker")
	for file in ClassDB.class_call_static("SimCourse", "course_files"):
		if file.saved:
			continue
		var path := str(file.course).trim_prefix("map:")
		var absolute := ProjectSettings.globalize_path("res://..").path_join(path)
		# TOML은 ConfigFile 대상이 아니므로 형식 식별 줄만 읽고 실제 검증은 Rust 로더에 맡긴다.
		var text := FileAccess.get_file_as_string(absolute)
		if text.contains('"lanelet2-course-v1"') or text.contains('"lanelet2-v1"'):
			paths.append(path)
			maps.add_item(str(file.label))
	if paths.is_empty():
		status.text = "등록된 Lanelet2 코스 설정이 없습니다."
	else:
		_open(0)

## 클릭 전용 버튼을 만들고 키보드 주행 입력과 포커스가 충돌하지 않게 한다.
func _button(parent: Control, button: Button, text: String, action: Callable) -> void:
	button.text = text
	button.custom_minimum_size.y = 38
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(action)
	parent.add_child(button)

## 지도 전환 실패 시 이전 경로·출처까지 비워 다른 지도 경로를 시작하지 못하게 한다.
func _open(index: int) -> void:
	canvas.configure([])
	notices.text = ""
	_reset()
	var result: Dictionary = picker.call("open", paths[index])
	if not result.ok:
		status.text = "지도를 열지 못했습니다: " + str(result.error)
		return
	canvas.configure(result.lanes)
	notices.text = "원본 지도·코스 안내 (선택 경로와 다를 수 있음)\n" + "\n".join(result.notices)
	status.text = "차선 내부를 클릭하세요. 차선 단위 선택이며 정확한 클릭 지점에서 출발하지 않습니다."

## 출발 차선을 다시 고를 때 도착 선택은 유지하되 경로를 즉시 재검증한다.
func _select_start_mode() -> void:
	mode = "start"
	_update_selection()

## 도착 차선 선택 모드로 바꾼다.
func _select_goal_mode() -> void:
	mode = "goal"
	_update_selection()

## 이전 경로를 제거하고 선택한 출발·도착이 모두 있을 때만 미리보기를 검증한다.
func _pick(id: int) -> void:
	if id == 0:
		status.text = "차선 밖입니다. 표시된 도로 면 안을 클릭하세요."
		return
	canvas.preview = {}
	start_button.disabled = true
	if mode == "start":
		canvas.selected_start = id
		mode = "goal"
	else:
		canvas.selected_goal = id
	if canvas.selected_start != 0 and canvas.selected_goal != 0:
		var result: Dictionary = picker.call("preview", canvas.selected_start, canvas.selected_goal)
		if result.ok:
			canvas.preview = result
			start_button.disabled = false
			status.text = "경로 확인 · %d개 차선. 차선 개수 기준 최단 경로이며 차로 변경·신호 규칙은 지원하지 않습니다." % result.ids.size()
		else:
			status.text = "경로를 만들 수 없습니다(방향·연결 또는 경로 모호성 확인): " + str(result.error)
	else:
		status.text = "도착 차선을 선택하세요."
	_update_selection()

## 출발·도착 표시와 현재 선택 모드를 갱신한다.
func _update_selection() -> void:
	selection.text = "출발: %s   →   도착: %s   · 현재: %s 선택" % [str(canvas.selected_start) if canvas.selected_start else "미선택", str(canvas.selected_goal) if canvas.selected_goal else "미선택", "출발" if mode == "start" else "도착"]
	start_mode.modulate = Color("69c8ff") if mode == "start" else Color.WHITE
	goal_mode.modulate = Color("ffd27a") if mode == "goal" else Color.WHITE
	canvas.queue_redraw()

## 선택·검증 상태를 함께 초기화해 남은 경로로 주행하지 않게 한다.
func _reset() -> void:
	canvas.selected_start = 0
	canvas.selected_goal = 0
	canvas.preview = {}
	mode = "start"
	start_button.disabled = true
	status.text = "출발 차선을 선택하세요."
	_update_selection()

## 저장 직전 다시 검증하고 원본을 보존한 새 코스 파일로 주행 화면을 요청한다.
func _drive() -> void:
	if start_button.disabled:
		return
	var result: Dictionary = picker.call("save", canvas.selected_start, canvas.selected_goal)
	if not result.ok:
		status.text = "선택 경로 저장 실패: " + str(result.error)
		return
	drive_requested.emit(str(result.course))
