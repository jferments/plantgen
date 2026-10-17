# Task: render to match the photos

A render can only be compared with photos that show the same thing. In
PlantGen's own reviews more time was lost to mismatched renders than to any
drawing fault: a 60-year whitebark pine against photos of trees centuries
old; fruit "not drawn on the render day" while half the photos showed fruit.
Do this step before any comparison.

## 1. Read the photo set's conditions from triage

From `triage.jsonl` and the photo metadata, per view:
- the commonest **age class**, and any height or trunk diameter a scale cue
  gave;
- the **months** (from the photo date; flip by six months for the southern
  hemisphere) and the commonest **stage** (bud, flowering, fruiting,
  leafless);
- the commonest **setting** (open-grown, forest, garden);
- the **views** and roughly how far away the photographer stood.

If the photos split (half in flower, half in fruit), make renders for each
group and compare each with its own photos.

## 2. Choose the render conditions

| Photos | PlantGen render | Command option |
| --- | --- | --- |
| Age class | the age whose height and trunk match: read `plantc grow <id> --years N` keyframes and the spec's stated growth; old trees need their real age, not the default keyframe | `--age N` |
| Month and stage | a day in that month on which `plantc year <id>` shows the same stage | `--day N` (1-365; default 196, mid-July) |
| Open-grown | `open` | `--env open` |
| Forest | `interior` (or `edge` for a forest margin, `suppressed` for an understorey plant) | `--env interior` |
| Several plants vary | 3 to 5 seeds, all compared | `--seed N` |

Check before rendering: does the plant at this age and day show what the
photos show? If not, fix the conditions, not the plant. Only when no age or
day can show it is the difference a finding (it then gets cause D0 or D6).

## 3. The standard views

Make every view the photos have, at full quality, in PlantGen's photo look.
PlantGen's standing set is side, 45 degrees down, and a branch or leaf
close-up, always.

| View | PlantGen | Notes |
| --- | --- | --- |
| V1 whole, side | `plantc render <id> --view side --quality standard --size 1536` | camera at about eye height; the 1.8 m figure or striped rod gives scale; PlantLab's photo look on a GPU where available |
| V2 leafless | V1 on a leafless `--day` for deciduous plants | evergreens: V1 is all there is |
| V3 from above, 45 degrees | `--view oblique` (and `--view top` for rosettes and cacti) | `top` ignores `--focus` today |
| V4 shoot close-up | `--focus X,Y,Z --span 1.0 --parts on` on a typical mid-crown shoot | pick the point from `plantc grow`'s crown size |
| V5 leaf | `plantc parts <id>` (the leaf card alone, median size) | compare with face-on photos and herbarium sheets |
| V6-V7 flower | `plantc parts` for the face; `--focus ... --span 0.3 --parts on` on a flowering shoot for posture and clusters | |
| V8 fruit, cone, bark | `plantc parts`; `--focus` on the trunk at 1.3 m for bark | |
| Through the year | `plantc year <id>` | for colour and stage by month |

`plantc organs <id> --age N --day N` gives exact counts of every organ in a
render: use them as the answer key when your AI counts render parts, and as
a check that the organs you expect are there at all.

## 4. Record what you rendered

Write `render-set.json`, one entry per image (schema:
`schemas/observation.schema.json`, `render` block): commit, generator
revision, species id, seed, age, day, env, view, focus and span, size and
quality. Every finding must be reproducible from it.

## 5. If you are not using PlantGen's renderer

The same steps apply to any plant model: match age, season and setting;
render the eight views; record the conditions. Only the commands change.
