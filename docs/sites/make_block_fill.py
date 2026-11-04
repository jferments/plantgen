#!/usr/bin/env python3
"""Write docs/sites/block-fill/: the communities and the stand map that
fill the test city block (block.json) with `plantc fill --field`.

The tile is 40 m square, its south-west corner at (x, z) = (-20, 18), in
8 m stand cells (north is -z, so a cell's row counts north from z = 18):

- the rows north of z = -6, the woodland strip: second-growth Douglas-fir
  forest with bigleaf maple, vine maple, salal and sword fern, 60 years
  since it was last cleared;
- the row from z = 2 to -6, the street: no stand (bare); what grows there
  comes up in the pavement's cracks (`--cracks`);
- the rows south of z = 2, the sidewalk and the old yard: grasses and
  weeds of an untended lawn, 8 years since it was last mown, with ivy on
  the house and the fence. On the sidewalk they stand only in its
  cracks, as their open share lets them.

Species are chosen by eye for an old lot in the Pacific Northwest. Run
from the repository's root: python3 docs/sites/make_block_fill.py
"""

import json
from pathlib import Path

OUT = Path(__file__).resolve().parent / "block-fill"
TILE_M, CELL_M = 40.0, 8.0
CORNER_Z = 18.0

WOODLAND = {"id": "block-woodland", "members": [
    {"species": "pseudotsuga-menziesii", "layer": "canopy"},
    {"species": "acer-macrophyllum", "layer": "subcanopy", "abundance": 0.5},
    {"species": "acer-circinatum", "layer": "shrub", "abundance": 0.6},
    {"species": "gaultheria-shallon", "layer": "shrub", "abundance": 0.8},
    {"species": "polystichum-munitum", "layer": "herb"},
]}

YARD = {"id": "block-yard", "members": [
    {"species": "holcus-lanatus", "layer": "herb", "environments": ["open"], "abundance": 1.5},
    {"species": "dactylis-glomerata", "layer": "herb", "environments": ["open"]},
    {"species": "poa-annua", "layer": "herb", "environments": ["open"], "abundance": 0.6},
    {"species": "taraxacum-sect-taraxacum", "layer": "herb", "environments": ["open"], "abundance": 0.6},
    {"species": "plantago-major", "layer": "herb", "environments": ["open"], "abundance": 0.5},
    {"species": "trifolium-repens", "layer": "herb", "environments": ["open"], "abundance": 0.6},
    {"species": "erigeron-canadensis", "layer": "herb", "environments": ["open"], "abundance": 0.3},
    {"species": "hedera-helix", "layer": "climber", "abundance": 0.8},
]}


def main():
    OUT.mkdir(exist_ok=True)
    (OUT / "woodland.json").write_text(json.dumps(WOODLAND, indent=1) + "\n")
    (OUT / "yard.json").write_text(json.dumps(YARD, indent=1) + "\n")
    cols = int(TILE_M / CELL_M)
    cells = []
    for row in range(cols):
        # The cell's middle, z.
        z = CORNER_Z - (row + 0.5) * CELL_M
        if z < -6.0:
            cells.append({"community": 0, "years_since_disturbance": 60.0})
        elif z > 2.0:
            cells.append({"community": 1, "years_since_disturbance": 8.0})
        else:
            cells.append({"community": None, "years_since_disturbance": 0.0})
    # Rows from the south-west corner: each row's cell repeated across.
    stands = {"cell_m": CELL_M, "cols": cols,
              "cells": [cell for cell in cells for _ in range(cols)]}
    (OUT / "stands.json").write_text(json.dumps(stands) + "\n")


if __name__ == "__main__":
    main()
