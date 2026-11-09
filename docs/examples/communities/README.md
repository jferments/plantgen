# Example plant communities

Each file is a `Community` (`plantgen::api::Community::from_json`): the
species that grow together in one kind of place, the layer each keeps to,
how common each is against the others (`abundance`, 1 when left out, a
weight on how many of it stand; or `cover`, its mean cover in the
community's plots, percent, which splits its layer's cover as the plots
do: each member stands as often as its cover over its crown's area), and,
for kept ground, `"origin": "planted"`, or for ground people
cleared or built on, `"origin": "cleared"`. A fill takes a list of them,
and each cell of a `StandMap` names one by its place in that list
(`docs/LIBRARY-API.md`, "Stands over a whole map").

## Wind River old growth

`wind-river.json` is the old-growth Douglas-fir and western hemlock forest
of the Wind River plot in the southern Washington Cascades (Shaw et al.
2004). Fill it at `--years 500 --site 0.5,1,370,5` to check a stand
against the plot.

## A Puget lowland city: Olympia, Washington

Seven communities cover a lowland Puget Sound city, from its Douglas-fir
forest and creeks to its yards and streets. They were made for a
512 m tile of downtown Olympia and checked one by one on a flat hectare
against yield tables (below). A stand map gives each cell one of them by
what the ground is, in this order:

| File | Where it goes | Origin | Age the Olympia tile gave it |
| --- | --- | --- | --- |
| `olympia-alder-by-water.json` | within 15 m of a stream, lake or shore; give the cell its wet ground (`moisture` 0.8 or more) | natural | 40 years where trees stand, 10 where none do |
| `olympia-yard-trees.json` | tree cover of half or more with a house within 30 m, or on residential land or gardens | planted | 80 years |
| `olympia-lowland-forest.json` | tree cover of half or more elsewhere: parks, ravines, greenbelts | natural | 80 years (the years since the last clearing) |
| `olympia-garden-bed.json` | within 4 m of a house's wall | planted | 10 years |
| `olympia-lot.json` | bare ground, building sites, brownfields, within 6 m of a railway | cleared | 10 years |
| `olympia-verge.json` | a third or more paved: road and path edges, medians | cleared | 1 year (mown) |
| `olympia-lawn.json` | the rest of the open ground | planted | 1 year (mown) |
| `olympia-prairie.json` | a remnant or restored prairie (Mima Mounds, Glacial Heritage, Joint Base Lewis-McChord's prairies) | natural, `fire_years` 10 | years since its last fire: 0-10 kept by fire, 30 and more left unburnt |

Cells under a roof, or under 15% soil (paving, water), get none. Wall climbers
(ivy, orange honeysuckle) stand at the feet of walls and fences in the
communities that hold them, and roofs that hold soil and cracked pavement
take their plants from the fill's own rules.

What each holds:

- **Lowland forest:** Douglas-fir over western redcedar, western hemlock,
  bigleaf maple and red alder; Pacific dogwood and cascara beneath; vine
  maple, beaked hazel, osoberry, salal, dwarf Oregon grape, snowberry and
  salmonberry, with Himalayan blackberry at edges; sword fern and bracken;
  ivy.
- **Alder by water:** red alder with bigleaf maple, western redcedar and
  Oregon ash; Pacific and Scouler's willows; salmonberry, red-osier
  dogwood, Pacific ninebark, twinberry and osoberry; lady fern, skunk
  cabbage, giant horsetail, reed canarygrass and sword fern.
- **Yard trees (planted):** open-grown Douglas-fir, bigleaf maple and
  western redcedar, Pacific dogwood, rowan and hawthorn, over lawn grasses
  and weeds; ivy.
- **Garden bed (planted):** Pacific rhododendron, tall Oregon grape, red
  flowering currant, evergreen huckleberry, rugosa rose and butterfly
  bush; ivy and orange honeysuckle.
- **Lot:** red alder, Scouler's willow and hawthorn coming in over
  Himalayan blackberry, Scotch broom and butterfly bush, in rough grass
  and weeds (velvet grass, orchard grass, Queen Anne's lace, bull thistle,
  tansy ragwort, cat's ear, field horsetail); ivy.
- **Verge:** orchard grass, velvet grass and red fescue with plantain,
  dandelion, cat's ear, Queen Anne's lace, oxeye daisy and white clover.
- **Lawn (planted):** red fescue, annual bluegrass, velvet grass and white
  clover with cat's ear, dandelion and plantain; ivy.
- **Prairie (kept by fire):** Roemer's fescue, California oatgrass and
  junegrass with camas, yarrow, Oregon sunshine, spring gold and bracken;
  snowberry, Nootka rose, serviceberry and Scotch broom; Oregon white oak
  and Douglas-fir. Fire every 10 years on average (LANDFIRE's model of
  these prairies) keeps a few old oaks and firs over the sod; a cell's
  years since disturbance are the years since its last fire
  (`development::develop_kept`). The herbs and shrubs stand by their
  cover in the 14 plots of Chappell's Idaho fescue-white-top aster
  community (Native Vegetation of the South Puget Sound Prairie
  Landscape, Washington Natural Heritage Program, table 1), each the
  table's cover times the share of plots it was in (reading the table's
  "mean relative cover" as its mean where present): Roemer's fescue
  45%, camas 5.2, bracken 3.2, Scotch broom 2.3, Oregon sunshine (woolly
  sunflower) 1.9, oatgrass 1.6, yarrow 0.6, junegrass 0.6, spring gold
  (pomo-celery lomatium) 0.4, serviceberry 0.2, snowberry and Nootka rose
  0.1.

## How they were checked, and what is still off

Each was filled alone on a flat hectare at the ages above and compared
with the Douglas-fir yield tables (McArdle, Meyer and Bruce 1961, USDA
Tech. Bull. 201; site index 130-150 ft at 100 years for Olympia's glacial
outwash) and the red alder tables (Worthington et al. 1960, PNW Research
Paper 36).

- Lowland forest at 80 years: about 460 trees and 41 m² of basal area a
  hectare, 80% of a fully stocked stand's trees and 69% of its basal area
  (456 and 41.3 against 573 and 59.5). At 35 years it holds 1,737 trees
  and 38.7 m² against the table's 1,790 and 36.4. At 40 years Douglas-fir
  stands 7-10 m below the table's dominant height, and basal area at 150
  years is about two thirds of the table's.
- Alder by water on wet ground: red alder leads, but its stems are about
  half as thick as the table's at 40 years. Its layers follow western
  Oregon's wet hardwood plots (McIntosh et al. 2009), not the conifers':
  herbs cover 43-51% from 15 to 55 years (plots 47-56%). Shrubs cover only
  31-37% (plots 46-59%): every crown lets a tenth of the light through,
  and the broadleaf crowns' own leaf area is still too thin to say how much
  more an alder's lets through.
- Prairie on dry ground (`--site 0.3,1,30,0`), a hectare: 2 and 8 years
  after a fire, old oaks cover 14-15% at 8-9 trees a hectare, under the
  savanna's 20% (LANDFIRE BpS 11200) and within the under-1 to 49 trees a
  hectare of the 1800s on Joint Base Lewis-McChord's prairies (Peter and
  Harrington 2014). Its herbs cover 78-82% of the ground from above, about
  100,000 plants a hectare, 80,000 of them Roemer's fescue (8 a square
  metre); moss 36-39% under them (Calabria et al. 2016's plots: 33% just
  after a fire, 40% long after), thatch 12%, then 23%, and shrubs 3%, then
  13%. Its trees come into the sod slowly: 16% at 20 fire-free years (a
  few saplings under 4 m among the old oaks, LANDFIRE's grassland class)
  and 31% at 30, past LANDFIRE's closed mark at 28 (30% of the ground under
  trees 5 m tall or more), while its shrubs, mostly Scotch broom, build to
  33% and 44%. At 80 years it holds 150 trees a hectare, 98 of them
  Douglas-firs, with the oaks under them; those prairies' fir stands hold
  122-207 today. A 6 m patch of the grasses alone is 37% covered seen from
  above. Against photos of the Mima Mounds the sod is now continuous and
  tawny, but its fescues lack the straw skirt of dead blades, and the
  Cladonia lichens are not drawn.
- Yard trees: crowns cover most of a yard cell, with Douglas-fir, redcedar
  and dogwood; bigleaf maple fades early because its spec's lifespan reads
  50 years.
- Lot at 10 years: a thicket of Scouler's willow and hawthorn up to 4.7 m,
  over little blackberry or broom (shrubs 3%). Its floor holds no logs, as
  cleared ground, but is 61% moss: neither the willow nor the hawthorn has
  a `shed.json` yet, so the fill does not know their fallen leaves keep
  moss down.
- Thirteen members have no niche yet, so they suit every site and stand
  half tolerant of shade (`plantc fill` names them): hawthorn, rowan,
  butterfly bush, Pacific rhododendron, rugosa rose, ivy, orange
  honeysuckle, Queen Anne's lace, field horsetail, plantain, annual
  bluegrass, dandelion and white clover.
