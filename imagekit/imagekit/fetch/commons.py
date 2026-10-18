"""Wikimedia Commons: the files in a category (often the species' name), slowly."""
from __future__ import annotations

import re
import sys

from .. import licences, net

API = "https://commons.wikimedia.org/w/api.php"


def plain(html: str | None) -> str | None:
    return re.sub(r"<[^>]+>", "", html).strip() if html else None


def date(text: str | None) -> str | None:
    """2015-10-30, or a year, from Commons' free-text date field."""
    t = plain(text) or ""
    m = re.search(r"\d{4}-\d{2}-\d{2}", t) or re.search(r"\b(1[6-9]|20)\d{2}\b", t)
    return m.group(0) if m else None


def fetch(args, dl) -> None:
    category = args.category or args.taxon
    need = args.n - dl.held("commons", args.taxon or category)
    cont: dict = {}
    got = 0
    while got < need:
        q = {"action": "query", "format": "json", "generator": "categorymembers",
             "gcmtitle": "Category:" + category, "gcmtype": "file", "gcmlimit": 50,
             "prop": "imageinfo", "iiprop": "url|extmetadata|size|sha1", "iiurlwidth": args.width,
             "maxlag": 5, **cont}
        data = net.get_json(API, q)
        for page in ((data.get("query") or {}).get("pages") or {}).values():
            info = (page.get("imageinfo") or [{}])[0]
            meta = info.get("extmetadata") or {}
            lic = licences.norm((meta.get("LicenseShortName") or {}).get("value"))
            if lic not in dl.allowed or not info.get("thumburl") or dl.known("commons", page["pageid"]):
                continue
            cand = {
                "source": "commons", "id": page["pageid"], "url": info["thumburl"],
                "page": info.get("descriptionurl"), "licence": lic,
                "creator": plain((meta.get("Artist") or {}).get("value")),
                "date": date((meta.get("DateTimeOriginal") or {}).get("value")),
                "description": plain((meta.get("ImageDescription") or {}).get("value")),
                "taxon": args.taxon or category, "why": f"Category:{category}",
            }
            if not dl.take(cand):
                return
            got += 1
            if got >= need:
                break
        if "continue" not in data:
            break
        cont = data["continue"]
    print(f"{category}: {got} from Commons", file=sys.stderr)
