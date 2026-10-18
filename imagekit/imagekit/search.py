"""`imagekit search`: find images (or species) by words or by a photo, over the stored vectors.

Exact cosine search with numpy, no index. Several models are merged by
reciprocal-rank fusion (each image scores the sum of 1/(60 + its rank) in
each model). `--by-species` groups by taxon; a species scores the mean of
its best three images, so one lucky photo does not carry it.
"""
from __future__ import annotations

import base64
import html
import io
import sys
from hashlib import sha256 as _sha256
from pathlib import Path

from .store import Store, jsonl

RRF_K = 60
PER_SPECIES = 3


def register(subparsers) -> None:
    from . import vectors
    p = subparsers.add_parser("search", help="find images or species by words or by a photo (exact vector search)")
    p.add_argument("--model", required=True,
                   help=f"one or more of {', '.join(vectors.public_models())}, comma-separated (several: rank fusion)")
    q = p.add_mutually_exclusive_group(required=True)
    q.add_argument("--text", help=f"words; needs a model with a text encoder ({', '.join(vectors.text_models())})")
    q.add_argument("--image", help="a photo to find look-alikes of")
    p.add_argument("--box", help="with --image: search with this part of it, x0,y0,x1,y1 as fractions")
    p.add_argument("--k", type=int, default=20, help="results to show (images, or species with --by-species)")
    p.add_argument("--by-species", action="store_true", help="rank species by the mean of their best 3 images")
    p.add_argument("--exclude-genus", action="append", default=[], help="leave out this genus (repeatable)")
    p.add_argument("--template", default=vectors.TEXT_TEMPLATE, help="text prompt template (default %(default)r)")
    p.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    p.add_argument("--html", metavar="OUT.html", help="also write a contact sheet with thumbnails and credits")
    p.set_defaults(func=cmd_search)


def parse_box(text: str | None):
    if not text:
        return None
    box = [float(x) for x in text.split(",")]
    if len(box) != 4 or not (0 <= box[0] < box[2] <= 1 and 0 <= box[1] < box[3] <= 1):
        raise SystemExit("--box takes x0,y0,x1,y1 as fractions of width and height, e.g. 0.1,0.2,0.6,0.9")
    return box


def cmd_search(args, store: Store) -> None:
    models = [m.strip() for m in args.model.split(",") if m.strip()]
    res = search(store, models, text=args.text, image=args.image, box=parse_box(args.box), k=args.k,
                 by_species=args.by_species, exclude_genus=args.exclude_genus, template=args.template,
                 device=args.device)
    print(format_text(res))
    if args.html:
        Path(args.html).write_text(contact_sheet(store, res), encoding="utf-8")
        print(f"wrote {args.html}", file=sys.stderr)


# --- metadata ---------------------------------------------------------------------------------------------------

def image_info(store: Store) -> dict:
    """sha256 -> {taxon, genus, licence, creator, page}, from the first images.jsonl row of each image."""
    out = {}
    for b in store.batches():
        for r in jsonl(store.batch_dir(b) / "images.jsonl"):
            if r["sha256"] in out:
                continue
            t, s = r.get("taxon") or {}, r.get("source") or {}
            name = t.get("accepted_name") or t.get("name_as_given") or "(no taxon)"
            out[r["sha256"]] = {"taxon": name, "genus": t.get("genus") or name.split(" ")[0],
                                "licence": s.get("licence"), "creator": s.get("creator") or "unknown",
                                "page": s.get("page") or s.get("observation_url") or s.get("url") or "",
                                "attribution": s.get("attribution")}
    return out


# --- search -----------------------------------------------------------------------------------------------------

def search(store: Store, models, *, text=None, image=None, box=None, k=20, by_species=False, exclude_genus=(),
           template=None, device="auto", embedders=None) -> dict:
    """Rank the store's images for one query. embedders: {model: Embedder} to reuse loaded encoders."""
    import numpy as np
    from . import vectors
    if (text is None) == (image is None):
        raise SystemExit("give --text or --image")
    for name in models:
        m = vectors.preset(name)
        if text is not None and not m["text"]:
            raise SystemExit(f"{name} has no text encoder, so it cannot search by words. Text search works with "
                             f"{', '.join(vectors.text_models())}; use --image with {name}.")
    query_sha = None
    if image is not None:
        query_sha = _sha256(Path(image).read_bytes()).hexdigest()
    info = image_info(store)
    skip_genus = {g.lower() for g in exclude_genus}
    notes, per_model = [], {}
    for name in models:
        vs = vectors.load(store, name)
        if not len(vs):
            raise SystemExit(f"no vectors for {name} in this store: run `imagekit embed --model {name}` first")
        for aid, n in vs.others.items():
            notes.append(f"{name}: also {n} vectors under analyzer {aid}, not used")
        q = None
        if image is not None and box is None and (query_sha, 0) in vs.pos:
            q = vs.vector(query_sha)  # the query is in the store: its own vector, no model needed
        if q is None:
            params = vs.definition.get("params") or {}
            enc = (embedders or {}).get(name) or vectors.load_embedder(
                name, vs.definition.get("model_revision") or vectors.preset(name)["revision"], params.get("image_px"),
                device)
            if text is not None:
                q = enc.texts([(template or "{}").format(text)])[0]
            else:
                q = enc.images([vectors.load_image(image, None, box)])[0]
        scores = vs.scores(q)
        keep = np.array([sha != query_sha and info.get(sha, {}).get("genus", "").lower() not in skip_genus
                         for sha, _ in vs.ids], dtype=bool)
        per_model[name] = (vs, scores, keep)
    if query_sha and any((query_sha, 0) in vs.pos for vs, _, _ in per_model.values()):
        notes.append("the query photo itself is in the store and is left out")
    if skip_genus:
        notes.append(f"left out genus {', '.join(sorted(exclude_genus))}")
    if len(models) == 1:
        vs, scores, keep = per_model[models[0]]
        ranked = {vs.ids[i]: float(scores[i]) for i in np.flatnonzero(keep)}
        cos = {key: {models[0]: s} for key, s in ranked.items()}
    else:
        ranked, cos = fuse(per_model)
    hits = sorted(ranked.items(), key=lambda kv: -kv[1])
    images = [{"sha256": key[0], "region": key[1], "score": s, "cosine": cos[key], **info.get(key[0], {})}
              for key, s in hits]
    res = {"query": text if text is not None else f"image {image}" + (f" box {box}" if box else ""),
           "models": models, "fused": len(models) > 1, "by_species": by_species, "notes": notes}
    if by_species:
        res["species"] = group_species(images)[:k]
    else:
        res["images"] = images[:k]
    return res


def fuse(per_model: dict, k: int = RRF_K):
    """Reciprocal-rank fusion over every model's full ranking of the eligible images."""
    import numpy as np
    fused, cos = {}, {}
    for name, (vs, scores, keep) in per_model.items():
        idx = np.flatnonzero(keep)
        order = idx[np.argsort(-scores[idx], kind="stable")]
        for rank, i in enumerate(order, 1):
            key = vs.ids[i]
            fused[key] = fused.get(key, 0.0) + 1.0 / (k + rank)
            cos.setdefault(key, {})[name] = float(scores[i])
    return fused, cos


def group_species(images: list[dict]) -> list[dict]:
    """Species ranked by the mean score of their best three images (images arrive best first)."""
    groups: dict[str, list] = {}
    for im in images:
        groups.setdefault(im.get("taxon") or "(no taxon)", []).append(im)
    out = []
    for taxon, ims in groups.items():
        best = ims[:PER_SPECIES]
        out.append({"taxon": taxon, "score": sum(i["score"] for i in best) / len(best), "n": len(ims), "images": best})
    return sorted(out, key=lambda s: -s["score"])


# --- output -----------------------------------------------------------------------------------------------------

def _score(res, s) -> str:
    return f"{s:.4f}" if res["fused"] else f"{s:.3f}"


def format_text(res: dict) -> str:
    head = f"query: {res['query']}  models: {', '.join(res['models'])}" + ("  (rank fusion)" if res["fused"] else "")
    lines = [head] + [f"note: {n}" for n in res["notes"]]
    if res["by_species"]:
        for r, sp in enumerate(res["species"], 1):
            lines.append(f"{r:3d}  {_score(res, sp['score'])}  {sp['taxon']}  ({sp['n']} images)")
            for im in sp["images"]:
                lines.append(f"       {_score(res, im['score'])}  {im.get('licence')}  {im.get('creator')}  {im.get('page')}")
    else:
        for r, im in enumerate(res["images"], 1):
            lines.append(f"{r:3d}  {_score(res, im['score'])}  {im.get('taxon')}  {im.get('licence')}  "
                         f"{im.get('creator')}  {im.get('page')}")
    return "\n".join(lines)


def thumbnail(store: Store, sha: str, px: int = 256) -> str:
    from PIL import Image
    im = Image.open(store.blob(sha))
    im.draft("RGB", (px, px))
    im = im.convert("RGB")
    im.thumbnail((px, px))
    out = io.BytesIO()
    im.save(out, "JPEG", quality=78)
    return "data:image/jpeg;base64," + base64.b64encode(out.getvalue()).decode()


def _card(store, res, im, rank=None) -> str:
    e = html.escape
    cos = ", ".join(f"{m} {v:.3f}" for m, v in (im.get("cosine") or {}).items())
    page = im.get("page") or ""
    return (f'<figure><a href="{e(page)}" rel="noopener"><img src="{thumbnail(store, im["sha256"])}" alt="{e(im.get("taxon", ""))}"></a>'
            f'<figcaption>{f"<b>{rank}.</b> " if rank else ""}<i>{e(im.get("taxon", ""))}</i>'
            f'<br><span class="s">score {_score(res, im["score"])}{" (" + e(cos) + ")" if res["fused"] else ""}</span>'
            f'<br><span class="c">{e(im.get("creator") or "unknown")}, {e(im.get("licence") or "?")}</span>'
            f'<br><a href="{e(page)}" rel="noopener">source</a></figcaption></figure>')


def contact_sheet(store: Store, res: dict) -> str:
    """A self-contained page: thumbnails with taxon, score, credit and source link. A private viewing aid."""
    e = html.escape
    body = []
    if res["by_species"]:
        for r, sp in enumerate(res["species"], 1):
            body.append(f'<h2>{r}. <i>{e(sp["taxon"])}</i> <span class="s">{_score(res, sp["score"])}, '
                        f'{sp["n"]} images</span></h2><div class="grid">'
                        + "".join(_card(store, res, im) for im in sp["images"]) + "</div>")
    else:
        body.append('<div class="grid">' + "".join(_card(store, res, im, r) for r, im in enumerate(res["images"], 1))
                    + "</div>")
    notes = "".join(f"<li>{e(n)}</li>" for n in res["notes"])
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>imagekit search</title>
<style>
:root {{ --bg: #fbfaf7; --fg: #1d1d1b; --muted: #6b6a66; --card: #fff; --line: #e3e0d8; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #171716; --fg: #ecebe6; --muted: #a3a29c; --card: #222220; --line: #3a3936; }} }}
body {{ background: var(--bg); color: var(--fg); font: 14px/1.4 system-ui, sans-serif; margin: 0 auto; padding: 16px; max-width: 1400px; }}
h1 {{ font-size: 18px; }} h2 {{ font-size: 15px; margin: 22px 0 8px; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; }}
figure {{ margin: 0; background: var(--card); border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }}
img {{ display: block; width: 100%; height: 180px; object-fit: cover; }}
figcaption {{ padding: 6px 8px 8px; font-size: 12px; }}
.s, .c, .note {{ color: var(--muted); }} a {{ color: inherit; }}
</style></head><body>
<h1>{e(res["query"])}</h1>
<p class="note">models: {e(", ".join(res["models"]))}{" (reciprocal-rank fusion)" if res["fused"] else " (cosine)"}.
Photos are other people's work: credit as shown, follow each licence, and do not publish this page.</p>
<ul class="note">{notes}</ul>
{"".join(body)}
</body></html>
"""
