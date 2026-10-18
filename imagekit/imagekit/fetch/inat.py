"""iNaturalist through its API: research-grade, wild observations, spread over observers and months.

Photos come from iNaturalist's open-data bucket on AWS (only openly
licensed photos are there). For thousands of species use `inat-dump`.
"""
from __future__ import annotations

import random
import sys

from .. import licences, net

API = "https://api.inaturalist.org/v1/observations"

# Controlled annotations: attribute id -> (name, {value id: value})
ANNOTATIONS = {
    12: ("phenology", {13: "flowering", 14: "fruiting", 15: "flower budding", 21: "no evidence of flowering"}),
    36: ("leaves", {37: "breaking leaf buds", 38: "green leaves", 39: "colored leaves", 40: "no live leaves"}),
    17: ("alive or dead", {18: "alive", 19: "dead", 20: "cannot be determined"}),
}
PHENOLOGY = {"flowering": 13, "fruiting": 14, "budding": 15, "none": 21}


def annotations(obs: dict) -> dict:
    out = {}
    for a in obs.get("annotations") or []:
        name, values = ANNOTATIONS.get(a.get("controlled_attribute_id"), (None, None))
        if name:
            out[name] = values.get(a.get("controlled_value_id"), a.get("controlled_value_id"))
    return out


def candidates(obs: dict, args, why: str) -> list[dict]:
    lat = lon = None
    if obs.get("location"):
        lat, lon = (float(v) for v in obs["location"].split(","))
    user = obs.get("user") or {}
    taxon = obs.get("taxon") or {}
    out = []
    for photo in (obs.get("photos") or [])[: args.photos_per_obs]:
        url = photo.get("url") or ""
        if "/square." not in url or licences.norm(photo.get("license_code")) not in args.licences:
            continue
        out.append({
            "source": "inat", "id": photo["id"], "url": url.replace("/square.", f"/{args.size}."),
            "page": f"https://www.inaturalist.org/photos/{photo['id']}",
            "licence": photo.get("license_code"), "creator": user.get("name") or user.get("login"),
            "attribution": photo.get("attribution"), "observer": user.get("login"),
            "observation": obs["id"], "observation_url": f"https://www.inaturalist.org/observations/{obs['id']}",
            "date": obs.get("observed_on"), "place": obs.get("place_guess"), "lat": lat, "lon": lon,
            "accuracy_m": obs.get("positional_accuracy"), "obscured": obs.get("obscured"),
            "quality": obs.get("quality_grade"), "captive": obs.get("captive"),
            "annotations": annotations(obs), "taxon": args.taxon,
            "source_taxon_id": taxon.get("id"), "source_taxon_name": taxon.get("name"),
            "dims": photo.get("original_dimensions"), "why": why,
        })
    return out


def query(args) -> dict:
    q = {"taxon_name": args.taxon, "quality_grade": "research", "photos": "true",
         "photo_license": ",".join(sorted(args.licences - {"pd"})), "per_page": 100}
    if not args.include_captive:
        q["captive"] = "false"
    if args.phenology:
        q.update(term_id=12, term_value_id=PHENOLOGY[args.phenology])
    if args.place_id:
        q["place_id"] = args.place_id
    if args.months:
        q["month"] = args.months
    return q


def pool(q: dict, want: int, why: str, args, max_pages: int = 10) -> list[dict]:
    out = []
    for page in range(1, max_pages + 1):
        res = net.get_json(API, dict(q, page=page))["results"]
        for obs in res:
            out.extend(candidates(obs, args, why))
        if len(res) < q["per_page"] or len(out) >= want:
            break
    return out


def fetch(args, dl) -> None:
    need = args.n - dl.held("inat", args.taxon)
    if need <= 0:
        print(f"{args.taxon}: {args.n} already held", file=sys.stderr)
        return
    q = query(args)
    rng = random.Random(f"{args.seed}-{args.taxon}")
    over = 3 * need  # fetch more candidates than needed: the observer cap and held photos drop some
    if args.spread == "votes":
        pools = [pool(dict(q, order_by="votes"), over, "votes", args)]
    elif args.spread == "random":
        pools = [pool(dict(q, order_by="random"), over, "random", args)]
    else:  # months: half most-voted, half round-robin over the months it is seen in
        top = pool(dict(q, order_by="votes"), over // 2 + 1, "votes", args)
        hist = net.get_json(API + "/histogram", dict(q, date_field="observed", interval="month_of_year"))
        counts = hist["results"]["month_of_year"]
        months = [int(m) for m, c in sorted(counts.items(), key=lambda kv: -kv[1]) if c > 0]  # busiest first
        by_month = {}
        if not args.months:
            for m in months:
                by_month[m] = pool(dict(q, month=m, order_by="random", per_page=30), 30, f"month {m:02d}", args, 1)
        spread = []
        while any(by_month.values()):
            for m in by_month:
                if by_month[m]:
                    spread.append(by_month[m].pop(0))
        pools = [top, spread] if spread else [top]
    per_observer: dict = {}
    seen_obs = set()
    picked = []
    quota = [(need + 1) // 2, need - (need + 1) // 2] if len(pools) == 2 else [need]
    for cands, k in zip(pools, quota):
        got = 0
        if cands and cands[0]["why"] in ("random",):
            rng.shuffle(cands)
        for c in cands:
            if got >= k:
                break
            if dl.known("inat", c["id"]) or (c["observation"] in seen_obs and args.photos_per_obs == 1):
                continue
            if per_observer.get(c["observer"], 0) >= args.per_observer:
                continue  # one keen observer must not set a species' look
            per_observer[c["observer"]] = per_observer.get(c["observer"], 0) + 1
            seen_obs.add(c["observation"])
            picked.append(c)
            got += 1
    if len(picked) < need:  # top up from any pool when one ran short
        for c in [c for p in pools for c in p]:
            if len(picked) >= need:
                break
            if c in picked or dl.known("inat", c["id"]) or per_observer.get(c["observer"], 0) >= args.per_observer:
                continue
            per_observer[c["observer"]] = per_observer.get(c["observer"], 0) + 1
            picked.append(c)
    for c in picked:
        if not dl.take(c):
            return
    print(f"{args.taxon}: {len(picked)} picked of {need} wanted", file=sys.stderr)
