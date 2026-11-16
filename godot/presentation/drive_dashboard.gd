extends Control
## 실제 Rust 상태만 표시하는 계기판. 입력과 차량 운동은 처리하지 않는다.

var speed := Label.new()
var gears := Label.new()
var state := Label.new()
var metrics := Label.new()
var panel := PanelContainer.new()


## 도로 시야를 남기는 하단 계기판과 자동 줄바꿈 상태 안내를 만든다.
func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	panel.name = "DashboardPanel"
	panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(panel)
	panel.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_WIDE)
	panel.offset_left = 20
	panel.offset_right = -20
	panel.offset_top = -310
	panel.offset_bottom = -100
	var style := StyleBoxFlat.new()
	style.bg_color = Color("101e2bee")
	style.border_color = Color("37576c")
	style.set_border_width_all(1)
	style.set_corner_radius_all(12)
	style.set_content_margin_all(16)
	panel.add_theme_stylebox_override("panel", style)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 28)
	panel.add_child(row)
	var cluster := VBoxContainer.new()
	cluster.custom_minimum_size.x = 180
	row.add_child(cluster)
	speed.text = "0 km/h"
	speed.add_theme_font_size_override("font_size", 36)
	cluster.add_child(speed)
	gears.add_theme_font_size_override("font_size", 20)
	gears.add_theme_color_override("font_color", Color("65e4ce"))
	cluster.add_child(gears)
	metrics.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	metrics.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	metrics.add_theme_font_size_override("font_size", 14)
	row.add_child(metrics)
	var scroll := ScrollContainer.new()
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(scroll)
	state.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	state.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	state.add_theme_font_size_override("font_size", 14)
	scroll.add_child(state)


## 속도·실제 기어와 안전 안내를 표시하고 없는 측정값은 미지원으로 구분한다.
func update_state(kmh: float, gear: String, description: String, values: Dictionary, elapsed_s: float) -> void:
	speed.text = "%.0f km/h" % absf(kmh)
	gears.text = "기어 " + gear
	state.text = description
	state.add_theme_color_override("font_color", Color("ffb08e") if description.contains("비상 정지 작동") else Color.WHITE)
	var seconds := int(elapsed_s)
	metrics.text = "시뮬레이션 시간 %02d:%02d\n" % [seconds / 60, seconds % 60]
	if values.get("active", false):
		metrics.text += "주행 거리 %.1f m · 충돌 %d회\nCTE %.2f m\n목표까지 직선 거리 %.1f m%s" % [
			values.distance_m, values.impact_count, values.cte_m, values.remaining_m,
			" · 목표 도달" if values.goal_reached else ""]
	else:
		metrics.text += "경로 평가 미지원 코스\nCTE·누적 거리·충돌 합계: —"


## 코스 초기화에 실패해도 숨은 텍스트 대신 화면에 오류를 표시한다.
func show_failure(message: String) -> void:
	speed.text = "준비 실패"
	gears.text = "주행 중지"
	metrics.text = "메뉴로 돌아가 다른 코스를 선택하세요."
	state.text = message
	state.add_theme_color_override("font_color", Color("ffb08e"))
