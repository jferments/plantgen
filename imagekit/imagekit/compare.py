"""`imagekit compare`: renders against photos and the text, as photo-rules/04-compare.md says.

    imagekit compare --taxon "Asimina triloba" --out compare/asimina
    imagekit compare --taxon "Asimina triloba" --renders render-asimina-triloba-2026-10-18 --setting relaxed

Reads one observe analyzer's answers on the taxon's photos and on its
renders (the same model for both, as the rules require), drops what the
rules drop (unusable photos, cultivar looks, other settings for whole-plant
features, other leaf states, one vote per observation), aggregates each
side per feature, gives each feature a verdict (section 2), brings in what
the species' sources state (`plantc traits`, section 3), checks for organs
the photos show and no render does, and ranks the misses by severity
(section 4). It writes, in --out:

    COMPARE.md               one table per species: sources, photos (n), renders, verdict
    aggregate-photos.json    schemas/aggregate.schema.json
    aggregate-renders.json
    findings.json            schemas/finding.schema.json, one per ✗, ~ or conflict, by severity

A render is an image whose source has a `render` block (`imagekit render`
writes one): commit, revision, species id, seed, age, day, env and view.
Diagnosis (05-diagnose.md) starts from findings.json.
"""
from __future__ import annotations

import collections
import json
import math
import re
import statistics
import subprocess
import sys
from pathlib import Path

from .prompts import Rules
from .run import image_rows, outputs
from .store import Store, is_render

WEIGHT = {"habit": 1.0, "architecture": 0.9, "foliage": 0.8, "surface": 0.6, "shoot": 0.6,
          "flower": 0.5, "inflorescence": 0.5, "fruit": 0.4, "leaf": 0.4, "season": 0.4}
WHOLE = {"habit", "architecture", "foliage"}          # compared only with photos from the same setting
LEAFY = {"foliage", "leaf", "shoot", "season"}         # compared only with photos in the renders' leaf state
SETTINGS = {"strict": {"open_grown"}, "relaxed": {"open_grown", "garden_or_street", "cannot tell"}}
ENV_SETTING = {"open": "open_grown", "edge": "forest", "interior": "forest", "suppressed": "forest"}
FACE_ON_VIEWS = {"atlas", "card"}                      # render images that show organs flat to the camera
ORGANS = ("flower", "fruit", "cone", "inflorescence")
BINS = {"16-30": 23, "31-60": 45, "61-120": 90, ">120": 150}
MIN_PHOTOS, MIN_PHOTOGRAPHERS, MIN_RENDERS = 5, 3, 3
COLOUR_OK, COLOUR_PART = 15, 25
# The least spread a photo IQR is given, as the profile rule gives its bands 0.05 of the height: observers
# answer in steps (shares in tenths, angles in fives), so six identical answers are not an IQR of zero.
READING = {"share": 0.1, "angle": 10.0, "count": 1.0}
SEASON = {"flower": "flowering_months", "inflorescence": "flowering_months", "fruit": "fruiting_months"}
NO_ANSWER = (None, "cannot tell", "not applicable")


def register(sub) -> None:
    p = sub.add_parser("compare", help="renders against photos and the text (photo-rules 04-compare.md)")
    p.add_argument("--taxon", required=True, action="append",
                   help="scientific name, as fetched or accepted; repeat for several species")
    p.add_argument("--observe", help="the observe analyzer (default: the one that answered most of the taxon's "
                                     "photos and renders)")
    p.add_argument("--triage", help="the triage analyzer (default: the one the observe analyzer read views from)")
    p.add_argument("--photos", nargs="*", help="only photos from these batches (default: every batch)")
    p.add_argument("--renders", nargs="*", help="only renders from these batches (default: every render batch)")
    p.add_argument("--setting", choices=sorted(SETTINGS), default="strict",
                   help="photos kept for whole-plant features: strict, open-grown only (the rule); relaxed adds "
                        "gardens and unknown settings")
    p.add_argument("--plantc", default="plantc", help="the plantc that made the renders, for `plantc traits`")
    p.add_argument("--no-text", action="store_true", help="skip the sources step (no plantc at hand)")
    p.add_argument("--rules", help="the photo-rules folder (default as for run)")
    p.add_argument("--out", required=True, help="folder for COMPARE.md, the aggregates and findings.json")
    p.set_defaults(func=cmd_compare)


# reading the store

def taxon_names(row: dict) -> set:
    t = row["taxon"]
    return {t.get("name_as_given"), t.get("accepted_name"), t.get("accepted")} - {None}


def pick_observe(store: Store, shas) -> str | None:
    shas = set(shas)
    best = collections.Counter()
    for aid, sha in store.db.execute("select analyzer, sha256 from analyses where task='observe' and status='ok' "
                                     "and region=0"):
        if sha in shas:
            best[aid] += 1
    return best.most_common(1)[0][0] if best else None


def triage_of(store: Store, observe_aid: str) -> str | None:
    for r in (store.root / "analyzers.jsonl").read_text().splitlines() if (store.root / "analyzers.jsonl").exists() else []:
        d = json.loads(r)
        if d["id"] == observe_aid:
            return (d.get("params") or {}).get("triage")
    return None


def items_from_store(store: Store, taxon: str, observe_aid: str, triage_aid: str | None,
                     photo_batches=None, render_batches=None) -> list[dict]:
    """One item per image of the taxon with an observation: what compare_species reads."""
    rows = {s: r for s, r in image_rows(store).items() if taxon in taxon_names(r)}
    obs = outputs(store, observe_aid)
    tri = outputs(store, triage_aid) if triage_aid else {}
    items = []
    for sha, row in rows.items():
        render = is_render(row)
        wanted = render_batches if render else photo_batches
        if wanted and row["batch"] not in wanted:
            continue
        if sha not in obs:
            continue
        src = row["source"]
        items.append({
            "id": "img-" + sha[:10], "sha256": sha, "kind": "render" if render else "photo",
            "who": src.get("observer") or src.get("creator") or src["key"],
            "vote": (f"obs:{src['observation']}" if src.get("observation") else
                     f"{src.get('observer') or src.get('creator')}|{src.get('date')}" if src.get("date") else src["key"]),
            "date": src.get("date"), "lat": src.get("lat"), "render": src.get("render"),
            "triage": tri.get(sha) or {}, "answers": (obs[sha] or {}).get("answers") or {}})
    return items


# numbers and colours

def number(v):
    if isinstance(v, dict) and "count" in v:
        return float(v["count"])
    if isinstance(v, bool):
        return None
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, str):
        if v in BINS:
            return float(BINS[v])
        try:
            return float(v)
        except ValueError:
            return None
    return None


def lab(hexs: str) -> tuple:
    rgb = [int(hexs[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    lin = [((c + 0.055) / 1.055) ** 2.4 if c > 0.04045 else c / 12.92 for c in rgb]
    x = (0.4124 * lin[0] + 0.3576 * lin[1] + 0.1805 * lin[2]) / 0.95047
    y = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]
    z = (0.0193 * lin[0] + 0.1192 * lin[1] + 0.9505 * lin[2]) / 1.08883
    f = lambda t: t ** (1 / 3) if t > 0.008856 else 7.787 * t + 16 / 116  # noqa: E731
    return (116 * f(y) - 16, 500 * (f(x) - f(y)), 200 * (f(y) - f(z)))


def de2000(a, b) -> float:
    L1, a1, b1 = a
    L2, a2, b2 = b
    C1, C2 = math.hypot(a1, b1), math.hypot(a2, b2)
    Cb = (C1 + C2) / 2
    G = 0.5 * (1 - math.sqrt(Cb ** 7 / (Cb ** 7 + 25 ** 7)))
    a1p, a2p = (1 + G) * a1, (1 + G) * a2
    C1p, C2p = math.hypot(a1p, b1), math.hypot(a2p, b2)
    hue = lambda y, x: math.degrees(math.atan2(y, x)) % 360  # noqa: E731
    h1p, h2p = hue(b1, a1p), hue(b2, a2p)
    dLp, dCp = L2 - L1, C2p - C1p
    if C1p * C2p == 0:
        dh = 0
    elif abs(h2p - h1p) <= 180:
        dh = h2p - h1p
    else:
        dh = h2p - h1p - 360 if h2p > h1p else h2p - h1p + 360
    dHp = 2 * math.sqrt(C1p * C2p) * math.sin(math.radians(dh / 2))
    Lbp, Cbp = (L1 + L2) / 2, (C1p + C2p) / 2
    if C1p * C2p == 0:
        hbp = h1p + h2p
    elif abs(h1p - h2p) <= 180:
        hbp = (h1p + h2p) / 2
    else:
        hbp = (h1p + h2p + 360) / 2 if h1p + h2p < 360 else (h1p + h2p - 360) / 2
    T = (1 - 0.17 * math.cos(math.radians(hbp - 30)) + 0.24 * math.cos(math.radians(2 * hbp))
         + 0.32 * math.cos(math.radians(3 * hbp + 6)) - 0.20 * math.cos(math.radians(4 * hbp - 63)))
    SL = 1 + 0.015 * (Lbp - 50) ** 2 / math.sqrt(20 + (Lbp - 50) ** 2)
    SC, SH = 1 + 0.045 * Cbp, 1 + 0.015 * Cbp * T
    RT = -2 * math.sqrt(Cbp ** 7 / (Cbp ** 7 + 25 ** 7)) * math.sin(math.radians(60 * math.exp(-((hbp - 275) / 25) ** 2)))
    return math.sqrt((dLp / SL) ** 2 + (dCp / SC) ** 2 + (dHp / SH) ** 2 + RT * (dCp / SC) * (dHp / SH))


def quartiles(xs) -> tuple:
    xs = sorted(xs)
    if len(xs) == 1:
        return xs[0], xs[0], xs[0]
    q = statistics.quantiles(xs, n=4, method="inclusive")
    return q[0], statistics.median(xs), q[2]


# section 1: aggregate

def merge_votes(answers: list[dict], ftype: str) -> dict:
    """Several photos of one observation (or one photographer on one day) give one vote (section 1.2)."""
    if len(answers) == 1:
        return answers[0]
    high = sum(a.get("confidence") == "high" for a in answers) * 2 > len(answers)
    vals = [a["value"] for a in answers]
    if ftype in ("state", "ordinal", "states"):
        value = collections.Counter(json.dumps(v, sort_keys=True) for v in vals).most_common(1)[0][0]
        value = json.loads(value)
    elif ftype == "colour":
        cols = [v for v in vals if isinstance(v, dict) and "hex" in v]
        if not cols:
            return answers[0]
        light = collections.Counter(c["lighting"] for c in cols).most_common(1)[0][0]
        same = [c for c in cols if c["lighting"] == light]
        family = collections.Counter(c.get("family") for c in same).most_common(1)[0][0]
        value = {"family": family, "lighting": light, "lab": [statistics.median(lab(c["hex"])[i] for c in same)
                                                              for i in range(3)]}
    elif ftype == "profile":
        lists = [v for v in vals if isinstance(v, list) and len(v) == 10]
        if not lists:
            return answers[0]
        value = [statistics.median(v[i] for v in lists) for i in range(10)]
        est = [a.get("estimated") or [] for a in answers if isinstance(a["value"], list)]
        merged = {"value": value, "confidence": "high" if high else "medium",
                  "estimated": sorted({b for e in est for b in e if sum(b in x for x in est) * 2 > len(est)})}
        return merged
    else:
        xs = [x for x in map(number, vals) if x is not None]
        if not xs:
            return answers[0]
        value = statistics.median(xs)
    return {"value": value, "confidence": "high" if high else "medium"}


def summarise(votes: list[tuple[dict, str]], feature: dict) -> dict:
    """votes: (answer, who) pairs, one per vote. The aggregate.schema.json entry for one feature."""
    t = feature["type"]
    n = len(votes)
    out = {"type": t, "n": n, "photographers": len({w for _, w in votes}),
           "high_confidence_share": round(sum(a.get("confidence") == "high" for a, _ in votes) / n, 3) if n else 0.0}
    if t in ("state", "ordinal", "states"):
        c = collections.Counter()
        for a, _ in votes:
            for s in (a["value"] if isinstance(a["value"], list) else [a["value"]]):
                c[str(s)] += 1
        out["shares"] = {k: round(x / n, 3) for k, x in c.most_common()}
        if c:
            m, x = c.most_common(1)[0]
            out["majority"], out["majority_share"] = m, round(x / n, 3)
    elif t == "colour":
        groups = collections.defaultdict(list)
        fam = collections.Counter()
        for a, _ in votes:
            v = a["value"]
            if not isinstance(v, dict):
                continue
            if "lab" in v:
                groups[v["lighting"]].append(tuple(v["lab"]))
            elif "hex" in v:
                groups[v["lighting"]].append(lab(v["hex"]))
            if v.get("family"):
                fam[v["family"]] += 1
        out["lab"] = {g: {"median": [round(statistics.median(p[i] for p in ls), 1) for i in range(3)], "n": len(ls)}
                      for g, ls in groups.items()}
        if fam:
            out["shares"] = {k: round(x / n, 3) for k, x in fam.most_common()}
            out["majority"] = fam.most_common(1)[0][0]
    elif t == "profile":
        lists = [(a["value"], a.get("estimated") or []) for a, _ in votes
                 if isinstance(a["value"], list) and len(a["value"]) == 10]
        bm, b1, b3, bn, est = [], [], [], [], []
        for i in range(10):
            xs = [v[i] for v, _ in lists]
            q1, m, q3 = quartiles(xs) if xs else (0, 0, 0)
            bm.append(round(m, 3)), b1.append(round(q1, 3)), b3.append(round(q3, 3)), bn.append(len(xs))
            if xs and sum(i in e for _, e in lists) * 2 > len(xs):
                est.append(i)
        out.update(n=len(lists), band_median=bm, band_q1=b1, band_q3=b3, band_n=bn, bands_estimated=est)
    else:
        xs = [x for x in (number(a["value"]) for a, _ in votes) if x is not None]
        if xs:
            q1, m, q3 = quartiles(xs)
            out.update(q1=round(q1, 3), median=round(m, 3), q3=round(q3, 3))
        out["n"] = len(xs)
    return out


def aggregate(items: list[dict], feats: dict, keep_answer, merge: bool) -> tuple[dict, collections.Counter]:
    """feature id -> summary. keep_answer(item, feature) says whether an item's answer counts."""
    by_vote = collections.defaultdict(lambda: collections.defaultdict(list))
    dropped = collections.Counter()
    for it in items:
        for fid, a in it["answers"].items():
            f = feats.get(fid)
            if f is None or not isinstance(a, dict) or a.get("value") in NO_ANSWER:
                continue
            why = keep_answer(it, f)
            if why:
                dropped[why] += 1
                continue
            by_vote[fid][(it["vote"] if merge else it["id"], it["who"])].append(a)
    out = {}
    for fid, groups in by_vote.items():
        votes = [(merge_votes(ans, feats[fid]["type"]), who) for (_, who), ans in groups.items()]
        out[fid] = summarise(votes, feats[fid])
    return out, dropped


# section 2: verdicts

def verdict(f: dict, ph: dict, rd: dict) -> tuple[str | None, str]:
    t = f["type"]
    if t in ("state", "states"):
        rs = rd.get("majority")
        share = ph.get("shares", {}).get(rs, 0)
        if share >= 0.25:
            return "✓", f"the renders' {rs} has {share:.0%} of the photo answers"
        if ph.get("majority_share", 0) < 0.6:
            return "~", f"photos split (no state over 60%); the renders' {rs} has {share:.0%}"
        return ("~" if share >= 0.10 else "✗"), f"the renders' {rs} has {share:.0%} of the photo answers"
    if t == "ordinal":
        order = list((f.get("states") or {}).keys())
        rs, pm = rd.get("majority"), ph.get("majority")
        if rs == pm or ph.get("shares", {}).get(rs, 0) >= 0.25:
            return "✓", ""
        if rs in order and pm in order:
            step = abs(order.index(rs) - order.index(pm))
            return ("~" if step == 1 else "✗"), f"{step} step{'s' if step > 1 else ''} from the photos' {pm}"
        return "~", "a state outside the scale"
    if t == "colour":
        rl = rd.get("lab", {}).get("render") or next(iter(rd.get("lab", {}).values()), None)
        groups = ph.get("lab", {})
        if not rl or not groups:
            return None, "no colours"
        light, best = max(groups.items(), key=lambda kv: kv[1]["n"])
        d = de2000(best["median"], rl["median"])
        return ("✓" if d < COLOUR_OK else "~" if d <= COLOUR_PART else "✗"), f"ΔE2000 {d:.0f} against photos in {light}"
    if t == "profile":
        return profile_verdict(ph, rd)
    if "median" not in ph or "median" not in rd:
        return None, "no numbers"
    q1, m, q3, r = ph["q1"], ph["median"], ph["q3"], rd["median"]
    floor = READING.get(t, 0.1 * abs(m))  # ratios: a tenth of the median
    iqr = max(q3 - q1, floor, 1e-9)
    if min(q1, m - iqr / 2) <= r <= max(q3, m + iqr / 2):
        return "✓", ""
    return ("~" if abs(r - m) <= 1.5 * iqr else "✗"), f"render {r} against the photos' IQR {q1}-{q3}"


def profile_verdict(ph: dict, rd: dict) -> tuple[str | None, str]:
    misses, partly, compared, where = 0, 0, 0, []
    zone = lambda i: "lower" if i <= 2 else "middle" if 3 <= i <= 6 else "upper"  # noqa: E731
    for i in range(10):
        if ph["band_n"][i] < MIN_PHOTOS or not rd.get("band_n") or not rd["band_n"][i]:
            continue
        compared += 1
        q1, m, q3, r = ph["band_q1"][i], ph["band_median"][i], ph["band_q3"][i], rd["band_median"][i]
        iqr = max(q3 - q1, 0.05)  # the reading's own accuracy
        z = (r - m) / iqr
        if min(q1, m - iqr / 2) <= r <= max(q3, m + iqr / 2):
            continue
        if abs(r - m) <= 1.5 * iqr or i in ph.get("bands_estimated", []):  # an estimated band is at most ~
            partly += 1
        else:
            misses += 1
        where.append(f"{0.05 + 0.1 * i:.2f} H {zone(i)} {'wider' if z > 0 else 'narrower'} {z:+.1f} IQR")
    if compared < 6:
        return None, f"only {compared} bands with five photo widths"
    v = "✓" if misses == 0 and partly <= 2 else "~" if misses <= 1 else "✗"
    return v, "; ".join(where)


# section 3: the text

def stated(plantc: str, species_id: str) -> dict:
    """{trait: value words} that the species' sources state, from `plantc traits` (as at the rendered commit)."""
    try:
        out = subprocess.run([plantc, "traits", species_id], capture_output=True, text=True, timeout=120).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    facts = {}
    for m in re.finditer(r"^\s+([a-z_]+) = (.+?)\s+\((.+)\)\s*$", out, re.M):
        facts[m.group(1)] = (m.group(2).strip(), m.group(3).strip())
    return facts


def trait_of(f: dict) -> str | None:
    t = (f.get("maps_to") or {}).get("trait")
    return t if isinstance(t, str) else None


def words(v) -> set:
    return set(re.findall(r"[a-z0-9_]+", str(v).lower()))


# the whole comparison for one species

def compare_species(items: list[dict], feats: dict, setting: str = "strict", facts: dict | None = None,
                    species: str = "") -> dict:
    facts = facts or {}
    renders = [i for i in items if i["kind"] == "render"]
    photos = [i for i in items if i["kind"] == "photo"]
    kept_photos, dropped = [], collections.Counter()
    for p in photos:
        t = p["triage"]
        if t and not t.get("usable", True):
            dropped["photo unusable"] += 1
        elif t.get("cultivar_look") == "yes":
            dropped["cultivar look"] += 1
        else:
            kept_photos.append(p)
    common = lambda key: collections.Counter(  # noqa: E731
        r["triage"].get(key) for r in renders if r["triage"].get(key) not in (None, "cannot tell")).most_common(1)
    leaf_state = (common("leaf_state") or [(None,)])[0][0]
    age_class = (common("age_class") or [(None,)])[0][0]
    envs = {(r.get("render") or {}).get("env") for r in renders} - {None}
    keep_settings = set(SETTINGS[setting])
    if envs and all(ENV_SETTING.get(e) == "forest" for e in envs):  # forest renders go with forest photos
        keep_settings = {"forest"} | ({"cannot tell"} if setting == "relaxed" else set())
    by_age = sum(p["triage"].get("age_class") == age_class for p in kept_photos) >= MIN_PHOTOS if age_class else False

    def keep_photo(it, f):
        t = it["triage"]
        if f["scale"] in WHOLE and t.get("setting") not in keep_settings:
            return "answers from another setting"
        if f["scale"] in LEAFY and leaf_state and t.get("leaf_state") not in (leaf_state, "cannot tell", None):
            return "answers from another leaf state"
        if f["type"] == "profile" and by_age and t.get("age_class") != age_class:
            return "profiles from another age class"
        return None

    def keep_render(it, f):
        if f.get("face_on") and (it.get("render") or {}).get("view") not in FACE_ON_VIEWS:
            return "face-on features from a tilted render"
        return None

    agg_p, d1 = aggregate(kept_photos, feats, keep_photo, merge=True)
    agg_r, d2 = aggregate(renders, feats, keep_render, merge=False)
    dropped.update(d1)
    dropped.update(d2)
    for a in agg_p.values():
        a["insufficient"] = a["n"] < MIN_PHOTOS or a["photographers"] < MIN_PHOTOGRAPHERS
    for fid, a in agg_r.items():
        # a flat card is the drawn organ itself, the same at every seed: one answers a face-on feature
        a["insufficient"] = a["n"] < (1 if feats[fid].get("face_on") else MIN_RENDERS)

    rows, findings = [], []
    # missing organs on the render day (section 2, "Missing organs")
    days = sorted({(r.get("render") or {}).get("day") for r in renders} - {None})
    whole = [r for r in renders if (r.get("render") or {}).get("view") not in FACE_ON_VIEWS | {"parts"}]
    for day in days:
        month = min(12, int((day - 1) / 30.42) + 1)
        near = []
        for p in kept_photos:
            if not p.get("date"):
                continue
            m = int(p["date"][5:7])
            if (p.get("lat") or 0) < 0:  # southern hemisphere: the same season six months on
                m = (m + 5) % 12 + 1
            if min(abs(m - month), 12 - abs(m - month)) <= 1:
                near.append(p)
        on_day = [r for r in whole if (r.get("render") or {}).get("day") == day]
        for org in ORGANS:
            share = sum(org in (p["triage"].get("organs_visible") or []) for p in near) / len(near) if near else 0
            shown = sum(org in (r["triage"].get("organs_visible") or []) for r in on_day)
            if len(near) >= MIN_PHOTOS and share >= 0.25 and on_day and shown == 0:
                ph_txt = f"{org} in {share:.0%} of {len(near)} photos within a month of day {day}"
                rd_txt = f"in 0 of {len(on_day)} renders on day {day}"
                said = facts.get(SEASON.get(org, ""))
                stated_months = {int(x) for x in re.findall(r"\d+", said[0])} if said else set()
                v, why = "✗", "organ missing on the render day"
                if stated_months and month not in stated_months:
                    v, why = "conflict", f"the sources give months {said[0]}, not month {month}: check the photos' dates"
                rows.append((f"presence.{org}", f"{SEASON[org]} = {said[0]} ({said[1]})" if said else "",
                             ph_txt, rd_txt, v, why))
                f = {"id": f"{species}:presence.{org}:d{day}", "species": species, "feature": f"presence.{org}",
                     "verdict": v, "photos": ph_txt, "render": rd_txt,
                     "severity": round((0.5 if v == "✗" else 0.125) * share, 3), "render_set": render_set(renders)}
                if said:
                    f["sources_say"] = f"{SEASON[org]} = {said[0]} ({said[1]})"
                findings.append(f)

    order = sorted(set(agg_p) | set(agg_r), key=lambda x: (-WEIGHT.get(feats[x]["scale"], 0.4), x))
    for fid in order:
        f, ph, rd = feats[fid], agg_p.get(fid), agg_r.get(fid)
        tr = trait_of(f)
        said = facts.get(tr) if tr else None
        said_txt = f"{tr} = {said[0]} ({said[1]})" if said else ""
        if not ph or ph["insufficient"]:
            rows.append((fid, said_txt, describe(ph), describe(rd), "insufficient", ""))
            continue
        if not rd or rd["insufficient"]:
            why = "face-on cards needed (plantc atlas)" if f.get("face_on") else "fewer than three render answers"
            rows.append((fid, said_txt, describe(ph), describe(rd), "render insufficient", why))
            continue
        v, why = verdict(f, ph, rd)
        if v in ("✗", "~") and said:
            if rd.get("majority") and rd["majority"] in words(said[0]):
                v, why = "conflict", "the sources state what is drawn; the photos differ: re-read the source, check the photos"
            elif ph.get("majority") and ph["majority"] in words(said[0]):
                why += "; the sources state what the photos show"
        rows.append((fid, said_txt, describe(ph), describe(rd), v or "–", why))
        if v in ("✗", "~", "conflict"):
            size = {"✗": 1.0, "~": 0.5, "conflict": 0.25}[v]
            finding = {"id": f"{species}:{fid}", "species": species, "feature": fid, "verdict": v,
                       "photos": describe(ph), "render": describe(rd),
                       "severity": round(WEIGHT.get(f["scale"], 0.4) * size * ph["high_confidence_share"], 3),
                       "render_set": render_set(renders) + (f"; {why}" if why else "")}
            if said:
                finding["sources_say"] = said_txt
            findings.append(finding)
    findings.sort(key=lambda x: -x["severity"])
    filters = {"setting": sorted(keep_settings), "leaf_state": leaf_state, "age_class": age_class if by_age else None,
               "dropped": dict(dropped)}
    return {"species": species, "photos": agg_p, "renders": agg_r, "rows": rows, "findings": findings,
            "filters": filters, "photo_ids": sorted(p["id"] for p in kept_photos),
            "render_ids": sorted(r["id"] for r in renders), "render_set": render_set(renders)}


def describe(a: dict | None) -> str:
    if not a:
        return "none"
    if a["type"] == "profile":
        return "widths by band " + ", ".join(f"{m:.2f}" for m in a["band_median"]) + f" (n={a['n']})"
    if "lab" in a and a["lab"]:
        light, g = max(a["lab"].items(), key=lambda kv: kv[1]["n"])
        fam = f"{a['majority']}, " if a.get("majority") else ""
        return f"{fam}Lab {g['median']} in {light}, n={g['n']}"
    if "shares" in a:
        return ", ".join(f"{k} {round(v * a['n'])}/{a['n']}" for k, v in list(a["shares"].items())[:3])
    if "median" in a:
        return f"median {a['median']} (IQR {a['q1']}-{a['q3']}), n={a['n']}"
    return f"n={a['n']}"


def render_set(renders: list[dict]) -> str:
    rs = [r["render"] for r in renders if r.get("render") and r["render"].get("view") not in FACE_ON_VIEWS]
    if not rs:
        return f"{len(renders)} renders"
    pick = lambda k: ",".join(sorted({str(r.get(k)) for r in rs if r.get(k) is not None}))  # noqa: E731
    cards = len(renders) - len(rs)
    head = f"{len(rs)} renders" + (f" and {cards} flat cards" if cards else "")
    return (f"{head} at {pick('commit')[:40]} (revision {pick('generator_revision')}), "
            f"age {pick('age')}, day {pick('day')}, env {pick('env')}, seeds {pick('seed')}")


# writing it down (section 5)

def write(out: Path, results: list[dict], meta: dict) -> None:
    out.mkdir(parents=True, exist_ok=True)
    md = [f"# Renders against photos and the text ({meta['setting']} setting)", "",
          f"Observe analyzer `{meta['observe']}`, triage `{meta['triage']}`; photo-rules `{meta['rules']}`. "
          "Written by `imagekit compare` (photo-rules/04-compare.md). ✗ is a claim that the drawing misses; "
          "~ is a lead to look at; conflict means the sources state what is drawn and the photos differ.", ""]
    agg_p, agg_r, findings = [], [], []
    for r in results:
        sp = r["species"]
        tally = collections.Counter(row[4] for row in r["rows"])
        md += [f"## {sp} ({r['render_set']})", "",
               f"Photos kept {len(r['photo_ids'])}; filters {json.dumps(r['filters'], ensure_ascii=False)}. "
               f"Verdicts: {', '.join(f'{k} {v}' for k, v in tally.most_common())}.", "",
               "| Feature | Sources say | Photos show (n) | Renders show | | Why |", "| --- | --- | --- | --- | --- | --- |"]
        md += ["| " + " | ".join(str(c).replace("|", "/") for c in row) + " |" for row in r["rows"]]
        md.append("")
        agg_p.append({"species": sp, "of": "photos", "filters": r["filters"], "images": r["photo_ids"],
                      "features": r["photos"]})
        agg_r.append({"species": sp, "of": "renders", "filters": {"render_set": r["render_set"]},
                      "images": r["render_ids"], "features": r["renders"]})
        findings += r["findings"]
    findings.sort(key=lambda x: -x["severity"])
    md += ["## Next", "", "Diagnose each ✗ and conflict with photo-rules/05-diagnose.md (findings.json), then lift "
           "the fix to the genus or family (06-generalize.md). Show renders and photos side by side from the same "
           "views before anyone acts on a finding.", ""]
    (out / "COMPARE.md").write_text("\n".join(md))
    one = lambda xs: xs[0] if len(xs) == 1 else xs  # noqa: E731
    (out / "aggregate-photos.json").write_text(json.dumps(one(agg_p), indent=1, ensure_ascii=False) + "\n")
    (out / "aggregate-renders.json").write_text(json.dumps(one(agg_r), indent=1, ensure_ascii=False) + "\n")
    (out / "findings.json").write_text(json.dumps(findings, indent=1, ensure_ascii=False) + "\n")


def cmd_compare(args, store: Store) -> None:
    rules = Rules(args.rules)
    feats = {f["id"]: f for f in rules.features}
    results, used = [], {"observe": set(), "triage": set()}
    for taxon in args.taxon:
        shas = [s for s, r in image_rows(store).items() if taxon in taxon_names(r)]
        if not shas:
            raise SystemExit(f"no images of {taxon} in the store: imagekit fetch ... --taxon '{taxon}'")
        aid = args.observe or pick_observe(store, shas)
        if not aid:
            raise SystemExit(f"no observe answers on {taxon}: imagekit run observe --taxon '{taxon}' ...")
        tri = args.triage or triage_of(store, aid)
        items = items_from_store(store, taxon, aid, tri, args.photos, args.renders)
        n_r = sum(i["kind"] == "render" for i in items)
        if not n_r:
            print(f"{taxon}: no observed renders under {aid}; imagekit render, then run observe on them",
                  file=sys.stderr)
        ids = {(i.get("render") or {}).get("species") for i in items if i["kind"] == "render"} - {None}
        facts = {} if args.no_text or not ids else stated(args.plantc, sorted(ids)[0])
        res = compare_species(items, feats, args.setting, facts, taxon)
        used["observe"].add(aid)
        used["triage"].add(tri or "none")
        results.append(res)
        tally = collections.Counter(row[4] for row in res["rows"])
        print(f"{taxon}: {len(res['photo_ids'])} photos, {n_r} renders; " +
              ", ".join(f"{k} {v}" for k, v in tally.most_common()), file=sys.stderr)
    meta = {"setting": args.setting, "observe": ",".join(sorted(used["observe"])),
            "triage": ",".join(sorted(used["triage"])), "rules": rules.version}
    write(Path(args.out).expanduser(), results, meta)
    print(json.dumps({"out": args.out, "findings": sum(len(r["findings"]) for r in results)}))
