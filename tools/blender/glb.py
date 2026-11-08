"""What a .glb holds, read from its own JSON: what Blender should hold after importing it.

Plain Python with no dependencies, so it runs both outside Blender
(check_exports.py) and inside it (in_blender.py). Only the JSON chunk is read.
"""
from __future__ import annotations

import json
import struct

JSON_CHUNK = 0x4E4F534A

# Triangle-drawing primitive modes (glTF 2.0, mesh.primitive.mode).
TRIANGLES, TRIANGLE_STRIP, TRIANGLE_FAN = 4, 5, 6


def read_json(path: str) -> dict:
    """The JSON chunk of a GLB file; ValueError if it is not one."""
    with open(path, "rb") as f:
        head = f.read(20)
        if len(head) < 20:
            raise ValueError(f"{path}: too short for a GLB")
        magic, version, _length, chunk_len, chunk_type = struct.unpack("<4sIIII", head)
        if magic != b"glTF" or version != 2 or chunk_type != JSON_CHUNK:
            raise ValueError(f"{path}: not a glTF 2.0 binary file")
        return json.loads(f.read(chunk_len))


def primitive_triangles(gltf: dict, prim: dict) -> int:
    """Triangles one primitive draws (0 for points and lines)."""
    mode = prim.get("mode", TRIANGLES)
    if prim.get("indices") is not None:
        n = gltf["accessors"][prim["indices"]]["count"]
    else:
        n = gltf["accessors"][prim["attributes"]["POSITION"]]["count"]
    if mode == TRIANGLES:
        return n // 3
    if mode in (TRIANGLE_STRIP, TRIANGLE_FAN):
        return max(0, n - 2)
    return 0


def instances(gltf: dict, node: dict) -> int:
    """How many times a node draws its mesh: EXT_mesh_gpu_instancing's count, else 1."""
    ext = node.get("extensions", {}).get("EXT_mesh_gpu_instancing")
    if not ext:
        return 1
    attrs = ext.get("attributes", {})
    if not attrs:
        return 1
    return gltf["accessors"][next(iter(attrs.values()))]["count"]


def summary(gltf: dict) -> dict:
    """What the default scene draws: triangles (instances counted), meshes
    drawn, images, extensions, and whether any primitive has vertex colours
    or custom attributes (names starting with an underscore)."""
    meshes = gltf.get("meshes", [])
    nodes = gltf.get("nodes", [])
    mesh_tris = [sum(primitive_triangles(gltf, p) for p in m["primitives"]) for m in meshes]
    triangles = 0
    drawn = 0
    used_meshes: set[int] = set()
    stack = list(gltf["scenes"][gltf.get("scene", 0)]["nodes"]) if gltf.get("scenes") else []
    seen: set[int] = set()
    while stack:
        i = stack.pop()
        if i in seen:  # glTF forbids it; never loop on a bad file
            continue
        seen.add(i)
        node = nodes[i]
        if "mesh" in node:
            k = instances(gltf, node)
            triangles += mesh_tris[node["mesh"]] * k
            drawn += k
            used_meshes.add(node["mesh"])
        stack.extend(node.get("children", []))
    attrs = {a for m in (meshes[i] for i in used_meshes) for p in m["primitives"] for a in p["attributes"]}
    return {
        "triangles": triangles,
        "mesh_objects": drawn,
        "images": len(gltf.get("images", [])),
        "extensions_used": sorted(gltf.get("extensionsUsed", [])),
        "extensions_required": sorted(gltf.get("extensionsRequired", [])),
        "vertex_colours": "COLOR_0" in attrs,
        "custom_attributes": sorted(a for a in attrs if a.startswith("_")),
    }


def summarise(path: str) -> dict:
    return summary(read_json(path))
