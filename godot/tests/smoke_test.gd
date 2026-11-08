# Phase 5-0 연결 확인. Godot을 헤드리스로 띄워 Rust 확장(SimBridge)을 부르고 성공 0 / 실패 1로 끝낸다.
# 실행(워크트리 최상위): godot --headless --path godot --import 후
#                        godot --headless --path godot --script res://tests/smoke_test.gd
extends SceneTree


## 확장 클래스 등록 여부와 함수 결과를 검사하고 결과에 맞는 종료 코드로 끝낸다.
func _init() -> void:
	if not ClassDB.class_exists("SimBridge"):
		printerr("SMOKE FAIL: SimBridge 클래스가 등록되지 않았습니다.")
		quit(1)
		return
	var version: String = ClassDB.class_call_static("SimBridge", "version")
	var mps: float = ClassDB.class_call_static("SimBridge", "kmh_to_mps", 36.0)
	if not version.begins_with("sim-godot ") or not is_equal_approx(mps, 10.0):
		printerr("SMOKE FAIL: version=%s, 36 km/h -> %f m/s" % [version, mps])
		quit(1)
		return
	print("SMOKE OK: %s, 36 km/h = %.1f m/s" % [version, mps])
	quit(0)
