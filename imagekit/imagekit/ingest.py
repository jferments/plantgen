"""Turn downloads into image records: hash, merge duplicates, cap the size, strip GPS, name the taxon.

One row per download goes to sources/images/<batch>/images.jsonl. A
download whose bytes, or whose look (64-bit difference hash within 3 bits,
same shape), match an image already held becomes a second source of that
image, not a new one, so it is never stored or analysed twice.
"""
from __future__ import annotations

import csv
import hashlib
import io
import json
import sys
from collections import Counter
from pathlib import Path

from PIL import Image, ImageOps

from . import __version__, licences
from .store import Store, append_jsonl, jsonl, now

MAX_PX = 2048
SOURCE_KEYS = ("source", "id", "key", "url", "page", "licence", "creator", "attribution", "observer",
               "observation", "observation_url", "observation_uuid", "date", "place", "country", "lat", "lon",
               "accuracy_m", "obscured", "quality", "captive", "annotations", "basis", "institution",
               "catalogue_number", "title", "description", "name", "why", "original_sha256", "fetched")


def dhash(im: Image.Image) -> str:
    """64-bit difference hash: survives resizing and re-encoding, not cropping."""
    g = im.convert("L").resize((9, 8), Image.Resampling.LANCZOS)
    px = list(g.getdata())
    bits = 0
    for row in range(8):
        for col in range(8):
            bits = (bits << 1) | (px[row * 9 + col] > px[row * 9 + col + 1])
    return f"{bits:016x}"


def prepare(raw: bytes, max_px: int = MAX_PX) -> tuple[bytes, Image.Image, list[str]]:
    """The bytes to store. Kept as fetched unless they carry EXIF (GPS, camera), exceed max_px or are an odd format."""
    Image.open(io.BytesIO(raw)).verify()
    im = Image.open(io.BytesIO(raw))
    fmt = im.format
    changes = []
    if len(im.getexif()):
        changes.append("metadata stripped")
    if max(im.size) > max_px:
        changes.append(f"scaled to {max_px} px")
    if fmt not in ("JPEG", "PNG"):
        changes.append(f"{fmt} to JPEG")
    if not changes:
        im.load()
        return raw, im, changes
    im = ImageOps.exif_transpose(im)
    im.thumbnail((max_px, max_px), Image.Resampling.LANCZOS)
    keep_png = fmt == "PNG" and not any(c.startswith("scaled") for c in changes)
    out = io.BytesIO()
    if keep_png:
        im.save(out, "PNG")
    else:
        im.convert("RGB").save(out, "JPEG", quality=92, optimize=True)
    data = out.getvalue()
    return data, Image.open(io.BytesIO(data)), changes


class Wcvp:
    """Names resolved against WCVP's wcvp_names.csv (pipe-separated), cached in the store's index."""

    def __init__(self, path: str | None, store: Store):
        self.path, self.store, self.by_name, self.by_id = path, store, None, None

    def _load(self) -> None:
        self.by_name, self.by_id = {}, {}
        with open(self.path, newline="", encoding="utf-8") as fh:
            for r in csv.DictReader(fh, delimiter="|"):
                self.by_id[r["plant_name_id"]] = r
                self.by_name.setdefault(r["taxon_name"], []).append(r)

    def resolve(self, name: str | None) -> dict:
        if not name:
            return {}
        hit = self.store.db.execute("select record from taxa where name=?", (name,)).fetchone()
        if hit:
            return json.loads(hit[0])
        if not self.path:
            return {}
        if self.by_name is None:
            self._load()
        rows = self.by_name.get(name, [])
        row = next((r for r in rows if r["taxon_status"] == "Accepted"), rows[0] if rows else None)
        out = {}
        if row:
            acc = self.by_id.get(row.get("accepted_plant_name_id") or "", row) if row["taxon_status"] != "Accepted" else row
            out = {"accepted": f"{acc['taxon_name']} {acc.get('taxon_authors') or ''}".strip(),
                   "accepted_name": acc["taxon_name"], "wcvp_id": acc["plant_name_id"],
                   "family": acc.get("family"), "genus": acc.get("genus"), "status_of_given_name": row["taxon_status"]}
        self.store.db.execute("insert or replace into taxa values (?,?)", (name, json.dumps(out)))
        return out


def ingest(store: Store, batch: str, *, max_px: int = MAX_PX, wcvp: str | None = None, keep: bool = False) -> Counter:
    inc = store.incoming(batch)
    out_dir = store.batch_dir(batch)
    done = {r["source"]["key"] for r in jsonl(out_dir / "images.jsonl")}
    names = Wcvp(wcvp, store)
    tally = Counter()
    rows = []
    for cand in jsonl(inc / "fetched.jsonl"):
        if cand["key"] in done or store.has_ref(cand["key"]):
            continue
        f = inc / cand["file"]
        try:
            data, im, changes = prepare(f.read_bytes(), max_px)
        except Exception as err:  # noqa: BLE001 - a broken file is skipped, not fatal
            print(f"  not an image, skipped: {cand['key']} ({err})", file=sys.stderr)
            tally["unreadable"] += 1
            continue
        sha = hashlib.sha256(data).hexdigest()
        ph = dhash(im)
        size = {"width": im.size[0], "height": im.size[1], "bytes": len(data)}
        dup = None
        if store.has_image(sha):
            dup = {"of": sha, "match": "sha256", "distance": 0}
        else:
            near = store.near(ph, *im.size)
            if near:  # the row names the held image; the download's own hash and size are kept in `duplicate`
                dup = {"of": near[0], "match": "phash", "distance": near[1], "download": {"sha256": sha, **size}}
                sha = near[0]
        if dup:
            held = store.image(sha)
            ph, size = held["phash"], {k: held[k] for k in ("width", "height", "bytes")}
        src = {k: cand[k] for k in SOURCE_KEYS if cand.get(k) is not None}
        src.setdefault("attribution", licences.attribution(cand.get("creator"), cand["licence"]))
        taxon = {"name_as_given": cand.get("taxon"), "source_taxon_id": cand.get("source_taxon_id"),
                 "source_taxon_name": cand.get("source_taxon_name")}
        taxon.update(names.resolve(cand.get("source_taxon_name") or cand.get("taxon")))
        row = {"sha256": sha, "phash": ph, **size, "format": im.format, "stored": "re-encoded" if changes else "as fetched", "changes": changes,
               "duplicate": dup, "source": src, "taxon": {k: v for k, v in taxon.items() if v is not None},
               "batch": batch, "ingested": now()}
        if not dup:
            store.put_blob(sha, data)
        store.index_row(row)  # index at once, so a duplicate later in this batch is caught
        rows.append(row)
        tally["duplicate" if dup else "new"] += 1
        if not keep:
            f.unlink(missing_ok=True)
    append_jsonl(out_dir / "images.jsonl", rows)
    store.commit()
    write_batch_json(store, batch)
    write_parquet(out_dir)
    return tally


def write_batch_json(store: Store, batch: str) -> None:
    d = store.batch_dir(batch)
    rows = list(jsonl(d / "images.jsonl"))
    if not rows:
        return
    old = json.loads((d / "batch.json").read_text()) if (d / "batch.json").exists() else {}
    commands = list(jsonl(store.incoming(batch) / "commands.jsonl")) or old.get("commands", [])
    doc = {
        "batch": batch, "created": old.get("created", rows[0]["ingested"]), "updated": now(),
        "tool": f"imagekit {__version__}", "commands": commands,
        "images": sum(1 for r in rows if not r["duplicate"]), "rows": len(rows),
        "duplicates": sum(1 for r in rows if r["duplicate"]),
        "by_source": dict(Counter(r["source"]["source"] for r in rows)),
        "by_licence": dict(Counter(r["source"]["licence"] for r in rows)),
        "by_taxon": dict(Counter(r["taxon"].get("name_as_given") for r in rows)),
        "bytes": sum(r["bytes"] for r in rows if not r["duplicate"]),
    }
    (d / "batch.json").write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")


def write_parquet(d: Path) -> None:
    """images.parquet beside images.jsonl, when pyarrow is installed (pip install imagekit[parquet])."""
    try:
        import pyarrow as pa
        import pyarrow.parquet as pq
    except ImportError:
        return
    rows = list(jsonl(d / "images.jsonl"))
    flat = [{**{k: v for k, v in r.items() if k not in ("source", "taxon", "duplicate", "changes")},
             "duplicate_of": (r["duplicate"] or {}).get("of"), "changes": ", ".join(r["changes"]),
             "source": json.dumps(r["source"], ensure_ascii=False, sort_keys=True),
             "taxon": json.dumps(r["taxon"], ensure_ascii=False, sort_keys=True),
             "source_key": r["source"]["key"], "licence": r["source"]["licence"],
             "taxon_name": r["taxon"].get("accepted_name") or r["taxon"].get("name_as_given")} for r in rows]
    pq.write_table(pa.Table.from_pylist(flat), d / "images.parquet", compression="zstd")
