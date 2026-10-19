"""`imagekit render`: PlantGen renders of one species into a batch, to observe like photos.

    imagekit render asimina-triloba --age 15 --day 135
    imagekit render ferocactus-wislizeni --age 40 --day 200 --seeds 1,2,3 --plantc ~/code/plantgen/target/release/plantc

Makes photo-rules/03-match-renders.md's standing set at each seed: side,
45 degrees down, and three framed close-ups (a foliage shoot, a flowering
shoot or a second shoot lower down, a leaf underside; for cacti the ribs
at mid-height, a flower and the apex), plus the species' face-on cards
from `plantc atlas`, one image per card, for the face-on features. A
frame that is mostly sky or ground is dropped, as the rules say.

Each image goes into the store with source `render`, licence CC0 and a
`render` block (commit, generator revision, species id, seed, age, day,
env, view, focus, span, size, quality, command), so `imagekit compare`
can tell renders from photos and every finding can be made again. Then:

    imagekit run triage --batch <render batch> ...; imagekit run observe --batch <render batch> ...

with the same model as the photos.
"""
from __future__ import annotations

import json
import re
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from PIL import Image

from .fetch import Downloader
from .ingest import ingest
from .store import Store, check_open

SKY_GROUND = ((195, 208, 224), (160, 173, 136))  # the flat sky and ground of plantc's render look
ATLAS_BG = (203, 203, 203)
MIN_PLANT = 0.15  # a close-up with less plant than this is sky or ground: moved on, not observed
BLOOM_AT = (0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95)  # heights tried for the bloom close-up
MIN_FLOWER = 0.01  # a bloom close-up with less flower than this shows none: the fallback is used


def register(sub) -> None:
    p = sub.add_parser("render", help="PlantGen renders of one species into a batch, to observe like photos")
    p.add_argument("species", help="plantc species id, e.g. asimina-triloba")
    p.add_argument("--age", type=float, help="years: match the photos' age class (default the oldest keyframe)")
    p.add_argument("--day", type=int, action="append",
                   help="day of the year (1-365) whose stage matches the photos; repeat for each stage (default 196)")
    p.add_argument("--env", default="open", choices=["open", "edge", "interior", "suppressed"])
    p.add_argument("--seeds", default="1,2,3", help="comma-separated (default 1,2,3: three votes per view)")
    p.add_argument("--size", type=int, default=1536)
    p.add_argument("--quality", default="standard", choices=["draft", "standard"])
    p.add_argument("--views", default="side,oblique,closeups,atlas",
                   help="any of side, oblique, closeups, atlas (default all)")
    p.add_argument("--no-parts", action="store_true", help="close-ups without --parts on (large conifers can run "
                                                            "out of memory with it)")
    p.add_argument("--plantc", default="plantc", help="the plantc to render with (default plantc on PATH)")
    p.add_argument("--batch", help="batch name (default render-<species>-<date>)")
    p.add_argument("--dry-run", action="store_true", help="print the plantc commands, render nothing")
    p.set_defaults(func=cmd_render)


def checkout_of(plantc: str) -> Path | None:
    """The plantgen checkout a plantc binary was built in (target/release/plantc), if any."""
    exe = Path(plantc).expanduser()
    if not exe.is_file():
        found = subprocess.run(["sh", "-c", f"command -v {shlex.quote(plantc)}"], capture_output=True, text=True)
        exe = Path(found.stdout.strip()) if found.stdout.strip() else exe
    for d in exe.resolve().parents:
        if (d / "crates" / "plantgen").is_dir():
            return d
    return None


def provenance(plantc: str) -> dict:
    """commit and generator revision of the checkout plantc was built from; `dirty` if it had changes."""
    root = checkout_of(plantc)
    if root is None:
        return {}
    git = lambda *a: subprocess.run(["git", "-C", str(root), *a], capture_output=True, text=True).stdout.strip()  # noqa: E731
    out = {"commit": git("rev-parse", "HEAD")[:12] or "unknown"}
    if git("status", "--porcelain", "--untracked-files=no", "--", "crates", "apps"):
        out["commit"] += "-dirty"
    for src in sorted((root / "crates" / "plantgen" / "src").rglob("*.rs")):
        m = re.search(r"pub const GENERATOR_REVISION\s*:\s*u\w+\s*=\s*(\d+)", src.read_text())
        if m:
            out["generator_revision"] = int(m.group(1))
            break
    return out


def species_name(plantc: str, sid: str) -> tuple[str, str]:
    """(scientific name, program) from `plantc list`."""
    out = subprocess.run([plantc, "list"], capture_output=True, text=True).stdout
    m = re.search(rf"^\s*{re.escape(sid)}\s+.*?\(([^)]+)\), program `([^`]+)`", out, re.M)
    if not m:
        raise SystemExit(f"{sid} is not a species `{plantc} list` knows")
    return m.group(1), m.group(2)


def plant_share(path: Path) -> float:
    im = Image.open(path).convert("RGB").resize((128, 128))
    px = list(im.getdata())
    far = lambda p, q: sum((a - b) ** 2 for a, b in zip(p, q)) > 25 ** 2  # noqa: E731
    return sum(all(far(p, q) for q in SKY_GROUND) for p in px) / len(px)


def flower_share(path: Path) -> float:
    """Share of the frame that is flower-coloured: saturated and not green, or near white."""
    im = Image.open(path).convert("RGB").resize((128, 128))
    n = 0
    for r, g, b in im.getdata():
        hi, lo = max(r, g, b), min(r, g, b)
        n += (hi - lo > 70 and not (g >= r and g >= b)) or lo > 215
    return n / (128 * 128)


def bloom_focus(base: list, span: str, parts: list, tmp: Path) -> str | None:
    """The bloom:F whose draft close-up holds the most flower, or None when none shows any."""
    best = (MIN_FLOWER, None)
    probe = base[:]
    probe[probe.index("--out") + 1] = str(tmp / "probe.png")
    probe[probe.index("--size") + 1] = "256"
    probe[probe.index("--quality") + 1] = "draft"
    for f in BLOOM_AT:
        cmd = probe + ["--view", "side", "--focus", f"bloom:{f:g}", "--span", span, *parts]
        if subprocess.run(cmd, capture_output=True).returncode == 0 and (tmp / "probe.png").exists():
            share = flower_share(tmp / "probe.png")
            if share > best[0]:
                best = (share, f"bloom:{f:g}")
    return best[1]


def grown(plantc: str, sid: str, env: str, seed: int, age: float | None, day: int) -> dict:
    """Height and crown radius at that age, from a draft render's summary line."""
    with tempfile.TemporaryDirectory() as tmp:
        cmd = [plantc, "render", sid, "--out", f"{tmp}/g.png", "--env", env, "--seed", str(seed), "--day", str(day),
               "--size", "64", "--quality", "draft"] + (["--age", f"{age:g}"] if age else [])
        out = subprocess.run(cmd, capture_output=True, text=True).stdout + ""
    h = re.search(r"height\s+([\d.]+) m", out)
    r = re.search(r"crown radius\s+([\d.]+) m", out)
    a = re.search(r"age\s+([\d.]+)", out)
    return {"height": float(h.group(1)) if h else 1.0, "radius": float(r.group(1)) if r else 0.5,
            "age": float(a.group(1)) if a else age}


def shots(sid: str, cactus: bool, size: dict, views: set, parts: list) -> list[tuple[str, list, str | None]]:
    """(view name, extra plantc args, fallback focus) for one seed and day, after 03-match-renders.md §3."""
    out = []
    if "side" in views:
        out.append(("side", ["--view", "side"], None))
    if "oblique" in views:
        out.append(("oblique", ["--view", "oblique"], None))
    if "closeups" in views:
        h, r = size["height"], size["radius"]
        if cactus:
            out += [("ribs", ["--view", "side", "--focus", f"{r:.2f},{h / 2:.2f},0", "--span", "0.3", *parts], None),
                    ("bloom", ["--view", "side", "--focus", "bloom:0.9", "--span", "0.3", *parts], None),
                    ("apex", ["--view", "oblique", "--focus", f"0,{h:.2f},0", "--span", f"{max(0.2, 3 * r):.2f}", *parts],
                     None)]
        else:  # frames scale with the plant, so a herb's shoot and flower fill them as a tree's do
            shoot, bloom = f"{min(1.0, max(0.2, 0.4 * h)):.2f}", f"{min(0.3, max(0.1, 0.25 * h)):.2f}"
            out += [("shoot", ["--view", "side", "--focus", "leaf:0.7", "--span", shoot, *parts], None),
                    ("bloom", ["--view", "side", "--focus", "bloom:0.7", "--span", bloom, *parts], "leaf:0.4"),
                    ("under", ["--view", "under", "--focus", "leaf:0.5", "--span", "0.3"], None)]
    return out


def cut_atlas(path: Path, out_dir: Path) -> list[Path]:
    """One image per card: columns of the atlas separated by background."""
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    filled = [any(sum((a - b) ** 2 for a, b in zip(px[x, y], ATLAS_BG)) > 20 ** 2 for y in range(0, h, 2))
              for x in range(w)]
    spans, start = [], None
    for x, f in enumerate(filled + [False]):
        if f and start is None:
            start = x
        elif not f and start is not None:
            if x - start > 8:
                spans.append((start, x))
            start = None
    cards = []
    for i, (a, b) in enumerate(spans):
        pad = 6
        card = im.crop((max(0, a - pad), 0, min(w, b + pad), h))
        p = out_dir / f"{path.stem}-card{i}.png"
        card.save(p)
        cards.append(p)
    return cards


def cmd_render(args, store: Store) -> None:
    plantc = args.plantc
    name, program = species_name(plantc, args.species)
    cactus = program == "cactaceae"
    prov = provenance(plantc)
    if not prov:
        print("note: plantc is not inside a plantgen checkout; commit and revision left unknown", file=sys.stderr)
    batch = args.batch or store.unsealed("batch", f"render-{args.species}-{time.strftime('%Y-%m-%d')}")
    check_open(store.batch_dir(batch))
    views = {v.strip() for v in args.views.split(",") if v.strip()}
    parts = [] if args.no_parts else ["--parts", "on"]
    seeds = [int(s) for s in args.seeds.split(",") if s]
    days = args.day or [196]
    dl = Downloader(store, batch, max_mb=1e9, dry_run=False, allowed={"cc0"})
    dl.note_command(sys.argv[1:])
    work = Path(tempfile.mkdtemp(prefix="imagekit-render-"))
    made = dropped = 0

    def take(path: Path, block: dict, label: str) -> None:
        nonlocal made
        block = {**prov, **block}
        ident = f"{args.species}-{label}-{prov.get('commit', 'unknown')}"
        dl.take({"source": "render", "id": ident, "url": None, "path": str(path), "licence": "cc0",
                 "creator": "PlantGen", "taxon": name, "why": f"render for comparison ({label})",
                 "attribution": f"PlantGen render {ident}, CC0", "render": block})
        made += 1

    for day in days:
        for seed in seeds:
            size = grown(plantc, args.species, args.env, seed, args.age, day)
            for view, extra, fallback in shots(args.species, cactus, size, views, parts):
                label = f"{view}-s{seed}-d{day}"
                out = work / f"{label}.png"
                base = [plantc, "render", args.species, "--out", str(out), "--env", args.env, "--seed", str(seed),
                        "--day", str(day), "--size", str(args.size), "--quality", args.quality] + \
                    (["--age", f"{args.age:g}"] if args.age else [])
                cmd = base + extra
                if view == "bloom" and not cactus and not args.dry_run:  # the height where the flowers are
                    at = bloom_focus(base, extra[extra.index("--span") + 1], parts, work)
                    cmd[cmd.index("--focus") + 1] = at or fallback
                    if not at:
                        cmd[cmd.index("--span") + 1] = "1.0"
                        view = "shoot-low"
                if args.dry_run:
                    print(shlex.join(cmd))
                    continue
                ok = subprocess.run(cmd, capture_output=True, text=True).returncode == 0 and out.exists()
                if (not ok or (view not in ("side", "oblique") and plant_share(out) < MIN_PLANT)) and fallback:
                    cmd = base + extra
                    cmd[cmd.index("--focus") + 1] = fallback
                    if "--span" in cmd:
                        cmd[cmd.index("--span") + 1] = "1.0"
                    ok = subprocess.run(cmd, capture_output=True, text=True).returncode == 0 and out.exists()
                    view = "shoot-low"
                if not ok:
                    print(f"  {label}: plantc failed: {shlex.join(cmd)}", file=sys.stderr)
                    continue
                if view not in ("side", "oblique") and plant_share(out) < MIN_PLANT:
                    print(f"  {label}: mostly sky or ground, dropped", file=sys.stderr)
                    dropped += 1
                    continue
                block = {"species": args.species, "seed": seed, "age": size["age"], "day": day, "env": args.env,
                         "view": view, "size": args.size, "quality": args.quality, "command": shlex.join(cmd[1:])}
                if "--focus" in cmd:
                    at = cmd[cmd.index("--focus") + 1]
                    if "," in at:
                        block["focus"] = [float(x) for x in at.split(",")]
                    else:
                        block["focus_on"] = at  # leaf:F, bloom:F
                if "--span" in cmd:
                    block["span"] = float(cmd[cmd.index("--span") + 1])
                take(out, block, label)
    if "atlas" in views and not args.dry_run:
        out = work / "atlas.png"
        cmd = [plantc, "atlas", args.species, "--out", str(out)]
        if subprocess.run(cmd, capture_output=True, text=True).returncode == 0 and out.exists():
            for i, card in enumerate(cut_atlas(out, work)):
                take(card, {"species": args.species, "seed": 0, "age": 0, "day": days[0], "env": args.env,
                            "view": "atlas", "command": shlex.join(cmd[1:]) + f"  (card {i})"}, f"atlas-card{i}")
    if args.dry_run:
        return
    print(f"rendered {made} images of {name} ({dropped} close-ups dropped as sky or ground) into batch {batch}",
          file=sys.stderr)
    tally = ingest(store, batch)
    print(json.dumps({"batch": batch, "taxon": name, **{k: v for k, v in tally.items()}}))
