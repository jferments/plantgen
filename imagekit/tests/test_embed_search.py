"""imagekit embed and search, with the fake embedders (numpy and Pillow; no torch, no network)."""
from __future__ import annotations

import argparse
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

from imagekit import analyzer, cli  # noqa: E402
from imagekit.store import Store, jsonl  # noqa: E402

HAVE_NUMPY = importlib.util.find_spec("numpy") is not None
SCHEMAS = Path(__file__).resolve().parents[1] / "schemas"
COLOURS = {"red": (230, 30, 30), "white": (245, 245, 245), "green": (30, 190, 50), "blue": (30, 50, 220)}
# folder (taxon) -> the colour its photos lean to, and how many
TAXA = {"Rosa_rubra": ("red", 3), "Achillea_millefolium": ("white", 3), "Achillea_alba": ("white", 2),
        "Polystichum_munitum": ("green", 3), "Camassia_quamash": ("blue", 2)}


def picture(seed: int, colour) -> bytes:
    """Random blocks (distinct look per seed, so none merge as duplicates) washed with one colour."""
    rng = random.Random(seed)
    small = Image.new("RGB", (8, 6))
    small.putdata([tuple(rng.randrange(256) for _ in range(3)) for _ in range(48)])
    im = Image.blend(small.resize((320, 240), Image.Resampling.BICUBIC), Image.new("RGB", (320, 240), colour), 0.75)
    out = io.BytesIO()
    im.save(out, "JPEG", quality=92)
    return out.getvalue()


@unittest.skipUnless(HAVE_NUMPY, "numpy not installed")
class EmbedSearch(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.store_dir = str(self.tmp / "store")
        photos = self.tmp / "photos"
        seed = 100
        for folder, (colour, n) in TAXA.items():
            (photos / folder).mkdir(parents=True)
            for i in range(n):
                seed += 1
                (photos / folder / f"{i}.jpg").write_bytes(picture(seed, COLOURS[colour]))
        with contextlib.redirect_stderr(io.StringIO()):
            cli.main(["--store", self.store_dir, "fetch", "folder", str(photos), "--licence", "cc-by", "--creator",
                      "Jay", "--taxon-from-folders", "--batch", "own"])
        self.n_images = sum(n for _, n in TAXA.values())

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def run_cmd(self, *argv) -> str:
        """Run embed or search through their own register(), as cli.main would with them in COMMAND_MODULES."""
        from imagekit import embed, search
        ap = argparse.ArgumentParser()
        sub = ap.add_subparsers(dest="cmd", required=True)
        embed.register(sub)
        search.register(sub)
        args = ap.parse_args(list(argv))
        out = io.StringIO()
        with Store(self.store_dir) as store, contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            args.func(args, store)
        return out.getvalue()

    def search(self, models, **kw):
        from imagekit import search
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            return search.search(store, models, **kw)

    def taxon_of(self):
        return {r["sha256"]: r["taxon"]["name_as_given"] for r in jsonl(Path(self.store_dir) / "sources/images/own/images.jsonl")}

    def test_embeds_only_missing_and_writes_valid_rows(self):
        import numpy as np
        from imagekit import embed, vectors
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            first = embed.embed(store, "fake", run="r1", limit=5, part_rows=3)
            self.assertEqual(first["embedded"], 5)
            second = embed.embed(store, "fake", run="r1")
            self.assertEqual(second["embedded"], self.n_images - 5, "only the images still missing")
            third = embed.embed(store, "fake", run="r2")
            self.assertEqual(third["embedded"], 0, "a rerun embeds nothing")
            self.assertFalse(store.run_dir("r2").exists(), "nothing to do writes no run")
            aid = analyzer.analyzer_id(vectors.definition("fake", "fake-1"))
            self.assertEqual(store.missing(aid, store.all_images()), [])
        run = Path(self.store_dir) / "sources/image-analyses/r1"
        parts = sorted((run / "vectors/fake").glob("part-*.npy"))
        self.assertEqual([p.name for p in parts], ["part-0000.npy", "part-0001.npy", "part-0002.npy"])
        rows = list(jsonl(run / "analyses.jsonl"))
        self.assertEqual(len(rows), self.n_images)
        self.assertEqual(len({r["sha256"] for r in rows}), self.n_images)
        for r in rows:  # each row points at its own vector
            arr = np.load(run / r["out"]["part"])
            ids = list(jsonl(run / r["out"]["part"].replace(".npy", ".ids.jsonl")))
            self.assertEqual(arr.dtype, np.float16)
            self.assertEqual(arr.shape[1], r["out"]["dim"])
            self.assertEqual(ids[r["out"]["row"]], {"sha256": r["sha256"], "region": 0})
            self.assertAlmostEqual(float(np.linalg.norm(arr[r["out"]["row"]].astype(np.float32))), 1.0, places=2)
        info = json.loads((run / "run.json").read_text())
        self.assertEqual(info["models"]["fake"]["rows"], self.n_images)
        self.assertEqual(info["models"]["fake"]["revision"], "fake-1")
        self.assertEqual(set(info["models"]["fake"]["parts"]), {p.name for p in parts})
        defs = list(jsonl(Path(self.store_dir) / "analyzers.jsonl"))
        self.assertEqual(defs[0]["params"], {"image_px": 4, "region_kind": "whole", "pooling": "mean", "normalized": True})
        try:
            import jsonschema
        except ImportError:
            return
        for r in rows:
            jsonschema.validate(r, json.loads((SCHEMAS / "analysis.schema.json").read_text()))
        for r in defs:
            jsonschema.validate(r, json.loads((SCHEMAS / "analyzer.schema.json").read_text()))

    def test_a_new_image_size_is_a_new_analyzer(self):
        from imagekit import embed, vectors
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            embed.embed(store, "fake", run="r1")
            self.assertEqual(embed.embed(store, "fake", run="r1", image_px=2)["embedded"], self.n_images)
            vs = vectors.load(store, "fake")  # same count under both: the first analyzer listed wins, never a mix
            self.assertEqual(len(vs), self.n_images)
            self.assertEqual(len(vs.others), 1)

    def test_search_by_text_and_by_image(self):
        self.run_cmd("embed", "--model", "fake", "--run", "r1")
        taxon = self.taxon_of()
        res = self.search(["fake"], text="red", k=3)
        self.assertEqual([taxon[i["sha256"]] for i in res["images"]], ["Rosa rubra"] * 3)
        self.assertGreater(res["images"][0]["score"], res["images"][-1]["score"] - 1e-9)
        out = self.run_cmd("search", "--model", "fake", "--text", "green leaves", "--k", "2")
        self.assertEqual(out.count("Polystichum munitum"), 2)
        self.assertIn("cc-by", out)
        # a photo from the store finds its own species and leaves itself out
        with Store(self.store_dir) as store:
            blue = next(s for s, t in taxon.items() if t == "Camassia quamash")
            path = str(store.blob(blue))
        from imagekit import vectors
        saved = vectors.load_embedder
        vectors.load_embedder = lambda *a, **k: self.fail("a stored image is searched with its stored vector")
        try:
            res = self.search(["fake"], image=path, k=1)
        finally:
            vectors.load_embedder = saved
        self.assertEqual(taxon[res["images"][0]["sha256"]], "Camassia quamash")
        self.assertNotEqual(res["images"][0]["sha256"], blue)
        self.assertIn("the query photo itself is in the store and is left out", res["notes"])
        # a box: the crop is embedded with the model
        res = self.search(["fake"], image=path, box=[0.0, 0.0, 0.5, 0.5], k=1)
        self.assertEqual(taxon[res["images"][0]["sha256"]], "Camassia quamash")

    def test_text_needs_a_text_model(self):
        self.run_cmd("embed", "--model", "fake-gray", "--run", "r1")
        with self.assertRaises(SystemExit) as cm:
            self.search(["fake-gray"], text="red")
        self.assertIn("no text encoder", str(cm.exception))
        with self.assertRaises(SystemExit) as cm:
            self.search(["fake"], text="red")
        self.assertIn("imagekit embed --model fake", str(cm.exception))

    def test_by_species_and_exclude_genus(self):
        self.run_cmd("embed", "--model", "fake", "--run", "r1")
        res = self.search(["fake"], text="white", by_species=True, k=10)
        sp = res["species"]
        self.assertEqual({s["taxon"] for s in sp[:2]}, {"Achillea millefolium", "Achillea alba"})
        self.assertEqual(len(sp), len(TAXA))
        top = sp[0]
        self.assertLessEqual(len(top["images"]), 3)
        self.assertAlmostEqual(top["score"], sum(i["score"] for i in top["images"]) / len(top["images"]))
        self.assertEqual({s["n"] for s in sp if s["taxon"] == "Achillea alba"}, {2})
        res = self.search(["fake"], text="white", by_species=True, exclude_genus=["Achillea"], k=10)
        self.assertNotIn("Achillea", " ".join(s["taxon"] for s in res["species"]))
        self.assertEqual(len(res["species"]), len(TAXA) - 2)

    def test_rank_fusion(self):
        from imagekit import search, vectors
        self.run_cmd("embed", "--model", "fake,fake-gray", "--run", "r1")
        with Store(self.store_dir) as store:
            shas = store.all_images()
            path = str(store.blob(shas[0]))
        res = self.search(["fake", "fake-gray"], image=path, k=self.n_images)
        self.assertTrue(res["fused"])
        self.assertEqual(len(res["images"]), self.n_images - 1)
        # recompute: each image's fused score is the sum over models of 1/(60 + its rank in that model)
        with Store(self.store_dir) as store:
            expect = {}
            for name in ("fake", "fake-gray"):
                vs = vectors.load(store, name)
                q = vs.vector(shas[0])
                s = vs.scores(q)
                order = sorted((i for i in range(len(vs)) if vs.ids[i][0] != shas[0]), key=lambda i: -s[i])
                for rank, i in enumerate(order, 1):
                    expect[vs.ids[i][0]] = expect.get(vs.ids[i][0], 0) + 1 / (search.RRF_K + rank)
        for im in res["images"]:
            self.assertAlmostEqual(im["score"], expect[im["sha256"]], places=9)
            self.assertEqual(set(im["cosine"]), {"fake", "fake-gray"})
        best = max(expect, key=expect.get)
        self.assertEqual(res["images"][0]["sha256"], best)

    def test_reindex_keeps_embed_analyses(self):
        from imagekit import embed, vectors
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            embed.embed(store, "fake", run="r1")
            before = store.stats()
        (Path(self.store_dir) / "index.sqlite").unlink()
        with Store(self.store_dir) as store, contextlib.redirect_stderr(io.StringIO()):
            store.reindex()
            self.assertEqual(store.stats(), before)
            self.assertEqual(before["analyses"], self.n_images)
            self.assertEqual(embed.embed(store, "fake", run="r1")["embedded"], 0)
            self.assertEqual(len(vectors.load(store, "fake")), self.n_images)

    def test_html_contact_sheet(self):
        self.run_cmd("embed", "--model", "fake", "--run", "r1")
        out = self.tmp / "sheet.html"
        self.run_cmd("search", "--model", "fake", "--text", "white", "--by-species", "--k", "2", "--html", str(out))
        page = out.read_text()
        self.assertEqual(page.count("data:image/jpeg;base64,"), 5)  # 3 + 2 Achillea photos
        self.assertIn("Jay, cc-by", page)
        self.assertIn("<i>Achillea alba</i>", page)


if __name__ == "__main__":
    unittest.main()
