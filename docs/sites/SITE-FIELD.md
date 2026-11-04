# Site field file (version 1)

A site field is what one tile of a site offers plants. It is measured once
from the tile's mesh, material table and paved tiles, so that stand
placement and other hosts can read it without cutting the mesh again.
PlantGen builds it in `plantgen::site_field`. A host can build it in three
ways:

- In Rust: `plantgen::api::site_field(&SiteInput { positions, triangles,
  materials, table, pavements, z_up }, low, high, &FieldSettings::default())`.
  The input is a plain triangle mesh, a material index per triangle into a
  table of `Surface`s, and the crack tiles. No files are needed.
- From the command line: `plantc field SITE.json --out FIELD.json
  [--tile X0,Z0,X1,Z1] [--cell 1] [--margin 20] [--patch 0.75]
  [--sky-rays 24] [--latitude 45]`. `SITE.json` is the same site document
  `plantc site` reads.
- From JSON: `SiteField::from_json` reads the file back, and it refuses any
  other version.

The same inputs give the same file, byte for byte, however many cores
built it. Build it once per tile and cache it. It changes only when the
site does, or when `version` goes up. It uses every idle core: the
40 x 36 m test block takes about 0.1 s, and 480 x 432 m of it (144 blocks,
1.9 million triangles) about 6 s on 4 cores.

## Reading it for stands

`SiteField` has the lookups a stand needs, so a caller does not index the
lists itself:

- `cell_at(x, z)` and `cell_centre(cell)`.
- `height_at(x, z)`: the ground's height between cell centres (bilinear,
  leaving out cells with no ground).
- `relative_heat_load(cell)`: the heat load over level ground's at the
  field's latitude. A species' niche writes its `heat_load` range on this
  scale (1 on level ground), not McCune and Keon's own.
- `site_factors(cell, datum_m, moisture)`: what
  `Niche::site_response` reads (`niche::SiteFactors`): relative heat load,
  slope, and elevation as the height above `datum_m`. The field does not
  turn its wetness index into a niche's moisture; the host gives it.

Frame: metres, +Y up, +X east, -Z north (growth's frame). With `z_up`,
the input's +Z is up and its +Y north.

## Top level

| Field | Meaning |
| --- | --- |
| `version` | `1`. It goes up when a field is renamed, removed or changes meaning. Fields added beside the others leave it as it is. |
| `settings` | How the field was measured: `cell_m`, `margin_m`, `patch_m`, `sky_rays`, `sky_reach_m`, `wall_reach_m`, `latitude_deg`, `fence_bay_m`. |
| `tile_low`, `tile_high` | The tile asked for, as (x, z). |
| `origin` | The grid's low corner, as (x, z). This is `tile_low` less `margin_m`. |
| `columns`, `rows` | The grid's size. |
| `ground` | The ground grid. |
| `hosts`, `patches` | Everything that is not ground. |
| `cracks` | The openings of the paved tiles. |

Pass the mesh round the tile as far as `margin_m` reaches (20 m by default,
a big crown's reach). Then two neighbouring tiles measure their shared edge
the same way.

## Ground grid

Each value in `ground` is a list with one entry per cell, row by row. Cell
`i` is in column `i % columns` and row `i / columns`. Its centre is at
`origin + ((column + 0.5) * cell_m, (row + 0.5) * cell_m)`. A cell keeps
its index for as long as the tile and settings stay the same.

| Field | Meaning |
| --- | --- |
| `height` | Height of the ground a plant stands on at the cell's centre: the highest soil there, else the highest surface that faces up (a slab, bare rock). It is `null` where there is no ground: outside the mesh, or inside a closed solid such as a boulder's sunk foot or a house's footprint. |
| `slope_deg`, `aspect_deg` | Slope from level, and the bearing the slope faces (0 north, 90 east). Steps of more than two cells' height, such as a curb or a wall, are not counted as slope. |
| `heat_load` | McCune and Keon's heat load index (2002, equation 3): how much sun the slope takes in over the year, from latitude, slope and folded aspect. South-west slopes are warmest. It runs from about 0.5 to 1.1, and level ground at 45° is 0.71. South of the equator, north and south swap. |
| `material` | 1 soil, 2 rock, 3 bark, 4 wood, 5 built. A paved tile counts as 5. |
| `rooting_m` | Depth of rootable soil, down to the next surface below. It is `null` where the soil goes on deeper than roots do (an open ground sheet), and 0 on rock or a roof. In a paved cell it is the deepest soil in any of its cracks and holes. |
| `open_share` | Share of the cell open to soil: 1 on soil off a pavement, the cracks' and holes' share on a pavement, 0 elsewhere. |
| `wetness` | Topographic wetness index, ln(a / tan β), with D8 flow over the grid. It is higher where water gathers. |
| `sky` | Share of the sky the ground sees, weighted as light falls on a level surface (a sky view factor), from `sky_rays` rays out to `sky_reach_m`. In a crack, the rays start from the pavement's surface. |
| `wall_m`, `clear_m` | Distance to the nearest upright solid that is not soil (wall, fence, pole, boulder side), and to the nearest surface kept clear (window, door). Both are capped at `wall_reach_m`. |
| `under_roof` | Whether a roof, eave or other cover stands over the ground. |
| `cracks` | The cell's crack sites, as indices into `cracks`. |

## Hosts and patches

Every face of the mesh that is not ground becomes a patch on a host. The
exceptions are faces pressed against another solid (a wall's top under its
roof) and faces below the ground. A host is one of these:

| `kind` | What it is | `u` runs |
| --- | --- | --- |
| `wall` | An upright face of a building, split where the building turns more than about 25°. A window or door kept clear is a wall of its own whose patches are `clear`. | along the face |
| `roof` | A face of a building that faces up: a roof slope, a ledge, a curb's top. | along the eaves |
| `overhang` | A face that faces down: the underside of eaves or a balcony. | along it |
| `pole` | A solid at most 0.8 m across and at least three times as tall (and at least 1 m): a post, utility pole or snag. | round it, as arc length from north |
| `fence` | Wood, or thin parts, standing on the ground, no taller than 3 m and no thicker than 0.35 m, running longer than it is tall. Pickets, rails, posts and wires within 0.12 m of each other join into one fence, which is split into hosts of `fence_bay_m`. | along it, unbroken from bay to bay |
| `thin` | Thin parts that are not a fence, such as wires strung between poles. | along it |
| `boulder` | Rock standing out of the ground. | round it |
| `log` | Wood or bark lying down. | along it |

A host has `id`, `kind`, `material` (its commonest), `foot_m` (its lowest
point, where `v` is 0), `low` and `high` (its box), `u_axis` (the direction
`u` runs, or `null` round a pole or boulder) and `axis` (the upright axis's
(x, z) for a pole or boulder).

A patch is a square of about `patch_m` in (u, v) on one host. Its fields:

| Field | Meaning |
| --- | --- |
| `host`, `u`, `v` | Its host, and its corner in metres along the host and up from its foot (up the slope on a roof). A climber's cover can be laid across neighbouring patches of one host by (u, v). The two sides of a board are separate patches, and so are differently facing faces round a pole. |
| `centre`, `normal`, `area` | Its centre and normal (both weighted by area), and its area in m². |
| `material` | Its commonest material. |
| `height_m` | Height of its centre above the ground under it. |
| `sky` | Share of the sky it sees round its normal. An upright face sees at most half. |
| `root_cell` | The ground cell a climber on it roots in: half a cell out from the face. It is `null` where there is no ground. |
| `clear`, `climbable` | Whether most of it is kept clear, and whether a climber may hold to most of it. |
| `thin_m` | A thin part's thickness (a wire's, a slat's), or 0. |
| `soil_m` | Soil lying on it where it faces up, as the material table states (`soil_m`): a mossy roof, a gutter, a ledge. It is 0 elsewhere. |

## Crack sites

`cracks` holds one entry per opening patch of 0.1 m in each paved tile.
These are the same openings `plantgen::succession` runs on. Each entry has
`tile`, `x`, `z`, `y` (the soil's surface), `soil_depth_m`, `width_m`,
`sunk_m` (how far the soil lies below the pavement's surface), `wear`
(0 to 1, from the tile's traffic lanes) and `cell`.

## Material table

A site's `materials` gives each `usemtl` name a material (`soil`, `rock`,
`bark`, `wood`, `built`), or an object with:

- `as`: the material.
- `thin_m`: a thin part's thickness.
- `climb`: whether a climber may hold to it.
- `clear`: whether it is kept clear.
- `colour`.
- `soil_m`: soil gathered on it, as on a decayed roof.

`docs/sites/block.json` (made by `docs/sites/make_block.py`) is a mixed
test block with each kind of host, and `docs/sites/field_view.py` draws a
field so that `plantc site` can render it.
