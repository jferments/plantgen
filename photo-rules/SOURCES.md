# Where to get good plant photos

What PlantGen's reviews used, and what was checked live on 2026-10-14,
ranked
by what each source is best for. "Used" means a PlantGen review relied on it;
"checked" means its access and licences were tested; "known" means a good
source we have not tried yet.

## The short list

| For | Best source | Then |
| --- | --- | --- |
| Organs close up (leaves on the plant, flowers, fruit, bark) | iNaturalist research grade | Wikimedia Commons |
| Whole-plant habit and architecture | your own photos (`CAPTURE.md`) | Wikimedia Commons; iNaturalist filtered by triage for view V1 |
| True sizes, flat leaf outlines, margins, venation | herbarium sheets with a ruler | botanical drawings with a scale |
| Shape words (outline, flower form) in standard views | public-domain botanical drawings | herbarium sheets |
| Training an organ classifier for triage | Pl@ntNet-300K | PlantCLEF |
| Phenology (when it flowers and fruits) | iNaturalist annotations and dates | herbarium collection dates |

## iNaturalist (used, checked)

- **What:** citizen-science observations; "research grade" means the
  identification is agreed by the community.
- **Good for:** organs in daylight, variation across the range, colour,
  phenology (observers annotate "Flowering", "Fruiting"), and volume: a common
  species has thousands of usable photos (daisy, research grade with open
  licences: 12,491 observations, 1,813 annotated flowering).
- **Weak for:** whole trees (people photograph flowers), absolute size (no
  scale), garden plants (filter them out), and the most-voted photos skew
  toward showy flowers and early season.
- **Filters to use:** research grade; not captive or cultivated; photo
  licence CC0, CC-BY or CC-BY-SA (much is CC-BY-NC; see licences below);
  annotations for stage; order by votes for quality, but sample beyond the
  top for variety; one photo per observer per species unless views differ.
- **Access:** the API (`api.inaturalist.org`, at most about one request a
  second) for searches and annotations; the open-data bucket on AWS
  (`s3://inaturalist-open-data`, no signing; photos in five sizes up to
  2048 px, with a metadata dump) for bulk.
- **How PlantGen used it:** 16 research-grade photos per species for the
  magnoliid and Chloranthaceae comparisons, the cacti baseline and the
  woody-forms baseline; the daisy and camas pilot (200 photos each).

## Herbarium sheets (checked)

- **What:** pressed, dried specimens on paper, photographed flat, usually
  with a ruler and often a colour card.
- **Good for:** true sizes (the ruler), leaf outlines and margins with no
  foreshortening, venation, leaf arrangement on a twig, many specimens per
  species from across the range and across a century.
- **Weak for:** colour (faded and browned), posture (pressed flat), habit.
- **Where:** GBIF occurrence search with images and basis of record
  "preserved specimen" (camas alone: 2,280 sheets with images); iDigBio;
  regional portals such as the Consortium of Pacific Northwest Herbaria,
  SEINet, SERNEC, CCH2; the Herbarium 2022 dataset (about 1.05 million
  sheets of 15,500 North American species, research-use terms).
- **Licences:** often CC-BY-NC, some CC-BY. Image files sit on each
  herbarium's own server.

## Public-domain botanical drawings (checked access, partly)

- **What:** line drawings and plates from floras and botanical magazines
  published before about 1930.
- **Good for:** shape words. A drawing shows each organ in a standard view,
  often at a stated scale, with the features the botanist thought
  diagnostic. Because they are public domain, outlines traced from them can
  be kept and committed.
- **Weak for:** colour (line art), variation (one idealised plant), habit.
- **Where:** Britton and Brown's *Illustrated Flora of the Northern United
  States and Canada* (1913), on the Biodiversity Heritage Library and
  archive.org (`illustratedflor03brit` and its sister volumes) and per
  species on USDA PLANTS; known but untried: *Curtis's Botanical Magazine*,
  *Flora Danica*, Sowerby's *English Botany*, Köhler's *Medizinal-Pflanzen*,
  all on the Biodiversity Heritage Library.
- **Access note:** the Biodiversity Heritage Library and archive.org's image
  servers were blocked from PlantGen's cloud sessions; they work from an
  ordinary machine.

## Wikimedia Commons (checked)

- **Good for:** whole-tree habit shots and bark, often large and well
  exposed; "Quality images" and "Featured pictures" are reviewed for
  technical quality.
- **Weak for:** coverage of less common species (tens to hundreds of files
  per species), and identifications are not reviewed like iNaturalist's.
- **Licences:** CC-BY, CC-BY-SA, public domain, per file.
- **Access:** the MediaWiki API (category members, then image info); it
  rate-limits quickly, so wait between calls.

## Pl@ntNet-300K and PlantCLEF (checked)

- **Good for:** organ-tagged images (flower, leaf, fruit, bark, habit), so a
  small model can learn triage without an AI; Pl@ntNet-300K has 306,000
  images of 1,081 species on Zenodo and Hugging Face (CC-BY data).
- **Weak for:** mostly European species; mixed licences per image.

## Your own photos (used)

The best source for habit and architecture, which online collections lack.
Five photos of whitebark pine taken for PlantGen drove that species' refit.
Follow `CAPTURE.md`.

## Book figures (used; comparison only)

The figures in the floras and monographs we extract text from (PlantGen's
book store catalogues 8,106 cropped figures) are tied to the very text that
describes them. They are copyright: look, compare, never keep or publish.

## Known, not yet tried

- **CalPhotos** (University of California, Berkeley): many habit photos of
  western North American plants; licences vary, many non-commercial.
- **USDA Forest Service** photo collections and the Fire Effects Information
  System (FEIS) species reviews, which PlantGen already cites for text.
- **The Gymnosperm Database** (conifers): descriptions and photos;
  copyright, comparison only.
- **Plants of the World Online** (Kew) and **Tropicos** (Missouri Botanical
  Garden): curated images for many tropical species.

## Licences

- Keep numbers, words and photo ids; never redistribute photos. Every
  observation records the photo id, source, creator and licence.
- CC0, CC-BY and CC-BY-SA: fine for observations that are kept.
- CC-BY-NC: may feed stored numbers and words (the owner's decision, 2026-10-17),
  like CC0 and CC-BY; the photos themselves are still never kept public.
  Credit the creator in the observation record.
- Copyright (book figures, most specialist sites): compare only.

## Photos by view, to a target per view

Take photos by view, to the target for each view below, not at random. In
the v1 replay, 29-32 photos per species fetched at random still left the
whole plant, leafless and leaf-underside views under five answers, and 13 of
62 hand-found misses could not be tested for want of them. Online
collections are mostly flowers; the other views have to be sought.

A species' set is solid when each view that applies has its target of
usable photos (triage `usable`, not `cultivar_look`), from at least three
photographers:

| View | Triage shows | Photos |
| --- | --- | --- |
| Whole plant, side, open-grown | V1 | 8 |
| Leafless, in winter (deciduous trees and shrubs) | V2 | 5 |
| Shoot close-up | V4 | 8 |
| Leaf face-on, plus herbarium sheets for size | V5 | 10 + 5 sheets |
| Leaf underside | V4 or V5 with `leaf_face` lower or both | 5 |
| Flower face-on | V6 | 8 |
| Flower or cluster from the side | V7 | 6 |
| Fruit or cone | V8 with fruit or cone | 8 |
| Bark or stem surface | V8 with bark or stem | 5 |
| Cacti: the apex, from above or close | V3 or V8 with apex | 5 |
| Cacti: areole close-up | V8 with areole | 8 |

For cacti, the leaf views do not apply. For a family rule, the same for
three or more species in two or more genera.

How to get there: fetch more than the targets need, triage, and count what
each view has (`imagekit views`, see `imagekit/README.md`); then fetch again
for the views still short:

- whole plants: Wikimedia Commons and your own photos (`CAPTURE.md`); from
  iNaturalist, fetch many and keep what triage calls V1;
- leafless: iNaturalist with `--months` set to the winter months;
- flowers and fruit: iNaturalist with `--phenology flowering` or
  `fruiting`;
- leaf face-on and sizes: herbarium sheets (GBIF, `--basis
  PRESERVED_SPECIMEN`);
- undersides, apexes and areoles: a large iNaturalist fetch, searched by
  look (`imagekit search --model siglip2 --text "underside of a leaf"`)
  before triage.
