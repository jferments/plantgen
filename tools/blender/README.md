# Blender import check

Blender is a core host for PlantGen's files, so every kind of `.glb`
PlantGen writes is imported by Blender's own glTF importer, headless, on
every run of CI that could change them (the `blender` job in
`.github/workflows/ci.yml`). A file fails when Blender refuses a file it
should read, holds fewer or more triangles than the file draws (every GPU
instance counted), holds fewer images than the file has, drops its vertex
colours or reads them in no material, or warns of an extension it does not
support. Blender 4.5 cannot read `EXT_meshopt_compression`; it must refuse
such files with that error and nothing else, and the same files after
`plantc unpack` must import whole. Custom attributes (`_PLANTGEN_*`)
Blender keeps or drops are reported, not judged.

| File | What it does |
| --- | --- |
| `get_blender.py` | Fetches Blender 4.5 LTS (version and SHA-256 pinned in the file) into a cache folder once; `--libs` also unpacks the X11 and GL libraries a portable Blender links to, without root (`apt-get download`) |
| `check_exports.py` | Makes the fixed export set with `plantc` (its `CASES`), imports it in Blender, judges each file, writes `report.json` and `REPORT.md`, exits 1 on any failure |
| `in_blender.py` | Runs inside Blender: imports each file into an empty scene with File > Import > glTF 2.0 defaults and prints what Blender holds |
| `glb.py` | Reads what a `.glb` draws from its own JSON (no dependencies) |
| `test_check.py` | Tests of the above with no Blender: `python3 -m unittest tools/blender/test_check.py` |

The set: a yarrow's tiers uncompressed, a young bigleaf maple's tiers
quantized with its bark textures, two yarrow variants, a 10 m tile of the
block woodland (tile file at level 2, 5 m cells at levels 1 and 3, every
prototype) uncompressed and with meshopt, the meshopt folder after
`plantc unpack`, and moss on the test wall at patch detail: 164 files.
Budgets (`--triangles`) keep it quick; the files are the same kinds a full
export writes. A new export format or flag that changes what a file holds
gets a case here.

## Run it yourself

```bash
cargo build --release -p plantc
python3 tools/blender/get_blender.py --cache ~/.cache/plantgen-blender
python3 tools/blender/check_exports.py --out /tmp/blender-check \
    --blender ~/.cache/plantgen-blender/blender-4.5.14/blender
```

On a desktop the libraries are already there; add `--libs` to
`get_blender.py` on a server without them and put the folders it prints on
`LD_LIBRARY_PATH`. Any Blender works with `--blender` (5.2 must read the
meshopt files whole too). `--make-only` makes the files and stops;
`--check-only` imports the files already in `--out`; `--only fill-none,moss`
runs some cases.
