# PlantLab

PlantLab grows PlantGen's plants under the conditions you choose and
shows you what they look like: in a window, and without one as
thumbnails and review sheets, each picture with a JSON file that says
what it shows.

## The window

```text
cargo run --release -p plantlab -- open acer-macrophyllum
```

One plant on its ground patch, with the scale figure or rod, between the
library at the left and the inspector at the right, under a top bar and
over a status bar.

- **Top bar.** The menus (every command), the library's switch, the
  chosen species' order and family (each a click away in the library),
  its common and scientific names; at the right the search, the
  inspector's switch and Settings.
- **Library** (L). Every species of the library in the tree of life:
  group (eudicots, monocots, magnoliids, gymnosperms, early flowering
  plants, ferns and allies), order, family, genus, each with how many
  species it holds; a family with one genus lists its species directly.
  Each species shows its common name over its scientific one, and a dot
  when its last render is kept. The search matches every word in any
  name or rank (`rose rosaceae`, `pinales`); the chips keep one growth
  form (trees, shrubs, herbs, grasses, ferns, succulents, palms,
  climbers). The species chosen lately are at the top. A click chooses a
  species; a double-click chooses it and generates. Hovering shows its
  last render.
- **The view.** Its toolbar has the views (Side, ¾, 45° and Top, or 1 to
  4), framing again (F), the grid (G) and ruler (R), the looks (Review and
  Photo), snapshot, recording, the GIF and the Overlay (O); a narrow view
  keeps the views and the Overlay, the rest being in the menus.
- **Inspector** (I), its pages as tabs: **Render** (below), **Readings**,
  **Pictures**, **Spec**, **Bark** and **Keys** (F1). Readings docks
  wide; the view keeps at least as much as it. The docks' widths come
  from the window's width alone.
- **Overlay** (O) hides the bars and docks: the view fills the window.
- **Search** (Ctrl+K or Ctrl+P) finds any species by any of its names or
  ranks, and any command, as you type; Enter runs the best match.
- **Settings** (Ctrl+,) are kept in `~/.config/plantlab/settings.json`
  (`$XDG_CONFIG_HOME` when set; `--settings FILE` for another), with the
  species chosen last and the ones chosen lately. Each control saves only
  its own setting, keeps every other key in the file, and writes nothing
  when the file on disk does not load. Flags win over the file.

**Choosing and generating.** Choosing a species grows nothing. The view
shows its last render (below) or, never rendered, what to do, with
**Generate** (Enter) under it; the Render page holds its settings, with
Generate at its foot:

- **Plant**: the age (a slider, and the keyframes its spec checks and
  packages hold as chips), the site it grows on (its typical one, open,
  edge, interior or suppressed) and the variant (each of the spec's
  seeds grows another plant; the first is the typical one).
- **Season**: the day of the year, on a strip of the year with each
  seasonal organ's phases drawn as bands (leaves unfolding, in full leaf,
  turning; buds, flowers, unripe and ripe fruit, as the spec's seasons
  say), a click or drag along it choosing the day; Spring, Summer,
  Autumn and Winter jump to the middle of April, July, October and
  January. **Play the year** draws the plant in the middle of each month
  in turn from the plant grown once; with "Record growth" on, Save GIF
  makes an animation of its year.
- **Detail**: Draft, Standard, or **Full** (standard meshes with every
  flower, fruit, cone and needle shoot drawn as its part mesh, as
  `plantc render --parts on` and full-quality exports draw them up
  close), and the level of detail (0 to 3; Full is level 0).

While a setting differs from the plant on screen, a pill over the view
names what changed. What Generate does then depends on what changed: a
new age, site or variant grows the plant again; a new day, detail or
level only draws the plant already grown again, in a moment. The view
and the look never need Generate: the plant on screen is seen from the
new view, or lit in the new look, at once.

**The last render** of each species, a picture and the settings it was
made with, is kept in `$XDG_CACHE_HOME/plantlab/renders` (else
`~/.cache/plantlab/renders`; `--renders DIR` for another) as `ID.png` and
`ID.json`. Choosing the species again shows that picture with its
settings in words, and takes up its settings, so Generate makes it again.
Settings > Last renders says where they are; delete the folder to forget
them. `--generate` generates the first plant as the window opens.

`--window WxH` sets the window's size, `--page readings` the inspector's
page it opens with and `--overlay` starts in Overlay. The Pictures page
gives the `plantlab thumbs` command that renders what you see without the
window; Spec the species' merged spec; Bark its bark pattern and numbers.

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

**Growing.** **Stop** (Esc) ends a growth and keeps the last frame;
**Pause** (Space) holds it. A progress card over the view gives the step,
the age and the time. "Draw the plant as it grows, every N steps" draws
the plant as it stands every N steps (`--live N`, default 10, 0 for
none), and "Move the camera with the plant" keeps it framed as it grows.
`--age YEARS` grows the first plant to that age instead of its oldest
keyframe.

**The camera.** Drag to turn around the plant, middle-drag (or Shift and
drag) to move it, scroll to come closer; "Frame the plant again" or F frames it
again.

**Measures.** A grid on the ground (G,
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
last frame held (both times are in the Pictures page), with "and PNGs" every
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
| `--view three-quarter\|side\|oblique\|top` | Where the camera stands (default three-quarter); `oblique` (or `above`) looks down at 45 degrees from the south-east. |
| `--focus X,Y,Z`, `--span M` | A close-up of M metres (default 0.5) round a point of the plant, metres from its foot with +Y up, as `plantc render --focus` frames one. |
| `--quality draft\|standard\|full` | Detail of the meshes (default standard); `full` is standard with part meshes (`--parts on`). |
| `--parts on\|off` | Draw flowers, fruit, cones and needle shoots as part meshes, at level 0. |
| `--env typical\|open\|edge\|interior\|suppressed` | The site the plant grows on (default its typical one). |
| `--seed N` | The variant's seed (default the spec's first). |
| `--height PX` | Picture height; `--size` is then its width (default square). |
| `--clear` | The plant alone on a transparent background, without its ground and scale: a cut-out for a web page or a sprite sheet. |
| `--lod 0-3` | The level of detail, 0 the nearest (default 0). |
| `--scale on\|off` | The 1.8 m figure or the rod beside the plant (default on). |
| `--look review\|photo` | `review` (default) is fixed, so species compare fairly. `photo` adds soft shadows from the sun's real size, sky light from every direction, ambient occlusion, a filmic tone map and 3×3 supersampling. |

## Review sheets

```text
cargo run --release -p plantlab -- review acer-macrophyllum --out sheets
```

`sheets/ID-review.png` shows the species' growth ages side by side at one
scale, the mature plant from the side, the south-east, 45 degrees down and
above and close up, and, for a species with seasons, the mature plant at six times of the
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
leaves out the scale figure), `focus` (`"X,Y,Z"`, `[x, y, z]` or
`[x, y, z, span]` in metres, a close-up of that point) and `span`.
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

Not in the window yet: wind and comparing plants side by side.

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
