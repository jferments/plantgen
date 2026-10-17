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
  image that already has an answer from the same model, rules and settings;
- **run** a `photo-rules/` task (triage, observe, describe) on the images
  that lack it, through a model on your own machine, OpenRouter, the Claude
  API or a chat window, with the cost estimated first and capped;
- **embed** each image once per model (SigLIP2, DINOv2, BioCLIP-2) and
  **search** the library by words or by a photo.

## Install

Python 3.11 or newer.

```sh
cd ~/code/plantgen
uv tool install ./imagekit            # or: pipx install ./imagekit
imagekit --help
```

Extras: `[parquet]` writes each batch as Parquet too, `[claude]` adds
the Claude API's SDK, and `[local]` adds torch and the image models for
`embed` and `search` (`uv tool install './imagekit[local,claude]'`).

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
<batch or run>/.pushed.json           the seal `imagekit push` and `pull` write: no more rows
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

## Run a model

```sh
imagekit plan triage --backend workstation        # images to send, and the cost
imagekit run triage --backend workstation
imagekit run observe --backend workstation        # uses the triage's views
imagekit run describe --backend openrouter --model qwen/qwen3-vl-235b-a22b-instruct --max-usd 3
```

Every request is built from `photo-rules/` the same way for every model:
`00-ground-rules.md` as the system text, then the task file (for
`observe`, the palette and the features of the views triage found), an
opaque photo id, the answer's JSON Schema and one image. The species is
never sent. An answer that breaks the schema is asked for once more with
the problems listed; if it still fails it is kept with status `invalid`
(`--retry-invalid` sends those again). Each answer goes in the run's
`analyses.jsonl` with its tokens, cost and the model the server named, and
the full reply in `raw/answers.jsonl.gz`.

**Nothing twice.** Before sending, the run drops every image that already
has an answer under the same analyzer. A second run with the same model,
rules and settings sends nothing; a new model, a changed rule file or
another image size is a new analyzer.

**Cost.** `plan` and `run` estimate the cost from earlier answers under the
same analyzer, else from list prices. Over $1 a run needs `--yes`, and it
stops once `--max-usd` (default $5) is spent. Local servers cost nothing.

**Backends** are named in `~/.config/imagekit/backends.toml`. Keys are
read only from the environment:

```toml
[workstation]                 # vLLM, SGLang, llama.cpp, Ollama, LM Studio
kind = "openai"
base_url = "http://localhost:8000/v1"
model = "Qwen/Qwen3-VL-8B-Instruct"
revision = "<the Hugging Face commit>"
parallel = 8

[openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
key_env = "OPENROUTER_API_KEY"
model = "qwen/qwen3-vl-235b-a22b-instruct"
usd_per_mtok_in = 0.21        # for the estimate before the first answers
usd_per_mtok_out = 1.90

[claude]
kind = "anthropic"
key_env = "ANTHROPIC_API_KEY"
model = "claude-haiku-5-5"
effort = "low"
```

`openrouter`, `vllm`, `ollama`, `claude` and `chat` work without a config
file; `--model`, `--base-url` and `--parallel` override any backend's.
Servers that cannot hold an answer to a JSON Schema are asked for plain
JSON instead.

**The Claude API** uses structured outputs and caches the rules. With
`--claude-batch` the requests go through the Message Batches API at half
price; `imagekit run --collect RUN --backend claude` reads the answers
back once the batch has finished (within a day).

**A chat window** costs nothing beyond a subscription:

```sh
imagekit run describe --backend chat --model "Claude app" --chat-dir ~/chat-describe
```

writes the system text, and each image with its prompt, to the folder. Paste
them into a chat, save each answer as `<photo id>.json` beside them, and run
the same command again to read them in.

## Search by look

```sh
imagekit embed --model siglip2,dinov2-small          # every image that lacks these vectors
imagekit search --model siglip2 --text "lacy, finely divided fern-like leaves"
imagekit search --model siglip2,dinov2-small --image yarrow.jpg --by-species --exclude-genus Achillea
imagekit search --model siglip2 --image leaf.jpg --box 0.2,0.1,0.7,0.6 --html near.html
```

`embed` stores one L2-normalised vector per image and model under
`sources/image-analyses/<run>/vectors/`, keyed like any other analysis, so
a rerun embeds nothing and a stopped run picks up where it left off.

| Model | Good for | Text queries | CPU, s per image |
| --- | --- | --- | --- |
| `siglip2` (so400m, 384 px) | words and photos | yes | ~2.5 |
| `siglip2-base` (224 px) | the same, faster | yes | ~0.14 |
| `dinov2-small`, `dinov2` | shape and texture: "looks like this one" | no | ~0.05 |
| `bioclip2` | trained on organisms; species look-alikes | yes | ~0.5 |

`search` is exact cosine search with numpy. Several models are merged by
reciprocal-rank fusion. `--by-species` ranks species by the mean of their
best three images, and `--html` writes a contact sheet with each photo's
credit and licence.

## Share through data.plantgen.io

```sh
imagekit push --dry-run      # what would go up
imagekit push                # every batch and finished run not yet pushed
imagekit pull                # every batch and run this store lacks
imagekit pull --run pilot-2026-10-14 --no-images
```

The server keeps the store's layout in two private sources of the
plantgen-io catalogue: `images` (one version per batch: its records and the
image files it holds) and `image-analyses` (one version per run: its rows,
raw answers, vectors and the analyzers it used). `push` hands each one to
`pgio push`, so it needs plantgen-io's uploader (installed, or a checkout
beside this one or in `~/code`; `IMAGEKIT_PGIO` overrides) and either the
owner's AWS login or a cloud session's upload key (`PLANTGEN_UPLOAD_KEY_ID`
and `PLANTGEN_UPLOAD_SECRET`, from plantgen-io `tools/cloud-upload-key.sh`).
A run with Claude batches still to collect is skipped.

A pushed batch or run is sealed: it takes no more rows, so the copy on the
server never goes stale. Later work goes in a new batch or run, and nothing
already answered is sent again, because `missing` sees the answers in every
run.

`pull` needs only the reader key (`PGIO_READER_KEY`; a cloud session's
proxy adds it). It checks every file's SHA-256, keeps nothing that does not
match, merges the analyzers and rebuilds the index. A session that pulls
first pays only for images and analyzers nobody has run yet.

## Tests

```sh
cd ~/code/plantgen/imagekit && python3 -m unittest discover -s tests
```

They fake the network, a model server and the Claude API, and need only
Pillow and PyYAML (plus jsonschema for the schema tests and the
`anthropic` SDK for the Claude tests, which are skipped without it).
