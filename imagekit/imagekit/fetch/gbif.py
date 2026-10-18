"""GBIF occurrences with still images; herbarium sheets with --basis PRESERVED_SPECIMEN.

Image files sit on each publisher's own server, so expect some to fail.
"""
from __future__ import annotations

import sys

from .. import licences, net

API = "https://api.gbif.org/v1/occurrence/search"


def fetch(args, dl) -> None:
    need = args.n - dl.held("gbif", args.taxon)
    offset, got = 0, 0
    while got < need and offset < 10000:
        q = {"scientificName": args.taxon, "mediaType": "StillImage", "limit": 300, "offset": offset}
        if args.basis:
            q["basisOfRecord"] = args.basis
        if args.country:
            q["country"] = args.country
        data = net.get_json(API, q)
        for occ in data["results"]:
            for i, media in enumerate(occ.get("media") or []):
                lic = licences.norm(media.get("license") or occ.get("license"))
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
                    "why": "gbif search",
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
