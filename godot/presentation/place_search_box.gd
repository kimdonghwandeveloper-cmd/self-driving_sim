extends HBoxContainer
## 장소 검색 칸 하나(출발지 또는 도착지, PLAN 4-11 2단계). 입력할 때마다 열린 지역의 건물·장소에서 찾고(4-9),
## Enter·검색 버튼이면 전국 검색(4-10, Nominatim)도 요청한다. 결과를 고르면 `picked(choice)`를 낸다.
## choice = {place: 열린 장소 번호(전국 결과는 -1), latlon: [위도, 경도](장소 번호면 빈 배열), label: 이름, view: 지도 좌표}.

signal picked(choice: Dictionary)

## Rust SimRoutePicker(지도 화면이 넘겨준다). 없으면 검색하지 않는다.
var picker: Object
var edit := LineEdit.new()
var results := OptionButton.new()
## 결과 상자 항목별 choice. 안내문·구분선은 null.
var items: Array = []
## 열린 지역 검색 결과({index, name, kind})와 마지막 전국 검색 결과({index, name, address, kind, view}).
var local_results: Array = []
var area_results: Array = []
## 기다리는 전국 검색 요청 번호(0이면 없음).
var request := 0
## 마지막 안내(검색 결과 수·실패 사유). 지도 화면이 상태 줄에 보인다.
var feedback := ""


## 칸·버튼·결과 상자를 만들고 이벤트를 잇는다. `placeholder`는 빈 칸 안내.
func setup(owner_picker: Object, placeholder: String) -> void:
	picker = owner_picker
	edit.placeholder_text = placeholder
	edit.custom_minimum_size.x = 170
	# 한글 IME는 Enter 때 조합 중인 마지막 글자가 아직 text에 없을 수 있어, 입력할 때마다 찾고 Enter·버튼은 조합 글자까지 합쳐 찾는다.
	edit.text_changed.connect(_typed)
	edit.text_submitted.connect(_submitted)
	add_child(edit)
	var button := Button.new()
	button.text = "검색"
	button.pressed.connect(func(): _submitted(edit.text))
	add_child(button)
	results.custom_minimum_size.x = 200
	results.item_selected.connect(_select)
	add_child(results)
	_fill([])


## 칸 글자를 검색 없이 바꾼다(고른 출발·도착 이름 표시용). `text` 속성 대입은 text_changed를 내지 않는다.
func show_label(text: String) -> void:
	edit.text = text
	area_results = []
	_fill([])


## 열린 지역의 건물·장소에서 찾아 결과 상자를 채운다.
func search_local(text: String) -> void:
	if picker == null:
		return
	var found: Array = picker.call("search_places", text)
	_fill(found)
	feedback = "검색 결과 %d곳" % found.size() if not found.is_empty() else "'%s' 이름의 장소가 열린 지역에 없습니다(Enter를 누르면 전국 검색)" % text


## 입력이 바뀔 때마다 열린 지역 안에서만 찾는다(Nominatim 이용 규칙: 입력 중 전국 검색 금지). 빈 칸이면 결과만 비운다.
func _typed(text: String) -> void:
	area_results = []
	if text.strip_edges().is_empty():
		_fill([])
		feedback = ""
	else:
		search_local(text)


## Enter·검색 버튼: 조합 중인 한글 글자까지 붙여 열린 지역에서 찾고 전국 검색도 요청한다.
func _submitted(text: String) -> void:
	var composing := DisplayServer.ime_get_text() if edit.has_focus() else ""
	var query := (text + composing).strip_edges()
	if query.is_empty() or picker == null:
		return
	area_results = []
	search_local(query)
	var result: Dictionary = picker.call("search_nationwide", query)
	if result.get("ok", false):
		request = int(result.request_id)
		feedback = "전국 검색 중... (OpenStreetMap Nominatim)"
	else:
		feedback = str(result.error)


## 이 칸이 기다리던 전국 검색 결과면 결과 상자 아래쪽에 붙이고 true.
func take_results(event: Dictionary) -> bool:
	if request == 0 or int(event.request_id) != request:
		return false
	request = 0
	area_results = event.get("results", [])
	_fill(local_results)
	feedback = str(event.message)
	if not area_results.is_empty() and is_inside_tree():
		results.show_popup()
	return true


## 결과 상자를 안내문 한 줄, 열린 지역 결과(이름 · 종류), 전국 결과(구분선 아래, 이름 — 주소)로 다시 만든다.
func _fill(found: Array) -> void:
	local_results = found
	results.clear()
	items.clear()
	results.add_item("검색 결과에서 고르기")
	items.append(null)
	for item in found:
		results.add_item("%s · %s" % [item.name, item.kind] if str(item.kind) != "" else str(item.name))
		items.append({"place": int(item.index), "latlon": PackedFloat64Array(), "label": str(item.name), "view": Vector2.INF})
	if not area_results.is_empty():
		results.add_separator("전국 검색")
		items.append(null)
		for item in area_results:
			results.add_item("%s — %s" % [item.name, item.address])
			var latlon: PackedFloat64Array = picker.call("view_to_latlon", item.view)
			items.append({"place": -1, "latlon": latlon, "label": str(item.name), "view": item.view})
	results.disabled = items.size() <= 1


## 결과 상자에서 고른 항목을 picked로 알리고 칸에 이름을 보인다.
func _select(index: int) -> void:
	results.select(0)
	if index < 1 or index >= items.size() or items[index] == null:
		return
	var choice: Dictionary = items[index]
	edit.text = choice.label
	picked.emit(choice)
