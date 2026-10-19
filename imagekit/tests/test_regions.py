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
        kept = [t for t in ("triage", "observe", "describe") if (t, r.task_version(t)) in LEGACY_VERSIONS]
        if not kept:
            self.skipTest("every task's files have changed since rules v2")
        for task in kept:
            self.assertEqual(r.analyzer_version(task), "b9470f22326e2d05")
        self.assertEqual(r.analyzer_version("branching"), r.task_version("branching"))


class Tally(unittest.TestCase):
    def test_one_vote_per_observation_and_renders_apart(self):
        from imagekit import tally
        def item(kind, vote, **answers):
            return {"kind": kind, "vote": vote, "answers": {k: {"value": v} for k, v in answers.items()}}
        items = [item("photo", "obs:1", leader="lost", limb_angle=60), item("photo", "obs:1", leader="lost", limb_angle=70),
                 item("photo", "obs:2", leader="kept", limb_angle=40), item("photo", "obs:3", leader="lost"),
                 item("photo", "obs:4", leader="cannot tell", limb_angle="cannot tell"),
                 item("render", "r1", leader="kept", limb_angle=30), item("render", "r2", leader="kept", limb_angle=35)]
        res = tally.tally(items, ["leader", "limb_angle"])
        self.assertEqual(res["leader"]["photos"]["states"], {"lost": 2, "kept": 1})
        self.assertEqual((res["leader"]["photos"]["votes"], res["leader"]["photos"]["unknown"]), (3, 1))
        self.assertEqual(res["limb_angle"]["photos"]["median"], 52.5)  # obs:1 is one vote of 65, obs:2 one of 40
        self.assertEqual(res["leader"]["renders"]["states"], {"kept": 2})
        md = tally.markdown("Testus", "abc", res, 5, 2)
        self.assertIn("| leader | lost 2, kept 1; 3 votes thin, unknown 1 | kept 2; 2 votes thin, unknown 0 | no |", md)


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
        tall = self.cli("crops", "--triage", "none", "--organ", "leaf", "--min-height", "0.5", "--list").split()
        self.assertEqual(sorted(tall), sorted([self.sha["a.jpg"], self.sha["b.jpg"]]))  # d's leaf is not usable

    def test_photos_detected_with_all_keep_nothing_the_triage_did_not_pass(self):
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--all", "--run", "det")
        self.assertIn(self.sha["d.jpg"], self.detections())  # --all looked at the unusable photo
        with Store(self.store_dir) as store:  # answers are read from the run file: drop a's triage there
            tri = store.run_dir("tri") / "analyses.jsonl"
            tri.write_text("".join(x for x in tri.read_text().splitlines(True) if self.sha["a.jpg"] not in x))
        listed = self.cli("crops", "--list").split()
        self.assertEqual(listed, [self.sha["b.jpg"]])  # a has no triage now and d is unusable: neither is kept
        everything = self.cli("crops", "--triage", "none", "--list").split()
        self.assertIn(self.sha["d.jpg"], everything)

    @unittest.skipUnless(HAVE_NUMPY and importlib.util.find_spec("pyarrow") is not None, "needs numpy and pyarrow")
    def test_compact_merges_into_a_snapshot(self):
        import pyarrow.parquet as pq
        from imagekit import embed, library
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            embed.embed(store, "fake", run="whole")
            stats = library.compact(store)
            again = library.compact(store)
            snap = library.latest(store)
        self.assertEqual(snap.name, again["snapshot"])
        self.assertNotEqual(stats["snapshot"], again["snapshot"])  # snapshots are written once
        self.assertEqual((stats["images"], stats["photos"]), (4, 4))
        self.assertEqual(stats["answers by task"], {"triage": 4, "detect": 3, "embed": 4})
        tri = pq.read_table(snap / "triage.parquet").to_pylist()
        self.assertEqual(sorted(r["usable"] for r in tri), [False, True, True, True])
        regs = pq.read_table(snap / "regions.parquet").to_pylist()
        self.assertEqual(len(regs), stats["regions"])
        self.assertEqual({r["organ"] for r in regs}, {"plant", "leaf"})
        sp = pq.read_table(snap / "species.parquet").to_pylist()
        self.assertEqual(json.loads(sp[0]["usable_photos_by_view"]), {"V1": 1, "V4": 2})
        ids = pq.read_table(next((snap / "vectors" / "fake").glob("*.ids.parquet"))).to_pylist()
        self.assertEqual(len(ids), 4)

    def test_mixed_columns_become_text(self):
        from imagekit.library import uniform
        rows = uniform([{"id": 3, "x": 1}, {"id": "a3", "x": 2.5}, {"y": True}])
        self.assertEqual([r["id"] for r in rows], ["3", "a3", None])
        self.assertEqual([r["x"] for r in rows], [1, 2.5, None])

    @unittest.skipUnless(HAVE_NUMPY, "numpy not installed")
    def test_count_points_at_parts_of_kept_regions_once(self):
        from imagekit import count
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        first = json.loads(self.cli("count", "--pointer", "fake", "--organ", "leaf", "--what", "the leaves",
                                    "--run", "cnt"))
        self.assertEqual(first["counted"], 1)  # a's leaf; b's triage saw no leaf, d is unusable
        self.assertEqual(json.loads(self.cli("count", "--pointer", "fake", "--organ", "leaf", "--what",
                                             "the leaves", "--run", "cnt2"))["counted"], 0)
        other = json.loads(self.cli("count", "--pointer", "fake", "--organ", "leaf", "--what", "the leaflets",
                                    "--run", "cnt3"))
        self.assertNotEqual(other["analyzer"], first["analyzer"])  # each question is its own analyzer
        with Store(self.store_dir) as store:
            row = next(jsonl(store.run_dir("cnt") / "analyses.jsonl"))
            self.assertEqual((row["sha256"], row["out"]["count"]), (self.sha["a.jpg"], 1))
            self.assertEqual(len(row["out"]["points"]), 1)
            self.assertTrue(all(0 <= v <= 1 for v in row["out"]["crop"]))
            by = count.summary(count.counts(store, first["analyzer"]))
            self.assertEqual(by["Asimina triloba"]["photos"]["median"], 1)
            self.assertIsNone(by["Asimina triloba"]["renders"])

    def test_count_answers(self):
        from imagekit import count
        status, out = count.answer('{"points": [[100, 200], [300, 400]], "count": 3}', "flower", "x", [0, 0, 1, 1])
        self.assertEqual((status, out["count"], out["count_stated"], out["points"][0]), ("ok", 2, 3, [0.1, 0.2]))
        row = json.dumps({"points": [[500, 100 + 50 * i] for i in range(8)], "count": 8})
        status, out = count.answer(row, "flower", "x", [0, 0, 1, 1])
        self.assertTrue(out["degenerate"])
        self.assertIsNone(out["count"])
        self.assertEqual(count.answer('{"points": [], "count": null}', "flower", "x", [0, 0, 1, 1])[0], "empty")
        self.assertEqual(count.answer('{"points": [], "count": 0}', "flower", "x", [0, 0, 1, 1])[1]["count"], 0)
        cut = count.answer('{"points": [[10, 20], [30, 40], [5', "flower", "x", [0, 0, 1, 1])[1]
        self.assertEqual(cut["count"], 2)  # a cut-off answer keeps its whole points

    def test_count_chat_writes_crops_then_reads_answers(self):
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        d = self.tmp / "chat"
        args = ("count", "--backend", "chat", "--model", "observer", "--chat-dir", str(d), "--organ", "leaf",
                "--what", "the leaves", "--run", "cc")
        self.assertEqual(json.loads(self.cli(*args))["waiting"], 1)
        crops = sorted(d.glob("*.jpg"))
        self.assertEqual(len(crops), 1)
        self.assertIn("Point to each of the leaves", (d / "prompt.md").read_text())
        crops[0].with_suffix(".json").write_text('{"points": [[500, 500]], "count": 1}')
        self.assertEqual(json.loads(self.cli(*args))["counted"], 1)
        self.assertEqual(json.loads(self.cli(*args[:-1], "cc2"))["counted"], 0)

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
    def test_measure_runs_plantc_on_each_mask_with_a_cover(self):
        import stat
        from imagekit import measure
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        self.cli("segment", "--model", "fake", "--organ", "leaf", "--run", "seg")
        fake = self.tmp / "plantc"
        fake.write_text(f"""#!{sys.executable}
import json, sys
from PIL import Image
args = sys.argv[1:]
assert args[0] == "measure-mask"
m = Image.open(args[1]).convert("L")
c = Image.open(args[args.index("--cover") + 1]).convert("L") if "--cover" in args else None
print(json.dumps({{"view": args[args.index("--view") + 1], "cover": c is not None, "width": m.width,
                  "cover_size": list(c.size) if c else None}}))
""")
        fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            first = measure.measure(store, str(fake), run="m1")
            again = measure.measure(store, str(fake), run="m2")
            bare = measure.measure(store, str(fake), cover="none", run="m3")
        self.assertEqual((first["measured"], again["measured"], bare["measured"]), (1, 0, 1))
        self.assertNotEqual(first["analyzer"], bare["analyzer"])
        rows = list(jsonl(Path(self.store_dir) / "sources/image-analyses/m1/analyses.jsonl"))
        self.assertEqual((rows[0]["region"], rows[0]["task"]), (2, "measure"))
        self.assertEqual(rows[0]["out"], {"view": "side", "cover": True, "width": 320, "cover_size": [320, 240]})

    def test_sky_rule(self):
        import numpy as np
        from imagekit.measure import sky
        im = Image.new("RGB", (4, 1))
        im.putdata([(195, 208, 224), (240, 240, 238), (60, 140, 50), (120, 110, 90)])  # render sky, haze, leaf, bark
        self.assertEqual(list(np.asarray(sky(im))[0]), [True, True, False, False])

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
