# Task: observe one image

Give this task with `00-ground-rules.md` and the features to answer: the
entries of `features.yaml` whose `views` include one of the image's views
from triage. Send the whole entry (id, question, states with their
definitions, notes), not just the id: the definitions are what make two
models' answers comparable.

Use the image at 1024-1536 px. If the organs you are asked about are small in
the frame, crop them first (or tile the image into 2 x 2 crops) and send the
crops with the same photo id: models miss detail in a downscaled frame.

Renders are observed with this same task, unchanged. Never tell the model
which image is a render or what it was meant to show.

## Answer format

```json
{
  "photo_id": "<as given>",
  "description": "<2-4 sentences, ground rule 2>",
  "answers": {
    "<feature id>": {
      "value": "<a state | a number | a bin | [points] | {colour}>",
      "confidence": "high | medium | low",
      "evidence": "<where in the image>",
      "note": "<optional>"
    },
    "<feature id>": {"value": "cannot tell"}
  },
  "unlisted": ["<optional: something important no feature covers>"]
}
```

A colour value is `{"family": "maroon", "hex": "#6e2330", "lighting":
"overcast"}`. A count you pointed at is `{"points": [[x, y], ...], "count":
N}`. A profile (`habit.crown_profile`) is its ten widths, lowest band first,
with the bands you estimated listed by number:

```json
"habit.crown_profile": {
  "value": [0.0, 0.12, 0.31, 0.42, 0.44, 0.40, 0.33, 0.24, 0.15, 0.05],
  "estimated": [1],
  "method": "bands",
  "confidence": "medium",
  "evidence": "whole tree, ground to top; ticks drawn on the crop",
  "note": "grass hides the left side below 0.2 H"
}
```

## Cases the ground rules leave open

Observers of the pawpaw trial (2026-10-18) split on these; answer them so.

- **Lighting of a colour:** name what the light looks like, whatever made
  the image. Hard-edged shadows and lit faces are `"direct sun"`, even on a
  plain sky. `"studio"` is soft, even light on a plain backdrop with no
  cast shadows.
- **An organ that is missing:** if the plant has the organ but not now
  (no flowers in winter, fruit not yet set, leaves fallen), answer
  `"cannot tell"`. Use `"not applicable"` only for a part this kind of
  plant never has (ground rule 3).
- **Flowers borne singly:** `infl.type` is `solitary`. `infl.posture` is
  read from the flower's own stalk. `infl.density`, `infl.open_share` and
  `infl.length_over_height` are `"not applicable"`.

## How to observe, scale by scale

Work from the largest scale to the smallest, finishing one before starting
the next. That stops a striking flower from colouring your reading of the
crown.

### 1. Habit (view V1)

Look at the outline first, as if the plant were a silhouette against the
sky. Where is it widest? How wide is it compared with its height? Where do
the leaves start above the ground? Is it one stem or several from the base?
If you are asked for the crown profile, read it as "Reading the crown
profile" below says, on the tick bands, not by eye from the whole frame.
Then the setting: an open-grown tree and a forest tree of the same species
look different, and the comparison needs to know which this is.

### 2. Architecture (V2, or V1 where the branches show)

Follow the trunk up. Does it run to the top (one leader), or split into
several limbs of similar size? Count the main limbs you can follow from the
trunk. Read the angle each makes as it leaves the trunk and where its tip
points. Are there a few thick limbs and many thin ones, or many limbs of the
same thickness? Do the limbs come off in tiers or whorls?

### 3. Foliage mass (V1, V2)

Where does the foliage sit: as an even skin over the whole crown, in separate
clumps with sky between them, in tufts at the ends of bare limbs, along the
limbs, or in flat layers? Estimate how much sky shows through the crown.
Name the colour of the foliage mass from a lit part of the crown.

### 4. Shoot (V4)

Pick one or two typical shoots in focus. How are leaves placed: one per
node, two opposite, several in a ring? Are they evenly spaced along the
shoot or crowded toward the tip? Which way do they point relative to the
shoot? How long is the bare stem between leaves compared with a leaf? For
needles: in bundles of how many, pointing forward, sideways, or all round
the twig, or in two flat rows?

### 5. Leaf (V5)

Choose a flat, face-on, mature leaf. Read the outline in this order: simple
or divided; lobed or not; overall shape and where it is widest; tip; base;
edge. Then the veins, the stalk, and the two faces' colours if both show.
For divided leaves, read the leaflet the same way, and count the leaflets.

### 6. Flower and inflorescence (V6, V7)

Face-on (V6): colour of each part, symmetry, number of petals or tepals,
their shape and tips, whether neighbours overlap or have gaps. For a daisy
head, the ray tips, ray sides, disc shape and colour. Then the middle: the
pistils or carpels (count them, or bin them), and whether a ring of hairs
sits between the perianth and the stamens. From the side (V7): the
corolla's shape (flat, cup, bell, funnel, tube), how the outer parts bend at
their tips, whether the inner whorl is as long as the outer, which way the
flower faces, and the form and posture of the whole cluster. On a bud or
the tube below the petals, look for scales, wool or spines.

### 7. Fruit and cone (V8)

Shape, colour, which way it hangs, whether single, clustered or in chains,
the surface, what stays on its top (sepals, a dried flower, nothing), and its
size compared with a leaf.

### 8. Surface (V8, or any close view)

Bark texture and colour on the trunk; twig colour; for cacti, the ribs, the
bumps (tubercles) along them, the spacing of the spine clusters, how much of
the skin the spines hide, and spine colour and shape.

Cacti need close-ups of their own (`SOURCES.md`): a cactus's look at arm's
length is in its areoles and its top, which a whole-plant photo shows as a
blur. On a close-up of the ribs, pick two or three grown areoles from the
middle of the stem and read, in this order:

1. the ribs: straight, winding gradually round the stem, or wavy; and
   where the rib count changes, whether one rib forks at a time or many end
   together along a line;
2. the areole: its length along the rib against the distance to the next
   one, and its wool (none, a thin felt, a cushion, a tuft) and the wool's
   colour;
3. the spines: radials counted round the edge, centrals counted in the
   middle, how the centrals are set (a cross of four, one main spine, no
   pattern), and the longest central's shape;
4. near the apex only, any short, thick, blunt gland spines above the true
   spines, often yellow or red, sometimes with a drop of nectar or ants.

On a view of the top (from above, or a close-up of it), say what covers the
growing point: bare skin or a small dot of felt, a small felted cap, or a
thick thatch of wool and young spines.

## Is it face-on?

Features marked `face_on: true` (shapes, ratios, tips and bases of leaves,
leaflets and petals) are answered only on an organ lying roughly parallel
to the image plane. Tilt shortens the organ across the tilt and nothing
else: a blade tilted 25 degrees reads a tenth narrower (or shorter), 35
degrees nearly a fifth, so a tilted leaf looks narrower and pointier than it
is. Before answering, check the organ you chose:

- the midrib (or a petal's middle line) is straight in the image, not bent
  by perspective, and the blade's two halves are about equally wide on
  either side of it (for a leaf that is symmetric);
- you see one face only, evenly lit, with neither edge showing its
  thickness or a strip of the other face;
- its outline is sharp all round, with no end much blurrier or larger
  than the other (a sign it points toward the camera);
- on a herbarium sheet or a leaf laid on a table, it lies flat and is not
  folded or curled.

If no organ in the image passes, answer "cannot tell": a ratio from a
tilted organ is a wrong vote, not a rough one. A round flower that looks
like an ellipse is tilted too.

## How to measure by eye

- **Ratios.** Pick the two lengths on the same organ, along the axes the
  feature names (length: base of blade to tip, not including the stalk;
  width: across the widest part, at right angles to the length). Compare
  them as fractions. Round to 0.05.
- **"Where is it widest".** Divide the organ's length into thirds from the
  base. Answer `lower third`, `middle`, `upper third`.
- **Angles.** Imagine a vertical line through the point where the part
  leaves its parent. Read the angle between the vertical and the part's first
  straight stretch. 0 is up, 90 is level, 180 is down.
- **Shares (sky through the crown, skin hidden by spines).** Picture the
  crown outline filled in: what fraction of it shows sky? Answer in tenths.
- **Counts.** Follow ground rule 6. If the organ repeats (several flowers),
  count two or three of them and give the commonest number.

## Reading the crown profile

`habit.crown_profile` gives the crown's width at ten heights. Read it on a
crop with the bands drawn on, never by eye from the whole frame: in the
Conifers review, the words "conical" and "columnar" missed that drawn crowns
were twice as wide as real ones in their top 40%, and the ten numbers found
it.

Steps 1 and 2 are for whoever prepares the images (a person, or an agent
that can run a script): the banded crop is then sent with the photo, under
the same photo id. An observer sent a photo with no bands drawn reads the
widths against the plant's height by eye, answers with confidence `low` and
`"method": "vlm"`, and writes "no bands" in the note.

1. **Set the frame.** Find the ground point, where the trunk (or the clump's
   middle) meets the ground, and the top, the plant's highest point. On
   sloping ground, take the ground halfway between where the trunk enters
   it uphill and downhill. Draw a percentage grid
   on the photo to read both (`photo-rules/tools/crown_profile.py grid
   PHOTO --out grid.png`) and write them as per cent of the image's width
   and height from its top left.
2. **Draw the bands.** `crown_profile.py bands PHOTO --ground X,Y --top X,Y
   --out bands.png` crops the plant and draws ten bands, each a tenth of the
   height, centred on 0.05, 0.15 ... 0.95 of it, with ticks every 0.02 of
   the height out from the trunk line on both sides, labelled every 0.1. Any
   tool that draws the same will do (it is a few lines of Pillow: crop,
   then lines and ticks at fixed shares of the ground-to-top distance).
3. **Read each band.** Where does the outer foliage edge cross the band on
   the left, and on the right, in ticks from the trunk line? Leave out a
   single twig sticking out alone. The band's width is the two readings
   added. 0 where only bare trunk crosses it (below the crown base).
4. **Mark what you estimated.** Where grass, rocks, a neighbour or the frame
   hides one side of a band, read the side you can see, double it, and list
   that band in `estimated`. A band hidden on both sides is estimated from
   its neighbours, or, if more than two are, answer "cannot tell".

Read so (`"method": "bands"`), each width is good to about ±0.03-0.05 of
the height; estimated ones are worse. Photos taken from near the foot shorten the top (the upper
widths read a little wide): say so in the note when the camera looks up.
A render is read the same way on its side view, or measured from its
geometry (`04-compare.md` §1).

## When to hand a feature to a tool

If your platform has them, these tools beat any model's eye:

| Job | Tool | Why |
| --- | --- | --- |
| Counts above 15 | a pointing model (Molmo2, or a model that returns point coordinates); count the points | models guess large counts; the pilot over-counted rays by a third |
| Ratios and widest points | a detector + segmentation mask (Grounding DINO + SAM 2.1), then OpenCV ellipse and medial-axis fits | sub-pixel lengths, the same ruler for photos and renders |
| Sky through the crown | a mask of the crown outline and of sky pixels inside it | a fraction of pixels, not an impression |
| Colour | the mean CIELAB of masked organ pixels after white balance | removes guesswork about tint |
| Absolute sizes | herbarium sheets with a ruler | photos without a scale give only ratios |

Record which method answered each feature in the answer's `"method"` field
(`"vlm"`, `"points"`, `"mask"`, `"pixels"` for a colour sampled from pixels,
`"bands"` for a profile read on drawn tick bands, `"ruler"`, `"hand"`), so
aggregation can weigh them.
