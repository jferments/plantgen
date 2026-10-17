"""`imagekit push` and `imagekit pull`: share a store through data.plantgen.io.

The server keeps the store's own layout, in three private sources of the
plantgen-io catalogue:

    images          one version per fetch batch: sources/images/<batch>/... and
                    sha256/<aa>/<sha256> for each image the batch holds
    image-analyses  one version per run: sources/image-analyses/<run>/... and
                    the run's analyzers (analyzers.jsonl beside its rows)

`push` hands each finished batch and run to `pgio push` (the uploader in
jferments/plantgen-io, with the owner's AWS login or a cloud session's
upload key), then seals it: a sealed
batch or run takes no more rows, so what the server holds never goes stale;
later work goes in a new batch or run. `pull` needs no login, only the
reader key (a cloud session's proxy adds it): it downloads every version the
store lacks, checks each file's hash, and rebuilds the index.
"""
from __future__ import annotations

import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

from .store import SEALED, Store, append_jsonl, jsonl, now

SOURCES = {"images": "batch", "image-analyses": "run"}
BASE = os.environ.get("PGIO_BASE_URL", "https://data.plantgen.io")


def register(sub) -> None:
    p = sub.add_parser("push", help="upload finished batches and runs to data.plantgen.io (needs pgio and its login)")
    p.add_argument("--batch", nargs="*", help="only these batches (default: every one not yet pushed)")
    p.add_argument("--run", nargs="*", help="only these runs (default: every finished one not yet pushed)")
    p.add_argument("--dry-run", action="store_true", help="hash and check with pgio, upload and seal nothing")
    p.set_defaults(func=cmd_push)
    p = sub.add_parser("pull", help="download the batches and runs this store lacks from data.plantgen.io")
    p.add_argument("--batch", nargs="*", help="only these batches")
    p.add_argument("--run", nargs="*", help="only these runs")
    p.add_argument("--no-images", action="store_true", help="batch records only, not the image files")
    p.add_argument("--base", default=BASE)
    p.set_defaults(func=cmd_pull)


# push

def staging_files(store: Store, kind: str, name: str) -> list[tuple[str, Path]]:
    """(path on the server, local file) for one batch or run, in the store's layout."""
    d = store.batch_dir(name) if kind == "batch" else store.run_dir(name)
    out = [(p.relative_to(store.root).as_posix(), p) for p in sorted(d.rglob("*"))
           if p.is_file() and not any(part.startswith(".") for part in p.relative_to(d).parts)]
    if kind == "batch":
        for sha in sorted({r["sha256"] for r in jsonl(d / "images.jsonl")}):
            blob = store.blob(sha)
            if not blob.exists():
                raise SystemExit(f"batch {name}: image {sha} is missing from the store")
            out.append((blob.relative_to(store.root).as_posix(), blob))
    return out


def run_analyzers(store: Store, run: str) -> list[dict]:
    used = {r["analyzer"] for r in jsonl(store.run_dir(run) / "analyses.jsonl")}
    info = store.run_dir(run) / "run.json"
    if info.exists():
        doc = json.loads(info.read_text())
        used |= {doc[k] for k in ("analyzer",) if doc.get(k)}
        used |= set((doc.get("parts") or {}).values()) if isinstance(doc.get("parts"), dict) else set()
    return [r for r in jsonl(store.root / "analyzers.jsonl") if r["id"] in used]


def pgio_command() -> list[str]:
    """`pgio` as installed, else from a plantgen-io checkout beside this one or in ~/code."""
    if os.environ.get("IMAGEKIT_PGIO"):
        return shlex.split(os.environ["IMAGEKIT_PGIO"])
    if shutil.which("pgio"):
        return ["pgio"]
    here = Path(__file__).resolve().parents[2]  # the plantgen checkout
    for d in (here.parent / "plantgen-io", Path.home() / "code" / "plantgen-io"):
        if (d / "uploader" / "pgio" / "cli.py").exists():
            boot = f"import sys; sys.path.insert(0, {str(d / 'uploader')!r}); from pgio.cli import main; main()"
            return [sys.executable, "-c", boot]
    raise SystemExit("pgio not found: install it (pipx install ~/code/plantgen-io/uploader) or set IMAGEKIT_PGIO")


def push_one(store: Store, kind: str, name: str, dry_run: bool) -> dict:
    source = "images" if kind == "batch" else "image-analyses"
    stage = store.root / ".push" / source / name
    shutil.rmtree(stage, ignore_errors=True)
    for rel, src in staging_files(store, kind, name):
        dst = stage / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        os.symlink(src.resolve(), dst)
    if kind == "run":
        rows = run_analyzers(store, name)
        p = stage / "sources" / "image-analyses" / name / "analyzers.jsonl"
        p.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
    cmd = [*pgio_command(), "push", source, name, str(stage)] + (["--dry-run"] if dry_run else [])
    print("$ " + " ".join(shlex.quote(c) for c in cmd), file=sys.stderr)
    res = subprocess.run(cmd)
    shutil.rmtree(stage, ignore_errors=True)
    if res.returncode:
        raise SystemExit(f"pgio push {source} {name} failed (exit {res.returncode}); nothing was sealed")
    d = store.batch_dir(name) if kind == "batch" else store.run_dir(name)
    if not dry_run:
        (d / SEALED).write_text(json.dumps({"source": source, "version": name, "pushed": now()}) + "\n")
    return {"source": source, "version": name, "dry_run": dry_run}


def cmd_push(args, store: Store) -> None:
    def pick(names, asked, kind):
        chosen = asked if asked is not None else [n for n in names if not store.sealed(kind, n)]
        unknown = set(chosen) - set(names)
        if unknown:
            raise SystemExit(f"no such {kind}: {', '.join(sorted(unknown))}")
        return chosen
    only_runs = args.run is not None and args.batch is None
    only_batches = args.batch is not None and args.run is None
    batches = [] if only_runs else pick(store.batches(), args.batch, "batch")
    runs = [] if only_batches else pick(store.runs(), args.run, "run")
    waiting = [r for r in runs if any(store.run_dir(r).glob("pending-*.json"))]
    if waiting:
        print(f"skipping runs with Claude batches still to collect: {', '.join(waiting)}", file=sys.stderr)
        runs = [r for r in runs if r not in waiting]
    if not batches and not runs:
        print("nothing to push: every batch and run is already on data.plantgen.io")
        return
    done = [push_one(store, "batch", b, args.dry_run) for b in batches]
    done += [push_one(store, "run", r, args.dry_run) for r in runs]
    print(json.dumps({"pushed": len(done), "dry_run": args.dry_run,
                      "batches": batches, "runs": runs}, indent=1))


# pull

def _get(url: str):
    req = urllib.request.Request(url, headers={"User-Agent": "imagekit"})
    key = os.environ.get("PGIO_READER_KEY")
    if key:
        req.add_header("Authorization", f"Bearer {key}")
    return urllib.request.urlopen(req, timeout=120)


def get_json(url: str):
    with _get(url) as r:
        return json.load(r)


def server_versions(base: str) -> dict:
    """source -> its version names, from the private index."""
    try:
        index = get_json(f"{base}/private/sources/index.json")
    except urllib.error.HTTPError as err:
        hint = " (a reader key is needed: PGIO_READER_KEY)" if err.code in (401, 403) else ""
        raise SystemExit(f"cannot read the index on {base}: HTTP {err.code}{hint}") from err
    return {s: sorted(index.get("sources", {}).get(s, {})) for s in SOURCES}


def download(base: str, f: dict, target: Path) -> None:
    if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() == f["sha256"]:
        return
    sha = f["sha256"]
    key = f.get("key") or f"private/sha256/{sha[:2]}/{sha}"
    with _get(base.rstrip("/") + "/" + key) as r:
        data = r.read()
    if hashlib.sha256(data).hexdigest() != sha:
        raise SystemExit(f"{f['path']}: hash mismatch; nothing kept")
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_name(target.name + ".part")
    tmp.write_bytes(data)
    tmp.replace(target)


def pull_one(store: Store, base: str, source: str, version: str, images: bool) -> int:
    manifest = get_json(f"{base}/private/sources/{source}/{version}/manifest.json")
    n = 0
    for f in manifest["files"]:
        path = f["path"]
        target = (store.root / path).resolve()
        if not target.is_relative_to(store.root.resolve()) or path.startswith("."):
            raise SystemExit(f"unsafe path in the manifest of {source} {version}: {path}")
        if path.startswith("sha256/") and not images:
            continue
        if source == "image-analyses" and path.endswith("/analyzers.jsonl"):
            known = {r["id"] for r in jsonl(store.root / "analyzers.jsonl")}
            tmp = store.root / ".pull-analyzers.jsonl"
            download(base, f, tmp)
            append_jsonl(store.root / "analyzers.jsonl", [r for r in jsonl(tmp) if r["id"] not in known])
            tmp.unlink()
            continue
        download(base, f, target)
        n += 1
    d = store.root / "sources" / source / version
    d.mkdir(parents=True, exist_ok=True)
    (d / SEALED).write_text(json.dumps({"source": source, "version": version, "pulled": now()}) + "\n")
    return n


def cmd_pull(args, store: Store) -> None:
    have = {"images": set(store.batches()), "image-analyses": set(store.runs())}
    wanted = {"images": args.batch, "image-analyses": args.run}
    got = {}
    for source, versions in server_versions(args.base).items():
        if wanted["images"] is not None and wanted["image-analyses"] is None and source != "images":
            continue
        if wanted["image-analyses"] is not None and wanted["images"] is None and source != "image-analyses":
            continue
        todo = [v for v in versions if v not in have[source] and (wanted[source] is None or v in wanted[source])]
        for v in todo:
            got[f"{source}/{v}"] = pull_one(store, args.base, source, v, not args.no_images)
    counts = store.reindex()
    print(json.dumps({"pulled": got, **counts}, indent=1))
