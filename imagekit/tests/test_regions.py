"""imagekit detect, crops, and organ vectors, with the fake detector and embedder (Pillow and numpy; no torch)."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import random
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from imagekit import analyzer, cli, regions  # noqa: E402
from imagekit.store import Store, is_render, jsonl  # noqa: E402

HAVE_NUMPY = importlib.util.find_spec("numpy") is not None


def photo(seed: int, leaf_box=None) -> bytes:
    """Grey-brown noise, with a green 'leaf' rectangle at leaf_box (fractions) when given."""
    rng = random.Random(seed)
    small = Image.new("RGB", (8, 6))
    small.putdata([(120 + rng.randrange(40), 100 + rng.randrange(30), 90 + rng.randrange(30)) for _ in range(48)])
    im = small.resize((320, 240), Image.Resampling.BICUBIC)
    if leaf_box:
        x0, y0, x1, y1 = leaf_box
        im.paste((40, 200, 50), (int(x0 * 320), int(y0 * 240), int(x1 * 320), int(y1 * 240)))
    out = io.BytesIO()
    im.save(out, "JPEG", quality=92)
    return out.getvalue()


def analyzer_params(store, aid: str) -> dict:
    return next(r for r in jsonl(store.root / "analyzers.jsonl") if r["id"] == aid)["params"]


class RulesVersion(unittest.TestCase):
    def setUp(self):
        from imagekit.prompts import Rules
        self.tmp = Path(tempfile.mkdtemp())
        shutil.copytree(Rules().path, self.tmp / "photo-rules")
        self.path = self.tmp / "photo-rules"

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def test_a_new_task_file_keeps_other_tasks_analyzers(self):
        from imagekit.prompts import Rules
        before = Rules(self.path)
        (self.path / "09-new-task.md").write_text("# A task no prompt reads yet\n")
        after = Rules(self.path)
        self.assertNotEqual(before.version, after.version)
        for task in ("triage", "observe", "describe"):
            self.assertEqual(before.analyzer_version(task), after.analyzer_version(task))
        triage, observe = after.analyzer_version("triage"), after.analyzer_version("observe")
        with open(self.path / "01-triage.md", "a") as fh:
            fh.write("\nOne more line.\n")
        changed = Rules(self.path)
        self.assertNotEqual(changed.analyzer_version("triage"), triage)
        self.assertEqual(changed.analyzer_version("observe"), observe)

    def test_rules_v2_answers_keep_their_analyzer(self):
        from imagekit.prompts import LEGACY_VERSIONS, Rules
        r = Rules(self.path)
        if r.version != "b9470f22326e2d05":
            self.skipTest("photo-rules has changed since v2")
        for task in ("triage", "observe", "describe"):
            self.assertIn((task, r.task_version(task)), LEGACY_VERSIONS)
            self.assertEqual(r.analyzer_version(task), "b9470f22326e2d05")


class Labels(unittest.TestCase):
    def test_run_together_labels(self):
        f = lambda label: regions.organ_of_label(label, regions.PHRASES)  # noqa: E731
        self.assertEqual(f("tree"), "plant")
        self.assertEqual(f("tree shrub plant"), "plant")  # Grounding DINO joins neighbouring phrases
        self.assertEqual(f("tree trunk"), "bark")
        self.assertEqual(f("flower cluster"), "inflorescence")
        self.assertIsNone(f("leaf flower"))  # two organs: neither
        self.assertIsNone(f("cone fruit"))

    def test_tidy_merges_overlaps_and_drops_small_boxes(self):
        defn = regions.definition("fake", ["plant", "leaf"])
        found = [("leaf", [0.1, 0.1, 0.5, 0.5], 0.6), ("leaf", [0.12, 0.1, 0.5, 0.52], 0.5),  # one leaf, twice
                 ("leaf", [0.6, 0.6, 0.62, 0.62], 0.9),  # too small to read
                 ("leaf", [0.55, 0.1, 0.9, 0.4], 0.4), ("plant", [0, 0, 1, 1], 0.7)]
        out = regions.tidy(found, defn)
        self.assertEqual([(r["region"], r["organ"], r["score"]) for r in out],
                         [(1, "plant", 0.7), (2, "leaf", 0.6), (3, "leaf", 0.4)])

    def test_label_rule_is_in_the_analyzer(self):
        a = regions.definition("gdino-tiny", ["leaf"])
        self.assertEqual(a["params"]["labels"], regions.LABEL_RULE)
        self.assertNotEqual(analyzer.analyzer_id(a), analyzer.analyzer_id(regions.definition("gdino-tiny", ["plant"])))

    def test_replay_renders_count_as_renders(self):
        self.assertTrue(is_render({"source": {"source": "replay", "creator": "PlantGen render asimina-side-s2"}}))
        self.assertTrue(is_render({"source": {"source": "render", "render": {"view": "side"}}}))
        self.assertFalse(is_render({"source": {"source": "inat", "creator": "someone"}}))


class Detect(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.store_dir = str(self.tmp / "store")
        photos = self.tmp / "photos" / "Asimina_triloba"
        photos.mkdir(parents=True)
        self.boxes = {"a.jpg": (0.25, 0.25, 0.75, 0.75), "b.jpg": (0.0, 0.5, 0.5, 1.0), "c.jpg": None, "d.jpg": (0.5, 0, 1, 0.5)}
        for i, (name, box) in enumerate(self.boxes.items()):
            (photos / name).write_bytes(photo(i, box))
        with contextlib.redirect_stderr(io.StringIO()):
            cli.main(["--store", self.store_dir, "fetch", "folder", str(self.tmp / "photos"), "--licence", "cc-by",
                      "--creator", "Jay", "--taxon-from-folders", "--batch", "own"])
        rows = list(jsonl(Path(self.store_dir) / "sources/images/own/images.jsonl"))
        self.sha = {Path(r["source"]["name"]).name: r["sha256"] for r in rows}
        # a triage: d is unusable, b shows a whole plant from the side but names no leaf, a and c show leaves
        tri = {"task": "triage", "model": "test", "model_revision": "1"}
        answers = {"a.jpg": {"usable": True, "views": ["V4"], "organs_visible": ["leaf"]},
                   "b.jpg": {"usable": True, "views": ["V1"], "organs_visible": ["stem"]},
                   "c.jpg": {"usable": True, "views": ["V4"], "organs_visible": ["leaf"]},
                   "d.jpg": {"usable": False, "views": [], "organs_visible": ["leaf"]}}
        (self.tmp / "tri.jsonl").write_text("".join(
            json.dumps({"sha256": self.sha[k], "analyzer": tri, "out": v}) + "\n" for k, v in answers.items()))
        with contextlib.redirect_stdout(io.StringIO()):
            cli.main(["--store", self.store_dir, "import-analyses", str(self.tmp / "tri.jsonl"), "--run", "tri"])

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def cli(self, *argv) -> str:
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            cli.main(["--store", self.store_dir, *argv])
        return out.getvalue()

    def detections(self) -> dict:
        rows = list(jsonl(Path(self.store_dir) / "sources/image-analyses/det/analyses.jsonl"))
        return {r["sha256"]: r for r in rows}

    def test_detects_usable_photos_once_and_stores_fractions(self):
        first = json.loads(self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det"))
        self.assertEqual(first["detected"], 3)  # d is unusable
        rows = self.detections()
        self.assertNotIn(self.sha["d.jpg"], rows)
        a = rows[self.sha["a.jpg"]]
        self.assertEqual((a["task"], a["region"], a["out"]["image"]), ("detect", 0, [320, 240]))
        leaf = next(r for r in a["out"]["regions"] if r["organ"] == "leaf")
        for got, want in zip(leaf["box"], self.boxes["a.jpg"]):
            self.assertAlmostEqual(got, want, delta=0.04)
        self.assertEqual([r["organ"] for r in rows[self.sha["c.jpg"]]["out"]["regions"]], ["plant"])
        again = json.loads(self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det2"))
        self.assertEqual(again["detected"], 0)

    def test_needs_a_triage_or_all(self):
        with Store(self.store_dir) as store:
            store.db.execute("delete from analyses where task='triage'")
            store.commit()
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                regions.detect(store, "fake", ["leaf"], triage=None)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(regions.detect(store, "fake", ["leaf"], all_images=True, run="all")["detected"], 4)

    def test_crops_keep_what_triage_saw(self):
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        out = self.tmp / "crops"
        res = json.loads(self.cli("crops", "--out", str(out)))
        lines = [json.loads(x) for x in (out / "crops.jsonl").read_text().splitlines()]
        got = {(Path(next(k for k, v in self.sha.items() if v == x["sha256"])).name, x["organ"]) for x in lines}
        # a: its leaf (no plant: not a whole view); b: the plant (whole view), not the leaf (triage saw none)
        self.assertEqual(got, {("a.jpg", "leaf"), ("b.jpg", "plant")})
        self.assertEqual(res["crops"], 2)
        leaf = next(x for x in lines if x["organ"] == "leaf")
        w = Image.open(out / leaf["file"]).width
        self.assertAlmostEqual(w / 320, 0.5 * 1.08, delta=0.08)  # the box plus its margin
        everything = json.loads(self.cli("crops", "--out", str(self.tmp / "all"), "--triage", "none"))
        self.assertEqual(everything["crops"], 5)

    @unittest.skipUnless(HAVE_NUMPY, "numpy not installed")
    def test_segment_masks_kept_regions_once(self):
        import numpy as np
        from imagekit import segment
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        first = json.loads(self.cli("segment", "--model", "fake", "--organ", "plant", "--organ", "leaf",
                                    "--run", "seg"))
        self.assertEqual(first["segmented"], 2)  # a's leaf, b's plant (whole view); c's plant is not kept
        again = json.loads(self.cli("segment", "--model", "fake", "--organ", "plant", "--organ", "leaf",
                                    "--run", "seg2"))
        self.assertEqual(again["segmented"], 0)
        with Store(self.store_dir) as store:
            got = segment.masks(store, first["analyzer"])
            path, out = got[(self.sha["a.jpg"], 2)]
            self.assertEqual(out["organ"], "leaf")
            m = np.asarray(Image.open(path).convert("L")) > 0
            self.assertEqual(m.shape, (240, 320))
            self.assertAlmostEqual(out["area"], 0.25, delta=0.03)  # the green square is a quarter of the image
            self.assertGreater(out["fill"], 0.9)
            for got_v, want in zip(out["box"], self.boxes["a.jpg"]):
                self.assertAlmostEqual(got_v, want, delta=0.02)
            params = analyzer_params(store, first["analyzer"])
            self.assertEqual(params["regions"]["organs"], ["leaf", "plant"])

    @unittest.skipUnless(HAVE_NUMPY, "numpy not installed")
    def test_organ_vectors_stay_apart_from_whole_images(self):
        from imagekit import embed, search, vectors
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            embed.embed(store, "fake", run="whole")
            spec = embed.region_spec(store, store.all_images())
            first = embed.embed(store, "fake", regions=spec)
            second = embed.embed(store, "fake", regions=spec)
            self.assertEqual((first["embedded"], second["embedded"]), (2, 0))
            self.assertTrue(first["run"].startswith("embed-fake-organs-"))
            organ = vectors.load(store, "fake", region_kind="organ")
            whole = vectors.load(store, "fake")
            self.assertEqual(sorted(r for _, r in organ.ids), [1, 2])
            self.assertEqual(len(whole), 4)
            self.assertEqual({r for _, r in whole.ids}, {0})
            self.assertEqual(organ.definition["params"]["regions"]["detector"], spec["detector"])
            q = self.tmp / "q.jpg"
            q.write_bytes(photo(99, (0, 0, 1, 1)))
            res = search.search(store, ["fake"], image=str(q), organ="leaf", k=5)
            self.assertEqual([(i["sha256"], i["organ"]) for i in res["images"]], [(self.sha["a.jpg"], "leaf")])
            self.assertEqual(len(res["images"][0]["box"]), 4)


if __name__ == "__main__":
    unittest.main()
