# Task: check your AI before trusting it

Vision models differ a lot from feature to feature. In PlantGen's pilot on
daisy and camas photos, one model matched 8 of 10 hand answers on daisies
and another 4 of 10, and small models missed camas's one-tepal-set-apart
symmetry almost every time. Run this once per model (and again when the
model or the rules change). It tells you which features your model's answers
can count for.

## 1. The calibration set

Two kinds of image with known answers:

- **Photos with hand answers.** A person who knows the terms answers the
  features for each photo, without an AI. PlantGen's first key is the
  daisy and camas key (16 hand answers on oxeye daisy and camas photos,
  kept privately with the image pilot); it is to grow to about 200 photos covering
  the eight scales, kept as photo ids and answers only.
- **Renders with exact answers.** `plantc organs` gives exact counts; the
  spec gives the states the render was drawn from (`plantc spec`). These
  also show whether your model reads renders the way it reads photos.

## 2. Run it

Run triage and observation on the calibration set exactly as in a real run:
the same ground rules, task files, image sizes and crops. Never show the
model the answers.

## 3. Score per feature

| Type | Score | Trusted when |
| --- | --- | --- |
| state, ordinal | share of answers equal to the key (ordinal: within one step counts half) | 80% or more, with at least 10 answered |
| ratio, share | median absolute error as a share of the key's value | under 20% |
| angle | median absolute error | under 20 degrees |
| count | share within ±1 (up to 15), or right bin | 80% or more |
| colour | median ΔE2000 from the key's measured colour | under 12 |
| "cannot tell" rate | share of features the key answers but the model refused | under 30%, or the feature is not trusted |
| false answers | share of features the key marks as not visible but the model answered | under 10% |

A feature that misses its threshold is **not trusted** for that model.

## 4. Publish the table

Write `calibration-<model>-<date>.json`: the model's name and version, the
rules version, the settings (image size, effort or temperature), and per
feature: n, score, trusted or not. Every run's metadata names the
calibration table it used, and `04-compare.md` drops untrusted answers.

## 5. Mixing models

Use each model for the features it is trusted on. A cheap model can triage
and answer the gross features; a stronger one the subtle shapes and
symmetry; a pointing model the counts; masks the ratios and colours. Two
trusted models that disagree on a photo send that photo to a person or a
third model.
