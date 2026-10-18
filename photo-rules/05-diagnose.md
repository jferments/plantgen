# Task: diagnose each difference

For a coding agent (any make) or a person with the PlantGen repository
(`jferments/plantgen`) and a built `plantc`. Input: `findings.json` from
`04-compare.md`. Output: the same findings, each with one cause and the file
and key that own it.

Work through the causes **in this order**. The first three change what the
others are measured against; D8 hides every rule beneath it.

| Code | Cause | How to test | The fix lands in |
| --- | --- | --- | --- |
| D0 | **Setup mismatch**: the render's age, day, env, view or distance differs from the photos | Re-render with matched conditions (`03-match-renders.md`); is the miss still there? | the comparison |
| D9 | **Photo bias**: the photos are cultivars, juveniles, pruned, one region, one season, or one organ | Re-check triage for the photos behind the majority; do wild, mature, open-grown photos alone still show it? | the photo set |
| D10 | **Source or extraction error**: photos overwhelmingly contradict a stated fact | Re-read the source passage (book store, flora); was it extracted right? Does another source disagree? | Part 1: correct the fact |
| D8 | **A hand-set species value blocks the rule** | `plantc spec <id>`: is the value set in the species' own `spec.json` with an `Authored` evidence note, while a rule exists for its character? | delete the hand value; let the rule decide |
| D1 | **Fact missing from the spec**: the books state it, the spec does not | `plantc traits <id>` lists the character as missing; search the book store and species-facts run for it | Part 1: state the fact at the highest rank it holds for |
| D2 | **A character no rule reads** | `plantc rules`: no rule reads the character; or `plantc spec` shows the value comes from a rank default | a new rule in a rank file |
| D3 | **The rule skips this state** | The rule's `map` for the character has no entry for the stated word, so it falls to `else` or the default | add the state to the rule's map |
| D4 | **The rule maps the word to the wrong number** | The rule fires (shown in `plantc spec`), but across all species stating the word, renders miss the photos the same way | recalibrate the map (`06-generalize.md` §4) |
| D5 | **No character for what the photos show** | The feature is `proposed` in `features.yaml`, or no `traits.json` character fits | add the character and states; ask Part 1 to extract it |
| D6 | **The engine cannot draw it** | No organ, parameter or growth behaviour in the program can produce it: `plantc organs` shows the organ absent, or no parameter in the program's file changes it | engine work: a new behaviour, off unless a character turns it on |
| D7 | **Material, shading or colour pipeline** | The spec's colour or value is right (`plantc spec`), but the rendered pixels are not: sample the render's lit pixels against the spec RGB | the renderer |
| D11 | **Truly species-specific**: nothing in any source explains it | Every cause above ruled out | a species value, with photo evidence, listed as debt |

## How to find the code behind a feature

1. `features.yaml` `maps_to` names the character (`trait`) or the measure.
2. `plantc spec <id>` prints every spec value with the file that set it
   (species, genus, family, `library/_ranks/...`, or a rule) and its evidence.
   Find the drawing value the feature depends on (for example
   `appearance.organs.leaf.shape.width` for leaf length over width).
3. `plantc rules` lists every rule by its home rank, and which character it
   reads. The rule's `map`, `template`, `between` and `else` show what each
   word becomes.
4. The rank files are JSON under `crates/plantgen/library/`: `family.json`
   and `genus.json` beside the species, and `_ranks/<rank>/<name>.json`.
   Values merge top-down; the nearest file wins.
5. Drawing code that consumes the value: search `crates/plantgen/src/` for
   the spec key (for example `ray_tip` in `capitula.rs`).

## Write it down

Fill each finding's `cause`, `suspect` (file, key, rule id) and `test` (the
command and what it showed). A finding is diagnosed when someone else could
run the test and see the same thing.

## Examples from PlantGen's own reviews

| Finding | Cause | Why |
| --- | --- | --- |
| Whitebark pine render far smaller and more regular than photos | D0 | rendered at 60 years; photos show trees centuries old |
| Magnoliid fruit absent | D0 then D6 | not on the render day; herbs' fruit share defaults to 0 |
| Spicebush drawn as one big flower | D1 | `inflorescence` stated, `inflorescence_length_m` and `flower_size_m` missing |
| Toothless leaves on Sarcandra | D2 | no rule read `leaf_margin` (fixed in plantgen#157) |
| Wild ginger leaves without a notch | D3 | `leaf_shape` cordate had no entry |
| Cholla tubercles too low | D4 | "prominent" mapped to 0.3; pooled photos say about 0.42 |
| Pawpaw leaves crowded at shoot tips | D5 | no character for leaf crowding |
| Old whitebark with hundreds of thin limbs | D6 | no limb self-thinning in the conifer program |
| Pawpaw's maroon flowers drawn pink | D7 | spec colour right, rendered colour light |
| Sassafras lobes narrow and spiky | D8 | a hand-cut leaf card in the species file |
| Jumping cholla's hanging fruit chains | D11 | species habit, after the rules |
