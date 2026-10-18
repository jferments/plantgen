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
| V1 whole, side | `plantc render <id> --view side --quality standard --size 1536` | camera at about eye height; the orange post is a 1.8 m figure for scale (observers record it as a pole); PlantLab's photo look on a GPU where available |
| V2 leafless | V1 on a leafless `--day` for deciduous plants | evergreens: V1 is all there is |
| V3 from above, 45 degrees | `--view oblique` (and `--view top` for rosettes and cacti) | `top` ignores `--focus` today |
| V4 shoot close-up | `--focus leaf:F --span 1.0 --parts on` (the middle of the leaf F of the way up the plant), or `--focus X,Y,Z` on a typical outer-crown shoot | look at the image: a frame that is mostly sky or ground answers nothing, so move the focus onto foliage. `--parts on` can run out of memory on large conifers; leave it off there |
| V4-V5 leaf underside | `--view under --focus leaf:F --span 0.3` | looks up at the lower faces of the leaves |
| V5 leaf | `plantc atlas <id> --out atlas.png` (each organ's card flat and face-on, upper and lower faces) for shapes and ratios; the numbers from `plantc validate <id> --group sizes` (each stated size measured on the drawn organs, such as `leaf_length_m` and `leaf_width_m`) | compare with face-on photos and herbarium sheets. Never read a shape or ratio off `plantc parts` or a whole-plant or close-up view: their leaves are tilted, and the v1 replay's pawpaw leaf finding (length over width 3.0 against the photos' 2.4) was that foreshortening; the flat card measures 2.5. `plantc measure` gives only the median leaf card's length. An atlas is a row of cards: cut it into one image per card before triage |
| V6-V7 flower | `plantc atlas` for the face's parts and their shapes (flat cards); `--focus ... --span 0.3 --parts on` on a flowering shoot for posture and clusters | petal shapes and ratios only from the flat cards, as for leaves |
| V8 fruit, cone, bark | `plantc parts`; `--focus` on the trunk at 1.3 m for bark | |
| Through the year | `plantc year <id>` | for colour and stage by month |

Observe at least three render images per view and species (three seeds, or
three focus points for close-ups): one render image is one vote, and a
single odd frame then decides the verdict.

### Close-ups: three per seed, three seeds

In the v1 replay, most shoot features got one to three render answers, and
the features read inconsistently on renders (leaves crowded toward the tip,
drooping leaves, a felted underside, tufts at the limb tips) were those. So
every species gets, at each of three seeds, three framed close-ups:

| Close-up | Answers | Command (`plantc render <id> --seed S --age N --day D --env E`, then) |
| --- | --- | --- |
| a foliage shoot; for cacti, the ribs and areoles at mid-height | V4: shoot, leaf posture, needles; cacti V8: areoles, spines, ribs | `--focus leaf:0.7 --span 1.0 --parts on --size 1536 --quality standard --out shoot-S.png`; cacti `--focus R,Y,0 --span 0.3 --parts on ...`, R the stem's radius and Y half its height |
| a flowering shoot, when the render day is in flower; else a second foliage shoot lower in the crown | V4, and V6-V7 for flowers | `--focus X,Y,Z --span 0.3 --parts on --size 1536 --quality standard --out flower-S.png` on a flower (`plantc organs` confirms they are open that day), else `--focus leaf:0.4 --span 1.0 --parts on ...` |
| cacti: the apex; other plants: a leaf underside | V3/V8 apex wool, areoles near the top, ribs from above; or the leaf's lower face | `--view oblique --focus 0,H,0 --span W --parts on --size 1536 --quality standard --out apex-S.png`, H the height from `plantc grow` at that age and W about 1.5 times the stem's width; or `--view under --focus leaf:0.5 --span 0.3 --out under-S.png` |

Both shoot close-ups answer the shoot features, so each gets six render
answers; for cacti, the rib close-up and the apex both answer the areole
and spine features. Look at every frame before observing it: the oblique
apex view does not always centre the apex, and a frame of sky, ground or
bare wood is moved, not observed. `--view top` ignores `--focus` today.

Check that renders made on
different days actually differ (`plantc year <id>`): a species with no
seasonal organs draws the same plant every day, so leafless and autumn
photos cannot be matched and are compared only on what the season does not
change.

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
