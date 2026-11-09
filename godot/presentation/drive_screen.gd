extends Node3D
## Rust 차량 또는 정지 미리보기에 코스·실내·카메라·HUD를 연결한다.

const Preview = preload("res://preview/vehicle_preview.gd")
const CourseMesh = preload("res://presentation/course_mesh.gd")
const Visual = preload("res://presentation/vehicle_visual.gd")
const Rig = preload("res://presentation/camera_rig.gd")
var vehicle: Node3D
var rig: Node3D
var hud := Label.new()
var warnings_label := Label.new()
var notice := Label.new()
var is_preview := false
var failure := ""
var dismiss := Button.new()
var scene_content: Node3D
var command_notice := ""
var command_notice_until := 0

## 전달받은 코스로 화면을 만들고 실패 시 메시를 생성하지 않는다.
func start(course_name: String, force_preview: bool = false) -> void:
	_environment()
	is_preview = force_preview or not ClassDB.class_exists("SimVehicle")
	vehicle = Preview.new() if is_preview else ClassDB.instantiate("SimVehicle")
	vehicle.set("course", course_name)
	add_child(vehicle)
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
	vehicle.add_child(Visual.new())
	rig = Rig.new()
	vehicle.add_child(rig)
	rig.configure(vehicle.call("operator_view"))
	if not is_preview:
		vehicle.connect("view_changed", rig.apply_view)
		vehicle.connect("debug_camera_toggled", rig.set_external)
		vehicle.connect("course_failed", _fail)
		vehicle.connect("command_rejected", _command_rejected)
		rig.apply_view(str(vehicle.call("view")))
		rig.set_external(bool(vehicle.call("debug_camera")))
	warnings_label.text = "\n".join(vehicle.call("config_warnings"))
	dismiss.visible = not warnings_label.text.is_empty()
	notice.text = "\n".join(vehicle.call("course_notices"))
	if is_preview:
		notice.text += "\n정지 미리보기 · 1 전방 / 2 후방 / 3 좌측 / 4 우측 / C 외부"

## 하늘·주광·보조광과 넓은 바닥을 만든다.
func _environment() -> void:
	var environment := Environment.new()
	environment.background_mode = Environment.BG_COLOR
	environment.background_color = Color("a7c3d5")
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.ambient_light_color = Color("d8e1ea")
	environment.ambient_light_energy = 0.3
	var world := WorldEnvironment.new()
	world.environment = environment
	add_child(world)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-55, -35, 0)
	sun.light_energy = 0.65
	sun.shadow_enabled = true
	add_child(sun)
	var fill := DirectionalLight3D.new()
	fill.rotation_degrees = Vector3(-35, 140, 0)
	fill.light_energy = 0.15
	add_child(fill)
	var ground := MeshInstance3D.new()
	var plane := PlaneMesh.new()
	plane.size = Vector2(500, 500)
	ground.mesh = plane
	ground.material_override = CourseMesh.material(Color("8c9a7d"))
	add_child(ground)

## HUDと閉じられる設定警告の表示領域を配置する。
func _ui() -> void:
	var canvas := CanvasLayer.new()
	add_child(canvas)
	var panel := VBoxContainer.new()
	var overlay := Control.new()
	overlay.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	overlay.mouse_filter = Control.MOUSE_FILTER_IGNORE
	canvas.add_child(overlay)
	panel.set_anchors_and_offsets_preset(Control.PRESET_TOP_WIDE)
	panel.offset_left = 24
	panel.offset_right = -24
	panel.offset_top = 80
	overlay.add_child(panel)
	for label in [hud, notice, warnings_label]:
		label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		label.mouse_filter = Control.MOUSE_FILTER_IGNORE
		label.add_theme_color_override("font_color", Color.WHITE)
		label.add_theme_color_override("font_shadow_color", Color.BLACK)
		label.add_theme_constant_override("shadow_offset_x", 1)
		label.add_theme_constant_override("shadow_offset_y", 1)
		panel.add_child(label)
	hud.add_theme_font_size_override("font_size", 24)
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
	hud.text = "%05.1f km/h    %s    시동 %s    비상 %s\n입력 %s    접촉 %s    시점 %s" % [
		vehicle.call("speed_kmh"), vehicle.call("gear"), "ON" if vehicle.call("engine_on") else "OFF",
		"ON" if vehicle.call("emergency_active") else "OFF", vehicle.call("input_source"),
		"ON" if vehicle.call("in_contact") else "OFF", "외부" if rig.external else rig.current_view]

	if Time.get_ticks_msec() < command_notice_until:
		hud.text += "\n" + command_notice

## 임시 노드에서만 시야 미리보기 키를 받으며 주행 입력은 만들지 않는다.
func _unhandled_key_input(event: InputEvent) -> void:
	if not is_preview or not failure.is_empty() or not event is InputEventKey or not event.pressed or event.echo:
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
## 경고 문구와 닫기 버튼을 함께 숨긴다.
func _dismiss_warnings() -> void:
	warnings_label.hide()
	dismiss.hide()
## 명령 거부 사유를 잠시 표시하며 상태 처리는 Rust에 맡긴다.
func _command_rejected(command: String, _source: String, reason: String) -> void:
	command_notice = "%s: %s" % [command, reason]
	command_notice_until = Time.get_ticks_msec() + 4000
