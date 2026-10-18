# Task: turn a cause into a drawing rule

For a coding agent or a person with the PlantGen repository. Input:
diagnosed findings with causes D2, D3, D4, D5 or D8. Output: a `proposal`
(`schemas/proposal.schema.json`), then a pull request.

The aim: **fix the word, not the species.** Every species that states the
same character improves with one change, and a species added later from
text alone is drawn right without anyone looking at its photos.

## 1. Name the character

Find the source sentence that describes what the photos show.
- Whitebark needles forward and crowded at the twig tips: Farjon,
  "dense tufts near branchlet ends, connivent, curved forward against
  shoot".
- Daisy rays squared off with small teeth: `ray_apex` toothed.

Search the book store and species-facts run for the word and its synonyms.
If no source has a word for it, the cause is D5: propose a character
(name, organ, type, states with visual definitions from `features.yaml`).

## 2. Count its breadth

How many taxa state the same character and state? Look in the library
(`plantc traits` over all species, or the spec files), the book store, and
the species-facts run. Farjon describes foliage tufted at branch ends for
about 50 conifers; a toothed `leaf_margin` appears for over 11,000 species
in the species-facts run. Breadth sets the priority and the rank.

## 3. Choose the rank

The rule goes at the most inclusive rank where the word means the same
drawing:

| Rank | When | Examples |
| --- | --- | --- |
| all plants or seed plants | an organ word whose look does not depend on lineage | leaf margin, apex, base; corolla shape; fruit shape; colours |
| family or subfamily | the family gives the word its own geometry | `tubercle_relief` in Cactaceae; ray tips in Asteraceae; cupped tepals in Opuntioideae |
| genus | a lineage default the genus states | Ferocactus funnelform flowers; flowers between tubercles in Cochemiea |
| species | only cause D11 | |

State the character itself at the highest taxon it holds for, too. A
character that photos show for many species in a genus belongs in the
genus file, not repeated in each species.

## 4. Fit the word, not the species

When the rule maps a word to a number (cause D4, or a new rule):

1. Take every species that states the word and has enough photos: at least
   three species from at least two genera where the library has them.
2. Measure the feature on their photos (pooled per species, then the median
   of species medians, so one well-photographed species does not dominate).
3. Choose the value or formula that brings all their renders inside the
   photo ranges.
4. Hold one species out of the fit; it must improve too.
5. Record the fit: species used, n per species, the value before and after.

The Cactaceae `tubercle_relief` map (barely 0.01, low 0.03, distinct 0.2,
prominent 0.42) was set this way across six species in three genera.

## 5. Know the blast radius

List every library species whose drawing changes (compare growth digests
before and after, `plantc grow --digest`). New behaviours stay off unless a
character turns them on, so species that do not state the word keep their
digests. Re-render every changed species in the standard views and compare
them again (`04-compare.md`), not only the species that raised the
finding.

## 6. Remove what the rule replaces

Delete species-level hand-set values the rule now covers, in the same change,
and compare before and after. This is how hand-tuning comes down.

## 7. The proposal

Fill `schemas/proposal.schema.json`: the character and states, the rank and
file, the map and how it was fitted, supporting and held-out species, the
blast radius, source statements, the findings it closes, and the hand-set
values it removes. Then build it as a pull request with before and after
renders and the new `COMPARE.md` for every changed species.

PlantGen's own rules for pull requests apply: the owner's go before merging,
values carry evidence notes, source text in our own words, no AI in
`plantc`.
