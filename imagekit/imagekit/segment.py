"""`imagekit segment`: a mask for each detected organ, the whole plant's crown by default.

    imagekit detect --taxon "Asimina triloba"
    imagekit segment --taxon "Asimina triloba"                # SAM 2.1 on each kept whole-plant box

SAM 2.1 is prompted with the organ's box from `imagekit detect` and draws
the pixels that belong to it, so sky seen through a crown stays outside
the mask. Each mask is a 1-bit PNG in its run folder
(`masks/<sha256>-r<region>.png`, at the size the model saw) and an analysis
of that region (task `segment`):

    {"mask": "masks/<sha256>-r1.png", "size": [w, h], "image": [W, H], "area": 0.21, "fill": 0.55, "box": [...]}

`area` is the mask's share of the image, `fill` its share of the detector's
box, and `box` the mask's own bounds (fractions). The analyzer names the
model, the detector, the organs and the triage that kept them, so a mask
always says whose regions it is of. Only regions the triage kept are
segmented: the whole plant only in a whole-plant view (V1 to V3), every
region of a render.

The masks are what pixel measures read (crown outline, see-through and
gaps), through the same code for photos and renders.
"""
from __future__ import annotations

import json
import socket
import sys
import time

from . import __version__, analyzer
from .store import Store, append_jsonl, check_open, now

SEGMENTERS = {
    "sam2.1-tiny": {"repo": "facebook/sam2.1-hiera-tiny", "revision": "de431c4043854a71d8101e17995dfe596bf101a5"},
    "sam2.1-base": {"repo": "facebook/sam2.1-hiera-base-plus", "revision": "b7320756a13354e7530a63935656d35b2f91a290"},
    "sam2.1-large": {"repo": "facebook/sam2.1-hiera-large", "revision": "665f8e2ad61cf5f53d65644ff27c8ee525124610"},
    # Test model: the strongly green pixels inside the box. No torch.
    "fake": {"repo": "imagekit/fake-segmenter", "revision": "fake-1", "test": True},
}
MASK_PX = 1024


def register(sub) -> None:
    p = sub.add_parser("segment", help="a mask for each detected organ (default: the whole plant) that lacks one")
    p.add_argument("--model", default="sam2.1-tiny", help=f"one of {', '.join(public())} (default sam2.1-tiny)")
    p.add_argument("--organ", action="append", help="organs to mask (repeatable; default plant)")
    p.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    p.add_argument("--batch", nargs="*")
    p.add_argument("--detector", help="the detect analyzer (default: the one with most answers on these images)")
    p.add_argument("--triage", help="keep organs this triage saw (default: the one with most answers; 'none' keeps all)")
    p.add_argument("--mask-px", type=int, default=MASK_PX, help="long side the model sees (default %(default)s)")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--limit", type=int)
    p.add_argument("--run", help="run name (default segment-<model>-<date>)")
    p.set_defaults(func=cmd_segment)


def public() -> list[str]:
    return [k for k, v in SEGMENTERS.items() if not v.get("test")]


def definition(model: str, detector: str, organs, triage: str | None, mask_px: int = MASK_PX) -> dict:
    if model not in SEGMENTERS:
        raise SystemExit(f"unknown segmenter {model!r}: one of {', '.join(public())}")
    m = SEGMENTERS[model]
    return {"task": "segment", "model": m["repo"], "model_revision": m["revision"],
            "params": {"regions": {"detector": detector, "organs": sorted(organs), "triage": triage},
                       "mask_px": mask_px, "prompt": "box", "multimask": False}}


class Sam2:
    def __init__(self, defn: dict, device: str):
        import torch
        from transformers import Sam2Model, Sam2Processor
        from transformers.utils import logging as hf_logging
        from .vectors import pick_device
        self.torch, self.device = torch, pick_device(device)
        level = hf_logging.get_verbosity()
        hf_logging.set_verbosity_error()  # the checkpoint is a sam2_video one; its image model loads as intended
        self.proc = Sam2Processor.from_pretrained(defn["model"], revision=defn["model_revision"])
        self.model = Sam2Model.from_pretrained(defn["model"], revision=defn["model_revision"]).to(self.device).eval()
        hf_logging.set_verbosity(level)

    def mask(self, im, box):
        """A boolean H x W array: the pixels of the object in box (pixels x0, y0, x1, y1)."""
        import numpy as np
        with self.torch.inference_mode():
            inputs = self.proc(images=im, input_boxes=[[box]], return_tensors="pt").to(self.device)
            out = self.model(**inputs, multimask_output=False)
            m = self.proc.post_process_masks(out.pred_masks.cpu(), inputs["original_sizes"])[0]
        return np.asarray(m).reshape(im.size[1], im.size[0]) > 0.5


class FakeSegmenter:
    device = "cpu"

    def __init__(self, defn: dict, device: str):
        pass

    def mask(self, im, box):
        import numpy as np
        a = np.asarray(im.convert("RGB")).astype(int)
        green = (a[..., 1] > 120) & (a[..., 1] > a[..., 0] + 40) & (a[..., 1] > a[..., 2] + 40)
        inside = np.zeros(green.shape, dtype=bool)
        x0, y0, x1, y1 = (round(v) for v in box)
        inside[y0:y1, x0:x1] = True
        return green & inside


def segment(store: Store, model: str = "sam2.1-tiny", organs=("plant",), *, taxa=None, batches=None,
            detector: str | None = None, triage: str | None = None, mask_px: int = MASK_PX, device: str = "auto",
            limit: int | None = None, run: str | None = None, log=None) -> dict:
    """Mask the kept regions of these organs that lack this segmenter's mask; return counts."""
    import numpy as np
    from PIL import Image
    from .regions import default_detector, regions, scope, triage_answers
    from .vectors import load_image
    log = log or (lambda *a: print(*a, file=sys.stderr))
    rows = scope(store, taxa, batches)
    det = detector or default_detector(store, rows)
    if not det:
        raise SystemExit("no detections on these images: run `imagekit detect` first")
    tri, answers = triage_answers(store, rows, triage)
    defn = definition(model, det, organs, tri, mask_px)
    aid = analyzer.register(store, defn)
    found = regions(store, det, rows, answers if tri else None, set(organs))
    done = {(s, r) for s, r in store.db.execute("select sha256, region from analyses where analyzer=?", (aid,))}
    todo = [(sha, reg) for sha in sorted(found) for reg in found[sha] if (sha, reg["region"]) not in done]
    if limit is not None:
        todo = todo[:limit]
    log(f"segment {model} (analyzer {aid}, detector {det}{', triage ' + tri if tri else ''}): "
        f"{len(todo)} regions in {len(found)} images")
    if not todo:
        return {"analyzer": aid, "segmented": 0}
    run = run or store.unsealed("run", f"segment-{model}-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    (store.run_dir(run) / "masks").mkdir(parents=True, exist_ok=True)
    t = time.perf_counter()
    seg = FakeSegmenter(defn, device) if model == "fake" else Sam2(defn, device)
    load_s = time.perf_counter() - t
    secs, n, cached = 0.0, 0, (None, None)
    for sha, reg in todo:
        t = time.perf_counter()
        if cached[0] != sha:
            try:
                full = load_image(store.blob(sha))
            except Exception as err:  # noqa: BLE001 - one unreadable image does not stop a run
                log(f"  unreadable, skipped: {sha} ({err})")
                continue
            im = full.copy()
            im.thumbnail((mask_px, mask_px), Image.Resampling.LANCZOS)
            cached = (sha, (full.size, im))
        size, im = cached[1]
        w, h = im.size
        b = reg["box"]
        m = seg.mask(im, [b[0] * w, b[1] * h, b[2] * w, b[3] * h])
        rel = f"masks/{sha}-r{reg['region']}.png"
        Image.fromarray(m).convert("1").save(store.run_dir(run) / rel, optimize=True)
        ys, xs = np.nonzero(m)
        box_px = max(1.0, (b[2] - b[0]) * w * (b[3] - b[1]) * h)
        out = {"mask": rel, "size": [w, h], "image": list(size), "organ": reg["organ"],
               "area": round(float(m.mean()), 4), "fill": round(min(1.0, float(m.sum()) / box_px), 4),
               "box": [round(xs.min() / w, 4), round(ys.min() / h, 4), round((xs.max() + 1) / w, 4),
                       round((ys.max() + 1) / h, 4)] if len(xs) else None}
        dt = time.perf_counter() - t
        secs += dt
        row = {"sha256": sha, "region": reg["region"], "analyzer": aid, "task": "segment",
               "status": "ok" if len(xs) else "empty", "out": out, "run": run, "at": now(), "seconds": round(dt, 3)}
        append_jsonl(store.run_dir(run) / "analyses.jsonl", [row])
        store.index_analysis(row, run)
        n += 1
        if n % 20 == 0:
            store.commit()
            log(f"  {n}/{len(todo)} ({secs / n:.2f} s/region)")
    store.commit()
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "task": "segment", "created": now()}
    info.update({"updated": now(), "tool": f"imagekit {__version__}", "host": socket.gethostname(), "analyzer": aid,
                 "model": defn["model"], "revision": defn["model_revision"], "device": seg.device,
                 "seconds_loading": round(load_s, 2)})
    info["rows"] = info.get("rows", 0) + n
    info["seconds"] = round(info.get("seconds", 0) + secs, 2)
    p.write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")
    return {"analyzer": aid, "run": run, "segmented": n, "seconds per region": round(secs / max(n, 1), 3)}


def masks(store: Store, aid: str) -> dict:
    """(sha256, region) -> (the mask's path, its answer) under a segment analyzer."""
    from .store import jsonl
    out = {}
    for (run,) in store.db.execute("select distinct run from analyses where analyzer=?", (aid,)):
        for row in jsonl(store.run_dir(run) / "analyses.jsonl"):
            if row["analyzer"] == aid and row.get("status") == "ok":
                out[(row["sha256"], row["region"])] = (store.run_dir(run) / row["out"]["mask"], row["out"])
    return out


def cmd_segment(args, store: Store) -> None:
    print(json.dumps(segment(store, args.model, args.organ or ["plant"], taxa=args.taxon, batches=args.batch,
                             detector=args.detector, triage=args.triage, mask_px=args.mask_px, device=args.device,
                             limit=args.limit, run=args.run), indent=1))
