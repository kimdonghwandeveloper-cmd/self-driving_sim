extends Control
## 첨부 UI 시안 기반의 자율주행 교육 시뮬레이터 메인 메뉴 화면.
## 코스/시나리오 선택, 조작법 안내, 환경 설정 및 프로그램 종료 기능을 제공한다.

const DriveScreen = preload("res://presentation/drive_screen.gd")
const BG_TEXTURE_PATH := "res://assets/ui/main_menu_bg.png"

signal drive_requested(course: String)
signal map_requested
signal quit_requested

var courses := OptionButton.new()
## 선택 항목 번호 순서의 코스 선택 문자열(`training`, `calibration`, `map:<파일>`, `scenario:<파일>`).
var course_names := PackedStringArray()

var _controls_dialog: Control
var _settings_dialog: Control

## 메인 메뉴 화면 레이아웃과 배경, 헤더, 우측 메뉴 카드, 팝업 다이얼로그를 초기화한다.
func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_build_background()
	_build_header()
	_build_main_card()
	_build_footer_tag()
	_build_dialogs()
	_populate_courses()
	courses.grab_focus()

## 여수 엑스포 셔틀버스 고화질 배경 이미지를 배치하고, 미존재 시 딥블루 그라데이션으로 대체한다.
func _build_background() -> void:
	if ResourceLoader.exists(BG_TEXTURE_PATH):
		var bg_tex := load(BG_TEXTURE_PATH) as Texture2D
		if bg_tex != null:
			var tex_rect := TextureRect.new()
			tex_rect.texture = bg_tex
			tex_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
			tex_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
			tex_rect.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
			tex_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
			add_child(tex_rect)
			return

	var fallback_bg := ColorRect.new()
	fallback_bg.color = Color("0d1721")
	fallback_bg.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(fallback_bg)

## 좌측 상단 로고와 우측 상단 버전 표기를 구성한다.
func _build_header() -> void:
	var top_bar := MarginContainer.new()
	top_bar.set_anchors_and_offsets_preset(Control.PRESET_TOP_WIDE)
	top_bar.add_theme_constant_override("margin_left", 36)
	top_bar.add_theme_constant_override("margin_top", 28)
	top_bar.add_theme_constant_override("margin_right", 36)
	add_child(top_bar)

	var header_hbox := HBoxContainer.new()
	header_hbox.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	top_bar.add_child(header_hbox)

	var logo_vbox := VBoxContainer.new()
	logo_vbox.add_theme_constant_override("separation", 0)
	header_hbox.add_child(logo_vbox)

	var logo_title := Label.new()
	logo_title.text = "🚗 DRIVE TRAINING"
	logo_title.add_theme_font_size_override("font_size", 17)
	logo_title.add_theme_color_override("font_color", Color.WHITE)
	logo_vbox.add_child(logo_title)

	var logo_sub := Label.new()
	logo_sub.text = "    SIMULATOR"
	logo_sub.add_theme_font_size_override("font_size", 12)
	logo_sub.add_theme_color_override("font_color", Color("90a4ae"))
	logo_vbox.add_child(logo_sub)

	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header_hbox.add_child(spacer)

	var version_label := Label.new()
	version_label.text = "v0.1.0"
	version_label.add_theme_font_size_override("font_size", 15)
	version_label.add_theme_color_override("font_color", Color("b0bec5"))
	header_hbox.add_child(version_label)

## 우측 메인 타이틀 및 인터랙티브 액션 버튼 카드(글래스모피즘 스타일)를 구성한다.
func _build_main_card() -> void:
	var right_margin := MarginContainer.new()
	right_margin.set_anchors_and_offsets_preset(Control.PRESET_RIGHT_WIDE)
	right_margin.add_theme_constant_override("margin_right", 54)
	right_margin.add_theme_constant_override("margin_top", 100)
	right_margin.add_theme_constant_override("margin_bottom", 60)
	add_child(right_margin)

	var card_panel := PanelContainer.new()
	card_panel.custom_minimum_size = Vector2(430, 0)
	card_panel.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var panel_style := StyleBoxFlat.new()
	panel_style.bg_color = Color(0.06, 0.12, 0.18, 0.78)
	panel_style.corner_radius_top_left = 18
	panel_style.corner_radius_top_right = 18
	panel_style.corner_radius_bottom_left = 18
	panel_style.corner_radius_bottom_right = 18
	panel_style.border_width_left = 1
	panel_style.border_width_top = 1
	panel_style.border_width_right = 1
	panel_style.border_width_bottom = 1
	panel_style.border_color = Color(0.28, 0.45, 0.55, 0.45)
	card_panel.add_theme_stylebox_override("panel", panel_style)
	right_margin.add_child(card_panel)

	var card_margin := MarginContainer.new()
	for side in ["left", "top", "right", "bottom"]:
		card_margin.add_theme_constant_override("margin_" + side, 30)
	card_panel.add_child(card_margin)

	var column := VBoxContainer.new()
	column.add_theme_constant_override("separation", 18)
	card_margin.add_child(column)

	# 타이틀 영역
	var title_vbox := VBoxContainer.new()
	title_vbox.add_theme_constant_override("separation", 4)
	column.add_child(title_vbox)

	var title_accent := Label.new()
	title_accent.text = "DRIVING"
	title_accent.add_theme_font_size_override("font_size", 34)
	title_accent.add_theme_color_override("font_color", Color("4ee1d4"))
	title_vbox.add_child(title_accent)

	var title_main := Label.new()
	title_main.text = "SIMULATOR"
	title_main.add_theme_font_size_override("font_size", 38)
	title_main.add_theme_color_override("font_color", Color.WHITE)
	title_vbox.add_child(title_main)

	var subtitle := Label.new()
	subtitle.text = "자율주행 차량 운전 교육 시뮬레이터"
	subtitle.add_theme_font_size_override("font_size", 15)
	subtitle.add_theme_color_override("font_color", Color("b0c4de"))
	title_vbox.add_child(subtitle)

	# 코스 선택 드롭다운 영역
	var course_section := VBoxContainer.new()
	course_section.add_theme_constant_override("separation", 6)
	column.add_child(course_section)

	var course_label := Label.new()
	course_label.text = "주행 코스 선택"
	course_label.add_theme_font_size_override("font_size", 13)
	course_label.add_theme_color_override("font_color", Color("7ec8e3"))
	course_section.add_child(course_label)

	courses.custom_minimum_size.y = 40
	courses.fit_to_longest_item = false
	courses.clip_text = true
	var dropdown_style := StyleBoxFlat.new()
	dropdown_style.bg_color = Color(0.12, 0.20, 0.28, 0.85)
	dropdown_style.corner_radius_top_left = 8
	dropdown_style.corner_radius_top_right = 8
	dropdown_style.corner_radius_bottom_left = 8
	dropdown_style.corner_radius_bottom_right = 8
	dropdown_style.border_width_left = 1
	dropdown_style.border_width_top = 1
	dropdown_style.border_width_right = 1
	dropdown_style.border_width_bottom = 1
	dropdown_style.border_color = Color("385f7a")
	courses.add_theme_stylebox_override("normal", dropdown_style)
	courses.add_theme_stylebox_override("hover", dropdown_style)
	courses.add_theme_stylebox_override("pressed", dropdown_style)
	course_section.add_child(courses)

	# 메인 액션 버튼 목록
	var buttons_vbox := VBoxContainer.new()
	buttons_vbox.add_theme_constant_override("separation", 12)
	column.add_child(buttons_vbox)

	_create_action_button(buttons_vbox, "▶   시뮬레이션 시작", _open_course, true)
	_create_action_button(buttons_vbox, "🗺   경로 선택 · 지도", _open_map, false)
	_create_action_button(buttons_vbox, "🎮   조작법 안내", _toggle_controls, false)
	_create_action_button(buttons_vbox, "⚙   환경 설정", _toggle_settings, false)
	_create_action_button(buttons_vbox, "⏻   프로그램 종료", _request_quit, false)

## 좌측 하단 여수 엑스포 교육용 시뮬레이션 위치 태그를 구성한다.
func _build_footer_tag() -> void:
	var bottom_margin := MarginContainer.new()
	bottom_margin.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_LEFT)
	bottom_margin.add_theme_constant_override("margin_left", 36)
	bottom_margin.add_theme_constant_override("margin_bottom", 28)
	add_child(bottom_margin)

	var pill_panel := PanelContainer.new()
	var pill_style := StyleBoxFlat.new()
	pill_style.bg_color = Color(0.08, 0.14, 0.20, 0.75)
	pill_style.corner_radius_top_left = 12
	pill_style.corner_radius_top_right = 12
	pill_style.corner_radius_bottom_left = 12
	pill_style.corner_radius_bottom_right = 12
	pill_style.border_width_left = 1
	pill_style.border_width_top = 1
	pill_style.border_width_right = 1
	pill_style.border_width_bottom = 1
	pill_style.border_color = Color(0.3, 0.5, 0.6, 0.4)
	pill_panel.add_theme_stylebox_override("panel", pill_style)
	bottom_margin.add_child(pill_panel)

	var pill_margin := MarginContainer.new()
	pill_margin.add_theme_constant_override("margin_left", 16)
	pill_margin.add_theme_constant_override("margin_top", 8)
	pill_margin.add_theme_constant_override("margin_right", 16)
	pill_margin.add_theme_constant_override("margin_bottom", 8)
	pill_panel.add_child(pill_margin)

	var tag_label := Label.new()
	tag_label.text = "📍 여수 엑스포역 공원   |   교육용 시뮬레이션"
	tag_label.add_theme_font_size_override("font_size", 13)
	tag_label.add_theme_color_override("font_color", Color("cfd8dc"))
	pill_margin.add_child(tag_label)

## 스타일이 적용된 액션 버튼을 생성하여 부모 노드에 추가한다.
func _create_action_button(parent: Control, text: String, action: Callable, is_primary: bool) -> Button:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size.y = 48
	button.alignment = HORIZONTAL_ALIGNMENT_LEFT
	button.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS

	var style_normal := StyleBoxFlat.new()
	style_normal.corner_radius_top_left = 12
	style_normal.corner_radius_top_right = 12
	style_normal.corner_radius_bottom_left = 12
	style_normal.corner_radius_bottom_right = 12

	if is_primary:
		# 민트/시안 강조 스타일
		style_normal.bg_color = Color("3ed8c9")
		button.add_theme_color_override("font_color", Color("092027"))
		button.add_theme_color_override("font_hover_color", Color("05161c"))
		button.add_theme_color_override("font_pressed_color", Color("000000"))
		button.add_theme_font_size_override("font_size", 16)
	else:
		# 다크 글래스모피즘 스타일
		style_normal.bg_color = Color(0.14, 0.22, 0.30, 0.72)
		style_normal.border_width_left = 1
		style_normal.border_width_top = 1
		style_normal.border_width_right = 1
		style_normal.border_width_bottom = 1
		style_normal.border_color = Color(0.32, 0.48, 0.60, 0.4)
		button.add_theme_color_override("font_color", Color.WHITE)
		button.add_theme_color_override("font_hover_color", Color("7de8db"))
		button.add_theme_font_size_override("font_size", 15)

	var style_hover := style_normal.duplicate() as StyleBoxFlat
	if is_primary:
		style_hover.bg_color = Color("5af2e4")
	else:
		style_hover.bg_color = Color(0.20, 0.32, 0.44, 0.88)
		style_hover.border_color = Color("4ee1d4")

	button.add_theme_stylebox_override("normal", style_normal)
	button.add_theme_stylebox_override("hover", style_hover)
	button.add_theme_stylebox_override("pressed", style_hover)
	button.pressed.connect(action)
	parent.add_child(button)
	return button

## 조작법 안내 및 환경 설정 팝업 다이얼로그를 생성한다.
func _build_dialogs() -> void:
	_controls_dialog = _create_modal_dialog("🎮 조작법 안내", [
		"W  /  ↑ : 전진 가속",
		"S  /  ↓ : 감속 및 후진",
		"A  /  D  /  ←  /  → : 조향 제어",
		"Space : 긴급 제동 (브레이크)",
		"R : 차량 위치 초기화 (리셋)",
		"C : 탑다운 3인칭 ↔ 1인칭 실내 시점 전환",
		"1 / 2 / 3 / 4 : 전방 / 후방 / 좌측 / 우측 시야",
		"Esc : 메인 메뉴로 복귀",
		"F5 / F6 / F7 / F8 : 시나리오 시작 / 일시정지 / 재시작 / 종료"
	])
	add_child(_controls_dialog)

	_settings_dialog = _create_modal_dialog("⚙ 환경 설정", [
		"디스플레이: 1280 x 720 (기본 해상도)",
		"물리 시뮬레이션: 64 Hz 고정밀 틱",
		"렌더러: Godot 4.x Forward+ / Compatibility 자동 전환",
		"시점 기본값: 탑다운(Top-down) 3인칭 뷰 지원",
		"지도 형식: Lanelet2 HD Map & OSM Graph 지원"
	])
	add_child(_settings_dialog)

## 공통 모달 다이얼로그 위젯을 생성한다.
func _create_modal_dialog(title_text: String, items: Array) -> Control:
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	center.mouse_filter = Control.MOUSE_FILTER_STOP

	var panel := PanelContainer.new()
	panel.custom_minimum_size = Vector2(460, 0)
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.14, 0.22, 0.94)
	style.corner_radius_top_left = 16
	style.corner_radius_top_right = 16
	style.corner_radius_bottom_left = 16
	style.corner_radius_bottom_right = 16
	style.border_width_left = 2
	style.border_width_top = 2
	style.border_width_right = 2
	style.border_width_bottom = 2
	style.border_color = Color("4ee1d4")
	panel.add_theme_stylebox_override("panel", style)
	center.add_child(panel)

	var margin := MarginContainer.new()
	for side in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + side, 24)
	panel.add_child(margin)

	var column := VBoxContainer.new()
	column.add_theme_constant_override("separation", 14)
	margin.add_child(column)

	var title := Label.new()
	title.text = title_text
	title.add_theme_font_size_override("font_size", 20)
	title.add_theme_color_override("font_color", Color("4ee1d4"))
	column.add_child(title)

	for item in items:
		var lbl := Label.new()
		lbl.text = "• " + str(item)
		lbl.add_theme_font_size_override("font_size", 14)
		lbl.add_theme_color_override("font_color", Color("eceff1"))
		column.add_child(lbl)

	var close_btn := Button.new()
	close_btn.text = "닫기"
	close_btn.custom_minimum_size.y = 38
	close_btn.pressed.connect(func(): center.hide())
	column.add_child(close_btn)

	center.hide()
	return center

## 조작법 안내 모달의 표시 상태를 토글한다.
func _toggle_controls() -> void:
	if _settings_dialog.visible:
		_settings_dialog.hide()
	_controls_dialog.visible = not _controls_dialog.visible

## 환경 설정 모달의 표시 상태를 토글한다.
func _toggle_settings() -> void:
	if _controls_dialog.visible:
		_controls_dialog.hide()
	_settings_dialog.visible = not _settings_dialog.visible

## 시스템에 등록된 코스 및 시나리오 목록을 조회하여 드롭다운에 추가한다.
func _populate_courses() -> void:
	_add_course("여수엑스포 교차로 · Lanelet2 실측 HD 맵", "map:assets/maps/lanelet2-real-demo/course.toml")
	_add_course("여수엑스포 공원 · OSM 배경 코스", DriveScreen.YEOSU_COURSE)
	_add_course("여수엑스포 공원 · 내 맵 도로 코스", DriveScreen.YEOSU_CUSTOM_ROADS_COURSE)
	_add_course("Lanelet2 예제 · 합성 직선 코스", "map:assets/maps/lanelet2-demo/course.toml")
	_add_course("기본 주행 개발 코스 (Training)", "training")
	_add_course("차량 거동 보정 측정 코스 (Calibration)", "calibration")

	if ClassDB.class_exists("SimCourse") and ClassDB.class_has_method("SimCourse", "course_files"):
		for file in ClassDB.class_call_static("SimCourse", "course_files"):
			var course_path := str(file.course)
			if course_path not in course_names:
				var label := "지도: " + str(file.label)
				_add_course(label, course_path)

	if ClassDB.class_exists("SimScenario"):
		for file in ClassDB.class_call_static("SimScenario", "scenario_files"):
			_add_course("시나리오: " + str(file.label), "scenario:" + str(file.path))

## 코스 목록에 이름과 선택 문자열을 하나 더한다.
func _add_course(label: String, course: String) -> void:
	courses.add_item(label, course_names.size())
	course_names.append(course)

## 선택한 코스 선택 문자열을 전달하여 주행 화면을 연다.
func _open_course() -> void:
	if _controls_dialog.visible or _settings_dialog.visible:
		_controls_dialog.hide()
		_settings_dialog.hide()
	var selected_idx := maxi(courses.selected, 0)
	if selected_idx < course_names.size():
		drive_requested.emit(course_names[selected_idx])
	else:
		drive_requested.emit("training")

## 지도 화면으로 이동을 요청한다.
func _open_map() -> void:
	if _controls_dialog.visible or _settings_dialog.visible:
		_controls_dialog.hide()
		_settings_dialog.hide()
	map_requested.emit()

## 사용자 종료 요청을 상위 화면 관리자에게 전달한다.
func _request_quit() -> void:
	quit_requested.emit()
