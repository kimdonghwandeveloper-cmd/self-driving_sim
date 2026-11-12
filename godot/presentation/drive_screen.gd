extends Node3D
## Rust 차량 또는 정지 미리보기에 코스·실내·카메라·HUD를 연결한다.

const Preview = preload("res://preview/vehicle_preview.gd")
const CourseMesh = preload("res://presentation/course_mesh.gd")
const Visual = preload("res://presentation/vehicle_visual.gd")
const Rig = preload("res://presentation/camera_rig.gd")
## GLB의 미터 원점을 공유하는 여수 고정 코스. 저장 경로는 시작점이 달라 이 배경을 쓰지 않는다.
const YEOSU_COURSE := "map:testdata/yeosu-expo/course.toml"
const YEOSU_BACKGROUND = preload("res://assets/maps/yeosu-expo/background.glb")
## 코스 범위 밖으로 바닥을 더 까는 여백(m).
const GROUND_MARGIN_M := 300.0
## HUD에 보일 입력 장치·시점 이름(계약 값 → 한국어).
const INPUT_NAMES := {"keyboard": "키보드", "gamepad": "게임패드", "manual": "외부 입력", "none": "없음"}
const VIEW_NAMES := {"front": "전방", "rear": "후방", "left": "좌측", "right": "우측"}
## 외부(디버그) 시점에서 보이는 차체가 임시 모델임을 알리는 문구.
const EXTERIOR_NOTICE := "외부 시점 · 차체 외관은 임시 모델이며 충돌 판정 차체와 크기가 다릅니다"
## 1인칭 전방이 아닌 시점일 때 시선과 진행 방향이 다름을 알리는 문구.
const SIDE_VIEW_NOTICE := "현재 시선은 차량 전방과 다릅니다"
## 충돌 뒤 HUD에 충격 크기를 보이는 시간(물리 틱 수, 64 Hz에서 3초, 표시용 임시값, 2-7).
const IMPACT_SHOW_TICKS := 192
## 충격 크기 줄. 크기는 임시값으로 계산한 값이라 실제 충돌 거동이 아님을 함께 알린다.
const IMPACT_TEXT := "충돌 · 속도 변화 %.1f m/s · 충격량 %.1f kN·s (임시값, 실제 충돌 거동 아님)"
## HUD 이동 방향을 "정지"로 보일 속도 크기 상한(m/s, 표시용 임시값).
const MOVING_THRESHOLD_MPS := 0.01
## 지도 코스(`map:`)를 열지 못했을 때 대신 여는 개발 코스와 안내 문구(Bevy `--course map:` 대체와 같음, 4-6).
const FALLBACK_COURSE := "training"
## 보정 측정(2-5)을 보여 주는 코스와 그 상자 제목.
const CALIBRATION_COURSE := "calibration"
const CALIBRATION_TITLE := "보정 측정 · 시뮬레이터 임시값이며 실차 성능이 아닙니다"
const FALLBACK_NOTICE := "지도 코스를 열지 못해 개발 코스(training)로 대체했습니다.\n%s"
## 시나리오 선택 문자열 머리(`scenario:<저장소 루트 기준 파일>`, 5-1).
const SCENARIO_PREFIX := "scenario:"
## 시나리오 조작 키(주행 입력 설정 `config/input.toml`과 겹치지 않는 기능 키)와 화면 동작 이름.
const SCENARIO_KEYS := {KEY_F5: "go", KEY_F6: "pause", KEY_F7: "restart", KEY_F8: "end"}
## 단계별 조작 안내.
const SCENARIO_HELP := {
	"briefing": "F5 시작 · F8 종료",
	"running": "F6 일시정지 · F7 재시작 · F8 종료",
	"paused": "F5 계속 · F7 재시작 · F8 종료",
	"finished": "F7 다시 하기 · Esc 메뉴",
}
## 시나리오 메시지 글자색.
const SCENARIO_MESSAGE_COLOR := Color("ffd27a")
var vehicle: Node3D
var rig: Node3D
## 차체 외관·실내(서스펜션 움직임을 받는다, 2-8).
var visual: Node3D
## 입석 카메라를 얹는 받침. 차체 기울기를 `body_follow`만큼 따라간다(외부 시점에서는 따라가지 않음).
var camera_mount := Node3D.new()
## 카메라가 차체 기울기를 따라가는 정도(계약 v1.9 `operator_view().body_follow`, 0 = 끔).
var body_follow := 0.0
var hud := Label.new()
var warnings_label := Label.new()
var notice := Label.new()
## 보정 코스에서만 보이는 측정 상자와 글자.
var calibration_box: PanelContainer
var calibration_label := Label.new()
## 시나리오 주행일 때만 보이는 안내 상자·글자·메시지와 조작 버튼(동작 이름 → 버튼).
var scenario_box: PanelContainer
var scenario_label := Label.new()
var scenario_message := Label.new()
var scenario_buttons := {}
var is_preview := false
var failure := ""
## 지도 코스 대신 개발 코스를 열었을 때의 안내. 대체하지 않았으면 빈 문자열.
var fallback_notice := ""
var dismiss := Button.new()
var scene_content: Node3D
var command_notice := ""
var command_notice_until := 0
## 코스 둘레 바닥. 코스 범위를 안 뒤 크기를 맞춘다.
var ground := MeshInstance3D.new()

## 전달받은 코스로 화면을 만들고 실패 시 메시를 생성하지 않는다.
func start(course_name: String, force_preview: bool = false) -> void:
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
	if course_name == YEOSU_COURSE and not is_preview and fallback_notice.is_empty():
		bounds = _add_yeosu_background(bounds)
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
		notice.text += "\n정지 미리보기 · 1 전방 / 2 후방 / 3 좌측 / 4 우측 / C 외부"

## `course_name` 코스의 Rust 차량(또는 정지 미리보기) 노드를 만들어 장면에 넣는다. 준비 결과는 `is_ready`로 본다.
## `scenario:<파일>`이면 차량의 `scenario` 속성에 파일을 넣고 코스는 시나리오 파일이 정한다.
func _make_vehicle(course_name: String) -> Node3D:
	var node: Node3D = Preview.new() if is_preview else ClassDB.instantiate("SimVehicle")
	if course_name.begins_with(SCENARIO_PREFIX):
		node.set("scenario", course_name.substr(SCENARIO_PREFIX.length()))
	else:
		node.set("course", course_name)
	add_child(node)
	return node

## 시각 배경을 원점에 배치하고, 바닥이 배경 도로를 덮지 않도록 높이와 범위를 맞춘다.
func _add_yeosu_background(bounds: Rect2) -> Rect2:
	var background := YEOSU_BACKGROUND.instantiate() as Node3D
	background.name = "YeosuBackground"
	scene_content.add_child(background)
	ground.position.y = -0.10
	for mesh_node: MeshInstance3D in background.find_children("*", "MeshInstance3D", true, false):
		var box: AABB = mesh_node.global_transform * mesh_node.get_aabb()
		bounds = bounds.merge(Rect2(Vector2(box.position.x, box.position.z), Vector2(box.size.x, box.size.z)))
	return bounds

## 절차 생성 하늘·태양광(그림자)·보조광·옅은 안개와 바닥을 만든다.
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
		# Compatibility 렌더러는 하늘 주변광·태양광이 과하게 밝게 나와 이전 단색 주변광 값과 약한 태양광을 쓴다.
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

## 바닥이 코스 범위와 여백을 덮도록 크기·위치를 맞춘다(최소 500 m 사방).
func _fit_ground(bounds: Rect2) -> void:
	var size := (bounds.size + Vector2.ONE * GROUND_MARGIN_M * 2.0).max(Vector2(500, 500))
	(ground.mesh as PlaneMesh).size = size
	var center := bounds.get_center()
	ground.position = Vector3(center.x, ground.position.y, center.y)

## HUD와 닫을 수 있는 설정 경고 표시 영역을 배치한다.
func _ui() -> void:
	var canvas := CanvasLayer.new()
	add_child(canvas)
	var overlay := Control.new()
	overlay.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	overlay.mouse_filter = Control.MOUSE_FILTER_IGNORE
	canvas.add_child(overlay)
	var panel := VBoxContainer.new()
	panel.set_anchors_and_offsets_preset(Control.PRESET_TOP_LEFT)
	panel.offset_left = 20
	panel.offset_top = 64
	panel.add_theme_constant_override("separation", 8)
	overlay.add_child(panel)
	var hud_box := _backdrop()
	panel.add_child(hud_box)
	hud_box.add_child(hud)
	scenario_box = _backdrop()
	scenario_box.hide()
	panel.add_child(scenario_box)
	var scenario_column := VBoxContainer.new()
	scenario_column.add_theme_constant_override("separation", 6)
	scenario_box.add_child(scenario_column)
	scenario_column.add_child(scenario_label)
	scenario_column.add_child(scenario_message)
	var scenario_row := HBoxContainer.new()
	scenario_column.add_child(scenario_row)
	for entry in [["go", "시작·계속 (F5)"], ["pause", "일시정지 (F6)"], ["restart", "재시작 (F7)"], ["end", "종료 (F8)"]]:
		var button := Button.new()
		button.text = entry[1]
		# 버튼이 포커스를 가지면 Enter·Space(시동·비상 정지 키)가 버튼을 누르게 되므로 포커스를 받지 않는다.
		button.focus_mode = Control.FOCUS_NONE
		button.pressed.connect(_scenario_action.bind(entry[0]))
		scenario_row.add_child(button)
		scenario_buttons[entry[0]] = button
	calibration_box = _backdrop()
	calibration_box.hide()
	panel.add_child(calibration_box)
	calibration_box.add_child(calibration_label)
	panel.add_child(warnings_label)
	var notice_box := _backdrop()
	notice_box.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	notice_box.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	notice_box.grow_vertical = Control.GROW_DIRECTION_BEGIN
	notice_box.offset_right = -12
	notice_box.offset_bottom = -12
	overlay.add_child(notice_box)
	notice_box.add_child(notice)
	for label in [hud, notice, warnings_label, calibration_label, scenario_label, scenario_message]:
		label.mouse_filter = Control.MOUSE_FILTER_IGNORE
		label.add_theme_color_override("font_color", Color.WHITE)
		label.add_theme_color_override("font_shadow_color", Color.BLACK)
		label.add_theme_constant_override("shadow_offset_x", 1)
		label.add_theme_constant_override("shadow_offset_y", 1)
	warnings_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	warnings_label.custom_minimum_size.x = 640
	hud.add_theme_font_size_override("font_size", 22)
	notice.add_theme_font_size_override("font_size", 14)
	calibration_label.add_theme_font_size_override("font_size", 16)
	scenario_label.add_theme_font_size_override("font_size", 18)
	scenario_message.add_theme_font_size_override("font_size", 20)
	scenario_message.add_theme_color_override("font_color", SCENARIO_MESSAGE_COLOR)
	for label in [scenario_label, scenario_message]:
		label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		label.custom_minimum_size.x = 560
	notice.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	dismiss.text = "설정 경고 닫기"
	dismiss.pressed.connect(_dismiss_warnings)
	dismiss.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	panel.add_child(dismiss)

## 읽기 함수만 호출해 주행 상태를 표시한다.
func _process(_delta: float) -> void:
	if not failure.is_empty() or not is_instance_valid(vehicle):
		return
	if not bool(vehicle.call("is_ready")):
		_fail(str(vehicle.call("last_error")))
		return
	_apply_body_pose(vehicle.call("body_pose"))
	var emergency: bool = vehicle.call("emergency_active")
	var source := str(vehicle.call("input_source"))
	hud.text = "%5.1f km/h    기어 %s    시동 %s    비상 정지 %s\n입력 %s    접촉 %s    시점 %s" % [
		vehicle.call("speed_kmh"), vehicle.call("gear"), "켜짐" if vehicle.call("engine_on") else "꺼짐",
		"작동" if emergency else "해제", INPUT_NAMES.get(source, source),
		"있음" if vehicle.call("in_contact") else "없음",
		"외부" if rig.external else VIEW_NAMES.get(rig.current_view, rig.current_view)]
	hud.text += "    이동 " + _movement(float(vehicle.call("speed_mps")))
	hud.add_theme_color_override("font_color", Color("ff8a7a") if emergency else Color.WHITE)
	if rig.external:
		hud.text += "\n" + EXTERIOR_NOTICE
	elif rig.current_view != "front":
		hud.text += "\n" + SIDE_VIEW_NOTICE

	var impact_line := _impact_line(vehicle.call("last_impact"), int(vehicle.call("tick_count")))
	if not impact_line.is_empty():
		hud.text += "\n" + impact_line
	if Time.get_ticks_msec() < command_notice_until:
		hud.text += "\n" + command_notice
	if calibration_box.visible:
		calibration_label.text = CALIBRATION_TITLE + "\n" + "\n".join(vehicle.call("calibration_lines"))
	if scenario_box.visible:
		_update_scenario(vehicle.call("scenario_status"))

## 차체 움직임(`SimVehicle.body_pose`, 계약 v1.9)으로 차체를 기울이고, 입석 카메라는 `body_follow`만큼 따라가게 한다.
func _apply_body_pose(pose: Dictionary) -> void:
	visual.set_body_pose(pose)
	camera_mount.transform = Transform3D.IDENTITY if rig.external else Visual.pose_transform(pose, body_follow)

## 마지막 충돌 사전(`SimVehicle.last_impact`, 계약 v1.8)으로 충격 크기 줄을 만든다.
## 충돌이 없거나 [constant IMPACT_SHOW_TICKS]보다 오래됐으면 빈 문자열.
func _impact_line(impact: Dictionary, tick: int) -> String:
	if impact.is_empty() or tick - int(impact.get("tick", 0)) >= IMPACT_SHOW_TICKS:
		return ""
	return IMPACT_TEXT % [float(impact.get("delta_v_mps", 0.0)), float(impact.get("impulse_ns", 0.0)) / 1000.0]

## 시나리오 상태 사전(`SimVehicle.scenario_status`)으로 안내 상자 글자·메시지·버튼 사용 가능 여부를 갱신한다.
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
	if float(status.get("max_impact_delta_v_mps", 0.0)) > 0.0:
		collision_text += " · 최대 속도 변화 %.1f m/s" % float(status.max_impact_delta_v_mps)
	lines.append(time_text + "    " + collision_text)
	lines.append(SCENARIO_HELP.get(phase, ""))
	scenario_label.text = "\n".join(lines)
	scenario_message.text = str(status.get("message", ""))
	scenario_message.visible = not scenario_message.text.is_empty()
	scenario_buttons["go"].disabled = phase != "briefing" and phase != "paused"
	scenario_buttons["pause"].disabled = phase != "running"
	scenario_buttons["end"].disabled = phase == "finished"

## 시나리오 차량이 준비됐는지 반환한다.
func _has_scenario() -> bool:
	if is_preview or not is_instance_valid(vehicle) or not bool(vehicle.call("is_ready")):
		return false
	return bool((vehicle.call("scenario_status") as Dictionary).get("active", false))

## 화면 동작 이름(`go`, `pause`, `restart`, `end`)을 시나리오 조작으로 보낸다. `go`는 시작 전이면 시작, 일시정지면 계속이다.
## 지금 단계에서 받을 수 없는 조작이면 Rust가 거부하고 아무것도 바뀌지 않는다.
func _scenario_action(action: String) -> void:
	if not failure.is_empty() or not _has_scenario():
		return
	if action == "go":
		var phase := str((vehicle.call("scenario_status") as Dictionary).get("phase", ""))
		action = "start" if phase == "briefing" else "resume"
	vehicle.call("scenario_control", action)

## 부호 있는 속도(m/s)를 이동 방향 이름으로 바꾼다. 정지 판정 문턱은 표시용이다.
static func _movement(speed_mps: float) -> String:
	if absf(speed_mps) < MOVING_THRESHOLD_MPS:
		return "정지"
	return "전진" if speed_mps > 0.0 else "후진"

## 시나리오 조작 키(F5~F8)와, 임시 노드에서만 시야 미리보기 키를 받는다. 주행 입력은 만들지 않는다.
func _unhandled_key_input(event: InputEvent) -> void:
	if not failure.is_empty() or not event is InputEventKey or not event.pressed or event.echo:
		return
	if event.physical_keycode in SCENARIO_KEYS and _has_scenario():
		_scenario_action(SCENARIO_KEYS[event.physical_keycode])
		get_viewport().set_input_as_handled()
		return
	if not is_preview:
		return
	var directions := {KEY_1: "front", KEY_2: "rear", KEY_3: "left", KEY_4: "right"}
	if event.physical_keycode in directions:
		rig.apply_view(directions[event.physical_keycode])
	elif event.physical_keycode == KEY_C:
		rig.set_external(not rig.external)

## 코스 실패 후 주행을 멈추고 오류만 표시한다.
func _fail(message: String) -> void:
	if message.is_empty():
		message = "차량을 준비하지 못했습니다(사유 없음)."
	failure = message
	if is_instance_valid(vehicle):
		vehicle.set_physics_process(false)
		vehicle.hide()
	if is_instance_valid(scene_content):
		scene_content.hide()
	hud.text = "코스를 열 수 없습니다\n" + message
	notice.text = "상단 메뉴로 돌아가세요."
	warnings_label.hide()
	dismiss.hide()
	calibration_box.hide()
	scenario_box.hide()
## 반투명 어두운 배경 상자를 만든다(HUD·안내 글자가 하늘·노면 위에서도 읽히게).
func _backdrop() -> PanelContainer:
	var box := PanelContainer.new()
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0, 0, 0, 0.45)
	style.set_corner_radius_all(6)
	style.set_content_margin_all(10)
	box.add_theme_stylebox_override("panel", style)
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return box

## 경고 문구와 닫기 버튼을 함께 숨긴다.
func _dismiss_warnings() -> void:
	warnings_label.hide()
	dismiss.hide()
## 명령 거부 사유를 잠시 표시하며 상태 처리는 Rust에 맡긴다.
func _command_rejected(command: String, _source: String, reason: String) -> void:
	command_notice = "%s: %s" % [command, reason]
	command_notice_until = Time.get_ticks_msec() + 4000
