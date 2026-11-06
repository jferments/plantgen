# Moss on buildings

`plantgen::mosses` grows mosses over a site's surfaces: colonies of moss
genera that settle on walls, copings, roof tiles and steps, spread year by
year, and are drawn as the cushions, turfs and mats they grow into.
`plantc moss` runs it on a site document.

This guide is for whoever changes this code. The site document and mesh
are those of `plantc site` (see [DEVELOPERS.md](DEVELOPERS.md)).

## Why a moss grows where it does

A moss has no roots. It holds to rock, mortar, tile, bark or wood by
rhizoids and takes its water through its whole surface from rain, dew and
splash. It grows only while wet, dries out and recovers again (Goffinet
and Shaw, *Bryophyte Biology*, 2009, pp. 193 and 254). So on a building,
where moss grows is set by how long a face stays wet and how much light
it gets, not by soil:

| Habitat | Read from the mesh |
| --- | --- |
| Light | The sky the face sees (rays over its hemisphere) and three equinox suns, morning, noon and afternoon, from the south (+Z), each ray dimmed by the tree crowns it passes through when the site stands under a canopy (below). |
| Wetness | Rain from near-vertical rays the face is open to, a little driven onto upright faces, damp near the ground (falling off over 0.3 m), less what the sun dries, more in shade. |
| Hold | How well spores and water stay: 1 on a face turned up, 0.15 on an upright one, nothing where no sky or rain reaches. |
| Nook | Short rays along the face: the joints between bricks, the step under a tile's edge and inner corners catch spores, grit and water, so a landing there holds and stays wet the better. |
| Substrate | The face's material; built faces (mortar, concrete, tile) are basic, bark and wood acid. |

Each genus fits a habitat by its library traits: the `light` and
`moisture` classes it grows in, its `substrates`, its `substrate_reaction`
and its `desiccation_tolerance`. Vigour is that fit times the share of the
year the face stays wet enough to grow.

### Under trees

A site document may carry a `canopy`: the crowns of trees standing over
it, each the upright cylinder of foliage the stands use
(`conditions::Envelope`: `centre_m` [east, north] = (x, −z) of the
site's origin, `base_m` and `top_m` in the site's y, `radius_m`, and
`density`, shade per metre, 0.5 times the crown's leaf area per cubic
metre; `Crown::envelope` gives it from a grown plant). `grow_under`
dims every sky and sun ray of a face's habitat by exp(−density × its
path inside each crown), as plant light does; less sun dries the face
less, so it is wetter too. Rain is left as it is: a crown catches some
and drips the rest. OSU Extension: roof mosses do best on north slopes,
"especially where trees shade them". `moss-shakes-tree.json` is the
shake roof with a broadleaf crown (leaf area index 4) over its south
slope: after 30 years that slope carries 46% moss against 10% in the
open, Dicranoweisia and Rosulabryum in place of the sun-tolerant
Niphotrichum and Ceratodon; the north slope stays at about 58%.

## Growth

`grow` walks the years. Each year spores land on every face in the region
(`arrivals_per_m2`, default 40). Where one lands, a genus is drawn by its
vigour there times the spores it sheds (`MossKind::spore_output`, the
square root of its `capsule_share` against a fiftieth, from 0.3 to 3), so
the spore rain is mostly of the mosses that fruit freely, and the genus
starts a colony with a chance of half its vigour, unless a colony already
covers the spot. A site document may name its region's genera in a
`mosses` list; spores of the others do not reach it. Colonies two years old or more also shed pieces (broken shoots, gemmae,
and the spores that fall near their capsule) that lodge a few centimetres
beyond their rim and take root more readily than a spore, since they need
no protonema. Most of a moss's realized dispersal is that near, and on walls
the pieces lodge along a seam and grow into cushions that join, so moss
spreads in clumps and fills the gaps between its colonies rather than
standing in separate discs. Then every colony's edge moves out by
its genus's `colony_spread_m_per_year`, more when vigorous, until:

- it meets a neighbour (cushions and turfs press together; a mat's
  creeping shoots run a little over its neighbour's edge);
- it runs out of surface: half its rim must lie on faces turned as its
  own, a step down a tile's or a brick's edge at most;
- it reaches its form's size (a cushion a hemisphere, a little wider than high).

A colony in a poor place may die (4% a year at no vigour). A colony takes
the way its surface mostly faces within 3 cm, so a spore lodged on the
narrow step under a tile grows out over the tile below, not up the step.
Faces pressed flat against another (a tile's underside on its deck, a
brick's back on its mortar) and faces kept clear carry nothing.

The result, a `MossCover`, is a list of colonies (kind, centre, normal,
radius, age, vigour) with the kinds and settings that made it: small
enough for a host to keep, draw with its own systems, or tint a texture
with (`MossCover::tint`).

## Drawing

`MossCover::draw` gives one mesh, at one of three levels of detail
(`Detail`):

| Level | What a colony is | Triangles a colony | Test scene (9,000 colonies) |
| --- | --- | --- | --- |
| `Patch` | a low patch: its ragged rim and height, a ring or two | about 11 | 0.1 million |
| `Shell` | its rim frayed into outlying tufts, its top in clumps of shoots (in its shape and its colours) | about 130 | 1.2 million |
| shoots | every shoot, leaf, mat stem and capsule | thousands to hundreds of thousands | |

`Detail::Near` draws shoots within twice the near sphere's radius, shells
out to `PATCH_BEYOND` (20) times it, patches farther. Capsules come only
with the shoots.

Up close every shoot is drawn over a shell that sinks and darkens to the
shoots' older lower parts:

- Cushions and turfs: crowded upright shoots, a stem every two thirds
  of a leaf length or so, their tips standing clear of the shell; leaves
  in a spiral at the golden angle, standing out more when wet, folded,
  twisted or crisped when dry as the genus's `leaf_dry_posture` says
  (a crisped blade is drawn in pieces that curl in toward the stem and
  twist, so it coils dry and straightens moist), with their awns; the older leaves down the shoot in the genus's
  `older_leaf_colour` (Tortula brown below, Ceratodon tinged red).
- Mats (pleurocarps): creeping, pinnately branched shoots crossing the
  whole mat and wandering so that they interweave; where the genus's
  `dry_branch_posture` is curled (Homalothecium), the side branches curl
  up and in as the mat dries.
- Capsules on setae, straight or arcuate (`seta_form`). An arcuate seta
  bows over 150 degrees dry, its capsule tucked among the leaves, and
  85 degrees moist, the capsule held out (Wikipedia, *Grimmia pulvinata*;
  to confirm against Glime). The capsules sit in the posture the genus's
  `capsule_posture` gives, on the share of shoots `capsule_share` gives,
  their lids a short cone, a tall cone or a beak as `operculum_form`
  says.

Every point is laid on a face of the site within reach: of the faces
straight above or below it, the outermost (a tile, not the deck under
it). A colony bridges hollows up to a quarter of its radius, at most a
12 mm mortar joint, and drapes over a step (a tile's edge) no steeper
than it runs out. Nothing floats or reaches round a corner.

### Colour

A genus's `leaf_colour` is a lit leaf's: its hue measured from photos
(the median r/g and b/g of the moss pixels in about 20 Washington
iNaturalist photos a taxon, imagekit run `moss-colour-2026-11-05`), its
brightness a green reflectance of 0.13. Photos are exposed brighter than
moss is, so only the hue comes from them. A shell is darker by
`CANOPY_SHADE` (0.62), the shade among its shoots: moss canopies reflect
0.05 to 0.11 of visible light. `DrawOptions::hydration` (0 dry to 1 just
rained on) sets the leaf angles and colour: wet moss is darker and a
deeper green, dry moss paler, duller and yellower, its hair-points and
hyaline tips greying it the more.

### Tint: the level with no triangles

At a distance even a patch a colony is wasted on a roof that holds
thousands. `MossCover::tint(hydration)` gives a `Tint`, which a host
reads to colour its own surface:

- `Tint::at(p, normal)` gives the moss over a point: its far colour (the
  shell's) and the share of the surface it hides, 0 to 1. The share
  follows the colony's ragged rim as its shell draws it, fading over the
  outer fifth, and is lower where the colony struggles. A host sampling
  it a texel at a time bakes moss into its roof's or wall's texture.
- `Tint::paint(mesh, step, low, high)` does the same to a site mesh as
  drawn: triangles near moss inside the box are split to `step` and
  every vertex blended toward the moss. `plantc moss ... --detail tint`
  renders that, with no moss triangles. The split mesh is only a
  stand-in for a texture, so it is not cheap to draw: about 0.9 million
  site triangles for the whole test scene at a 4 cm step.

Cushions smaller than the step fade, as in a texture of that texel. The
patches or shells belong within a few tens of metres. Levels between
are still to come: shells merged per face into one heightfield, and
shell texturing (a few stacked alpha layers, as for fur) where the
renderer has alpha.

## The genera

Nine genera, each in its own rank file under `crates/plantgen/library/`
with evidence from the *Flora of North America* vols. 27 and 28,
*Bryophyte Biology* and, for the roofs of western Oregon and Washington,
Oregon State's rooftop study (nine Willamette Valley roofs: *Dicranoweisia
cirrata* in 45% of samples, *Bryum capillare* 33%, *Racomitrium canescens*
var. *ericoides* 11%; cedar shakes carry more moss than asphalt or metal):

| Genus | Family | Form | Where on a building |
| --- | --- | --- | --- |
| *Grimmia* | Grimmiaceae | hoary cushions, awned leaves, arcuate setae | sunny wall tops and copings, mortar |
| *Tortula* | Pottiaceae | small cushions, long awns | walls and mortar |
| *Bryum* | Bryaceae | silvery turfs | roofs, pavement, gutters |
| *Ceratodon* | Ditrichaceae | turfs, red setae | sunny roofs and dry ground |
| *Hypnum* | Hypnaceae | glossy mats | shaded roofs, wall feet |
| *Homalothecium* | Brachytheciaceae | golden mats | walls and stone |
| *Dicranoweisia* | Rhabdoweisiaceae | crisped cushions, freely fruiting, yellow setae | PNW roofs and shakes, trunks |
| *Niphotrichum* (*Racomitrium* in older books) | Grimmiaceae | mats, hoary when dry | PNW roofs, gravel, rock |
| *Rosulabryum* (*Bryum capillare*) | Bryaceae | bright green turfs, leaves twisted dry, inclined red-brown capsules | shaded roofs, shakes, rotting wood |

Every value comes from traits that any moss genus may carry; adding a
genus means adding its rank file with `body_plan` leafy and a
`colony_form`, nothing in the code.

## Running it

```bash
python3 docs/sites/make_moss_wall.py   # rewrites moss-wall.obj, .json and moss-shakes.json
cargo build --release -p plantc
./target/release/plantc moss docs/sites/moss-wall.json --years 30 --out /tmp/cover.json
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --render /tmp/moss.png --detail shell --focus 0.5,1.2,0 --span 9 --size 1400
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --render /tmp/grimmia.png --genus Grimmia --span 0.045 --view oblique --wet 0.05
```

`--genus G` looks at that genus's widest well-grown colony
(`MossCover::showpiece`) instead of a `--focus`, and every close render
prints the genus of the colony it is centred on (`MossCover::nearest`).
Use it for any view named after a genus, and copy that printed line into
the caption: round 5 captioned a Tortula cushion as Grimmia because a
hand-picked focus sat 1.7 cm from the Tortula and 5 cm from the Grimmia.

`moss-shakes.json` is the same site with the shed roofed in cedar shakes
and the genera of western Oregon and Washington; after 30 years its roof
carries *Dicranoweisia* about half, *Rosulabryum* a third and
*Niphotrichum* a tenth of the moss, close to the rooftop study's shares.

`--detail tint|patch|shell|shoots|near` picks the level, `--from north` turns
the view half round, `--wet 0.1` draws it dry,
`--capsules no` leaves out the sporophytes. Growing the test wall and shed
takes about 30 s; a close view with shoots, a few million triangles and
seconds to tens of seconds.

### In Blender

`--glb FILE.glb` writes what a render would draw (the same `--detail`,
`--focus` or `--genus`, `--span` and `--wet`, with or without
`--render`) as one glTF file: a node `site` and a node `moss`, each one
mesh in its vertex colours (`COLOR_0`, linear) under a white, rough
material, in the site's frame (+Y up, -Z north; Blender turns it to +Z
up, +Y north). `--detail tint` writes the site alone, its surfaces
coloured by the moss over them. Arrays are plain floats, so Blender 4.5
and later open it with File > Import > glTF 2.0 and no add-on; the
vertex colours come in wired into the material. The files are large
(about 120 bytes a triangle: the test wall's shells are 690,000
triangles and 81 MB; a 4.5 cm close view with shoots 325,000 and 38 MB).

```bash
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --glb /tmp/grimmia.glb --genus Grimmia --span 0.045 --wet 0.95
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --glb /tmp/wall.glb --detail shell
```

## Tests

`crates/plantgen/tests/mosses.rs`: the genera and their forms, colonies
kept off a board kept clear and the slab pressed on it, the shaded side
of a wall carrying more moss than the sunny one, the same cover every
time and through JSON, patches far with shells between and shoots near,
the roof genera of western Oregon and their spore output, moss darker and greener wet than dry, a cover tinting the surface
under it with no triangles of its own, and a view named for a genus
finding a colony of that genus, a drawn cover writing as a glTF file
with no required extensions, and a tree's crown over a wall's sunny face
letting moss grow there.

## Changing it

- **A new genus** is a rank file under `crates/plantgen/library/<family>/<genus>/genus.json`
  with `body_plan` leafy, a `colony_form` and the moss traits the others
  carry, each with a note citing its source. No code names a genus.
  Moss genera have no species, so a change to them needs no
  `GENERATOR_REVISION` bump.
- **Leaf colours** are measured from regional photos (imagekit run
  `moss-colour-2026-11-05`, Washington iNaturalist photos), not chosen by
  eye: a genus's hue at moss-canopy brightness.
- **Check a change** on `moss-wall.json` and `moss-shakes.json`: write
  down what the floras say each view should show, then render the views
  and mark each point as matching, close or wrong. Renders that change
  the cover need a fresh `--years 30 --out` first.
- **Tests:** `cargo test -p plantgen --test mosses`. The full `species`
  and `checks` suites are heavy; run them alone, not beside renders.
- Lay points on a face's outermost side; faces pressed against another
  or kept clear carry nothing; sample big faces at random, not on a
  lattice.

## Still open

- Dry *Ceratodon* turf opens gaps between its coiled shoots; FNA calls
  its leaves only "somewhat crisped", so it may want a weaker coil than
  *Dicranoweisia*'s, which needs a trait for how strongly a leaf crisps.
- Roof cushions on the test roof mostly stay separate, up to about 3 cm
  across, though the north slope's cover (59%) already matches the
  rooftop study's "over 40%"; whether they should join into mats is for
  photos to say.
- Growth rates, spread distances, spore output by fruiting and
  colonisation order are estimates still to check against Glime's
  *Bryophyte Ecology*.

## Into the site fill

The cover sits beside the fill as a surface layer, not as plant
instances. A fill tile over a site field grows its own:
`MossSettings::over_tile(corner, tile_m, years, seed)` is the box of the
tile whose south-west corner `Site::from_field` was given, at every
height, and `grow` on the same mesh the field was measured from gives the
tile's cover, which a host keeps beside the tile's instances, draws, or
reads to tint its own textures (`MossCover::tint`, above). `plantc fill
--field SITE.json --moss YEARS` does this with the genera the site's
`mosses` list names (all the library's without one), writes the cover to
`DIR/tiles/X_Y.moss.json` and, with `--render`, draws it: shoots within a
twentieth of the view's span of its focus, shells out to the span,
patches beyond (`--wet` sets `DrawOptions::hydration`). On the test block
(DEVELOPERS.md), with the eight Pacific Northwest genera its site names,
thirty years grow about 39,000 colonies in 12 s on four cores: the shaded
north slope of the house's roof, toward the street, mossed in clumps; the
south slope spotted; moss along the curb and the fence's feet.
