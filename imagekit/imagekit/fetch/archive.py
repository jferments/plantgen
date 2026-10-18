"""Page images of a public-domain book on archive.org, e.g. Britton and Brown (1913), `illustratedflor03brit`."""
from __future__ import annotations

from .. import net


def fetch(args, dl) -> None:
    meta = net.get_json(f"https://archive.org/metadata/{args.identifier}")["metadata"]
    for n in args.pages:
        cand = {
            "source": "archive", "id": f"{args.identifier}-n{n}",
            "url": f"https://archive.org/download/{args.identifier}/page/n{n}_w{args.width}.jpg",
            "page": f"https://archive.org/details/{args.identifier}/page/n{n}",
            "licence": "pd", "creator": meta.get("creator"), "title": meta.get("title"),
            "date": meta.get("date"), "taxon": args.taxon, "why": "book page",
        }
        if not dl.take(cand):
            return
