# PlantLab

PlantLab grows PlantGen's plants under the conditions you choose and
shows you what they look like: in a window, and without one as
thumbnails and review sheets, each picture with a JSON file that says
what it shows.

## The window

```text
cargo run --release -p plantlab -- open acer-macrophyllum
```

One plant on its ground patch, with the scale figure or rod, laid out
as Project After's WorldLab is: a top bar, a rail of panels at the left,
the open panels docked beside the 3D view (never over it) and a status
bar.

- **Top bar.** The menus (every command), the species (click to find
  another), Grow, Pause and Stop, the age and day, Side, ¾ and Top, then
  frame, grid, ruler, snapshot, record and Overlay; at the right the
  search field and Settings.
- **Rail.** Plant (1), Grow (2), View (3), Readings (4), Pictures (5),
  Spec (6), Bark (7) and Keys (F1). A click opens a panel alone,
  Shift+click opens it beside the others, a click on an open one closes
  it. Readings docks wide; the view keeps at least as much as it. The
  panels' widths come from the window's width alone.
- **Overlay** (O) hides the bars: the view fills the window, with one
  panel as a floating card and a small bar of the rail's buttons.
- **Search or run** (Ctrl+P) finds any command or species as you type;
  Enter runs the best match.
- **Settings** (Ctrl+,) are kept in `~/.config/plantlab/settings.json`
  (`$XDG_CONFIG_HOME` when set; `--settings FILE` for another). Each
  control saves only its own setting, keeps every other key in the file,
  and writes nothing when the file on disk does not load. Flags win over
  the file.

`--window WxH` sets the window's size, `--panels plant,readings` the
panels it opens with and `--overlay` starts in Overlay. The Plant panel
chooses the species (type to filter the list), its age and the day of the
year; View the level of detail, the quality (draft or standard), the
look (review or photo) and the view; Readings the plant's height, crown
width, triangles and cards as a board of cards (below); Pictures the `plantlab thumbs` command that
renders what you see without the window; Spec the species' merged spec;
Bark its bark pattern and numbers.

**Readings** is a board of cards placed from the panel's width alone:
the plant's measures; its triangles and cards by part (wood, leaves,
flowers, fruit, other); its sizes as files on a log scale; each level of
detail's triangles, cards and packaged size (the plant's growth dressed
at every level on a worker, without growing it again); its height and
crown at each frame shown as it grew; each growth step's time; and the
window's frame times over the last ten seconds, with their median and
99th percentile. It gathers only while it shows. **Freeze** holds the
page still and stops gathering; **Dump to the console** prints its
numbers.

**Growing.** A change grows the plant again ("Grow when the settings
change"; turn it off to grow only on **Grow**). **Stop** ends a growth and
keeps the last frame; **Pause** holds it. A progress bar gives the step,
the age and the time. "Show it growing, every N steps" draws the plant as
it stands every N steps (`--live N`, default 10, 0 for none), and "Move
the camera with the plant" keeps it framed as it grows. `--age YEARS`
grows the first plant to that age instead of its oldest keyframe.

**The camera.** Drag to turn around the plant, middle-drag (or Shift and
drag) to move it, scroll to come closer; "Frame the plant again" or F frames it
again.

**Measures.** A grid on the ground (G; its squares' size is in the View panel,
every fifth line bold, the bold lines' distances on the rim) and a ruler
beside the plant (R), at the ground's edge or nearer where the edge is
off screen, labelled in metres or centimetres, with the plant's height
marked across to it. `--no-grid` and `--no-ruler` start without them.

**File sizes.** "packaged" is exactly what this level of detail's mesh
takes in a PlantGen package; ".glb" and ".obj" estimate the drawn
triangles as glTF binary and Wavefront OBJ with the card textures as one
PNG. A whole package holds four levels for every keyframe and variant,
and impostors; `plantc build` gives its size.

**Pictures and animations.** **Snapshot** (P) saves a PNG; **Record
growth** (`--record`) takes a frame each time a plant or a frame of its
growth is shown, and **Save GIF** (S) writes them as a looping GIF, the
last frame held (both times are in the Pictures panel), with "and PNGs" every
frame too. Pictures are of the scene as the window shows it (ground,
scale and measures, without the interface) or of the plant alone on a clear
background (`--plant-only`): transparent PNGs and GIFs. They go to
`plantlab-pictures` (`--out DIR`). For an animation that shows the plant
grow rather than staying the same size, turn off "Move the camera with
the plant" and frame the grown plant first.

`--capture FILE` saves a picture of the window once the plant stands, or
after `--capture-after SECONDS`, and closes it.

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
| `--height PX` | Picture height; `--size` is then its width (default square). |
| `--clear` | The plant alone on a transparent background, without its ground and scale: a cut-out for a web page or a sprite sheet. |
| `--env open\|edge\|interior\|suppressed` | Where the plant grows (default its typical site). |
| `--seed N` | The variant (default the spec's first seed). |
| `--lod 0-3` | The level of detail, 0 the nearest (default 0). |
| `--scale on\|off` | The 1.8 m figure or the rod beside the plant (default on). |
| `--focus X,Y,Z[,SPAN]` | A close-up of the point X,Y,Z metres from the plant's foot (+Y up), SPAN metres tall (default 0.5). |
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
`view`, `quality` and `look` work as the options above, as do `height`
(a picture `size` wide and `height` tall), `env` (`open`, `edge`,
`interior` or `suppressed`), `seed`, `lod` (0 to 3), `scale` (`"off"`
leaves out the scale figure) and `focus` (`[x, y, z]` or
`[x, y, z, span]` in metres, a close-up of that point).
`"background": "clear"` draws the plant alone on a transparent
background, with no ground or scale figure, and `name` names the files
(`NAME.png` and `NAME.json`; letters, digits, `-`, `_` and `.`), so one
batch can hold several pictures of a species. Lines starting with `#`
are skipped. A picture whose sidecar records the same inputs (the
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
- `ui` (`plantlab-ui`): the window's look and controls (below).
- `app` (`plantlab`): draws scenes with Bevy 0.19 (the version Project
  After uses), without a window. `src/shaders/card.wgsl` cuts each organ
  card out by its template and blends its colour exactly as
  `Templates::albedo` does. `src/shaders/wood.wgsl` draws the species'
  bark pattern on its wood pixel by pixel: `plantgen::bark::sample`
  operation for operation, as `plantc`'s previews and every renderer of
  PlantGen's plants draw it, from the bark coordinates and stem radius the
  wood mesh carries. Nothing else about the plant is computed on the GPU.

Not drawn yet: part meshes. Not in the window yet: growing conditions, the timeline, wind and
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

`ui` (`plantlab-ui`) is the interface kit: the theme and its fonts (Inter
4.1, JetBrains Mono 2.304 and Phosphor's icons; `ui/fonts/README.md` gives
each file's source and SHA-256), the controls, boards of cards, and the
figures cards draw. It depends on egui and `egui-phosphor` only, so its
tests run without Bevy. It is a copy of WorldLab's redesigned interface
from Project After's `lab-ui`, owned here and never synced: PlantGen never
depends on After (no shared UI crate, by the owner's choice).
