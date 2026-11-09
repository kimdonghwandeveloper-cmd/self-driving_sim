# GD-3 프레임 시간 측정. 실제 창(헤드리스 아님)에서 주행 화면을 열고 차량을 천천히 몰며 프레임 시간을 잰다.
# 결과는 평균·95번째 백분위·최대 프레임 시간(ms)과 평균 FPS로 출력한다. 측정이므로 목표(60 FPS) 미달이어도 exit 0이다.
# 실행(워크트리 최상위):
#   godot --path godot --resolution 1280x720 --script res://tests/frame_time_probe.gd -- [--course=<코스>] [--seconds=<초>] [--capture=<png 경로>]
# 기본 코스는 저장소의 여수 교차로 경로(map:assets/maps/yeosu-expo/route-junctions.toml), 기본 측정 시간은 20초.
extends SceneTree

const Drive = preload("res://presentation/drive_screen.gd")
## 측정 전에 버리는 프레임 수(셰이더 컴파일·첫 그림자 등).
const WARMUP_FRAMES := 120

var course := "map:assets/maps/yeosu-expo/route-junctions.toml"
var seconds := 20.0
var capture := ""
var drive: Node3D
var frame_ms := PackedFloat64Array()
var warmup := WARMUP_FRAMES
var started_ms := 0


## 인자를 읽고 주행 화면을 연다.
func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--course="):
			course = arg.trim_prefix("--course=")
		elif arg.begins_with("--seconds="):
			seconds = maxf(float(arg.trim_prefix("--seconds=")), 1.0)
		elif arg.begins_with("--capture="):
			capture = arg.trim_prefix("--capture=")
	call_deferred("_open")


## 주행 화면을 만들고 차량이 있으면 시동·D·약한 가속을 넣는다.
func _open() -> void:
	drive = Drive.new()
	root.add_child(drive)
	drive.start(course)
	if not drive.failure.is_empty():
		printerr("FRAME PROBE FAIL: " + drive.failure)
		quit(1)
		return
	if not drive.is_preview:
		drive.vehicle.set("read_input", false)
		drive.vehicle.call("send_command", "engine_toggle")
		drive.vehicle.call("send_command", "gear_drive")
		drive.vehicle.call("set_manual_input", 0.25, 0.0, 0.0)


## 매 프레임 시간을 모으고 측정 시간이 지나면 결과를 출력한다.
func _process(delta: float) -> bool:
	if drive == null or not drive.failure.is_empty():
		return false
	if warmup > 0:
		warmup -= 1
		if warmup == 0:
			started_ms = Time.get_ticks_msec()
		return false
	frame_ms.append(delta * 1000.0)
	if Time.get_ticks_msec() - started_ms >= seconds * 1000.0:
		call_deferred("_report")
		drive = null
	return false


## 통계를 출력하고, 요청이 있으면 마지막 화면을 저장한 뒤 끝낸다.
func _report() -> void:
	var sorted := frame_ms.duplicate()
	sorted.sort()
	var total := 0.0
	for value in sorted:
		total += value
	var average := total / sorted.size()
	var p95: float = sorted[mini(int(sorted.size() * 0.95), sorted.size() - 1)]
	print("FRAME PROBE: course=%s renderer=%s gpu=%s size=%s frames=%d avg=%.2f ms p95=%.2f ms max=%.2f ms fps=%.1f" % [
		course, RenderingServer.get_current_rendering_method(), RenderingServer.get_video_adapter_name(),
		root.size, sorted.size(), average, p95, sorted[sorted.size() - 1], 1000.0 / average])
	if not capture.is_empty():
		await RenderingServer.frame_post_draw
		root.get_texture().get_image().save_png(capture)
	quit(0)
