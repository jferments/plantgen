# Task: the named pass (check the stated facts)

Every photo gets two passes, always in this order and always in separate
conversations or requests:

| | Pass A: blind (`02-observe.md`) | Pass B: named (this file) |
| --- | --- | --- |
| The model knows | nothing about the plant | the species name and its stated facts |
| Its job | measure: answer the fixed features from the pixels | check: does this photo show each stated fact; what else here is diagnostic |
| Its answers are used as | votes in the aggregate | a second opinion on conflicts, new features to add, and a measure of how much the name sways the model |
| Never | told the name or the facts | allowed to see or change pass A's answers |

Why both: a blind observer measures without bias but does not know what
matters for this plant, and misses features nobody thought to list. A named
observer knows what to look for, but tends to see what the book says. Pass A
gives the numbers; pass B points at what the numbers missed.

## What to give the model

- `00-ground-rules.md`, with this change: rule 1 does not apply. You are
  told the species; you must still answer only from what this image shows.
- The species' scientific name.
- The stated facts for the features this photo's views can answer: for
  each, the character, its stated value, and the source's words in brief
  (`plantc traits <id>` lists them for a PlantGen species).
- The image, at the same size as pass A.

## Answer format

```json
{
  "photo_id": "<as given>",
  "identity": "consistent | inconsistent | cannot tell",
  "identity_note": "<if inconsistent: what in the image does not fit>",
  "checks": [
    {
      "character": "<as given>",
      "stated": "<as given>",
      "photo": "shows | contradicts | partly | cannot tell",
      "evidence": "<where in the image>",
      "note": "<what it shows instead, if it contradicts>"
    }
  ],
  "diagnostic": [
    {
      "what": "<a feature visible here that sets this plant apart and is not in the stated facts>",
      "evidence": "<where in the image>",
      "drawing_matters": "high | medium | low"
    }
  ]
}
```

## Rules for the named pass

1. **The image decides.** If the stated fact says "leaves serrate" and the
   leaves in this photo are smooth, answer `contradicts`, however sure the
   source sounds. Juvenile or atypical organs: `cannot tell`, with a note.
2. **`shows` needs evidence.** Name the part of the image. A fact you
   cannot see is `cannot tell`, not `shows`.
3. **Diagnostic features are things you can see.** List what a botanist
   would point at to tell this plant from its relatives, only if it is
   visible in this image. Rate how much it would matter to a drawing seen at
   normal distance.
4. **No measuring.** Numbers belong to pass A. Don't give counts, ratios or
   colours here except to explain a contradiction.

## How the two passes are used

- **Conflicts.** Where pass A's aggregate disagrees with a stated fact
  (`04-compare.md` §3), pass B's `contradicts` and `shows` counts on the
  same photos are the second opinion. A conflict both passes agree on goes
  to diagnosis as D9 or D10; one only pass A sees stays a conflict to check
  by hand.
- **New features.** `diagnostic` items that recur across photos (three or
  more photographers) and are not in `features.yaml` become proposed
  features, and often proposed characters (cause D5).
- **Suggestibility.** For each feature both passes cover, compare how often
  pass A's answer equals the stated fact with how often pass B says `shows`.
  A large gap means the name sways this model: its named-pass answers are
  weighted down, and the gap is recorded in its calibration table
  (`07-calibrate.md`).
- **Identity.** Photos with `identity: inconsistent` from pass B are
  dropped from pass A's aggregate too.
