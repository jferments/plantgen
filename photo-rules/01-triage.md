# Task: triage one image

Give this task with `00-ground-rules.md`. Use a small image (about 512 px on
the long side) and a fast model if you have the choice. The answer decides
whether the image is observed in full, and which features it may answer.

For triage only, you may be told the species, to answer `"species_match"`.
Do not use that knowledge for anything else.

## Answer format

```json
{
  "photo_id": "<as given>",
  "description": "<2-4 sentences, ground rule 2>",
  "usable": true,
  "unusable_reason": null,
  "species_match": "yes | no | unsure",
  "kind": "photo | herbarium_sheet | drawing | render (optional; see below)",
  "views": ["V1", "V4"],
  "organs_visible": ["leaf", "flower"],
  "leaf_face": "upper | lower | both | none | cannot tell",
  "stage": "vegetative | bud | flowering | fruiting | leafless | mixed | cannot tell",
  "leaf_state": "none | flushing | full | autumn_colour | leafless | cannot tell",
  "setting": "open_grown | forest | garden_or_street | pot_or_indoor | pruned_or_hedge | cannot tell",
  "cultivar_look": "yes | no | cannot tell",
  "age_class": "seedling | juvenile | young | mature | old | cannot tell",
  "scale_cue": "none | ruler | colour_card | hand | person | pole | other",
  "lighting": "direct sun | shade | overcast | dim | flash | studio | render",
  "quality": "sharp | soft | blurred"
}
```

## How to answer

- **usable**: `false` if the plant is tiny in the frame, blurred, mostly
  hidden, heavily filtered or edited, a collage of different plants or
  photos, not a plant, or clear but showing no view in the table below
  (reason `"no view fits"`). Give the reason in `"unusable_reason"`. A sheet
  of panels that each show one organ of the same plant (a herbarium sheet,
  PlantGen's `plantc parts` sheet) is not a collage: the person running the
  rules cuts it into its panels, and each panel is triaged on its own.
- **kind**: what the image is. When the person running the rules already
  knows (they made the renders, they fetched the herbarium sheets), they
  record it themselves and leave this out of the request; an observer that
  is not asked does not guess.
- **views**: every view code this image shows clearly enough to measure:

  | Code | The image shows |
  | --- | --- |
  | V1 | the whole plant from the side, base and top in frame (from eye level; from below or above, say so in the description) |
  | V2 | the whole plant with its branches visible (leafless, or a see-through crown) |
  | V3 | the whole plant from 30-60 degrees above (under 30 degrees, it is V1) |
  | V4 | a branch or shoot, up to 2 m of it, with its leaves (or needles, buds, flowers) in place |
  | V5 | one or more leaves face-on and flat (herbarium sheets usually) |
  | V6 | one or more flowers or flower heads face-on |
  | V7 | flowers or an inflorescence from the side |
  | V8 | a close-up of fruit, cones, bark, or the stem surface (a cactus's ribs and areoles, its top) |

  A species is photographed to a target number of images per view
  (`SOURCES.md`), and these views are what `imagekit views` counts.
- **organs_visible**: from `plant, stem, bark, leaf, leaflet, shoot, flower,
  inflorescence, fruit, cone, seed, spine, areole, apex, root`. `apex` is
  the growing top of a stem seen close enough to tell what covers it: a
  cactus's crown with its wool and youngest spines, a rosette's heart.
- **leaf_face**: which face of the leaves shows clearly enough to read its
  colour and hairs: `upper`, `lower` (the underside: a leaf turned over, or
  a shoot seen from below), `both`, or `none` (no leaves in frame). Leaf
  undersides are a target of their own: felted and coloured undersides
  were missed in v1 for want of photos of them.
- **age_class**: for trees and shrubs, judge by trunk thickness, bark, crown
  shape and any scale cue: `seedling` (under knee height), `juvenile`
  (no flowers or cones, slim stem), `young` (adult form, still slim,
  regular), `mature` (full size, full crown), `old` (thick trunk, broken or
  heavy limbs, flattened or irregular top). For herbs, `young` or `mature`.
- **setting**: `open_grown` means the plant stands alone with light on all
  sides (a field, a lone tree, open desert); `forest` means crowded by other
  plants.
- **cultivar_look**: `yes` for double flowers, variegated or unusually
  coloured leaves, clipped shapes, or rows of identical plants in a
  nursery.
