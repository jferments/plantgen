# Using PlantGen from your own code

PlantGen is a Rust library (`crates/plantgen`) and a command-line tool
(`plantc`). This page is for developers who want plants in their own
project: one at a time, or many distinct plants of a species at once.

## The stable entry point: `plantgen::api`

Every module of the crate is public, because PlantLab, `plantc` and
Project After reach into them, and they change whenever the generator
does. `plantgen::api` is the part that holds still: its items keep their
names, arguments and meaning until `api::API_VERSION` goes up, and it goes
up only when one of them changes. Build on it, not on the other modules.

What an item returns can still change in its bytes when
`GENERATOR_REVISION` goes up, as every drawing does: plants get better.
The same species, revision, options and seed give the same bytes on every
machine.

Add PlantGen to `Cargo.toml`, pinned to a revision:

```toml
[dependencies]
plantgen = { git = "https://github.com/jferments/plantgen", rev = "<commit>" }
```

One `Generator` per species does everything. Making one reads the
species' spec and draws its organs' textures once; every plant grown from
it after shares them, so keep it for as many plants as you want.

```rust
use plantgen::api::{Generator, Options, Variants};

fn main() -> Result<(), plantgen::api::Error> {
    // Every built-in species, by id.
    for id in plantgen::api::species() {
        println!("{id}");
    }

    // Yarrow, drawn as its package draws it.
    let yarrow = Generator::new("achillea-millefolium", &Options::default())?;

    // One plant: seed 1 in its typical setting at its oldest age, as glTF
    // binary files (one per level of detail) and an index.
    let plant = yarrow.grow(1)?;
    let files = yarrow.export(&plant, &Default::default())?;
    plantgen::export::write(&files, "exports".as_ref())?;

    // Fifty distinct yarrows, written as they are drawn, on every core.
    let set = yarrow.write_variants(&Variants::count(50), "yarrows".as_ref())?;
    println!("{} plants, listed in yarrows/variants.json", set.plants.len());
    Ok(())
}
```

| Item | What it does |
| --- | --- |
| `api::species()` | The ids of the built-in species |
| `Generator::new(id, &options)` | A built-in species |
| `Generator::of(&spec, &options)` | A spec of your own (`PlantSpec::from_json`), grown on its taxa's program |
| `Generator::of_in(&spec, library, &options)` | The same, from your own folder of species (`Library::from_dir`) |
| `generator.grow(seed)` | One plant in its typical setting at its oldest age |
| `generator.grow_in(environment, seed, age)` | One plant in `open`, `edge`, `interior` or `suppressed`, at any age it grows to |
| `generator.export(&plant, &settings)` | glTF binary files, one per level of detail, and their index |
| `generator.package(out)` | The species' content-addressed package, as After loads it |
| `generator.write_variants(&variants, out)` | Many plants, each written as soon as it is drawn, and `variants.json` |

`Options` chooses the quality (`STANDARD`, as packages are built, or
`DRAFT`) and the day of the year seasoned organs show. `Settings` chooses
compression (`None`, `Quantize`, `Meshopt`), which attributes to keep and
a budget (`Budget::new(triangles, bytes)` or `Budget::tiers()`), as
`plantc export`'s flags do. Start from `Options::default()`,
`Settings::default()` and `Variants::count(n)` and set the fields you
choose.

## Levels of detail: package levels or tiers

An export has four levels. By default they are the levels the plant's
package keeps, which suit After: the nearest at full detail, the farther
ones as its organs go over to cards. For a flowering herb that can fall
from hundreds of thousands of triangles to a few hundred in one step.

Tiers (`Budget::tiers()`, `plantc export --levels tiers`, and the default
for variants) space the levels instead: max is the full drawing, and high,
mid and low come as close as the plant allows to 1/8, 1/64 and 1/512 of
its triangles, kept within 20,000 to 500,000, 2,000 to 50,000 and 200 to
5,000. Between two of the package's levels, organs go over to cards a
share at a time, so a budget anywhere in between is met. A triangle budget
given with tiers caps each tier too. Measured: yarrow 1.07M, 131k, 9.9k
and 1.4k triangles (its low tier baked cards); a 60-year Douglas-fir
616M, 483k, 49k and 4.9k (which takes minutes to search on one core,
against seconds for package levels).

The low tier is the plant's baked cards where they take fewer triangles
than mid and at most 5,000: its leaves, flowers and twigs as a few dozen
cards whose pictures are baked from the whole plant, over one texture of
at most 1,024 texels a side (`Level::cards` names the tier, and the index
lists it). A lacy herb's own cards thin to nothing drawn that small, and
baked ones keep its outline. Cards that a PlantGen package turns to the
eye stand fixed in a file, as a star of three.

A plant of fleshy bodies (a cholla, a prickly pear) keeps its solid
drawing as its low tier while that is within twice the tier's most,
since cards draw its joints as scraps. Some plants cannot reach every
tier yet: a small cactus's skin and spines are drawn in big steps, so its
mid is much coarser than its share (Graham's fishhook cactus: 136k
triangles high, 394 mid, which still keeps its outline), and a pencil
cholla's thousand joints keep its mid and low at about 16k.

## Seeds and variants

Any seed grows a plant. Seed `n` is the same plant whether 5 plants or
5,000 are made beside it, so you can make more later and keep the ones you
have. The seeds a species' spec lists (`variants.seeds`, 1 always among
them) are the plants its package keeps, and seed 1 is the typical plant.
Other seeds draw the plant's parameters across the spreads its characters
give (its height, branching and the like) by the seed alone, and grow
their own branching, leaves and flowers.

A set lists each plant's seed, size (height, crown, stem diameter, leaf
area), files, bytes and triangles per level, and a fingerprint of its
bytes; `duplicates` names any seed drawn exactly as an earlier one.

## Stands over a whole map: `Fill` (draft)

For city- or region-sized scenes, never grow a plant per place. A `Fill`
develops each community's stand once at a few stand ages (which cohorts
of which species a stand of that age holds, and how much of the ground
each layer covers: `plantgen::development`, after Franklin et al. 2002),
keeps each cohort as *prototypes* (a species, seed, setting and age
each), measures them by growing each species once per seed and setting,
and lays out each tile as packed *instances* of them that any engine
draws with GPU instancing.

```rust
use plantgen::api::{Community, Fill, FillSettings, Site, StandMap, TileId};

fn main() -> Result<(), plantgen::api::Error> {
let forest = Community::from_json(r#"{"id": "fir-forest", "members": [
    {"species": "pseudotsuga-menziesii", "layer": "canopy"},
    {"species": "gaultheria-shallon"},
    {"species": "polystichum-munitum"}
]}"#)?;
let fill = Fill::new(&[forest], &FillSettings::default())?;
// Grow each prototype once (minutes for old trees), its size kept in
// "city/prototypes" so the next run grows none.
fill.measure(&[], 8, Some("city/prototypes".as_ref()))?;

// Per tile: the ground, and which community grows in each ~40 m cell.
let site = Site::flat(TileId { x: 12, y: 40 }, 240.0)?;
let stands = StandMap::uniform(240.0, 0, 120.0); // community 0, 120 years since disturbance
let plan = fill.tile(&site, &stands, 7)?;
plan.write("tiles".as_ref())?; // tiles/12_40.json and tiles/12_40.bin
Ok(())
}
```

- **Stands that develop.** A stand cell is drawn at the stand age
  (`FillSettings::stand_ages`, 10 to 640 years unless chosen) nearest its
  years since disturbance. A young stand's trees are one cohort grown as
  in the open; once its crowns close (35 years for Douglas-fir's 500-year
  lifespan) they grow as in a closed stand and the canopy covers 85% of
  the ground, falling to 70% in old growth's gaps; from a fifth of the
  pioneers' lifespan shade-tolerant trees establish under them, fewer of
  the older. Shrubs and herbs fill their layers' cover where enough light
  reaches them, and herbs take the ground an open canopy leaves: 90% of
  a meadow or a stand just after its disturbance. How many stems a cell
  holds follows from the crowns, so a stand of narrow crowns holds more
  than one of wide ones.
- **Edges.** A stand's first row of trees beside open ground (a bare
  cell, or a stand whose canopy stands under half as tall: a clearcut or
  a young stand beside an old one) grows as an `edge`, its crown free
  toward the opening and turned to face it (`FillSettings::edges`, on by
  default). A tree is in the first row when its stem stands within its
  crown's width of the cell's open side. `fill.tile` does not know the
  cells past the tile's sides, so it grows no edge there;
  `fill.tile_among(&site, &stands, &outside, seed)` takes them from
  `outside(col, row)` (the neighbouring tiles' maps), and a tile filled
  whole or in quarters then places the same plants.

  ![A 160-year stand either side of a 40 m clearing, edges off and on](images/stands/clearing-edges.jpg)
- **Site-aware.** Each cell reads the site factors at its middle
  (`Site::site_factors_at`: moisture, heat load, elevation, slope); each
  species' share follows how well its niche suits them, and a site no
  tree suits opens toward woodland. Each plant stands only where
  `Site::plantable_at` lets it (none in a building or on sound pavement,
  a crack's share on broken pavement). `Site::from_field` reads both from
  a site field (`docs/sites/SITE-FIELD.md`).
- **Fast over a city.** Each community, stand age, site class and cell
  size is laid out once as a wrapping stand tile and stamped over every
  cell that shares it, turned and shifted by the cell's place in the
  world, from four layouts each. `TilePlan::stats` gives the stand
  numbers to check against plots: stems and trees (5 cm or more at
  breast height) per hectare, basal area, canopy cover and leaf area
  index, and edge trees per hectare. `FillSettings::herbs = false`
  leaves herbs to the ground cover raster for hosts that draw ground
  cover their own way.
- **Pure and local.** A tile's plan depends only on the fill's
  communities, the site, the stand map, the seed and
  `GENERATOR_REVISION`. Its draws are keyed by each stand cell's place in
  the world, so neighbouring tiles meet without a seam and any tile can
  be filled alone, on any thread (`Fill` is `Send + Sync`).
- **Many communities per tile.** The `StandMap` gives each cell a
  community (or none: pavement, building, water) and the years since it
  was disturbed or abandoned.
- **Prototypes** (`fill.prototypes()`) are keyed by species, seed,
  setting and age (`PrototypeKey::age_tenths`, its files named
  `<species>-<setting>-<seed>-<age>y`). `fill.measure(which, threads,
  cache)` sizes them before `fill.tile` (which says so when one is not);
  `fill.write_prototypes` exports them and measures what it writes;
  `fill.grow_prototype(i)` grows one to export with its species'
  `Generator`. A city shares one prototype set.
- **Ingrowth beneath the canopy.** A shade-tolerant tree that came up
  under its stand's crowns (hemlock, cedar and yew in an old Douglas-fir
  forest) grows beneath its community's canopy, as tall as the tallest of
  its trees commonly grows (`PrototypeKey::beneath_m`, its stem's setting
  `beneath60m`; `Generator::grow_ages_beneath`): its like around it as in a
  stand, and over it a canopy whose shade leaves 17% of the sky's light at
  the ground (`spec::UNDERSTOREY_DEPTH`), more toward the canopy. A
  western hemlock that came up 120 years ago beneath Wind River's 60 m
  canopy is 19.7 m tall, where a stand of hemlocks its age is 43.6 m. Only
  conifers grow so for now (`fill::BENEATH_PROGRAMS`): grown there, a
  broadleaf sheds every leaf, so a broadleaf's ingrowth grows as in a
  stand.
- **Parts of tall plants.** A prototype 6 m or taller is also written in
  parts (`FillSettings::parts`, on by default): its bole under the crown
  and its crown in up to four bands sized by its own height, each limb
  whole in the part where it leaves the stem, so no seam cuts through
  foliage. Each part has a file per tier (`STEM-<part>-lodN.glb`, listed
  in `prototypes.json` under `parts` with its box), and a level's parts
  together draw exactly the level. A caller draws each part at the tier
  its own distance asks for: from the ground a tall tree's top is coarse
  and its lower limbs fine, and from a roof at crown height the top is
  fine. The instance stays the same; only which file draws each part
  changes. `Generator::export_parts(&plant, &settings, &crown_regions(&plant.size))`
  does the same for any one plant.
- **Instances** pack into 32 bytes, little-endian: u32 prototype, f32 x3
  position (east, north, up, metres from the tile's south-west corner),
  i16 x4 snorm rotation quaternion (z up), f16 scale, u16 age in tenths
  of a year, i8 x2 tint (lightness, hue shift), u16 extra (a climber's
  reach clip in decimetres). `Instance::from_bytes` reads them back.
- **Ground cover and far canopy** come per stand cell in the plan
  (`cover`: kind and fraction; `canopy`: height, cover, colour), for
  hosts that draw ground cover and far forest with their own systems.
  The canopy's colour is its trees' foliage in full light, linear RGB,
  each species' weighed by its crowns' area, so a maple stand reads
  lighter than a fir one from afar.

From the command line, `plantc fill` does the same (`--ages` chooses the
stand ages, `--herbs no` leaves herbs to the ground cover, `--edges no`
grows no edges) and also
measures every prototype and exports each one the tile places (both
cached by key, so the next tile only grows and draws the ones it adds):

```bash
./target/release/plantc fill meadow.json --tile 2,3 --tile-m 240 --seed 5 \
    --compress meshopt --out city
# city/tiles/2_3.json, city/tiles/2_3.bin, city/prototypes/prototypes.json and GLBs
```

`docs/examples/communities/wind-river.json` is the old-growth
Douglas-fir and western hemlock forest of the Wind River plot (Shaw et al.
2004): fill it at `--years 500 --site 0.5,1,370,5` to check a stand
against the plot.

`docs/examples/fill-viewer.html` is an example caller with no PlantGen
code: copy it into the `--out` folder, serve that folder
(`python3 -m http.server`) and open `fill-viewer.html?tile=2_3`. It draws
every plant with one three.js `InstancedMesh` per prototype part and
tier, each part's tier by how big it looks from the eye (`&lod=N` fixes
it, `&parts=no` draws plants whole, `&view=ground` or `&view=roof`
stands the eye under or among the crowns). On a 40 m tile of 60-year
Douglas-firs, parts drew 1.48 M triangles from the ground where whole
plants drew 2.50 M, and 1.67 M where 2.33 M from crown height.

**A tile as one glTF file.** For a host that only reads glTF (Blender,
Godot, Unity's glTFast, a three.js `GLTFLoader`), `plantc fill ... --glb 3`
(or `TilePlan::glb(&prototypes, dir, level, meshopt)`) also writes
`tiles/X_Y.glb`: each prototype the tile places, at one level for the
whole tile, stored once, and a node per prototype mesh that draws every
plant using it with `EXT_mesh_gpu_instancing`. Its root node sits at the
tile's south-west corner (+X east, +Y up, -Z north), so tiles laid side by
side meet. A 40 m tile of an 80-year Douglas-fir stand with yarrow
(1,047 plants) is 0.7 MiB at the low tier and 1.1 MiB at the mid tier
with `--compress meshopt`, which three.js and glTFast read; Blender and
Godot do not read meshopt, so for them write `--compress quantize` (0.9
and 3.6 MiB). It draws
the whole tile at one level; a caller that wants detail by distance reads
the packed instances and the prototype files as above.

Dead wood is not placed yet. A climber member (layer `climber`) has one
prototype per seed grown at the foot of a 6 m brick wall
(`Generator::grow_ages_on_wall`, the stem's setting `wall`), to the first
age its spec measures it that tall. On a tile over a site field
(`Site::from_field`) it stands at the feet of walls and fences: one per
`CLIMBER_SPACING_M` (3 m) of climbable foot at abundance 1, times the
open share of the ground it roots in, its face turned onto the wall's,
and the instance's `extra` its reach clip in decimetres, the host's top
above its foot. A renderer drops what grows above the clip. On a
height-field tile there is nothing to climb and none stands.

A community member names a `species` and may give a `layer` (canopy,
subcanopy, shrub, herb, ground, climber) that keeps it to that layer
in place of its niche's, the `environments` it grows in (interior, edge,
open, suppressed; all four when left out) and an `abundance` (a share
against the others, 1 when left out). A species without a niche needs
its layer. It gives no density: how many stems a cell holds comes from
the fill, from the site and the stand's age, not from the caller.

Any plant can be grown past its spec's years to its old ages (doubling
past its last keyframe up to its lifespan; `PlantSpec::old_ages`), and
`Generator::grow_ages_in(environment, seed, &ages)` keeps several ages
from one growth, as a stand's cohorts of one species and seed are.

Each instance's `rotation` carries its yaw and its lean together.
`Instance::lean(yaw, [east, north])` builds it, with the lean in metres
per metre up, so a plant on a slope or reaching for a gap stands as the
fill grew it with no extra field to read.

## From the command line

```bash
cargo build --release
./target/release/plantc variants achillea-millefolium --count 50
./target/release/plantc variants pseudotsuga-menziesii --count 20 --age 30 \
    --compress meshopt --out firs
```

The first writes 50 yarrows into `variants/achillea-millefolium/`: for
each, `STEM-lod0.glb` to `STEM-lod3.glb` and `STEM.json`, where STEM is
`<species>-<environment>-<seed>-<age>y`, and `variants.json` listing them.
`plantc help` lists every flag: `--first-seed`, `--env`, `--age`, `--day`,
`--quality`, `--threads`, and `plantc export`'s compression, attribute,
budget and `--levels` flags (variants draw tiers unless `--levels package`).

Measured on a 4-core cloud machine (2026-11-01): 6 yarrows at 6 years in
7 s; 4 Douglas-firs at 60 years in 82 s.

The files are glTF 2.0 that need no PlantGen code. Uncompressed and
quantized files open in Blender, Godot, Unity (glTFast) and three.js;
`--compress meshopt` files need a viewer with meshopt decoding, such as
three.js. The max level is drawn at full detail, which for a flowering
herb or an old tree is a lot of triangles: use `--compress` and the
farther tiers, or a budget, for real-time scenes.
