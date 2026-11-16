extends Control
## 0~60km/h 눈금과 속도 바늘만 그리는 표시용 계기판.

var kmh := 0.0


## 속도가 바뀔 때만 바늘을 다시 그린다.
func set_speed(value: float) -> void:
	if value != kmh:
		kmh = value
		queue_redraw()


## 실제 속도는 숫자로 별도 표시하며 바늘은 눈금 끝에서 멈춘다.
func _draw() -> void:
	var center := size * 0.5
	var radius := minf(size.x, size.y) * 0.43
	var first := deg_to_rad(140.0)
	var last := deg_to_rad(400.0)
	draw_arc(center, radius, first, last, 64, Color("375467"), 3.0, true)
	for tick in 13:
		var angle := lerpf(first, last, float(tick) / 12.0)
		var direction := Vector2.from_angle(angle)
		draw_line(center + direction * (radius - (7.0 if tick % 2 == 0 else 4.0)), center + direction * radius, Color("b1c5d2"), 1.0, true)
	var angle := lerpf(first, last, clampf(absf(kmh) / 60.0, 0.0, 1.0))
	draw_line(center, center + Vector2.from_angle(angle) * (radius - 9), Color("72e5ce"), 2.0, true)
