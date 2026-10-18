"""imagekit views tests: triage answers imported into a store, counted against the per-view targets; no network."""
from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from test_imagekit import Base, picture  # noqa: E402

from imagekit import analyzer  # noqa: E402
from imagekit.store import Store, jsonl  # noqa: E402

TRIAGE = {"task": "triage", "model": "vlm-8b", "model_revision": "r1"}
OTHER = {**TRIAGE, "model": "vlm-2b"}


def triage(views, organs=(), usable=True, **extra) -> dict:
    return {"description": "a plant", "usable": usable, "views": list(views), "organs_visible": list(organs),
            "stage": "vegetative", "setting": "open_grown", "age_class": "mature", "lighting": "overcast", **extra}


class Views(Base):
    def quiet(self, *argv) -> str:
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            self.run_cli(*argv)
        return out.getvalue()

    def add_photos(self, taxon: str, creator: str, seeds) -> list[str]:
        folder = self.tmp / f"{taxon}-{creator}".replace(" ", "-")
        folder.mkdir()
        for s in seeds:
            (folder / f"p{s}.jpg").write_bytes(picture(s))
        batch = folder.name.lower()
        self.quiet("fetch", "folder", str(folder), "--licence", "cc0", "--creator", creator, "--taxon", taxon,
                   "--batch", batch)
        return [r["sha256"] for r in self.rows(batch)]

    def answer(self, run: str, defn: dict, pairs) -> None:
        with Store(self.store_dir) as store:
            analyzer.import_analyses(store, run, [{"sha256": sha, "analyzer": defn, "out": out} for sha, out in pairs])

    def setUp(self):
        super().setUp()
        maple = self.add_photos("Acer circinatum", "Ann", range(10, 16)) + self.add_photos("Acer circinatum", "Bo", [16, 17])
        cactus = self.add_photos("Ferocactus wislizeni", "Cy", range(30, 34))
        self.maple, self.cactus = maple, cactus
        self.answer("t1", TRIAGE, [
            (maple[0], triage(["V1", "V2"])),
            (maple[1], triage(["V1"])),
            (maple[2], triage(["V4", "V5"], ["leaf"], leaf_face="lower")),
            (maple[3], triage(["V5"], ["leaf"], leaf_face="upper")),
            (maple[4], triage(["V8"], ["bark"])),
            (maple[5], triage(["V1"], usable=False, unusable_reason="blurred")),
            (maple[6], triage(["V1"], cultivar_look="yes")),
            (maple[7], triage(["V1", "V6"], ["flower"])),
            (cactus[0], triage(["V8"], ["stem", "spine", "areole"])),
            (cactus[1], triage(["V3"], ["apex", "spine"])),
            (cactus[2], triage(["V1"])),
            (cactus[3], triage(["V8"], ["fruit"])),
        ])
        self.answer("t2", OTHER, [(maple[0], triage(["V4"]))])  # fewer answers: not the default

    def report(self, *argv) -> dict:
        return json.loads(self.quiet("views", "--json", *argv))

    def test_counts_against_targets(self):
        out = self.report()
        self.assertEqual(out["analyzer"], analyzer.analyzer_id(TRIAGE))
        maple = out["taxa"]["Acer circinatum"]
        self.assertEqual((maple["triaged"], maple["usable"], maple["cactus"]), (8, 6, False))
        v = {r["view"]: r for r in maple["views"]}
        self.assertEqual((v["V1 whole plant"]["have"], v["V1 whole plant"]["photographers"]), (3, 2))
        self.assertEqual(v["V1 whole plant"]["need"], 5)
        self.assertEqual((v["V5 leaf face-on"]["have"], v["leaf underside"]["have"]), (2, 1))
        self.assertEqual((v["V8 bark or stem"]["have"], v["V8 fruit or cone"]["have"]), (1, 0))
        self.assertNotIn("apex (cacti)", v)

    def test_cacti_get_their_own_targets(self):
        cactus = self.report("--taxon", "Ferocactus wislizeni")["taxa"]
        self.assertEqual(list(cactus), ["Ferocactus wislizeni"])
        v = {r["view"]: r for r in cactus["Ferocactus wislizeni"]["views"]}
        self.assertNotIn("V5 leaf face-on", v)
        self.assertEqual((v["apex (cacti)"]["have"], v["areole close-up (cacti)"]["have"]), (1, 1))
        self.assertEqual((v["V8 bark or stem"]["have"], v["V8 fruit or cone"]["have"]), (1, 1))

    def test_analyzer_and_table(self):
        other = self.report("--analyzer", analyzer.analyzer_id(OTHER))["taxa"]["Acer circinatum"]
        self.assertEqual({r["view"]: r["have"] for r in other["views"]}["V4 shoot"], 1)
        table = self.quiet("views")
        self.assertIn("Acer circinatum: 6 usable of 8 triaged", table)
        self.assertIn("(cactus targets)", table)
        runs = Path(self.store_dir) / "sources" / "image-analyses"
        self.assertEqual(len(list(jsonl(runs / "t1" / "analyses.jsonl"))), 12)  # views only reads the store


if __name__ == "__main__":
    unittest.main()
