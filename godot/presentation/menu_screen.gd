extends Control
## 코스 선택·지도·도움말을 제공하는 반응형 메뉴. 물리 상태는 생성하지 않는다.

const DriveScreen = preload("res://presentation/drive_screen.gd")
signal drive_requested(course: String)
signal map_requested
signal quit_requested
var courses := OptionButton.new()
var course_names := PackedStringArray()
var start_button := Button.new()
var map_button := Button.new()
var controls_button := Button.new()
var settings_button := Button.new()
var _controls_dialog := AcceptDialog.new()
var _settings_dialog := AcceptDialog.new()
var _columns := GridContainer.new()
var _brand := VBoxContainer.new()


## 스크롤 가능한 두 열 메뉴를 구성하고 코스 목록을 바인딩한다.
func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var background := ColorRect.new()
	background.color = Color("0b1420")
	background.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(background)
	background.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var scroll := ScrollContainer.new()
	add_child(scroll)
	scroll.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	var margin := MarginContainer.new()
	margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	for side in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 32)
	scroll.add_child(margin)
	_columns.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_columns.add_theme_constant_override("h_separation", 40)
	_columns.add_theme_constant_override("v_separation", 24)
	margin.add_child(_columns)
	_brand.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_brand.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_brand.add_theme_constant_override("separation", 18)
	_columns.add_child(_brand)
	_label(_brand, "DRIVE / TRAINING", 16, Color("65e4ce"))
	_label(_brand, "주행 교육\n시뮬레이터", 40)
	_label(_brand, "차량을 직접 조작하며 주행과 안전 정지를 연습합니다.", 18)
	_label(_brand, "01  코스 또는 시나리오 선택\n02  시동 · 기어 선택\n03  주행 · 정지 · 결과 확인", 16, Color("adc1d0"))
	_label(_brand, "지도 출처와 실측 여부는 각 코스 안내를 확인하세요.\n차량 거동과 평가 기준은 교육용 임시값입니다.", 14, Color("90a6b8"))
	var panel := PanelContainer.new()
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_columns.add_child(panel)
	var style := StyleBoxFlat.new()
	style.bg_color = Color("162536")
	style.border_color = Color("385a6d")
	style.set_border_width_all(1)
	style.set_corner_radius_all(16)
	style.set_content_margin_all(24)
	panel.add_theme_stylebox_override("panel", style)
	var actions := VBoxContainer.new()
	actions.add_theme_constant_override("separation", 14)
	panel.add_child(actions)
	_label(actions, "주행 준비", 26)
	_label(actions, "코스 선택", 14, Color("adc1d0"))
	courses.custom_minimum_size.y = 46
	courses.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	courses.fit_to_longest_item = false
	courses.clip_text = true
	courses.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	actions.add_child(courses)
	_button(actions, start_button, "선택한 코스 시작", _open_course, true)
	_button(actions, map_button, "경로 선택 · 지도", _open_map)
	_button(actions, controls_button, "조작법 안내", _toggle_controls)
	_button(actions, settings_button, "실행 환경 안내", _toggle_settings)
	_button(actions, Button.new(), "프로그램 종료", _request_quit)
	_build_dialogs()
	_populate_courses()
	resized.connect(_layout)
	_layout()
	courses.grab_focus()


## 좁은 창에서는 제목 열을 접어 모든 조작 버튼에 접근할 수 있게 한다.
func _layout() -> void:
	var wide := size.x >= 1000
	_columns.columns = 2 if wide else 1
	_brand.visible = wide


## 자동 줄바꿈되는 설명 라벨을 부모 컨테이너에 추가한다.
func _label(parent: Control, text: String, font_size: int, color: Color = Color.WHITE) -> Label:
	var label := Label.new()
	label.text = text
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.add_theme_font_size_override("font_size", font_size)
	label.add_theme_color_override("font_color", color)
	parent.add_child(label)
	return label


## 키보드 포커스와 마우스 호버가 보이는 실제 동작 버튼을 만든다.
func _button(parent: Control, button: Button, text: String, action: Callable, primary: bool = false) -> void:
	button.text = text
	button.custom_minimum_size.y = 46
	button.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	var style := StyleBoxFlat.new()
	style.bg_color = Color("237e73") if primary else Color("22394d")
	style.set_corner_radius_all(8)
	style.set_content_margin_all(12)
	button.add_theme_stylebox_override("normal", style)
	var hover := style.duplicate() as StyleBoxFlat
	hover.bg_color = Color("31958a") if primary else Color("34546b")
	hover.set_border_width_all(2)
	hover.border_color = Color("65e4ce")
	for state in ["hover", "focus", "pressed"]:
		button.add_theme_stylebox_override(state, hover)
	button.pressed.connect(action)
	parent.add_child(button)


## 실제 기본 키 설정과 일치하는 도움말 및 읽기 전용 환경 안내 창을 만든다.
func _build_dialogs() -> void:
	_controls_dialog.title = "기본 조작법 · config/input.toml에서 변경 가능"
	_controls_dialog.dialog_text = "Enter: 시동 켜기/끄기\nJ: D · L: R · K: N · I: P\nW/↑: 선택한 기어 방향으로 가속\nS/↓: D·R 공통 제동 (후진 기어 전환 아님)\nA/D, ←/→: 조향\nSpace: 비상 정지 · Backspace: 비상 정지 해제\nR: 정지 후 시작 위치 복귀\n1/2/3/4: 전방/후방/좌/우 · C: 탑다운 보기\nF5/F6/F7/F8: 시나리오 시작/일시정지/재시작/종료\nEsc: 메뉴 복귀"
	_settings_dialog.title = "실행 환경 안내"
	_settings_dialog.dialog_text = "차량 물리: 초당 64틱\n시점: 실내 기본 · C키로 탑다운 전환\n입력 설정: config/input.toml\n차량 설정: config/vehicle.toml\n카메라 설정: config/camera.toml\n이 창은 안내용이며 설정 파일을 변경하지 않습니다."
	add_child(_controls_dialog)
	add_child(_settings_dialog)


## 도움말을 실제 모달 창으로 열어 뒤쪽 메뉴의 클릭을 차단한다.
func _toggle_controls() -> void:
	_settings_dialog.hide()
	_controls_dialog.popup_centered()


## 환경 안내를 모달 창으로 연다.
func _toggle_settings() -> void:
	_controls_dialog.hide()
	_settings_dialog.popup_centered()


## 기본 코스 뒤에 저장소 코스·저장 경로·시나리오를 중복 없이 등록한다.
func _populate_courses() -> void:
	_add_course("개발 코스 · Training", "training")
	_add_course("차량 보정 코스 · Calibration", "calibration")
	if ClassDB.class_exists("SimVehicle"):
		_add_course("여수 엑스포 · OSM 배경", DriveScreen.YEOSU_COURSE)
		_add_course("여수 엑스포 · 내 맵 도로", DriveScreen.YEOSU_CUSTOM_ROADS_COURSE)
		for key: String in DriveScreen.CONVENTION_COURSES:
			_add_course("여수 컨벤션 · " + DriveScreen.CONVENTION_COURSES[key], DriveScreen.CONVENTION_PREFIX + key + ".toml")
	if ClassDB.class_exists("SimCourse"):
		for file in ClassDB.class_call_static("SimCourse", "course_files"):
			var label := "지도: " + str(file.label)
			_add_course(label, str(file.course))
	if ClassDB.class_exists("SimScenario"):
		for file in ClassDB.class_call_static("SimScenario", "scenario_files"):
			_add_course("시나리오: " + str(file.label), "scenario:" + str(file.path))


## 표시 항목과 코스 식별자를 같은 순서로 저장한다.
func _add_course(label: String, course: String) -> void:
	if course in course_names:
		return
	courses.add_item(label)
	course_names.append(course)


## 선택한 코스가 있을 때만 주행 화면 전환을 요청한다.
func _open_course() -> void:
	if courses.selected >= 0 and courses.selected < course_names.size():
		drive_requested.emit(course_names[courses.selected])


## 기존 OSM 경로 선택 화면 전환을 요청한다.
func _open_map() -> void:
	map_requested.emit()


## 상위 화면 관리자에게 종료를 요청한다.
func _request_quit() -> void:
	quit_requested.emit()
