# 사용자가 BlenderGIS로 만든 여수 엑스포 .blend를 코스 좌표(실제 미터)의 표시용 GLB로 내보낸다.
#
# 실행(원본 .blend는 읽기만 하고 저장하지 않는다):
#   blender -b <원본.blend> --factory-startup --python tools/blender/yeosu_background.py -- <출력.glb> [<도로.json>]
#
# - `<도로.json>`을 주면 도로(`Ways`)를 주행 코스용 중심선 자료(road-lines-v1)로도 쓴다. 맵 도로는 way마다
#   사각형 면이 이어진 띠이므로, 띠를 따라 가로 변의 가운데 점을 이어 중심선을, 가로 변 길이의 중앙값을 폭으로 삼는다.
#   좌표는 코스 지면 (x = 동쪽, z = 남쪽) m이며, 높이 0 m 부근(1 m 미만)의 위를 보는 면만 쓴다.
# - 원본은 Web Mercator(EPSG:3857) 좌표이고, 장면 속성 `crs x`/`crs y`가 Blender 원점의 3857 좌표다.
# - 각 정점을 위경도로 되돌린 뒤 sim-core `world::map::project`와 같은 식(구 반지름 6371008.8 m,
#   원점 위도의 cos)으로 코스 원점 기준 동(+X)·북(+Y) 미터로 바꾼다. 객체 배율·위치(꾸밈용 3배 등)는 쓰지 않는다.
# - glTF +Y up으로 내보내므로 Godot에서는 +X 동쪽, +Z 남쪽, 원점 = 코스 원점이 된다.
import json
import math
import sys

import bpy

# testdata/yeosu-expo/course.toml의 원점 [위도, 경도].
COURSE_ORIGIN = (34.7509700, 127.7493575)
# sim-core world::map의 지구 반지름(m).
EARTH_RADIUS_M = 6_371_008.8
# Web Mercator 구 반지름(m).
MERCATOR_RADIUS_M = 6_378_137.0
# 주행 코스 중심선 자료 형식 이름(sim-core world::road_polygons와 같아야 한다).
ROAD_LINES_FORMAT = "road-lines-v1"
# 이 높이(m, 원본 기준)보다 높은 점이 있는 면은 도로 바닥이 아닌 것으로 보고 뺀다.
MAX_ROAD_FACE_Z_M = 1.0
# 내보낼 원본 객체 → (GLB 이름, 재질 이름, 높이 오프셋 m). 주행 노면(높이 0)과 바닥(-0.10) 사이에 둔다.
KEEP = {
    "Ways": ("Roads", "Road", -0.04),
    "Areas": ("Areas", "Area", -0.06),
    "EXPORT_OSM_MAPNIK_WM": ("Basemap", "Basemap", -0.08),
}


def mercator_to_lat_lon(x, y):
    """Web Mercator 좌표(m)를 위경도(도)로 바꾼다."""
    lon = math.degrees(x / MERCATOR_RADIUS_M)
    lat = math.degrees(2.0 * math.atan(math.exp(y / MERCATOR_RADIUS_M)) - math.pi / 2.0)
    return lat, lon


def project(lat, lon):
    """위경도를 코스 원점 기준 동·북 미터로 바꾼다(sim-core world::map::project와 같은 식)."""
    lat0, lon0 = COURSE_ORIGIN
    east = EARTH_RADIUS_M * math.radians(lon - lon0) * math.cos(math.radians(lat0))
    north = EARTH_RADIUS_M * math.radians(lat - lat0)
    return east, north


def output_paths():
    """`--` 뒤 인자에서 출력 GLB 경로와 (있으면) 도로 중심선 JSON 경로를 읽는다."""
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not 1 <= len(args) <= 2 or not args[0].endswith(".glb") or (len(args) == 2 and not args[1].endswith(".json")):
        raise SystemExit("usage: blender -b <src.blend> --python yeosu_background.py -- <out.glb> [<roads.json>]")
    return args[0], (args[1] if len(args) == 2 else None)


def road_faces(mesh, z_offset):
    """위를 보고 높이 0 m 부근(원본 기준 1 m 미만)에 있는 사각형 면 번호만 고른다."""
    faces = []
    for face in mesh.polygons:
        points = [mesh.vertices[index].co for index in face.vertices]
        if len(points) != 4 or abs(face.normal.z) < 0.5:
            continue
        if any(p.z - z_offset >= MAX_ROAD_FACE_Z_M for p in points):
            continue
        faces.append(face.index)
    return faces


def opposite_edge(mesh, face_index, edge_key):
    """사각형 면에서 `edge_key`와 꼭짓점을 나누지 않는 맞은편 변을 찾는다."""
    for key in mesh.polygons[face_index].edge_keys:
        if not set(key) & set(edge_key):
            return key
    raise SystemExit(f"face {face_index} is not a quad strip piece")


def road_lines(mesh, faces):
    """면이 이어진 띠마다 중심선 점(가로 변 가운데)과 폭(가로 변 길이 중앙값), 닫힌 고리 여부를 구한다."""
    face_set = set(faces)
    edge_faces = {}
    for index in faces:
        for key in mesh.polygons[index].edge_keys:
            edge_faces.setdefault(key, []).append(index)
    neighbors = {index: [] for index in faces}
    for key, owners in edge_faces.items():
        if len(owners) == 2:
            a, b = owners
            neighbors[a].append((b, key))
            neighbors[b].append((a, key))
    if any(len(links) > 2 for links in neighbors.values()):
        raise SystemExit("road faces branch; expected one strip per way")

    def middle(key):
        """변의 가운데 점을 코스 지면 (x, z)로 돌려준다."""
        a, b = (mesh.vertices[i].co for i in key)
        return [round((a.x + b.x) / 2.0, 3), round(-(a.y + b.y) / 2.0, 3)]

    def length(key):
        """변 길이(m)."""
        a, b = (mesh.vertices[i].co for i in key)
        return math.hypot(a.x - b.x, a.y - b.y)

    lines = []
    seen = set()
    for first in faces:
        if first in seen:
            continue
        # 띠의 한쪽 끝(이웃 1개 이하)부터 걷는다. 끝이 없으면 닫힌 고리다.
        stack, component = [first], []
        seen.add(first)
        while stack:
            face = stack.pop()
            component.append(face)
            for other, _ in neighbors[face]:
                if other not in seen and other in face_set:
                    seen.add(other)
                    stack.append(other)
        ends = [face for face in component if len(neighbors[face]) < 2]
        closed = not ends
        start = min(ends) if ends else min(component)
        if not neighbors[start]:
            # 면 하나짜리 띠: 짧은 쪽 맞은편 변 두 개를 가로 변으로 본다.
            keys = mesh.polygons[start].edge_keys
            pair = min(((keys[0], keys[2]), (keys[1], keys[3])), key=lambda p: length(p[0]) + length(p[1]))
            cross = list(pair)
        else:
            cross = []
            if not closed:
                # 열린 띠의 시작 면: 이웃과 나누는 변의 맞은편이 시작 가로 변.
                cross.append(opposite_edge(mesh, start, neighbors[start][0][1]))
            previous, face = None, start
            while True:
                links = [(other, key) for other, key in neighbors[face] if other != previous][:1]
                if not links:
                    break
                other, key = links[0]
                cross.append(key)
                if other == start:
                    break
                previous, face = face, other
                if len(neighbors[face]) == 1:
                    cross.append(opposite_edge(mesh, face, key))
                    break
        widths = sorted(length(key) for key in cross)
        lines.append({
            "points": [middle(key) for key in cross],
            "width_m": round(widths[len(widths) // 2], 3),
            "closed": closed,
        })
    return lines


def write_road_lines(obj, z_offset, path):
    """변환된 도로 객체의 띠마다 중심선·폭을 뽑아 road-lines-v1 JSON으로 쓴다."""
    mesh = obj.data
    lines = road_lines(mesh, road_faces(mesh, z_offset))
    data = {
        "format": ROAD_LINES_FORMAT,
        "origin": list(COURSE_ORIGIN),
        "source": "user BlenderGIS map (OSM ways) via tools/blender/yeosu_background.py",
        "lines": lines,
    }
    with open(path, "w", encoding="utf-8", newline="\n") as file:
        json.dump(data, file, separators=(",", ":"))
        file.write("\n")
    print(f"wrote {path}: {len(lines)} road lines")


def convert_object(obj, crs_x, crs_y, name, material_name, z_offset):
    """객체 메시 정점을 코스 미터 좌표로 바꾸고 변환을 단위 행렬로 만든다(메모리에서만)."""
    if any(abs(angle) > 1e-6 for angle in obj.rotation_euler):
        raise SystemExit(f"{obj.name}: rotated objects are not supported")
    mesh = obj.data
    for vertex in mesh.vertices:
        lat, lon = mercator_to_lat_lon(crs_x + vertex.co.x, crs_y + vertex.co.y)
        east, north = project(lat, lon)
        vertex.co = (east, north, vertex.co.z + z_offset)
    mesh.update()
    obj.location = (0.0, 0.0, 0.0)
    obj.scale = (1.0, 1.0, 1.0)
    obj.name = name
    mesh.name = name
    for material in mesh.materials:
        if material is not None:
            material.name = material_name


def main():
    """필요한 객체만 남겨 좌표를 바꾸고 GLB로 내보낸다. .blend는 저장하지 않는다."""
    out, roads_out = output_paths()
    scene = bpy.context.scene
    crs_x, crs_y = float(scene["crs x"]), float(scene["crs y"])
    for obj in list(bpy.data.objects):
        if obj.name not in KEEP:
            bpy.data.objects.remove(obj)
    for source, (name, material_name, z_offset) in KEEP.items():
        obj = bpy.data.objects.get(source)
        if obj is None or obj.type != "MESH":
            raise SystemExit(f"missing mesh object {source}")
        convert_object(obj, crs_x, crs_y, name, material_name, z_offset)
    for obj in bpy.data.objects:
        mesh = obj.data
        xs = [v.co.x for v in mesh.vertices]
        ys = [v.co.y for v in mesh.vertices]
        print(f"converted {obj.name}: {len(mesh.vertices)} vertices, "
              f"east {min(xs):.1f}..{max(xs):.1f} m, north {min(ys):.1f}..{max(ys):.1f} m")
    if roads_out is not None:
        write_road_lines(bpy.data.objects["Roads"], KEEP["Ways"][2], roads_out)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_yup=True,
        export_apply=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
    )
    print(f"wrote {out}")


main()
