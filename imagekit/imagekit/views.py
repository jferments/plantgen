"""`imagekit views`: usable photos per view and taxon against photo-rules' targets.

    imagekit views                       # every taxon with a triage answer
    imagekit views --taxon "Ferocactus wislizeni" --json

Photos are taken by view, to a target per view (photo-rules/SOURCES.md),
not at random. This reads a triage run's answers (the analyzer with most
answers unless --analyzer names one, as `run observe` does) and counts, per
taxon, the usable photos showing each view, how many photographers took
them, and how many more each view needs.
"""
from __future__ import annotations

import json

from .run import default_triage, image_rows, outputs
from .store import Store

PHOTOGRAPHERS = 3  # a view's photos come from at least this many people


def _has(out: dict, views=(), organs=(), faces=()) -> bool:
    return (not views or bool(set(out.get("views") or []) & set(views))) \
        and (not organs or bool(set(out.get("organs_visible") or []) & set(organs))) \
        and (not faces or out.get("leaf_face") in faces)


# (name, which plants, target, test on one usable triage answer), as in photo-rules/SOURCES.md
TARGETS = [
    ("V1 whole plant", "all", 8, lambda o: _has(o, ["V1"])),
    ("V2 leafless (deciduous)", "leafy", 5, lambda o: _has(o, ["V2"])),
    ("V4 shoot", "leafy", 8, lambda o: _has(o, ["V4"])),
    ("V5 leaf face-on", "leafy", 10, lambda o: _has(o, ["V5"])),
    ("leaf underside", "leafy", 5, lambda o: _has(o, ["V4", "V5"], faces=("lower", "both"))),
    ("V6 flower face-on", "all", 8, lambda o: _has(o, ["V6"])),
    ("V7 flower side", "all", 6, lambda o: _has(o, ["V7"])),
    ("V8 fruit or cone", "all", 8, lambda o: _has(o, ["V8"], ["fruit", "cone"])),
    ("V8 bark or stem", "all", 5, lambda o: _has(o, ["V8"], ["bark", "stem"])),
    ("apex (cacti)", "cacti", 5, lambda o: _has(o, ["V3", "V8"], ["apex"])),
    ("areole close-up (cacti)", "cacti", 8, lambda o: _has(o, ["V8"], ["areole"])),
]


def register(sub) -> None:
    p = sub.add_parser("views", help="usable photos per view and taxon, against the per-view targets")
    p.add_argument("--analyzer", help="the triage analyzer to read (default: the one with most answers)")
    p.add_argument("--taxon", help="only this taxon (name as fetched, or accepted name)")
    p.add_argument("--json", action="store_true", help="print JSON instead of a table")
    p.set_defaults(func=cmd_views)


def usable(out: dict) -> bool:
    return bool(out.get("usable")) and out.get("cultivar_look") != "yes"


def tally(store: Store, analyzer: str, taxon: str | None = None) -> dict:
    """taxon -> {"triaged", "usable", "cactus", "views": [{view, have, photographers, target, need}]}."""
    rows = image_rows(store)
    groups: dict[str, list] = {}
    for sha, out in outputs(store, analyzer).items():
        row = rows.get(sha)
        if row is None:
            continue
        t = row["taxon"]
        name = t.get("accepted_name") or t.get("name_as_given") or "(no taxon)"
        if taxon and taxon not in (name, t.get("name_as_given")):
            continue
        groups.setdefault(name, []).append((row, out))
    report = {}
    for name, items in sorted(groups.items()):
        good = [(r, o) for r, o in items if usable(o)]
        cactus = any(r["taxon"].get("family") == "Cactaceae" for r, _ in items) \
            or any("areole" in (o.get("organs_visible") or []) for _, o in good)
        views = []
        for view, plants, target, test in TARGETS:
            if plants == ("leafy" if cactus else "cacti"):
                continue
            hits = [r for r, o in good if test(o)]
            people = {r["source"].get("creator") or r["source"]["key"] for r in hits}
            views.append({"view": view, "have": len(hits), "photographers": len(people), "target": target,
                          "need": max(0, target - len(hits))})
        report[name] = {"triaged": len(items), "usable": len(good), "cactus": cactus, "views": views}
    return report


def cmd_views(args, store: Store) -> None:
    aid = args.analyzer or default_triage(store)
    if not aid:
        raise SystemExit("no triage answers yet: imagekit run triage --backend ...")
    report = tally(store, aid, args.taxon)
    if args.json:
        print(json.dumps({"analyzer": aid, "taxa": report}, indent=1, ensure_ascii=False))
        return
    print(f"triage analyzer {aid}")
    for name, r in report.items():
        print(f"\n{name}: {r['usable']} usable of {r['triaged']} triaged" + (" (cactus targets)" if r["cactus"] else ""))
        print(f"  {'view':26} {'have':>5} {'target':>7} {'need':>5} {'photographers':>14}")
        for v in r["views"]:
            few = "  (fewer than 3)" if v["have"] and v["photographers"] < PHOTOGRAPHERS else ""
            print(f"  {v['view']:26} {v['have']:5} {v['target']:7} {v['need']:5} {v['photographers']:14}{few}")
