"""imagekit compare and render tests: verdicts on made-up answers, and renders from a fake plantc; no network."""
from __future__ import annotations

import io
import json
import os
import stat
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from PIL import Image  # noqa: E402

from imagekit import cli, compare, render  # noqa: E402
from imagekit.prompts import Rules  # noqa: E402
from imagekit.store import Store, jsonl  # noqa: E402

FEATS = {f["id"]: f for f in Rules().features}


def photo(i, answers, who=None, vote=None, date="2025-05-10", **triage):
    t = {"usable": True, "setting": "open_grown", "leaf_state": "full", "organs_visible": [], **triage}
    return {"id": f"p{i}", "kind": "photo", "who": who or f"person{i}", "vote": vote or f"obs{i}", "date": date,
            "lat": 40.0, "render": None, "triage": t, "answers": answers}


def rendered(i, answers, view="side", day=130, **triage):
    t = {"usable": True, "leaf_state": "full", "organs_visible": [], **triage}
    return {"id": f"r{i}", "kind": "render", "who": "render", "vote": f"r{i}", "date": None, "lat": None,
            "render": {"species": "x", "seed": i, "age": 20, "day": day, "env": "open", "view": view},
            "triage": t, "answers": answers}


def a(value, confidence="high"):
    return {"value": value, "confidence": confidence, "evidence": "seen"}


def verdicts(items, **kw):
    res = compare.compare_species(items, FEATS, species="x", **kw)
    return {row[0]: row[4] for row in res["rows"]}, res


class CompareTest(unittest.TestCase):
    def test_state_miss_and_match(self):
        items = [photo(i, {"shoot.leaf_arrangement": a("alternate")}) for i in range(6)]
        items += [rendered(i, {"shoot.leaf_arrangement": a("opposite")}) for i in range(3)]
        v, res = verdicts(items)
        self.assertEqual(v["shoot.leaf_arrangement"], "✗")
        self.assertEqual(res["findings"][0]["feature"], "shoot.leaf_arrangement")
        items = items[:6] + [rendered(i, {"shoot.leaf_arrangement": a("alternate")}) for i in range(3)]
        self.assertEqual(verdicts(items)[0]["shoot.leaf_arrangement"], "✓")

    def test_split_photos_are_a_lead_not_a_miss(self):
        states = ["alternate", "alternate", "alternate", "opposite", "opposite", "whorled", "whorled"]
        items = [photo(i, {"shoot.leaf_arrangement": a(s)}) for i, s in enumerate(states)]
        items += [rendered(i, {"shoot.leaf_arrangement": a("spiral")}) for i in range(3)]
        self.assertEqual(verdicts(items)[0]["shoot.leaf_arrangement"], "~")

    def test_one_vote_per_observation(self):
        # ten photos, but only four observations from four people: under five votes, insufficient
        items = [photo(i, {"shoot.leaf_arrangement": a("alternate")}, who=f"person{i % 4}", vote=f"obs{i % 4}")
                 for i in range(10)]
        items += [rendered(i, {"shoot.leaf_arrangement": a("opposite")}) for i in range(3)]
        v, res = verdicts(items)
        self.assertEqual(v["shoot.leaf_arrangement"], "insufficient")
        self.assertEqual(res["photos"]["shoot.leaf_arrangement"]["n"], 4)

    def test_three_render_answers_needed(self):
        items = [photo(i, {"shoot.leaf_arrangement": a("alternate")}) for i in range(6)]
        items += [rendered(i, {"shoot.leaf_arrangement": a("opposite")}) for i in range(2)]
        self.assertEqual(verdicts(items)[0]["shoot.leaf_arrangement"], "render insufficient")

    def test_ratio_and_face_on_cards(self):
        fid = next(f for f, d in FEATS.items() if d.get("face_on") and d["type"] == "ratio")
        items = [photo(i, {fid: a(2.4 + 0.05 * i)}) for i in range(6)]
        tilted = items + [rendered(i, {fid: a(3.0)}) for i in range(3)]
        self.assertEqual(verdicts(tilted)[0][fid], "render insufficient")
        cards = items + [rendered(i, {fid: a(3.0)}, view="atlas") for i in range(3)]
        self.assertEqual(verdicts(cards)[0][fid], "✗")
        cards = items + [rendered(i, {fid: a(2.5)}, view="atlas") for i in range(3)]
        self.assertEqual(verdicts(cards)[0][fid], "✓")

    def test_colour_delta_e(self):
        fid = next(f for f, d in FEATS.items() if d["type"] == "colour" and d["scale"] == "foliage")
        green = {"family": "green", "hex": "#4a7a30", "lighting": "direct sun"}
        items = [photo(i, {fid: a(green)}) for i in range(6)]
        same = items + [rendered(i, {fid: a({**green, "lighting": "render"})}) for i in range(3)]
        self.assertEqual(verdicts(same)[0][fid], "✓")
        red = {"family": "red", "hex": "#a02020", "lighting": "render"}
        self.assertEqual(verdicts(items + [rendered(i, {fid: a(red)}) for i in range(3)])[0][fid], "✗")

    def test_setting_and_leaf_state_filters(self):
        fid = "habit.growth_form"
        items = [photo(i, {fid: a("shrub")}, setting="forest") for i in range(6)]
        items += [rendered(i, {fid: a("decurrent_tree")}) for i in range(3)]
        _, res = verdicts(items)
        self.assertNotIn(fid, res["photos"])  # forest photos never judge an open-grown render's habit
        self.assertEqual(res["filters"]["dropped"]["answers from another setting"], 6)

    def test_sources_turn_a_miss_into_a_conflict(self):
        items = [photo(i, {"shoot.leaf_arrangement": a("alternate")}) for i in range(6)]
        items += [rendered(i, {"shoot.leaf_arrangement": a("opposite")}) for i in range(3)]
        _, res = verdicts(items, facts={"leaf_arrangement": ("opposite", "x/spec.json; a-flora")})
        self.assertEqual(res["findings"][0]["verdict"], "conflict")
        self.assertIn("a-flora", res["findings"][0]["sources_say"])

    def test_organ_missing_on_the_render_day(self):
        items = [photo(i, {}, organs_visible=["leaf", "flower"]) for i in range(6)]
        items += [rendered(i, {}) for i in range(3)]
        v, _ = verdicts(items)
        self.assertEqual(v["presence.flower"], "✗")

    def test_organ_out_of_its_stated_months_is_a_conflict(self):
        items = [photo(i, {}, organs_visible=["leaf", "flower"]) for i in range(6)]
        items += [rendered(i, {}, day=166) for i in range(3)]
        _, res = verdicts(items, facts={"flowering_months": ("4, 5", "x/spec.json; a-flora")})
        self.assertEqual(res["findings"][0]["verdict"], "conflict")

    def test_identical_photo_answers_keep_a_reading_step(self):
        items = [photo(i, {"habit.see_through": a(0.4)}) for i in range(6)]
        items += [rendered(i, {"habit.see_through": a(0.45)}) for i in range(3)]
        self.assertEqual(verdicts(items)[0]["habit.see_through"], "✓")

    def test_profile_bands(self):
        widths = [0.2, 0.3, 0.35, 0.35, 0.3, 0.25, 0.2, 0.15, 0.1, 0.05]
        items = [photo(i, {"habit.crown_profile": a([w + 0.01 * (i % 3) for w in widths])}) for i in range(6)]
        same = items + [rendered(i, {"habit.crown_profile": a(widths)}) for i in range(3)]
        self.assertEqual(verdicts(same)[0]["habit.crown_profile"], "✓")
        wide = [w * 2 for w in widths]
        self.assertEqual(verdicts(items + [rendered(i, {"habit.crown_profile": a(wide)}) for i in range(3)])[0]
                         ["habit.crown_profile"], "✗")


FAKE_PLANTC = r"""#!/usr/bin/env python3
import sys
from PIL import Image
args = sys.argv[1:]
if args[0] == "list":
    print("  fake-plant                               fake (Fakea plantae), program `broadleaf`")
    sys.exit(0)
out = args[args.index("--out") + 1]
if args[0] == "atlas":
    im = Image.new("RGB", (300, 120), (203, 203, 203))
    for x0, c in ((10, (40, 120, 40)), (110, (50, 140, 50)), (210, (200, 200, 60))):
        im.paste(c, (x0, 10, x0 + 80, 110))
    im.save(out)
    sys.exit(0)
seed = int(args[args.index("--seed") + 1])
view = args[args.index("--view") + 1] if "--view" in args else "side"
colour = (30 + 20 * seed, 120 + (10 if view == "under" else 0), 40 + (40 if "--focus" in args else 0))
im = Image.new("RGB", (64, 64), (195, 208, 224))
im.paste(colour, (8, 8, 56, 56))
im.save(out)
print("age  20.0  height  5.00 m  dbh     - cm  crown base  0.07 m  crown radius  2.00 m")
"""


class RenderTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.plantc = self.root / "plantc"
        self.plantc.write_text(FAKE_PLANTC)
        self.plantc.chmod(self.plantc.stat().st_mode | stat.S_IEXEC)

    def tearDown(self):
        self.tmp.cleanup()

    def test_render_batch_records_how_each_image_was_made(self):
        store = self.root / "store"
        with redirect_stderr(io.StringIO()), redirect_stdout(io.StringIO()) as out:
            cli.main(["--store", str(store), "render", "fake-plant", "--age", "20", "--day", "130", "--seeds", "1,2",
                      "--plantc", str(self.plantc), "--batch", "render-test"])
        self.assertEqual(json.loads(out.getvalue())["taxon"], "Fakea plantae")
        rows = list(jsonl(store / "sources/images/render-test/images.jsonl"))
        views = sorted(r["source"]["render"]["view"] for r in rows)
        self.assertEqual(views.count("atlas"), 3)          # the atlas cut into its three cards
        self.assertEqual(views.count("side"), 2)            # one per seed
        for r in rows:
            self.assertEqual(r["source"]["source"], "render")
            self.assertEqual(r["source"]["licence"], "cc0")
            self.assertEqual(r["taxon"]["name_as_given"], "Fakea plantae")
            self.assertIn("command", r["source"]["render"])
        shoot = next(r for r in rows if r["source"]["render"]["view"] == "shoot")
        self.assertEqual(shoot["source"]["render"]["focus_on"], "leaf:0.7")
        self.assertEqual(shoot["source"]["render"]["day"], 130)
        with Store(store) as s:  # a second run renders the same images: nothing new is stored
            before = s.stats()["images"]
        with redirect_stderr(io.StringIO()), redirect_stdout(io.StringIO()):
            cli.main(["--store", str(store), "render", "fake-plant", "--age", "20", "--day", "130", "--seeds", "1,2",
                      "--plantc", str(self.plantc), "--batch", "render-test-2"])
        with Store(store) as s:
            self.assertEqual(s.stats()["images"], before)

    def test_compare_reads_renders_from_the_store(self):
        store = Store(self.root / "s2")
        rows = [{"sha256": f"{i:064x}", "batch": "b", "source": ({"source": "render", "key": f"render:{i}",
                                                                  "render": {"view": "side", "day": 130}}
                                                                 if i >= 6 else
                                                                 {"source": "inat", "key": f"inat:{i}",
                                                                  "observation": i, "creator": f"c{i}"}),
                 "taxon": {"name_as_given": "Fakea plantae"}, "phash": f"{i:016x}", "width": 10, "height": 10,
                 "bytes": 1, "duplicate": None} for i in range(9)]
        d = store.batch_dir("b")
        d.mkdir(parents=True)
        (d / "images.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
        run = store.run_dir("obs")
        run.mkdir(parents=True)
        lines = []
        for i, r in enumerate(rows):
            value = "opposite" if i >= 6 else "alternate"
            lines.append({"sha256": r["sha256"], "region": 0, "analyzer": "obs1", "task": "observe", "status": "ok",
                          "out": {"photo_id": "x", "description": "", "answers": {"shoot.leaf_arrangement": a(value)}}})
        (run / "analyses.jsonl").write_text("".join(json.dumps(x) + "\n" for x in lines))
        store.reindex()
        out = self.root / "cmp"
        with redirect_stderr(io.StringIO()), redirect_stdout(io.StringIO()):
            compare.cmd_compare(type("A", (), dict(taxon=["Fakea plantae"], observe=None, triage=None, photos=None,
                                                   renders=None, setting="strict", plantc="plantc", no_text=True,
                                                   rules=None, out=str(out)))(), store)
        findings = json.loads((out / "findings.json").read_text())
        self.assertEqual([f["feature"] for f in findings], ["shoot.leaf_arrangement"])
        self.assertIn("| shoot.leaf_arrangement |", (out / "COMPARE.md").read_text())
        store.close()

    def test_observe_takes_the_triage_that_answered_its_images(self):
        from imagekit.run import default_triage
        store = Store(self.root / "s3")
        run = store.run_dir("t")
        run.mkdir(parents=True)
        rows = [{"sha256": f"{i:064x}", "region": 0, "analyzer": "big" if i < 5 else "mine", "task": "triage",
                 "status": "ok", "out": {}} for i in range(7)]
        (run / "analyses.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
        store.reindex()
        self.assertEqual(default_triage(store), "big")
        self.assertEqual(default_triage(store, [f"{5:064x}", f"{6:064x}"]), "mine")
        store.close()

    def test_cut_atlas(self):
        im = Image.new("RGB", (200, 50), render.ATLAS_BG)
        im.paste((40, 120, 40), (10, 5, 60, 45))
        im.paste((40, 120, 40), (120, 5, 190, 45))
        p = self.root / "atlas.png"
        im.save(p)
        self.assertEqual(len(render.cut_atlas(p, self.root)), 2)


if __name__ == "__main__":
    os.environ.setdefault("IMAGEKIT_STORE", tempfile.mkdtemp())
    unittest.main()
