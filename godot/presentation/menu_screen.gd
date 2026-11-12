extends Control
## 개발 코스·지도 코스(저장소 코스와 저장 경로)·교육 시나리오 선택, 지도 화면, 종료를 제공하는 메인 메뉴다.

const DriveScreen = preload("res://presentation/drive_screen.gd")

signal drive_requested(course: String)
signal map_requested
signal quit_requested
var courses := OptionButton.new()
## 선택 항목 번호 순서의 코스 선택 문자열(`training`, `calibration`, `map:<파일>`, `scenario:<파일>`).
var course_names := PackedStringArray()

## 창 크기에 맞춘 중앙 메뉴와 현재 주행 연결 안내를 구성한다.
func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	var background := ColorRect.new()
	background.color = Color("17242f")
	background.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(background)
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(center)
	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(400, 0)
	center.add_child(panel)
	var margin := MarginContainer.new()
	for side in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 28)
	panel.add_child(margin)
	var column := VBoxContainer.new()
	column.add_theme_constant_override("separation", 16)
	margin.add_child(column)
	var title := Label.new()
	title.text = "주행 시뮬레이터"
	title.add_theme_font_size_override("font_size", 30)
	column.add_child(title)
	var info := Label.new()
	info.text = "개발 코스 또는 지도 코스를 골라 주행합니다.\n지도 코스는 OSM 도로로 만든 가상 교육 코스입니다(실측 아님)."
	if not ClassDB.class_exists("SimVehicle"):
		info.text = "주행 기능 연결 전 · 정지 미리보기\n코스와 네 방향 시야를 확인할 수 있습니다."
	column.add_child(info)
	_add_course("개발 코스", "training")
	_add_course("보정 코스", "calibration")
	if ClassDB.class_exists("SimVehicle"):
		_add_course("여수 엑스포 · OSM 배경", DriveScreen.YEOSU_COURSE)
	if ClassDB.class_exists("SimCourse") and ClassDB.class_has_method("SimCourse", "course_files"):
		for file in ClassDB.class_call_static("SimCourse", "course_files"):
			_add_course("지도: " + str(file.label), str(file.course))
	if ClassDB.class_exists("SimScenario"):
		for file in ClassDB.class_call_static("SimScenario", "scenario_files"):
			_add_course("시나리오: " + str(file.label), "scenario:" + str(file.path))
	courses.fit_to_longest_item = false
	courses.clip_text = true
	column.add_child(courses)
	_add_button(column, "선택한 코스 열기", _open_course)
	_add_button(column, "경로 선택·지도", _open_map)
	_add_button(column, "종료", _request_quit)
	courses.grab_focus()

## 메뉴 버튼을 생성해 지정한 동작에 연결한다.
func _add_button(parent: Control, text: String, action: Callable) -> void:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size.y = 42
	button.pressed.connect(action)
	parent.add_child(button)

## 코스 목록에 이름과 선택 문자열을 하나 더한다.
func _add_course(label: String, course: String) -> void:
	courses.add_item(label, course_names.size())
	course_names.append(course)

## 선택한 코스 선택 문자열만 전달한다. 주행 상태는 여기서 만들지 않는다.
func _open_course() -> void:
	drive_requested.emit(course_names[maxi(courses.selected, 0)])

## 지도 화면으로 이동을 요청한다.
func _open_map() -> void:
	map_requested.emit()

## 사용자 종료 요청을 상위 화면 관리자에게 전달한다.
func _request_quit() -> void:
	quit_requested.emit()