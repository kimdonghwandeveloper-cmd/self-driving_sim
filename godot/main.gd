extends Node3D
## Godot 화면 뼈대 진입점. Rust 로직은 호출 경계를 통해서만 사용한다.

const Drive = preload("res://presentation/drive_screen.tscn")
const Map = preload("res://presentation/map_screen.tscn")
var screen: Node
var toolbar := HBoxContainer.new()

## 최소 화면 전환 버튼을 만들고 개발 코스 화면을 연다.
func _ready() -> void:
	var canvas := CanvasLayer.new()
	canvas.layer = 10
	add_child(canvas)
	toolbar.position = Vector2(20, 20)
	canvas.add_child(toolbar)
	_button("개발 코스", _show_drive.bind("training"))
	_button("보정 코스", _show_drive.bind("calibration"))
	_button("지도 화면", _show_map)
	_show_drive("training")

## 화면 선택 버튼을 연결한다.
func _button(text: String, action: Callable) -> void:
	var button := Button.new()
	button.text = text
	button.pressed.connect(action)
	toolbar.add_child(button)

## 이전 화면을 즉시 트리에서 분리해 입력·물리가 다음 화면에 남지 않게 한다.
func _clear_screen() -> void:
	if is_instance_valid(screen):
		screen.process_mode = Node.PROCESS_MODE_DISABLED
		remove_child(screen)
		screen.queue_free()

## 코스 화면을 새 차량 세션으로 연다.
func _show_drive(course: String) -> void:
	_clear_screen()
	screen = Drive.instantiate()
	add_child(screen)
	screen.start(course)

## 외부 요청을 하지 않는 지도 뼈대를 연다.
func _show_map() -> void:
	_clear_screen()
	screen = Map.instantiate()
	add_child(screen)