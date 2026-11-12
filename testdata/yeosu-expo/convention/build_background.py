"""Generate aligned training courses and a visual layer from the reviewed image trace."""
import json
import math
import os
from pathlib import Path

import bpy
from mathutils import Vector

directory = Path(os.environ["YEOSU_CONVENTION"])
data = json.loads((directory / "trace.json").read_text(encoding="utf-8-sig"))
lat0, lon0 = data["origin"]
radius = 6371008.8
a, b = data["bridge_controls"]
pixel_a = complex(a["pixel"][0], -a["pixel"][1])
pixel_b = complex(b["pixel"][0], -b["pixel"][1])
world_a = complex(*a["east_north_m"])
world_b = complex(*b["east_north_m"])
scale_rotation = (world_b - world_a) / (pixel_b - pixel_a)
preview_width = float(data["reference_width_px"])
content_width, content_height = data["reference_content_size_px"]


def world(point):
    """Map preview pixels to the simulator's east/north metre coordinates."""
    p = world_a + (complex(point[0], -point[1]) - pixel_a) * scale_rotation
    return Vector((p.real, p.imag))


def lat_lon(point):
    """Invert the simulator's local projection for the course JSON."""
    return {
        "lat": round(lat0 + math.degrees(point.y / radius), 8),
        "lon": round(lon0 + math.degrees(point.x / (radius * math.cos(math.radians(lat0)))), 8),
    }


def principled_shader(mat):
    """Return a Principled shader, creating and connecting one when Blender omitted it."""
    nodes = mat.node_tree.nodes
    shader = next((node for node in nodes if node.type == "BSDF_PRINCIPLED"), None)
    if shader is None:
        shader = nodes.new("ShaderNodeBsdfPrincipled")
        output = next((node for node in nodes if node.type == "OUTPUT_MATERIAL"), None)
        if output is None:
            output = nodes.new("ShaderNodeOutputMaterial")
        mat.node_tree.links.new(shader.outputs["BSDF"], output.inputs["Surface"])
    return shader


def material(name, rgb):
    """Create a rough material shared by a group of map features."""
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*rgb, 1)
    mat.use_nodes = True
    shader = principled_shader(mat)
    shader.inputs["Base Color"].default_value = mat.diffuse_color
    shader.inputs["Roughness"].default_value = 0.82
    return mat


def make_mesh(name, verts, faces, mat):
    """Add a render-only mesh with no collision metadata."""
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    mesh.materials.append(mat)
    return obj


def lift_transport_layers():
    """Raise imported OSM road and rail surfaces above the aerial reference plane."""
    prefixes = ("roads_", "paths_", "railways")
    for obj in bpy.context.scene.objects:
        if obj.type == "MESH" and obj.name.startswith(prefixes):
            for vertex in obj.data.vertices:
                vertex.co.z += 0.07


def aerial_context():
    """Project the supplied reference image onto the same local-metre frame as the roads."""
    image_path = Path(os.environ["YEOSU_REFERENCE_IMAGE"])
    image = bpy.data.images.load(str(image_path), check_existing=False)
    preview_height = content_height * preview_width / content_width
    corners = [world((0, 0)), world((preview_width, 0)),
               world((preview_width, preview_height)), world((0, preview_height))]
    mesh = bpy.data.meshes.new("Aerial_Context")
    mesh.from_pydata([(p.x, p.y, 0.005) for p in corners], [], [(0, 3, 2, 1)])
    mesh.uv_layers.new(name="Aerial UV")
    right = content_width / image.size[0]
    bottom = 1.0 - content_height / image.size[1]
    for loop, uv in zip(mesh.uv_layers.active.data,
                        [(0, 1), (0, bottom), (right, bottom), (right, 1)]):
        loop.uv = uv
    obj = bpy.data.objects.new("Aerial_Context", mesh)
    bpy.context.scene.collection.objects.link(obj)
    mat = bpy.data.materials.new("Aerial reference")
    mat.use_nodes = True
    nodes = mat.node_tree.nodes
    texture = nodes.new("ShaderNodeTexImage")
    texture.image = image
    shader = principled_shader(mat)
    mat.node_tree.links.new(texture.outputs["Color"], shader.inputs["Base Color"])
    shader.inputs["Roughness"].default_value = 0.95
    mesh.materials.append(mat)
    obj["source"] = image_path.name
    obj["purpose"] = "visual reference only; no collision"
    return obj


def ribbon(points, width, z):
    """Create upward-facing faces with shared vertices through each bend."""
    verts = []
    for i, point in enumerate(points):
        u = (points[i] - points[i - 1]).normalized() if i else (points[1] - point).normalized()
        v = (points[i + 1] - point).normalized() if i + 1 < len(points) else u
        n = Vector((-u.y, u.x))
        m = Vector((-v.y, v.x))
        mid = (n + m).normalized()
        off = mid * (width / 2 / max(0.5, mid.dot(m)))
        verts.extend([(point.x + off.x, point.y + off.y, z), (point.x - off.x, point.y - off.y, z)])
    return verts, [(2*i, 2*i+1, 2*i+3, 2*i+2) for i in range(len(points)-1)]


def append_beam(verts, faces, a, b, width, z, height):
    """Append one elevated rectangular rib without adding posts to the road."""
    base, _ = ribbon([a, b], width, z)
    i = len(verts)
    verts.extend(base + [(x, y, zz + height) for x, y, zz in base])
    faces.extend(tuple(i + v for v in f) for f in [(3,2,1,0),(4,5,7,6),(0,1,5,4),(1,3,7,5),(3,2,6,7),(2,0,4,6)])


asphalt = material("Convention pavement", (0.135, 0.145, 0.155))
gallery = material("Gallery paving", (0.24, 0.25, 0.26))
roof = material("Gallery roof white steel", (0.65, 0.72, 0.75))
lift_transport_layers()
aerial_context()
road_elements = []
for entry in data["roads"]:
    points = [world(p) for p in entry["pixels"]]
    verts, faces = ribbon(points, entry["width_m"], 0.03)
    make_mesh("Convention_" + entry["file"], verts, faces, gallery if "gallery" in entry["file"] else asphalt)
    road_elements.append({"type": "way", "id": entry["id"], "tags": {"highway": "service", "source": "manual aerial-image training approximation"}, "geometry": [lat_lon(p) for p in points]})
    (directory / (entry["file"] + ".toml")).write_text(
        '# Aerial-image training trace; ID is local and is not an OSM way ID.\n'
        f'source = "traced-roads.json"\norigin = [{lat0}, {lon0}]\nways = [{entry["id"]}]\n'
        f'temporary_width_m = {entry["width_m"]}\n', encoding="utf-8")
(directory / "traced-roads.json").write_text(json.dumps({"elements": road_elements}, indent=2) + "\n", encoding="utf-8")

canopy = [world(p) for p in data["gallery_canopy"]]
verts, faces = [], []
for a, b in zip(canopy, canopy[1:]):
    axis = (b - a).normalized()
    side = Vector((-axis.y, axis.x))
    for distance in range(2, int((b - a).length), 4):
        center = a + axis * distance
        append_beam(verts, faces, center - side * 5.4, center + side * 5.4, 0.28, 7.5, 0.32)
    for offset in [-5.4, 5.4]:
        append_beam(verts, faces, a + side * offset, b + side * offset, 0.36, 7.4, 0.5)
make_mesh("Convention_Gallery_Canopy", verts, faces, roof)

bpy.ops.object.select_all(action="DESELECT")
for obj in bpy.context.scene.objects:
    if obj.type == "MESH" and not obj.hide_get() and obj.data.polygons:
        obj.select_set(True)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.wm.save_as_mainfile(filepath=str(Path(os.environ["YEOSU_OUTPUT"]) / "yeosu-convention.blend"))
assert bpy.ops.export_scene.gltf(
    filepath=str(Path(os.environ["YEOSU_MAP_ASSET"]) / "background.glb"),
    export_format="GLB", export_yup=True, export_extras=True, use_selection=True
) == {"FINISHED"}
