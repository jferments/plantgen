"""Import .glb files in Blender, one at a time, and say what Blender holds.

Runs inside Blender, headless:

    blender -b --factory-startup --python-exit-code 1 \
        --python tools/blender/in_blender.py -- FILE.glb...

Each file is imported into an empty scene with File > Import > glTF 2.0 and
its defaults. For each, one line `PLANTGEN_BLENDER {json}` gives the
triangles Blender holds (every instance counted), the mesh objects, the
images that have pixels, the meshes with colour attributes and whether a
material reads them, the mesh attributes, the importer's error if it
refused the file, and its warnings. check_exports.py compares that with
what the file says it holds (glb.py); this script judges nothing.
"""
import contextlib
import io
import json
import sys
import time

import bpy

MARK = "PLANTGEN_BLENDER "
WORDS = ("error", "warn", "not supported", "unsupported", "not available", "extension")


def held() -> dict:
    tris_of: dict[str, int] = {}
    triangles = 0
    objects = 0
    depsgraph = bpy.context.evaluated_depsgraph_get()
    for inst in depsgraph.object_instances:
        ob = inst.object
        if ob.type != "MESH":
            continue
        key = ob.original.data.name if ob.original and ob.original.data else ob.name
        if key not in tris_of:
            mesh = ob.data
            mesh.calc_loop_triangles()
            tris_of[key] = len(mesh.loop_triangles)
        triangles += tris_of[key]
        objects += 1
    meshes = [m for m in bpy.data.meshes if m.users]
    coloured = [m for m in meshes if len(m.color_attributes)]
    reads_colour = False
    for m in coloured:
        for mat in m.materials:
            if mat and mat.use_nodes and any(
                n.type in ("VERTEX_COLOR", "ATTRIBUTE") for n in mat.node_tree.nodes
            ):
                reads_colour = True
    attributes = sorted({a.name for m in meshes for a in m.attributes})
    images = [i for i in bpy.data.images if i.type == "IMAGE" and i.size[0] > 0]
    return {
        "triangles": triangles,
        "mesh_objects": objects,
        "images": len(images),
        "coloured_meshes": len(coloured),
        "material_reads_colour": reads_colour,
        "attributes": attributes,
    }


def check(path: str) -> dict:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    log = io.StringIO()
    error = None
    start = time.monotonic()
    try:
        with contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
            result = bpy.ops.import_scene.gltf(filepath=path)
        if "FINISHED" not in result:
            error = f"importer returned {sorted(result)}"
    except Exception as exc:  # the importer raises on a refused file
        lines = str(exc).strip().splitlines() or [type(exc).__name__]
        error = lines[-1][:300]
    seconds = round(time.monotonic() - start, 2)
    warnings = [
        line.strip()[:300]
        for line in log.getvalue().splitlines()
        if any(w in line.lower() for w in WORDS)
    ][:10]
    out = {"file": path, "seconds": seconds, "error": error, "warnings": warnings}
    out.update(held())
    return out


def main() -> None:
    files = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    print(MARK + json.dumps({"blender": bpy.app.version_string}), flush=True)
    for path in files:
        print(MARK + json.dumps(check(path)), flush=True)


main()
