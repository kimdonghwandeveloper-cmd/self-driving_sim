extends SceneTree
## 전용 지도 화면/클래스가 사라지고 OSM 지도와 공통 평가 기능이 유지되는지 검사한다.
var failures := PackedStringArray()

## 확장과 화면 검사를 다음 프레임에 시작한다.
func _init() -> void:
    call_deferred("_run")

## 제거한 클래스·메뉴·옛 파일 거부 및 OSM 안내선 평가의 활성 상태를 확인한다.
func _run() -> void:
    _check(not ClassDB.class_exists("SimLaneletPicker"), "제거한 지도 선택 클래스가 남아 있음")
    var menu: Control = load("res://presentation/menu_screen.gd").new()
    root.add_child(menu)
    for course in menu.course_names:
        _check(not str(course).to_lower().contains("lanelet"), "철회된 코스가 메뉴에 노출됨")
    menu.free()
    var vehicle := SimVehicle.new()
    vehicle.course = "map:testdata/yeosu-expo/route-junctions.toml"
    vehicle.read_input = false
    root.add_child(vehicle)
    await process_frame
    _check(vehicle.evaluation_metrics().get("active", false), "OSM 경로 평가기가 사라짐")
    vehicle.free()
    var file_name := "user://retired-format-%s.toml" % OS.get_process_id()
    var file := FileAccess.open(file_name, FileAccess.WRITE)
    file.store_string('format = "lanelet2-course-v1"\n')
    file.close()
    var result: Dictionary = SimCourse.build("map:" + ProjectSettings.globalize_path(file_name))
    _check(not result.get("ok", false) and str(result.get("error", "")).contains("support was removed"), "옛 형식의 지원 종료 안내 없음")
    DirAccess.remove_absolute(ProjectSettings.globalize_path(file_name))
    for frame in 3:
        await process_frame
    if failures.is_empty():
        print("OSM ONLY OK: 전용 지도 제거 · OSM 코스 · 공통 평가 · 옛 형식 안내")
        quit(0)
    else:
        for message in failures:
            printerr(message)
        quit(1)

## 실패를 모아 다른 회귀 항목도 검사한다.
func _check(condition: bool, message: String) -> void:
    if not condition:
        failures.append(message)
