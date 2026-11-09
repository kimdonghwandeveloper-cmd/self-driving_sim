extends Node3D
## 같은 눈 위치에서 네 방향을 보는 입석 카메라. 주행 상태는 수정하지 않는다.

var camera: Camera3D
var pitch_deg := -3.0
var current_view := "front"
var external := false
var eye := Vector3(0, 1.65, -2.75)

## 계약의 카메라 설정으로 리그를 초기화한다.
func configure(settings: Dictionary) -> void:
	eye = settings.get("eye_m", eye)
	pitch_deg = float(settings.get("pitch_deg", -3.0))
	camera = Camera3D.new()
	camera.keep_aspect = Camera3D.KEEP_HEIGHT
	camera.fov = clampf(float(settings.get("vertical_fov_deg", 62.0)), 30.0, 110.0)
	camera.near = clampf(float(settings.get("near_clip_m", 0.05)), 0.01, 0.5)
	camera.far = 1500.0
	add_child(camera)
	camera.current = true
	apply_view("front")

## 입력 또는 Rust 시그널로 전달된 방향만 표시한다.
func apply_view(direction: String) -> void:
	if direction not in ["front", "rear", "left", "right"]:
		return
	current_view = direction
	_update_pose()

## 외부 차체를 볼 수 있는 디버그 카메라로 전환한다.
func set_external(enabled: bool) -> void:
	external = enabled
	_update_pose()

## 외부 보기와 실내 보기의 자세·레이어를 적용한다.
func _update_pose() -> void:
	if not is_instance_valid(camera):
		return
	if external:
		position = Vector3.ZERO
		rotation = Vector3.ZERO
		camera.position = Vector3(8, 6, 8)
		camera.look_at(to_global(Vector3(0, 1, -1.4)), Vector3.UP)
		camera.cull_mask = 3
	else:
		position = eye
		rotation = Vector3(0, {"front": 0.0, "rear": PI, "left": PI / 2, "right": -PI / 2}[current_view], 0)
		camera.position = Vector3.ZERO
		camera.rotation = Vector3(deg_to_rad(pitch_deg), 0, 0)
		camera.cull_mask = 1