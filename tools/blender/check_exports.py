#!/usr/bin/env python3
"""Check that Blender imports every kind of file PlantGen exports, whole.

    python3 tools/blender/check_exports.py --out DIR [--blender PATH] [--plantc PATH]
        [--jobs N] [--make-only | --check-only] [--only CASE,...]

Makes a small fixed set of exports with plantc (CASES below: a herb's tiers
uncompressed, a young tree's tiers quantized with its bark textures, two
variants, a 10 m woodland tile with its tile file, cells and prototypes in
`none` and in `meshopt`, the meshopt folder after `plantc unpack`, and a
moss site), imports each .glb headless in Blender (in_blender.py, File >
Import > glTF 2.0 with its defaults) and compares what Blender holds with
what the file says it holds (glb.py). A file fails when Blender:

- refuses it, unless it needs EXT_meshopt_compression and the Blender is
  older than 5.2, which cannot read meshopt (`plantc unpack` is the path
  there, and its output must import whole);
- holds a different number of triangles (every GPU instance counted);
- holds fewer images with pixels than the file has;
- drops vertex colours (COLOR_0), or no material reads them;
- warns of an extension it does not support.

Custom attributes (`_PLANTGEN_*`) Blender does or does not keep are
reported, not judged. Writes DIR/report.json and DIR/REPORT.md, prints
one line per case, and exits 1 if any file fails.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import glb  # noqa: E402

REPO = HERE.parent.parent
MARK = "PLANTGEN_BLENDER "
MESHOPT = "EXT_meshopt_compression"
# The first Blender seen to read meshopt (2026-11-06: 5.2.3 did, 4.5.9 did not).
MESHOPT_FROM = (5, 2)

# Budgets keep the set quick to grow and import; the files are the same kind.
TRIS = "60000,20000,5000,1000"
FILL = [
    "fill", "docs/sites/block-fill/woodland.json", "--tile", "0,0", "--tile-m", "10",
    "--years", "8", "--ages", "8", "--quality", "draft", "--triangles", "40000,10000,2500,600",
    "--glb", "2", "--glb-cells", "1,3", "--cell-m", "5",
]

# name: (what it is, steps). A step is a plantc argument list ({out} is the
# case's folder, {set} the whole set's) or ("seed", case): start from that
# case's folder without its .glb files, so a fill measures nothing again.
CASES: dict[str, tuple[str, list]] = {
    "export-none": (
        "yarrow tiers, --compress none",
        [["export", "achillea-millefolium", "--age", "2", "--levels", "tiers",
          "--triangles", TRIS, "--compress", "none", "--out", "{out}"]],
    ),
    "export-bark-quantize": (
        "bigleaf maple tiers with bark textures, --compress quantize",
        [["export", "acer-macrophyllum", "--age", "6", "--levels", "tiers", "--quality", "draft",
          "--bark-look", "spec", "--triangles", TRIS, "--compress", "quantize", "--out", "{out}"]],
    ),
    "variants-quantize": (
        "two yarrow variants, --compress quantize",
        [["variants", "achillea-millefolium", "--count", "2", "--age", "2",
          "--triangles", TRIS, "--compress", "quantize", "--out", "{out}"]],
    ),
    "fill-none": (
        "10 m woodland tile: tile file, cells at levels 1 and 3, prototypes; --compress none",
        [FILL + ["--compress", "none", "--out", "{out}"]],
    ),
    "fill-meshopt": (
        "the same tile, --compress meshopt",
        [("seed", "fill-none"), FILL + ["--compress", "meshopt", "--out", "{out}"]],
    ),
    "fill-unpacked": (
        "the meshopt tile after plantc unpack",
        [["unpack", "{set}/fill-meshopt", "--out", "{out}"]],
    ),
    "moss": (
        "moss on the test wall, --glb at patch detail",
        [["moss", "docs/sites/moss-wall.json", "--years", "10", "--detail", "patch",
          "--glb", "{out}/moss-wall-patch.glb"]],
    ),
}


def make(plantc: Path, out: Path, names: list[str]) -> dict[str, float]:
    times = {}
    for name in names:
        case = out / name
        shutil.rmtree(case, ignore_errors=True)
        case.mkdir(parents=True)
        start = time.monotonic()
        for step in CASES[name][1]:
            if isinstance(step, tuple):
                src = out / step[1]
                shutil.rmtree(case)
                shutil.copytree(src, case, ignore=shutil.ignore_patterns("*.glb"))
                continue
            argv = [str(plantc)] + [a.format(out=case, set=out) for a in step]
            print("$ " + " ".join(argv[1:]), flush=True)
            log = case.parent / f"{name}.plantc.log"
            with log.open("a") as f:
                r = subprocess.run(argv, cwd=REPO, stdout=f, stderr=subprocess.STDOUT)
            if r.returncode != 0:
                sys.stdout.write(log.read_text()[-4000:])
                raise SystemExit(f"plantc failed making {name} (exit {r.returncode}); log {log}")
        times[name] = round(time.monotonic() - start, 1)
    return times


def run_blender(blender: str, files: list[Path], jobs: int, timeout: int) -> tuple[str, dict]:
    """Import files in `jobs` Blender processes; the version and one result per file."""
    groups: list[list[Path]] = [[] for _ in range(max(1, jobs))]
    sizes = [0] * len(groups)
    for f in sorted(files, key=lambda p: -p.stat().st_size):
        i = sizes.index(min(sizes))
        groups[i].append(f)
        sizes[i] += f.stat().st_size
    cmd = [blender, "-b", "--factory-startup", "--python-exit-code", "1",
           "--python", str(HERE / "in_blender.py"), "--"]
    procs = [
        subprocess.Popen(cmd + [str(f) for f in g], stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True)
        for g in groups if g
    ]
    version = "?"
    results: dict[str, dict] = {}
    tails = []
    deadline = time.monotonic() + timeout
    for p in procs:
        try:
            text, _ = p.communicate(timeout=max(1, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            p.kill()
            text, _ = p.communicate()
            tails.append(f"Blender stopped after {timeout} s")
        for line in text.splitlines():
            if line.startswith(MARK):
                rec = json.loads(line[len(MARK):])
                if "blender" in rec:
                    version = rec["blender"]
                else:
                    results[rec["file"]] = rec
        if p.returncode not in (0, None):
            tails.append("\n".join(text.splitlines()[-30:]))
    for t in tails:
        print(t)
    return version, results


def version_tuple(v: str) -> tuple[int, ...]:
    return tuple(int(x) for x in v.split(" ")[0].split(".")[:2] if x.isdigit())


def judge(want: dict, got: dict | None, version: str) -> tuple[str, list[str]]:
    """'pass', 'refused' (a meshopt file an old Blender cannot read) or 'fail', and why."""
    if got is None:
        return "fail", ["no result: Blender stopped before it reported this file"]
    needs_meshopt = MESHOPT in want["extensions_required"]
    old = version_tuple(version) < MESHOPT_FROM
    if got["error"]:
        if needs_meshopt and old and MESHOPT in got["error"]:
            return "refused", [f"Blender {version} does not read {MESHOPT}: {got['error']}"]
        return "fail", [f"import refused: {got['error']}"]
    why = []
    if got["triangles"] != want["triangles"]:
        why.append(f"triangles: Blender holds {got['triangles']}, the file draws {want['triangles']}")
    if got["images"] < want["images"]:
        why.append(f"images: Blender holds {got['images']} with pixels, the file has {want['images']}")
    if want["vertex_colours"]:
        if got["coloured_meshes"] == 0:
            why.append("vertex colours (COLOR_0) were dropped")
        elif not got["material_reads_colour"]:
            why.append("vertex colours were imported but no material reads them")
    ext = [w for w in got["warnings"] if "extension" in w.lower()]
    if ext:
        why.append("extension warnings: " + "; ".join(ext))
    return ("fail" if why else "pass"), why


def kept_attributes(want: dict, got: dict | None) -> dict:
    held = set((got or {}).get("attributes", []))
    names = want["custom_attributes"]
    return {"kept": [a for a in names if a in held], "dropped": [a for a in names if a not in held]}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--blender", default=os.environ.get("BLENDER", "blender"))
    ap.add_argument("--plantc", type=Path, default=REPO / "target/release/plantc")
    ap.add_argument("--jobs", type=int, default=min(4, os.cpu_count() or 1))
    ap.add_argument("--timeout", type=int, default=1500, help="seconds for all Blender imports")
    ap.add_argument("--only", help="comma-separated case names")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--make-only", action="store_true", help="make the files, run no Blender")
    mode.add_argument("--check-only", action="store_true", help="import the files already in --out")
    a = ap.parse_args()
    names = a.only.split(",") if a.only else list(CASES)
    for n in names:
        if n not in CASES:
            raise SystemExit(f"no case {n}; cases: {', '.join(CASES)}")
    out = a.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    make_s = {} if a.check_only else make(a.plantc.resolve(), out, names)
    if a.make_only:
        return 0

    files = {n: sorted((out / n).rglob("*.glb")) for n in names}
    for n, fs in files.items():
        if not fs:
            raise SystemExit(f"case {n} has no .glb files in {out / n}")
    every = [f for fs in files.values() for f in fs]
    start = time.monotonic()
    version, results = run_blender(a.blender, every, a.jobs, a.timeout)
    blender_s = round(time.monotonic() - start, 1)

    report = {"blender": version, "make_seconds": make_s, "blender_seconds": blender_s, "cases": {}}
    failed = 0
    lines = [f"# Blender import check\n\nBlender {version}; {len(every)} files, "
             f"imported in {blender_s} s.\n",
             "| Case | What | Files | Pass | Refused (meshopt) | Fail | Triangles | Images | Custom attributes dropped |",
             "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    for n in names:
        rows = []
        for f in files[n]:
            want = glb.summarise(str(f))
            got = results.get(str(f))
            verdict, why = judge(want, got, version)
            rows.append({"file": str(f.relative_to(out)), "verdict": verdict, "why": why,
                         "file_says": want, "blender": got, "attributes": kept_attributes(want, got)})
        count = {v: sum(r["verdict"] == v for r in rows) for v in ("pass", "refused", "fail")}
        failed += count["fail"]
        tris = sum(r["file_says"]["triangles"] for r in rows)
        imgs = sum(r["file_says"]["images"] for r in rows)
        dropped = sorted({a for r in rows for a in r["attributes"]["dropped"]})
        report["cases"][n] = {"what": CASES[n][0], "count": count, "files": rows}
        lines.append(f"| {n} | {CASES[n][0]} | {len(rows)} | {count['pass']} | {count['refused']} | "
                     f"{count['fail']} | {tris:,} | {imgs} | {', '.join(dropped) or 'none'} |")
        print(f"{n}: {len(rows)} files, {count['pass']} pass, {count['refused']} refused (meshopt), "
              f"{count['fail']} fail")
        for r in rows:
            if r["verdict"] == "fail":
                print(f"  FAIL {r['file']}: " + "; ".join(r["why"]))
    fails = [r for c in report["cases"].values() for r in c["files"] if r["verdict"] == "fail"]
    if fails:
        lines.append("\n## Failures\n")
        lines += [f"- `{r['file']}`: " + "; ".join(r["why"]) for r in fails]
    (out / "report.json").write_text(json.dumps(report, indent=1))
    (out / "REPORT.md").write_text("\n".join(lines) + "\n")
    print(f"Blender {version}: {len(every)} files, {failed} failed. Report: {out / 'REPORT.md'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
