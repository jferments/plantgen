"""`imagekit count`: count the parts of each detected organ by pointing at them.

    imagekit detect --taxon "Echinocereus" --organs flower
    imagekit count --taxon "Echinocereus" --organ flower --what "the stamens of this flower"   # Molmo2, local GPU
    imagekit count ... --backend vllm --model Qwen/Qwen3-VL-8B-Instruct                      # any vision backend
    imagekit count ... --backend chat --model "Claude Opus 5.5 subagent" --chat-dir D        # free observers

Each organ region the triage kept (`imagekit detect`) is cropped with a
small margin, and the model puts one point on every part `--what` names.
A count is the number of points, so it can be checked against the crop:

    {"organ": "flower", "what": "...", "count": 52, "points": [[x, y], ...], "crop": [x0, y0, x1, y1]}

`points` are fractions of the crop, `crop` is the crop's box in the image
(fractions). A row or column of evenly spaced points is the model listing,
not looking: it is kept with `"degenerate": true` and no count. Counting
from masks failed in the daisy pilot (rays merge), so counts come from
pointing only. Each `--what` is its own analyzer, so a new question never
hides an old answer.
"""
from __future__ import annotations

import hashlib
import io
import json
import re
import socket
import sys
import time
from pathlib import Path

from . import __version__, analyzer
from .store import Store, append_jsonl, check_open, is_render, now

POINTERS = {
    "molmo2-8b": {"repo": "allenai/Molmo2-8B", "revision": "e28fa28597e5ec5e0cca2201dd8ab33d48bc4a1b"},
    # Test model: one point per strongly green blob row. No torch.
    "fake": {"repo": "imagekit/fake-pointer", "revision": "fake-1", "test": True},
}
CROP_PX = 1024
PAD = 0.08


def prompt(what: str) -> str:
    """For vision models without native pointing (Qwen-VL, API models, chat observers)."""
    return (f"Point to each of {what}. Give every one its own point, even where they overlap or are partly hidden; "
            "point only at ones you can see, and do not expect any particular number. Answer with one JSON object "
            "and nothing else:\n"
            '{"points": [[x, y], ...], "count": N, "face_on": true}\n'
            "x and y are positions in the image from 0 to 1000 (0,0 is the top left corner). face_on is true when "
            "you look straight into the organ (a flower or head from the front), so that none of its parts hides "
            "behind another, and false when it is seen from the side or at a slant. "
            'If the image does not show them at all, answer {"points": [], "count": null}.')


SCHEMA = {"type": "object", "additionalProperties": False, "required": ["points", "count"],
          "properties": {"points": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}},
                         "count": {"type": ["integer", "null"]}, "face_on": {"type": "boolean"}}}

_PAIR = re.compile(r"\[\s*(-?[0-9]+(?:\.[0-9]+)?)\s*,\s*(-?[0-9]+(?:\.[0-9]+)?)\s*\]")


def points_from_text(text: str) -> list[list[float]]:
    """Every [x, y] pair in an answer (0-1000 scale), less near-duplicates. Works on cut-off JSON too."""
    pts: list[list[float]] = []
    for m in _PAIR.finditer(text or ""):
        x, y = float(m.group(1)), float(m.group(2))
        if 0 <= x <= 1000 and 0 <= y <= 1000 and all((x - a) ** 2 + (y - b) ** 2 > 4 for a, b in pts):
            pts.append([x, y])
    return pts


def degenerate(pts) -> bool:
    """A row or column of evenly spaced points: the model listing, not looking."""
    if len(pts) < 6:
        return False

    def spread(v):
        m = sum(v) / len(v)
        return (sum((a - m) ** 2 for a in v) / len(v)) ** 0.5
    return spread([p[0] for p in pts]) < 3 or spread([p[1] for p in pts]) < 3


def answer(text: str, organ: str, what: str, crop_box) -> tuple[str, dict]:
    """(status, out) from a model's text. The points decide the count; a stated count alone is kept apart."""
    pts = points_from_text(text)
    said = None
    m = re.search(r'"count"\s*:\s*(null|[0-9]+)', text or "")
    if m and m.group(1) != "null":
        said = int(m.group(1))
    bad = degenerate(pts)
    out = {"organ": organ, "what": what, "count": None if bad else len(pts),
           "points": [[round(x / 1000, 4), round(y / 1000, 4)] for x, y in pts], "crop": crop_box}
    if bad:
        out["degenerate"] = True
    f = re.search(r'"face_on"\s*:\s*(true|false)', text or "")
    if f:
        out["face_on"] = f.group(1) == "true"
    if said is not None and said != len(pts):
        out["count_stated"] = said
    if m and m.group(1) == "null" and not pts:
        out["count"] = None  # the crop does not show them
        return "empty", out
    return ("ok" if pts or said == 0 else "empty"), out


class Molmo2:
    COORDS = re.compile(r'<(?:points|tracks)[^>]*?coords="([0-9\t:;, .]+)"')
    POINT = re.compile(r"([0-9]+) ([0-9]{1,4}) ([0-9]{1,4})")

    def __init__(self, defn: dict, device: str):
        import torch
        from transformers import AutoModelForImageTextToText, AutoProcessor

        from .vectors import pick_device
        self.torch, self.device = torch, pick_device(device)
        dtype = torch.bfloat16 if self.device == "cuda" else torch.float32
        self.proc = AutoProcessor.from_pretrained(defn["model"], revision=defn["model_revision"], trust_remote_code=True)
        self.model = AutoModelForImageTextToText.from_pretrained(
            defn["model"], revision=defn["model_revision"], trust_remote_code=True, dtype=dtype,
            device_map="auto" if self.device == "cuda" else None).eval()

    def ask(self, im, what: str) -> str:
        """Molmo2's answer, rewritten as a points JSON on the 0-1000 scale."""
        messages = [{"role": "user", "content": [{"type": "text", "text": f"Point to {what}."},
                                                 {"type": "image", "image": im}]}]
        with self.torch.inference_mode():
            inputs = self.proc.apply_chat_template(messages, tokenize=True, add_generation_prompt=True,
                                                   return_tensors="pt", return_dict=True)
            inputs = {k: v.to(self.model.device) for k, v in inputs.items()}
            out = self.model.generate(**inputs, max_new_tokens=2048, do_sample=False)
        text = self.proc.tokenizer.decode(out[0, inputs["input_ids"].size(1):], skip_special_tokens=True)
        pts = [[float(m.group(2)), float(m.group(3))]
               for c in self.COORDS.finditer(text) for m in self.POINT.finditer(c.group(1))]
        return json.dumps({"points": pts, "count": len(pts), "raw": text[:4000]})


class FakePointer:
    """One point at the centre of each run of strongly green rows: enough to test the plumbing."""
    device = "cpu"

    def __init__(self, defn: dict, device: str):
        pass

    def ask(self, im, what: str) -> str:
        import numpy as np
        a = np.asarray(im.convert("RGB").resize((64, 64))).astype(int)
        green = ((a[..., 1] > 120) & (a[..., 1] > a[..., 0] + 40) & (a[..., 1] > a[..., 2] + 40)).any(axis=1)
        pts, start = [], None
        for y, g in enumerate([*green, False]):
            if g and start is None:
                start = y
            elif not g and start is not None:
                pts.append([500 + 10 * len(pts), (start + y) / 2 / 64 * 1000])
                start = None
        return json.dumps({"points": pts, "count": len(pts)})


class BackendPointer:
    """Any imagekit backend (openai-compatible or Claude): the crop and the pointing prompt."""

    def __init__(self, backend):
        self.backend, self.device = backend, "remote"

    def ask(self, im, what: str) -> str:
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=90)
        r = self.backend.ask("You count plant parts in photographs by pointing at each one.", [prompt(what)],
                             buf.getvalue(), SCHEMA)
        return r.text


def definition(model: str, revision: str, what: str, organ: str, detector: str, triage: str | None,
               crop_px: int = CROP_PX, pad: float = PAD, route: str = "local") -> dict:
    return {"task": "count", "model": model, "model_revision": revision,
            "params": {"what": what, "organ": organ, "route": route,
                       "regions": {"detector": detector, "triage": triage}, "crop_px": crop_px, "pad": pad,
                       "prompt_hash": hashlib.sha256(prompt(what).encode()).hexdigest()[:16]}}


def crop_box(box, pad: float) -> list[float]:
    """The region's box with a margin of pad x its long side, in image fractions (an approximation for
    non-square images: the margin is taken on each axis's own fraction)."""
    m = pad * max(box[2] - box[0], box[3] - box[1])
    return [round(max(0.0, box[0] - m), 4), round(max(0.0, box[1] - m), 4),
            round(min(1.0, box[2] + m), 4), round(min(1.0, box[3] + m), 4)]


def count(store: Store, what: str, organ: str, *, pointer: str | None = None, backend: str | None = None,
          model: str | None = None, chat_dir: str | None = None, taxa=None, batches=None, detector: str | None = None,
          triage: str | None = None, crop_px: int = CROP_PX, pad: float = PAD, device: str = "auto",
          limit: int | None = None, run: str | None = None, log=None) -> dict:
    """Count `what` in each kept `organ` region that lacks this counter's answer; return counts."""
    from .regions import crop, default_detector, regions, scope, triage_answers
    log = log or (lambda *a: print(*a, file=sys.stderr))
    rows = scope(store, taxa, batches)
    det = detector or default_detector(store, rows)
    if not det:
        raise SystemExit("no detections on these images: run `imagekit detect` first")
    tri, answers = triage_answers(store, rows, triage)
    be = None
    if backend:
        from . import backends
        be = backends.load(backend, {"model": model})
        if be is None and not chat_dir:
            raise SystemExit(f"the {backend} backend needs --chat-dir")
        name, rev, route = (model or "chat", model or "chat", backend) if be is None else (be.model, be.revision, backend)
    else:
        pointer = pointer or "molmo2-8b"
        if pointer not in POINTERS:
            raise SystemExit(f"unknown pointer {pointer!r}: one of {', '.join(k for k, v in POINTERS.items() if not v.get('test'))}")
        name, rev, route = POINTERS[pointer]["repo"], POINTERS[pointer]["revision"], "local"
    defn = definition(name, rev, what, organ, det, tri, crop_px, pad, route)
    aid = analyzer.register(store, defn)
    found = regions(store, det, rows, answers if tri else None, {organ})
    done = {(s, r) for s, r in store.db.execute("select sha256, region from analyses where analyzer=?", (aid,))}
    todo = [(sha, reg) for sha in sorted(found) for reg in found[sha] if (sha, reg["region"]) not in done]
    if limit is not None:
        todo = todo[:limit]
    log(f"count {what!r} on {organ} with {name} (analyzer {aid}, detector {det}{', triage ' + tri if tri else ''}): "
        f"{len(todo)} regions in {len(found)} images")
    if not todo:
        return {"analyzer": aid, "counted": 0}
    run = run or store.unsealed("run", f"count-{organ}-{time.strftime('%Y-%m-%d')}")
    check_open(store.run_dir(run))
    if backend and be is None:
        return _chat(store, aid, defn, todo, Path(chat_dir).expanduser(), run, crop_px, pad, log)
    t = time.perf_counter()
    ptr = BackendPointer(be) if be else (FakePointer if pointer == "fake" else Molmo2)(
        {"model": name, "model_revision": rev}, device)
    load_s = time.perf_counter() - t
    secs, n, by = 0.0, 0, {"ok": 0, "empty": 0, "errors": 0}
    for sha, reg in todo:
        t = time.perf_counter()
        cb = crop_box(reg["box"], pad)
        try:
            im = crop(store, sha, cb, 0.0, crop_px)
            text = ptr.ask(im, what)
        except Exception as err:  # noqa: BLE001 - one bad image or reply does not stop a run; a rerun retries it
            log(f"  {sha[:12]} r{reg['region']}: {err}")
            by["errors"] += 1
            continue
        status, out = answer(text, organ, what, cb)
        dt = time.perf_counter() - t
        secs += dt
        _record(store, run, aid, sha, reg["region"], status, out, dt)
        by[status] += 1
        n += 1
        if n % 20 == 0:
            store.commit()
            log(f"  {n}/{len(todo)} ({secs / n:.2f} s/region)")
    store.commit()
    _run_json(store, run, aid, defn, {"device": ptr.device, "seconds_loading": round(load_s, 2)}, n, secs)
    return {"analyzer": aid, "run": run, "counted": n, **by, "seconds per region": round(secs / max(n, 1), 3)}


def _record(store: Store, run: str, aid: str, sha: str, region: int, status: str, out: dict, dt: float | None):
    row = {"sha256": sha, "region": region, "analyzer": aid, "task": "count", "status": status, "out": out,
           "run": run, "at": now(), **({"seconds": round(dt, 3)} if dt is not None else {})}
    append_jsonl(store.run_dir(run) / "analyses.jsonl", [row])
    store.index_analysis(row, run)


def _run_json(store: Store, run: str, aid: str, defn: dict, extra: dict, n: int, secs: float):
    p = store.run_dir(run) / "run.json"
    info = json.loads(p.read_text()) if p.exists() else {"run": run, "task": "count", "created": now()}
    info.update({"updated": now(), "tool": f"imagekit {__version__}", "host": socket.gethostname(), "analyzer": aid,
                 "model": defn["model"], "revision": defn["model_revision"], "what": defn["params"]["what"], **extra})
    info["rows"] = info.get("rows", 0) + n
    info["seconds"] = round(info.get("seconds", 0) + secs, 2)
    p.write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")


def _chat(store: Store, aid: str, defn: dict, todo, d: Path, run: str, crop_px: int, pad: float, log) -> dict:
    """First call: write each crop and the prompt to d. Later calls: read back <id>.json answers."""
    from .regions import crop
    d.mkdir(parents=True, exist_ok=True)
    what, organ = defn["params"]["what"], defn["params"]["organ"]
    (d / "00-system.md").write_text(
        "You count plant parts in photographs by pointing at each one. Read prompt.md once; then for each "
        "<id>.jpg here, look at it and write <id>.json with your answer. Look at every image on its own: "
        "never answer several images at once or copy an answer.\n")
    (d / "prompt.md").write_text(prompt(what) + "\n")
    got = 0
    for sha, reg in todo:
        rid = f"{sha[:12]}-r{reg['region']}"
        cb = crop_box(reg["box"], pad)
        ans = d / f"{rid}.json"
        if ans.exists():
            status, out = answer(ans.read_text(), organ, what, cb)
            _record(store, run, aid, sha, reg["region"], status, out, None)
            got += 1
        elif not (d / f"{rid}.jpg").exists():
            crop(store, sha, cb, 0.0, crop_px).save(d / f"{rid}.jpg", "JPEG", quality=90)
    store.commit()
    if got:
        _run_json(store, run, aid, defn, {"device": "chat"}, got, 0.0)
    log(f"{got} answers read back; {len(todo) - got} crops waiting in {d}")
    return {"analyzer": aid, "run": run, "counted": got, "waiting": len(todo) - got}


def counts(store: Store, aid: str, rows: dict | None = None, face_on: bool = False) -> dict:
    """taxon -> {"photos": [counts], "renders": [counts]} under a count analyzer (ok answers with a count); with
    face_on, only the organs seen face-on, where no part hides behind another."""
    from .run import image_rows
    from .store import jsonl
    rows = rows if rows is not None else image_rows(store)
    out: dict = {}
    for (run,) in store.db.execute("select distinct run from analyses where analyzer=?", (aid,)):
        for row in jsonl(store.run_dir(run) / "analyses.jsonl"):
            if row["analyzer"] != aid or row.get("status") != "ok" or row["out"].get("count") is None:
                continue
            if face_on and not row["out"].get("face_on"):
                continue
            r = rows.get(row["sha256"])
            if not r:
                continue
            taxon = r["taxon"].get("accepted_name") or r["taxon"].get("name_as_given")
            side = "renders" if is_render(r) else "photos"
            out.setdefault(taxon, {"photos": [], "renders": []})[side].append(row["out"]["count"])
    return out


def summary(by_taxon: dict) -> dict:
    def stats(v):
        if not v:
            return None
        s = sorted(v)
        return {"n": len(s), "median": s[len(s) // 2] if len(s) % 2 else (s[len(s) // 2 - 1] + s[len(s) // 2]) / 2,
                "min": s[0], "max": s[-1]}
    return {t: {k: stats(v) for k, v in sides.items()} for t, sides in sorted(by_taxon.items())}


def register(sub) -> None:
    p = sub.add_parser("count", help="count the parts of each detected organ by pointing at them")
    p.add_argument("--what", help='what to point at, e.g. "the stamens of this flower"')
    p.add_argument("--organ", help="the detected organ whose regions are counted (e.g. flower)")
    p.add_argument("--pointer", help="a local pointing model: molmo2-8b (default when no --backend)")
    p.add_argument("--backend", help="or any vision backend (vllm, openrouter, claude, chat, or one in backends.toml)")
    p.add_argument("--model", help="the backend's model")
    p.add_argument("--chat-dir", help="for --backend chat: where crops and answers go")
    p.add_argument("--taxon", action="append", help="only images of this taxon (repeatable)")
    p.add_argument("--batch", nargs="*")
    p.add_argument("--detector", help="the detect analyzer (default: the one with most answers on these images)")
    p.add_argument("--triage", help="keep organs this triage saw (default: the one with most answers; 'none' keeps all)")
    p.add_argument("--crop-px", type=int, default=CROP_PX, help="long side of each crop (default %(default)s)")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--limit", type=int)
    p.add_argument("--run", help="run name (default count-<organ>-<date>)")
    p.add_argument("--summary", metavar="ANALYZER", help="print counts per taxon, photos and renders, and stop")
    p.add_argument("--face-on", action="store_true", help="with --summary: only organs seen face-on")
    p.set_defaults(func=cmd_count)


def cmd_count(args, store: Store) -> None:
    if args.summary:
        print(json.dumps(summary(counts(store, args.summary, face_on=args.face_on)), indent=1))
        return
    if not (args.what and args.organ):
        raise SystemExit("count needs --what and --organ (or --summary ANALYZER)")
    print(json.dumps(count(store, args.what, args.organ, pointer=args.pointer, backend=args.backend, model=args.model,
                           chat_dir=args.chat_dir, taxa=args.taxon, batches=args.batch, detector=args.detector,
                           triage=args.triage, crop_px=args.crop_px, device=args.device, limit=args.limit,
                           run=args.run)))
