# GD-2 헤드리스 재생 비교(계약 docs/design/godot-interface.md 5-3).
# SimVehicle을 read_input = false로 띄워 정해진 입력 열을 실제 물리 틱으로 재생하고,
# 같은 열의 SimReplay.run(Rust 단독 재생) 표본과 비교한다. 같으면 exit 0, 다르면 첫 불일치를 출력하고 exit 1.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --fixed-fps 64 --path godot --script res://tests/drive_replay_test.gd
extends SceneTree

## 재생할 입력 열(계약 5-2 형식). 시동·기어·가속·조향·주행 중 기어 거부·제동·P·리셋을 한 번씩 거친다.
const ENTRIES := [
	{"ticks": 64, "command": "engine_toggle"},
	{"ticks": 1, "command": "gear_drive"},
	{"ticks": 320, "throttle": 0.5, "brake": 0.0, "steering": 0.2},
	{"ticks": 64, "command": "gear_reverse", "throttle": 0.3, "steering": -0.4},
	{"ticks": 192, "throttle": 0.0, "brake": 1.0, "steering": 0.0},
	{"ticks": 1, "command": "gear_park"},
	{"ticks": 64, "command": "reset_course", "brake": 0.0},
]
## 비교할 코스.
const COURSE := "training"
## 허용 오차(계약 5-3): 위치 m, heading rad, 속도 m/s.
const POSITION_TOLERANCE := 1e-4
const HEADING_TOLERANCE := 1e-5
const SPEED_TOLERANCE := 1e-4
## 물리 틱이 멈춰 끝나지 않을 때를 막는 프레임 상한(입력 열 틱 합의 몇 배).
const MAX_FRAMES := 4000

var vehicle: Node3D
## 지금 재생 중인 항목 번호와 그 항목의 남은 틱 수.
var index := 0
var remaining := 0
## 항목 사이에 유지되는 연속 입력(없는 키는 이전 값 유지, 처음은 0).
var input := {"throttle": 0.0, "brake": 0.0, "steering": 0.0}
## 실제 물리 틱 재생으로 모은 표본과, 지금 항목에서 거부된 api 명령 이름.
var samples: Array[Dictionary] = []
var rejected := PackedStringArray()
var frames := 0
var finished := false


## 확장을 확인하고 차량 노드를 장면에 넣는다. 첫 항목은 ready 시그널 뒤에 넣는다(계약 v1.1).
func _init() -> void:
	if not ClassDB.class_exists("SimVehicle") or not ClassDB.class_exists("SimReplay"):
		_fail("SimVehicle·SimReplay 클래스가 등록되지 않았습니다(cargo build -p sim-godot 필요).")
		return
	vehicle = ClassDB.instantiate("SimVehicle")
	vehicle.set("read_input", false)
	vehicle.set("course", COURSE)
	vehicle.connect("command_rejected", _on_rejected)
	vehicle.connect("tick_finished", _on_tick)
	vehicle.ready.connect(_on_ready)
	root.add_child(vehicle)


## 차량이 준비되면(틱 0) 첫 항목을 넣는다. 준비에 실패했으면 사유를 출력하고 끝낸다.
func _on_ready() -> void:
	if not vehicle.call("is_ready"):
		_fail("차량 준비 실패: %s" % vehicle.call("last_error"))
		return
	_start_entry()


## 지금 항목의 명령과 연속 입력을 넣고 남은 틱 수를 정한다.
func _start_entry() -> void:
	var entry: Dictionary = ENTRIES[index]
	if entry.has("command") and not vehicle.call("send_command", entry["command"]):
		_fail("entries[%d]: send_command(%s)가 대기열에 들어가지 않았습니다." % [index, entry["command"]])
		return
	for key in input:
		if entry.has(key):
			input[key] = float(entry[key])
	vehicle.call("set_manual_input", input["throttle"], input["brake"], input["steering"])
	remaining = entry["ticks"]


## api에서 보낸 명령이 상태 머신에서 거부되면 지금 항목의 거부 목록에 넣는다.
func _on_rejected(command: String, source: String, _reason: String) -> void:
	if source == "api":
		rejected.append(command)


## 틱이 끝날 때마다 남은 틱을 줄이고, 항목이 끝나면 표본을 읽은 뒤 다음 항목을 넣는다.
func _on_tick(tick: int) -> void:
	if finished:
		return
	remaining -= 1
	if remaining > 0:
		return
	samples.append({
		"tick": tick,
		"position": vehicle.position,
		"heading_rad": vehicle.rotation.y,
		"speed_mps": vehicle.call("speed_mps"),
		"gear": vehicle.call("gear"),
		"rejected": rejected,
	})
	rejected = PackedStringArray()
	index += 1
	if index < ENTRIES.size():
		_start_entry()
	else:
		finished = true
		call_deferred("_compare")


## 물리 틱이 돌지 않아 끝나지 않으면 프레임 상한에서 실패로 끝낸다.
func _process(_delta: float) -> bool:
	frames += 1
	if not finished and frames > MAX_FRAMES:
		_fail("%d 프레임 안에 재생이 끝나지 않았습니다(항목 %d, 남은 틱 %d)." % [MAX_FRAMES, index, remaining])
	return false


## Rust 단독 재생 표본과 하나씩 비교하고 결과에 맞는 종료 코드로 끝낸다.
func _compare() -> void:
	var expected: Dictionary = ClassDB.class_call_static("SimReplay", "run", COURSE, ENTRIES)
	if not expected["ok"]:
		_fail("SimReplay.run 실패: %s" % expected["error"])
		return
	var want: Array = expected["samples"]
	if want.size() != samples.size():
		_fail("표본 수가 다릅니다: 노드 %d, 재생 %d" % [samples.size(), want.size()])
		return
	for i in samples.size():
		var problem := _difference(samples[i], want[i])
		if not problem.is_empty():
			_fail("entries[%d] 표본 불일치(%s)\n  노드: %s\n  재생: %s" % [i, problem, samples[i], want[i]])
			return
	var bad: Dictionary = ClassDB.class_call_static("SimReplay", "run", COURSE, [{"ticks": 1, "throttle": 1.5}])
	if bad["ok"] or String(bad["error"]).is_empty():
		_fail("범위 밖 입력 열이 거부되지 않았습니다: %s" % bad)
		return
	var last: Dictionary = samples[samples.size() - 1]
	print("DRIVE REPLAY OK: %d개 표본 일치, 마지막 틱 %d, 거부 %s" % [samples.size(), last["tick"], _all_rejected()])
	_quit(0)


## 두 표본의 차이를 사람이 읽는 문장으로 돌려준다. 허용 오차 안이면 빈 문자열.
func _difference(node: Dictionary, replay: Dictionary) -> String:
	if node["tick"] != replay["tick"]:
		return "tick %d != %d" % [node["tick"], replay["tick"]]
	var distance: float = (node["position"] as Vector3).distance_to(replay["position"])
	if distance > POSITION_TOLERANCE:
		return "위치 차이 %.6f m" % distance
	var heading := absf(angle_difference(node["heading_rad"], replay["heading_rad"]))
	if heading > HEADING_TOLERANCE:
		return "heading 차이 %.7f rad" % heading
	var speed := absf(float(node["speed_mps"]) - float(replay["speed_mps"]))
	if speed > SPEED_TOLERANCE:
		return "속도 차이 %.6f m/s" % speed
	if node["gear"] != replay["gear"]:
		return "기어 %s != %s" % [node["gear"], replay["gear"]]
	if PackedStringArray(node["rejected"]) != PackedStringArray(replay["rejected"]):
		return "거부 %s != %s" % [node["rejected"], replay["rejected"]]
	return ""


## 모든 항목에서 거부된 명령 이름을 모은다(출력용).
func _all_rejected() -> PackedStringArray:
	var names := PackedStringArray()
	for sample in samples:
		names.append_array(sample["rejected"])
	return names


## 실패 사유를 출력하고 exit 1로 끝낸다.
func _fail(message: String) -> void:
	finished = true
	printerr("DRIVE REPLAY FAIL: " + message)
	_quit(1)


## 차량 노드를 정리한 뒤 종료한다(종료 시 남은 노드 경고를 줄인다).
func _quit(code: int) -> void:
	if vehicle != null and is_instance_valid(vehicle):
		vehicle.queue_free()
	quit(code)
