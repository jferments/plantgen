# Worked example: whitebark pine

This walks the loop on the case that shaped it: PlantGen's whitebark pine
(*Pinus albicaulis*) refit against five photos taken for PlantGen,
2026-10-16 and 17. The findings and sources below are that review's. The JSON files beside this page show the formats; their answer
wording is illustrative, written to the rules, not a recorded model run.

## 1. Triage

Each photo gets a triage object ([`whitebark-pine/triage.json`](whitebark-pine/triage.json)).
What mattered: `age_class: old`, `setting: open_grown`, views V1 and V2, and
three stems from the base.

## 2. Observe

Each usable photo answers the habit, architecture and foliage features for
V1 and V2, and the close-ups answer the shoot features
([`whitebark-pine/observation-photo.json`](whitebark-pine/observation-photo.json)).
The whole-tree photo says "cannot tell" for needle direction: the needles
are too small to read from there, and a guess would have been a false vote.
It also gives the crown profile, read on tick bands drawn on the crop, with
the lowest band marked estimated where a boulder hides one side.

## 3. Match the render

The first comparison was against the default render, 60 years old. Triage
says "old", and FEIS gives seedlings about 2 cm of growth a year and a
lifespan of 600 to over 1,270 years. That was cause **D0**: no crown change
could make a 60-year render look like these trees. The renders were grown to
200-300 years before anything else was compared.

## 4. Compare

| Feature | Sources say | Photos show | Render shows | | Cause |
| --- | --- | --- | --- | --- | --- |
| age and size | seedlings ~2 cm a year; lives 600 to over 1,270 years (FEIS) | old trees | 60 years | ✗ | D0 |
| stems from the base | "2 to several stems, branching very low"; "often 2-5 trees growing together from bird caches" (Farjon); 6-47% single-stemmed by site (FEIS) | two to three | one | ✗ | D5, became `stems` in the conifer program |
| needle direction | "dense tufts near branchlet ends, connivent, curved forward against shoot" (Farjon) | forward, crowded at the tips | spreading at 58 degrees along the whole shoot | ✗ | D5 (no character), D6 (card had no control) |
| limb hierarchy | "first-order branches short, contorted, persistent; higher orders flexible, assurgent" (Farjon) | a few thick contorted limbs | hundreds of thin limbs | ✗ | D6: no limb self-thinning |
| foliage after 300 years | open-grown trees keep their crowns | full crowns | leaf area fell from 271 to 35 m², limbs sagging | ✗ | D8: `pipe_rings` hand-set low to match a guessed 25 cm trunk at 60 years, so limbs could not hold themselves up |

## 5. Diagnose and generalize

- **Age (D0).** A rule for every long-lived tree, not for whitebark: grow to
  the age the photos show, and check the growth curve passes through both the
  stated seedling growth and the stated mature size.
- **Needles (D5, D6).** The needle-direction finding
  ([`finding.json`](whitebark-pine/finding.json)) named Farjon's words.
  Farjon describes foliage tufted at branch ends for about 50 conifers, so
  the fix is a character at the needle-leaved conifers with two new card
  controls, `lean` and `bare` (draft plantgen#167), not a whitebark setting
  ([`proposal.json`](whitebark-pine/proposal.json)).
- **Stems (D5).** Farjon and FEIS state stem counts, so it became a conifer
  program control read from that statement.
- **Limbs (D6).** Five trials of spec values alone (branch orders, whorl
  vigour, side-shoot life, shedding, stems) did not produce a few thick limbs.
  The next general block is limb self-thinning with a shared resource from
  the base, which would also fix old ponderosa's hanging lower limbs. It is
  engine work, and stays open.

## What the example shows

- The most expensive mistake, the age, was a matching mistake: check it
  first.
- "Cannot tell" from a whole-tree photo kept the needle question honest; the
  close-ups answered it.
- Each fix went to a word in a source, and each word covered many species.
- The one difference with no rule yet is written down as engine work with its
  trials, not hidden by a species value.
