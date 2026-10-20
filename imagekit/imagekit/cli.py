"""imagekit command line."""
from __future__ import annotations

import argparse
import importlib
import json
import sys
import time
from pathlib import Path

from . import __version__, analyzer, licences
from .fetch import Downloader, archive, commons, gbif, inat, inat_dump, local
from .ingest import MAX_PX, ingest
from .store import DEFAULT_ROOT, Store, check_open, jsonl

# Commands that live in their own modules, each with register(subparsers). Their
# heavy imports (models, numpy) happen inside the command, so `fetch` stays light.
COMMAND_MODULES = ("run", "views", "embed", "search", "sync", "render", "compare", "regions", "segment", "tally", "library", "measure", "count")

FETCHERS = {
    "inat": inat.fetch, "inat-dump": inat_dump.fetch, "gbif": gbif.fetch, "commons": commons.fetch,
    "archive": archive.fetch, "folder": local.folder, "manifest": local.manifest,
}


def add_fetch(sub) -> None:
    f = sub.add_parser("fetch", help="download images with their licences into the store")
    srcs = f.add_subparsers(dest="source", required=True)

    def common(p, taxon_required=True):
        p.add_argument("--n", type=int, default=60, help="images wanted per taxon; a rerun tops up to this (default 60)")
        p.add_argument("--licences", default=",".join(licences.DEFAULT),
                       help="comma-separated; default cc0,cc-by,cc-by-sa,pd. Add cc-by-nc only if you keep just "
                            "measurements from those images")
        p.add_argument("--batch", help="batch name (default <source>-<date>)")
        p.add_argument("--max-mb", type=float, default=1000, help="stop after this many MB (default 1000)")
        p.add_argument("--seed", type=int, default=1)
        p.add_argument("--dry-run", action="store_true", help="list what would be fetched; download nothing")
        p.add_argument("--no-ingest", action="store_true", help="leave downloads in incoming/ for `imagekit ingest`")
        p.add_argument("--wcvp", help="WCVP wcvp_names.csv, to resolve names to accepted ones at ingest")
        if taxon_required is not None:
            p.add_argument("--taxon", required=taxon_required, default="", help="scientific name, e.g. 'Acer circinatum'")

    p = srcs.add_parser("inat", help="iNaturalist API (research grade, wild)")
    common(p)
    p.add_argument("--spread", choices=["months", "votes", "random"], default="months",
                   help="months: half most-voted, half spread over the months it is seen (default)")
    p.add_argument("--phenology", choices=sorted(inat.PHENOLOGY), help="only observations annotated so")
    p.add_argument("--months", help="only these months, e.g. 4,5,6")
    p.add_argument("--place-id", help="an iNaturalist place id")
    p.add_argument("--per-observer", type=int, default=2)
    p.add_argument("--photos-per-obs", type=int, default=1)
    p.add_argument("--size", choices=["medium", "large", "original"], default="large",
                   help="large is 1024 px, original up to 2048 px")
    p.add_argument("--include-captive", action="store_true")

    p = srcs.add_parser("inat-dump", help="iNaturalist open-data dump, many species at once")
    common(p, taxon_required=None)
    p.add_argument("--dump", required=True, help="folder with taxa.csv, observations.csv, photos.csv, observers.csv")
    p.add_argument("--taxa", required=True, help="file with one scientific name per line")
    p.add_argument("--per-observer", type=int, default=2)
    p.add_argument("--size", choices=["medium", "large", "original"], default="large")
    p.add_argument("--manifest-only", metavar="FILE", help="write a manifest instead of downloading")

    p = srcs.add_parser("gbif", help="GBIF occurrence images (herbarium sheets with --basis PRESERVED_SPECIMEN)")
    common(p)
    p.add_argument("--basis", help="e.g. PRESERVED_SPECIMEN or HUMAN_OBSERVATION")
    p.add_argument("--country", help="two-letter country code")

    p = srcs.add_parser("commons", help="Wikimedia Commons category")
    common(p, taxon_required=False)
    p.add_argument("--category", help="category name (default: the taxon)")
    p.add_argument("--width", type=int, default=2048)

    p = srcs.add_parser("archive", help="archive.org book pages (public domain)")
    common(p, taxon_required=False)
    p.add_argument("--identifier", required=True)
    p.add_argument("--pages", required=True, help="page numbers, e.g. 101,102")
    p.add_argument("--width", type=int, default=2048)

    p = srcs.add_parser("folder", help="your own photos")
    common(p, taxon_required=False)
    p.add_argument("path")
    p.add_argument("--licence", required=True, help="the licence you give them, e.g. cc-by")
    p.add_argument("--creator", required=True, help="who took them, as they should be credited")
    p.add_argument("--source-name", default="own", help="source label (default own)")
    p.add_argument("--taxon-from-folders", action="store_true", help="each image's folder names its taxon")

    p = srcs.add_parser("manifest", help="a JSON Lines manifest (from inat-dump --manifest-only)")
    common(p, taxon_required=None)
    p.add_argument("manifest")


def cmd_fetch(args, store) -> None:
    args.licences = licences.parse(args.licences)
    if hasattr(args, "pages") and isinstance(args.pages, str):
        args.pages = [int(x) for x in args.pages.split(",") if x]
    if args.source == "folder":
        args.licences |= {licences.norm(args.licence)}
        args.licence = licences.norm(args.licence)
    batch = args.batch or store.unsealed(
        "batch", f"{args.source if args.source != 'inat-dump' else 'inat'}-{time.strftime('%Y-%m-%d')}")
    check_open(store.batch_dir(batch))
    dl = Downloader(store, batch, max_mb=args.max_mb, dry_run=args.dry_run, allowed=args.licences)
    dl.note_command(sys.argv[1:])
    FETCHERS[args.source](args, dl)
    print(f"fetched {dl.kept} ({dl.moved / 1e6:.1f} MB) into batch {batch}", file=sys.stderr)
    if not (args.dry_run or args.no_ingest or getattr(args, "manifest_only", None)):
        report(batch, ingest(store, batch, wcvp=args.wcvp))


def report(batch: str, tally) -> None:
    print(f"ingested {batch}: " + ", ".join(f"{v} {k}" for k, v in sorted(tally.items())) if tally else
          f"ingested {batch}: nothing new", file=sys.stderr)


def cmd_ingest(args, store) -> None:
    batches = args.batch
    if not batches:
        inc = store.root / "incoming"
        batches = sorted(p.name for p in inc.iterdir() if p.is_dir()) if inc.exists() else []
    for b in batches:
        report(b, ingest(store, b, max_px=args.max_px, wcvp=args.wcvp, keep=args.keep))


def cmd_status(args, store) -> None:
    print(json.dumps(store.stats(), indent=1, ensure_ascii=False))


def cmd_reindex(args, store) -> None:
    print(json.dumps(store.reindex()))


def cmd_analyzer(args, store) -> None:
    defn = json.load(open(args.definition, encoding="utf-8"))
    if args.register:
        print(analyzer.register(store, defn))
    else:
        print(analyzer.analyzer_id(defn))


def cmd_missing(args, store) -> None:
    shas = store.all_images()
    if args.batch:
        shas = sorted({r["sha256"] for b in args.batch for r in jsonl(store.batch_dir(b) / "images.jsonl")})
    todo = store.missing(args.analyzer, shas, args.region)
    print(f"{len(todo)} of {len(shas)} images lack analyzer {args.analyzer}", file=sys.stderr)
    if args.list:
        print("\n".join(todo))


def cmd_import(args, store) -> None:
    rows = list(jsonl(Path(args.file)))
    print(json.dumps(analyzer.import_analyses(store, args.run, rows)))


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(prog="imagekit", description=__doc__)
    ap.add_argument("--version", action="version", version=f"imagekit {__version__}")
    ap.add_argument("--store", default=DEFAULT_ROOT, help=f"store folder (default {DEFAULT_ROOT}, or $IMAGEKIT_STORE)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    add_fetch(sub)
    p = sub.add_parser("ingest", help="hash, merge duplicates and record downloads waiting in incoming/")
    p.add_argument("batch", nargs="*")
    p.add_argument("--max-px", type=int, default=MAX_PX)
    p.add_argument("--wcvp")
    p.add_argument("--keep", action="store_true", help="keep the downloads in incoming/")
    sub.add_parser("status", help="what the store holds")
    sub.add_parser("reindex", help="rebuild index.sqlite from the batch and run files")
    p = sub.add_parser("analyzer-id", help="the id of an analyzer definition (JSON file)")
    p.add_argument("definition")
    p.add_argument("--register", action="store_true", help="also record it in analyzers.jsonl")
    p = sub.add_parser("missing", help="images with no analysis under an analyzer: the only ones a run may send")
    p.add_argument("--analyzer", required=True)
    p.add_argument("--region", type=int, default=0)
    p.add_argument("--batch", nargs="*")
    p.add_argument("--list", action="store_true")
    p = sub.add_parser("import-analyses", help="add analyses made elsewhere as a run (JSON Lines)")
    p.add_argument("file")
    p.add_argument("--run", required=True)
    for name in COMMAND_MODULES:  # each adds its parsers with set_defaults(func=...)
        importlib.import_module(f"imagekit.{name}").register(sub)
    args = ap.parse_args(argv)
    run = getattr(args, "func", None) or {
        "fetch": cmd_fetch, "ingest": cmd_ingest, "status": cmd_status, "reindex": cmd_reindex,
        "analyzer-id": cmd_analyzer, "missing": cmd_missing, "import-analyses": cmd_import}[args.cmd]
    with Store(args.store) as store:
        run(args, store)
