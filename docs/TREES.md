# How PlantGen grows trees

This is how PlantGen draws broadleaf trees as of generator revision 175,
and how a change to them is checked. Conifers, palms and cacti grow on
programs of their own and are not covered here. The broadleaf program is
`crates/plantgen/programs/broadleaf.lsys`; its header holds the history of
every revision.

The approach in one line: **the measured outline sets the crown's form,
growth roughens it inside that outline, and every render is judged
against photographs of the species.**

## 1. Growth, not modelling

A broadleaf tree is grown one year at a time, after Pałubicki et al.
(2009):

- **Buds grow toward free space** inside a crown region (space
  colonization, Runions et al. 2007). Points of free space are scattered
  through the region; each bud turns toward the points near it and uses
  them up as it grows, so branches spread apart and stop where the space
  runs out (`space@4` in `crates/plantgen/src/lsys/tools.rs`).
- **Light becomes vigour.** Each tip's light, from a voxel shade model,
  flows down to the base and back out to the tips (Borchert-Honda), and a
  bud grows as many internodes in a year as its vigour allows. A tip
  gathers light the way a leaf photosynthesises, half of full light's
  worth at `light_half` 0.3, so shaded shoots keep growing slowly.
- **Apical control relaxes with age**: strong in a sapling, which raises a
  trunk, then weaker, so limbs compete with the leader and the crown
  spreads.
- **A year's shoot is a growth unit** with a bud and leaf at every node.
  Only some of its buds grow long shoots (at the tip, as in oaks, or at
  the base, as in shrubs); the next ones grow short shoots that leaf out
  each year while they have light; the rest stay dormant and may sprout
  later where light reaches them.
- **Shade kills.** Buds below `shade_death` die, a branch with no living
  bud is shed, and the lower stem clears itself.
- **Wood carries load.** Branches thicken by the pipe model, bend under
  their own weight and their leaves', and hold a set angle from upright by
  reaction wood.

The parameters come from rules (§3), not from values set by hand for a
species. A few species still carry older hand-set values (Oregon white
oak's `apical` and `path_half`, for one); `plantc spec SPECIES` shows them
as set in the species' own `spec.json`, and they are being replaced by
rules.

## 2. The outline sets form

The free-space region is where the crown can grow. Its shape is measured,
not invented:

- **Size and growth rate.** Crown width against height (`crown_spread`),
  crown base as a share of height (`crown_base_share`) and leaf area come
  from the USDA Urban Tree Database (McPherson et al. 2016) for
  open-grown trees. How fast the crown approaches its full size
  (`crown_rate`) comes from `height_growth_rate`, fitted to the
  database's median heights by age (k in H(1 − e^(−k(age+3)))). Height
  levels off by itself, because shoots shorten as water has farther to
  climb (`path_half`).
- **Profile.** `crown_form` (a flora's "conical", "rounded",
  "columnar"...) sets where the region is widest and how full it is
  toward a cylinder, so a southern magnolia grows a cone that is leafy to
  the ground and an oak grows a dome.
- **Sub-crowns.** Where the leader is lost and the trunk divides into
  scaffold limbs (`leader`, `trunk_fork_stems`), the region is split into
  that many smaller regions, one for each limb, so the crown is a
  cluster of domes, as in sequential reiteration (Barthélémy and Caraglio
  2007).
- **Masses (revision 175).** The free space is a cluster of rounded
  masses with gaps between them (`mass_size` 0.45 of the crown's radius,
  `mass_share` 0.5), and half of the lateral buds that would grow a long
  shoot die (`bud_loss`; Thomas 2014, Dmyterko and Bruchwald 2010). Twigs
  gather into masses with sky between them, as in a photographed crown,
  where an evenly filled region grew a smooth egg. Inside a mass, twigs
  sit closer together by as much as the mass packs its space, so the
  crown keeps its leaf area.
- **Neighbours.** Among other crowns or at a stand's edge, the region
  narrows as far as the shade allows (Duchemin et al. 2018), so trees in a
  stand grow tall and narrow, and toward open sky at an edge.

What does not work: lumps imposed on the outline ("billows") gave
clipped-hedge shapes, and hand-set crown bases and growth rates pushed
each species away from its measured shape. Roughness has to come from
growth inside the outline, not from the outline itself.

## 3. Architecture from a flora's words

A flora's habit description is turned into traits with states (stated
once, in the highest taxon they hold for, with the source cited). Each
trait has one rule in a rank file that sets the program's parameters:

| Words in a flora | Trait | What the program does |
| --- | --- | --- |
| "branches ascending / spreading" | `limb_angle` | the angle limbs hold from upright, and how they arch outward as they lengthen |
| "branchlets pendulous" | `branch_posture` | branches droop, and how hard they hold their angle |
| "crooked, tortuous limbs" | `limb_course` | each year's shoot sets off at a random angle the limb only partly corrects |
| "excurrent / decurrent", "trunk dividing low" | `leader`, `trunk_fork_stems` | a lasting leader, or a fork into scaffold limbs and sub-crowns |
| "branchlets without a terminal bud" | `terminal_bud` | the tip dies each year and the top lateral bud carries the shoot on, so twigs zigzag |
| "flat sprays" | `branch_orientation` | shoots on limbs grow level with leaves to either side |
| self-pruning, epicormic sprouts | `self_pruning`, `epicormic` | shedding of starved limbs, dormant buds that sprout in light |
| shade tolerance | `shade_tolerance` | how far down the crown keeps leaves |

`plantc spec SPECIES` shows which file set each value, and `plantc rules`
lists every rule by its home. The trees' shared defaults live in the
flowering plants' `decurrent_tree` form
(`library/_ranks/clade/angiosperms.json`), and each lineage's program
extends `broadleaf`.

## 4. Density from measured leaf area

The Urban Tree Database measures leaf area from crown photographs
(silhouette share times crown area), so a crown that holds the database's
leaf area per crown area should look about as solid as the photographed
trees. The leaves face outward from the crown, and the look field `fill`,
from `crown_see_through` measured on photos, adds card copies at draw time
until the crown shows as much sky as its photos do. That changes how the
crown is drawn, not how it grows.

## 5. How a change is checked

In this order:

1. **Photos first.** Fetch open-licence photos of each changed species
   with `imagekit` (whole open-grown trees, more than one photographer,
   iNaturalist research grade). Render the tree at the age the photos
   show, from eye height (`plantc render SPECIES --view eye`), from about
   45° above (`--view three-quarter`) and close up (`--focus leaf:0.75
   --span 10`), before and after the change, and put them side by side.
   Compare by eye: the silhouette, how much sky shows through, whether the
   limbs show, how the leaves cluster, how many trunks there are and how
   thick. A tree that matches its numbers but does not look like the
   species has failed.
2. **Words second.** Reread the flora's habit description and check each
   phrase against the render.
3. **Numbers third.** `plantc grow SPECIES --years N` prints sizes against
   the reference lines; `plantc measure` gives crown width and height;
   `plantc validate SPECIES --seeds 3` checks every stated character on
   grown plants, as `tests/checks.rs` does. Grow every species of the
   family, not just one, and grow them to the oldest age the sources
   describe. Problems at 150 years often do not show at 60.
4. **Rules, never per-species values.** A gap is fixed by a trait and a
   rule at genus, family or library level, or by a new mechanism in the
   program. A hand-set number in one species' spec is a last resort, and
   its evidence note says why no rule fits.

Traps: a new random draw in the middle of the program shifts every later
draw (use place hashes); save raw `plantc grow` output to a file before
parsing it.

## 6. What is still wrong

Revision 175 brings the bigleaf maple, Oregon ash, tulip tree and paper
birch closer to their photos. These are still off:

- **Oregon white oak** grows as a narrow upright ball of tufts. Photos
  show a crown wider than it is tall, on heavy crooked horizontal limbs.
- **Southern magnolia** at 50 years is a round ball. Photos show a broad
  pyramid.
- **Red alder and paper birch** grow as smooth upright eggs. Photos show
  open, leaning, often several-stemmed trees.
- **Quaking aspen** draws a lollipop on a thick trunk. Its genus values
  come from cottonwood street trees.
- **Pacific madrone** holds its leaves in small tufts where photos show
  broad sheets, and its smooth orange-red bark draws as pale speckle.
- **Trunks and bark** need the most work: trunk girth and taper, root
  flare, how limbs join the trunk, and bark colour and texture against
  photos.
- **Seedlings**: a young envelope narrower than the spacing of its
  free-space points can leave a seedling waiting years for a shoot.

## Sources

- Pałubicki W. et al. 2009. Self-organizing tree models for image
  synthesis. ACM Transactions on Graphics 28(3).
- Runions A., Lane B., Prusinkiewicz P. 2007. Modeling trees with a space
  colonization algorithm. Eurographics Workshop on Natural Phenomena.
- McPherson E.G., van Doorn N.S., Peper P.J. 2016. Urban tree database
  and allometric equations. USDA Forest Service PSW-GTR-253.
- Barthélémy D., Caraglio Y. 2007. Plant architecture: a dynamic,
  multilevel and comprehensive approach to plant form, structure and
  ontogeny. Annals of Botany 99: 375-407.
- Duchemin L. et al. 2018. Tree crowns grow into self-similar shapes
  controlled by gravity and light sensing. Journal of the Royal Society
  Interface 15.
- Thomas P. 2014. Trees: Their Natural History, 2nd edition. Cambridge.
- Dmyterko E., Bruchwald A. 2010. Model rozwoju pędu wierzchołkowego i
  jego ugałęzienia u dojrzałej brzozy omszonej (Betula pubescens Ehrh.).
  Leśne Prace Badawcze 71(1): 21-28.

Each species value cites its own sources in its spec's evidence notes
(`plantc sources cite ID`).
