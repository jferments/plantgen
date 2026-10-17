# imagekit

Fetch open-licence plant images with their licences and credits, keep one
record per image, and never analyse an image twice.

imagekit gathers photos for [`photo-rules/`](../photo-rules/README.md):
the rules a vision model follows to read plant photos and compare them with
PlantGen's renders. It is a contributor tool. Nothing in `plantgen` or
`plantc` uses it, and it is the only part of this repository that uses the
network.

What it does now:
- **fetch** from iNaturalist (API and the open-data dump), GBIF, Wikimedia
  Commons, archive.org, your own folders, or a manifest;
- **ingest** each download: hash it, merge duplicates across sources, cap
  it at 2048 px, strip camera metadata (GPS included), and record where it
  came from;
- keep **analyses** keyed by image and analyzer, so a model run skips every
  image that already has an answer from the same model, rules and settings.

Running vision models on the images (local servers, OpenRouter, the Claude
API) and searching the library by look come next.

## Install

Python 3.10 or newer.

```sh
cd ~/code/plantgen
uv tool install ./imagekit            # or: pipx install ./imagekit
imagekit --help
```

Add `[parquet]` (`uv tool install './imagekit[parquet]'`) to also write
each batch as Parquet.

## Fetch

```sh
imagekit fetch inat --taxon "Acer circinatum" --n 60
imagekit fetch inat --taxon "Camassia quamash" --n 40 --phenology flowering
imagekit fetch gbif --taxon "Acer circinatum" --n 30 --basis PRESERVED_SPECIMEN
imagekit fetch commons --taxon "Acer circinatum" --n 20
imagekit fetch archive --identifier illustratedflor03brit --pages 101,102
imagekit fetch folder ~/Pictures/plants --licence cc-by --creator "Your Name" --taxon-from-folders
```

- **iNaturalist:** research grade and wild by default. `--spread months`
  (the default) takes half the most-voted photos and spreads the other half
  over the months the species is seen in, busiest months first. At most
  `--per-observer` photos (default 2) come from one person, and one photo
  from one observation, so no single photographer or plant sets a species'
  look. `--size large` is 1024 px; `original` is up to 2048 px.
- **Many species:** `fetch inat-dump --dump DIR --taxa names.txt` reads
  iNaturalist's open-data dump instead of the API. Run it where the images
  will be processed. `--manifest-only FILE` writes a small list that
  `fetch manifest FILE` downloads somewhere else.
- **Reruns top up:** `--n` is a total per taxon. Nothing already held is
  downloaded again.
- **Limits:** `--max-mb` (default 1000) stops a run. `--dry-run` lists
  what would come and downloads nothing.
- **Commons:** reads the files directly in the category. Many species keep
  most of their files in subcategories, so name one with `--category`.
- **GBIF:** images sit on each herbarium's own server, and some fail.

## Licences

The default is CC0, CC BY, CC BY-SA and public domain. Non-commercial
photos (`--licences cc0,cc-by,cc-by-sa,cc-by-nc`) are opt-in. Use them only
to measure, and keep only the numbers. Every record carries the licence,
the creator and an attribution line.

The images are other people's work. Keep them in your store and never
commit them to a public repository. What PlantGen keeps from them, in
`library/`, is numbers and words of its own, with the photo ids that back
them.

## The store

Everything goes into one folder: `~/plantgen-images`, `$IMAGEKIT_STORE`
or `--store`. It has the layout of `data.plantgen.io`'s private side:

```text
sha256/<aa>/<sha256>                  each image's bytes, once
sources/images/<batch>/               one fetch batch
  batch.json                          the commands, counts by source, licence and taxon
  images.jsonl                        one row per download (schemas/image-row.schema.json)
sources/image-analyses/<run>/         one analysis run
  run.json, analyses.jsonl            (schemas/analysis.schema.json)
analyzers.jsonl                       each analyzer id's definition (schemas/analyzer.schema.json)
incoming/<batch>/                     downloads not yet ingested
index.sqlite                          a lookup index; `imagekit reindex` rebuilds it from the files
```

**One image, one key.** An image is named by the SHA-256 of its stored
bytes. A download whose bytes match an image already held, or whose look
matches it (a 64-bit difference hash within 3 bits, same shape), is
recorded as a second source of that image. It is not stored or analysed
again.

**Analyzers.** An analyzer is everything that decides an answer except the
image: the task, the model and its exact revision, the `photo-rules/`
commit, the prompt's hash and the settings. Its id is a hash of those.

```sh
imagekit analyzer-id triage-haiku.json --register
imagekit missing --analyzer 3f1c0a9e5b7d2c44     # how many images still lack it
imagekit import-analyses pilot.jsonl --run pilot-2026-10-14
```

`missing` lists the only images a run may send. `import-analyses` takes
in answers made elsewhere, such as a pilot or a chat session, so they are
not paid for twice.

## Tests

```sh
cd ~/code/plantgen/imagekit && python3 -m unittest discover -s tests
```

They fake the network and need only Pillow (and jsonschema for the schema
test).
