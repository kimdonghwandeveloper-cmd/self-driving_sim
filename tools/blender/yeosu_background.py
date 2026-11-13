# 사용자가 BlenderGIS로 만든 여수 엑스포 .blend를 코스 좌표(실제 미터)의 표시용 GLB로 내보낸다.
#
# 실행(원본 .blend는 읽기만 하고 저장하지 않는다):
#   blender -b <원본.blend> --factory-startup --python tools/blender/yeosu_background.py -- <출력.glb> [<도로.json>]
#
# - `<도로.json>`을 주면 도로(`Ways`)의 바닥면을 주행 코스용 다각형 자료(road-polygons-v1)로도 쓴다.
#   좌표는 코스 지면 (x = 동쪽, z = 남쪽) m이며, 높이 0 m 부근(1 m 미만)의 위를 보는 면만 넣는다.
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
# 주행 코스 다각형 자료 형식 이름(sim-core world::road_polygons와 같아야 한다).
ROAD_POLYGONS_FORMAT = "road-polygons-v1"
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
    """`--` 뒤 인자에서 출력 GLB 경로와 (있으면) 도로 다각형 JSON 경로를 읽는다."""
    args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not 1 <= len(args) <= 2 or not args[0].endswith(".glb") or (len(args) == 2 and not args[1].endswith(".json")):
        raise SystemExit("usage: blender -b <src.blend> --python yeosu_background.py -- <out.glb> [<roads.json>]")
    return args[0], (args[1] if len(args) == 2 else None)


def write_road_polygons(obj, z_offset, path):
    """변환된 도로 객체에서 위를 보는 바닥면만 골라 코스 지면 (x, z) 다각형 JSON으로 쓴다."""
    mesh = obj.data
    polygons = []
    for face in mesh.polygons:
        points = [mesh.vertices[index].co for index in face.vertices]
        if abs(face.normal.z) < 0.5 or any(p.z - z_offset >= MAX_ROAD_FACE_Z_M for p in points):
            continue
        polygons.append([[round(p.x, 2), round(-p.y, 2)] for p in points])
    data = {
        "format": ROAD_POLYGONS_FORMAT,
        "origin": list(COURSE_ORIGIN),
        "source": "user BlenderGIS map (OSM ways) via tools/blender/yeosu_background.py",
        "polygons": polygons,
    }
    with open(path, "w", encoding="utf-8", newline="\n") as file:
        json.dump(data, file, separators=(",", ":"))
        file.write("\n")
    print(f"wrote {path}: {len(polygons)} road polygons")


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
        write_road_polygons(bpy.data.objects["Roads"], KEEP["Ways"][2], roads_out)
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
