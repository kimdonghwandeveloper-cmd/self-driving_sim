# 시작 장면 스크립트. 실행하면 Rust 확장 연결 상태를 출력 패널에 찍는다.
extends Node3D


## 장면이 준비되면 SimBridge(Rust)를 불러 버전과 계산 결과를 출력한다. 확장이 없으면 오류를 남긴다.
func _ready() -> void:
	if not ClassDB.class_exists("SimBridge"):
		push_error("SimBridge 없음: cargo.exe build -p sim-godot 후 에디터를 다시 여세요.")
		return
	var version: String = ClassDB.class_call_static("SimBridge", "version")
	var mps: float = ClassDB.class_call_static("SimBridge", "kmh_to_mps", 40.0)
	print("%s 연결됨, 40 km/h = %.3f m/s" % [version, mps])
