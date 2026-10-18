# Task: compare renders with photos and with the text

Inputs: the photo observations, the render observations (same features,
same model), the species' text facts, and `render-set.json`. Steps 1 and 2
are arithmetic and can be a script or a spreadsheet. Step 4 is written by
your AI or by you.

## 1. Aggregate the photo answers (per species, per feature)

1. Drop "cannot tell" and answers from photos triage marked unusable,
   `cultivar_look: yes`, or (for habit and architecture) a setting different
   from the one being compared.
2. One vote per observation: if several photos come from the same
   observation, or the same photographer on the same day, merge them first
   (the majority of their answers, or their median).
3. If the run has a calibration table (`07-calibrate.md`), drop answers from
   a model that is "not trusted" for this feature.
4. Summarise:
   - **state, states, ordinal**: the share of each state, the majority and
     its share, n;
   - **ratio, share, angle, count**: median, 25th and 75th percentile, n;
   - **colour**: convert hex to CIELAB; median L, a, b per lighting group; n
     per group.
5. Mark the feature **insufficient** if fewer than 5 answers from at least
   3 photographers remain. Insufficient features are not compared; they go
   on the list of photos to find.

Render answers are aggregated the same way over seeds.

## 2. Give each feature a verdict

| Type | ✓ matches | ~ partly | ✗ misses |
| --- | --- | --- | --- |
| state, states | the render's state has 25% or more of the photo answers | 10-25% | under 10%, while the photo majority has 60% or more |
| ordinal | same state as the majority, or a neighbour with 25% or more | one step from the majority | two or more steps |
| ratio, share, angle, count | render median inside the photo IQR | within 1.5 x IQR of the photo median | further |
| colour (same lighting) | ΔE2000 under 15 | 15-25 | over 25 |

If photos are split with no majority (no state over 60%), a render showing
any state with 25% or more passes: the plant varies, and so may the
drawing.

Two more rules from the first replay against hand verdicts (v1):

- **Renders need numbers too.** Compare a feature only when at least three
  render answers exist (three images: seeds, focus points, panels). With one
  or two, the verdict is "render insufficient": make more renders. A single
  render frame decided most of v0's false misses.
- **Missing organs.** A render that shows no flowers or fruit gives no
  answer to flower and fruit features, so nothing gets compared. Check
  presence separately: among the photos taken within a month of the render
  day (at least five), if 25% or more show an organ (triage
  `organs_visible`) and no whole-plant or shoot render does, that is a ✗
  "organ missing on the render day".
- ✗ is a claim that the drawing misses; ~ is a lead to look at, not a
  claim. Report them apart.

Colour thresholds were widened from 10/20 to 15/25 in v1: in the replay, a
typical photo sat ΔE 8-12 from the median of its own species, feature and
lighting label (49 groups), so ΔE 10 flagged renders as close as a typical
photo.

## 3. Bring in the text

For every feature with a `maps_to` trait, look up what the species' sources
say about that character: in PlantGen, `plantc traits <id>` shows each
stated character, its value and its source. Quote the source words, in your
own words where a quote is long, with the source named.

Then read the four together:

| Sources | Photos | Render | Means |
| --- | --- | --- | --- |
| state X | mostly X | X | ✓ |
| state X | mostly X | Y | the drawing misses a stated fact: find why (`05-diagnose.md`) |
| silent | mostly X | Y | the drawing misses an unstated feature: the character may need stating (Part 1) or adding |
| state X | mostly Y | X or Y | **conflict**: check the photos (cultivars, age, region) and re-read the source; never change the drawing on photos alone |

This step can be a script: map each feature's `maps_to` trait to the
species' stated value; when the render's state is the stated one and the
photos differ, the verdict becomes "conflict", not ✗.
| state X | insufficient | Y | the drawing misses a stated fact; no photos needed to say so |

## 4. Severity and order

For each ✗ and ~:

```
severity = scale weight x miss size x confidence
```

- scale weight: habit 1.0, architecture 0.9, foliage 0.8, surface colour
  0.6, shoot 0.6, flower and inflorescence 0.5, fruit 0.4, leaf 0.4 (the
  larger the scale, the further away the mistake shows);
- miss size: ✗ = 1, ~ = 0.5;
- confidence: the share of the photo answers that were high confidence.

Sort findings by severity. After diagnosis, sort again by severity times the
number of species the same cause affects (`06-generalize.md`).

## 5. Write it down: `COMPARE.md`

Before anyone shows the render, write one table per species:

```markdown
## <Species> (<renders: commit, age, day, env, seeds>)

| Feature | Sources say | Photos show (n) | Render shows | | Cause |
| --- | --- | --- | --- | --- | --- |
| crown outline | "short branches turned up at the ends make a regular spire-like crown" (Abrams) | conical 9/14, rounded 4/14 | rounded egg | ✗ | |
| needle colour | "bright yellow-green" (Abrams) | yellow-green, sun, n=11 | mid-green, ΔE 23 | ✗ | |
| needles per bundle | 3 (Abrams, FEIS) | 3 (8/8) | 3 | ✓ | |
```

Then the renders and the photos side by side, from the same views: side,
45 degrees down, and a close-up of a branch or leaf at least.

Also write `findings.json` (`schemas/finding.schema.json`), one object per
✗ or ~, which diagnosis fills in.

## 6. What not to do

- Don't call a match on a feature the photos cannot answer: insufficient is
  insufficient.
- Don't compare a forest photo with an open-grown render, or July photos with
  a render in October.
- Don't let a striking difference in one photo outweigh the aggregate.
- Don't write "looks wrong": name the feature, the states, the numbers.
