"""`imagekit tally`: one task's answers counted per taxon, photos against renders.

    imagekit tally branching --taxon "Asimina triloba" --out ~/pawpaw-branching

For tasks whose answers are characters rather than features to compare
(`09-branching.md`): each character's states counted with one vote per
observation (several photos of one plant on one day are one vote, their
majority), numbers as a median and quartiles, and the renders counted
apart. `cannot tell` and `not applicable` are left out of the counts and
reported as how many images could not show the character. A tally backs or
challenges a trait value at the rank where it holds; one photo decides
nothing, so a character with fewer than MIN_VOTES votes is marked thin.
"""
from __future__ import annotations

import collections
import json
import statistics
from pathlib import Path

from .store import Store

MIN_VOTES = 5
UNKNOWN = {"cannot tell", "not applicable"}


def register(sub) -> None:
    p = sub.add_parser("tally", help="count one task's answers per taxon, photos against renders (e.g. branching)")
    p.add_argument("task", choices=["branching"])
    p.add_argument("--taxon", action="append", required=True, help="repeatable")
    p.add_argument("--analyzer", help="the task's analyzer (default: the one with most answers on the taxon)")
    p.add_argument("--out", help="folder for TALLY.md and tally.json (default: print)")
    p.set_defaults(func=cmd_tally)


def pick(store: Store, task: str, shas) -> str | None:
    shas, n = set(shas), collections.Counter()
    for aid, sha in store.db.execute("select analyzer, sha256 from analyses where task=? and status='ok' and region=0",
                                     (task,)):
        if sha in shas:
            n[aid] += 1
    return n.most_common(1)[0][0] if n else None


def count(items: list[dict], character: str) -> dict:
    """{votes, images, unknown, states: {state: votes}} or, for numbers, {votes, images, unknown, median, q1, q3}."""
    by_vote: dict[str, list] = collections.defaultdict(list)
    unknown = 0
    for it in items:
        v = (it["answers"].get(character) or {}).get("value")
        if v is None or v in UNKNOWN:
            unknown += 1
            continue
        by_vote[it["vote"]].append(v)
    out = {"images": sum(len(v) for v in by_vote.values()), "votes": len(by_vote), "unknown": unknown}
    if not by_vote:
        return out
    if all(isinstance(v, (int, float)) for vs in by_vote.values() for v in vs):
        xs = sorted(statistics.median(vs) for vs in by_vote.values())
        q = statistics.quantiles(xs, n=4) if len(xs) > 1 else [xs[0], xs[0], xs[0]]
        out.update({"median": round(statistics.median(xs), 3), "q1": round(q[0], 3), "q3": round(q[2], 3)})
        return out
    states = collections.Counter()
    for vs in by_vote.values():
        top = collections.Counter(map(str, vs)).most_common()
        if len(top) > 1 and top[0][1] == top[1][1]:
            states["(split)"] += 1  # one plant's photos disagree evenly
        else:
            states[top[0][0]] += 1
    out["states"] = dict(states.most_common())
    return out


def tally(items: list[dict], characters) -> dict:
    photos = [i for i in items if i["kind"] == "photo"]
    renders = [i for i in items if i["kind"] == "render"]
    return {c: {"photos": count(photos, c), "renders": count(renders, c)} for c in characters}


def show(c: dict) -> str:
    if not c.get("votes"):
        return f"none (unknown {c['unknown']})"
    if "median" in c:
        body = f"{c['median']} ({c['q1']}-{c['q3']})"
    else:
        body = ", ".join(f"{s} {n}" for s, n in c["states"].items())
    thin = " thin" if c["votes"] < MIN_VOTES else ""
    return f"{body}; {c['votes']} votes{thin}, unknown {c['unknown']}"


def leader(c: dict):
    if "median" in c:
        return c["median"]
    states = [s for s in (c.get("states") or {}) if s != "(split)"]
    return states[0] if states else None


def markdown(taxon: str, aid: str, res: dict, n_photos: int, n_renders: int) -> str:
    lines = [f"# {taxon}: branching tally", "",
             f"Analyzer {aid}: {n_photos} photos and {n_renders} renders with an answer. One vote per observation; "
             f"fewer than {MIN_VOTES} votes is thin.", "",
             "| Character | Photos | Renders | Same lead? |", "| --- | --- | --- | --- |"]
    for c, r in res.items():
        ph, rd = r["photos"], r["renders"]
        same = ""
        if ph.get("votes") and rd.get("votes"):
            a, b = leader(ph), leader(rd)
            if isinstance(a, (int, float)) and isinstance(b, (int, float)):
                same = "yes" if ph["q1"] <= b <= ph["q3"] else "no"
            else:
                same = "yes" if a == b else "no"
        lines.append(f"| {c} | {show(ph)} | {show(rd)} | {same} |")
    return "\n".join(lines) + "\n"


def cmd_tally(args, store: Store) -> None:
    from .compare import items_from_store, taxon_names
    from .prompts import Rules
    from .run import image_rows
    schema = Rules().schema(args.task)
    characters = sorted(schema["properties"]["answers"]["properties"])
    rows = image_rows(store)
    report, texts = {}, []
    for taxon in args.taxon:
        shas = [s for s, r in rows.items() if taxon in taxon_names(r)]
        aid = args.analyzer or pick(store, args.task, shas)
        if not aid:
            raise SystemExit(f"no {args.task} answers for {taxon}: run `imagekit run {args.task} ...` first")
        items = items_from_store(store, taxon, aid, None)
        res = tally(items, characters)
        n_ph = sum(1 for i in items if i["kind"] == "photo")
        report[taxon] = {"analyzer": aid, "photos": n_ph, "renders": len(items) - n_ph, "characters": res}
        texts.append(markdown(taxon, aid, res, n_ph, len(items) - n_ph))
    if args.out:
        out = Path(args.out).expanduser()
        out.mkdir(parents=True, exist_ok=True)
        (out / "TALLY.md").write_text("\n".join(texts))
        (out / "tally.json").write_text(json.dumps(report, indent=1, ensure_ascii=False) + "\n")
        print(f"wrote {out / 'TALLY.md'}")
    else:
        print("\n".join(texts))
