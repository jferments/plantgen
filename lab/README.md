# PlantLab

PlantLab grows PlantGen's plants under the conditions you choose and
shows you what they look like: in a window, and without one as
thumbnails and review sheets, each picture with a JSON file that says
what it shows.

## The window

```text
cargo run --release -p plantlab -- open acer-macrophyllum
```

One plant on its ground patch, with the scale figure or rod. Drag to turn
around it and scroll to come closer. The panel on the left chooses the
species (type to filter the list), its age, the day of the year, the level
of detail, the quality (draft or standard), the look (review or photo)
and the view. A change grows the plant again; the old one stays until the
new one is ready, and the panel shows how long it took. Under the plant's
height, crown width, triangles and cards, the panel gives the `plantlab
thumbs` command that renders what you see without the window.
`--capture FILE` saves a picture of the window once the plant stands and
closes it.

On Linux the window needs X11 and `libxkbcommon-x11`.

PlantLab simulates nothing itself. Every plant is grown by PlantGen's own
code, the same steps `plantc render` takes, so a picture from PlantLab
shows the plant `plantc` and every host of PlantGen grow from the same
spec, seed and day.

## Render thumbnails

From this folder:

```text
cargo run --release -p plantlab -- thumbs acer-macrophyllum carnegiea-gigantea --out thumbs
cargo run --release -p plantlab -- thumbs all --out thumbs --quality draft
```

Each species gets `thumbs/ID.png` and `thumbs/ID.json`. The picture is
512 pixels square: the plant at its oldest age, on its typical site, seen
from the south-east under a fixed sun, on a neutral grey background, with
a 1.8 m figure beside it (a rod striped in 10 cm bands beside plants under
1.5 m). Options:

| Option | Does |
| --- | --- |
| `--size PX` | Picture size, 64 to 4096 pixels square (default 512). |
| `--day N` | Day of the year, 1 to 365, for organs with seasons (default 196). |
| `--age YEARS` | The plant's age (default: its oldest keyframe). |
| `--view three-quarter\|side\|top` | Where the camera stands (default three-quarter). |
| `--quality draft\|standard` | Detail of the meshes (default standard). |
| `--look review\|photo` | `review` (default) is fixed, so species compare fairly. `photo` adds soft shadows from the sun's real size, sky light from every direction, ambient occlusion, a filmic tone map and 3×3 supersampling. |

## Review sheets

```text
cargo run --release -p plantlab -- review acer-macrophyllum --out sheets
```

`sheets/ID-review.png` shows the species' growth ages side by side at one
scale, the mature plant from the side, the south-east and above and close
up, and, for a species with seasons, the mature plant at six times of the
year. A title names the species, its height, conditions and the
generator's revision; every tile has a caption. `ID-review.json` gives
each tile's rectangle, caption and what it shows, so photos can be placed
beside the right tiles.

## Batches, and choosing the GPU

```text
cargo run --release -p plantlab -- batch jobs.jsonl --out pictures --gpu 1
```

A batch file has one JSON object a line, for example
`{"species": "acer-macrophyllum", "kind": "review", "look": "photo"}`;
`kind` is `thumb` (the default) or `review`, and `size`, `day`, `age`,
`view`, `quality` and `look` work as the options above. Lines starting
with `#` are skipped. A picture whose sidecar records the same inputs (the
spec, the generator's revision and every setting) is skipped, so a
stopped batch picks up where it stopped; `--force` renders everything
again.

`plantlab gpus` lists the GPUs; `--gpu I` (on `thumbs`, `review` and
`batch`) renders on the I-th. On a machine with two of the same card, run
one batch per card, each with its own half of the jobs.

## Ray-traced pictures (a spike)

```text
cargo run --release -p plantlab --features solari -- solari acer-macrophyllum --out photos
```

Lights one plant with Bevy's experimental hardware ray tracing (Solari),
by default its path tracer over 1,000 frames (`--mode realtime` and
`--frames N` change that), and writes `photos/ID-solari.png`. It needs a
GPU with Vulkan ray queries: an RTX card, or Mesa's lavapipe, which is
very slow. Solari treats every triangle as opaque and reads no vertex
colours, so here leaf cards are cut into triangles along their outline
(`--cut-cards N`, 24 by 24 cells by default) and every colour sits in one
palette texture; a dome of sky lights the plant beside the sun. This is
an experiment, not yet a look PlantLab promises.

`--cut-cards N` also works with `thumbs` and `review`, to see the cut
outlines with the ordinary renderer.

PlantLab needs a GPU with Vulkan, Metal or DirectX 12. Without one,
Mesa's software renderer (lavapipe; on Ubuntu, `mesa-vulkan-drivers`)
works too, more slowly.

## How it is built

PlantLab is its own Cargo workspace, so the generator's workspace never
builds its graphics stack.

- `scene` (`plantlab-scene`): the engine-free layer. It grows a plant
  through `plantgen::drawing`, turns it into meshes, card templates and a
  camera framing, and writes the sidecar. No Bevy, no GPU; tested on the
  CPU.
- `app` (`plantlab`): draws scenes with Bevy 0.19 (the version Project
  After uses), without a window. `src/shaders/card.wgsl` cuts each organ
  card out by its template and blends its colour exactly as
  `Templates::albedo` does; nothing else about the plant is computed on
  the GPU.

Not drawn yet: the bark pattern on wood (wood shows its colour) and part
meshes. Not in the window yet: growing conditions, the timeline, wind and
comparing plants side by side (the next steps in the design).

Check before you commit, from this folder:

```text
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo test -p plantlab -- --ignored   # draws a fern; needs a GPU or lavapipe
```

A cold build of this workspace took 10.5 minutes on a 4-core cloud
session (2026-10-08, before the window; 388 crates in its lock then, 479
with the window, egui and the captions).

`src/theme.rs` is a copy of Project After's `lab-ui` theme, owned here and
never synced (no shared UI crate, by the owner's choice).
