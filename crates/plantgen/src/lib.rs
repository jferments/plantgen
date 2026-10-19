//! PlantGen: procedural plants, grown from species data.
//!
//! A species is data (a [`spec::PlantSpec`]), with a note on how each of
//! its values is known ([`evidence`]); its form comes from a plant
//! program written in an open L-system language ([`lsys`]). The compiler
//! grows each variant once, from seed to old age, inside a synthetic
//! neighbourhood, and keeps a [`graph::PlantGraph`] at each keyframe age.
//! Meshes, level-of-detail chains and impostors are baked from those graphs
//! ([`mesh`], [`raster`], [`impostor`]), with organ cards cut out by
//! templates drawn from the species' looks ([`looks`], [`templates`]), into
//! a content-addressed `.afterplant` package ([`package`]). Fleshy bodies,
//! the stems of cacti and other succulents, are meshed by [`body`], and
//! their spines grow per areole from a pattern per species ([`spines`]).
//! The textures of the ground between plants (litter, moss, grass, soil,
//! sand, gravel and rock) are drawn here too ([`ground`]). Beside its spec,
//! a species' record says where it grows ([`niche`]), the site it typically
//! grows on ([`conditions`]) and what falls from it ([`shed`]). The
//! built-in species are compiled in from the library tree ([`library`]);
//! Project After uses all of this through its `after-plants` crate.
//!
//! Everything here is engine-independent and deterministic: the same spec,
//! generator revision and seed give bit-identical output on every machine,
//! because every random draw is a pure function of lineage identities
//! ([`rng`]) and every transcendental function goes through `libm`
//! ([`math`]).

pub mod bark;
pub mod bark_look;
pub mod bend;
pub mod blooms;
pub mod body;
pub mod budget;
pub mod capitula;
pub mod checks;
pub mod clusters;
pub mod conditions;
pub mod cores;
pub mod divided;
pub mod drawing;
pub mod evidence;
pub mod export;
pub mod flowers;
pub mod form;
pub mod formula;
pub mod fruit;
pub mod gltf;
pub mod graph;
pub mod ground;
pub mod grow;
pub mod hairs;
pub mod impostor;
pub mod inherit;
pub mod json;
pub mod lace;
pub mod leaves;
pub mod library;
pub mod litter;
pub mod looks;
pub mod lsys;
pub mod margins;
pub mod math;
pub mod mechanics;
pub mod mesh;
pub mod meshopt;
pub mod motion;
pub mod niche;
pub mod occlusion;
pub mod organs;
pub mod package;
pub mod parts;
pub mod preview;
pub mod prickles;
pub mod quality;
pub mod raster;
pub mod rng;
pub mod roots;
pub mod shed;
pub mod shoots;
pub mod spec;
pub mod spikelets;
pub mod spines;
pub mod stand;
pub mod substrate;
pub mod templates;
pub mod texture;
pub mod traits;
pub mod venation;

/// Revision of the generator as a whole: the L-system engine, its tools and
/// the bakers. Bump it whenever output for an unchanged spec changes; it is
/// part of every package key. 3: cluster cards of the far levels gained
/// upright cards, so far crowns show from the side. 4: cluster cards cover
/// what their organs cover, no more, and keep the organs' proportions, so
/// far levels are not denser than near ones. 5: leaf, blade and frond
/// cards bend where they are drawn one per organ (plant forms F2), which
/// changes the impostors and the atlas records. 6: flowers, fruit and cones
/// drawn as solids on the nearest level (F3). 7: every broadleaf package
/// gains the thorn organ type (F5). 8: organ types with part meshes
/// (`crate::parts`, plant roadmap P4) keep their cards on level 0, listed
/// as sites, instead of being drawn solid into its wood mesh, and the
/// package stores their part meshes. 9: needle sprays are solid shoots on
/// the nearest level (`crate::shoots`), and each part mesh stores its span
/// (`APPARTS2`). 10: bark vertices carry the stem's radius in their
/// colour's alpha, for the species' bark pattern (`crate::bark`). 11: leaf
/// templates draw their veins grown by space colonization
/// (`crate::venation`). 12: every level keeps the nearest one's widths
/// (plant leftovers L10 and L12): cluster cards count overlapping organs
/// once, follow their bent organs, never reach past them and share their
/// area in rows; rings widen to the stem's mean width; the thin wood a
/// level drops comes back as sticks; and impostor alpha keeps the plant's
/// coverage at the cut-out. 13: organs take their colour, and leaves their
/// sun or shade form, along the logarithm of their light between
/// `looks::DEEP_SHADE` and `looks::FULL_SUN`, so a dense crown's surface is
/// drawn sunlit; and bark is darkened by the sky it sees through the
/// plant's leaves (`crate::occlusion`). 14: a rosette's blades become a
/// star of its own blades at coarse levels, fewer and wider, with thick
/// leaves' edges (render review S1), instead of a cross of cluster cards.
/// 15: a cluster's cover is counted from 16 directions on a 24-cell grid,
/// not 32 on 48, so coarse levels bake several times faster. 16: a
/// cluster's part mesh may take `parts::CLUSTER_TRIANGLES`, as a needle
/// shoot's does, drawing each flower or head as one flat hexagon a step
/// coarser, and a cluster whose axis bears no branches is never merged
/// into one solid to fit (spec rework L1c). 17: a flower head is laid out
/// from its florets (`crate::capitula`, spec rework P2): its phyllaries in
/// rows, its receptacle, its disc florets opening from the rim inward,
/// rays or straps, and its fruit with their pappus; its solid, the head a
/// cluster draws and its card all come from that layout, and a head's
/// part mesh may take `parts::CLUSTER_TRIANGLES`. 18: a grass's or a
/// sedge's spikelet is laid out from its glumes and florets
/// (`crate::spikelets`, spec rework P3), and a cluster's units may be
/// spikelets; a tapered branch bears fewer units, each as long as on a
/// branch of full length, instead of as many smaller ones; and a
/// cluster's card holds units that reach past its organ's tip, which it
/// cut off. 19: every organ type has a term, what it is, and a program
/// may declare structures that what it draws is within (`crate::organs`,
/// spec rework M7); the grass, herb, fern and palm programs draw their
/// leaves and fronds in them, and a graph with structures is `APGRAPH2`.
/// 20: an organ a rule rewrites into another lives on in it, with its
/// id and birth (organ phases, spec rework M7), so a grass's thatch is
/// the blade it was and falls only once; and a head with a season opens
/// from its rim by the day, its bud the head itself, closed.
/// 21: an organ type's term no longer hangs on the day a package shows
/// (spec rework M7B): a cluster a stage draws whole, as an alder's
/// catkins in flower, is an inflorescence on every day.
/// 22: a leaf's teeth and lobes are laid down as its blade grows, each
/// fed by a vein and leaning its way, the middle ones largest (spec
/// rework L3); and a leaf grows in five steps of form from deep shade to
/// full sun, its size following its light smoothly.
/// 23: a flower that is more than a whorl of alike petals is laid out
/// whorl by whorl (`crate::flowers`, spec rework P5): sepals, roles,
/// corolla and floral tubes, spurs, stamens and pistil, on its solid, its
/// cluster's units, its card and its named parts; stamens and a flower's
/// tube moved from its form to its card, and a lone flower's bud is the
/// flower closed.
/// 24: every flowering family states its flowers, so every species' flower
/// is laid out whorl by whorl (spec rework P5b); an inferior ovary of
/// many carpels is one pistil with as many stigma lobes.
/// 25: a package ships part meshes' coarser steps beside them (spec
/// rework SEMLOD): the organ drawn one or more steps coarser, each with at
/// most half the triangles of the one before, the size of its triangles,
/// and kept only where it covers and colours like the part mesh from every
/// direction above, so a renderer draws parts farther out at fewer
/// triangles. A head one step coarser draws each ray as its outline in
/// eight corners with its underside shaded, not a kite, which changes the
/// part meshes of heads too large for their finest step.
/// 26: a part-mesh step may keep up to two thirds of the triangles of the
/// one before, and its cover is weighed against each direction's own; a
/// needle shoot drawn coarser widens its needles for those it leaves out,
/// and a spikelet one step coarser folds each bract's kite along its
/// midrib, so conifer sprays and grass panicles keep their cover and get
/// steps; a fruit drawn with fewer facets than its finest is widened to
/// cover what it does there, so clusters of fruit and berries keep their
/// cover drawn coarser.
/// 27: a cluster of spikelets drawn as coarse as its branches merge draws
/// half its spikelets, each longer, then each smallest branch as the
/// outline of its spikelets, two crossed kites, not solids round them, so
/// dense grass panicles get coarser steps; each step records the cover it
/// loses or gains (`part-steps` is APSTEPS2).
/// 28: a shoot is juvenile or adult (spec rework P6): a program's
/// `adult(a)` sets the adultness of the organs drawn after it in the
/// branch, a leaf type's juvenile family follows it, and a graph whose
/// organs record it is `APGRAPH3`. The climber program draws it (an ivy's
/// adult shoots bear entire leaves, rise and fruit), and the programs that
/// measure first flowering by the plant's age name it `adult_age`, set
/// from a taxon's juvenile period.
/// 29: a fern's frond is sized from its stated blade, stipe and blade
/// width by rules of its class (spec rework E6a): the frond's length, its
/// bare share and its longest pinna, so the sword, wood and deer ferns
/// and bracken grow fronds of their flora's sizes; a crown a fern's
/// rhizome sends out starts as old as the crown it came from
/// (`polypodiidae` revision 4), so its first fronds are full-sized.
/// 30: crown calibration: in the broadleaf program a short shoot lives
/// while it has light, for up to 40 years, and its leaves carry their own
/// turn, so open-grown crowns keep the leaf area per crown area of the
/// Urban Tree Database's trees and their inner leaves grow in shade; and
/// the deep-shade end of the light scale leaves acclimate on,
/// `looks::DEEP_SHADE`, is 0.08, where the growth model's short shoots stop
/// leafing out, not 0.04, which no crown's leaves reach.
/// 31: crown form, step A: in the broadleaf program a year's shoot is a
/// growth unit of several nodes, only those at its acrotonic or basitonic
/// end growing long shoots, a few next to them short shoots and the rest
/// latent; and every broadleaf tree may fork where its flowers end a
/// shoot, lose tips that have no space, shed limbs that do not pay their
/// way, sprout from dormant buds and grow back into freed space, as a
/// maple did, each set by rules from the taxon's architecture traits.
/// 32: a cluster can wrap a spathe round its units (spec rework E6b): a
/// leaf-like sheet from below the lowest unit, its length, width, hood,
/// cup and colour set on the look, drawn solid near the camera and on the
/// cluster's card, so the skunk cabbage's spadix stands in its yellow
/// spathe.
/// 33: a cactus's ribs, areoles, spines and joints are grown from the
/// characters its floras state (spec rework E6c), by rules of its family,
/// subfamily and genus: rib and spine counts within their stated ranges,
/// areoles' spacing, spines a fifth shorter than the longest a flora
/// gives, a column's girth and a joint's length and width; spine sets
/// name their role, and the chollas' areoles bear glochids.
/// 34: a value a rule sets from one stated range varies within it (spec
/// rework VARY): the effective spec records its spread, and each organ
/// drawn solid is one of four varieties of its look, which draw its
/// counts and sizes across their ranges; a part mesh's variant `k` is its
/// look's variety `k`.
/// 35: a conifer's foliage and cones are grown from the characters its
/// floras state (spec rework E6d), by rules at Pinidae: how long a shoot
/// keeps its needles, a spray card's needles as long as the stated ones
/// on a usual spray, a pine's needles to a bundle, needles in two ranks
/// or all round, and a seed cone's length, posture and years on the tree.
/// 36: the pieces of a strap join (an arching grass or sedge blade, a
/// basal leaf drawn in pieces): each piece of a chain is turned against
/// its card's droop by half of it and lengthened by its arc over its
/// chord, so its bent card ends where the next piece stands, and drawn at
/// its own size, not its sun or shade form's; before, every piece drooped
/// away from the next and the strap fell apart into floating pieces.
/// 37: a cactus column's ring of flowers sits at the radius of its newest
/// piece as drawn, not its tip's, so a young pup's flowers stand on its
/// body (36 is the strap pieces' join, #65).
/// 38: a package's plants share out the range of each character of the
/// whole plant that a parameter takes in its own units (spec rework
/// VARY): one plant is the typical one, three take the middles of the
/// range's thirds; a part's size, or a parameter a formula works out from
/// a range, records no spread and keeps its value.
/// 39: western redcedar keeps its foliage as long as its scale leaves
/// were measured to live (about nine years), not the three years its
/// branchlets were once said to last.
/// 40: a broadleaf crown grows its own outline inside a loose region
/// (crown form step B, broadleaf revision 11): free space fills 1.6 times
/// the crown's radius and 1.3 times its height; a shoot's nodes shorten
/// with the wood between it and the base (`path_half`, fitted per species
/// to its height); a tip's light gathers with saturation, so shaded shoots
/// keep growing; the stem steers upright, limbs toward their held angle,
/// and the wood bends under its load, the stem and a fork's stems held up;
/// a tip with no free space leafs out. Maples keep their lobed envelope
/// (sapindaceae revision 6). (39 is redcedar's leaf life, #70.)
/// 41: nothing drawn reaches below the ground the plant stands on (the
/// level plane of its foot): a card that would, a leaf drooping into the
/// soil or a spray at a seedling's foot, turns up about its base just
/// enough to rest on the ground, or rises onto it when no turn clears it;
/// and a stem's foot ring meets the ground in a flat cut where a leaning
/// or flared trunk's side would dip into it.
/// 42: a conifer's lower crown stays full (pinidae 5): a branch's shoot
/// shortens with its wood from the stem, not its age; foliage lasts longer
/// low in the crown; dormant buds sprout on lit branch wood; side shoots
/// grow 20 years; sprays are drawn with their own turns; a limb that
/// reaches the ground is shed there, and a dead branch falls (41 is the
/// ground plane, #73).
/// 43: a fern's, a palm's, a tree fern's or a cycad's fronds and a cactus's
/// joints that sag to the ground lie on it (`rest`, on the conditions'
/// substrate or level soil) instead of passing into it, so the deer and
/// sword ferns, Engelmann's prickly pear and saw palmetto stay above the
/// ground; bend@1 can hold bent wood up on the ground too (`ground`),
/// which no program uses yet (41 is the ground plane, #73; 42 the
/// conifers' lower crowns, #72).
/// 44: leaves, fronds, blooms, fruit, dead fronds and boots, rosette
/// leaves and stalks, canes and epiphyte colonies live as long as their
/// parameters say, not one year more: a deciduous tree draws this year's
/// leaves only. Velvet mesquite (`bud_break`), bigleaf maple
/// (`sprout_light`) and Oregon white oak (`spur_light`) are refitted to
/// keep their sizes, their crowns within the module limit and the oak's
/// leaf area per crown area (43 is the fronds that lie on the ground,
/// #77).
/// 45: a flower whose zygomorphy is of no named kind (palo verde,
/// piggyback plant) draws its upper petal unlike the rest, wider by
/// `banner` and more upright (an unequal corolla), not a corolla of like
/// petals (44 is the leaf lives, #79).
/// 46: nine broadleaf shrubs and trees are drawn within their floras'
/// sizes: desert willow, burrobush, beaked hazelnut, cascara and creosote
/// bush no taller than their greatest height (`path_half` refitted), and
/// desert willow, burrobush, vine maple, osoberry, thimbleberry and jojoba
/// with leaves no larger than their floras give, desert willow's narrowly
/// linear. (45 is the unequal corolla, #86.)
/// 47: a dioecious species' plants are male or female, each by its own
/// draw from its seed, and only females bear fruit or seed cones: a male
/// yew bears no arils, a broadleaf's male blooms fall where they would
/// fruit, a herb's keep flowering and a climber's set no fruit. Every
/// built-in package's own seed grows the plant it grew before; other
/// seeds of a dioecious species may now grow males. (46 is the
/// broadleaf sizes, #88.)
/// 48: a regular corolla may stand in two like whorls (`petal_whorls`
/// 2), the inner turned half a petal: tall and dull Oregon grape draw
/// three petals and three, bleeding heart an outer and an inner pair, and
/// their stamens stand in whorls of as many (47 is dioecy, #103).
/// 49: the daisy, grass, carrot and pea families' 34 built-in species grow
/// from their characters (spec rework E1 to E4): head, leaf, blade, culm,
/// panicle and cane sizes from their floras by rules in their taxa; a
/// grass holds its blades in two ranks (grass program 6, `roll_spread`
/// per lineage, sedges and rushes as before); cow parsnip's leaves are
/// three leaflets; broom's flowers stand alone in the leaf axils (48 is
/// corollas in two whorls, #90).
/// 50: a conifer's limbs sag under their load, not by their age (pinidae
/// 7): bend@1 with each species' green wood, and reaction wood (bend@1's
/// `reaction`) turning a limb back toward its line by each year's ring,
/// so old crowns keep spreading limbs instead of hanging ones (49 is
/// #97).
/// 51: a conifer's foliage and cones live as long as their parameters
/// say, not one year more (pinidae 8, as 44 did for the other programs),
/// and a pine's lowest needles as long as its species' longest stated
/// needle life (Pinus's `needle_low`) (50 is conifer sag, #95).
/// 52: a broadleaf tree's or shrub's crown narrows among neighbours
/// (broadleaf 15, sapindaceae 8, `space@4`): its free-space region is
/// scaled at each height and side as far as a crown front grown from the
/// lineage's tropisms under the neighbours' shade falls short of the
/// open-grown one, so stand-grown crowns are narrower and an edge tree's
/// reaches toward open sky; open-grown plants are unchanged. At a stand's
/// edge only a point reaching toward the stand's side reaches into the
/// neighbours' crowns (51 is
/// #96).
/// 53: 62 built-in species take their heights, culm lengths and juvenile
/// periods from floras in place of hand-set `crown_height`, `height_max`,
/// `stem_height`, `culm_height` and `adult_age`, so the rank rules size
/// them, and a stated range spreads their heights across seeds.
/// 54: a conifer's side shoots live as long as their light pays for them
/// (pinidae 9), not 20 years, so old limbs keep foliage along their
/// length and open-grown spruces, firs, hemlocks and cedars fill in
/// (53 is heights from floras,
/// #113).
/// 55: palms grow from their facts (palm 7, arecaceae, Arecaceae's palm
/// form): a crownshaft of leaf sheaths on the trunk's girth, trunks that
/// flare at the foot and may swell mid-way, arched fans with hanging
/// segment tips, old fronds turning down and skirts of dead fronds; the
/// royal, cabbage, date, California fan and saw palms are refitted, and
/// a drafted palm's frond, petiole and trunk come from its leaf length,
/// heights and stem diameter (54 is full conifer crowns, #107).
/// 56: flowers and fruit follow the year from the months a species states
/// them in: rules at the flowering plants set a season's bud, flower,
/// fruit, ripe and fall days from `flowering_months` and
/// `fruiting_months`, and 28 species that state both drop their hand-set
/// days (55 is palms, #110).
/// 57: succulents grow from their facts (column, the value-free cactus
/// body the cacti now extend, cactaceae 7; Angiosperms' stem- and
/// rosette-succulent forms), and Aloe vera clumps and hangs its flowers
/// (aloe 2: pups as it flowers, nodding tubular flowers on a tall
/// raceme) (56 is flowering and fruiting months, #109).
/// 58: every organ a program draws has a look: one its looks do not list
/// takes its kind's from `appearance.defaults`, stated at the kingdom, so
/// the rules on its taxa's characters reach it (a species drafted from
/// facts draws its family's heads, spikelets, flowers, leaf cards and
/// fruit), and grasses draw blade leaves and spikelet clusters from
/// Poaceae's graminoid looks; 21 built-ins take a fruit kind or basal leaf
/// width they left unstated (57 is succulents, #111).
/// 59: organs take the colours their species' characters state, through
/// colour rules: a flower or bloom its `flower_colour`, a head its rays',
/// straps' or disc's and its disc the accent, a grass's panicle its
/// spikelets', a fruit its ripe colour, the foliage and its shade the
/// leaves' and the bark its own; 3 built-ins take a colour they left
/// unstated (58 is default looks, #115).
/// 60: finer detail from photographs and floras: a head's rays may end
/// square (`ray_tip`, a solid leaf's `tip`), a ray's width and tip come
/// from `ray_width_m` and `ray_apex`, and herb racemes may crowd their
/// buds at the tip (`bud_spacing`, `bud_lean`, `bud_stalk`); oxeye daisy
/// and common camas measured from photographs (59 is organs take the
/// colours their characters state, #117).
/// 61: a flower's filaments and style may take their own colour
/// (`filament`), and an unequal corolla's odd petal or tepal may bend down
/// as well as up (`odd_rise`); common camas is zygomorphic, its lowest
/// tepal set apart (60 is daisy and camas detail, #116).
/// 62: a flower may give its petals' taper (`taper`) where `point` alone
/// does not, and its anthers' length (`anther`); common camas grows one
/// scape from its bulb, its tepals elliptic, its anthers long, its spent
/// flowers withered round the young capsule (61 is camas zygomorphy,
/// #119).
/// 63: on the nearest level every petal takes all its stations and both
/// samples across, however short, and anthers are short bars; a flower
/// may turn its anthers across their filaments (`anther_tilt`, from
/// `anther_attachment`) and colour its ovary (`pistil`); common camas's
/// anthers are versatile and its ovary a pale green bead (62 is camas
/// detail 3, #120).
/// 64: flowers take their clades' and families' flower plans from Simpson
/// (2019) and Ronse De Craene (2010): the monocots' tepals, stamens and
/// carpels, the rosids' two whorls of stamens, the euasterids' five on the
/// tube and two carpels, and 56 families' stamens, carpels, perianth,
/// merosity, ovary, floral cup and corolla shape; willows and poplars
/// draw no perianth (63 is camas anthers and ovary, #121).
/// 65: a herb's raceme may set each flower, bud and fruit in the axil of a
/// floral bract (`herb` revision 9, `bract_length`, absent by default);
/// common camas's bracts are narrow, pale and papery, a little longer
/// than the flower stalks (64 is flower plans, #118).
/// 66: an organ whose area parameter its species' own spec does not set
/// shades with the area its look draws, not a rank file's, a form's or
/// its program's default, so a species drafted from facts shades with what
/// it shows; every built-in sets its own and grows as before (65 is floral
/// bracts, #123).
/// 67: a bud without a look of its own is its flower closed, as a lone
/// flower's bud is in its season, and the flower's rules reach it; the
/// eight herbs whose buds were unsourced ovoids (foxglove, columbine,
/// bleeding heart, monkeyflower, foamflower, paintbrush, Oregon grape,
/// tiger lily) draw them so, the lily's green-gold from photographs (66
/// is drawn areas, #124).
/// 68: flowers in clusters: a flower or bloom whose taxa state a cluster
/// kind and its length is drawn as that cluster, as long as the
/// inflorescence, its flowers as wide as the flower and on stalks as long
/// as the pedicel; a species' own cluster stands; desert ironwood's and
/// the umbrella thorn's blooms become clusters (67 is closed buds,
/// #126).
/// 69: a forb drafted from facts sends up one stem where its taxa say it
/// lasts as a bulb or a corm (the new character `perennation`, stated for
/// 84 families from Watson and Dallwitz), a clump as before otherwise;
/// every built-in sets its own and grows as before (68 is flowers in
/// clusters, #127).
/// 70: leaves on petioles of their own: a leaf whose blade is latently
/// or manifestly peltate (`insertion`, from `leaf_peltation`; Kaplan) is
/// drawn as a petiole stalk with its blade level on the tip, a peltate
/// one centred there; a palmate leaf whose spread is unset opens as far
/// as its leaflets need; a flora's measured leaf and leaflet widths come
/// before shape words; rosette leaves take the species' leaf rules;
/// large-leaved lupine's leaves are peltate rings, and 8 built-ins'
/// rosette leaves take their species' shapes (69 is one stem from a bulb,
/// #129).
/// 71: broadleaf limbs arch outward as they lengthen and an umbrella's
/// axes level out at its top (`broadleaf` revision 16, `limb_arch`,
/// `flat_top`), a self-pruning tree stands on a clear trunk
/// (`bole_clear`), and a crown's region takes its widest point and
/// fullness from its `crown_form`; Garry oak spreads, umbrella thorn is
/// flat-topped, Pacific madrone leans on a clear stem and the baobab
/// swells, from photographs (70 is leaves on petioles, #134).
/// 72: a pine's branch apex spreads a small whorl of its own each year,
/// as the trunk does (`pinidae` revision 10, `apex_whorl`), where its
/// genus follows Rauh's model; the rule at the seed plants sets it from
/// `architectural_model`, which Pinus (Rauh) and Abies (Massart) now
/// state from Simpson; lodgepole, western white and ponderosa pines grow
/// clumped, upturned branch ends; a self-pruning conifer keeps living
/// branches only on the top `crown_ratio` of its height (rule on
/// `self_pruning`), so ponderosa pine, self-pruning by FEIS, stands on a
/// clear bole (71 is broadleaf limbs, #133).
/// 73: monocot leaves: a linear monocot leaf is an arching strap on the
/// blade card, as wide for its length as its flora measures, its rosette
/// leaves as long as the leaves; a flowering stem without leaves
/// (`cauline_leaves` none) is a scape; a card may roll about its length
/// (`roll`), and unifacial sword leaves (`leaf_faces`) turn edgewise, their
/// strap pieces joined as rolled; camas states its leaves at the genus
/// (Watson and Dallwitz), Iridaceae its sword leaves; Oregon iris and
/// Idaho blue-eyed grass stand edgewise (72 is pine whorls, #136).
/// 74: cactus barrels: areoles crowd over the dome and ribs merge toward
/// the tip, and a woolly cap may top it (`Areoles.apex_wool`); a share of
/// stems keeps a Fibonacci rib count and winds left or right
/// (`Ribs.fibonacci`, `Ribs.lean`; The Cactus Primer); spine cards draw
/// only as many sub-texel spines as their width covers, and the far tint's
/// cover counts the skin between ribs, so spined succulents keep their
/// colour across levels; both Ferocactus take their ribs, wool, flower
/// rings and central spines from the Primer and Anderson (73 is monocot
/// leaves, #140).
/// 75: a column's crown may hold older rings of fruit below this year's
/// ring (`column` revision 2, `crown_rings`, `ring_step`; `cactaceae`
/// revision 8), split four ways so it nests no deeper; both Ferocactus
/// keep last year's fruit, and F. cylindraceus takes Anderson's rib,
/// radial and central counts (74 is cactus barrels, #138).
/// 76: a strap's pieces (a grass blade's, a basal leaf's) meet edge to
/// edge: each joint's edge lies across both midribs where a piece's
/// twist of up to 15 degrees reaches it, and the pieces draw in the light
/// and colour of the strap's first, so camas and other strap leaves no
/// longer step in width and shade where their pieces join (75 is cactus
/// fruit rings, #147).
/// 77: a colour character is sRGB, so a colour rule reads it as linear
/// light: every stated flower, fruit, leaf and bark colour draws as deep
/// as it is stated, where it drew paler (76 is strap joints, #145).
/// 78: a crownshaft's sheaths are its fronds' (palm program revision 8):
/// each frond draws its sheath within its `frond` structure, so the royal
/// palm and every crownshaft palm build again (until 77 a year's sheaths
/// stood within no structure, which the organ check refuses for a piece;
/// 77 is colour characters read as sRGB, #152).
/// 79: ferns from the Fern Grower's Manual: a frond arches as it unrolls
/// (`arch`, from `leaf_posture`), its fiddlehead a tight coil; fronds
/// stand on their own feet round a compact crown at the ground, among old
/// stipe bases, with no stalk under them; a rhizome's habit sets its
/// crowns and how far apart they stand, a blade's outline where its
/// longest pinnae are, and pinnae are as many, as long and as broad as the
/// blade fits, lanceolate; evergreen fronds outlast a winter; the curated
/// ferns arch, and the sword fern's pinnae and angles are the fern rules'
/// (78 is crownshaft sheaths in their fronds, #150).
/// 80: shape and fruit words draw what they mean: a cordate or reniform
/// blade hangs two basal lobes back beside its petiole (`Simple.sinus`),
/// and cordate, reniform and deltate blades take their outlines and
/// widths; a palmately veined blade fans two main veins a side from its
/// base with the midrib (`Simple.fan`, from `leaf_venation`); achenes,
/// cypselas, grains, nuts, schizocarps, aggregate and multiple fruits
/// take fruit kinds, and a fruit is as long as `fruit_size_m` (79 is
/// ferns from the Fern Grower's Manual, #153).
/// 81: cycads, palms and tree ferns told apart (palm program revision 9):
/// cycad and tree fern trunks thicken toward the base with age, cycads
/// flush their leaves in whorls inside rings of cataphylls, leaves unfold
/// by group (palm spears pleated, Cycas leaflets coiled, fern croziers
/// tight), palm leaflets fold induplicate or reduplicate by subfamily,
/// plumose palms hold their leaflets in groups, a leaf's petiole and
/// rachis take the leaf axis's colour, a palm trunk may be ringed by its
/// leaf scars, and a crownshaft is a closed column (80 is shape and
/// fruit words, #155).
/// 82: palms, cycads and tree ferns in their described shades (palm
/// program revision 10): colours their authors chose by eye as screen
/// (sRGB) shades are read as the linear light a colour field holds, so
/// trunks, fronds, boots and dead fronds no longer draw near white;
/// Phoenix's lowest leaflets are spines (`acanthophylls`), and a palm
/// without a crownshaft may clasp its stem with its fronds' sheaths
/// (`sheath_length`), the California fan palm's chestnut leaf bases
/// (81 is cycads, palms and tree ferns told apart, #154).
/// 83: a blade's tip and base draw as stated (`leaf_apex`,
/// `leaf_base_shape`): hollow-sided, straight, blunt, rounded or square
/// over its last and first three tenths (`Simple.apex_taper`,
/// `Simple.base_taper`), with a tail, cusp, mucro or bristle beyond its
/// tip (`Simple.apex_point`) or a notch in it (`Simple.apex_notch`); a
/// petal is as broad for its length as its outline is stated among any
/// count of petals (`Flower.petal_ratio`, from `petal_shape`) and its tip
/// as pointed or notched as stated (`petal_apex`) (82 is palm, cycad
/// and tree fern shades, #162).
/// 84: a blade stated serrate, dentate, crenate or spiny draws teeth
/// (`teeth`, `tooth_depth` and the new `round`, from `leaf_margin`, at the
/// kingdom), where its own look draws none: Chloranthaceae's leaves, and
/// rosette leaves that take the species' margin; a species' own teeth
/// stand (83 is leaf tip, base and petal words, #161).
/// 85: the conifers' pipe model takes its exponent from a parameter
/// (`pinidae` revision 11, `pipe_exponent`, 2.4 as before), and ponderosa
/// pine's stout shoots (0.7 to 2 cm, Farjon 2017) grow on stout limbs that
/// hold their line at 100 years, where thin limbs under its heavy needle
/// tufts hung as whips; a conifer with short shoots (`spur_years`, as
/// in larches) leafs out again along its older wood each year, so a
/// deciduous larch keeps its needle tufts along its branches; and a
/// needle card shades with its needles' own area, not its painted lines,
/// which drew each needle at least two texels wide and counted the
/// crossing card's needles again, 2.2 to 4.6 times their area: conifers'
/// crowns had starved to tufts at their limb tips and now carry foliage
/// along their limbs, a pine's card is one brush of needles at their
/// stated length (Pinus rules), and the module limit is 4,000,000; a
/// conifer's apex lives on shoots down to 5 mm, not 25, so a narrow,
/// slow crown such as subalpine fir's stays dense along its limbs (84 is
/// leaf teeth, #157).
/// 86: a flowering look the files leave without a year takes one from the
/// months its taxa state it flowers and fruits in (`appearance.organs.*.
/// season`): a flower's look becomes its look in flower and its own look
/// its fruit, drawn by the fruit's rules (its colour, a cluster's fruit at
/// its stated size, unripe fruit in the leaf green); a head or spikelet
/// keeps its look; a herb's bud and fruit organs show only in their own
/// stages. A lone flower's fruit is drawn at the fruit's size, not the
/// flower's (`season.fruit_size`), a cluster whose fruit outgrow it grows
/// to hold them, and a cluster still flowering when its first fruit ripen
/// opens over those months (`season.opening`). The four Chloranthaceae,
/// which flowered all year and never fruited, now bud, flower, fruit and
/// drop in their months, as do the magnoliids that state their months
/// (85 is conifer foliage along the limbs, #142).
/// 87: a flower with no perianth (no petals, tube, calyx or cup) spreads
/// its stamens and pistil over the reach its pieces would have taken, and
/// the reduced drawing shows them as small blunt stamens in the flower's
/// colour on a ring at any distance, not as bare sticks; seventeen
/// species state their fruiting months from their sources, so their year
/// now bears fruit.
/// 88: a fern's stipe stated scaly (`stipe_scales`) is drawn with its
/// scales (the new `palea` organ, in `stipe_scale_colour`), largest at
/// its foot, on its frond and on the stipe bases of an old crown; a
/// rachis narrows from its foot to its tip, and a small frond's is
/// thinner (fern program revision 8); the sword fern's stipe is as long
/// as Flora of North America gives (87 is flowers with no perianth,
/// #168).
/// 89: a far level's spine tint greys with each station's age as the near
/// levels' spines do, covers a rib valley half as much as a crest, and
/// hides half the skin it did, so a spiny cactus keeps its colour and rib
/// shading from afar; cacti state stem, spine, old spine and wool colours,
/// which the family's rules draw (88 is scaly fern stipes, #163).
/// 90: hairs (`crate::hairs`): a blade's hairs as floras name them
/// (`leaf_hairs`, `leaf_ciliate`) cast its colour toward theirs, add a
/// sheen and a backlit glow, fringe its edge, and up close stand from it
/// as strands of their own (89 is cactus colours, #156).
/// 91: leaves with two faces and variegation: a look's `underside` colour
/// paints its cards' lower faces (the side that looks more toward the
/// ground), and its `variegation` paints pale margins, centres, stripes,
/// bands, spots or mottles, a different share on every leaf; the royal
/// palm's pinnae are paler below and the rattlesnake plantain's leaves
/// carry their pale midvein band (90 is leaf hairs, #166).
/// 92: stem hairs (`stem_hairs`, `crate::hairs::Tube`): the hairs on
/// young stems and leaf stalks, in a blade's words and `crisped`, cast
/// the first year's wood toward their colour, wear off in its second and
/// up close stand from it as strands (91 is leaves with two faces, #164).
/// 93: a hooked spine curls into an open J at its tip: its shaft runs
/// further and its hook turns over its last two pieces, not folded flat on
/// the shaft over the last one (92 is stem hairs, #173).
/// 94: slender pedicels and short-shoot tufts (`Unit::stalk_width`): a
/// flower's pedicel is drawn 0.35 as thick as before, and a needle look's
/// bundle of more than five, up to 45, is a short shoot's tuft spread all
/// round, the tufts half a needle apart (93 is hooked spines, #176).
/// 95: lacy leaves: a compound leaf divides up to three times (`segments`,
/// `inner`); one whose taxa say how its divisions are held
/// (`division_planes`) is drawn on the two nearest levels as geometry, its
/// divisions lifted, scattered or set all round their rachis
/// (`crate::lace`); a pinnate leaf's laterals reach no farther than its
/// stated blade width (`blade_width`) (94 is pedicels and tufts, #177).
/// 96: species draw from the species-facts records: 189 species state the
/// characters no source of theirs gives (heights, leaf sizes, shapes,
/// margins and habits, inflorescence and fruit sizes, months, rosettes)
/// as the records estimate them, and a look a species set by hand gives
/// way wherever a rule fed by a stated fact now sets it (leaf widths,
/// inflorescence kinds, flower and fruit sizes and kinds, seasons); a
/// strap leaf is never narrower than its card draws (95 is lacy
/// leaves, #180).
/// 97: lacy leaves drawn as geometry: a segment at least ten times as long
/// as it is wide is a round thread, bowed a little (`lace::THREAD`), and a
/// thrice-divided leaf whose divisions roll round their rachis sets its
/// segments round each pinnule's too; a leaflet may be as narrow as a
/// hundredth of its length (96 is the species-facts records, #171).
/// 98: variegation along a parallel-veined blade's veins (`veins`: a
/// midrib stripe, then veinlet dashes, then a net round dark islands as
/// the share grows; `slant` turns the cross veins into a pinnate blade's
/// lateral veins), its inverse `between` (silver fields between the veins),
/// `zone` (a band along the outline, or a V from the base), a rim the
/// pattern never reaches, and patches on the upper face only (`faces`);
/// a compound leaf's pattern lies on each leaflet; the rattlesnake
/// plantain's leaves are drawn so, dark blue-green, six to a flat rosette
/// (97 is lacy leaves as geometry, #183).
/// 99: a fern's stipe scales stand only on its base where its flora says
/// so (`stipe_scale_extent`, fern program revision 9), as long as stated
/// (`stipe_scale_length_m`), and its stipe and rachis are drawn in its
/// stated `stipe_colour`; the fern families state whether their stipes are
/// scaly as Flora of North America gives (scaly at the base in
/// Polypodiaceae, Aspleniaceae and Pteridaceae as the Library joins them,
/// bare in Dennstaedtiaceae and Osmundaceae), so the lady, spreading wood
/// and deer ferns grow scaly stipe bases; the lady fern's frond and stipe
/// are as long as its flora gives (98 is variegation along the
/// veins, #175).
/// 100: a dimorphic fern bears fertile fronds as well as sterile ones (fern
/// program revision 10): `frond_dimorphism` dimorphic makes about three in
/// ten fertile, standing nearer the crown's centre, as long, as stalked and
/// as upright as their stated `fertile_*` characters give, with their own
/// pinnae (`fertile_pinna`, as narrow as stated); the deer fern's erect
/// fertile fronds rise from its spreading rosette as Flora of North America
/// gives, and its reference height is theirs (99 is fern stipes scaly at
/// their base, #174).
/// 101: a fern's frond comes up as a crozier (fern program revision 11): a
/// circinate frond's blade winds `coil_turns` turns (2.5 by `vernation`)
/// into a tight inward spiral of shortening pieces, its pinnae small and
/// folded inside, its scales on the coil if its stipe is scaly; it unrolls
/// from its foot, and its stipe rises uncoiled as it lengthens (100 is
/// dimorphic ferns, #178).
/// 102: a fern's pinnae bear sori on their lower faces (`looks::Sori`):
/// round, oblong, linear or marginal as `sorus_shape` says, where
/// `sorus_position` puts them on each segment the card's outline gives,
/// under the `indusium` stated; most pinnae of a fern whose fronds are
/// alike, and only the fertile pinnae of a dimorphic one (101 is fern
/// fiddleheads, #181).
/// 103: 23 species state facts the species-facts records give that the
/// checks now judge as floras measure them (#189): basal leaf sizes of
/// foxglove, columbine, foamflower, bull thistle, lupine and mule's ears,
/// grasses' and sedges' deciduous blades, and fruit, flower, cluster,
/// shape and height facts held back from 96; broom's stated obovate
/// leaflets draw where a hand-drawn profile stood (102 is fern sori,
/// #182).
/// 104: a flowering herb's top leaf nodes may each bear a side branch
/// ending in a raceme (`flowering_branches`; herb program revision 10)
/// (103 is held-back species facts, #192).
/// 105: hairs beneath a blade (`hairs_beneath`, from `leaf_hairs_beneath`)
/// and hairs gathered on its main veins (`Hairs::veins`, from
/// `leaf_hairs_on_veins`); a face's strands stand on the face that wears
/// them and on the drawn cells of a bent card, and a stated hair length
/// wins over the word's; branched hairs (`hair_branching`: stellate,
/// dendritic) and wool (simple hairs curled through more than a turn)
/// drawn as stars, little trees and winding strands, white with air;
/// glandular and stinging hairs among them (`glandular_hairs`,
/// `stinging_hairs`), with round heads and glass needles on green feet;
/// the strand budget spent on the hairs nearest the eye first (104 is
/// flowering branches, #188).
/// 106: evergreen herbs keep their basal leaves, and stems bearing leaves
/// only, into a second year, and wintergreen grasses, rushes and sedges
/// keep last year's blades green, from the leaf habit their taxa state
/// (`leaf_years`; herb program revision 11, grass revision 8) (105 is
/// hairs, #185).
/// 107: a zone's edge drawn out along the lateral veins (`feather`),
/// spots and mottle drawn out along the blade (`stretch`), and a simple
/// blade's parallel veins (`parallel`): Cyclamen's toothed silver, the
/// trout lily's long blotches and its veins meeting at the tip; and a
/// variegation's share set by the plant rather than the leaf (`plants`,
/// the card's packed seed moved toward the plant's), as Cyclamen's and
/// white clover's are (106 is evergreen herbs, #200).
/// 108: unripe fruit, and a fruit's stalk and cup, green as the foliage
/// unless an accent is stated; a cluster ripens only its
/// `fruits_per_inflorescence` (`Inflorescence::fruits`), its other units
/// gone in fruit; an aggregate drawn as its `fruitlets` say: beaked
/// follicles with their seeds hanging in `fruit_seed_colour`, samaras up a
/// spindle, berries splayed from one stalk, or a hip; a fruit with a
/// `fruit_cup_colour` stands in a cup on a swollen stalk; tepals take
/// `perianth_parts`, standing in whorls of the merosity, each inner whorl
/// more upright and narrower; `petal_whorls` doubles a
/// corolla; stamens as long as `stamen_length_m`, flat straps where
/// `stamen_form` is laminar, filaments in `filament_colour`, anthers in
/// `anther_colour`; an elongate `floral_receptacle` raises the stamens and
/// free carpels up a cone, the carpels in `pistil_colour`; `corolla_shape`
/// sets how far the petals rise; a lone flower's bud as large as its
/// flower and in `bud_colour`; up to 400 stamens and 100 carpels; reduced
/// blunt petals keep their shoulders; ripening fruit pass through
/// `fruit_colour_ripening`, and a follicle's seeds hang on threads
/// `fruit_seed_thread_m` long (107 is leaf zones and veins, #191).
/// 109: a fern's frond is green while it unrolls and turns to its stipe's
/// colour after (the L-system's `young` and the appearance's `young_axis`,
/// fern program revision 12), a crozier is scaly only where its stipe is
/// scaly all along, its pinnae lie along its coil, and a ternate frond
/// (`ternate_blade`, bracken) comes up as three coils, an eagle's claw (108
/// is magnoliid fruit, #202).
/// 110: each pinna in a fern's crozier is rolled as its frond is (fern
/// program revision 13): a short green coil of its own on each side of the
/// rachis while coiled, its card small until it unrolls (109 is green
/// croziers and the eagle's claw, #186).
/// 111: a tree fern's or a marattioid's frond comes up as a fern's crozier
/// does (palm program revision 11): where a taxon's vernation is
/// circinate (the rule now at the ferns' class), its rachis winds a tight
/// inward spiral of shortening pieces, its leaflets small and rolled
/// beside it, unrolling from its foot; the soft tree fern's croziers and
/// young stipes are a hairy red-brown (110 is rolled pinnae, #197).
/// 112: the soft tree fern puts out a year's fronds in one spring flush
/// (`emerge_spread` 0.15, from 0.3), so its croziers stand together at
/// the crown's top (111 is tree-fern croziers, #205).
/// 113: a spine set can keep to the upper areoles of a pad or joint
/// (`SpineSet::distal`, from `lower_areole_spines`): its lower areoles
/// carry fewer spines or none and draw smaller far cards, and a tuft
/// stores where it sits along its segment (112 is the soft tree fern's
/// spring flush, #207).
/// 114: a cactus column's flowers may open down its flank as well as round
/// its crown (`column` revision 3, `flank`: the pieces of its last flank
/// years open a few on their sides; `cactaceae` revision 9), from the new
/// `flower_zone`: a hedgehog's along the upper half of its stems, an organ
/// pipe's near the tips and down their sides (113 is spines on the
/// upper areoles).
/// 115: a cactus's fruit wears its pericarpel (`FruitKind::Scaly`,
/// `Areolate` and `Spiny`, from the new `fruit_surface`): broad scales on
/// a barrel's or the saguaro's, felted areoles with glochids on a prickly
/// pear's, areoles with short spines on a hedgehog's, the organ pipe's or
/// a spiny cholla's, each crowned by its dried perianth; and a cactus
/// stem's tip tapers, rounds or broadens into a shoulder as the new
/// `stem_apex` says (`BodyLook::shoulder`), and a club-shaped column
/// stays narrower at its foot (`stem_outline`) (114 is flank flowers).
/// 116: a cactus flower's pericarpel wears its fruit's skin
/// (`Flower::pericarpel`, from `fruit_surface`), its bud is the flower
/// closed (no hand bud looks; `bud_size` seven tenths of the flower's), a
/// prickly pear's or cholla's flower sits on a pericarpel longer than its
/// radius, a fruit narrows toward its base or tip (`Fruit::flare`, from
/// the new `fruit_shape`), and stigma lobes take their own colour
/// (`Flower::stigma`, from the new `stigma_colour`) (115 is fruit skins
/// and stem tips).
/// 117: a lineage whose flowers end its shoots (`flowering_position`
/// terminal or both: `tip_bloom`, broadleaf revision 17) ends each shoot
/// tip that has stopped at the crown's edge, once adult and in the light,
/// in its year's blooms, nearly along the shoot, so its flowers and fruit
/// stand on the crown's surface, not only on the short shoots inside it;
/// other plants draw as before. Sarcandra and Chloranthus serratus state
/// their terminal inflorescences. The leaf-arrangement check counts only
/// leaves born with their internode, so short shoots crowding a node are
/// not read as a whorl (116 is cactus
/// pericarpels and stigmas, #184).
/// 118: a flowering herb's side branches bear leaves, as large as the leaf
/// in whose axil they stand, and branch again where the taxon states two
/// orders (`flowering_branch_orders`; herb program revision 12) (117 is
/// terminal blooms, #170).
/// 119: a herb's stem may be a sympodium (`sympodium`; herb program
/// revision 13, program `solanaceae`); a measured corolla tube is drawn as
/// long as stated (`corolla_tube_length_m`, a flower's `tube_reach`); a
/// flower's posture turns a sympodium's flowers and a woody plant's blooms
/// (`flower_posture`); a panicle's cluster size is its branches' length; a
/// lone bloom may hang on a stalk of its own (`pedicel_length_m`;
/// broadleaf program revision 18); a flower's sepals, petals, floral tube,
/// stamens and style are drawn at their stated lengths and postures and in
/// their own colours; a fruit keeps its calyx or bears prickles or
/// bristles (`fruit_armature`); young wood takes its twig colour; bark
/// texture sets the bark pattern; leaves in whorls of three or more are
/// drawn round the node (broadleaf program revision 19); a fruit hangs,
/// spreads or stands as its posture says (`fruit_posture`, a fruit's
/// `hang`).
/// 120: a lily is drawn from its flora's words: a herb's flower and bud
/// stalks arch over to the flower's posture (herb program revision 14); a
/// flower's tepals bend back from part way along them (`curl_from`, from
/// `perianth_reflexion` and `perianth_reflexion_from`), up to 270 degrees
/// where they roll; spots on their inner faces (`spots`, from
/// `perianth_spots`, in `perianth_spot_colour`); anthers in their own
/// colour (`anther_colour`); stamens as far out as `stamen_exsertion`
/// says; free tepals unjoined however bell-shaped (`perianth_fused`); a
/// petal's or tepal's foot is drawn in its own colour where a flora gives
/// one (`base_colour`, `base_length`, from `perianth_base_colour` and
/// `perianth_base_length`), its nectaries as a star at the flower's heart
/// (`star_colour`, from `nectary_colour`); free tepals rising as a bell
/// are widened to overlap; a lily's whorled stem bears a few scattered
/// leaves below and above its whorls, and bulbils in its upper leaf axils
/// where its flora says (liliaceae program revision 2, `bulbils`,
/// `bulbil_colour`).
/// 121: previews and impostors cut a card where its coverage, read
/// bilinearly between texels, is below one half, so an entire blade's edge
/// follows its outline rather than a staircase of texels that reads as
/// teeth up close (118 is herb branch leaves, #190).
/// 122: 39 species state their leaf apex as FNA, Flora of China or FEIS
/// give it, so their blades narrow to the stated tip; six of them
/// (kinnikinnick, Canadian wild ginger, golden chinquapin, beaked
/// hazelnut, fish mint, false lily-of-the-valley) draw the outline of
/// their stated leaf shape where a hand-drawn profile stood (118 is
/// flowering herbs' branch leaves, #190).
/// 123: the family programs built on broadleaf (asterales, fabales,
/// malvaceae, moraceae, salicaceae, simmondsiaceae, each revision 2) bear
/// one bloom on each short shoot in the light once adult where a species
/// leaves `blooms` unset, as broadleaf's forms do; until now nine species
/// on them (bursage, umbrella thorn, baobab, Florida strangler fig,
/// aspen, black cottonwood, two willows, jojoba) never flowered or fruited,
/// whatever months they stated. A regular flower is not judged by the
/// kind of zygomorphy its family states (109 is fern
/// croziers green, #186).
/// 124: a chain-fruit cholla's chains of fruit hang toward the ground
/// (`column` revision 4, `cactaceae` revision 10), the organ pipe's
/// flowers carry hundreds of stamens and an 11- to 12-lobed stigma, and a
/// type of laid-out flowers too many to draw fine is drawn one step
/// coarser instead of as cards (`blooms::step`) (116 is pericarpels, buds,
/// fruit outlines and stigma colours).
/// 125: a deciduous tree's, shrub's or woody vine's leaves keep a year
/// (`appearance.organs.leaf.leafing`, set from `leaf_habit`,
/// `leaf_out_months`, `leaf_fall_months` and `leaf_colour_autumn`): the
/// plant stands bare in winter, puts out small pale leaves in spring that
/// grow to full size and green, turns its autumn colour and drops them,
/// where its leaves had stood green all year; months falling before the
/// leaves unfold are the next year's, as a southern beech's are (109 is fern
/// croziers green, #186).
/// 126: clusters draw their stated length and units: a cluster's unit
/// share (`unit`) sizes the innermost axis's flowers or fruit through the
/// branch sizes above it, a cyme's own unit ends its peduncle and its
/// units are alike, and a corymb stated rounded or globose
/// (`inflorescence_outline`, `dome`) lays its units on a sphere's cap, a
/// mophead hydrangea's a ball; a trifoliolate leaf sets its laterals at
/// the petiole's top and its terminal on a short stalk, and lobed or
/// cleft leaflets (`leaflet_incision`) draw their lobes; a pedate fern
/// frond forks into a level fan and every fern's pinnae are sized where
/// they stand, its stipe scales on the stipe (fern program revision 14);
/// a shrub's stated stem clump (`stem_clump`) sends up suckers (broadleaf
/// revision 20) and a herb's lying stems (`stem_posture`) run along the
/// ground before they rise (herb revision 15) (125 is the leafing year).
/// 127: broadleaf foliage at true size (broadleaf program revision 19): a
/// leaf is drawn at the length its species states, a lineage with short
/// shoots carries every leaf not on its long shoots on them, twigs take
/// the colour and radius their taxon states, shade-intolerant crowns shed
/// shaded limbs sooner, a living tip shades the crown as a bud, not a
/// leaf, and a shrub whose stems rise from its root crown divides at the
/// ground (126 is #206).
/// 128: cycad cones by sex (palm revision 12): a male plant bears its
/// pollen cones (`cone_male`), a female its seed cones or, in Cycas, a
/// crown of megasporophylls (`sporophyll`) with seeds along them, each
/// with its own length, count, stalk, lean and life, and the stem grows on
/// through them; a seed cone stays closed over its seeds until it breaks
/// up as it ripens (`cone_open`), showing them (`seed`). A cone is drawn
/// with an outline (`profile`: ovoid, conical, cylindrical, barrel,
/// fusiform), an apiculate tip (`apiculum`), scales parted (`open`), faces
/// by genus (`face`: Zamia's shields, Encephalartos's facets, Dioon's
/// points, a Cycas or Macrozamia spine) and as many scales as faces of
/// their stated width cover it, flush with its core (`face_width`), each
/// face as high for its width as stated (`face_height`), on a spiral or,
/// in Zamia, in vertical rows of flat-topped hexagons fitting together
/// (`rows`); a compound card's stalk may be stout (`stalk_width`: a Cycas
/// sporophyll's, its seeds hanging from it); a blade card may narrow to
/// its foot (`foot`: a Zamia leaflet's jointed base) (127 is broadleaf
/// foliage, #143).
/// 129: a fruit that stays on the plant for years
/// (`fruit_persistence` persistent) keeps no season in a program's `fruit`
/// organ, the fruit of earlier years: a chain-fruit cholla's chains hang
/// all year while each year's flowers bud, open and fruit in their months
/// (128 is cycad cones, #211).
/// 130: a reduced flower mesh (a part mesh's coarser steps) draws what its
/// perianth stands on, a floral cup or an inferior ovary such as a
/// cactus's pericarpel, so the tepals no longer float above the joint; a
/// pericarpel the tepals open straight from is globose, not a stalk.
/// 131: a cluster of flowers draws its axis as for flowers no wider than
/// half again their pedicels, not a tenth of a flower thick, so a few
/// large flowers no longer hang from a stalk thicker than their stem (130
/// is joint flowers on their pericarpels, #220).
/// 132: a petal's or tepal's outer face is drawn in its own colour where
/// a flora gives one (`back_colour`, from `perianth_abaxial_colour`), and
/// its foot outside in another (`back_base_colour`, `back_base_length`,
/// from `perianth_abaxial_base_colour` and `perianth_abaxial_base_length`);
/// a stripe down the middle of its inner face from its foot where a flora
/// names one (`stripe_colour`, `stripe_length`, from
/// `perianth_stripe_colour` and `perianth_stripe_length`), as Kellogg's
/// lily's yellow stripe; a flower's spots are drawn one-sided, as their
/// backs lie against the part; a lily with scattered leaves bears at least
/// as many as its flora counts on the stem (`shoot_leaves`), so the
/// orange lily's 50 to 75 (131 is the cluster axis, #209).
/// 133: a raceme's, spike's, catkin's, panicle's or corymb's nodes run
/// from its peduncle's top to where its top unit reaches the tip, not
/// half a node in from each, so a panicle of few branches draws its
/// stated length; a cluster whose flowers are all sterile
/// (`sterile_flowers`, a mophead hydrangea's) draws them as its showy
/// sepals (`sterile_sepal_shape`) with no stamens, pistil or floral cup; a
/// flower with no carpels stands on no ovary; a petal whose outline is
/// stated is as wide on the solid as on its card; and from the flat
/// coarse step a flower of three to six petals is a polygon through
/// their tips, not a hexagon. A lineage whose inflorescences end leafy
/// shoots (`flowering_shoot_leaves`) unfolds that many leaves below each
/// cluster, and one that flowers on new shoots (`flowering_shoots`)
/// flowers on a short shoot or a stopped tip only in its first year
/// (broadleaf program revision 22), so a hydrangea's heads stand at the
/// crown's surface above their leaves, not along the old wood; and a
/// flower may be drawn up to eight times its fruit's size, where it was
/// four, so a mophead's sterile flowers reach their stated size (132 is
/// lily leftovers, #218).
/// 134: fan palms' blades, petiole teeth, skirts and fishtail palms (palm
/// revision 13): a fan's segments part only over their outer share
/// (`split_depth`, from `fan_split_depth`), inside it one piece drawn as
/// wedges (`palman`), their free parts turning down where they part
/// (`tip_droop`, from `segment_posture`) and bearing threads in the
/// sinuses and along their margins (`filament`, from
/// `segment_filaments`); a petiole's margins bear teeth (`tooth`) as long,
/// far apart, leaning and shrinking toward the blade as stated
/// (`petiole_armature` and the `petiole_tooth_*` words); a dead frond
/// hangs from the trunk's surface, standing out at first and falling flat
/// against it over a year (`dead_fold_years`), in the colour stated for
/// dead leaves (`dead_leaf_colour`); a twice-pinnate frond bears pairs
/// of pinnae with leaflets of their own (`pinnule`, from `pinnae` and
/// `leaflets`), and a simple blade may end in a bitten, jagged tip
/// (`Simple.praemorse`, from `leaflet_end`) and lack a midrib
/// (`Simple.midrib`, from `leaflet_midrib`): Caryota's fishtail leaflets
/// (133 is panicles and hydrangeas, #219).
/// 135: a fruit's kept calyx lies as stated (`kept_posture`, from
/// `fruiting_calyx_posture`: an eggplant's cap pressed over the fruit, a
/// belladonna's flat star, or turned back); a ripe capsule opens from its
/// tip into its valves (`valves`, from `capsule_valves`: a jimsonweed's
/// four, a tobacco's two), each turned out from half way down and lined
/// paler inside; a burr's bristles may be hooked (`armed` `hooks`, from
/// `fruit_armature` `hooked`) (134 is palm details, #223).
/// 136: black cottonwood and wild fig draw single leaves at their flora's
/// size on its petiole length, and young twigs in its colour, in place of
/// hand-sized sprig cards; the fig's path_half is refitted to its height
/// (135 is nightshades round 4, #227).
/// 137: ten Pacific Northwest natives in families short of three species
/// (giant chain fern, licorice fern, scouring rush, fringecup, western dog
/// violet, California poppy, blueblossom, spreading gooseberry, bush
/// penstemon, orange honeysuckle); a climber bears its leaves in opposite
/// pairs where its taxon states them so (climber program revision 7) (136 is single
/// leaves for cottonwood and fig, #232).
/// 138: a conifer's nodding leader (`pinidae` revision 12, `nod`, as in
/// hemlocks) carries its year's needles in two sprays, one at the foot of
/// each half of the shoot, so they follow the bend; the one spray at the
/// shoot's foot had run on straight and left the nodding tip a bare
/// stick. Leaders that do not nod grow as before. A conifer may grow
/// several stems from one foot (`stems`, `stem_angle`), and a needle card
/// may lean its needles forward (`lean`) and leave the foot of each axis
/// bare (`bare`) so they crowd in a tuft near its end; both default to
/// what was drawn before (137 is ten Pacific Northwest natives, #234).
/// 139: old conifers (`pinidae` revision 13): a limb whose tips catch too
/// little light for the wood it carries is shed (`limb_light`, scaled by
/// `shade_death`), so Douglas-fir, western hemlock and Sitka spruce grow
/// past 150 years under the module limit; a shoot shorter than
/// `bud_length` sets proportionally fewer side buds, so a slow crown no
/// longer smothers itself; and a pine's limbs keep a tenth of each year's
/// sag, not half, so old pines' limbs no longer hang to the ground; and
/// a branch's yearly shoot may be a share of the leader's (`apical`,
/// fitted to photographed crowns for six Pinaceae, 0.5 for the rest but
/// pines; `branch_floor`), so crowns are cones instead of tubes, and the
/// family's limbs keep 0.15 of each year's sag; and the family's old
/// limbs branch again (`side_order` 2, twigs living `twig_years` with one
/// spray each), stop lengthening with their reach, start short below the
/// treetop (`top_ramp`) and clear the trunk in step with height (`bole`),
/// so fully branched crowns grow to 300 years; shade strengthens apical
/// control (`apical_light`, scaled by shade tolerance), so a stand-grown
/// Douglas-fir keeps a narrow crown and a trunk of its reference size;
/// limb shedding and short shoots' fewer buds are set for Pinaceae, not
/// the program, so yews and cypresses grow as before (138 is nodding
/// leaders, #167).
/// 140: a cactus's pericarpel is drawn in its stem's colour
/// (`pericarpel_colour`, from `stem_colour`), and a reduced flower mesh
/// wears its skin (scales, felted areoles or spines) as the full one does;
/// a spiny fruit whose spine clusters fall (`fruit_spine_fall` deciduous:
/// Stenocereus, Echinocereus) ripens with its felted areoles alone; a
/// pericarpel's and a cactus fruit's areoles take the stem's wool colour
/// and their glochids or spines the colours floras give them (new trait
/// `glochid_colour`, also the stem's glochids); and a round fruit with no
/// stated width is as wide as long (137 is #234; 138 and 139 are #167's
/// nodding leaders and old-conifer refit).
/// 141: each organ type's part mesh (a flower, fruit, cluster, flower head
/// or needle shoot drawn near the camera) is the organ at its finest, with
/// no cap on its triangles; simplifying is the caller's choice (`plantc
/// export --triangles`, `--max-mib`) (140 is cactus pericarpels, #226).
/// 142: a palm's fronds stand up the year's trunk as they emerge over the
/// year (palm 14), so its leaf bases and boots wind evenly up the trunk in
/// place of bands at each year's top; Phoenix keeps its broad leaf bases
/// for the trunk's life, standing close to the stem (141 is full-quality
/// part meshes, #237).
/// 143: a cactus's central spine form draws its spines: curved and twisted
/// centrals bend (`curve`), a hooked principal central turns back as a
/// fishhook (`hook`), in place of hooks and curves set by hand; a woolly
/// pericarpel and fruit (`fruit_surface` woolly: Homalocephala,
/// Astrophytum); a flat stem top (`stem_apex` flat); white hair flecks
/// between the areoles (new trait `stem_flecks`, body `flecks`); an
/// apex's woolly cap closes over the tip; a reduced flower with more than
/// 12 stamens draws a ring of anthers; a cactus's floral tube wears its
/// pericarpel's colour and skin and flares near its mouth, over a
/// pericarpel shortened to the ovary's swelling; a column stem's sink follows
/// its height curve, so a seedling's top shows (142 is palm
/// trunks, #240).
/// 144: branching form (broadleaf 23): a shoot without a terminal bud
/// dies back each year and its topmost lateral carries it on, sometimes
/// as a fork (`tip_abort`, from `terminal_bud`); a plagiotropic plant's
/// limbs carry flat sprays (`plagio`, from `branch_orientation`); a
/// tree's dead branches stand until they fall (`dead_keep`); a
/// small-leaved tree's or shrub's long shoots set their leaves no farther
/// apart than a leaf (`unit_nodes`); crown break-up and chance tip loss
/// are switches no rule turns on yet (`breakup_age`, `tip_loss`) (143 is
/// cactus genera, #239).
/// 145: a simple leaf's petiole may be up to 0.9 of its card (a sundew's
/// blade on a stalk four times its length); fleshy and succulent blades
/// are drawn solid (`leaf_succulence`, `leaf_cross_section`); a herb's
/// rosette and flowering stems are as many as its taxa count
/// (`rosette_leaves`, `flowering_stems`), a leafless flowering stem is a
/// scape in any flowering plant, and a compound inflorescence's branches
/// take the kind its taxa name (`inflorescence_branch_kind`); smooth bark
/// draws with a valid pattern; a shrub without short shoots has a short
/// share; a glandular hair has a head, a count per square centimetre,
/// a margin length that grades it from short and upright to long and
/// spreading, a stalk colour and a drop of clear, milky or amber secretion,
/// and strands draw on flower and cluster cards (fifteen Caryophyllales
/// species, two more sundews and hemp, #243; 144 is #244).
/// 146: an opened capsule shows its seeds (`seed_size`, from
/// `fruit_seed_length_m`, where `fruit_seed_colour` is stated) packed
/// round a placenta between its valves; a joined calyx is drawn from
/// `sepal_length_m` and `calyx_lobe_length_m` (`sepals_fused`), a sheath
/// standing close round the corolla tube, angled where `calyx_section`
/// says so (`calyx_angled`), its teeth carrying on up its line; a calyx
/// that breaks off round its base (`calyx_in_fruit` `circumscissile`)
/// leaves a torn rim; a lobed blade takes its lobes, sinus depth, lobe
/// tips (`leaf_lobe_apex`), width and petiole from its traits, acute lobes
/// narrowing fast to a point; a joined calyx may swell low down
/// (`calyx_swell`, from `calyx_base` `swollen`); a herb's stem leaf card
/// holds its petiole unless the herb has a basal rosette; a map whose
/// states tie reads its `else` formula's names; a nightshade's prickles
/// stand among its stem hairs (`prickles`, from `spines` `prickles` and
/// `prickle_density`) (145 is the
/// Caryophyllales families, #243).
/// 147: old conifers keep their foliage from 150 to 300 years: a side
/// shoot keeps growing at least `side_floor` once the leader stops rising
/// (coming in over the top `floor_ramp` share of the height), the family's
/// dormant buds wait on the limbs' main axes for 300 years (`epi_order`),
/// a limb is shed for the mean light of its tips (`limb_tip`) more than
/// for its size, and slow shoots are laid down every few years
/// (`seg_min`) so the fully branched old crowns fit the module limit,
/// each drawing its foliage at two places along it so its needles stay
/// near one year's length; a
/// tall tree lays down less trunk wood per square metre of foliage once
/// its leader passes `ring_share` of its full height; and the top whorls'
/// branch shoots lengthen over `top_ramp` years of the leader's growth in
/// place of a fixed depth (146 is nightshades round 5, #233).
/// 148: a depressed cactus top sinks to a pit at its centre, with its
/// crown's flowers and fruit set down into it (`stem_apex` depressed,
/// body and program `pit`: Lophophora); an apex's woolly cap reaches only
/// the apical region, three fifths of the tip's radius, not the whole top
/// of a rounded or broad stem; twisted centrals turn about their length
/// (new trait `central_spines_twisted`, spine `twist`) (147 is old
/// conifers, #251).
/// 149: palm 15 and Phoenix's details. A Phoenix's dead frond hangs limp,
/// its leaflets folded up along the rachis and the rachis drooping, in
/// place of a flat plank standing out below the crown. A boot wears shorter
/// as it ages (`boot_wear`, `boot_wear_years`) and each is a little longer
/// or shorter (`boot_spread`), so a Phoenix trunk's lower stubs are shorter
/// and its outline ragged. The foot puts out a mass of adventitious roots
/// from a root zone that rises with age, about half stopping short in the
/// air (`roots`). An open sheath (`sheath_wedge`, Phoenix's) is drawn as a
/// broad brown wedge along its petiole's base, so the crown's centre is
/// clothed in leaf bases where the bare stem showed as a flat-topped
/// cylinder (148 is cacti, #248).
/// 150: poppies. A corolla of merosity 2 in two whorls draws its four
/// petals (a poppy's 2 + 2), where the petals rule clamped the merosity to
/// 3 and drew six; a caducous calyx (`calyx_caducous`, Papaveraceae)
/// falls as its flower opens, so a bud wears its sepals and an open flower
/// shows none (`calyx_shed` on the flower card). California poppy grows
/// leafy branched stems with larger flowers and a capsule beside them;
/// opium poppy is new; Pacific bleeding heart's leaves are ternate and
/// dissected, glaucous, held over the clump.
/// Round 2: two crossed pairs of petals open as one cup; a disymmetric
/// corolla (`flower_symmetry: disymmetric`, Dicentra) draws two pouched
/// outer petals with turned-out tips around two narrow inner ones, a
/// heart; a pendent or nodding flower of a cluster hangs from its pedicel
/// (`nod`, from `flower_posture`); a capsule with a stigmatic disc
/// (`stigmatic_disc`, Papaver) wears a dark rayed dome, one ray per
/// carpel; a fruit's unripe share takes `fruit_colour_unripe` before the
/// leaves' green. Round 3: a herb's flowering side branches flower after
/// its stem, each a share of a raceme younger (`branch_lag`, herb
/// revision 16; Papaveraceae a third); a nodding bud (`bud_posture`)
/// hangs from a stalk hooked in its top quarter, as long as floras
/// measure it (`bud_length_m`); a caducous calyx closed over a regular
/// corolla's bud is drawn as an ovoid of its sepals; a heart's petal
/// parts have curved sides. A herb that flowers on for months while its
/// first fruit ripen, bearing fruit below its flowers (`fruit_below`),
/// shows its earlier flowers' fruit from its first fruiting month and
/// keeps flowering to its last (149 is palms, #250).
/// 151: a columnar Pinaceae crown stops widening once its height slows:
/// its shoots' floor growth fades past `floor_reach` of the height out
/// from the trunk, down to `floor_keep` (Pinaceae rule on `crown_form`;
/// subalpine fir; 150 is poppies, #242).
/// 152: a species' spec states its bark in the bark language
/// (`appearance.bark_look`, `crate::bark_look`): stages by stem diameter,
/// keyed by name so a genus states how its bark breaks up and its species
/// add their colours; eleven reference genera state theirs (Acer, Alnus,
/// Arbutus, Betula, Pinus, Populus, Prunus, Pseudotsuga, Quercus, Rosa,
/// Thuja), and every other species with a bark pattern draws a look made
/// from it; a species may instead describe its bark in a flora's words
/// (`bark_architecture`, `fissure_network`, `exfoliation`,
/// `lenticel_shape`, `old_bark_on` and the rest), which rules at phylum
/// Tracheophyta turn into its young, grown and old stages; an old stage
/// shows up from the trunk's base, or on limbs' undersides, where stated,
/// and each stage gives way to the next in mixed steps;
/// `plantc export --bark-look spec` bakes it; and a thorn or prickle
/// stands on its stem's surface, a prickle as a flattened cone curving
/// back as its flora says (`spine_curve`, `spine_angle`, `spine_width_m`)
/// with more between the nodes (`internodal_prickles`, `aciculi`) or
/// only there (`prickle_position`), and
/// a shoot's thorns stand alone where they are shoots (`thorn_origin`),
/// on as many nodes as its flora says (`thorn_frequency`), as long as its
/// range runs, falling from older stems where it says so
/// (`thorn_persistence`), and tipping its twigs where they are terminal
/// (`thorn_position`: a palo verde's; broadleaf 24), by rules for all
/// flowering plants
/// (151 is
/// subalpine fir; the desk
/// renumbers on landing).
pub const GENERATOR_REVISION: u32 = 152;
