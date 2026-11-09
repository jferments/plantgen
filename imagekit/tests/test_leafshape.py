"""`imagekit leaf-outline`: the measures, the photo pass and the judge against a stand-in plantc."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import stat
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from imagekit.store import Store, jsonl

HAVE = all(importlib.util.find_spec(m) is not None for m in ("numpy", "scipy"))
if HAVE:
    import test_regions


def blob(w=300, h=300, lobes=0, stalk=False):
    """A leaf mask: an ellipse 200 x 100 px, notched at its base into `lobes` hollows, with a stalk."""
    import numpy as np
    from PIL import Image, ImageDraw
    im = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(im)
    d.ellipse((50, 100, 250, 200), fill=255)
    for i in range(lobes):
        x = 50 + 200 * (i + 1) / (lobes + 1)
        d.polygon([(x - 12, 100), (x + 12, 100), (x, 150)], fill=0)
    if stalk:
        d.line([(250, 150), (295, 150)], fill=255, width=3)
    return np.asarray(im) > 127


@unittest.skipUnless(HAVE, "numpy and scipy not installed")
class Measures(unittest.TestCase):
    def test_an_ellipse(self):
        from imagekit.leafshape import measures
        m = measures(blob())
        self.assertAlmostEqual(m["elongation"], 0.5, delta=0.02)
        self.assertGreater(m["solidity"], 0.97)
        self.assertEqual(m["notches"], 0)
        self.assertAlmostEqual(m["fill"], 0.5, delta=0.03)  # half the disc on its 200 px chord

    def test_a_stalk_is_cut_off(self):
        from imagekit.leafshape import measures
        bare, stalked = measures(blob()), measures(blob(stalk=True))
        for k in ("elongation", "solidity", "fill"):
            self.assertAlmostEqual(bare[k], stalked[k], delta=0.02, msg=k)

    def test_hollows_count_as_notches(self):
        from imagekit.leafshape import measures
        m = measures(blob(lobes=2))
        self.assertEqual(m["notches"], 2)
        self.assertLess(m["solidity"], 0.97)

    def test_photo_filter(self):
        import numpy as np
        from imagekit.leafshape import photo_measures
        self.assertTrue(photo_measures(blob())["kept"])
        edge = blob()
        edge[:, :60] = True
        self.assertEqual(photo_measures(edge)["why"], "cut by the frame")
        two = blob()
        two[10:60, 10:60] = True
        self.assertEqual(photo_measures(two)["why"], "in pieces")
        self.assertEqual(photo_measures(np.zeros((300, 300), bool))["why"], "under 2% of the image")


FAKE_PLANTC = """#!{python}
import json, sys
from PIL import Image, ImageDraw
args = sys.argv[1:]
if args[0] == "spec":
    print("  appearance.organs.leaf.shape.template = \\"simple\\"  (spec.json)")
    sys.exit(0)
assert args[0] == "atlas", args
organ, out = args[args.index("--organ") + 1], args[args.index("--out") + 1]
if organ != "leaf" or args[1].endswith(".json"):
    sys.exit(1)  # no basal card; the leaf stays one piece
im = Image.new("RGB", (320, 240), (203, 203, 203))
ImageDraw.Draw(im).rectangle((80, 60, 239, 179), fill=(40, 120, 40))  # the photos' leaf: 160 x 120
im.save(out)
"""


@unittest.skipUnless(HAVE, "numpy and scipy not installed")
class Photos(unittest.TestCase):
    if HAVE:  # test_regions.Detect's store, not its tests
        setUp, tearDown, cli = test_regions.Detect.setUp, test_regions.Detect.tearDown, test_regions.Detect.cli

    def leaf_masks(self):
        self.cli("detect", "--model", "fake", "--organs", "plant,leaf", "--run", "det")
        self.cli("segment", "--model", "fake", "--organ", "leaf", "--triage", "none", "--run", "seg")

    def test_photos_measured_once(self):
        self.leaf_masks()
        first = json.loads(self.cli("leaf-outline", "photos", "--run", "lo"))
        self.assertGreaterEqual(first["kept"], 1)
        again = json.loads(self.cli("leaf-outline", "photos", "--run", "lo2"))
        self.assertEqual(again["measured"], 0)
        rows = list(jsonl(Path(self.store_dir) / "sources/image-analyses/lo/analyses.jsonl"))
        a = next(r for r in rows if r["sha256"] == self.sha["a.jpg"])
        self.assertEqual(a["task"], "leaf-outline")
        self.assertTrue(a["out"]["kept"])
        self.assertAlmostEqual(a["out"]["elongation"], 0.75, delta=0.03)  # a's leaf is 160 x 120 px

    def test_judge_scores_a_card_against_its_photos(self):
        from imagekit import leafshape
        self.leaf_masks()
        self.cli("leaf-outline", "photos", "--run", "lo")
        fake = self.tmp / "plantc"
        fake.write_text(FAKE_PLANTC.format(python=sys.executable))
        fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
        lib = self.tmp / "library" / "annonaceae" / "asimina" / "asimina-triloba"
        lib.mkdir(parents=True)
        (lib / "spec.json").write_text(json.dumps({"taxon": {"scientific_name": "Asimina triloba Dunal"}}))
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            res = leafshape.judge(store, ["asimina-triloba"], str(fake), str(self.tmp / "library"), least=1)
        self.assertEqual([(r["id"], r["organ"], r["part"]) for r in res["drawn"]],
                         [("asimina-triloba", "leaf", "leaf")])
        top = res["scores"][0]
        self.assertEqual((top["id"], top["part"]), ("asimina-triloba", "leaf"))
        self.assertGreaterEqual(top["photos"], 1)
        self.assertLess(abs(top["elongation_z"]), 2)  # the card is drawn as the photographed leaf
        out = self.tmp / "judge.csv"
        self.cli("leaf-outline", "judge", "asimina-triloba", "--plantc", str(fake), "--library",
                 str(self.tmp / "library"), "--min", "1", "--out", str(out))
        self.assertTrue(out.read_text().startswith("id,organ,part,photos,distance,solidity_photo"))


if __name__ == "__main__":
    unittest.main()
