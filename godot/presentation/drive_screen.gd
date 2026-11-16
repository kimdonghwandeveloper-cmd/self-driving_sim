extends Node3D
## 첨부 UI 시안 기반의 디지털 클러스터 HUD 및 자율주행 시뮬레이터 주행 화면.
## Rust 차량(SimVehicle) 또는 정지 미리보기에 3D 코스 메시, 탑다운 카메라, 차량 외관, HUD를 연동한다.

const Preview = preload("res://preview/vehicle_preview.gd")
const CourseMesh = preload("res://presentation/course_mesh.gd")
const Visual = preload("res://presentation/vehicle_visual.gd")
const Rig = preload("res://presentation/camera_rig.gd")

## GLB의 미터 원점을 공유하는 여수 고정 코스.
const YEOSU_COURSE := "map:testdata/yeosu-expo/course.toml"
## 사용자 제작 맵의 도로 면을 그대로 노면·연석으로 쓰는 여수 코스.
const YEOSU_CUSTOM_ROADS_COURSE := "map:testdata/yeosu-expo/custom-roads.toml"
## 도로 이탈 시 안내 문구 및 도로 격자 크기(m).
const OFF_ROAD_NOTICE := "도로 이탈 · 도로 면 밖입니다"
const ROAD_CELL_M := 20.0
const YEOSU_BACKGROUND = preload("res://assets/maps/yeosu-expo/background.glb")
const GROUND_MARGIN_M := 300.0

const INPUT_NAMES := {"keyboard": "키보드", "gamepad": "게임패드", "manual": "외부 입력", "none": "없음"}
const VIEW_NAMES := {"front": "전방", "rear": "후방", "left": "좌측", "right": "우측"}
const EXTERIOR_NOTICE := "탑다운 3인칭 시점 (C키로 실내 전환)"
const SIDE_VIEW_NOTICE := "현재 시선은 차량 전방과 다릅니다"
const IMPACT_SHOW_TICKS := 192
const IMPACT_TEXT := "충돌 발생 · 속도 변화 %.1f m/s · 충격량 %.1f kN·s"
const MOVING_THRESHOLD_MPS := 0.01
const FALLBACK_COURSE := "training"
const CALIBRATION_COURSE := "calibration"
const CALIBRATION_TITLE := "보정 측정 · 시뮬레이터 임시값이며 실차 성능이 아닙니다"
const FALLBACK_NOTICE := "지도 코스를 열지 못해 개발 코스(training)로 대체했습니다.\n%s"
const SCENARIO_PREFIX := "scenario:"
const SCENARIO_KEYS := {KEY_F5: "go", KEY_F6: "pause", KEY_F7: "restart", KEY_F8: "end"}
const SCENARIO_HELP := {
	"briefing": "F5 시작 · F8 종료",
	"running": "F6 일시정지 · F7 재시작 · F8 종료",
	"paused": "F5 계속 · F7 재시작 · F8 종료",
	"finished": "F7 다시 하기 · Esc 메뉴",
}
const SCENARIO_MESSAGE_COLOR := Color("ffd27a")
const HAZARD_COLOR := Color("ff7a1a")

var vehicle: Node3D
var rig: Node3D
var visual: Node3D
var camera_mount := Node3D.new()
var body_follow := 0.0

## 자동화 테스트(menu_flow_test.gd)와의 호환성을 위해 유지되는 백그라운드 HUD 텍스트 라벨.
var hud := Label.new()
var warnings_label := Label.new()
var notice := Label.new()
var calibration_box: PanelContainer
var calibration_label := Label.new()
var scenario_box: PanelContainer
var scenario_label := Label.new()
var scenario_message := Label.new()
var scenario_buttons := {}
var hazard_nodes := {}
var is_preview := false
var failure := ""
var fallback_notice := ""
var dismiss := Button.new()
var scene_content: Node3D
var command_notice := ""
var command_notice_until := 0
var ground := MeshInstance3D.new()
var road_cells := {}

# ==============================================================================
# 디지털 클러스터 HUD 노드 레퍼런스
# ==============================================================================
var _hud_overlay: Control
var _speed_val_label: Label
var _gear_labels := {}
var _turn_left_lbl: Label
var _turn_right_lbl: Label
var _ready_badge: PanelContainer
var _ready_badge_lbl: Label
var _emergency_badge: PanelContainer
var _impact_banner: PanelContainer
var _impact_banner_lbl: Label

# 좌측 내비게이션 위젯
var _nav_course_lbl: Label
var _nav_dist_lbl: Label
var _nav_cte_lbl: Label
var _nav_status_lbl: Label

# 우측 트립 및 상태 위젯
var _trip_dist_lbl: Label
var _trip_time_lbl: Label
var _trip_impacts_lbl: Label
var _trip_view_lbl: Label
var _trip_input_lbl: Label

var _current_course_name := ""
var _start_time_msec := 0

## 전달받은 코스로 화면을 초기화하고 차량 및 3D 코스 메시를 로드한다.
func start(course_name: String, force_preview: bool = false) -> void:
	_current_course_name = course_name
	_start_time_msec = Time.get_ticks_msec()
	_environment()
	is_preview = force_preview or not ClassDB.class_exists("SimVehicle")
	vehicle = _make_vehicle(course_name)
	if not is_preview and course_name.begins_with("map:") and not bool(vehicle.call("is_ready")):
		fallback_notice = FALLBACK_NOTICE % str(vehicle.call("last_error"))
		vehicle.free()
		vehicle = _make_vehicle(FALLBACK_COURSE)
	_ui()
	if not bool(vehicle.call("is_ready")):
		_fail(str(vehicle.call("last_error")))
		return
	var data: Dictionary = vehicle.call("course_data")
	if not data.get("ok", false):
		_fail(str(data.get("error", "코스 데이터 오류")))
		return
	scene_content = Node3D.new()
	add_child(scene_content)
	var course_mesh := CourseMesh.new()
	scene_content.add_child(course_mesh)
	course_mesh.build(data)
	var bounds := CourseMesh.ground_bounds(data)
	if course_name in [YEOSU_COURSE, YEOSU_CUSTOM_ROADS_COURSE] and not is_preview and fallback_notice.is_empty():
		bounds = _add_yeosu_background(bounds)
	if course_name == YEOSU_CUSTOM_ROADS_COURSE and fallback_notice.is_empty():
		_index_road_areas(data)
	_fit_ground(bounds)
	visual = Visual.new()
	vehicle.add_child(visual)
	camera_mount.name = "CameraMount"
	vehicle.add_child(camera_mount)
	rig = Rig.new()
	camera_mount.add_child(rig)
	var view_settings: Dictionary = vehicle.call("operator_view")
	body_follow = clampf(float(view_settings.get("body_follow", 0.0)), 0.0, 1.0)
	rig.configure(view_settings)
	if not is_preview:
		vehicle.connect("view_changed", rig.apply_view)
		vehicle.connect("debug_camera_toggled", rig.set_external)
		vehicle.connect("course_failed", _fail)
		vehicle.connect("command_rejected", _command_rejected)
		rig.apply_view(str(vehicle.call("view")))
		rig.set_external(bool(vehicle.call("debug_camera")))
	var warnings := Array(vehicle.call("config_warnings"))
	if not fallback_notice.is_empty():
		warnings.push_front(fallback_notice)
	warnings_label.text = "\n".join(warnings)
	dismiss.visible = not warnings_label.text.is_empty()
	notice.text = "\n".join(vehicle.call("course_notices"))
	calibration_box.visible = not is_preview and str(vehicle.get("course")) == CALIBRATION_COURSE
	scenario_box.visible = _has_scenario()
	if is_preview:
		notice.text += "\n정지 미리보기 · 1 전방 / 2 후방 / 3 좌측 / 4 우측 / C 탑다운"

## 지정된 코스 또는 시나리오의 차량 인스턴스를 생성한다.
func _make_vehicle(course_name: String) -> Node3D:
	var node: Node3D = Preview.new() if is_preview else ClassDB.instantiate("SimVehicle")
	if course_name.begins_with(SCENARIO_PREFIX):
		node.set("scenario", course_name.substr(SCENARIO_PREFIX.length()))
	else:
		node.set("course", course_name)
	add_child(node)
	return node

## 코스 면 다각형을 격자에 등록하여 도로 이탈 여부를 판정한다.
func _index_road_areas(data: Dictionary) -> void:
	road_cells.clear()
	for area in data.get("areas", []):
		var polygon := PackedVector2Array()
		for point: Vector3 in area.boundary:
			polygon.append(Vector2(point.x, point.z))
		var box := Rect2(polygon[0], Vector2.ZERO)
		for point in polygon:
			box = box.expand(point)
		for x in range(floori(box.position.x / ROAD_CELL_M), floori(box.end.x / ROAD_CELL_M) + 1):
			for z in range(floori(box.position.y / ROAD_CELL_M), floori(box.end.y / ROAD_CELL_M) + 1):
				var cell := Vector2i(x, z)
				if not road_cells.has(cell):
					road_cells[cell] = []
				road_cells[cell].append(polygon)

## 지면 위치가 도로 면 내부인지 검사한다.
func on_road(position_3d: Vector3) -> bool:
	if road_cells.is_empty():
		return true
	var point := Vector2(position_3d.x, position_3d.z)
	var cell := Vector2i(floori(point.x / ROAD_CELL_M), floori(point.y / ROAD_CELL_M))
	for polygon: PackedVector2Array in road_cells.get(cell, []):
		if Geometry2D.is_point_in_polygon(point, polygon):
			return true
	return false

## 여수 엑스포 3D 배경을 장면에 추가한다.
func _add_yeosu_background(bounds: Rect2) -> Rect2:
	var background := YEOSU_BACKGROUND.instantiate() as Node3D
	background.name = "YeosuBackground"
	scene_content.add_child(background)
	ground.position.y = -0.10
	for mesh_node: MeshInstance3D in background.find_children("*", "MeshInstance3D", true, false):
		var box: AABB = mesh_node.global_transform * mesh_node.get_aabb()
		bounds = bounds.merge(Rect2(Vector2(box.position.x, box.position.z), Vector2(box.size.x, box.size.z)))
	return bounds

## 환경광, 하늘, 그림자 조명 및 바닥을 구성한다.
func _environment() -> void:
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color("5d8fc4")
	sky_material.sky_horizon_color = Color("c3d5e2")
	sky_material.ground_bottom_color = Color("6b6f6a")
	sky_material.ground_horizon_color = Color("b9c3b5")
	sky_material.sun_angle_max = 20.0
	var sky := Sky.new()
	sky.sky_material = sky_material
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.45
	if RenderingServer.get_current_rendering_method() == "gl_compatibility":
		environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
		environment.ambient_light_color = Color("d8e1ea")
		environment.ambient_light_energy = 0.3
	environment.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	environment.fog_enabled = true
	environment.fog_light_color = Color("bccbd6")
	environment.fog_density = 0.0012
	environment.fog_sky_affect = 0.3
	var world := WorldEnvironment.new()
	world.environment = environment
	add_child(world)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-55, -35, 0)
	sun.light_energy = 0.85
	if RenderingServer.get_current_rendering_method() == "gl_compatibility":
		sun.light_energy = 0.6
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 120.0
	add_child(sun)
	var fill := DirectionalLight3D.new()
	fill.rotation_degrees = Vector3(-35, 140, 0)
	fill.light_energy = 0.12
	add_child(fill)
	ground.mesh = PlaneMesh.new()
	ground.material_override = CourseMesh.textured("grass", Color("6f7f5f"), Color("8e9a74"), 12.0)
	ground.position.y = -0.002
	_fit_ground(Rect2(Vector2(-250, -250), Vector2(500, 500)))
	add_child(ground)

## 바닥 크기와 위치를 코스 범위에 맞춘다.
func _fit_ground(bounds: Rect2) -> void:
	var size := (bounds.size + Vector2.ONE * GROUND_MARGIN_M * 2.0).max(Vector2(500, 500))
	(ground.mesh as PlaneMesh).size = size
	var center := bounds.get_center()
	ground.position = Vector3(center.x, ground.position.y, center.y)

## 디지털 클러스터 HUD 및 오버레이 컴포넌트를 구성한다.
func _ui() -> void:
	var canvas := CanvasLayer.new()
	canvas.layer = 5
	add_child(canvas)

	_hud_overlay = Control.new()
	_hud_overlay.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_hud_overlay.mouse_filter = Control.MOUSE_FILTER_IGNORE
	canvas.add_child(_hud_overlay)

	# 1. 중앙 상단 속도계 및 기어 인디케이터 클러스터
	_build_center_cluster()

	# 2. 좌측 내비게이션 및 차선 오차 위젯
	_build_left_nav_widget()

	# 3. 우측 트립 및 자율주행 모니터링 위젯
	_build_right_trip_widget()

	# 4. 하단 상태 바 (시간, 온도, 모드)
	_build_bottom_status_bar()

	# 5. 상단 시나리오 및 알림 배너
	_build_scenario_and_warnings()

	# 6. 테스트 호환용 백그라운드 HUD 라벨
	hud.visible = false
	_hud_overlay.add_child(hud)

## 중앙 속도계, 방향지시등, D/P/S/R/N 기어 인디케이터를 생성한다.
func _build_center_cluster() -> void:
	var center_margin := MarginContainer.new()
	center_margin.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
	center_margin.offset_top = 24
	center_margin.grow_horizontal = Control.GROW_DIRECTION_BOTH
	center_margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud_overlay.add_child(center_margin)

	var center_vbox := VBoxContainer.new()
	center_vbox.alignment = BoxContainer.ALIGNMENT_CENTER
	center_vbox.add_theme_constant_override("separation", 2)
	center_margin.add_child(center_vbox)

	# 대형 속도계
	var speed_hbox := HBoxContainer.new()
	speed_hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	speed_hbox.add_theme_constant_override("separation", 6)
	center_vbox.add_child(speed_hbox)

	_speed_val_label = Label.new()
	_speed_val_label.text = "0"
	_speed_val_label.add_theme_font_size_override("font_size", 54)
	_speed_val_label.add_theme_color_override("font_color", Color.WHITE)
	_speed_val_label.add_theme_color_override("font_shadow_color", Color.BLACK)
	_speed_val_label.add_theme_constant_override("shadow_offset_x", 2)
	_speed_val_label.add_theme_constant_override("shadow_offset_y", 2)
	speed_hbox.add_child(_speed_val_label)

	var unit_lbl := Label.new()
	unit_lbl.text = "KM/H"
	unit_lbl.add_theme_font_size_override("font_size", 14)
	unit_lbl.add_theme_color_override("font_color", Color("4ee1d4"))
	unit_lbl.size_flags_vertical = Control.SIZE_SHRINK_END
	unit_lbl.offset_bottom = -10
	speed_hbox.add_child(unit_lbl)

	# 방향 지시등
	var turn_hbox := HBoxContainer.new()
	turn_hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	turn_hbox.add_theme_constant_override("separation", 36)
	center_vbox.add_child(turn_hbox)

	_turn_left_lbl = Label.new()
	_turn_left_lbl.text = "◀"
	_turn_left_lbl.add_theme_font_size_override("font_size", 16)
	_turn_left_lbl.add_theme_color_override("font_color", Color(0.3, 0.4, 0.5, 0.5))
	turn_hbox.add_child(_turn_left_lbl)

	_turn_right_lbl = Label.new()
	_turn_right_lbl.text = "▶"
	_turn_right_lbl.add_theme_font_size_override("font_size", 16)
	_turn_right_lbl.add_theme_color_override("font_color", Color(0.3, 0.4, 0.5, 0.5))
	turn_hbox.add_child(_turn_right_lbl)

	# D P S R N 기어 인디케이터 바
	var gear_hbox := HBoxContainer.new()
	gear_hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	gear_hbox.add_theme_constant_override("separation", 18)
	center_vbox.add_child(gear_hbox)

	for g in ["D", "P", "S", "R", "N"]:
		var g_lbl := Label.new()
		g_lbl.text = g
		g_lbl.add_theme_font_size_override("font_size", 18)
		g_lbl.add_theme_color_override("font_color", Color("546e7a"))
		gear_hbox.add_child(g_lbl)
		_gear_labels[g] = g_lbl

	# READY 및 비상 상태 인디케이터
	var badge_hbox := HBoxContainer.new()
	badge_hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	badge_hbox.add_theme_constant_override("separation", 8)
	center_vbox.add_child(badge_hbox)

	_ready_badge = PanelContainer.new()
	var ready_style := StyleBoxFlat.new()
	ready_style.bg_color = Color(0.15, 0.45, 0.35, 0.8)
	ready_style.corner_radius_top_left = 6
	ready_style.corner_radius_top_right = 6
	ready_style.corner_radius_bottom_left = 6
	ready_style.corner_radius_bottom_right = 6
	_ready_badge.add_theme_stylebox_override("panel", ready_style)
	badge_hbox.add_child(_ready_badge)

	_ready_badge_lbl = Label.new()
	_ready_badge_lbl.text = " READY "
	_ready_badge_lbl.add_theme_font_size_override("font_size", 12)
	_ready_badge_lbl.add_theme_color_override("font_color", Color("4ee1d4"))
	_ready_badge.add_child(_ready_badge_lbl)

	_emergency_badge = PanelContainer.new()
	var emerg_style := StyleBoxFlat.new()
	emerg_style.bg_color = Color(0.7, 0.15, 0.15, 0.85)
	emerg_style.corner_radius_top_left = 6
	emerg_style.corner_radius_top_right = 6
	emerg_style.corner_radius_bottom_left = 6
	emerg_style.corner_radius_bottom_right = 6
	_emergency_badge.add_theme_stylebox_override("panel", emerg_style)
	badge_hbox.add_child(_emergency_badge)

	var emerg_lbl := Label.new()
	emerg_lbl.text = " EMERGENCY "
	emerg_lbl.add_theme_font_size_override("font_size", 12)
	emerg_lbl.add_theme_color_override("font_color", Color.WHITE)
	_emergency_badge.add_child(emerg_lbl)
	_emergency_badge.hide()

## 좌측 글래스모피즘 내비게이션 및 경로 정보 위젯을 생성한다.
func _build_left_nav_widget() -> void:
	var left_margin := MarginContainer.new()
	left_margin.set_anchors_and_offsets_preset(Control.PRESET_LEFT_WIDE)
	left_margin.offset_left = 28
	left_margin.offset_top = 80
	left_margin.offset_bottom = -90
	left_margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud_overlay.add_child(left_margin)

	var card := PanelContainer.new()
	card.custom_minimum_size = Vector2(250, 0)
	card.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.06, 0.10, 0.16, 0.75)
	style.corner_radius_top_left = 16
	style.corner_radius_top_right = 16
	style.corner_radius_bottom_left = 16
	style.corner_radius_bottom_right = 16
	style.border_width_left = 1
	style.border_width_top = 1
	style.border_width_right = 1
	style.border_width_bottom = 1
	style.border_color = Color(0.3, 0.5, 0.65, 0.4)
	card.add_theme_stylebox_override("panel", style)
	left_margin.add_child(card)

	var margin := MarginContainer.new()
	for s in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + s, 18)
	card.add_child(margin)

	var col := VBoxContainer.new()
	col.add_theme_constant_override("separation", 10)
	margin.add_child(col)

	var header := Label.new()
	header.text = "🧭 NAVIGATION"
	header.add_theme_font_size_override("font_size", 14)
	header.add_theme_color_override("font_color", Color("4ee1d4"))
	col.add_child(header)

	_nav_dist_lbl = Label.new()
	_nav_dist_lbl.text = "↱ 2.3 KM"
	_nav_dist_lbl.add_theme_font_size_override("font_size", 22)
	_nav_dist_lbl.add_theme_color_override("font_color", Color.WHITE)
	col.add_child(_nav_dist_lbl)

	var sub_dest := Label.new()
	sub_dest.text = "TO REACH DESTINATION"
	sub_dest.add_theme_font_size_override("font_size", 11)
	sub_dest.add_theme_color_override("font_color", Color("90a4ae"))
	col.add_child(sub_dest)

	var sep := HSeparator.new()
	col.add_child(sep)

	_nav_course_lbl = Label.new()
	_nav_course_lbl.text = "코스: " + _current_course_name
	_nav_course_lbl.add_theme_font_size_override("font_size", 13)
	_nav_course_lbl.add_theme_color_override("font_color", Color("cfd8dc"))
	_nav_course_lbl.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	col.add_child(_nav_course_lbl)

	_nav_cte_lbl = Label.new()
	_nav_cte_lbl.text = "경로 편차 (CTE): 0.00 m"
	_nav_cte_lbl.add_theme_font_size_override("font_size", 13)
	_nav_cte_lbl.add_theme_color_override("font_color", Color("81d4fa"))
	col.add_child(_nav_cte_lbl)

	_nav_status_lbl = Label.new()
	_nav_status_lbl.text = "차로 상태: 정상 주행 중"
	_nav_status_lbl.add_theme_font_size_override("font_size", 13)
	_nav_status_lbl.add_theme_color_override("font_color", Color("a5d6a7"))
	col.add_child(_nav_status_lbl)

## 우측 글래스모피즘 트립 및 자율주행 모니터링 위젯을 생성한다.
func _build_right_trip_widget() -> void:
	var right_margin := MarginContainer.new()
	right_margin.set_anchors_and_offsets_preset(Control.PRESET_RIGHT_WIDE)
	right_margin.offset_right = -28
	right_margin.offset_top = 80
	right_margin.offset_bottom = -90
	right_margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud_overlay.add_child(right_margin)

	var card := PanelContainer.new()
	card.custom_minimum_size = Vector2(250, 0)
	card.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.06, 0.10, 0.16, 0.75)
	style.corner_radius_top_left = 16
	style.corner_radius_top_right = 16
	style.corner_radius_bottom_left = 16
	style.corner_radius_bottom_right = 16
	style.border_width_left = 1
	style.border_width_top = 1
	style.border_width_right = 1
	style.border_width_bottom = 1
	style.border_color = Color(0.3, 0.5, 0.65, 0.4)
	card.add_theme_stylebox_override("panel", style)
	right_margin.add_child(card)

	var margin := MarginContainer.new()
	for s in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + s, 18)
	card.add_child(margin)

	var col := VBoxContainer.new()
	col.add_theme_constant_override("separation", 10)
	margin.add_child(col)

	var header := Label.new()
	header.text = "📊 DRIVING METRICS"
	header.add_theme_font_size_override("font_size", 14)
	header.add_theme_color_override("font_color", Color("b388ff"))
	col.add_child(header)

	var gauge_box := HBoxContainer.new()
	gauge_box.alignment = BoxContainer.ALIGNMENT_CENTER
	col.add_child(gauge_box)

	var gauge_lbl := Label.new()
	gauge_lbl.text = "READY  100%"
	gauge_lbl.add_theme_font_size_override("font_size", 18)
	gauge_lbl.add_theme_color_override("font_color", Color("c5cae9"))
	gauge_box.add_child(gauge_lbl)

	var sep := HSeparator.new()
	col.add_child(sep)

	_trip_dist_lbl = Label.new()
	_trip_dist_lbl.text = "주행 거리: 0.0 m"
	_trip_dist_lbl.add_theme_font_size_override("font_size", 13)
	_trip_dist_lbl.add_theme_color_override("font_color", Color("eceff1"))
	col.add_child(_trip_dist_lbl)

	_trip_time_lbl = Label.new()
	_trip_time_lbl.text = "경과 시간: 00:00"
	_trip_time_lbl.add_theme_font_size_override("font_size", 13)
	_trip_time_lbl.add_theme_color_override("font_color", Color("eceff1"))
	col.add_child(_trip_time_lbl)

	_trip_impacts_lbl = Label.new()
	_trip_impacts_lbl.text = "충돌 횟수: 0 회"
	_trip_impacts_lbl.add_theme_font_size_override("font_size", 13)
	_trip_impacts_lbl.add_theme_color_override("font_color", Color("eceff1"))
	col.add_child(_trip_impacts_lbl)

	_trip_view_lbl = Label.new()
	_trip_view_lbl.text = "시점: 탑다운 3인칭"
	_trip_view_lbl.add_theme_font_size_override("font_size", 13)
	_trip_view_lbl.add_theme_color_override("font_color", Color("b0bec5"))
	col.add_child(_trip_view_lbl)

	_trip_input_lbl = Label.new()
	_trip_input_lbl.text = "입력: 키보드 (W/A/S/D)"
	_trip_input_lbl.add_theme_font_size_override("font_size", 12)
	_trip_input_lbl.add_theme_color_override("font_color", Color("90a4ae"))
	col.add_child(_trip_input_lbl)

## 하단 시간, 온도, 교육 모드 상태 바를 생성한다.
func _build_bottom_status_bar() -> void:
	var bottom_margin := MarginContainer.new()
	bottom_margin.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM)
	bottom_margin.offset_bottom = -16
	bottom_margin.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud_overlay.add_child(bottom_margin)

	var pill := PanelContainer.new()
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.12, 0.18, 0.8)
	style.corner_radius_top_left = 12
	style.corner_radius_top_right = 12
	style.corner_radius_bottom_left = 12
	style.corner_radius_bottom_right = 12
	style.border_width_left = 1
	style.border_width_top = 1
	style.border_width_right = 1
	style.border_width_bottom = 1
	style.border_color = Color(0.25, 0.4, 0.5, 0.35)
	pill.add_theme_stylebox_override("panel", style)
	bottom_margin.add_child(pill)

	var m := MarginContainer.new()
	m.add_theme_constant_override("margin_left", 20)
	m.add_theme_constant_override("margin_top", 6)
	m.add_theme_constant_override("margin_right", 20)
	m.add_theme_constant_override("margin_bottom", 6)
	pill.add_child(m)

	var status_text := Label.new()
	status_text.text = "01:30 PM   •   🌙 18°C   •   SUNDAY   •   여수 엑스포 자율주행 교육 시뮬레이터"
	status_text.add_theme_font_size_override("font_size", 12)
	status_text.add_theme_color_override("font_color", Color("b0bec5"))
	m.add_child(status_text)

## 상단 시나리오 조작 상자 및 충돌 경고 플로팅 배너를 생성한다.
func _build_scenario_and_warnings() -> void:
	var top_panel := VBoxContainer.new()
	top_panel.set_anchors_and_offsets_preset(Control.PRESET_TOP_WIDE)
	top_panel.offset_top = 16
	top_panel.offset_left = 220
	top_panel.offset_right = -220
	top_panel.add_theme_constant_override("separation", 6)
	top_panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_hud_overlay.add_child(top_panel)

	# 시나리오 컨트롤 상자
	scenario_box = _backdrop()
	scenario_box.hide()
	top_panel.add_child(scenario_box)

	var scenario_column := VBoxContainer.new()
	scenario_column.add_theme_constant_override("separation", 6)
	scenario_box.add_child(scenario_column)
	scenario_column.add_child(scenario_label)
	scenario_column.add_child(scenario_message)

	var scenario_row := HBoxContainer.new()
	scenario_row.alignment = BoxContainer.ALIGNMENT_CENTER
	scenario_column.add_child(scenario_row)

	for entry in [["go", "시작·계속 (F5)"], ["pause", "일시정지 (F6)"], ["restart", "재시작 (F7)"], ["end", "종료 (F8)"]]:
		var button := Button.new()
		button.text = entry[1]
		button.focus_mode = Control.FOCUS_NONE
		button.pressed.connect(_scenario_action.bind(entry[0]))
		scenario_row.add_child(button)
		scenario_buttons[entry[0]] = button

	# 충돌 알림 플로팅 배너
	_impact_banner = PanelContainer.new()
	var imp_style := StyleBoxFlat.new()
	imp_style.bg_color = Color(0.8, 0.2, 0.1, 0.9)
	imp_style.corner_radius_top_left = 10
	imp_style.corner_radius_top_right = 10
	imp_style.corner_radius_bottom_left = 10
	imp_style.corner_radius_bottom_right = 10
	_impact_banner.add_theme_stylebox_override("panel", imp_style)
	_impact_banner.hide()
	top_panel.add_child(_impact_banner)

	var imp_m := MarginContainer.new()
	imp_m.add_theme_constant_override("margin_left", 16)
	imp_m.add_theme_constant_override("margin_top", 6)
	imp_m.add_theme_constant_override("margin_right", 16)
	imp_m.add_theme_constant_override("margin_bottom", 6)
	_impact_banner.add_child(imp_m)

	_impact_banner_lbl = Label.new()
	_impact_banner_lbl.text = "⚠️ 충돌 감지"
	_impact_banner_lbl.add_theme_font_size_override("font_size", 14)
	_impact_banner_lbl.add_theme_color_override("font_color", Color.WHITE)
	imp_m.add_child(_impact_banner_lbl)

	# 보정 및 설정 경고 상자
	calibration_box = _backdrop()
	calibration_box.hide()
	top_panel.add_child(calibration_box)
	calibration_box.add_child(calibration_label)

	top_panel.add_child(warnings_label)
	dismiss.text = "설정 경고 닫기"
	dismiss.pressed.connect(_dismiss_warnings)
	dismiss.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	top_panel.add_child(dismiss)

	# 우측 하단 알림 상자
	var notice_box := _backdrop()
	notice_box.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	notice_box.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	notice_box.grow_vertical = Control.GROW_DIRECTION_BEGIN
	notice_box.offset_right = -12
	notice_box.offset_bottom = -12
	_hud_overlay.add_child(notice_box)
	notice_box.add_child(notice)

	for label in [warnings_label, calibration_label, scenario_label, scenario_message]:
		label.mouse_filter = Control.MOUSE_FILTER_IGNORE
		label.add_theme_color_override("font_color", Color.WHITE)
		label.add_theme_color_override("font_shadow_color", Color.BLACK)
		label.add_theme_constant_override("shadow_offset_x", 1)
		label.add_theme_constant_override("shadow_offset_y", 1)

	warnings_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	scenario_label.add_theme_font_size_override("font_size", 16)
	scenario_message.add_theme_font_size_override("font_size", 18)
	scenario_message.add_theme_color_override("font_color", SCENARIO_MESSAGE_COLOR)

## 매 프레임 차량 상태를 읽어 디지털 클러스터 HUD 및 테스트 호환 라벨을 갱신한다.
func _process(_delta: float) -> void:
	if not failure.is_empty() or not is_instance_valid(vehicle):
		return
	if not bool(vehicle.call("is_ready")):
		_fail(str(vehicle.call("last_error")))
		return

	_apply_body_pose(vehicle.call("body_pose"))
	var emergency: bool = vehicle.call("emergency_active")
	var source := str(vehicle.call("input_source"))
	var speed_kmh: float = float(vehicle.call("speed_kmh"))
	var gear: String = str(vehicle.call("gear"))
	var engine_on: bool = bool(vehicle.call("engine_on"))
	var in_contact: bool = bool(vehicle.call("in_contact"))

	# 1. 중앙 속도계 갱신
	_speed_val_label.text = "%.0f" % absf(speed_kmh)

	# 2. 기어 인디케이터 강조 갱신
	for g in _gear_labels:
		var lbl: Label = _gear_labels[g]
		if g == gear:
			lbl.add_theme_color_override("font_color", Color("4ee1d4"))
			lbl.add_theme_font_size_override("font_size", 22)
		else:
			lbl.add_theme_color_override("font_color", Color("546e7a"))
			lbl.add_theme_font_size_override("font_size", 17)

	# 3. 비상 및 시동 뱃지 갱신
	_emergency_badge.visible = emergency
	if engine_on:
		_ready_badge_lbl.text = " READY "
		_ready_badge_lbl.add_theme_color_override("font_color", Color("4ee1d4"))
	else:
		_ready_badge_lbl.text = " OFF "
		_ready_badge_lbl.add_theme_color_override("font_color", Color("90a4ae"))

	# 4. 좌측 내비게이션 위젯 갱신
	if not road_cells.is_empty() and not on_road(vehicle.position):
		_nav_status_lbl.text = "차로 상태: ⚠️ " + OFF_ROAD_NOTICE
		_nav_status_lbl.add_theme_color_override("font_color", Color("ff8a7a"))
	else:
		_nav_status_lbl.text = "차로 상태: 정상 주행 중"
		_nav_status_lbl.add_theme_color_override("font_color", Color("a5d6a7"))

	# 5. 우측 트립 위젯 갱신
	var elapsed_sec := (Time.get_ticks_msec() - _start_time_msec) / 1000
	var mins := elapsed_sec / 60
	var secs := elapsed_sec % 60
	_trip_time_lbl.text = "경과 시간: %02d:%02d" % [mins, secs]
	_trip_view_lbl.text = "시점: %s (C키 전환)" % ("탑다운 3인칭" if rig.external else "1인칭 " + VIEW_NAMES.get(rig.current_view, rig.current_view))
	_trip_input_lbl.text = "입력: " + INPUT_NAMES.get(source, source)

	# 6. 충돌 배너 갱신
	var impact_line := _impact_line(vehicle.call("last_impact"), int(vehicle.call("tick_count")))
	if not impact_line.is_empty():
		_impact_banner.show()
		_impact_banner_lbl.text = "⚠️ " + impact_line
	else:
		_impact_banner.hide()

	# 7. 테스트 호환용 백그라운드 HUD 라벨 갱신
	hud.text = "%5.1f km/h    기어 %s    시동 %s    비상 정지 %s\n입력 %s    접촉 %s    시점 %s" % [
		speed_kmh, gear, "켜짐" if engine_on else "꺼짐",
		"작동" if emergency else "해제", INPUT_NAMES.get(source, source),
		"있음" if in_contact else "없음",
		"외부" if rig.external else VIEW_NAMES.get(rig.current_view, rig.current_view)]
	hud.text += "    이동 " + _movement(float(vehicle.call("speed_mps")))

	if calibration_box.visible:
		calibration_label.text = CALIBRATION_TITLE + "\n" + "\n".join(vehicle.call("calibration_lines"))
	if scenario_box.visible:
		_update_scenario(vehicle.call("scenario_status"))
		_update_hazards(vehicle.call("scenario_hazards"))

## 차체 움직임 자세를 적용한다.
func _apply_body_pose(pose: Dictionary) -> void:
	visual.set_body_pose(pose)
	camera_mount.transform = Transform3D.IDENTITY if rig.external else Visual.pose_transform(pose, body_follow)

## 마지막 충돌 정보를 포맷팅한다.
func _impact_line(impact: Dictionary, tick: int) -> String:
	if impact.is_empty() or tick - int(impact.get("tick", 0)) >= IMPACT_SHOW_TICKS:
		return ""
	return IMPACT_TEXT % [float(impact.get("delta_v_mps", 0.0)), float(impact.get("impulse_ns", 0.0)) / 1000.0]

## 시나리오 상태를 HUD에 반영한다.
func _update_scenario(status: Dictionary) -> void:
	var phase := str(status.get("phase", ""))
	var lines := PackedStringArray()
	lines.append("시나리오 · %s    [%s]" % [status.get("title", ""), status.get("phase_text", "")])
	if phase == "briefing" and not str(status.get("briefing", "")).is_empty():
		lines.append(str(status.briefing))
	var step_count := int(status.get("step_count", 0))
	var step_index := int(status.get("step_index", 0))
	if step_index < step_count:
		lines.append("단계 %d/%d · %s" % [step_index + 1, step_count, status.get("guide", "")])
	else:
		lines.append("단계 %d/%d 완료" % [step_count, step_count])
	var time_text := "경과 %.1f 초" % float(status.get("elapsed_s", 0.0))
	if float(status.get("time_limit_s", -1.0)) >= 0.0:
		time_text += " / 제한 %.0f 초" % float(status.time_limit_s)
	var collision_text := "충돌 %d회" % int(status.get("collisions", 0))
	if int(status.get("max_collisions", -1)) >= 0:
		collision_text += " / 허용 %d회" % int(status.max_collisions)
	lines.append(time_text + "    " + collision_text)
	if phase == "finished":
		lines.append_array(status.get("result_lines", PackedStringArray()))
	lines.append(SCENARIO_HELP.get(phase, ""))
	scenario_label.text = "\n".join(lines)
	scenario_message.text = str(status.get("message", ""))
	scenario_message.visible = not scenario_message.text.is_empty()
	scenario_buttons["go"].disabled = phase != "briefing" and phase != "paused"
	scenario_buttons["pause"].disabled = phase != "running"
	scenario_buttons["end"].disabled = phase == "finished"

## 보행자 등 시나리오 돌발 객체를 3D 장면에 렌더링한다.
func _update_hazards(hazards: Array) -> void:
	var seen := {}
	for hazard: Dictionary in hazards:
		var id := str(hazard.get("id", ""))
		seen[id] = true
		if not hazard_nodes.has(id):
			hazard_nodes[id] = _make_hazard(hazard.get("size", Vector3(0.6, 1.7, 0.6)))
			scene_content.add_child(hazard_nodes[id])
		var node: Node3D = hazard_nodes[id]
		node.position = hazard.get("position", Vector3.ZERO)
		node.rotation = Vector3(0.0, float(hazard.get("heading_rad", 0.0)), 0.0)
		node.visible = true
	for id in hazard_nodes:
		if not seen.has(id):
			hazard_nodes[id].visible = false

## 사람 형태의 대체 객체 3D 메시를 생성한다.
static func _make_hazard(size: Vector3) -> Node3D:
	var root := Node3D.new()
	root.name = "Hazard"
	var material := StandardMaterial3D.new()
	material.albedo_color = HAZARD_COLOR
	var head_d := minf(size.x, size.z) * 0.8
	var body := MeshInstance3D.new()
	var box := BoxMesh.new()
	box.size = Vector3(size.x, size.y - head_d, size.z)
	body.mesh = box
	body.material_override = material
	body.position.y = (size.y - head_d) / 2.0
	root.add_child(body)
	var head := MeshInstance3D.new()
	var sphere := SphereMesh.new()
	sphere.radius = head_d / 2.0
	sphere.height = head_d
	head.mesh = sphere
	head.material_override = material
	head.position.y = size.y - head_d / 2.0
	root.add_child(head)
	return root

## 시나리오 활성화 여부를 확인한다.
func _has_scenario() -> bool:
	if is_preview or not is_instance_valid(vehicle) or not bool(vehicle.call("is_ready")):
		return false
	return bool((vehicle.call("scenario_status") as Dictionary).get("active", false))

## 시나리오 시작/일시정지/재시작/종료 조작을 차량에 전달한다.
func _scenario_action(action: String) -> void:
	if not failure.is_empty() or not _has_scenario():
		return
	if action == "go":
		var phase := str((vehicle.call("scenario_status") as Dictionary).get("phase", ""))
		action = "start" if phase == "briefing" else "resume"
	vehicle.call("scenario_control", action)

## 속도에 따른 이동 방향 문구를 반환한다.
static func _movement(speed_mps: float) -> String:
	if absf(speed_mps) < MOVING_THRESHOLD_MPS:
		return "정지"
	return "전진" if speed_mps > 0.0 else "후진"

## 시나리오 및 시점 단축키 입력을 처리한다.
func _unhandled_key_input(event: InputEvent) -> void:
	if not failure.is_empty() or not event is InputEventKey or not event.pressed or event.echo:
		return
	if is_preview:
		match event.physical_keycode:
			KEY_1: rig.apply_view("front")
			KEY_2: rig.apply_view("rear")
			KEY_3: rig.apply_view("left")
			KEY_4: rig.apply_view("right")
			KEY_C: rig.set_external(not rig.external)
		return
	if not _has_scenario():
		return
	var action: String = SCENARIO_KEYS.get(event.physical_keycode, "")
	if not action.is_empty():
		_scenario_action(action)
		get_viewport().set_input_as_handled()

## 반투명 배경 패널 스타일을 생성한다.
func _backdrop() -> PanelContainer:
	var box := PanelContainer.new()
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.08, 0.12, 0.18, 0.78)
	style.corner_radius_top_left = 10
	style.corner_radius_top_right = 10
	style.corner_radius_bottom_left = 10
	style.corner_radius_bottom_right = 10
	style.border_width_left = 1
	style.border_width_top = 1
	style.border_width_right = 1
	style.border_width_bottom = 1
	style.border_color = Color(0.25, 0.4, 0.5, 0.35)
	box.add_theme_stylebox_override("panel", style)
	var margin := MarginContainer.new()
	for s in ["left", "top", "right", "bottom"]:
		margin.add_theme_constant_override("margin_" + s, 10)
	box.add_child(margin)
	return box

## 설정 경고 라벨을 숨긴다.
func _dismiss_warnings() -> void:
	warnings_label.hide()
	dismiss.hide()

## 거부된 명령 안내를 일정 시간 동안 표시한다.
func _command_rejected(reason: String) -> void:
	command_notice = "명령 거부 · %s" % reason
	command_notice_until = Time.get_ticks_msec() + 3000

## 실패 메시지를 설정하고 화면을 에러 모드로 전환한다.
func _fail(reason: String) -> void:
	failure = reason
	warnings_label.text = "화면 생성 실패: %s" % reason
	dismiss.hide()
