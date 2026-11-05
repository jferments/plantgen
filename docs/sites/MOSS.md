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
| Light | The sky the face sees (rays over its hemisphere) and three equinox suns, morning, noon and afternoon, from the south (+Z). |
| Wetness | Rain from near-vertical rays the face is open to, a little driven onto upright faces, damp near the ground (falling off over 0.3 m), less what the sun dries, more in shade. |
| Hold | How well spores and water stay: 1 on a face turned up, 0.15 on an upright one, nothing where no sky or rain reaches. |
| Nook | Short rays along the face: the joints between bricks, the step under a tile's edge and inner corners catch spores, grit and water, so a landing there holds and stays wet the better. |
| Substrate | The face's material; built faces (mortar, concrete, tile) are basic, bark and wood acid. |

Each genus fits a habitat by its library traits: the `light` and
`moisture` classes it grows in, its `substrates`, its `substrate_reaction`
and its `desiccation_tolerance`. Vigour is that fit times the share of the
year the face stays wet enough to grow.

## Growth

`grow` walks the years. Each year spores land on every face in the region
(`arrivals_per_m2`, default 40). Where one lands, a genus is drawn by its
vigour there and starts a colony with a chance of half its vigour, unless
a colony already covers the spot. Colonies two years old or more also shed pieces (broken shoots, gemmae,
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
with (`MossKind::far_colour`).

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
  twisted or crisped when dry as the genus's `leaf_dry_posture` says,
  with their awns; the older leaves down the shoot in the genus's
  `older_leaf_colour` (Tortula brown below, Ceratodon tinged red).
- Mats (pleurocarps): creeping, pinnately branched shoots crossing the
  whole mat and wandering so that they interweave; where the genus's
  `dry_branch_posture` is curled (Homalothecium), the side branches curl
  up and in as the mat dries.
- Capsules on setae (straight or arcuate), in the posture the genus's
  `capsule_posture` gives, on the share of shoots `capsule_share` gives.

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

### Lower settings and a texture-only level

At a distance even a patch a colony is wasted on a roof that holds
thousands. The next step, for the site fill, is a level with no
triangles: the cover tints the host's own surface (its texture or vertex
colours) by coverage and `MossKind::far_colour`, with the patches or
shells only within a few tens of metres. A host can do that now from
the colonies (centre, normal, radius); a helper that bakes coverage into
a host face's texture is to come with the fill work. Levels between:
shells merged per face into one heightfield, and shell texturing (a few
stacked alpha layers, as for fur) where the renderer has alpha.

## The genera

Six genera, each in its own rank file under `crates/plantgen/library/`
with evidence from the *Flora of North America* vols. 27 and 28 and
*Bryophyte Biology*:

| Genus | Family | Form | Where on a building |
| --- | --- | --- | --- |
| *Grimmia* | Grimmiaceae | hoary cushions, awned leaves, arcuate setae | sunny wall tops and copings, mortar |
| *Tortula* | Pottiaceae | small cushions, long awns | walls and mortar |
| *Bryum* | Bryaceae | silvery turfs | roofs, pavement, gutters |
| *Ceratodon* | Ditrichaceae | turfs, red setae | sunny roofs and dry ground |
| *Hypnum* | Hypnaceae | glossy mats | shaded roofs, wall feet |
| *Homalothecium* | Brachytheciaceae | golden mats | walls and stone |

Every value comes from traits that any moss genus may carry; adding a
genus means adding its rank file with `body_plan` leafy and a
`colony_form`, nothing in the code.

## Running it

```bash
python3 docs/sites/make_moss_wall.py   # rewrites moss-wall.obj and .json
cargo build --release -p plantc
./target/release/plantc moss docs/sites/moss-wall.json --years 30 --out /tmp/cover.json
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --render /tmp/moss.png --detail shell --focus 0.5,1.2,0 --span 9 --size 1400
./target/release/plantc moss docs/sites/moss-wall.json --cover /tmp/cover.json \
  --render /tmp/moss-close.png --focus -1.583,0.52,0.13 --span 0.07 --view side
```

`--detail patch|shell|shoots|near` picks the level, `--from north` turns
the view half round, `--wet 0.1` draws it dry,
`--capsules no` leaves out the sporophytes. Growing the test wall and shed
takes about 25 s; a close view with shoots, a few million triangles and
seconds to tens of seconds.

## Tests

`crates/plantgen/tests/mosses.rs`: the genera and their forms, colonies
kept off a board kept clear and the slab pressed on it, the shaded side
of a wall carrying more moss than the sunny one, the same cover every
time and through JSON, patches far with shells between and shoots near,
and moss darker and greener wet than dry.

## Into the site fill

The cover sits beside the fill as a surface layer, not as plant
instances. A fill tile over a site field grows its own:
`MossSettings::over_tile(corner, tile_m, years, seed)` is the box of the
tile whose south-west corner `Site::from_field` was given, at every
height, and `grow` on the same mesh the field was measured from gives the
tile's cover, which a host keeps beside the tile's instances, draws, or
reads to tint its own textures. `plantc fill --field SITE.json --moss
YEARS` does this, writes the cover to `DIR/tiles/X_Y.moss.json` and, with
`--render`, draws it: shoots within a twentieth of the view's span of its
focus, shells out to the span, patches beyond (`--wet` sets
`DrawOptions::hydration`). On the test block (DEVELOPERS.md) thirty years
grow about 45,000 colonies in 15 s on four cores: the shaded north slope
of the house's roof, toward the street, mossed over in clumps; the sunny
south slope spotted; moss along the curb and the fence's feet.
