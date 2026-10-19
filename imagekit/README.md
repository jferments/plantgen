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
- count the usable photos of each **view** per species against
  `photo-rules/`' targets;
- **render** a species' standing views with `plantc` into the store, and
  **compare** the renders' answers with the photos' and the text;
- **embed** each image once per model (SigLIP2, DINOv2, BioCLIP-2) and
  **search** the library by words or by a photo;
- **detect** organs (leaf, flower, fruit, bark, the whole plant) in each
  image, cut them out as **crops**, embed and search the organs alone, and
  **segment** the crown into a mask.

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

**Fetch by view.** A species needs photos of each view to a target (whole
plant, leafless, shoot, leaf face-on and underside, flower face and side,
fruit, bark, and for cacti the apex and areole close-ups; see
`photo-rules/SOURCES.md`), not a random sample, which is mostly flowers.
After a triage run, `views` counts the usable photos of each view per taxon
against those targets, with how many photographers took them and how many
more each view needs:

```sh
imagekit run triage --backend workstation
imagekit views                                     # every taxon triaged
imagekit views --taxon "Acer circinatum" --json
```

It reads the triage analyzer with most answers, or the one `--analyzer`
names, and counts a photo when triage found it usable and not a cultivar.
Cacti (family Cactaceae, or areoles seen) get the apex and areole targets in
place of the leaf ones. Then fetch again for the views still short: more
iNaturalist photos with `--months` for winter or `--phenology`, Commons and
your own for whole plants, GBIF sheets for leaves.

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
image: the task, the model and its exact revision, the rules version, the
prompt's hash and the settings. Its id is a hash of those. The rules
version hashes only the `photo-rules/` files the task's prompt is built
from, so adding or changing another task's file keeps every stored answer.

```sh
imagekit analyzer-id triage-haiku.json --register
imagekit missing --analyzer 3f1c0a9e5b7d2c44     # how many images still lack it
imagekit import-analyses pilot.jsonl --run pilot-2026-10-14
```

`missing` lists the only images a run may send. `import-analyses` takes
in answers made elsewhere, such as a pilot or a chat session, so they are
not paid for twice. A row names its image by `sha256` or by its source
`key` (such as `inat:5000`); answers hashed from a source's original bytes
match by key once that photo is fetched.

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
rules and settings sends nothing; a new model, a change to a file the
task reads or another image size is a new analyzer.

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

## Organs: detect, crop, embed

```sh
imagekit detect --taxon "Asimina triloba"                  # Grounding DINO on the usable photos and the renders
imagekit crops --taxon "Asimina triloba" --organ leaf --out ~/pawpaw-leaves
imagekit embed --model dinov2 --regions --taxon "Asimina triloba"
imagekit search --model dinov2 --organ leaf --image leaf.jpg --by-species
imagekit segment --taxon "Asimina triloba"                 # SAM 2.1 crown masks on the whole-plant boxes
imagekit measure --taxon "Asimina triloba" --plantc target/release/plantc   # crown outline measures per mask
imagekit count --taxon "Cornus nuttallii" --organ flower --what "the white petal-like bracts around this flower head"
imagekit count --summary <analyzer>                         # counts per taxon, photos against renders
```

`detect` asks Grounding DINO (`gdino-tiny` by default, `gdino-base` for
more recall) for a whole plant, leaves, flowers, flower clusters, fruit,
cones and trunks in one pass. It stores one `detect` analysis per image,
listing each region's organ, box (fractions of width and height) and
score. Photos need a triage first: only usable ones are detected, and
renders always are. About 5 s an image on a CPU at 1024 px.

Crops keep only the organs the image's triage saw (`organs_visible`, and
the whole plant in views V1 to V3), so fingers holding a flower are not
kept as leaves. `crops` writes them as JPEGs with `crops.jsonl` (source,
licence, box, score) for looking at them or handing them to a chat
observer. `embed --regions` embeds each kept organ with a small margin.
Its analyzer names the detector, organs, triage and margin, so organ
vectors never mix with whole-image vectors. `search --organ` ranks those
crops: give it a photo of just the organ, or `--box` round it.

`segment` gives SAM 2.1 each kept region's box (by default only the whole
plant, in whole-plant views and renders) and stores the object's mask as a
1-bit PNG in the run folder, with its share of the image and of the box.
About 1.3 s a region on a CPU. A mask's outline is reliable. Whether sky
inside the crown is left out varies from image to image, so measure
see-through from the pixels inside the outline, not from the mask alone.
These masks are what crown-shape measures read, for photos and renders
alike.

`measure` runs `plantc measure-mask` on each mask, the same code
(`crate::form`) that measures a grown plant's side view. It gives outline
fit, see-through, width to height, where the crown is widest, solidity
and large gaps. What covers the sky inside the outline is read from the
image (`--cover sky`: bright bluish or colourless pixels are sky). The
analyzer is plantc's commit, so a change to the measures measures again.

`count` crops each kept region of one organ with a small margin and has a
pointing model put one point on every part `--what` names (bracts, rays,
tepals, stamens, leaflets). The count is the number of points, stored
with the points and the crop's box, so any count can be checked by eye.
Molmo2 (`--pointer molmo2-8b`, the default) runs on a local GPU; any
vision backend works too (`--backend vllm --model Qwen/Qwen3-VL-8B-Instruct`,
`openrouter`, `claude`), and `--backend chat --chat-dir D` writes the crops
and one prompt for free observers, then reads their `<id>.json` answers
back. A row or column of evenly spaced points is the model listing, not
looking: it is kept as `degenerate` with no count. Each `--what` is its
own analyzer. Counting from masks failed in the daisy pilot (rays merge
into one mask), so counts come from pointing only.

## Render and compare

```sh
imagekit render asimina-triloba --age 20 --day 166 --plantc ../target/release/plantc
imagekit run triage --backend chat --model "my chat app" --taxon "Asimina triloba" --chat-dir ~/chat-triage
imagekit run observe --backend chat --model "my chat app" --taxon "Asimina triloba" --chat-dir ~/chat-observe
imagekit compare --taxon "Asimina triloba" --plantc ../target/release/plantc --out compare/asimina-triloba
```

`render` makes `photo-rules/03-match-renders.md`'s standing set of one
species at each seed (default three): side, 45 degrees down, and three
framed close-ups (a foliage shoot, a flower or fruit or a second shoot
lower down, a leaf underside; for cacti the ribs, a flower and the apex),
plus the species' flat cards from `plantc atlas`, one image per card, for
the face-on features. Close-up frames scale with the plant's height, so a
herb's shoot and flower fill them as a tree's do, and the flower close-up
is aimed at the height whose draft probe shows the most flower. Close-ups
that are still mostly sky or ground are dropped. Renders merge with a held
image only on identical bytes, never on a near look: plain atlas cards of
two species can sit within a few dHash bits of each other.
Each image goes into a batch (`render-<species>-<date>`) with source
`render`, licence CC0, and a `render` block saying how it was made: commit,
generator revision, species id, seed, age, day, env, view, focus, span and
the plantc command. Match `--age`, `--day` and `--env` to the photos first
(03-match-renders.md §1-2: `imagekit views` and the photos' dates help).

Observe the renders with the same model, rules and settings as the photos,
in one run: a render observed by one model and photos by another are not
compared. Taking `--taxon` covers both, since a render batch is filed under
the species' name.

`compare` follows `photo-rules/04-compare.md`: it drops what the rules
drop (unusable photos, cultivar looks, photos from another setting for
habit and crown features, another leaf state for leaf features, another
age class for crown profiles), counts several photos of one observation
(or one photographer on one day) as one vote, aggregates each side per
feature, gives a verdict (✓, ~, ✗, insufficient under 5 votes from 3
photographers, render insufficient under 3 render answers or without flat
cards for a face-on feature), reads what the species' sources state with
`plantc traits` and calls a disagreement that the sources side with the
drawing a conflict, checks for flowers or fruit the month's photos show
and no render does, and ranks the misses by severity. It writes
`COMPARE.md` (one table per species: sources, photos with n, renders,
verdict), the two aggregates and `findings.json` for diagnosis
(`05-diagnose.md`).

**The routine photo check.** After a family's text check, for each species
it changed: `pull`, `fetch inat` (60 photos spread over the months),
`render` at the photos' age and month, `run triage` and `run observe` on
both with one model, `compare`, then `push`. In a cloud session the chat
backend costs nothing: hand each folder's images to observers that see
only the folder (no species name), and read their answers back with the
same command. A paid model needs the owner's go and `plan` first.

## The library snapshot

```sh
imagekit compact                     # sources/image-library/<date>/: every batch and run merged into Parquet
imagekit push --library              # also upload the snapshots not yet on data.plantgen.io
```

A snapshot is a merged, read-only copy for readers and search:
`images.parquet` (one row per image, with provenance, taxon and whether it
is a render), `analyzers.parquet`, one Parquet file per task (`triage`,
`observe`, `branching`, `detect`, `segment`, ...; each answer's `out` as
JSON, and triage's main fields as columns), `regions.parquet` (each
detected organ's box), `species.parquet` (usable photos per view and
regions per organ, per taxon), each embedding analyzer's vectors in one
`.npy` with its ids, and `stats.json`. DuckDB, pandas or polars read it in
place, on disk or over HTTPS. The batches and runs stay the record, so a
snapshot holds nothing new and is made again when wanted. On the server it
is the private source `image-library`, one version per snapshot.

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
server never goes stale. Later work goes in a new batch or run (a default
name already pushed or pulled becomes `<name>-2`, `-3`, ...), and nothing
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
