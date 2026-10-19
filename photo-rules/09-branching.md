# Task: read a woody plant's branching

Use this with `00-ground-rules.md`. The answer is one `branching` object
(`schemas/branching.schema.json`). Send it for usable photos and renders of
trees and shrubs whose triage views include V1, V2, V3, V4 or V8, with
those views named in the request.

Floras and winter keys state these characters in words: "terminal bud
absent", "twigs zigzag", "branches horizontal", "leader soon lost". Photos
show them directly. Each answer is a tally mark for the plant's genus or
species: many photos together back or challenge a trait value, and no
single photo decides one. So answer only what this image shows, and say
`cannot tell` when it does not.

## Answer format

```json
{
  "photo_id": "<as given>",
  "description": "<1-3 sentences: what part of the plant shows, leafy or bare, from where>",
  "answers": {
    "<character>": {
      "value": "<a state | a number>",
      "confidence": "high | medium | low",
      "evidence": "<where in the image>"
    },
    "<character>": {"value": "cannot tell"},
    "<character>": {"value": "not applicable"}
  }
}
```

Answer every character below. A character the image's views cannot show
(a terminal bud in a whole-tree photo) is `not applicable`. One it could
show but does not show clearly (hidden by leaves, too far, blurred) is
`cannot tell`. Never guess from what the species usually does: you are not
told the species, and a guess makes the tally worthless.

## The characters

Work from the whole plant down to the twig, as in `02-observe.md`.

### Whole plant (V1, V2, V3)

- **leader**: what became of the main stem in the crown.
  - `kept`: one stem runs through to the top, clearly thicker than every
    branch off it (excurrent: spruce, sweetgum, young oaks).
  - `lost`: the stem divides into several limbs of about equal thickness,
    none carrying on as the leader (decurrent: old oaks, elms, apple).
  - `multiple`: two or more stems from at or near the ground (a shrub, a
    coppiced or multi-stemmed tree).
- **limb_habit**: the main limbs' course from trunk to tip.
  - `ascending`: they rise steeply, under about 45 degrees from vertical,
    all the way.
  - `spreading`: they leave the trunk at a wide angle and run out roughly
    level.
  - `arching`: they rise, then bow outward and down along their outer part.
  - `drooping`: the outer branches and twigs hang down (weeping).
  - `tortuous`: they twist and change direction often, with no single
    course.
- **reiteration**: upright shoots sprouting from the trunk or the inner
  parts of old limbs (water sprouts, epicormic shoots), each like a small
  tree of its own: `none`, `few` (one to three), `many`.
- **dead_limbs**: dead branches in the crown (bare, broken or bark-less
  wood inside a leafy crown; stubs on the trunk): `none`, `few`, `many`.
- **leaf_display** (leafy crowns only, V1 or V3): where the leaves sit in
  the crown's depth.
  - `shell`: in a dense outer layer; inside the crown is dark and bare.
  - `throughout`: spread through the depth; inner leaves and branches can be
    seen lit between the outer ones.
- **gap_pattern** (leafy crowns, V1): the sky or background showing through
  the crown.
  - `closed`: almost none.
  - `many_small`: many small holes, evenly spread.
  - `few_large`: a few large holes or deep notches between clumps of
    leaves.
- **clear_bole**: the height of the lowest living limb as a share of the
  plant's height, 0 to 1 (0 when leaves reach the ground). Only with base
  and top in frame.

### Limbs (V2: bare or see-through crowns)

- **limb_angle**: the typical angle of the main limbs from vertical where
  they leave the trunk or leader, in degrees (0 upright, 90 level, over 90
  pointing down). Read it over the first metre or so of each limb, not at
  its tips, and give the middle value of the limbs you can see.
- **scaffold_limbs**: how many main limbs leave the trunk or leader and
  each carry a large part of the crown (count the ones in view; a number).
- **limb_course**: how a main limb runs between its forks.
  - `straight`: nearly straight from fork to fork.
  - `wavy`: gently curving, bending one way and then the other.
  - `zigzag`: changing direction sharply at each fork or bud, like a row of
    short segments (sympodial growth).

### Branches and twigs (V2, V4, V8)

- **branch_orientation**: how the side branches are built.
  - `orthotropic`: they grow upward like small trunks, with leaves or buds
    all round.
  - `plagiotropic`: they lie flat, in sprays or fans, with leaves in two
    ranks along them (beech, elm, hornbeam).
  - `mixed`: upright near the trunk, flattening into sprays toward the
    tips.
- **twig_course**: the past year's twigs: `straight` or `zigzag` (bending
  at each bud).
- **terminal_bud** (V4 or V8, a bare twig tip or a shoot tip close enough
  to see the buds):
  - `present`: the twig ends in a bud that is plainly the tip's own,
    usually the largest bud, set squarely on the end.
  - `absent`: the tip has died back. The topmost bud sits to one side of
    the end, beside a small scar or a dried stub.
- **twig_thickness**: the past year's twigs against their buds and leaf
  scars (or leaf stalks).
  - `stout`: thick, blunt twigs with large buds and big leaf scars, few
    side twigs (ash, walnut, sumac).
  - `moderate`: in between.
  - `slender`: thin, whippy twigs with small buds, many side twigs (birch,
    willow, hornbeam).

## Renders

Renders are read with this same task, unchanged. Never tell the model which
image is a render.
