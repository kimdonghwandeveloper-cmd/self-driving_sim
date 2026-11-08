extends Node3D
## 메뉴·주행·지도를 관리한다. 각 화면은 새 세션으로 생성한다.

const Drive = preload("res://presentation/drive_screen.tscn")
const Map = preload("res://presentation/map_screen.tscn")
const Menu = preload("res://presentation/menu_screen.tscn")
var screen: Node
var back_button := Button.new()

## 메뉴 복귀 버튼을 구성하고 첫 화면으로 메인 메뉴를 연다.
func _ready() -> void:
	var canvas := CanvasLayer.new()
	canvas.layer = 10
	add_child(canvas)
	back_button.position = Vector2(20, 20)
	back_button.text = "메뉴로 돌아가기  (Esc)"
	back_button.pressed.connect(_show_menu)
	canvas.add_child(back_button)
	_show_menu()

## 이전 화면을 즉시 트리에서 분리해 입력·물리가 다음 화면에 남지 않게 한다.
func _clear_screen() -> void:
	if is_instance_valid(screen):
		screen.process_mode = Node.PROCESS_MODE_DISABLED
		remove_child(screen)
		screen.queue_free()

## 새 메인 메뉴를 만들고 각 화면 요청을 연결한다.
func _show_menu() -> void:
	_clear_screen()
	back_button.hide()
	screen = Menu.instantiate()
	add_child(screen)
	screen.drive_requested.connect(_show_drive)
	screen.map_requested.connect(_show_map)
	screen.quit_requested.connect(_quit)

## 코스 화면을 새 차량 세션으로 연다.
func _show_drive(course: String) -> void:
	_clear_screen()
	back_button.show()
	screen = Drive.instantiate()
	add_child(screen)
	screen.start(course)

## 외부 요청을 하지 않는 지도 화면을 연다.
func _show_map() -> void:
	_clear_screen()
	back_button.show()
	screen = Map.instantiate()
	add_child(screen)

## Esc를 눌렀을 때 현재 화면을 버리고 메뉴로 돌아간다.
func _unhandled_key_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo and event.physical_keycode == KEY_ESCAPE:
		_show_menu()
		get_viewport().set_input_as_handled()

## 메뉴에서 명시적으로 요청한 경우에만 애플리케이션을 종료한다.
func _quit() -> void:
	get_tree().quit()