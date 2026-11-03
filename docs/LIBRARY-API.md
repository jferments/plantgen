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
picks *prototypes* once for a set of plant communities (a species, seed,
setting and age class each: dominants to saplings, interior, edge, open
and suppressed) and lays out each tile as packed *instances* of them that
any engine draws with GPU instancing.

```rust
use plantgen::api::{Community, Fill, FillSettings, Site, StandMap, TileId};

fn main() -> Result<(), plantgen::api::Error> {
let forest = Community::from_json(r#"{"id": "fir-forest", "members": [
    {"species": "pseudotsuga-menziesii", "layer": "canopy", "stems_per_ha": 300}
]}"#)?;
let fill = Fill::new(&[forest], &FillSettings::default())?;

// Per tile: the ground, and which community grows in each ~40 m cell.
let site = Site::flat(TileId { x: 12, y: 40 }, 240.0)?;
let stands = StandMap::uniform(240.0, 0, 120.0); // community 0, 120 years since disturbance
let plan = fill.tile(&site, &stands, 7)?;
plan.write("tiles".as_ref())?; // tiles/12_40.json and tiles/12_40.bin
Ok(())
}
```

- **Pure and local.** A tile's plan depends only on the fill's
  communities, the site, the stand map, the seed and
  `GENERATOR_REVISION`. Its draws are keyed by each stand cell's place in
  the world, so neighbouring tiles meet without a seam and any tile can
  be filled alone, on any thread (`Fill` is `Send + Sync`).
- **Many communities per tile.** The `StandMap` gives each cell a
  community (or none: pavement, building, water) and the years since it
  was disturbed or abandoned.
- **Prototypes** (`fill.prototypes()`) are keyed by species, seed,
  setting and age class; `fill.grow_prototype(i)` grows one to export
  with its species' `Generator`. A city shares one prototype set.
- **Instances** pack into 32 bytes, little-endian: u32 prototype, f32 x3
  position (east, north, up, metres from the tile's south-west corner),
  i16 x4 snorm rotation quaternion (z up), f16 scale, u16 age in tenths
  of a year, i8 x2 tint (lightness, hue shift), u16 extra (a climber's
  reach clip in decimetres). `Instance::from_bytes` reads them back.
- **Ground cover and far canopy** come per stand cell in the plan
  (`cover`: kind and fraction; `canopy`: height, cover, colour), for
  hosts that draw ground cover and far forest with their own systems.

From the command line, `plantc fill` does the same and also grows and
exports each prototype the tile places (cached by key, so the next tile
only draws the ones it adds):

```bash
./target/release/plantc fill meadow.json --tile 2,3 --tile-m 240 --seed 5 \
    --compress meshopt --out city
# city/tiles/2_3.json, city/tiles/2_3.bin, city/prototypes/prototypes.json and GLBs
```

`docs/examples/fill-viewer.html` is an example caller with no PlantGen
code: copy it into the `--out` folder, serve that folder
(`python3 -m http.server`) and open `fill-viewer.html?tile=2_3`. It draws
every plant with one three.js `InstancedMesh` per prototype mesh, the
level by distance (`&lod=N` fixes it).

The types are settled; what a fill computes (density, layers, closure,
allometry, dead wood, climbers) is still being built, and until then
`Fill::tile` scatters each community's members at its `stems_per_ha`.

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
