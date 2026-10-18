"""Images you already have: a folder of your own photos, or a manifest written elsewhere."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

from .. import licences

SUFFIXES = {".jpg", ".jpeg", ".png", ".webp", ".tif", ".tiff"}


def folder(args, dl) -> None:
    """Every image under PATH. With --taxon-from-folders, the folder an image sits in names its taxon."""
    root = Path(args.path).expanduser()
    for p in sorted(root.rglob("*")):
        if p.suffix.lower() not in SUFFIXES or not p.is_file():
            continue
        taxon = p.parent.name.replace("_", " ").replace("-", " ") if args.taxon_from_folders else args.taxon
        ident = hashlib.sha256(p.read_bytes()).hexdigest()[:16]  # the file's own hash, so a rerun skips it
        cand = {"source": args.source_name, "id": ident, "url": None, "path": str(p),
                "name": str(p.relative_to(root)), "licence": args.licence, "creator": args.creator,
                "taxon": taxon, "why": "own photos"}
        if not dl.take(cand):
            return


def manifest(args, dl) -> None:
    """One JSON object per line with at least source, id, url, licence and taxon (inat-dump --manifest-only writes these)."""
    with open(args.manifest, encoding="utf-8") as fh:
        for line in fh:
            if not line.strip():
                continue
            cand = json.loads(line)
            cand.setdefault("id", cand.get("source_id"))  # older manifests
            if licences.norm(cand.get("licence")) not in dl.allowed:
                continue
            if not dl.take(cand):
                return
