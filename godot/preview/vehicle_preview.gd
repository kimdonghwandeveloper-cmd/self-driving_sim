extends Node3D
## Rust 차량이 없을 때만 쓰는 정지 표시 노드. 입력·운동·충돌·명령은 구현하지 않는다.

const Fixture = preload("res://preview/course_fixture.gd")
var course := "training"

## 화면이 읽는 준비 상태를 반환한다. 실제 주행 준비를 뜻하지 않는다.
func is_ready() -> bool:
	return true

## 임시 노드에는 초기화 오류가 없다.
func last_error() -> String:
	return ""

## 설정은 Rust 구현이 연결될 때 검증됨을 알린다.
func config_warnings() -> PackedStringArray:
	return PackedStringArray(["Rust SimVehicle 미연결: 정지 미리보기만 표시합니다."])

## 화면에 사용한 시험 코스만 반환한다.
func course_data() -> Dictionary:
	return Fixture.build(course)

## 표시용 속도는 정지값이다.
func speed_kmh() -> float:
	return 0.0

## 표시용 부호 있는 속도(m/s)도 정지값이다.
func speed_mps() -> float:
	return 0.0

## 표시용 기어는 주차다.
func gear() -> String:
	return "P"

## 임시 노드는 시동을 켜지 않는다.
func engine_on() -> bool:
	return false

## 임시 노드는 비상 정지 상태를 계산하지 않는다.
func emergency_active() -> bool:
	return false

## 임시 입력 장치는 없다.
func input_source() -> String:
	return "none"

## 임시 노드는 접촉 판정을 하지 않는다.
func in_contact() -> bool:
	return false

## 임시 노드는 충돌하지 않으므로 마지막 충돌이 없다(계약 v1.8).
func last_impact() -> Dictionary:
	return {}

## 임시 노드는 물리 틱을 돌리지 않는다.
func tick_count() -> int:
	return 0

## 시점 초기값은 전방이다.
func view() -> String:
	return "front"

## 디버그 카메라 초기값은 꺼짐이다.
func debug_camera() -> bool:
	return false

## 미리보기 안내를 계약과 같은 이름으로 제공한다.
func course_notices() -> PackedStringArray:
	return Fixture.build(course).notices

## 미리보기 카메라에 필요한 TOML 숫자만 읽는다. 정식 설정 처리는 Rust가 맡는다.
func operator_view() -> Dictionary:
	var result := {"eye_m": Vector3(0, 1.65, -2.75), "pitch_deg": -3.0, "vertical_fov_deg": 62.0, "near_clip_m": 0.05}
	var path := ProjectSettings.globalize_path("res://../config/camera.toml")
	if not FileAccess.file_exists(path):
		return result
	var section := ""
	for source in FileAccess.get_file_as_string(path).split("\n"):
		var line: String = source.split("#")[0].strip_edges()
		if line.begins_with("["):
			section = line
		if section != "[operator_view]" or not line.contains("="):
			continue
		var pair := line.split("=", true, 1)
		var key := pair[0].strip_edges()
		var value: Variant = JSON.parse_string(pair[1].strip_edges())
		if key == "eye_m" and value is Array and value.size() == 3:
			result[key] = Vector3(float(value[0]), float(value[1]), float(value[2]))
		elif key in ["pitch_deg", "vertical_fov_deg", "near_clip_m"] and (value is float or value is int):
			result[key] = float(value)
	return result