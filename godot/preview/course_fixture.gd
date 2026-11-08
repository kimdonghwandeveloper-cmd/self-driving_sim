extends RefCounted
## 메시 조립 검사용 데이터. 실제 training/calibration 코스나 충돌 자료가 아니다.

## 계약 3절의 모든 형상 종류가 있는 정지 미리보기 자료를 만든다.
static func build(course: String) -> Dictionary:
	var length := 90.0 if course == "calibration" else 50.0
	return {
		"ok": true, "error": "", "name": course,
		"notices": PackedStringArray(["시험 형상 · 실측값 아님 · 주행/충돌 비활성"]),
		"start": {"position": Vector3.ZERO, "heading_rad": 0.0},
		"surfaces": [_rect(Vector3(0, 0, -20), Vector2(8, length))],
		"strips": [{"kind": "road", "left": PackedVector3Array([Vector3(-4, 0, -35), Vector3(-12, 0, -45), Vector3(-22, 0, -45)]),
			"right": PackedVector3Array([Vector3(4, 0, -35), Vector3(-8, 0, -51), Vector3(-22, 0, -51)])}],
		"areas": [{"center": Vector3(0, 0, -15), "boundary": PackedVector3Array([Vector3(-6, 0, -19), Vector3(6, 0, -19), Vector3(6, 0, -11), Vector3(-6, 0, -11)])}],
		"markings": [{"kind": "edge_line", "rect": _rect(Vector3(-3.7, 0, -20), Vector2(0.12, length))},
			{"kind": "edge_line", "rect": _rect(Vector3(3.7, 0, -20), Vector2(0.12, length))},
			{"kind": "stop_line", "rect": _rect(Vector3(0, 0, -10), Vector2(7.5, 0.3))}],
		"colliders": [{"kind": "curb", "height_m": 0.18, "rect": _rect(Vector3(-4.2, 0, -20), Vector2(0.3, length))},
			{"kind": "curb", "height_m": 0.18, "rect": _rect(Vector3(4.2, 0, -20), Vector2(0.3, length))},
			{"kind": "barrier", "height_m": 0.8, "rect": _rect(Vector3(0, 0, -length * 0.5 - 20), Vector2(8, 0.4))}]
	}

## 회전 없는 지면 직사각형을 계약 형식으로 만든다.
static func _rect(center: Vector3, size: Vector2) -> Dictionary:
	return {"center": center, "size": size, "heading_rad": 0.0}