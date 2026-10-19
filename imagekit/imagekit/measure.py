"""`imagekit measure`: crown shape from each stored mask, by plantc's own code, so photos and renders score alike.

    imagekit segment --taxon "Asimina triloba"
    imagekit measure --taxon "Asimina triloba" --plantc ~/code/plantgen/target/release/plantc

Each mask from `imagekit segment` goes through `plantc measure-mask`, the
same functions (`crate::form`) that measure a grown plant's side view:
outline fit to its ellipse, see-through, width to height, where it is
widest, solidity and large gaps. The answer is stored as an analysis of the
mask's region (task `measure`). The analyzer is plantc's commit (or, for a
binary outside a checkout, its hash), the segmenter whose masks it read, the
view and the cover rule, so a change to any of them measures again.

What covers the sky inside the outline comes from the image itself
(`--cover sky`, the default): a pixel is sky when it is bright and bluish or
bright and colourless, the way open sky is in photos and in PlantGen's
render background. The mask gives the outline only, since SAM fills some
crowns' holes and not others. `--cover none` measures the mask alone.
"""
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from . import analyzer
from .store import Store, append_jsonl, check_open, now

COVER_RULES = {"sky": "sky-v1", "none": None}


def register(sub) -> None:
    p = sub.add_parser("measure", help="crown outline measures of each stored mask, through plantc measure-mask")
    p.add_argument("--plantc", default="plantc", help="the plantc binary (default: plantc on PATH)")
    p.add_argument("--segmenter", help="the segment analyzer whose masks to measure (default: most answers)")
    p.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    p.add_argument("--batch", nargs="*")
    p.add_argument("--cover", choices=sorted(COVER_RULES), default="sky",
                   help="what covers the sky inside the outline: sky-coloured pixels left out (default), or the mask")
    p.add_argument("--view", default="side")
    p.add_argument("--limit", type=int)
    p.add_argument("--run", help="run name (default measure-<date>)")
    p.set_defaults(func=cmd_measure)


def plantc_revision(plantc: str) -> str:
    """The commit plantc was built from (with -dirty), else the binary's own hash."""
    from .render import checkout_of, provenance
    if checkout_of(plantc) is not None:
        commit = provenance(plantc).get("commit")
        if commit and commit != "unknown":
            return commit
    exe = Path(plantc).expanduser()
    if not exe.is_file():
        found = subprocess.run(["sh", "-c", f"command -v {plantc}"], capture_output=True, text=True).stdout.strip()
        exe = Path(found)
    return "sha256:" + hashlib.sha256(exe.read_bytes()).hexdigest()[:16]


def definition(revision: str, segmenter: str, view: str, cover: str) -> dict:
    return {"task": "measure", "model": "plantc measure-mask", "model_revision": revision,
            "params": {"segmenter": segmenter, "view": view, "cover": COVER_RULES[cover]}}


def sky(im):
    """A boolean array: pixels bright and bluish, or bright and nearly colourless (rule sky-v1)."""
    import numpy as np
    a = np.asarray(im.convert("RGB")).astype(np.int16)
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    hi, lo = a.max(axis=2), a.min(axis=2)
    bluish = (b >= r + 12) & (b >= g) & (hi >= 150)
    pale = (lo >= 200) & (hi - lo <= 25)
    return bluish | pale


def cover_png(store: Store, sha: str, size, path: Path) -> None:
    """The image at the mask's size, as a PNG whose bright pixels are the ones that are not sky."""
    import numpy as np
    from PIL import Image
    from .vectors import load_image
    im = load_image(store.blob(sha)).resize(tuple(size), Image.Resampling.BILINEAR)
    Image.fromarray(((~sky(im)) * 255).astype(np.uint8)).save(path)


def default_segmenter(store: Store, shas=None) -> str | None:
    keep = set(shas) if shas is not None else None
    n: dict[str, int] = {}
    for aid, sha in store.db.execute("select analyzer, sha256 from analyses where task='segment' and status='ok'"):
        if keep is None or sha in keep:
            n[aid] = n.get(aid, 0) + 1
    return max(sorted(n), key=n.get) if n else None


def measure(store: Store, plantc: str = "plantc", *, taxa=None, batches=None, segmenter: str | None = None,
            cover: str = "sky", view: str = "side", limit: int | None = None, run: str | None = None,
            log=None) -> dict:
    from .regions import scope
    from .segment import masks
    log = log or (lambda *a: print(*a, file=sys.stderr))
    rows = scope(store, taxa, batches)
    seg = segmenter or default_segmenter(store, rows)
    if not seg:
        raise SystemExit("no masks on these images: run `imagekit segment` first")
    defn = definition(plantc_revision(plantc), seg, view, cover)
    aid = analyzer.register(store, defn)
    done = {(s, r) for s, r in store.db.execute("select sha256, region from analyses where analyzer=?", (aid,))}
    todo = [(k, v) for k, v in sorted(masks(store, seg).items()) if k[0] in rows and k not in done]
    if limit is not None:
        todo = todo[:limit]
    log(f"measure (analyzer {aid}, plantc {defn['model_revision']}, masks {seg}): {len(todo)} masks")
    if not todo:
        return {"analyzer": aid, "measured": 0}
    run = run or store.unsealed("run", f"measure-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    n = failed = 0
    with tempfile.TemporaryDirectory() as tmp:
        for (sha, region), (path, seg_out) in todo:
            t = time.perf_counter()
            cmd = [plantc, "measure-mask", str(path), "--view", view]
            if COVER_RULES[cover]:
                cpath = Path(tmp) / "cover.png"
                cover_png(store, sha, seg_out["size"], cpath)
                cmd += ["--cover", str(cpath)]
            res = subprocess.run(cmd, capture_output=True, text=True)
            try:
                out, status = json.loads(res.stdout.strip().splitlines()[-1]), "ok"
            except (ValueError, IndexError):
                out, status = {"error": (res.stderr or res.stdout).strip()[-500:]}, "failed"
                failed += 1
            row = {"sha256": sha, "region": region, "analyzer": aid, "task": "measure", "status": status,
                   "out": out, "run": run, "at": now(), "seconds": round(time.perf_counter() - t, 3)}
            append_jsonl(store.run_dir(run) / "analyses.jsonl", [row])
            store.index_analysis(row, run)
            n += 1
    store.commit()
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "task": "measure", "created": now()}
    info.update({"updated": now(), "analyzer": aid, "model": defn["model"], "revision": defn["model_revision"]})
    info["rows"] = info.get("rows", 0) + n
    p.write_text(json.dumps(info, indent=1) + "\n")
    return {"analyzer": aid, "run": run, "measured": n - failed, "failed": failed}


def cmd_measure(args, store: Store) -> None:
    print(json.dumps(measure(store, args.plantc, taxa=args.taxon, batches=args.batch, segmenter=args.segmenter,
                             cover=args.cover, view=args.view, limit=args.limit, run=args.run), indent=1))
