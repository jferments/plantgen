"""`imagekit detect` and `imagekit crops`: organ boxes in each image, and the organs cut out.

    imagekit detect --taxon "Asimina triloba"                 # Grounding DINO, usable photos and every render
    imagekit crops --taxon "Asimina triloba" --organ leaf --out ~/pawpaw-leaves

A detection is one analysis per image (region 0, task `detect`) whose
answer lists the regions found:

    {"image": [w, h], "regions": [{"region": 1, "organ": "leaf", "box": [x0, y0, x1, y1], "score": 0.44}, ...]}

Boxes are fractions of width and height, so they hold at any image size.
Region numbers belong to the detector: an analysis of region 3 (a leaf's
vector, say) names that detector in its own analyzer's params, so regions
from two detectors are never mixed. The detector's analyzer is the model,
its revision, the organ phrases and the thresholds; a rerun detects only
the images that lack it.

Crops keep only the organs the image's triage saw (`organs_visible`) when
the triage answered it, so fingers holding a flower are not kept as
leaves. Renders are always detected and need no triage.
"""
from __future__ import annotations

import json
import socket
import sys
import time
from pathlib import Path

from . import __version__, analyzer
from .store import Store, append_jsonl, check_open, is_render, now

# Detector presets: the repository and its pinned revision.
DETECTORS = {
    "gdino-tiny": {"repo": "IDEA-Research/grounding-dino-tiny", "revision": "a2bb814dd30d776dcf7e30523b00659f4f141c71"},
    "gdino-base": {"repo": "IDEA-Research/grounding-dino-base", "revision": "12bdfa3120f3e7ec7b434d90674b3396eccf88eb"},
    # Test model: the whole frame as the plant, and the box of strongly green pixels as a leaf. No torch.
    "fake": {"repo": "imagekit/fake-detector", "revision": "fake-1", "test": True},
}
# The words Grounding DINO is asked for, per triage organ (photo-rules/01-triage.md organs_visible).
PHRASES = {
    "plant": ("tree", "shrub", "plant"),
    "leaf": ("leaf",),
    "flower": ("flower",),
    "inflorescence": ("flower cluster",),
    "fruit": ("fruit",),
    "cone": ("cone",),
    "bark": ("tree trunk",),
}
DEFAULT_ORGANS = ("plant", "leaf", "flower", "inflorescence", "fruit", "cone", "bark")
WHOLE_VIEWS = {"V1", "V2", "V3"}
DETECT_PX = 1024
THRESHOLD, TEXT_THRESHOLD = 0.3, 0.25
MIN_SIDE = 0.04      # a box narrower than this fraction of the image's long side is too small to read
SAME = 0.5           # two boxes of one organ overlapping this much (IoU) are one
MAX_PER_ORGAN = 8
LABEL_RULE = 2       # how a run-together label is read (organ_of_label); a change is a new analyzer


def register(sub) -> None:
    p = sub.add_parser("detect", help="find organs (leaf, flower, fruit, whole plant...) in images that lack it")
    p.add_argument("--model", default="gdino-tiny", help=f"one of {', '.join(public())} (default gdino-tiny)")
    p.add_argument("--organs", default=",".join(DEFAULT_ORGANS),
                   help=f"comma-separated, from {', '.join(PHRASES)} (default all); a change is a new analyzer")
    p.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    p.add_argument("--batch", nargs="*", help="only images from these fetch batches")
    p.add_argument("--triage", help="the triage analyzer whose usable photos to detect (default: the one with most "
                                    "answers on these images)")
    p.add_argument("--all", action="store_true", help="also photos with no triage or an unusable one")
    p.add_argument("--threshold", type=float, default=THRESHOLD)
    p.add_argument("--detect-px", type=int, default=DETECT_PX, help="long side the detector sees (default %(default)s)")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--limit", type=int)
    p.add_argument("--run", help="run name (default detect-<model>-<date>)")
    p.set_defaults(func=cmd_detect)

    p = sub.add_parser("crops", help="cut detected organs out into a folder, with crops.jsonl")
    p.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    p.add_argument("--batch", nargs="*")
    p.add_argument("--organ", action="append", help="only this organ (repeatable)")
    p.add_argument("--detector", help="the detect analyzer (default: the one with most answers on these images)")
    p.add_argument("--triage", help="keep organs this triage saw (default: the one with most answers; 'none' keeps all)")
    p.add_argument("--min-score", type=float, default=0.0)
    p.add_argument("--pad", type=float, default=0.08, help="margin around each box, as a share of its long side")
    p.add_argument("--px", type=int, default=1024, help="long side of each crop at most (default %(default)s)")
    p.add_argument("--out", required=True)
    p.set_defaults(func=cmd_crops)


def public() -> list[str]:
    return [k for k, v in DETECTORS.items() if not v.get("test")]


def definition(model: str, organs, threshold: float = THRESHOLD, detect_px: int = DETECT_PX) -> dict:
    if model not in DETECTORS:
        raise SystemExit(f"unknown detector {model!r}: one of {', '.join(public())}")
    bad = [o for o in organs if o not in PHRASES]
    if bad:
        raise SystemExit(f"no phrase for organ {', '.join(bad)}: one of {', '.join(PHRASES)}")
    m = DETECTORS[model]
    return {"task": "detect", "model": m["repo"], "model_revision": m["revision"],
            "params": {"phrases": {o: list(PHRASES[o]) for o in organs}, "threshold": threshold,
                       "text_threshold": TEXT_THRESHOLD, "detect_px": detect_px, "min_side": MIN_SIDE,
                       "same_iou": SAME, "max_per_organ": MAX_PER_ORGAN, "labels": LABEL_RULE}}


# --- selection --------------------------------------------------------------------------------------------------

def scope(store: Store, taxa=None, batches=None) -> dict:
    """sha256 -> first images.jsonl row, for the taxa and batches asked."""
    from .run import image_rows
    rows = image_rows(store, batches)
    if taxa:
        want = set(taxa)
        rows = {s: r for s, r in rows.items()
                if want & {r["taxon"].get("name_as_given"), r["taxon"].get("accepted_name")}}
    return rows


def triage_answers(store: Store, rows: dict, triage: str | None) -> tuple[str | None, dict]:
    from .run import default_triage, outputs
    if triage == "none":
        return None, {}
    tri = triage or default_triage(store, rows)
    return tri, (outputs(store, tri) if tri else {})


def default_detector(store: Store, shas=None) -> str | None:
    """The detect analyzer with most answers among shas (or overall)."""
    keep = set(shas) if shas is not None else None
    n: dict[str, int] = {}
    for aid, sha in store.db.execute("select analyzer, sha256 from analyses where task='detect' and status='ok' "
                                     "and region=0"):
        if keep is None or sha in keep:
            n[aid] = n.get(aid, 0) + 1
    return max(sorted(n), key=n.get) if n else None


def regions(store: Store, detector: str, rows: dict, triage: dict | None = None, organs=None,
            min_score: float = 0.0) -> dict:
    """sha256 -> its regions under a detector, keeping organs the triage saw (all, for an image it did not answer)."""
    from .run import outputs
    out = {}
    for sha, ans in outputs(store, detector).items():
        if sha not in rows:
            continue
        seen = None
        if triage and sha in triage:
            seen = set(triage[sha].get("organs_visible") or [])
            if WHOLE_VIEWS & set(triage[sha].get("views") or []):
                seen.add("plant")  # a whole-plant view shows the plant, whether or not organs_visible says so
        keep = [r for r in ans.get("regions") or []
                if r.get("score", 1) >= min_score and (not organs or r["organ"] in organs)
                and (seen is None or r["organ"] in seen)]
        if keep:
            out[sha] = keep
    return out


def crop(store: Store, sha: str, box, pad: float = 0.0, px: int | None = None):
    """The region of an image (box in fractions), with a margin of pad x its long side, at most px on a side."""
    from PIL import Image
    from .vectors import load_image
    im = load_image(store.blob(sha))
    w, h = im.size
    x0, y0, x1, y1 = box[0] * w, box[1] * h, box[2] * w, box[3] * h
    m = pad * max(x1 - x0, y1 - y0)
    c = im.crop((max(0, round(x0 - m)), max(0, round(y0 - m)), min(w, round(x1 + m)), min(h, round(y1 + m))))
    if px and max(c.size) > px:
        c.thumbnail((px, px), Image.Resampling.LANCZOS)
    return c


# --- detectors --------------------------------------------------------------------------------------------------

def _iou(a, b) -> float:
    x0, y0, x1, y1 = max(a[0], b[0]), max(a[1], b[1]), min(a[2], b[2]), min(a[3], b[3])
    inter = max(0.0, x1 - x0) * max(0.0, y1 - y0)
    union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / union if union > 0 else 0.0


def tidy(found, defn: dict) -> list[dict]:
    """[(organ, box in fractions, score)] -> numbered regions: small boxes dropped, overlapping ones of an organ
    merged into the best, at most max_per_organ an organ, numbered by organ order then score."""
    p = defn["params"]
    order = list(p["phrases"])
    keep: dict[str, list] = {o: [] for o in order}
    for organ, box, score in sorted(found, key=lambda t: -t[2]):
        if organ not in keep or max(box[2] - box[0], box[3] - box[1]) < p["min_side"]:
            continue
        if len(keep[organ]) < p["max_per_organ"] and all(_iou(box, b) < p["same_iou"] for b, _ in keep[organ]):
            keep[organ].append((box, score))
    out = []
    for organ in order:
        for box, score in keep[organ]:
            out.append({"region": len(out) + 1, "organ": organ, "box": [round(v, 4) for v in box],
                        "score": round(score, 3)})
    return out


def organ_of_label(label: str, phrases: dict) -> str | None:
    """The organ a detector label names. Grounding DINO can run neighbouring phrases together ("tree shrub
    plant"): such a label goes to the one organ its words point to, counting only words one organ's phrases
    use ("tree" is in "tree" and "tree trunk"). It names none when two organs tie or a word belongs only to
    other organs ("leaf flower")."""
    label = " ".join(label.split())
    for organ, phs in phrases.items():
        if label in phs:
            return organ
    owners: dict[str, set] = {}
    for organ, phs in phrases.items():
        for word in " ".join(phs).split():
            owners.setdefault(word, set()).add(organ)
    votes: dict[str, int] = {}
    for word in label.split():
        if len(owners.get(word, ())) == 1:
            o = next(iter(owners[word]))
            votes[o] = votes.get(o, 0) + 1
    if not votes:
        return None
    best = max(votes.values())
    top = [o for o, n in votes.items() if n == best]
    if len(top) != 1 or any(top[0] not in owners.get(w, {top[0]}) for w in label.split()):
        return None  # two organs tie, or a word belongs only to other organs ("leaf flower")
    return top[0]


class GroundingDino:
    def __init__(self, defn: dict, device: str):
        import torch
        from transformers import AutoModelForZeroShotObjectDetection, AutoProcessor
        from .vectors import pick_device
        self.torch, self.device = torch, pick_device(device)
        self.proc = AutoProcessor.from_pretrained(defn["model"], revision=defn["model_revision"])
        self.model = AutoModelForZeroShotObjectDetection.from_pretrained(
            defn["model"], revision=defn["model_revision"]).to(self.device).eval()
        p = defn["params"]
        self.phrases = p["phrases"]
        self.text = ". ".join(ph for phs in self.phrases.values() for ph in phs) + "."
        self.threshold, self.text_threshold = p["threshold"], p["text_threshold"]

    def find(self, im) -> list:
        """[(organ, box in fractions, score)] in one pass over every phrase."""
        with self.torch.inference_mode():
            inputs = self.proc(images=im, text=self.text, return_tensors="pt").to(self.device)
            out = self.model(**inputs)
            res = self.proc.post_process_grounded_object_detection(
                out, inputs.input_ids, threshold=self.threshold, text_threshold=self.text_threshold,
                target_sizes=[im.size[::-1]])[0]
        w, h = im.size
        found = []
        labels = res.get("text_labels", res.get("labels"))
        for box, score, label in zip(res["boxes"].tolist(), res["scores"].tolist(), labels):
            organ = organ_of_label(str(label), self.phrases)
            if organ is None:
                continue
            found.append((organ, [max(0.0, box[0] / w), max(0.0, box[1] / h), min(1.0, box[2] / w),
                                  min(1.0, box[3] / h)], float(score)))
        return found


class FakeDetector:
    device = "cpu"

    def __init__(self, defn: dict, device: str):
        self.organs = set(defn["params"]["phrases"])

    def find(self, im) -> list:
        im = im.convert("RGB").resize((32, 32))
        px = im.load()
        green = [(x, y) for x in range(32) for y in range(32)
                 if px[x, y][1] > 120 and px[x, y][1] > px[x, y][0] + 40 and px[x, y][1] > px[x, y][2] + 40]
        found = [("plant", [0.0, 0.0, 1.0, 1.0], 0.9)] if "plant" in self.organs else []
        if green and "leaf" in self.organs:
            xs, ys = [p[0] for p in green], [p[1] for p in green]
            found.append(("leaf", [min(xs) / 32, min(ys) / 32, (max(xs) + 1) / 32, (max(ys) + 1) / 32], 0.8))
        return found


def load_detector(name: str, defn: dict, device: str):
    return FakeDetector(defn, device) if name == "fake" else GroundingDino(defn, device)


# --- commands ---------------------------------------------------------------------------------------------------

def detect(store: Store, model: str = "gdino-tiny", organs=DEFAULT_ORGANS, *, taxa=None, batches=None,
           triage: str | None = None, all_images: bool = False, threshold: float = THRESHOLD,
           detect_px: int = DETECT_PX, device: str = "auto", limit: int | None = None, run: str | None = None,
           log=None) -> dict:
    """Detect organs in the images in scope that lack this detector's answer; return counts."""
    from PIL import Image
    from .vectors import load_image
    log = log or (lambda *a: print(*a, file=sys.stderr))
    defn = definition(model, list(organs), threshold, detect_px)
    aid = analyzer.register(store, defn)
    rows = scope(store, taxa, batches)
    tri, answers = (None, {}) if all_images else triage_answers(store, rows, triage)
    if not all_images:
        if not tri and not all(is_render(r) for r in rows.values()):
            raise SystemExit("no triage on these images: run `imagekit run triage ...` first, or pass --all")
        rows = {s: r for s, r in rows.items() if is_render(r) or (answers.get(s) or {}).get("usable")}
    todo = store.missing(aid, sorted(rows))
    if limit is not None:
        todo = todo[:limit]
    log(f"detect {model} (analyzer {aid}{', triage ' + tri if tri else ''}): {len(todo)} of {len(rows)} images")
    if not todo:
        return {"analyzer": aid, "detected": 0, "regions": 0}
    run = run or store.unsealed("run", f"detect-{model}-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    t = time.perf_counter()
    det = load_detector(model, defn, device)
    load_s = time.perf_counter() - t
    path = store.run_dir(run) / "analyses.jsonl"
    n_regions, secs, by_organ = 0, 0.0, {}
    for i, sha in enumerate(todo, 1):
        t = time.perf_counter()
        try:
            im = load_image(store.blob(sha))
        except Exception as err:  # noqa: BLE001 - one unreadable image does not stop a run
            log(f"  unreadable, skipped: {sha} ({err})")
            continue
        size = list(im.size)
        im.thumbnail((detect_px, detect_px), Image.Resampling.LANCZOS)
        regs = tidy(det.find(im), defn)
        dt = time.perf_counter() - t
        secs += dt
        row = {"sha256": sha, "region": 0, "analyzer": aid, "task": "detect", "status": "ok",
               "out": {"image": size, "regions": regs}, "run": run, "at": now(), "seconds": round(dt, 3)}
        append_jsonl(path, [row])
        store.index_analysis(row, run)
        n_regions += len(regs)
        for r in regs:
            by_organ[r["organ"]] = by_organ.get(r["organ"], 0) + 1
        if i % 20 == 0 or i == len(todo):
            store.commit()
            log(f"  {i}/{len(todo)} ({secs / i:.2f} s/image)")
    store.commit()
    info_path = store.run_dir(run) / "run.json"
    info = json.loads(info_path.read_text()) if info_path.exists() else {"run": run, "task": "detect", "created": now()}
    info.update({"updated": now(), "tool": f"imagekit {__version__}", "host": socket.gethostname(),
                 "analyzer": aid, "model": defn["model"], "revision": defn["model_revision"], "device": det.device,
                 "seconds_loading": round(load_s, 2)})
    info["rows"] = info.get("rows", 0) + len(todo)
    info["seconds"] = round(info.get("seconds", 0) + secs, 2)
    info_path.write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")
    return {"analyzer": aid, "run": run, "detected": len(todo), "regions": n_regions, "by organ": by_organ,
            "seconds per image": round(secs / len(todo), 3)}


def cmd_detect(args, store: Store) -> None:
    organs = [o.strip() for o in args.organs.split(",") if o.strip()]
    print(json.dumps(detect(store, args.model, organs, taxa=args.taxon, batches=args.batch, triage=args.triage,
                            all_images=args.all, threshold=args.threshold, detect_px=args.detect_px,
                            device=args.device, limit=args.limit, run=args.run), indent=1))


def cmd_crops(args, store: Store) -> None:
    rows = scope(store, args.taxon, args.batch)
    det = args.detector or default_detector(store, rows)
    if not det:
        raise SystemExit("no detections on these images: run `imagekit detect` first")
    tri, answers = triage_answers(store, rows, args.triage)
    found = regions(store, det, rows, answers if tri else None, set(args.organ or []), args.min_score)
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    lines, n = [], 0
    for sha in sorted(found):
        r = rows[sha]
        for reg in found[sha]:
            name = f"img-{sha[:10]}-r{reg['region']}-{reg['organ']}.jpg"
            crop(store, sha, reg["box"], args.pad, args.px).save(out / name, quality=90)
            src = r.get("source") or {}
            lines.append({"file": name, "sha256": sha, "region": reg["region"], "organ": reg["organ"],
                          "box": reg["box"], "score": reg["score"], "taxon": r["taxon"].get("name_as_given"),
                          "key": src.get("key"), "licence": src.get("licence"), "creator": src.get("creator"),
                          "detector": det, "triage": tri})
            n += 1
    (out / "crops.jsonl").write_text("".join(json.dumps(x, ensure_ascii=False) + "\n" for x in lines))
    by = {}
    for x in lines:
        by[x["organ"]] = by.get(x["organ"], 0) + 1
    print(json.dumps({"crops": n, "images": len(found), "by organ": by, "detector": det, "triage": tri,
                      "out": str(out)}, indent=1))
