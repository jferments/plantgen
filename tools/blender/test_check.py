"""Tests of the Blender check's own logic, with no Blender: python3 -m unittest tools/blender/test_check.py"""
import json
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_exports  # noqa: E402
import glb  # noqa: E402


def gltf(nodes, meshes, accessors, scene_nodes=(0,), **extra):
    g = {"asset": {"version": "2.0"}, "scenes": [{"nodes": list(scene_nodes)}],
         "nodes": nodes, "meshes": meshes, "accessors": accessors}
    g.update(extra)
    return g


def write_glb(path, g):
    js = json.dumps(g).encode()
    js += b" " * (-len(js) % 4)
    body = (len(js)).to_bytes(4, "little") + glb.JSON_CHUNK.to_bytes(4, "little") + js
    head = b"glTF" + (2).to_bytes(4, "little") + (12 + len(body)).to_bytes(4, "little")
    Path(path).write_bytes(head + body)


class Summary(unittest.TestCase):
    def test_instances_children_and_modes(self):
        g = gltf(
            nodes=[
                {"children": [1, 2]},
                {"mesh": 0, "extensions": {"EXT_mesh_gpu_instancing": {"attributes": {"TRANSLATION": 2}}}},
                {"mesh": 1, "children": [3]},
                {"mesh": 0},
            ],
            meshes=[
                {"primitives": [{"attributes": {"POSITION": 0, "COLOR_0": 0}, "indices": 1}]},
                {"primitives": [{"attributes": {"POSITION": 0, "_PLANTGEN_WIND": 0}, "mode": 5},
                                {"attributes": {"POSITION": 0}, "mode": 1}]},
            ],
            accessors=[{"count": 10}, {"count": 12}, {"count": 7}],
            images=[{}, {}],
            extensionsRequired=["EXT_mesh_gpu_instancing"],
        )
        s = glb.summary(g)
        # 4 triangles x 7 instances, a strip of 10 (8), a line (0), 4 more.
        self.assertEqual(s["triangles"], 28 + 8 + 4)
        self.assertEqual(s["mesh_objects"], 7 + 1 + 1)
        self.assertEqual(s["images"], 2)
        self.assertTrue(s["vertex_colours"])
        self.assertEqual(s["custom_attributes"], ["_PLANTGEN_WIND"])

    def test_meshes_outside_the_scene_do_not_count(self):
        g = gltf(nodes=[{"mesh": 0}, {"mesh": 0}], scene_nodes=[0],
                 meshes=[{"primitives": [{"attributes": {"POSITION": 0}}]}], accessors=[{"count": 9}])
        self.assertEqual(glb.summary(g)["triangles"], 3)

    def test_read_glb(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "a.glb")
            write_glb(p, gltf(nodes=[{"mesh": 0}], meshes=[{"primitives": [{"attributes": {"POSITION": 0}}]}],
                              accessors=[{"count": 6}]))
            self.assertEqual(glb.summarise(p)["triangles"], 2)
            Path(p).write_bytes(b"not a glb at all, really")
            with self.assertRaises(ValueError):
                glb.summarise(p)


WANT = {"triangles": 100, "images": 2, "vertex_colours": True, "extensions_required": [],
        "custom_attributes": ["_PLANTGEN_WIND"]}
GOT = {"error": None, "triangles": 100, "images": 2, "coloured_meshes": 1,
       "material_reads_colour": True, "warnings": [], "attributes": ["_PLANTGEN_WIND"]}


class Judge(unittest.TestCase):
    def test_whole(self):
        self.assertEqual(check_exports.judge(WANT, GOT, "4.5.14 LTS"), ("pass", []))

    def test_each_loss_fails(self):
        for change in ({"triangles": 99}, {"images": 1}, {"coloured_meshes": 0},
                       {"material_reads_colour": False}, {"error": "boom"},
                       {"warnings": ["Extension KHR_x is not available"]}):
            verdict, why = check_exports.judge(WANT, dict(GOT, **change), "4.5.14 LTS")
            self.assertEqual(verdict, "fail", change)
            self.assertTrue(why)
        self.assertEqual(check_exports.judge(WANT, None, "4.5.14")[0], "fail")

    def test_meshopt_refusal_is_accepted_only_before_5_2(self):
        want = dict(WANT, extensions_required=["EXT_meshopt_compression"])
        got = dict(GOT, error="Extension EXT_meshopt_compression is not available on this addon version")
        self.assertEqual(check_exports.judge(want, got, "4.5.14 LTS")[0], "refused")
        self.assertEqual(check_exports.judge(want, got, "5.2.3")[0], "fail")
        self.assertEqual(check_exports.judge(WANT, got, "4.5.14")[0], "fail")

    def test_attributes_are_reported(self):
        self.assertEqual(check_exports.kept_attributes(WANT, dict(GOT, attributes=[])),
                         {"kept": [], "dropped": ["_PLANTGEN_WIND"]})


FAKE = """#!{py}
# Stands in for Blender: reports each file as glb.py reads it.
import json, sys
sys.path.insert(0, {here!r})
import glb
print("PLANTGEN_BLENDER " + json.dumps({{"blender": "4.5.14 LTS"}}))
for f in sys.argv[sys.argv.index("--") + 1:]:
    s = glb.summarise(f)
    print("PLANTGEN_BLENDER " + json.dumps({{"file": f, "error": None, "warnings": [], "seconds": 0,
        "triangles": s["triangles"] - ("short" in f), "mesh_objects": s["mesh_objects"],
        "images": s["images"], "coloured_meshes": 0, "material_reads_colour": False, "attributes": []}}))
"""


class RunBlender(unittest.TestCase):
    def test_results_come_back_per_file_across_jobs(self):
        with tempfile.TemporaryDirectory() as d:
            fake = Path(d) / "blender"
            fake.write_text(FAKE.format(py=sys.executable, here=str(Path(glb.__file__).parent)))
            fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
            files = []
            for name in ("a", "b", "short", "c"):
                p = Path(d) / f"{name}.glb"
                write_glb(p, gltf(nodes=[{"mesh": 0}], meshes=[{"primitives": [{"attributes": {"POSITION": 0}}]}],
                                  accessors=[{"count": 30}]))
                files.append(p)
            version, results = check_exports.run_blender(str(fake), files, 3, 60)
            self.assertEqual(version, "4.5.14 LTS")
            self.assertEqual(sorted(results), sorted(str(f) for f in files))
            verdicts = {Path(k).stem: check_exports.judge(glb.summarise(k), v, version)[0] for k, v in results.items()}
            self.assertEqual(verdicts, {"a": "pass", "b": "pass", "short": "fail", "c": "pass"})


if __name__ == "__main__":
    unittest.main()
