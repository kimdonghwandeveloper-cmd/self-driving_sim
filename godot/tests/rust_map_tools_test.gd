extends SceneTree
## Rust로 만든 실제 지도 산출물을 Godot glTF 로더와 기존 배경 형상으로 대조한다.
func _init() -> void:
 call_deferred("_run")

## GLB 파일을 읽어 독립 장면으로 생성하고 오류를 명시한다.
func _load_glb(path: String) -> Node3D:
 var document := GLTFDocument.new()
 var state := GLTFState.new()
 var error := document.append_from_file(path, state)
 if error != OK:
  push_error("GLB 읽기 실패: %s (%s)" % [path, error])
  return null
 return document.generate_scene(state)

## 자식 메시를 이름 기준으로 수집한다.
func _meshes(node: Node, result: Dictionary) -> void:
 if node is MeshInstance3D:
  result[node.name] = node
 for child in node.get_children():
  _meshes(child, result)

## 실제 내보내기 두 파일과 기존 여섯 통로·캐노피·지면을 비교한다.
func _run() -> void:
 var args := OS.get_cmdline_user_args()
 if args.size() != 2:
  printerr("usage: -- <converted-background.glb> <convention-background.glb>")
  quit(1)
  return
 var source := _load_glb(args[0])
 var generated := _load_glb(args[1])
 var original := _load_glb("res://assets/maps/yeosu-expo/convention-background.glb")
 if source == null or generated == null or original == null:
  quit(1)
  return
 var old_meshes := {}
 var new_meshes := {}
 var source_meshes := {}
 _meshes(source, source_meshes)
 _meshes(generated, new_meshes)
 _meshes(original, old_meshes)
 var failures: Array[String] = []
 if not source_meshes.has("Ways") or not source_meshes.has("Areas") or not source_meshes.has("EXPORT_OSM_MAPNIK_WM"):
  failures.append("변환한 원본 객체 누락")
 var file := FileAccess.open("res://../testdata/yeosu-expo/convention/trace.json", FileAccess.READ)
 var trace: Dictionary = JSON.parse_string(file.get_as_text().trim_prefix("\ufeff"))
 var pairs := {"Aerial_Context":"Aerial_Context", "Convention_Gallery_Canopy":"Convention_Gallery_Canopy"}
 for road in trace.roads:
  pairs["Convention_" + road.file] = "RustRoad_" + str(int(road.id))
 for old_name in pairs:
  var new_name: String = pairs[old_name]
  if not old_meshes.has(old_name) or not new_meshes.has(new_name):
   failures.append("메시 누락: " + old_name)
   continue
  var a: AABB = old_meshes[old_name].get_aabb()
  var b: AABB = new_meshes[new_name].get_aabb()
  if a.position.distance_to(b.position) > 0.003 or a.size.distance_to(b.size) > 0.006:
   failures.append("형상 범위 차이: %s %s / %s" % [old_name, a, b])
 var image_mesh: MeshInstance3D = new_meshes.get("Aerial_Context")
 if image_mesh:
  var material: StandardMaterial3D = image_mesh.mesh.surface_get_material(0)
  if material == null or material.albedo_texture == null:
   failures.append("이미지 텍스처 누락")
  var arrays := image_mesh.mesh.surface_get_arrays(0)
  var verts: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
  var indices: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
  var normal := (verts[indices[1]] - verts[indices[0]]).cross(verts[indices[2]] - verts[indices[0]])
  # Godot glTF 로더는 앞면 규칙에 맞춰 인덱스를 뒤집으므로 기존 배경과 방향을 비교한다.
  var old_arrays: Array = old_meshes["Aerial_Context"].mesh.surface_get_arrays(0)
  var old_verts: PackedVector3Array = old_arrays[Mesh.ARRAY_VERTEX]
  var old_indices: PackedInt32Array = old_arrays[Mesh.ARRAY_INDEX]
  var old_normal := (old_verts[old_indices[1]] - old_verts[old_indices[0]]).cross(old_verts[old_indices[2]] - old_verts[old_indices[0]])
  if normal.dot(old_normal) <= 0.0:
   failures.append("이미지 지면 뒤집힘")
 source.free()
 generated.free()
 original.free()
 if failures.is_empty():
  print("RUST MAP TOOLS OK: original GLB loaded; six roads, canopy and textured ground matched")
  quit(0)
 else:
  for failure in failures:
   printerr(failure)
  quit(1)