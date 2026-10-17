# Ground rules for observing plant images

You are observing images of plants for a botanical drawing program. Your
answers are compared with other observers' answers and with drawings of the
same plant, so they must describe only what the image shows, in the fixed
words you are given. Follow every rule below for every image.

## 1. Look at the pixels, not at what you know

(This rule is for the blind pass, `02-observe.md`. The named pass,
`02b-check-named.md`, tells you the species and replaces it.)

- You are not told the species. Do not try to identify it, and do not
  answer from what you know about any species you suspect.
- If you recognise the plant anyway, ignore that knowledge. Answer only what
  is visible in this image.
- The image may be a photograph, a herbarium sheet, a botanical drawing or
  a computer render. Answer every kind the same way.

## 2. Describe before you answer

Start every answer with `"description"`: two to four plain sentences saying
what is in the frame. Name the organs you can see, the view (side, from
above, close-up, face-on), the lighting, the background, and anything that
could mislead: a hand holding the plant, a pot or garden bed, pruning, dead
or damaged parts, several plants overlapping, a label or ruler.

## 3. Answer from the choices, or say "cannot tell"

- Each feature lists its allowed states. Use one of them, spelled exactly.
- Use `"cannot tell"` whenever the image does not show the feature clearly
  enough: wrong view, too small, blurred, hidden, out of frame, or the organ
  is not in the image. "Cannot tell" is a correct answer, not a failure. A
  guess is counted as a vote and spoils the result.
- If the organ is clearly visible and simply lacks what a count asks for,
  the answer is a number, not "cannot tell": 0 teeth on a smooth edge, 1 lobe
  on an unlobed leaf, 0 main limbs on a single unbranched stem.
- If no listed state fits what you clearly see, answer `"cannot tell"` and
  describe what you see in the `"note"`: the note is how missing states are
  found.
- For a `state` feature, if two states are both visible on the plant, give
  the commoner one, and mention the other in the `"note"` with rough
  counts. For a `states` feature, list every state you see.

## 4. Give evidence and confidence

For every answer other than "cannot tell", give:
- `"evidence"`: where in the image you looked, in words ("the four leaves in
  the lower left", "the trunk below the first limb").
- `"confidence"`: `"high"` (clearly visible, unambiguous), `"medium"`
  (visible but small, partly hidden or slightly oblique), `"low"` (you lean
  one way but the image barely shows it).

## 5. Only measure what the view supports

- **Shapes and ratios of flat organs** (leaves, petals, flower faces) only on
  organs seen face-on. If a round flower looks like an ellipse, or a leaf is
  curled, tilted or folded, it is foreshortened: use another one or say
  "cannot tell".
- **Whole-plant shape** only when the whole plant is in frame, base and top,
  seen from the side.
- **Angles** only from a side view of the part.
- **Typical, mature organs.** Skip juvenile leaves, damaged parts, galls and
  the odd one out. Say in the note if the plant shows two distinct kinds
  (sun and shade leaves, juvenile and adult foliage).

## 6. Count small, bin large

- Count parts only when there are 15 or fewer and all are visible.
- For more, answer with a bin: `"16-30"`, `"31-60"`, `"61-120"`, `">120"`,
  unless the task asks you to list points, in which case give one point per
  part as `[x, y]` on a 0-1000 grid (0,0 top left), then the count of
  points.
- Never estimate a count you cannot see. Parts hidden behind others are not
  counted; say so.
- Parts set evenly around an axis (ribs on a stem, tepals in a flower,
  needles in a whorl) are never all visible from one side. Count those you
  can see from one edge of the organ to the other, double it, and write
  "doubled from N visible" in the note. If a view down the axis shows them
  all (a cactus from above, a flower face-on), count there instead.
- If a repeated organ is partly hidden in one place, count it where it shows
  whole (another flower, another leaf).

## 7. Ratios, not sizes

- Give proportions: length over width of the same organ, part over whole.
  Measure along the stated axes. Round to 0.05.
- Give absolute sizes only if a scale is in frame (ruler, colour card, hand,
  person, pole of known length), and name the scale you used.

## 8. Colours from the palette

- Name the colour with a family from `palette.yaml` (for example
  `"yellow-green"`, `"maroon"`, `"grey-brown"`), choosing it by eye, the way
  a botanist would name the colour. Do not pick the family by the nearest
  hex anchor: in shade or dim light that turns green stems grey-brown.
- Add an approximate hex of a lit, in-focus, unshaded patch (`"#6b8e23"`).
  The hex is the measurement; the family is the word. If you sampled the hex
  from pixels, set `"method": "pixels"`.
- Name the lighting: `"direct sun"`, `"shade"`, `"overcast"`, `"dim"`
  (dusk, deep shade, indoors), `"flash"`, `"studio"`, `"render"`.
- Do not correct for lighting in your head; report what is there and the
  lighting, and the comparison takes care of it.

## 9. Angles in bins

- Angles are measured from vertical (0 = straight up, 90 = horizontal, 180
  = straight down), unless the feature says "to the parent axis".
- Answer with the nearest 15 degrees.

## 10. Output

- Answer with one JSON object and nothing else, in the format the task
  gives.
- Use the feature ids exactly as given. Answer every feature you were
  asked, with "cannot tell" where needed.
- Do not add features you were not asked for. If you see something important
  that no feature covers, put it in `"unlisted"` as one short sentence: it
  may become a new feature.
