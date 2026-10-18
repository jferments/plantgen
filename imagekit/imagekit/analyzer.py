"""Analyzer ids: the key that keeps any image from being analysed, and paid for, twice.

An analyzer is everything that decides an analysis's output except the
image: the task, the model and its exact version, the rules version, the
prompt and the settings. Its id is the first 16 hex digits of the SHA-256
of its canonical JSON. An analysis is stored under (image sha256, region,
analyzer id); before any model call, `Store.missing` drops the images that
already have one.
"""
from __future__ import annotations

import hashlib
import json

from .store import Store, append_jsonl, jsonl, now

TASKS = ("triage", "observe", "check", "describe", "detect", "count", "measure", "embed")
REQUIRED = ("task", "model", "model_revision")
KEYS = REQUIRED + ("rules_version", "prompt_hash", "params")


def canonical(defn: dict) -> dict:
    missing = [k for k in REQUIRED if not defn.get(k)]
    if missing:
        raise ValueError(f"analyzer needs {', '.join(missing)}")
    if defn["task"] not in TASKS:
        raise ValueError(f"unknown task {defn['task']!r} (one of {', '.join(TASKS)})")
    extra = sorted(set(defn) - set(KEYS) - {"id", "note"})
    if extra:
        raise ValueError(f"unknown analyzer keys: {', '.join(extra)}")
    return {k: defn[k] for k in KEYS if defn.get(k) not in (None, "", {})}


def analyzer_id(defn: dict) -> str:
    text = json.dumps(canonical(defn), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def prompt_hash(prompt: str) -> str:
    """The assembled prompt, without the image."""
    return hashlib.sha256(prompt.encode()).hexdigest()[:16]


def register(store: Store, defn: dict) -> str:
    """Record the analyzer's definition in analyzers.jsonl once; return its id."""
    aid = analyzer_id(defn)
    path = store.root / "analyzers.jsonl"
    if not any(r["id"] == aid for r in jsonl(path)):
        append_jsonl(path, [{"id": aid, **canonical(defn), **({"note": defn["note"]} if defn.get("note") else {}),
                             "registered": now()}])
    return aid


def import_analyses(store: Store, run: str, rows: list[dict], run_info: dict | None = None) -> dict:
    """Add analyses made elsewhere (a pilot, a chat session) as a run, so they are never redone.

    Each row: sha256 or a source `key` the store holds (the key is used when
    the sha256 is not one the store holds), region (default 0),
    analyzer (a definition dict, or the id of one already registered), and
    out (the answer). Optional: status, confidence, tokens_in, tokens_out,
    usd, seconds, raw.
    """
    d = store.run_dir(run)
    out, skipped = [], 0
    for r in rows:
        sha = r.get("sha256")
        if (not sha or not store.has_image(sha)) and r.get("key"):  # e.g. hashed from the source's original bytes
            hit = store.db.execute("select sha256 from refs where key=?", (r["key"],)).fetchone()
            sha = hit[0] if hit else None
        if not sha or not store.has_image(sha):
            skipped += 1
            continue
        aid = register(store, r["analyzer"]) if isinstance(r["analyzer"], dict) else r["analyzer"]
        task = r["analyzer"]["task"] if isinstance(r["analyzer"], dict) else r.get("task")
        if not store.missing(aid, [sha], r.get("region", 0)):
            skipped += 1
            continue
        row = {"sha256": sha, "region": r.get("region", 0), "analyzer": aid, "task": task,
               "status": r.get("status", "ok"), "out": r.get("out"), "run": run, "at": r.get("at") or now()}
        for k in ("confidence", "tokens_in", "tokens_out", "usd", "seconds"):
            if r.get(k) is not None:
                row[k] = r[k]
        store.index_analysis(row, run)
        out.append(row)
    append_jsonl(d / "analyses.jsonl", out)
    if out:
        info = json.loads((d / "run.json").read_text()) if (d / "run.json").exists() else {"run": run, "created": now()}
        info.update(run_info or {})
        info["rows"] = info.get("rows", 0) + len(out)
        info["updated"] = now()
        (d / "run.json").write_text(json.dumps(info, indent=1, ensure_ascii=False) + "\n")
    store.commit()
    return {"added": len(out), "skipped": skipped}
