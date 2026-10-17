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
  "kind": "photo | herbarium_sheet | drawing | render",
  "views": ["V1", "V4"],
  "organs_visible": ["leaf", "flower"],
  "stage": "vegetative | bud | flowering | fruiting | leafless | mixed",
  "leaf_state": "none | flushing | full | autumn_colour | leafless | cannot tell",
  "setting": "open_grown | forest | garden_or_street | pot_or_indoor | pruned_or_hedge | cannot tell",
  "cultivar_look": "yes | no | cannot tell",
  "age_class": "seedling | juvenile | young | mature | old | cannot tell",
  "scale_cue": "none | ruler | colour_card | hand | person | pole | other",
  "lighting": "direct sun | shade | overcast | flash | studio | render",
  "quality": "sharp | soft | blurred"
}
```

## How to answer

- **usable**: `false` if the plant is tiny in the frame, blurred, mostly
  hidden, heavily filtered or edited, a collage, or not a plant. Give the
  reason in `"unusable_reason"`.
- **views**: every view code this image shows clearly enough to measure:

  | Code | The image shows |
  | --- | --- |
  | V1 | the whole plant from the side at about eye level, base and top in frame |
  | V2 | the whole plant with its branches visible (leafless, or a see-through crown) |
  | V3 | the whole plant from above or 30-60 degrees down |
  | V4 | a branch or shoot, 0.3-2 m of it, with its leaves in place |
  | V5 | one or more leaves face-on and flat (herbarium sheets usually) |
  | V6 | one or more flowers or flower heads face-on |
  | V7 | flowers or an inflorescence from the side |
  | V8 | a close-up of fruit, cones, bark, or the stem surface |

- **organs_visible**: from `plant, stem, bark, leaf, leaflet, shoot, flower,
  inflorescence, fruit, cone, seed, spine, areole, root`.
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
