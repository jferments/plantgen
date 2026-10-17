# PlantGen photo rules (v0, 2026-10-17)

Rules that let any vision-capable AI look at plant photos, describe what
they show in a fixed vocabulary, look at PlantGen's renders the same way,
and turn the differences into concrete changes to PlantGen's drawing rules.

You can use any AI: Claude, ChatGPT, Gemini, a local open model, or several.
The rules are plain text; the answers are JSON checked against the schemas
in `schemas/`. Each task file says why its rules are there; the reasons come from
PlantGen's own reviews of its renders against photos and floras.

## What you need

- **Photos** of the species, with their source, licence and date. See
  `SOURCES.md` for where to get good ones and `CAPTURE.md` for taking your
  own. "Solid" means, per species, roughly: 8 whole-plant side views, 8
  shoot close-ups, 10 face-on leaves (plus a few herbarium sheets for size),
  10 flowers face-on and side, 8 fruits, 5 bark shots, from at least three
  different photographers.
- **Renders** of the same species from PlantGen (`plantc`, see
  `03-match-renders.md`), or someone who can make them for you.
- **An AI that reads images** at about 1024 px and can answer in JSON. Run
  `07-calibrate.md` once to learn which features your AI is good at.
- Optional: a measuring toolkit (segmentation masks, a pointing model for
  counts). The rules say where it beats an AI's eye.

## The loop

| Step | File to give your AI | Input | Output |
| --- | --- | --- | --- |
| 1. Triage each photo | `00-ground-rules.md` + `01-triage.md` | one small photo | one `triage` object |
| 2. Observe each usable photo, blind | `00-ground-rules.md` + `02-observe.md` + the features for that view from `features.yaml` | one photo (crops for small organs) | one `observation` object |
| 2b. Check it, named (a separate request, after 2) | `00-ground-rules.md` + `02b-check-named.md` + the species name and its stated facts | the same photo | one `check` object |
| 3. Aggregate | (a script, or a spreadsheet) `04-compare.md` §1 | all observations | `aggregate.json` |
| 4. Render to match | `03-match-renders.md` | the aggregate's views, ages, months | renders + `render-set.json` |
| 5. Observe the renders | the same files as step 2, unchanged | each render | `observation` objects |
| 6. Compare | `04-compare.md` | aggregate + render observations + the species' text facts | `findings.json` + `COMPARE.md` |
| 7. Diagnose | `05-diagnose.md` (needs the PlantGen repository) | findings | findings with cause and owner |
| 8. Generalize | `06-generalize.md` (needs the repository) | diagnosed findings | `proposal` objects, then a PR |

If you have no coding agent, stop after step 6 and hand in `findings.json`
and `COMPARE.md`: someone with the repository can carry on from there.

## Three ways to run it

1. **Chat window.** Start a new chat. Paste `00-ground-rules.md`, then the
   task file. Attach up to about five photos per message, each with its id
   in the message text. Copy the JSON answers into a `.jsonl` file, one
   object per line. Start a fresh chat for each species so earlier answers
   do not leak into later ones.
2. **API.** For each photo, send one request: system text =
   `00-ground-rules.md`; user text = the task file plus the photo id; one
   image. Parse the first JSON object in the answer and validate it against
   the schema; on failure, retry once with the validation error appended.
   Keep the raw answers.
3. **Local models.** The same requests to a local VLM server. Use a
   pointing model for counts above 15 and masks for ratios if you have them
   (`02-observe.md` §6).

Never tell the AI which species it is looking at in steps 2 and 5. Step 2b
is the one place it is told, in a separate request that cannot see step 2's
answers. See `02b-check-named.md` for why both passes are run.

## Files

- `00-ground-rules.md`: the rules every observing AI follows.
- `01-triage.md`: is the photo usable, what does it show.
- `02-observe.md`: the blind pass: how to observe each scale, and how to
  count, measure, read angles and colours.
- `02b-check-named.md`: the named pass: check the stated facts, list
  diagnostic features, measure how much the name sways the model.
- `features.yaml`: the catalogue of features, their states and visual
  definitions, and what each maps to in PlantGen.
- `palette.yaml`: colour families with hex anchors.
- `03-match-renders.md`: making renders comparable to photos.
- `04-compare.md`: aggregating, verdicts, severity, the write-up.
- `05-diagnose.md`: the twelve causes of a difference and how to test each.
- `06-generalize.md`: from a cause to a drawing rule at the right rank.
- `07-calibrate.md`: checking an AI before trusting it.
- `SOURCES.md`: photo sources, licences and access.
- `CAPTURE.md`: how to take reference photos yourself.
- `schemas/`: JSON Schemas for triage, observation, check, aggregate,
  finding, proposal.
- `examples/whitebark-pine.md`: one worked run.
- `tools/check.py`: checks every feature against `crates/plantgen/traits.json`
  and the examples against the schemas (`python3 photo-rules/tools/check.py`
  from the repository root; needs PyYAML and jsonschema).

## Rules that never bend

- Text constrains, photos fine-tune. A photo never overrides a sourced
  statement; a disagreement is a finding.
- Drawing rules are general: library, family or genus. A species-level value
  is the last resort and is recorded as debt with its evidence.
- Every render shown is compared point by point with the species' source
  descriptions first, in writing, from several angles (side, 45 degrees down,
  and a branch or leaf close-up).
- Keep numbers and words, never the photos: each observation carries the
  photo id, creator and licence. Photos stay private.
