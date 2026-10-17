"""`imagekit embed`: whole-image vectors from SigLIP2, DINOv2 or BioCLIP-2, each image once per analyzer.

The analyzer is the model, its exact revision and the settings (image size,
region kind, pooling). Before loading the model, `Store.missing` drops every
image that already has a vector under it, so a rerun embeds nothing and a
crashed run continues where it stopped (a part is written every
--part-rows images).

Each part is three writes, in this order: the vectors and their ids
(vectors/<model>/part-NNNN.npy and .ids.jsonl), run.json naming the part
and its analyzer, then one analyses.jsonl row per image pointing at its
row. Only the last makes an image count as done.

Only whole images (region 0) for now; the ids carry a region so organ crops
can join the same parts later.
"""
from __future__ import annotations

import json
import socket
import sys
import time

from . import __version__, analyzer
from .store import Store, append_jsonl, jsonl, now


def register(subparsers) -> None:
    from . import vectors
    p = subparsers.add_parser("embed", help="whole-image vectors (SigLIP2, DINOv2, BioCLIP-2) for images that lack them")
    p.add_argument("--model", required=True,
                   help=f"one or more of {', '.join(vectors.public_models())}, comma-separated")
    p.add_argument("--batch", nargs="*", help="only images from these fetch batches (default: every image)")
    p.add_argument("--run", help="run name (default embed-<model>-<date>)")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--image-px", type=int, help="model input size (default: the model's own); a change is a new analyzer")
    p.add_argument("--limit", type=int, help="embed at most this many images")
    p.add_argument("--batch-size", type=int, default=16, help="images per forward pass (default 16)")
    p.add_argument("--part-rows", type=int, default=2048, help="images per part file (default 2048)")
    p.set_defaults(func=cmd_embed)


def cmd_embed(args, store: Store) -> None:
    for name in [m.strip() for m in args.model.split(",") if m.strip()]:
        embed(store, name, batches=args.batch, run=args.run, device=args.device, image_px=args.image_px,
              limit=args.limit, batch_size=args.batch_size, part_rows=args.part_rows)


def images_of(store: Store, batches=None) -> list[str]:
    if not batches:
        return store.all_images()
    return sorted({r["sha256"] for b in batches for r in jsonl(store.batch_dir(b) / "images.jsonl")})


def embed(store: Store, name: str, *, batches=None, run: str | None = None, device: str = "auto",
          image_px: int | None = None, limit: int | None = None, batch_size: int = 16, part_rows: int = 2048,
          log=None) -> dict:
    """Embed the images that lack this model's vectors; return counts."""
    from . import vectors
    log = log or (lambda *a: print(*a, file=sys.stderr))
    m = vectors.preset(name)
    revision = vectors.resolve_revision(name)
    defn = vectors.definition(name, revision, image_px)
    aid = analyzer.register(store, defn)
    shas = images_of(store, batches)
    todo = store.missing(aid, shas)
    log(f"{name} ({m['repo']}@{revision[:12]}, analyzer {aid}): {len(todo)} of {len(shas)} images need vectors")
    if limit is not None:
        todo = todo[:limit]
    if not todo:
        return {"model": name, "analyzer": aid, "embedded": 0, "failed": 0, "seconds": 0.0}
    run = run or f"embed-{name}-{time.strftime('%Y-%m-%d')}"
    t_load = time.perf_counter()
    enc = vectors.load_embedder(name, revision, defn["params"]["image_px"], device)
    load_s = time.perf_counter() - t_load
    log(f"  loaded on {enc.device} in {load_s:.1f} s; writing run {run}")
    vdir = vectors.vector_dir(store, run, name)
    vdir.mkdir(parents=True, exist_ok=True)
    done = failed = 0
    t0 = time.perf_counter()
    for start in range(0, len(todo), part_rows):
        chunk = todo[start:start + part_rows]
        vecs, ids, secs = [], [], []
        for i in range(0, len(chunk), batch_size):
            group, ims = [], []
            for sha in chunk[i:i + batch_size]:
                try:
                    ims.append(vectors.load_image(store.blob(sha), enc.image_px))
                    group.append(sha)
                except Exception as err:  # noqa: BLE001 - one unreadable image does not stop a run
                    log(f"  unreadable, skipped: {sha} ({err})")
                    failed += 1
            if not group:
                continue
            t = time.perf_counter()
            v = enc.images(ims)
            per = (time.perf_counter() - t) / len(group)
            vecs.append(v)
            ids.extend(group)
            secs.extend([per] * len(group))
            log(f"  {done + len(ids)}/{len(todo)} ({per:.3f} s/image)")
        if ids:
            write_part(store, run, name, aid, defn, enc, ids, vecs, secs)
            done += len(ids)
    seconds = time.perf_counter() - t0
    update_run(store, run, name, {"seconds_loading": round(load_s, 2)}, add_seconds=0)
    log(f"  embedded {done} in {seconds:.1f} s ({seconds / max(done, 1):.3f} s/image); {failed} unreadable")
    return {"model": name, "analyzer": aid, "run": run, "embedded": done, "failed": failed, "seconds": seconds,
            "dim": enc.dim}


def write_part(store, run, name, aid, defn, enc, ids, vecs, secs) -> None:
    import numpy as np
    from . import vectors
    arr = np.concatenate(vecs).astype(np.float16)
    enc.dim = arr.shape[1]
    vdir = vectors.vector_dir(store, run, name)
    n = len(list(vdir.glob("part-*.npy")))
    part = f"part-{n:04d}.npy"
    tmp = vdir / (part + ".tmp")
    with open(tmp, "wb") as fh:
        np.save(fh, arr)
    ids_tmp = vdir / f"part-{n:04d}.ids.jsonl.tmp"
    ids_tmp.write_text("".join(json.dumps({"sha256": s, "region": 0}) + "\n" for s in ids))
    ids_tmp.replace(vdir / f"part-{n:04d}.ids.jsonl")
    tmp.replace(vdir / part)
    update_run(store, run, name, {"model": defn["model"], "revision": defn["model_revision"], "analyzer": aid,
                                  "params": defn["params"], "device": enc.device, "dim": int(arr.shape[1])},
               part=(part, aid), add_rows=len(ids), add_seconds=sum(secs))
    rel = f"vectors/{name}/{part}"
    at = now()
    rows = [{"sha256": s, "region": 0, "analyzer": aid, "task": "embed", "status": "ok",
             "out": {"part": rel, "row": i, "dim": int(arr.shape[1])}, "run": run, "at": at,
             "seconds": round(secs[i], 4)} for i, s in enumerate(ids)]
    append_jsonl(store.run_dir(run) / "analyses.jsonl", rows)
    for r in rows:
        store.index_analysis(r, run)
    store.commit()


def update_run(store: Store, run: str, name: str, fields: dict, part=None, add_rows: int = 0,
               add_seconds: float = 0.0) -> None:
    """Merge into run.json: per model its revision, analyzer, device, parts, rows and seconds."""
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "task": "embed", "created": now()}
    info.update({"updated": now(), "tool": f"imagekit {__version__}", "host": socket.gethostname()})
    mod = info.setdefault("models", {}).setdefault(name, {"rows": 0, "seconds": 0.0, "parts": {}})
    mod.update(fields)
    if part:
        mod["parts"][part[0]] = part[1]
    mod["rows"] += add_rows
    mod["seconds"] = round(mod["seconds"] + add_seconds, 2)
    if mod["rows"]:
        mod["seconds_per_image"] = round(mod["seconds"] / mod["rows"], 4)
    info["rows"] = sum(v["rows"] for v in info["models"].values())
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")
