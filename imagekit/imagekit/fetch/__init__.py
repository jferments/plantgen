"""Download candidates into <store>/incoming/<batch>/, skipping anything the store already holds.

Each source module turns a query into candidate dicts with at least
`source`, `id`, `url`, `licence` and `taxon`; `Downloader.take` does the rest.
"""
from __future__ import annotations

import hashlib
import json
import os
import sys
import urllib.parse

from .. import licences, net
from ..store import Store, append_jsonl, jsonl, now


class Downloader:
    def __init__(self, store: Store, batch: str, *, max_mb: float, dry_run: bool, allowed: set[str]):
        self.store, self.batch = store, batch
        self.dir = store.incoming(batch)
        self.log = self.dir / "fetched.jsonl"
        self.max_bytes = max_mb * 1e6
        self.dry_run = dry_run
        self.allowed = allowed
        self.moved = 0
        self.kept = 0
        self.pending = {r["key"]: r for r in jsonl(self.log)}

    def held(self, source: str, taxon: str) -> int:
        """Images of this taxon from this source already in the store or waiting to be ingested."""
        waiting = sum(1 for r in self.pending.values() if r["source"] == source and r.get("taxon") == taxon)
        return self.store.held(source, taxon) + waiting

    def known(self, source: str, ident) -> bool:
        key = f"{source}:{ident}"
        return key in self.pending or self.store.has_ref(key)

    def take(self, cand: dict) -> bool:
        """Download one candidate unless held. False once --max-mb is reached."""
        cand["key"] = f"{cand['source']}:{cand['id']}"
        cand["licence"] = licences.norm(cand.get("licence"))
        if cand["licence"] not in self.allowed or self.known(cand["source"], cand["id"]):
            return True
        if self.moved >= self.max_bytes:
            print(f"stopping at --max-mb ({self.moved / 1e6:.0f} MB this run)", file=sys.stderr)
            return False
        if self.dry_run:
            print(json.dumps(cand, ensure_ascii=False))
            self.pending[cand["key"]] = cand
            self.kept += 1
            return True
        if cand.get("path"):  # a local file (fetch folder)
            with open(cand.pop("path"), "rb") as fh:
                data = fh.read()
        else:
            try:
                data = net.get_bytes(cand["url"])
            except Exception as err:  # noqa: BLE001 - log and go on
                print(f"  skip {cand['key']}: {err}", file=sys.stderr)
                return True
        ext = os.path.splitext(urllib.parse.urlparse(cand.get("url") or "").path)[1].lower()
        rel = f"files/{cand['source']}/{str(cand['id']).replace('/', '_')}{ext if ext in ('.jpg', '.jpeg', '.png', '.webp', '.tif', '.tiff') else '.img'}"
        (self.dir / rel).parent.mkdir(parents=True, exist_ok=True)
        (self.dir / rel).write_bytes(data)
        self.moved += len(data)
        cand.update(file=rel, original_sha256=hashlib.sha256(data).hexdigest(), fetched=now())
        append_jsonl(self.log, [cand])
        self.pending[cand["key"]] = cand
        self.kept += 1
        return True

    def note_command(self, argv: list[str]) -> None:
        if not self.dry_run:
            append_jsonl(self.dir / "commands.jsonl", [{"argv": argv, "at": now()}])
