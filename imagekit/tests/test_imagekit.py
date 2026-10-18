"""imagekit tests: no network (net.get_json and net.get_bytes are faked)."""
from __future__ import annotations

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

from imagekit import analyzer, cli, licences, net  # noqa: E402
from imagekit.store import Store, jsonl  # noqa: E402

SCHEMAS = Path(__file__).resolve().parents[1] / "schemas"


def picture(seed: int, size=(800, 600), exif_gps=False) -> bytes:
    """Coarse blocks of random colour: distinct per seed, stable under resizing."""
    rng = random.Random(seed)
    small = Image.new("RGB", (8, 6))
    small.putdata([tuple(rng.randrange(256) for _ in range(3)) for _ in range(48)])
    im = small.resize(size, Image.Resampling.BICUBIC)
    out = io.BytesIO()
    if exif_gps:
        exif = Image.Exif()
        exif[0x8825] = {1: "N", 2: (45.0, 30.0, 0.0)}  # GPSInfo
        exif[0x010F] = "Camera maker"
        im.save(out, "JPEG", quality=90, exif=exif)
    else:
        im.save(out, "JPEG", quality=90)
    return out.getvalue()


def obs(i: int, user: str, licence: str | None, month: int) -> dict:
    return {
        "id": 1000 + i, "observed_on": f"2025-{month:02d}-10", "place_guess": "Somewhere", "location": "45.5,-122.6",
        "positional_accuracy": 20, "quality_grade": "research", "captive": False,
        "user": {"login": user, "name": user.title()}, "taxon": {"id": 48258, "name": "Acer circinatum"},
        "annotations": [{"controlled_attribute_id": 12, "controlled_value_id": 13}],
        "photos": [{"id": 5000 + i, "license_code": licence, "attribution": f"(c) {user}",
                    "url": f"https://inaturalist-open-data.s3.amazonaws.com/photos/{5000 + i}/square.jpg"}],
    }


class FakeNet:
    def __init__(self, observations):
        self.observations = observations
        self.downloads = []

    def get_json(self, url, params=None):
        params = params or {}
        if url.endswith("/histogram"):
            months = {}
            for o in self.observations:
                m = str(int(o["observed_on"][5:7]))
                months[m] = months.get(m, 0) + 1
            return {"results": {"month_of_year": months}}
        res = self.observations
        if "month" in params:
            res = [o for o in res if int(o["observed_on"][5:7]) == int(params["month"])]
        page = int(params.get("page", 1))
        per = int(params.get("per_page", 100))
        return {"results": res[(page - 1) * per: page * per]}

    def get_bytes(self, url, **_):
        self.downloads.append(url)
        return picture(int(url.split("/photos/")[1].split("/")[0]))


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.store_dir = str(self.tmp / "store")
        self._saved = (net.get_json, net.get_bytes)

    def tearDown(self):
        net.get_json, net.get_bytes = self._saved
        shutil.rmtree(self.tmp)

    def run_cli(self, *argv):
        cli.main(["--store", self.store_dir, *argv])

    def rows(self, batch):
        return list(jsonl(Path(self.store_dir) / "sources" / "images" / batch / "images.jsonl"))


class Licences(unittest.TestCase):
    def test_spellings(self):
        self.assertEqual(licences.norm("CC-BY-NC"), "cc-by-nc")
        self.assertEqual(licences.norm("http://creativecommons.org/licenses/by-sa/4.0/"), "cc-by-sa")
        self.assertEqual(licences.norm("https://creativecommons.org/publicdomain/zero/1.0/"), "cc0")
        self.assertEqual(licences.norm("CC BY 4.0"), "cc-by")
        self.assertEqual(licences.norm("Public domain"), "pd")
        self.assertEqual(licences.norm("all rights reserved"), "")
        self.assertEqual(licences.norm(None), "")


class Analyzers(unittest.TestCase):
    D = {"task": "observe", "model": "Qwen/Qwen3-VL-8B-Instruct", "model_revision": "abc123",
         "rules_version": "f1e57d0", "params": {"image_px": 1536, "temperature": 0}}

    def test_id_is_stable_and_order_free(self):
        a = analyzer.analyzer_id(self.D)
        b = analyzer.analyzer_id(dict(reversed(list(self.D.items()))))
        self.assertEqual(a, b)
        self.assertRegex(a, "^[0-9a-f]{16}$")

    def test_any_change_is_a_new_analyzer(self):
        a = analyzer.analyzer_id(self.D)
        self.assertNotEqual(a, analyzer.analyzer_id({**self.D, "model_revision": "abc124"}))
        self.assertNotEqual(a, analyzer.analyzer_id({**self.D, "params": {"image_px": 1024, "temperature": 0}}))
        self.assertEqual(a, analyzer.analyzer_id({**self.D, "note": "notes do not change the id"}))

    def test_bad_definitions(self):
        with self.assertRaises(ValueError):
            analyzer.analyzer_id({"task": "observe", "model": "x"})
        with self.assertRaises(ValueError):
            analyzer.analyzer_id({**self.D, "task": "guess"})
        with self.assertRaises(ValueError):
            analyzer.analyzer_id({**self.D, "seed_of_the_day": 3})


class InatFetch(Base):
    def setUp(self):
        super().setUp()
        users = ["ann", "ann", "ann", "bo", "bo", "cy", "cy", "di", "ed", "fa"]
        lics = ["cc-by", "cc0", "cc-by", "cc-by-nc", "cc-by", "cc-by-sa", None, "cc-by", "cc0", "cc-by"]
        self.fake = FakeNet([obs(i, u, lic, 3 + i % 5) for i, (u, lic) in enumerate(zip(users, lics))])
        net.get_json, net.get_bytes = self.fake.get_json, self.fake.get_bytes

    def test_licences_observer_cap_and_resume(self):
        self.run_cli("fetch", "inat", "--taxon", "Acer circinatum", "--n", "5", "--per-observer", "1", "--batch", "b1")
        rows = self.rows("b1")
        self.assertEqual(len(rows), 5)
        self.assertTrue(all(r["source"]["licence"] in ("cc0", "cc-by", "cc-by-sa") for r in rows))
        observers = [r["source"]["observer"] for r in rows]
        self.assertEqual(len(observers), len(set(observers)))
        self.assertEqual(rows[0]["source"]["annotations"], {"phenology": "flowering"})
        self.assertTrue(rows[0]["source"]["url"].endswith("/large.jpg"))
        n = len(self.fake.downloads)
        self.run_cli("fetch", "inat", "--taxon", "Acer circinatum", "--n", "5", "--per-observer", "1", "--batch", "b1")
        self.assertEqual(len(self.fake.downloads), n, "a rerun downloads nothing it holds")
        self.run_cli("fetch", "inat", "--taxon", "Acer circinatum", "--n", "7", "--per-observer", "2", "--batch", "b2")
        self.assertEqual(len(self.rows("b2")), 2, "a larger --n tops up")
        self.assertEqual(Store(self.store_dir).stats()["images"], 7)

    def test_nc_only_when_asked(self):
        self.run_cli("fetch", "inat", "--taxon", "Acer circinatum", "--n", "9", "--per-observer", "3",
                     "--licences", "cc0,cc-by,cc-by-sa,cc-by-nc", "--batch", "b")
        self.assertIn("cc-by-nc", {r["source"]["licence"] for r in self.rows("b")})
        self.assertNotIn(None, {r["source"]["licence"] for r in self.rows("b")})

    def test_dry_run_downloads_nothing(self):
        self.run_cli("fetch", "inat", "--taxon", "Acer circinatum", "--n", "3", "--dry-run", "--batch", "b")
        self.assertEqual(self.fake.downloads, [])
        self.assertEqual(Store(self.store_dir).stats()["images"], 0)


class InatDump(Base):
    def test_dump_spreads_and_caps(self):
        d = self.tmp / "dump"
        d.mkdir()
        tsv = lambda name, rows: (d / name).write_text("\n".join("\t".join(map(str, r)) for r in rows) + "\n")  # noqa: E731
        tsv("taxa.csv", [("taxon_id", "name", "active"), (48258, "Acer circinatum", "true"), (1, "Other plant", "true")])
        obs_rows = [("observation_uuid", "observer_id", "latitude", "longitude", "positional_accuracy", "taxon_id",
                     "quality_grade", "observed_on")]
        photo_rows = [("photo_uuid", "photo_id", "observation_uuid", "observer_id", "extension", "license", "width",
                       "height", "position")]
        for i in range(12):
            obs_rows.append((f"o{i}", i % 3, 45.0, -122.0, 10, 48258, "research" if i != 11 else "needs_id",
                             f"2024-{1 + i % 6:02d}-01"))
            photo_rows.append((f"p{i}", 7000 + i, f"o{i}", i % 3, "jpg", "CC-BY" if i != 10 else "CC-BY-NC", 2048, 1536, 0))
        tsv("observations.csv", obs_rows)
        tsv("photos.csv", photo_rows)
        tsv("observers.csv", [("observer_id", "login", "name"), (0, "ann", "Ann A"), (1, "bo", ""), (2, "cy", "Cy C")])
        (self.tmp / "names.txt").write_text("Acer circinatum\n")
        man = self.tmp / "m.jsonl"
        self.run_cli("fetch", "inat-dump", "--dump", str(d), "--taxa", str(self.tmp / "names.txt"), "--n", "5",
                     "--per-observer", "2", "--manifest-only", str(man))
        rows = list(jsonl(man))
        self.assertEqual(len(rows), 5)
        self.assertTrue(all(r["licence"] == "cc-by" for r in rows))
        self.assertLessEqual(max(sum(r["observer"] == o for r in rows) for o in ("ann", "bo", "cy")), 2)
        self.assertEqual({r["creator"] for r in rows if r["observer"] == "bo"} - {None}, {"bo"})
        self.assertTrue(rows[0]["url"].startswith("https://inaturalist-open-data.s3.amazonaws.com/photos/"))


class Duplicates(Base):
    def test_same_bytes_and_same_look_are_one_image(self):
        photos = self.tmp / "photos" / "Acer_circinatum"
        photos.mkdir(parents=True)
        (photos / "a.jpg").write_bytes(picture(7))
        (photos / "b.jpg").write_bytes(picture(8))
        self.run_cli("fetch", "folder", str(self.tmp / "photos"), "--licence", "cc-by", "--creator", "Jay",
                     "--taxon-from-folders", "--batch", "own")
        rows = self.rows("own")
        self.assertEqual({r["taxon"]["name_as_given"] for r in rows}, {"Acer circinatum"})
        self.assertEqual(len(rows), 2)
        # the same photo again from elsewhere: once as is, once smaller with GPS in its metadata
        served = {"https://x.org/same.jpg": picture(7),
                  "https://x.org/small.jpg": picture(7, size=(400, 300), exif_gps=True),
                  "https://x.org/other.jpg": picture(9)}
        net.get_bytes = lambda url, **_: served[url]
        man = self.tmp / "m.jsonl"
        man.write_text("".join(json.dumps({"source": "web", "id": n, "url": u, "licence": "CC BY", "taxon": "Acer circinatum"}) + "\n"
                               for n, u in enumerate(served)))
        self.run_cli("fetch", "manifest", str(man), "--batch", "web")
        web = self.rows("web")
        dup = {r["source"]["url"]: r["duplicate"] for r in web}
        self.assertEqual(dup["https://x.org/same.jpg"]["match"], "sha256")
        self.assertEqual(dup["https://x.org/small.jpg"]["match"], "phash")
        self.assertIsNone(dup["https://x.org/other.jpg"])
        own_a = next(r["sha256"] for r in rows if r["source"]["name"].endswith("a.jpg"))
        self.assertEqual(dup["https://x.org/small.jpg"]["of"], own_a)
        st = Store(self.store_dir).stats()
        self.assertEqual((st["images"], st["source references"]), (3, 5))

    def test_metadata_stripped_and_size_capped(self):
        net.get_bytes = lambda url, **_: picture(3, size=(3000, 2000), exif_gps=True)
        man = self.tmp / "m.jsonl"
        man.write_text(json.dumps({"source": "web", "id": 1, "url": "https://x.org/big.jpg", "licence": "cc0",
                                   "taxon": "Acer circinatum"}) + "\n")
        self.run_cli("fetch", "manifest", str(man), "--batch", "w")
        row = self.rows("w")[0]
        self.assertEqual((row["width"], row["height"]), (2048, 1365))
        self.assertEqual(row["stored"], "re-encoded")
        blob = Store(self.store_dir).blob(row["sha256"])
        self.assertEqual(len(Image.open(blob).getexif()), 0)


class Analyses(Base):
    def test_import_missing_and_reindex(self):
        photos = self.tmp / "p"
        photos.mkdir()
        for i in range(4):
            (photos / f"{i}.jpg").write_bytes(picture(20 + i))
        self.run_cli("fetch", "folder", str(photos), "--licence", "cc0", "--creator", "Jay", "--taxon", "Acer circinatum",
                     "--batch", "own")
        store = Store(self.store_dir)
        shas = store.all_images()
        d = {"task": "triage", "model": "claude-haiku-5-5", "model_revision": "claude-haiku-5-5",
             "rules_version": "f1e57d0", "params": {"image_px": 512}}
        rows = [{"sha256": s, "analyzer": d, "out": {"usable": True}, "usd": 0.00011} for s in shas[:3]]
        self.assertEqual(analyzer.import_analyses(store, "pilot", rows), {"added": 3, "skipped": 0})
        self.assertEqual(analyzer.import_analyses(store, "pilot", rows), {"added": 0, "skipped": 3})
        key = store.db.execute("select key from refs where sha256=?", (shas[3],)).fetchone()[0]
        other = {"sha256": "0" * 64, "analyzer": d, "out": {"usable": True}}  # hashed from the original bytes
        self.assertEqual(analyzer.import_analyses(store, "pilot-2", [other]), {"added": 0, "skipped": 1})
        self.assertEqual(analyzer.import_analyses(store, "pilot-2", [{**other, "key": key}]),
                         {"added": 1, "skipped": 0})
        self.assertEqual(store.missing(analyzer.analyzer_id(d), [shas[3]]), [])
        aid = analyzer.analyzer_id(d)
        self.assertEqual(store.missing(aid, shas), [])
        self.assertEqual(len(store.missing(analyzer.analyzer_id({**d, "model": "other"}), shas)), 4)
        before = store.stats()
        store.close()
        (Path(self.store_dir) / "index.sqlite").unlink()
        store = Store(self.store_dir)
        store.reindex()
        self.assertEqual(store.stats(), before)
        self.assertEqual(store.missing(aid, shas), [])


@unittest.skipUnless(__import__("importlib").util.find_spec("jsonschema"), "jsonschema not installed")
class Schemas(Base):
    def test_rows_validate(self):
        import jsonschema
        photos = self.tmp / "p"
        photos.mkdir()
        (photos / "a.jpg").write_bytes(picture(1))
        (photos / "b.jpg").write_bytes(picture(1, size=(400, 300)))  # the same look: a duplicate row
        self.run_cli("fetch", "folder", str(photos), "--licence", "cc-by", "--creator", "Jay", "--taxon", "Acer circinatum",
                     "--batch", "own")
        image_schema = json.loads((SCHEMAS / "image-row.schema.json").read_text())
        for r in self.rows("own"):
            jsonschema.validate(r, image_schema)
        store = Store(self.store_dir)
        d = {"task": "describe", "model": "m", "model_revision": "r", "note": "test"}
        analyzer.import_analyses(store, "r1", [{"sha256": store.all_images()[0], "analyzer": d, "out": {"text": "lacy"}}])
        for r in jsonl(store.run_dir("r1") / "analyses.jsonl"):
            jsonschema.validate(r, json.loads((SCHEMAS / "analysis.schema.json").read_text()))
        for r in jsonl(store.root / "analyzers.jsonl"):
            jsonschema.validate(r, json.loads((SCHEMAS / "analyzer.schema.json").read_text()))


if __name__ == "__main__":
    unittest.main()
