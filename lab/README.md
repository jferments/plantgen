# PlantLab

PlantLab grows PlantGen's plants under the conditions you choose and
shows you what they look like. This first version has no window yet: it
renders thumbnails on the GPU, one PNG per species, each with a JSON file
that says what the picture shows.

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

Not drawn yet: the bark pattern on wood (wood shows its colour), part
meshes, and the window, review sheets and batch runs that come next.

Check before you commit, from this folder:

```text
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo test -p plantlab -- --ignored   # draws a fern; needs a GPU or lavapipe
```

A cold build of this workspace took 10.5 minutes on a 4-core cloud
session (2026-10-08; 388 crates in its lock).
