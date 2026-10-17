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
N}`.

## How to observe, scale by scale

Work from the largest scale to the smallest, finishing one before starting
the next. That stops a striking flower from colouring your reading of the
crown.

### 1. Habit (view V1)

Look at the outline first, as if the plant were a silhouette against the
sky. Where is it widest? How wide is it compared with its height? Where do
the leaves start above the ground? Is it one stem or several from the base?
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
head, the ray tips, ray sides, disc shape and colour. From the side (V7): the
corolla's shape (flat, cup, bell, funnel, tube), which way the flower faces,
and the form and posture of the whole cluster.

### 7. Fruit and cone (V8)

Shape, colour, which way it hangs, whether single, clustered or in chains,
the surface, and its size compared with a leaf.

### 8. Surface (V8, or any close view)

Bark texture and colour on the trunk; twig colour; for cacti, the ribs, the
bumps (tubercles) along them, the spacing of the spine clusters, how much of
the skin the spines hide, and spine colour and shape.

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

## When to hand a feature to a tool

If your platform has them, these tools beat any model's eye:

| Job | Tool | Why |
| --- | --- | --- |
| Counts above 15 | a pointing model (Molmo2, or a model that returns point coordinates); count the points | models guess large counts; the pilot over-counted rays by a third |
| Ratios and widest points | a detector + segmentation mask (Grounding DINO + SAM 2.1), then OpenCV ellipse and medial-axis fits | sub-pixel lengths, the same ruler for photos and renders |
| Sky through the crown | a mask of the crown outline and of sky pixels inside it | a fraction of pixels, not an impression |
| Colour | the mean CIELAB of masked organ pixels after white balance | removes guesswork about tint |
| Absolute sizes | herbarium sheets with a ruler | photos without a scale give only ratios |

Record which method answered each feature (`"method": "vlm" | "points" |
"mask" | "ruler"`) in the answer's note, so aggregation can weigh them.
