# Describe one image in words (for search)

Use this with `00-ground-rules.md`. The answer is one `description` object
(`schemas/description.schema.json`).

Triage (`01-triage.md`) and observation (`02-observe.md`) answer fixed
questions. This task writes down what the image shows in plain words, so
people can later find plants by how they look: "lacy leaves like this
one", "a drooping cluster of small white bells", "bark in long papery
strips". The words are stored with the image and searched alongside its
image vectors. Write for a reader who will never see the photo.

## What to write

1. **`overall`**: one or two sentences: what the image shows (whole plant,
   a shoot, a leaf, a flower, a fruit, bark, a herbarium sheet, a drawing),
   from which side, and at what stage (in flower, in fruit, leafless,
   seedling) if that can be seen.
2. **`organs`**: one entry for each organ that shows clearly enough to
   describe, the most prominent first. For each:
   - `organ`: one of the listed organ words;
   - `text`: two to four sentences on shape, division, edge, surface,
     colour, size relative to its neighbours, arrangement and posture,
     in the vocabulary of `features.yaml` where a word fits (pinnate,
     palmately lobed, serrate, whorled, nodding, umbel...). Say what makes
     it look the way it does: "leaves twice divided into many narrow
     segments, so the foliage reads as fine and airy";
   - `look`: up to six everyday words a person might search with (lacy,
     feathery, ferny, strappy, glossy, leathery, woolly, papery, waxy,
     spiky, drooping, star-shaped, bell-shaped, button-like, plume-like,
     airy, dense). Use one only when it plainly fits.
3. **`tags`**: up to twenty `feature=state` pairs from `features.yaml`,
   only where the image settles the answer without doubt (for example
   `leaf.division=compound`, `flower.symmetry=actinomorphic`). Leave a feature
   out rather than guess; this is not the observation pass.
4. **`not_shown`**: organs a reader might expect but the image does not
   show (for example "no flowers or fruit", "leaf undersides not
   visible").

## Rules

- **Never name the plant**, its family or genus, and never write what the
  plant "is known for". Describe only what this image shows. The same
  plant in another photo may look different, and that is the point.
- **Describe, do not judge.** No "beautiful", "healthy", "typical".
- **Colour words** come from `palette.yaml` families (a hex is not
  needed here). Say when light or a filter may be shifting a colour.
- **Renders and drawings** are described the same way; do not mention
  that an image is a render or a drawing unless the triage `kind` says so
  and the person running the rules asks for it.
- **Answer with one JSON object** that validates against the schema, and
  nothing else.
