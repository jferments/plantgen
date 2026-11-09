"""`imagekit leaf-outline`: the leaf-outline judge, a species' drawn leaf against photographs of its leaves.

    imagekit fetch gbif --taxon "Physocarpus capitatus" --n 30 --organ leaf
    imagekit detect --organs leaf --taxon "Physocarpus capitatus"
    imagekit segment --organ leaf --triage none --taxon "Physocarpus capitatus"
    imagekit leaf-outline photos --taxon "Physocarpus capitatus"
    imagekit leaf-outline judge physocarpus-capitatus --plantc ~/code/plantgen/target/release/plantc --out judge.csv

**photos** measures each leaf mask from `imagekit segment` (regions whose
organ is `leaf`) and stores the answer as an analysis of that region (task
`leaf-outline`), so a mask is measured once. A mask is kept only when it is
one piece, clear of the frame and at least 2% of the image; the others are
stored too, with `kept: false` and why, so they are not read again.

**judge** draws each species' leaf and basal cards with `plantc atlas` (the
card face on, in true proportions), measures them the same way, and sets
them against the kept photo outlines of the species' taxon. The drawn cards
are not stored: they follow from plantc's commit, which the report names.

Measures, each blind to how the leaf lies in the frame and to its stalk (a
thin stalk is opened away first):

- `solidity`: area over its convex hull's (gaps between leaflets, deep lobes)
- `elongation`: the short principal axis over the long one, 1 round
- `notches`: hollows of the hull deeper than 6% of the leaf's length
- `fill`: area over the circle on its longest chord, 1 a disc

Like with like: a compound leaf is also drawn with its leaflets spread apart
on long stalks, and a sprig with two leaves spread wide, then cut apart; each
photo is scored against the nearer of the whole leaf and the median leaflet
(photos of compound leaves mostly show one leaflet). A species' distance is
the RMS of its z-scores, each measure's z against the pooled within-species
spread of the photos (1.4826 x the median MAD, at least 0.02).

Made on 2026-11-05/06 in plantgen-claude
`project-files/roadmap/leaf-outline-2026-11-05/leafshape.py` and run on 247
species (its `run-2026-11-06/README.md` has the findings and the judge's
known artefacts: whorls, mats and rosettes outlined as one leaf, flowers
taken for leaves). Needs numpy and scipy (`pip install -e imagekit[local]`).
"""
from __future__ import annotations

import csv
import json
import math
import statistics as st
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from . import analyzer
from .store import Store, append_jsonl, check_open, now

KEYS = ("solidity", "elongation", "notches", "fill")
GREY = (203, 203, 203)  # sRGB of plantc atlas's swatch, 0.6 linear grey
RULES = "leaf-outline-v1"  # the measures and the photo filter; change it when either changes


def register(sub) -> None:
    p = sub.add_parser("leaf-outline", help="the leaf-outline judge: drawn leaf cards against photographed leaves")
    cmds = p.add_subparsers(dest="leaf_cmd", required=True)
    ph = cmds.add_parser("photos", help="measure each stored leaf mask that has no leaf-outline answer")
    ph.add_argument("--segmenter", help="the segment analyzer whose leaf masks to measure (default: most answers)")
    ph.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    ph.add_argument("--batch", nargs="*")
    ph.add_argument("--limit", type=int)
    ph.add_argument("--run", help="run name (default leaf-outline-<date>)")
    ph.set_defaults(func=cmd_photos)
    j = cmds.add_parser("judge", help="draw each species' leaf with plantc and score it against its photos")
    j.add_argument("species", nargs="+", help="species ids, or @FILE with one per line")
    j.add_argument("--plantc", default="plantc", help="the plantc binary (default: plantc on PATH)")
    j.add_argument("--library", help="the species library plantc was built from (default: beside plantc's checkout)")
    j.add_argument("--min", type=int, default=3, help="fewest photo outlines a part needs to be scored (default 3)")
    j.add_argument("--out", help="write the scores here as CSV (default: print JSON)")
    j.set_defaults(func=cmd_judge)


# ---------------------------------------------------------------- measures

def _np():
    try:
        import numpy as np
        from scipy import ndimage
    except ImportError as err:  # pragma: no cover - the test skips without them
        raise SystemExit("leaf-outline needs numpy and scipy: pip install -e imagekit[local]") from err
    return np, ndimage


def _disc(r: int):
    np, _ = _np()
    y, x = np.ogrid[-r:r + 1, -r:r + 1]
    return (x * x + y * y) <= r * r


def clean(m):
    """The largest piece, holes filled, with a stalk cut off: open the leaf with a disc a quarter of its
    widest half-width, then of what that removes drop the long thin pieces (a petiole, a rachis stub) and
    keep the short ones (teeth, lobe tips)."""
    np, nd = _np()
    m = nd.binary_fill_holes(m)
    lab, n = nd.label(m)
    if n == 0:
        return m
    m = lab == (1 + int(np.argmax(nd.sum(m, lab, range(1, n + 1)))))
    big = nd.distance_transform_edt(m).max()
    opened = nd.binary_opening(m, structure=_disc(max(1, int(round(0.25 * big)))))  # noqa: RUF046
    lab, n = nd.label(opened)
    if not n:
        return m
    body = lab == (1 + int(np.argmax(nd.sum(opened, lab, range(1, n + 1)))))
    ys, xs = np.nonzero(body)
    length = math.hypot(xs.max() - xs.min(), ys.max() - ys.min())
    rest, _ = nd.label(m & ~body)
    keep = body.copy()
    for i, sl in enumerate(nd.find_objects(rest), start=1):
        piece = rest[sl] == i
        extent = math.hypot(sl[0].stop - sl[0].start, sl[1].stop - sl[1].start)
        if extent > 0.15 * length and piece.sum() / max(extent, 1.0) < 0.4 * big:
            continue  # a stalk: long, and thin against the blade (a sorrel's 4 px under a 24 px blade)
        keep[sl] |= piece
    return keep if keep.sum() > 0.5 * m.sum() else m


def hull(pts):
    """The convex hull of integer points (x, y), counter-clockwise (monotone chain)."""
    pts = sorted(set(map(tuple, pts)))
    if len(pts) < 3:
        return pts

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def measures(m, least: int = 200) -> dict | None:
    """The four measures of a boolean mask, or None for a leaf under `least` pixels once cleaned."""
    np, nd = _np()
    from PIL import Image, ImageDraw
    m = clean(m)
    area = float(m.sum())
    if area < least:
        return None
    ys, xs = np.nonzero(m)
    hp = np.array(hull(np.column_stack([xs, ys])), dtype=float)
    img = Image.new("L", (m.shape[1], m.shape[0]), 0)
    ImageDraw.Draw(img).polygon([tuple(p) for p in hp], fill=1)
    hm = np.asarray(img).astype(bool) | m
    pts = np.column_stack([xs, ys]).astype(float)
    ev = np.linalg.eigvalsh(np.cov((pts - pts.mean(0)).T))
    d = float(max(np.hypot(*(hp - p).T).max() for p in hp))
    lab, n = nd.label(hm & ~m)
    edge = nd.distance_transform_edt(hm)
    notches = sum(1 for k in range(1, n + 1) if edge[lab == k].max() > 0.06 * d)
    return {"solidity": area / float(hm.sum()), "elongation": math.sqrt(max(ev[0], 1e-9) / max(ev[1], 1e-9)),
            "notches": notches, "fill": area / (math.pi * (d / 2) ** 2)}


def photo_measures(m) -> dict:
    """A photo mask's measures, or why it is not kept: one piece, clear of the frame, 2% of the image."""
    np, nd = _np()
    h, w = m.shape
    if m.sum() < 0.02 * h * w:
        return {"kept": False, "why": "under 2% of the image"}
    ys, xs = np.nonzero(m)
    if xs.min() <= 1 or ys.min() <= 1 or xs.max() >= w - 2 or ys.max() >= h - 2:
        return {"kept": False, "why": "cut by the frame"}
    lab, n = nd.label(m)
    if n > 1 and nd.sum(m, lab, range(1, n + 1)).max() < 0.9 * m.sum():
        return {"kept": False, "why": "in pieces"}
    got = measures(m)
    return {"kept": True, **got} if got else {"kept": False, "why": "too small once its stalk is cut"}


# ---------------------------------------------------------------- photos

def definition(segmenter: str) -> dict:
    return {"task": "leaf-outline", "model": "imagekit leaf-outline", "model_revision": RULES,
            "params": {"segmenter": segmenter}}


def leaf_segmenter(store: Store, shas=None) -> str | None:
    """The segment analyzer with most leaf masks among shas."""
    from .segment import masks
    keep = set(shas) if shas is not None else None
    n: dict[str, int] = {}
    for (aid,) in store.db.execute("select distinct analyzer from analyses where task='segment' and status='ok'"):
        k = sum(1 for (sha, _), (_, out) in masks(store, aid).items()
                if out.get("organ") == "leaf" and (keep is None or sha in keep))
        if k:
            n[aid] = k
    return max(sorted(n), key=n.get) if n else None


def photos(store: Store, *, taxa=None, batches=None, segmenter: str | None = None, limit: int | None = None,
           run: str | None = None, log=None) -> dict:
    np, _ = _np()
    from PIL import Image

    from .regions import scope
    from .segment import masks
    log = log or (lambda *a: print(*a, file=sys.stderr))
    rows = scope(store, taxa, batches)
    seg = segmenter or leaf_segmenter(store, rows)
    if not seg:
        raise SystemExit("no leaf masks on these images: run `imagekit segment --organ leaf` first")
    aid = analyzer.register(store, definition(seg))
    done = {(s, r) for s, r in store.db.execute("select sha256, region from analyses where analyzer=?", (aid,))}
    todo = [(k, v) for k, v in sorted(masks(store, seg).items())
            if k[0] in rows and k not in done and v[1].get("organ") == "leaf"]
    if limit is not None:
        todo = todo[:limit]
    log(f"leaf-outline (analyzer {aid}, masks {seg}): {len(todo)} masks")
    if not todo:
        return {"analyzer": aid, "measured": 0, "kept": 0}
    run = run or store.unsealed("run", f"leaf-outline-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    kept = 0
    for (sha, region), (path, _) in todo:
        t = time.perf_counter()
        out = photo_measures(np.asarray(Image.open(path).convert("L")) > 127)
        kept += out["kept"]
        row = {"sha256": sha, "region": region, "analyzer": aid, "task": "leaf-outline", "status": "ok",
               "out": out, "run": run, "at": now(), "seconds": round(time.perf_counter() - t, 3)}
        append_jsonl(store.run_dir(run) / "analyses.jsonl", [row])
        store.index_analysis(row, run)
    store.commit()
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "task": "leaf-outline", "created": now()}
    info.update({"updated": now(), "analyzer": aid, "model": "imagekit leaf-outline", "revision": RULES})
    info["rows"] = info.get("rows", 0) + len(todo)
    p.write_text(json.dumps(info, indent=1) + "\n")
    return {"analyzer": aid, "run": run, "measured": len(todo), "kept": kept}


def photo_rows(store: Store) -> dict[str, list[dict]]:
    """Binomial -> the kept photo outlines of that taxon, under every leaf-outline analyzer of these rules."""
    from .run import image_rows
    from .store import jsonl
    images = image_rows(store)
    by: dict[str, list[dict]] = {}
    seen = set()
    for (run,) in store.db.execute("select distinct run from analyses where task='leaf-outline'"):
        for row in jsonl(store.run_dir(run) / "analyses.jsonl"):
            out, key = row.get("out") or {}, (row["sha256"], row.get("region", 0))
            if row.get("task") != "leaf-outline" or not out.get("kept") or key in seen:
                continue
            seen.add(key)
            tax = (images.get(row["sha256"]) or {}).get("taxon") or {}
            name = binomial(tax.get("accepted_name") or tax.get("name_as_given") or "")
            if name:
                by.setdefault(name, []).append({**{k: out[k] for k in KEYS}, "sha256": row["sha256"]})
    return by


def binomial(name: str) -> str:
    return " ".join(name.split()[:2])


# ---------------------------------------------------------------- drawn

def drawn_mask(png):
    np, _ = _np()
    from PIL import Image
    a = np.asarray(Image.open(png).convert("RGB")).astype(int)
    return np.abs(a - np.array(GREY)).sum(2) > 6  # pale and frosted leaves (brittlebush) differ by little


def leaflets(m):
    """A card's leaflets (or a sprig's leaves), cut apart where stalks or feet join them: an opening about
    4% of the leaf's size, pieces over a tenth of the largest. None for a leaf that stays one piece."""
    np, nd = _np()
    m = nd.binary_fill_holes(m)
    r = max(2, int(round(0.04 * math.sqrt(m.sum()))))  # noqa: RUF046 - numpy floats round to floats
    opened = nd.binary_opening(m, structure=_disc(r))
    lab, n = nd.label(opened)
    if n < 2:
        return None
    sizes = nd.sum(opened, lab, range(1, n + 1))
    keep = [k + 1 for k in range(n) if sizes[k] > 0.1 * sizes.max()]
    if len(keep) < 2:
        return None
    return [nd.binary_dilation(lab == k, iterations=r) & m & ~np.isin(lab, [j for j in keep if j != k])
            for k in keep]


def spec_path(library: Path, sid: str) -> Path | None:
    found = sorted(library.glob(f"**/{sid}/spec.json"))
    return found[0] if found else None


def atlas(plantc: str, spec: str, organ: str, png: Path) -> bool:
    res = subprocess.run([plantc, "atlas", spec, "--organ", organ, "--out", str(png)], capture_output=True, text=True, check=False)
    return res.returncode == 0 and png.exists()


def template_of(plantc: str, sid: str, organ: str) -> str | None:
    """The card's template as `plantc spec` resolves it (its own spec or a rank above), else None."""
    res = subprocess.run([plantc, "spec", sid], capture_output=True, text=True, check=False)
    want = f"appearance.organs.{organ}.shape.template = "
    for line in res.stdout.splitlines():
        line = line.strip()
        if line.startswith(want):
            return line[len(want):].split()[0].strip('"')
    return None


def apart(plantc: str, spec: dict, organ: str, tmp: Path, sprig: bool):
    """A compound leaf's leaflets each on its own: the card drawn again with its leaflets on long stalks and a
    palmate leaf's spread to a half circle (their own shapes untouched), cut apart at the stalks; a sprig's
    leaves drawn two to a twig, spread wide. None for a leaf that stays one piece."""
    path, png = tmp / f"apart-{organ}.json", tmp / f"apart-{organ}.png"
    tries = [{"leaves": 2, "angle": 80, "stalk": 0.3, "leaf": 0, "twigs": 0}] if sprig else \
        [{"petiolule": 0.25, "spread": 200}, {"petiolule": 0.25}]
    for extra in tries:
        d = json.loads(json.dumps(spec))
        sh = d.setdefault("appearance", {}).setdefault("organs", {}).setdefault(organ, {}).setdefault("shape", {})
        sh.update(extra)
        path.write_text(json.dumps(d))
        if atlas(plantc, str(path), organ, png):
            return leaflets(drawn_mask(png))
    return None


def drawn(plantc: str, library: Path, sid: str) -> list[dict]:
    """The measures of a species' drawn leaf and basal cards, whole and, where they part, per leaflet."""
    rows = []
    found = spec_path(library, sid)
    spec = json.loads(found.read_text()) if found else None
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        for organ in ("leaf", "basal"):
            png = tmp / f"{organ}.png"
            if not atlas(plantc, sid, organ, png):
                continue
            mask = drawn_mask(png)
            whole = measures(mask)
            if whole:
                rows.append({"id": sid, "organ": organ, "part": "leaf", **whole})
            sprig = template_of(plantc, sid, organ) == "sprig"
            pieces = (apart(plantc, spec, organ, tmp, sprig) if spec else None) or leaflets(mask)
            ms = [x for x in (measures(p, least=80) for p in pieces or []) if x]  # a sumac's leaflet ~150 px
            if ms:
                rows.append({"id": sid, "organ": organ, "part": "leaflet", "leaflets": len(ms),
                             **{k: st.median(x[k] for x in ms) for k in KEYS}})
    return rows


# ---------------------------------------------------------------- judge

def spreads(by: dict[str, list[dict]], least: int) -> dict[str, float | None]:
    """Each measure's pooled within-species spread of the photos."""
    out = {}
    for k in KEYS:
        mads = []
        for v in by.values():
            xs = [r[k] for r in v]
            if len(xs) >= least:
                med = st.median(xs)
                mads.append(1.4826 * st.median(abs(x - med) for x in xs))
        out[k] = max(st.median(mads), 0.02) if mads else None
    return out


def score(drawn_rows: list[dict], names: dict[str, str], by: dict[str, list[dict]], least: int) -> list[dict]:
    """Each drawn part against the photos nearer it than its sibling part; parts with too few go unscored."""
    spread = spreads(by, least)
    if any(v is None for v in spread.values()):
        return []

    def z2(photo, r):
        return sum(((r[k] - photo[k]) / spread[k]) ** 2 for k in KEYS)
    parts: dict[tuple, list[dict]] = {}
    for r in drawn_rows:
        parts.setdefault((r["id"], r["organ"]), []).append(r)
    out = []
    for r in drawn_rows:
        sib = parts[(r["id"], r["organ"])]
        v = [p for p in by.get(names.get(r["id"], ""), []) if min(sib, key=lambda q: z2(p, q)) is r]
        if len(v) < least:
            continue
        row = {"id": r["id"], "organ": r["organ"], "part": r["part"], "photos": len(v)}
        zs = []
        for k in KEYS:
            pm = st.median(x[k] for x in v)
            z = (r[k] - pm) / spread[k]
            zs.append(z)
            row.update({f"{k}_photo": round(pm, 3), f"{k}_drawn": round(r[k], 3), f"{k}_z": round(z, 2)})
        row["distance"] = round(math.sqrt(sum(z * z for z in zs) / len(zs)), 2)
        out.append(row)
    return sorted(out, key=lambda r: -r["distance"])


def default_library(plantc: str) -> Path:
    from .render import checkout_of
    root = checkout_of(plantc)
    if root is None:
        raise SystemExit("give --library: plantc is not in a plantgen checkout")
    return Path(root) / "crates" / "plantgen" / "library"


def judge(store: Store, species: list[str], plantc: str = "plantc", library: str | None = None,
          least: int = 3, log=None) -> dict:
    from .measure import plantc_revision
    log = log or (lambda *a: print(*a, file=sys.stderr))
    lib = Path(library).expanduser() if library else default_library(plantc)
    names, rows = {}, []
    for sid in species:
        found = spec_path(lib, sid)
        if found:
            names[sid] = binomial(json.loads(found.read_text())["taxon"]["scientific_name"])
        rows += drawn(plantc, lib, sid)
        log(sid)
    by = photo_rows(store)
    return {"plantc": plantc_revision(plantc), "rules": RULES, "spread": spreads(by, least),
            "scores": score(rows, names, by, least), "drawn": rows}


def cmd_photos(args, store: Store) -> None:
    print(json.dumps(photos(store, taxa=args.taxon, batches=args.batch, segmenter=args.segmenter, limit=args.limit,
                            run=args.run), indent=1))


def cmd_judge(args, store: Store) -> None:
    ids = []
    for s in args.species:
        ids += [ln.strip() for ln in Path(s[1:]).read_text().splitlines() if ln.strip()] if s.startswith("@") else [s]
    res = judge(store, ids, args.plantc, args.library, args.min)
    if not args.out:
        print(json.dumps(res, indent=1))
        return
    cols = ["id", "organ", "part", "photos", "distance"] + [f"{k}_{w}" for k in KEYS for w in ("photo", "drawn", "z")]
    with open(args.out, "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(res["scores"])
    print(json.dumps({k: res[k] for k in ("plantc", "rules", "spread")} | {"scored": len(res["scores"]),
                                                                           "out": args.out}, indent=1))
