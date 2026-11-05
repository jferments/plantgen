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
a colony already covers the spot. Then every colony's edge moves out by
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

`MossCover::draw` gives one mesh. Each colony is a shell laid over the
faces under it (a dome for a cushion, a level-topped turf, a low mat),
coloured by its leaves and, where they have them, the hair-points that
whiten it the more when it is dry. Near a point (`Detail::Near`) the shell
sinks and darkens to the shoots' older lower parts, and every shoot is
drawn over it:

- Cushions and turfs: upright shoot tips, leaves in a spiral at the
  golden angle, standing out more when wet, folded, twisted or crisped
  when dry as the genus's `leaf_dry_posture` says, with their awns.
- Mats (pleurocarps): creeping, pinnately branched shoots starting all
  over the mat and wandering outward so that they interweave.
- Capsules on setae (straight or arcuate), in the posture the genus's
  `capsule_posture` gives, on the share of shoots `capsule_share` gives.

`DrawOptions::hydration` (0 dry to 1 just rained on) sets the leaf
angles, colour (brighter wet, duller and darker dry) and whiteness.

Every point is laid on a face of the site within reach: of the faces
straight above or below it, the outermost (a tile, not the deck under
it). Nothing floats or reaches round a corner.

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

`--from north` turns the view half round, `--wet 0.1` draws it dry,
`--capsules no` leaves out the sporophytes. Growing the test wall and shed
takes about 20 s; a close view with shoots, a few million triangles and
seconds to tens of seconds.

## Tests

`crates/plantgen/tests/mosses.rs`: the genera and their forms, colonies
kept off a board kept clear and the slab pressed on it, the shaded side
of a wall carrying more moss than the sunny one, the same cover every
time and through JSON, and shells far with shoots near.

## Into the site fill

The cover is made to sit beside the fill as a surface layer, not as
plant instances: a fill tile can grow a cover over its host patches
(walls, roofs) and draw it, or hand its colonies to a host to tint its
textures. That step is to come, with the sites work.
