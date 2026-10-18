"""The store: a folder laid out like data.plantgen.io's private side.

    <root>/sha256/<aa>/<sha256>                 image bytes, once (as pgio stores them)
    <root>/sources/images/<batch>/              one fetch batch
        batch.json                              commands, counts, tool version
        images.jsonl                            one row per image taken in (schemas/image-row)
        images.parquet                          the same rows, when pyarrow is installed
    <root>/sources/image-analyses/<run>/        one analysis run
        run.json, analyses.jsonl                (schemas/analysis)
    <root>/analyzers.jsonl                      every analyzer id and its definition
    <root>/incoming/<batch>/                    downloads waiting for `imagekit ingest`
    <root>/index.sqlite                         a lookup index, rebuilt from the files by `imagekit reindex`

The JSON Lines files are the record; the index only makes lookups fast.
"""
from __future__ import annotations

import json
import os
import sqlite3
import time
from pathlib import Path

DEFAULT_ROOT = os.environ.get("IMAGEKIT_STORE", str(Path.home() / "plantgen-images"))

SCHEMA = """
create table if not exists images (sha256 text primary key, phash text, b0 int, b1 int, b2 int, b3 int,
    width int, height int, bytes int, batch text);
create index if not exists images_b0 on images(b0);
create index if not exists images_b1 on images(b1);
create index if not exists images_b2 on images(b2);
create index if not exists images_b3 on images(b3);
create table if not exists refs (key text primary key, sha256 text, source text, taxon text, batch text);
create index if not exists refs_taxon on refs(source, taxon);
create table if not exists analyses (sha256 text, region int, analyzer text, run text, task text, status text,
    primary key (sha256, region, analyzer));
create table if not exists taxa (name text primary key, record text);
"""


def now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def bands(phash: str) -> list[int]:
    """Four 16-bit bands of a 64-bit hash: two hashes within 3 bits share at least one band."""
    v = int(phash, 16)
    return [(v >> (16 * i)) & 0xFFFF for i in range(4)]


def jsonl(path: Path):
    if path.exists():
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                if line.strip():
                    yield json.loads(line)


SEALED = ".pushed.json"  # in a batch or run folder: it is on data.plantgen.io and takes no more rows


def check_open(folder: Path) -> None:
    if (folder / SEALED).exists():
        raise SystemExit(f"{folder.name} is already on data.plantgen.io and takes no more rows: "
                         "use a new batch or run name (--batch / --run)")


def append_jsonl(path: Path, rows) -> None:
    check_open(path.parent)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a", encoding="utf-8") as fh:
        for row in rows:
            fh.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


class Store:
    def __init__(self, root: str | os.PathLike = DEFAULT_ROOT):
        self.root = Path(root).expanduser()
        self.root.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.root / "index.sqlite")
        cols = {r[1] for r in self.db.execute("pragma table_info(analyses)")}
        if cols and "status" not in cols:  # an index from imagekit 0.1: rebuild the analyses part
            self.db.execute("drop table analyses")
            self.db.executescript(SCHEMA)
            for r in self.runs():
                for row in jsonl(self.run_dir(r) / "analyses.jsonl"):
                    self.index_analysis(row, r)
            self.db.commit()
        self.db.executescript(SCHEMA)

    def close(self) -> None:
        self.db.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    # paths
    def blob(self, sha: str) -> Path:
        return self.root / "sha256" / sha[:2] / sha

    def batch_dir(self, batch: str) -> Path:
        return self.root / "sources" / "images" / batch

    def incoming(self, batch: str) -> Path:
        return self.root / "incoming" / batch

    def run_dir(self, run: str) -> Path:
        return self.root / "sources" / "image-analyses" / run

    def sealed(self, kind: str, name: str) -> bool:
        d = self.batch_dir(name) if kind == "batch" else self.run_dir(name)
        return (d / SEALED).exists()

    def unsealed(self, kind: str, base: str) -> str:
        """`base`, or `base-2`, `base-3`, ... if it is already on data.plantgen.io (a default name only)."""
        name, n = base, 1
        while self.sealed(kind, name):
            n += 1
            name = f"{base}-{n}"
        return name

    def batches(self) -> list[str]:
        d = self.root / "sources" / "images"
        return sorted(p.name for p in d.iterdir() if p.is_dir()) if d.exists() else []

    def runs(self) -> list[str]:
        d = self.root / "sources" / "image-analyses"
        return sorted(p.name for p in d.iterdir() if p.is_dir()) if d.exists() else []

    # lookups
    def has_ref(self, key: str) -> bool:
        return self.db.execute("select 1 from refs where key=?", (key,)).fetchone() is not None

    def held(self, source: str, taxon: str) -> int:
        return self.db.execute("select count(*) from refs where source=? and taxon=?", (source, taxon)).fetchone()[0]

    def has_image(self, sha: str) -> bool:
        return self.db.execute("select 1 from images where sha256=?", (sha,)).fetchone() is not None

    def image(self, sha: str) -> dict | None:
        r = self.db.execute("select phash, width, height, bytes, batch from images where sha256=?", (sha,)).fetchone()
        return dict(zip(("phash", "width", "height", "bytes", "batch"), r)) if r else None

    def near(self, phash: str, width: int, height: int, max_bits: int = 3):
        """The closest stored image within max_bits of phash and of the same shape, or None."""
        b = bands(phash)
        rows = self.db.execute(
            "select sha256, phash, width, height from images where b0=? or b1=? or b2=? or b3=?", b).fetchall()
        best = None
        for sha, ph, w, h in rows:
            d = bin(int(ph, 16) ^ int(phash, 16)).count("1")
            if d <= max_bits and abs(w / h - width / height) <= 0.02 * (width / height):
                if best is None or d < best[1]:
                    best = (sha, d)
        return best

    # writes
    def put_blob(self, sha: str, data: bytes) -> None:
        p = self.blob(sha)
        if not p.exists():
            p.parent.mkdir(parents=True, exist_ok=True)
            tmp = p.with_suffix(".part")
            tmp.write_bytes(data)
            tmp.replace(p)

    def index_row(self, row: dict) -> None:
        """Index one images.jsonl row (a new image, or a source reference to one already held)."""
        if not row.get("duplicate"):
            self.db.execute("insert or ignore into images values (?,?,?,?,?,?,?,?,?,?)",
                            (row["sha256"], row["phash"], *bands(row["phash"]), row["width"], row["height"],
                             row["bytes"], row["batch"]))
        src = row["source"]
        self.db.execute("insert or ignore into refs values (?,?,?,?,?)",
                        (src["key"], row["sha256"], src["source"], row["taxon"].get("name_as_given"), row["batch"]))

    def index_analysis(self, row: dict, run: str) -> None:
        self.db.execute("insert or replace into analyses values (?,?,?,?,?,?)",
                        (row["sha256"], row.get("region", 0), row["analyzer"], run, row.get("task"),
                         row.get("status", "ok")))

    def commit(self) -> None:
        self.db.commit()

    def reindex(self) -> dict:
        self.db.executescript("delete from images; delete from refs; delete from analyses;")
        n_img = n_an = 0
        for b in self.batches():
            for row in jsonl(self.batch_dir(b) / "images.jsonl"):
                self.index_row(row)
                n_img += 1
        for r in self.runs():
            for row in jsonl(self.run_dir(r) / "analyses.jsonl"):
                self.index_analysis(row, r)
                n_an += 1
        self.commit()
        return {"image rows": n_img, "analysis rows": n_an}

    # analyses
    def missing(self, analyzer: str, shas, region: int = 0, redo: tuple[str, ...] = ()) -> list[str]:
        """The images among shas with no analysis under this analyzer: the only ones a run may send.

        redo names statuses (e.g. "invalid") whose images are sent again; an "ok" answer never is.
        """
        done = {r[0] for r in self.db.execute(
            "select sha256 from analyses where analyzer=? and region=? and status not in (%s)"
            % ",".join("?" * len(redo) or ["''"]), (analyzer, region, *redo))}
        return [s for s in shas if s not in done]

    def all_images(self) -> list[str]:
        return [r[0] for r in self.db.execute("select sha256 from images order by sha256")]

    def stats(self) -> dict:
        q = lambda s: self.db.execute(s).fetchall()  # noqa: E731
        return {
            "images": q("select count(*) from images")[0][0],
            "source references": q("select count(*) from refs")[0][0],
            "by source": dict(q("select source, count(*) from refs group by source")),
            "taxa": q("select count(distinct taxon) from refs")[0][0],
            "analyses": q("select count(*) from analyses")[0][0],
            "analyzers": q("select count(distinct analyzer) from analyses")[0][0],
            "batches": self.batches(),
            "runs": self.runs(),
        }
