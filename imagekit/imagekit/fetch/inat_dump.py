"""iNaturalist's open-data dump: pick photos for many species at once, without the API.

The metadata tarball (36 GB on 2026-09-27) holds tab-separated taxa.csv,
observations.csv, photos.csv and observers.csv:

    aws s3 cp --no-sign-request s3://inaturalist-open-data/metadata/inaturalist-open-data-latest.tar.gz .
    tar xzf inaturalist-open-data-latest.tar.gz

Run it where the photos will be processed (the bucket is in us-east-1), or
pass --manifest-only to write a small manifest that `imagekit fetch
manifest` downloads elsewhere. The dump has no annotations (phenology).
"""
from __future__ import annotations

import csv
import json
import os
import random
import sys
from collections import defaultdict

BUCKET = "https://inaturalist-open-data.s3.amazonaws.com/photos"
csv.field_size_limit(1 << 24)


def rows(path: str):
    with open(path, newline="", encoding="utf-8") as fh:
        yield from csv.DictReader(fh, delimiter="\t")


def select(dump: str, names: set[str], *, n: int, per_observer: int, licences: set[str], seed: int,
           size: str, held=lambda name: 0) -> list[dict]:
    taxon_name = {r["taxon_id"]: r["name"] for r in rows(os.path.join(dump, "taxa.csv"))
                  if r["name"] in names and r.get("active", "true").lower() == "true"}
    print(f"{len(taxon_name)} of {len(names)} names found in the dump", file=sys.stderr)
    obs = {}
    for r in rows(os.path.join(dump, "observations.csv")):
        if r["taxon_id"] in taxon_name and r["quality_grade"] == "research":
            obs[r["observation_uuid"]] = r
    by_taxon = defaultdict(list)
    observers = set()
    for r in rows(os.path.join(dump, "photos.csv")):
        o = obs.get(r["observation_uuid"])
        if o and r["license"].lower() in licences and r.get("position", "0") in ("0", ""):
            by_taxon[o["taxon_id"]].append((r, o))
            observers.add(o["observer_id"])
    who = {r["observer_id"]: (r.get("login"), r.get("name"))
           for r in rows(os.path.join(dump, "observers.csv")) if r["observer_id"] in observers}
    rng = random.Random(seed)
    out = []
    for tid, cands in sorted(by_taxon.items()):
        name = taxon_name[tid]
        want = n - held(name)
        if want <= 0:
            continue
        rng.shuffle(cands)
        months = defaultdict(list)  # round-robin over months so a species' season is covered
        for c in cands:
            months[(c[1].get("observed_on") or "")[5:7]].append(c)
        order = []
        while any(months.values()):
            for m in sorted(months):
                if months[m]:
                    order.append(months[m].pop())
        count = defaultdict(int)
        taken = 0
        for r, o in order:
            if count[o["observer_id"]] >= per_observer:
                continue
            count[o["observer_id"]] += 1
            login, real = who.get(o["observer_id"], (None, None))
            out.append({
                "source": "inat", "id": int(r["photo_id"]),
                "url": f"{BUCKET}/{r['photo_id']}/{size}.{r['extension']}",
                "page": f"https://www.inaturalist.org/photos/{r['photo_id']}",
                "licence": r["license"].lower(), "creator": real or login, "observer": login,
                "observation_uuid": r["observation_uuid"], "date": o.get("observed_on"),
                "lat": float(o["latitude"]) if o.get("latitude") else None,
                "lon": float(o["longitude"]) if o.get("longitude") else None,
                "accuracy_m": int(o["positional_accuracy"]) if (o.get("positional_accuracy") or "").isdigit() else None,
                "quality": "research", "taxon": name, "source_taxon_id": int(tid), "source_taxon_name": name,
                "dims": {"width": r.get("width"), "height": r.get("height")}, "why": "dump, spread by month",
            })
            taken += 1
            if taken >= want:
                break
    return out


def fetch(args, dl) -> None:
    names = {line.strip() for line in open(args.taxa, encoding="utf-8") if line.strip()}
    picked = select(args.dump, names, n=args.n, per_observer=args.per_observer, licences=args.licences,
                    seed=args.seed, size=args.size, held=lambda name: dl.held("inat", name))
    if args.manifest_only:
        with open(args.manifest_only, "w", encoding="utf-8") as fh:
            for c in picked:
                fh.write(json.dumps(c, ensure_ascii=False, sort_keys=True) + "\n")
        print(f"wrote {len(picked)} photos to {args.manifest_only}", file=sys.stderr)
        return
    for c in picked:
        if not dl.take(c):
            return
