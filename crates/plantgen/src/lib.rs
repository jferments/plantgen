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
pub mod impostor;
pub mod inherit;
pub mod json;
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
pub const GENERATOR_REVISION: u32 = 47;
