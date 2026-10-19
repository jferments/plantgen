"""`imagekit compact`: a library snapshot, every batch and run merged into Parquet that readers and search use.

    imagekit compact                          # sources/image-library/<date>/, and LATEST names it
    duckdb -c "select taxon, views, count(*) from '~/plantgen-images/sources/image-library/LATEST_DIR/triage.parquet' ..."

The batches and runs stay the record; a snapshot is a merged copy, made
again whenever it is wanted, and nothing in it is new information.

    sources/image-library/<snapshot>/
        images.parquet         one row per image: provenance (first source row), taxon, size, render or photo,
                               and every source key that names it
        analyzers.parquet      every analyzer id with its definition
        <task>.parquet         one row per (image, region, analyzer) answer that is ok: triage, observe,
                               describe, branching, detect, segment, ... (`out` as JSON); triage also has its
                               main fields as columns
        regions.parquet        one row per detected region: detector, organ, box, score
        vectors/<model>/<analyzer>.npy, .ids.parquet   each embedding analyzer's vectors in one file
        species.parquet        per taxon: images, photos, renders, usable photos per view, regions per organ
        stats.json             counts, the runs and batches merged, the tool version
    sources/image-library/LATEST                       the newest snapshot's name

Parquet is read over HTTPS with range requests by DuckDB, pandas, polars
and Arrow, so a snapshot on data.plantgen.io can be queried without
downloading it whole. Needs pyarrow (imagekit[parquet]) and numpy for the
vectors.
"""
from __future__ import annotations

import collections
import json
import shutil
import time
from pathlib import Path

from . import __version__
from .store import Store, is_render, jsonl, now

TRIAGE_COLUMNS = ("usable", "views", "organs_visible", "stage", "leaf_state", "setting", "age_class", "lighting",
                  "quality", "kind")


def register(sub) -> None:
    p = sub.add_parser("compact", help="merge every batch and run into a library snapshot (Parquet) for readers")
    p.add_argument("--name", help="snapshot name (default the date; -2, -3... if taken)")
    p.add_argument("--no-vectors", action="store_true", help="leave the embeddings out")
    p.set_defaults(func=cmd_compact)


def uniform(rows: list[dict]) -> list[dict]:
    """Rows Arrow can type: every key in every row, and a column whose values mix types (an id that is a
    number in one source and text in another) written as text; ints among floats stay numbers."""
    keys = list(dict.fromkeys(k for r in rows for k in r))
    mixed = set()
    for k in keys:
        kinds = {type(r.get(k)) for r in rows} - {type(None)}
        if len(kinds - {int, float}) and len(kinds) > 1:
            mixed.add(k)
    if not mixed:
        return [{k: r.get(k) for k in keys} for r in rows]
    text = lambda v: v if v is None or isinstance(v, str) else json.dumps(v, ensure_ascii=False)  # noqa: E731
    return [{k: (text(r.get(k)) if k in mixed else r.get(k)) for k in keys} for r in rows]


def library_dir(store: Store) -> Path:
    return store.root / "sources" / "image-library"


def latest(store: Store) -> Path | None:
    p = library_dir(store) / "LATEST"
    return library_dir(store) / p.read_text().strip() if p.exists() else None


def image_table(store: Store) -> list[dict]:
    first, keys = {}, collections.defaultdict(list)
    for b in store.batches():
        for r in jsonl(store.batch_dir(b) / "images.jsonl"):
            keys[r["sha256"]].append(r["source"]["key"])
            first.setdefault(r["sha256"], r)
    out = []
    for sha, r in sorted(first.items()):
        s, t = r["source"], r["taxon"]
        out.append({
            "sha256": sha, "width": r.get("width"), "height": r.get("height"), "bytes": r.get("bytes"),
            "phash": r.get("phash"), "batch": r["batch"], "kind": "render" if is_render(r) else "photo",
            "source": s.get("source"), "source_key": s["key"], "keys": sorted(set(keys[sha])),
            "licence": s.get("licence"), "creator": s.get("creator"), "attribution": s.get("attribution"),
            "page": s.get("page") or s.get("observation_url") or s.get("url"), "date": s.get("date"),
            "place": s.get("place"), "lat": s.get("lat"), "lon": s.get("lon"),
            "observation": str(s["observation"]) if s.get("observation") is not None else None,
            "observer": s.get("observer"),
            "taxon": t.get("accepted_name") or t.get("name_as_given"), "taxon_as_given": t.get("name_as_given"),
            "genus": t.get("genus") or (t.get("accepted_name") or t.get("name_as_given") or "").split(" ")[0] or None,
            "family": t.get("family"),
            "render": json.dumps(s["render"], ensure_ascii=False, sort_keys=True) if s.get("render") else None,
        })
    return out


def answers(store: Store) -> dict[str, list[dict]]:
    """task -> rows: every ok answer, one per (sha256, region, analyzer), the latest run's if two runs hold it."""
    seen, out = {}, collections.defaultdict(list)
    for run in store.runs():
        for r in jsonl(store.run_dir(run) / "analyses.jsonl"):
            if r.get("status", "ok") != "ok":
                continue
            key = (r["sha256"], r.get("region", 0), r["analyzer"])
            row = {"sha256": r["sha256"], "region": r.get("region", 0), "analyzer": r["analyzer"], "run": run,
                   "at": r.get("at"), "confidence": r.get("confidence"),
                   "out": json.dumps(r.get("out"), ensure_ascii=False, sort_keys=True), "_out": r.get("out")}
            task = r.get("task") or "unknown"
            if key in seen:
                old_task, i = seen[key]
                out[old_task][i] = row
            else:
                seen[key] = (task, len(out[task]))
                out[task].append(row)
    return out


def triage_row(row: dict) -> dict:
    o = row["_out"] or {}
    extra = {c: o.get(c) for c in TRIAGE_COLUMNS}
    return {**{k: v for k, v in row.items() if k != "_out"}, **extra}


def region_rows(rows: list[dict]) -> list[dict]:
    out = []
    for r in rows:
        for reg in (r["_out"] or {}).get("regions") or []:
            b = reg["box"]
            out.append({"sha256": r["sha256"], "detector": r["analyzer"], "region": reg["region"],
                        "organ": reg["organ"], "x0": b[0], "y0": b[1], "x1": b[2], "y1": b[3],
                        "score": reg.get("score")})
    return out


def species_table(images: list[dict], tasks: dict) -> list[dict]:
    by = collections.defaultdict(lambda: {"images": 0, "photos": 0, "renders": 0, "views": collections.Counter(),
                                          "regions": collections.Counter()})
    taxon_of = {i["sha256"]: i["taxon"] or "(no taxon)" for i in images}
    kind_of = {i["sha256"]: i["kind"] for i in images}
    for sha, t in taxon_of.items():
        by[t]["images"] += 1
        by[t]["photos" if kind_of[sha] == "photo" else "renders"] += 1
    # usable photos per view under each image's most-used triage analyzer (one per image, no double counting)
    tri_rows = tasks.get("triage") or []
    per_image = collections.defaultdict(dict)
    for r in tri_rows:
        if r["region"] == 0:
            per_image[r["sha256"]][r["analyzer"]] = r["_out"] or {}
    n_answers = collections.Counter(r["analyzer"] for r in tri_rows)
    for sha, by_aid in per_image.items():
        if sha not in taxon_of or kind_of[sha] != "photo":
            continue
        aid = max(sorted(by_aid), key=lambda a: n_answers[a])
        o = by_aid[aid]
        if o.get("usable"):
            for v in o.get("views") or []:
                by[taxon_of[sha]]["views"][v] += 1
    for r in region_rows(tasks.get("detect") or []):
        if r["sha256"] in taxon_of:
            by[taxon_of[r["sha256"]]]["regions"][r["organ"]] += 1
    return [{"taxon": t, "images": d["images"], "photos": d["photos"], "renders": d["renders"],
             "usable_photos_by_view": json.dumps(dict(sorted(d["views"].items()))),
             "regions_by_organ": json.dumps(dict(sorted(d["regions"].items())))}
            for t, d in sorted(by.items())]


def write_vectors(store: Store, snap: Path) -> dict:
    """Each embedding analyzer's vectors merged into vectors/<model>/<analyzer>.npy and .ids.parquet."""
    import numpy as np
    import pyarrow as pa
    import pyarrow.parquet as pq
    from . import vectors
    out = {}
    names = set()
    for run in store.runs():
        names |= set((vectors.read_run(store, run).get("models") or {}))
    defs = vectors.analyzers(store)
    for name in sorted(names):
        aids = set()
        for run in store.runs():
            aids |= set(((vectors.read_run(store, run).get("models") or {}).get(name) or {}).get("parts", {}).values())
        for aid in sorted(aids):
            vs = vectors.load(store, name, analyzer=aid)
            if not len(vs):
                continue
            d = snap / "vectors" / name
            d.mkdir(parents=True, exist_ok=True)
            np.save(d / f"{aid}.npy", np.asarray(vs.matrix, dtype=np.float16))
            pq.write_table(pa.Table.from_pylist([{"sha256": s, "region": r} for s, r in vs.ids]),
                           d / f"{aid}.ids.parquet", compression="zstd")
            kind = ((defs.get(aid) or {}).get("params") or {}).get("region_kind", "whole")
            out[f"{name}/{aid}"] = {"rows": len(vs), "dim": vs.dim, "region_kind": kind}
    return out


def compact(store: Store, name: str | None = None, with_vectors: bool = True) -> dict:
    try:
        import pyarrow as pa
        import pyarrow.parquet as pq
    except ImportError:
        raise SystemExit("compact needs pyarrow: python3 -m pip install 'imagekit[parquet]'") from None
    base = name or time.strftime("%Y-%m-%d")
    snap_name, n = base, 1
    while (library_dir(store) / snap_name).exists():
        if name:
            raise SystemExit(f"snapshot {name} exists: snapshots are written once; pick another --name")
        n += 1
        snap_name = f"{base}-{n}"
    snap = library_dir(store) / snap_name
    tmp = snap.with_name(snap.name + ".part")
    shutil.rmtree(tmp, ignore_errors=True)  # a snapshot stopped half-way
    tmp.mkdir(parents=True)

    def write(rows: list[dict], file: str) -> None:
        if rows:
            pq.write_table(pa.Table.from_pylist(uniform(rows)), tmp / file, compression="zstd")

    images = image_table(store)
    write(images, "images.parquet")
    defs = [{"id": r["id"], "task": r.get("task"), "model": r.get("model"), "model_revision": r.get("model_revision"),
             "rules_version": r.get("rules_version"), "prompt_hash": r.get("prompt_hash"),
             "params": json.dumps(r.get("params") or {}, ensure_ascii=False, sort_keys=True),
             "registered": r.get("registered")} for r in jsonl(store.root / "analyzers.jsonl")]
    write(defs, "analyzers.parquet")
    tasks = answers(store)
    counts = {}
    for task, rows in sorted(tasks.items()):
        if task == "embed":  # their vectors go in vectors/; the rows only point at parts
            counts[task] = len(rows)
            continue
        flat = [triage_row(r) for r in rows] if task == "triage" else [{k: v for k, v in r.items() if k != "_out"}
                                                                       for r in rows]
        write(flat, f"{task}.parquet")
        counts[task] = len(rows)
    regions = region_rows(tasks.get("detect") or [])
    write(regions, "regions.parquet")
    write(species_table(images, tasks), "species.parquet")
    vec = write_vectors(store, tmp) if with_vectors else {}
    stats = {"snapshot": snap_name, "made": now(), "tool": f"imagekit {__version__}", "images": len(images),
             "photos": sum(i["kind"] == "photo" for i in images), "renders": sum(i["kind"] == "render" for i in images),
             "taxa": len({i["taxon"] for i in images}), "answers by task": counts, "regions": len(regions),
             "analyzers": len(defs), "vectors": vec, "batches": store.batches(), "runs": store.runs()}
    (tmp / "stats.json").write_text(json.dumps(stats, indent=1, ensure_ascii=False) + "\n")
    tmp.replace(snap)
    (library_dir(store) / "LATEST").write_text(snap_name + "\n")
    return stats


def cmd_compact(args, store: Store) -> None:
    s = compact(store, args.name, not args.no_vectors)
    print(json.dumps({k: s[k] for k in ("snapshot", "images", "photos", "renders", "taxa", "answers by task",
                                        "regions", "analyzers")} | {"vectors": len(s["vectors"]),
                                                                   "folder": str(library_dir(store) / s["snapshot"])},
                     indent=1))
