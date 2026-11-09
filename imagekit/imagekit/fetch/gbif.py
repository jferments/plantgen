"""GBIF occurrences with still images; herbarium sheets with --basis PRESERVED_SPECIMEN.

Image files sit on each publisher's own server, so expect some to fail.

`--organ habit` keeps only photos from Pl@ntNet's observations whose
observer tagged them as the whole plant ("<name>: habit" in GBIF's media
title; the other tags are flower, leaf, fruit, bark and other). Whole-plant
photos are what silhouette measures need, and in iNaturalist they are rare
(about one photo in twenty of trees, fewer of herbs); Pl@ntNet holds tens
to hundreds a common species. Every record is read, so a species with many
records takes a few requests a thousand.
"""
from __future__ import annotations

import sys

from .. import licences, net

API = "https://api.gbif.org/v1/occurrence/search"
PLANTNET = "7a3679ef-5582-4aaa-81f0-8c2545cafc81"  # Pl@ntNet automatically identified occurrences
ORGANS = ("bark", "flower", "fruit", "habit", "leaf", "other")


def media_licence(media: dict, occ: dict) -> str:
    """The photo's licence: Pl@ntNet writes "<creator> (cc-by-sa)", so the part in brackets is read too,
    then the occurrence's own."""
    raw = media.get("license") or ""
    lic = licences.norm(raw)
    if not lic and raw.rstrip().endswith(")") and "(" in raw:
        lic = licences.norm(raw.rstrip()[:-1].rsplit("(", 1)[1])
    return lic or licences.norm(occ.get("license"))


def organ_of(media: dict) -> str | None:
    """The organ a Pl@ntNet photo's observer tagged, from GBIF's title "<name>: <organ>"."""
    title = (media.get("title") or "").strip()
    organ = title.rsplit(":", 1)[-1].strip().lower() if ":" in title else ""
    return organ if organ in ORGANS else None


def fetch(args, dl) -> None:
    organ = getattr(args, "organ", None)
    need = args.n - dl.held("gbif", args.taxon, organ)  # --n counts photos of that organ only
    offset, got = 0, 0
    while got < need and offset < 100000:
        q = {"scientificName": args.taxon, "mediaType": "StillImage", "limit": 300, "offset": offset}
        if args.basis:
            q["basisOfRecord"] = args.basis
        if args.country:
            q["country"] = args.country
        if organ:
            q["datasetKey"] = PLANTNET
        data = net.get_json(API, q)
        for occ in data["results"]:
            for i, media in enumerate(occ.get("media") or []):
                if organ and organ_of(media) != organ:
                    continue
                lic = media_licence(media, occ)
                if lic not in dl.allowed or not media.get("identifier") or dl.known("gbif", f"{occ['key']}-{i}"):
                    continue
                cand = {
                    "source": "gbif", "id": f"{occ['key']}-{i}", "url": media["identifier"],
                    "page": f"https://www.gbif.org/occurrence/{occ['key']}", "licence": lic,
                    "creator": media.get("creator") or media.get("rightsHolder") or occ.get("recordedBy"),
                    "basis": occ.get("basisOfRecord"),
                    "institution": occ.get("institutionCode"), "catalogue_number": occ.get("catalogNumber"),
                    "date": (occ.get("eventDate") or "")[:10] or None, "place": occ.get("locality"),
                    "country": occ.get("countryCode"),
                    "lat": occ.get("decimalLatitude"), "lon": occ.get("decimalLongitude"),
                    "accuracy_m": occ.get("coordinateUncertaintyInMeters"),
                    "taxon": args.taxon, "source_taxon_id": occ.get("acceptedTaxonKey") or occ.get("taxonKey"),
                    "source_taxon_name": occ.get("acceptedScientificName") or occ.get("scientificName"),
                    "why": f"gbif search, Pl@ntNet {organ} photo" if organ else "gbif search",
                    **({"organ": organ} if organ else {}),
                }
                if not dl.take(cand):
                    return
                got += 1
                break  # one image per occurrence
            if got >= need:
                break
        if data.get("endOfRecords"):
            break
        offset += 300
    print(f"{args.taxon}: {got} from GBIF", file=sys.stderr)
