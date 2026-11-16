extends Control
## 첨부 UI 시안 기반의 자율주행 교육 시뮬레이터 메인 메뉴 화면.
## 순수 Godot UI 노드로 구성되어 코스/시나리오 선택, 시뮬레이션 시작, 조작법 안내, 환경 설정 및 프로그램 종료 기능을 제공한다.

const DriveScreen = preload("res://presentation/drive_screen.gd")

signal drive_requested(course: String)
signal map_requested
signal quit_requested

var courses := OptionButton.new()
## 선택 항목 번호 순서의 코스 선택 문자열(`training`, `calibration`, `map:<파일>`, `scenario:<파일>`).
var course_names := PackedStringArray()

var _controls_dialog: Control
var _settings_dialog: Control

## 메인 메뉴 화면 레이아웃과 배경, 헤더, 좌측 브랜딩, 우측 인터랙티브 카드, 팝업 다이얼로그를 초기화한다.
func _ready() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_build_procedural_background()
	_build_header()
	_build_content_layout()
	_build_dialogs()
	_populate_courses()
	courses.grab_focus()

## 고해상도 다크 테크 그라데이션 및 그리드 액센트로 세련된 프로시저럴 배경을 구성한다.
func _build_procedural_background() -> void:
	# 베이스 딥 다크 블루 배경
	var bg_rect := ColorRect.new()
	bg_rect.color = Color("0a111a")
	bg_rect.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	bg_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(bg_rect)

	# 상단-하단 그라데이션 오버레이 패널
	var grad_panel := Panel.new()
	grad_panel.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	grad_panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var grad_style := StyleBoxFlat.new()
	grad_style.bg_color = Color(0.06, 0.12, 0.18, 0.65)
	grad_style.border_width_bottom = 2
	grad_style.border_color = Color(0.2, 0.4, 0.5, 0.25)
	grad_panel.add_theme_stylebox_override("panel", grad_style)
	add_child(grad_panel)

## 좌측 상단 시뮬레이터 로고와 우측 상단 시스템 버전 뱃지를 구성한다.
func _build_header() -> void:
	var top_bar := MarginContainer.new()
	top_bar.set_anchors_and_offsets_preset(Control.PRESET_TOP_WIDE)
	top_bar.add_theme_constant_override("margin_left", 48)
	top_bar.add_theme_constant_override("margin_top", 32)
	top_bar.add_theme_constant_override("margin_right", 48)
	top_bar.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(top_bar)

	var header_hbox := HBoxContainer.new()
	header_hbox.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	header_hbox.mouse_filter = Control.MOUSE_FILTER_IGNORE
	top_bar.add_child(header_hbox)

	var logo_badge := PanelContainer.new()
	var badge_style := StyleBoxFlat.new()
	badge_style.bg_color = Color(0.12, 0.22, 0.30, 0.85)
	badge_style.corner_radius_top_left = 8
	badge_style.corner_radius_top_right = 8
	badge_style.corner_radius_bottom_left = 8
	badge_style.corner_radius_bottom_right = 8
	badge_style.border_width_left = 1
	badge_style.border_width_top = 1
	badge_style.border_width_right = 1
	badge_style.border_width_bottom = 1
	badge_style.border_color = Color("4ee1d4")
	logo_badge.add_theme_stylebox_override("panel", badge_style)
	header_hbox.add_child(logo_badge)

	var badge_margin := MarginContainer.new()
	badge_margin.add_theme_constant_override("margin_left", 14)
	badge_margin.add_theme_constant_override("margin_top", 6)
	badge_margin.add_theme_constant_override("margin_right", 14)
	badge_margin.add_theme_constant_override("margin_bottom", 6)
	logo_badge.add_child(badge_margin)

	var logo_text := Label.new()
	logo_text.text = "🚗 DRIVE TRAINING SIMULATOR"
	logo_text.add_theme_font_size_override("font_size", 13)
	logo_text.add_theme_color_override("font_color", Color("4ee1d4"))
	badge_margin.add_child(logo_text)

	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	spacer.mouse_filter = Control.MOUSE_FILTER_IGNORE
	header_hbox.add_child(spacer)

	var version_label := Label.new()
	version_label.text = "v0.1.0 • Godot 4 & Rust GDExtension"
	version_label.add_theme_font_size_override("font_size", 13)
	version_label.add_theme_color_override("font_color", Color("78909c"))
	header_hbox.add_child(version_label)

## 좌측 정보 영역과 우측 인터랙티브 메뉴 카드를 포함하는 메인 2열 레이아웃을 생성한다.
func _build_content_layout() -> void:
	var main_margin := MarginContainer.new()
	main_margin.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	main_margin.add_theme_constant_override("margin_left", 56)
	main_margin.add_theme_constant_override("margin_top", 96)
	main_margin.add_theme_constant_override("margin_right", 56)
	main_margin.add_theme_constant_override("margin_bottom", 40)
	main_margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(main_margin)

	var hbox := HBoxContainer.new()
	hbox.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	hbox.add_theme_constant_override("separation", 48)
	hbox.mouse_filter = Control.MOUSE_FILTER_IGNORE
	main_margin.add_child(hbox)

	# 1. 좌측 브랜딩 및 시스템 안내 영역
	var left_vbox := VBoxContainer.new()
	left_vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left_vbox.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	left_vbox.add_theme_constant_override("separation", 20)
	left_vbox.mouse_filter = Control.MOUSE_FILTER_IGNORE
	hbox.add_child(left_vbox)

	_build_left_branding(left_vbox)

	# 2. 우측 인터랙티브 액션 카드 영역
	var right_container := VBoxContainer.new()
	right_container.size_flags_horizontal = Control.SIZE_SHRINK_END
	right_container.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	right_container.custom_minimum_size = Vector2(440, 0)
	hbox.add_child(right_container)

	_build_right_menu_card(right_container)

## 좌측 시뮬레이터 타이틀, 설명 및 실시간 기능 목록을 렌더링한다.
func _build_left_branding(parent: VBoxContainer) -> void:
	var title_accent := Label.new()
	title_accent.text = "AUTONOMOUS VEHICLE"
	title_accent.add_theme_font_size_override("font_size", 20)
	title_accent.add_theme_color_override("font_color", Color("4ee1d4"))
	parent.add_child(title_accent)

	var main_title := Label.new()
	main_title.text = "자율주행 운전 교육 시뮬레이터"
	main_title.add_theme_font_size_override("font_size", 34)
	main_title.add_theme_color_override("font_color", Color.WHITE)
	parent.add_child(main_title)

	var desc := Label.new()
	desc.text = "고정밀 HD 맵(Lanelet2)과 64Hz 물리 동역학 엔진 기반의\n실시간 자율주행 차량 거동 및 안전 평가 시뮬레이션 환경"
	desc.add_theme_font_size_override("font_size", 15)
	desc.add_theme_color_override("font_color", Color("b0bec5"))
	desc.add_theme_constant_override("line_spacing", 6)
	parent.add_child(desc)

	var sep := HSeparator.new()
	sep.custom_minimum_size.x = 240
	parent.add_child(sep)

	var features_vbox := VBoxContainer.new()
	features_vbox.add_theme_constant_override("separation", 10)
	parent.add_child(features_vbox)

	var feats := [
		"✓  Lanelet2 실측 HD 맵 및 차선 위상 토폴로지 지원",
		"✓  64 Hz 고정밀 차량 동역학 및 타이어 충돌 해석",
		"✓  실시간 경로 이탈 오차(CTE) 및 정밀 주행 성과 메트릭",
		"✓  상단 탑다운(Top-down) 3인칭 및 운전자 1인칭 시점"
	]
	for f in feats:
		var f_lbl := Label.new()
		f_lbl.text = f
		f_lbl.add_theme_font_size_override("font_size", 14)
		f_lbl.add_theme_color_override("font_color", Color("90a4ae"))
		features_vbox.add_child(f_lbl)

	var spacer := Control.new()
	spacer.custom_minimum_size.y = 12
	parent.add_child(spacer)

	var loc_badge := PanelContainer.new()
	loc_badge.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	var loc_style := StyleBoxFlat.new()
	loc_style.bg_color = Color(0.08, 0.15, 0.22, 0.75)
	loc_style.corner_radius_top_left = 10
	loc_style.corner_radius_top_right = 10
	loc_style.corner_radius_bottom_left = 10
	loc_style.corner_radius_bottom_right = 10
	loc_style.border_width_left = 1
	loc_style.border_width_top = 1
	loc_style.border_width_right = 1
	loc_style.border_width_bottom = 1
	loc_style.border_color = Color(0.3, 0.5, 0.65, 0.4)
	loc_badge.add_theme_stylebox_override("panel", loc_style)
	parent.add_child(loc_badge)

	var loc_m := MarginContainer.new()
	loc_m.add_theme_constant_override("margin_left", 14)
	loc_m.add_theme_constant_override("margin_top", 6)
	loc_m.add_theme_constant_override("margin_right", 14)
	loc_m.add_theme_constant_override("margin_bottom", 6)
	loc_badge.add_child(loc_m)

	var loc_text := Label.new()
	loc_text.text = "📍 여수 엑스포역 공원 · 스마트 시티 테스트베드"
	loc_text.add_theme_font_size_override("font_size", 12)
	loc_text.add_theme_color_override("font_color", Color("cfd8dc"))
	loc_m.add_child(loc_text)

## 우측 글래스모피즘 메인 액션 카드(코스 선택 및 버튼 목록)를 구성한다.
func _build_right_menu_card(parent: VBoxContainer) -> void:
	var card_panel := PanelContainer.new()
	var panel_style := StyleBoxFlat.new()
	panel_style.bg_color = Color(0.07, 0.13, 0.20, 0.88)
	panel_style.corner_radius_top_left = 18
	panel_style.corner_radius_top_right = 18
	panel_style.corner_radius_bottom_left = 18
	panel_style.corner_radius_bottom_right = 18
	panel_style.border_width_left = 1
	panel_style.border_width_top = 1
	panel_style.border_width_right = 1
	panel_style.border_width_bottom = 1
	panel_style.border_color = Color("3ed8c9")
	panel_style.shadow_color = Color(0, 0, 0, 0.45)
	panel_style.shadow_size = 16
	panel_style.shadow_offset = Vector2(0, 8)
	card_panel.add_theme_stylebox_override("panel", panel_style)
	parent.add_child(card_panel)

	var card_margin := MarginContainer.new()
	for side in ["left", "top", "right", "bottom"]:
		card_margin.add_theme_constant_override("margin_" + side, 28)
	card_panel.add_child(card_margin)

	var column := VBoxContainer.new()
	column.add_theme_constant_override("separation", 18)
	card_margin.add_child(column)

	# 카드 헤더
	var card_title_vbox := VBoxContainer.new()
	card_title_vbox.add_theme_constant_override("separation", 2)
	column.add_child(card_title_vbox)

	var card_accent := Label.new()
	card_accent.text = "DRIVING SIMULATOR"
	card_accent.add_theme_font_size_override("font_size", 13)
	card_accent.add_theme_color_override("font_color", Color("4ee1d4"))
	card_title_vbox.add_child(card_accent)

	var card_title := Label.new()
	card_title.text = "시뮬레이션 코스 및 설정"
	card_title.add_theme_font_size_override("font_size", 22)
	card_title.add_theme_color_override("font_color", Color.WHITE)
	card_title_vbox.add_child(card_title)

	# 코스 선택 드롭다운 영역
	var course_section := VBoxContainer.new()
	course_section.add_theme_constant_override("separation", 8)
	column.add_child(course_section)

	var course_label := Label.new()
	course_label.text = "주행 코스 선택"
	course_label.add_theme_font_size_override("font_size", 13)
	course_label.add_theme_color_override("font_color", Color("7ec8e3"))
	course_section.add_child(course_label)

	courses.custom_minimum_size.y = 44
	courses.fit_to_longest_item = false
	courses.clip_text = true
	courses.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	var dropdown_style := StyleBoxFlat.new()
	dropdown_style.bg_color = Color(0.12, 0.20, 0.30, 0.95)
	dropdown_style.corner_radius_top_left = 10
	dropdown_style.corner_radius_top_right = 10
	dropdown_style.corner_radius_bottom_left = 10
	dropdown_style.corner_radius_bottom_right = 10
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
	buttons_vbox.add_theme_constant_override("separation", 10)
	column.add_child(buttons_vbox)

	_create_action_button(buttons_vbox, "▶   시뮬레이션 시작", _open_course, true)
	_create_action_button(buttons_vbox, "🗺   경로 선택 · 지도", _open_map, false)
	_create_action_button(buttons_vbox, "🎮   조작법 안내", _toggle_controls, false)
	_create_action_button(buttons_vbox, "⚙   환경 설정", _toggle_settings, false)
	_create_action_button(buttons_vbox, "⏻   프로그램 종료", _request_quit, false)

## 스타일 및 클릭 이벤트가 연결된 액션 버튼을 생성한다.
func _create_action_button(parent: Control, text: String, action: Callable, is_primary: bool) -> Button:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size.y = 48
	button.alignment = HORIZONTAL_ALIGNMENT_LEFT
	button.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	button.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND

	var style_normal := StyleBoxFlat.new()
	style_normal.corner_radius_top_left = 12
	style_normal.corner_radius_top_right = 12
	style_normal.corner_radius_bottom_left = 12
	style_normal.corner_radius_bottom_right = 12

	if is_primary:
		# 메인 강조 민트/시안 버튼
		style_normal.bg_color = Color("3ed8c9")
		button.add_theme_color_override("font_color", Color("081d24"))
		button.add_theme_color_override("font_hover_color", Color("000000"))
		button.add_theme_color_override("font_pressed_color", Color("000000"))
		button.add_theme_font_size_override("font_size", 16)
	else:
		# 다크 글래스모피즘 보조 버튼
		style_normal.bg_color = Color(0.13, 0.22, 0.32, 0.75)
		style_normal.border_width_left = 1
		style_normal.border_width_top = 1
		style_normal.border_width_right = 1
		style_normal.border_width_bottom = 1
		style_normal.border_color = Color(0.30, 0.46, 0.60, 0.45)
		button.add_theme_color_override("font_color", Color.WHITE)
		button.add_theme_color_override("font_hover_color", Color("6ef5e5"))
		button.add_theme_font_size_override("font_size", 14)

	var style_hover := style_normal.duplicate() as StyleBoxFlat
	if is_primary:
		style_hover.bg_color = Color("58f7e7")
	else:
		style_hover.bg_color = Color(0.20, 0.32, 0.45, 0.92)
		style_hover.border_color = Color("4ee1d4")

	var style_pressed := style_normal.duplicate() as StyleBoxFlat
	if is_primary:
		style_pressed.bg_color = Color("2eb8aa")
	else:
		style_pressed.bg_color = Color(0.10, 0.16, 0.24, 0.95)

	button.add_theme_stylebox_override("normal", style_normal)
	button.add_theme_stylebox_override("hover", style_hover)
	button.add_theme_stylebox_override("pressed", style_pressed)
	button.add_theme_stylebox_override("focus", style_hover)
	button.pressed.connect(action)
	parent.add_child(button)
	return button

## 조작법 안내 및 환경 설정 팝업 모달 다이얼로그를 생성한다.
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
	panel.custom_minimum_size = Vector2(480, 0)
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.14, 0.22, 0.96)
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
		margin.add_theme_constant_override("margin_" + side, 26)
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
	close_btn.text = "확인 및 닫기"
	close_btn.custom_minimum_size.y = 42
	close_btn.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	var btn_style := StyleBoxFlat.new()
	btn_style.bg_color = Color("3ed8c9")
	btn_style.corner_radius_top_left = 8
	btn_style.corner_radius_top_right = 8
	btn_style.corner_radius_bottom_left = 8
	btn_style.corner_radius_bottom_right = 8
	close_btn.add_theme_stylebox_override("normal", btn_style)
	close_btn.add_theme_stylebox_override("hover", btn_style)
	close_btn.add_theme_color_override("font_color", Color("081d24"))
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
