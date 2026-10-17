"""imagekit push/pull tests: a stand-in for pgio that writes the server's layout to a folder, served on localhost."""
from __future__ import annotations

import functools
import io
import json
import os
import sys
import threading
from contextlib import redirect_stderr, redirect_stdout
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from test_imagekit import Base, picture  # noqa: E402

from imagekit.store import Store, jsonl  # noqa: E402

FAKE_PGIO = '''
import hashlib, json, os, shutil, sys
from pathlib import Path
server, (cmd, source, version, path, *rest) = Path(sys.argv[1]), sys.argv[2:]
assert cmd == "push"
if "--dry-run" in rest:
    sys.exit(0)
files = []
for root, dirs, names in os.walk(path):
    dirs[:] = [d for d in dirs if not d.startswith(".")]
    for n in names:
        f = Path(root) / n
        data = f.read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        key = f"private/sha256/{sha[:2]}/{sha}"
        (server / key).parent.mkdir(parents=True, exist_ok=True)
        (server / key).write_bytes(data)
        files.append({"path": f.relative_to(path).as_posix(), "size": len(data), "sha256": sha, "key": key})
m = server / "private/sources" / source / version / "manifest.json"
m.parent.mkdir(parents=True, exist_ok=True)
m.write_text(json.dumps({"source": source, "version": version, "files": sorted(files, key=lambda f: f["path"])}))
ix = server / "private/sources/index.json"
index = json.loads(ix.read_text()) if ix.exists() else {"sources": {}}
index["sources"].setdefault(source, {})[version] = {"files": len(files)}
ix.write_text(json.dumps(index))
'''


class TestSync(Base):
    def setUp(self):
        super().setUp()
        self.server_dir = self.tmp / "server"
        self.server_dir.mkdir()
        (self.tmp / "fakepgio.py").write_text(FAKE_PGIO)
        self._env = os.environ.get("IMAGEKIT_PGIO")
        os.environ["IMAGEKIT_PGIO"] = f"{sys.executable} {self.tmp / 'fakepgio.py'} {self.server_dir}"
        handler = functools.partial(QuietHandler, directory=str(self.server_dir))
        self.http = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=self.http.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.http.server_port}"
        photos = self.tmp / "photos"
        photos.mkdir()
        for i in range(3):
            (photos / f"p{i}.jpg").write_bytes(picture(200 + i))
        self.quiet("fetch", "folder", str(photos), "--licence", "cc-by", "--creator", "Jay",
                   "--taxon", "Acer circinatum", "--batch", "own")
        with Store(self.store_dir) as s:
            shas = s.all_images()
        rows = [{"sha256": sha, "analyzer": {"task": "triage", "model": "m", "model_revision": "r",
                                             "rules_version": "v1", "prompt_hash": "p", "params": {}},
                 "out": {"usable": True}} for sha in shas]
        (self.tmp / "pilot.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
        self.quiet("import-analyses", str(self.tmp / "pilot.jsonl"), "--run", "pilot")

    def tearDown(self):
        self.http.shutdown()
        self.http.server_close()
        if self._env is None:
            os.environ.pop("IMAGEKIT_PGIO", None)
        else:
            os.environ["IMAGEKIT_PGIO"] = self._env
        super().tearDown()

    def quiet(self, *argv, store=None) -> str:
        from imagekit import cli
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            cli.main(["--store", store or self.store_dir, *argv])
        return out.getvalue()

    def test_push_then_pull_into_an_empty_store(self):
        self.quiet("push", "--dry-run")
        self.assertFalse((self.server_dir / "private").exists())
        self.assertFalse((Path(self.store_dir) / "sources/images/own/.pushed.json").exists())

        out = json.loads(self.quiet("push"))
        self.assertEqual((out["batches"], out["runs"]), (["own"], ["pilot"]))
        man = json.loads((self.server_dir / "private/sources/images/own/manifest.json").read_text())
        paths = [f["path"] for f in man["files"]]
        self.assertIn("sources/images/own/images.jsonl", paths)
        self.assertEqual(sum(p.startswith("sha256/") for p in paths), 3)
        self.assertFalse(any("/." in p or p.startswith(".") for p in paths))  # the seal is never uploaded
        run_man = json.loads((self.server_dir / "private/sources/image-analyses/pilot/manifest.json").read_text())
        self.assertIn("sources/image-analyses/pilot/analyzers.jsonl", [f["path"] for f in run_man["files"]])
        self.assertIn("nothing to push", self.quiet("push"))

        other = str(self.tmp / "other")
        got = json.loads(self.quiet("pull", "--base", self.base, store=other))
        self.assertEqual(sorted(got["pulled"]), ["image-analyses/pilot", "images/own"])
        with Store(other) as a, Store(self.store_dir) as b:
            self.assertEqual(a.all_images(), b.all_images())
            self.assertTrue(all(a.blob(s).read_bytes() == b.blob(s).read_bytes() for s in a.all_images()))
            aid = next(iter(jsonl(Path(self.store_dir) / "analyzers.jsonl")))["id"]
            self.assertEqual(a.missing(aid, a.all_images()), [])  # nothing to pay for again
        self.assertEqual([r["id"] for r in jsonl(Path(other) / "analyzers.jsonl")], [aid])
        self.assertEqual(json.loads(self.quiet("pull", "--base", self.base, store=other))["pulled"], {})

    def test_a_pushed_batch_or_run_takes_no_more_rows(self):
        self.quiet("push")
        photos = self.tmp / "more"
        photos.mkdir()
        (photos / "q.jpg").write_bytes(picture(300))
        with self.assertRaises(SystemExit):
            self.quiet("fetch", "folder", str(photos), "--licence", "cc0", "--creator", "Jay",
                       "--taxon", "Acer circinatum", "--batch", "own")
        with self.assertRaises(SystemExit):
            self.quiet("import-analyses", str(self.tmp / "pilot.jsonl"), "--run", "pilot")
        self.quiet("fetch", "folder", str(photos), "--licence", "cc0", "--creator", "Jay",
                   "--taxon", "Acer circinatum", "--batch", "own-2")
        self.assertEqual(json.loads(self.quiet("push"))["batches"], ["own-2"])

    def test_pull_checks_hashes(self):
        self.quiet("push")
        man = json.loads((self.server_dir / "private/sources/images/own/manifest.json").read_text())
        f = next(f for f in man["files"] if f["path"].endswith("images.jsonl"))
        (self.server_dir / f["key"]).write_bytes(b"tampered\n")
        with self.assertRaises(SystemExit):
            self.quiet("pull", "--base", self.base, store=str(self.tmp / "other"))
        self.assertFalse((self.tmp / "other/sources/images/own/images.jsonl").exists())


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *_):
        pass

