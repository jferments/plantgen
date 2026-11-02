#!/usr/bin/env python3
"""Write docs/sites/road.obj, road-*.json crack tiles and road.json: a
two-lane street run along X between grass verges, in three tiles at decay
levels 0.2, 0.5 and 0.8, with weeds set by hand in its cracks.

This is PlantGen's own test maker (pavement-cracks plan K1), not a game's
crack generator: a host makes its cracks from its own decay model and hands
PlantGen the tiles. The kinds of cracking and where they fall follow the
FHWA's Distress Identification Manual for asphalt pavements (Miller and
Bellinger 2014): transverse cracks across the road, longitudinal cracks
along the wheel paths and the lane joint, fatigue (alligator) cracking in
the wheel paths, block cracking, edge cracking and potholes. How many come
at each decay level, how wide they open and how much grit lies in them are
chosen by eye for a test road; nothing reads them as facts.

Run from the repository's root: python3 docs/sites/make_road.py
"""

import json
import math
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent

ROAD_WIDTH = 6.0  # two 3 m lanes, Z from -3 to 3
TILE_LENGTH = 5.0
VERGE = 2.0
TOP = 0.0
THICKNESS = 0.08
BASE = 0.25
DECAYS = [0.2, 0.5, 0.8]
WHEEL_PATHS = [-2.25, -0.75, 0.75, 2.25]  # Z of each lane's two wheel paths


def wander(rng, start, end, step=0.05, sway=0.012):
    """A crack's line from start to end, wandering a little each step."""
    (x0, z0), (x1, z1) = start, end
    length = math.hypot(x1 - x0, z1 - z0)
    n = max(2, int(length / step))
    nx, nz = -(z1 - z0) / length, (x1 - x0) / length
    off = 0.0
    points = []
    for i in range(n + 1):
        t = i / n
        if 0 < i < n:
            off += rng.gauss(0.0, sway)
            off *= 0.97
        else:
            off = 0.0
        points.append([round(x0 + (x1 - x0) * t + nx * off, 4),
                       round(z0 + (z1 - z0) * t + nz * off, 4)])
    return points


def crack(rng, points, width, decay):
    """A crack with its opening along it and the grit lying in it: wider
    cracks and older pavement hold more, up to the slab's brim."""
    widths, fills = [], []
    for _ in points:
        w = max(0.001, width * (0.6 + 0.8 * rng.random()))
        widths.append(round(w, 4))
        fill = THICKNESS * min(1.0, (0.2 + decay) * w / (w + 0.006))
        fills.append(round(fill, 4))
    return {"points_m": points, "width_m": widths, "fill_m": fills}


def tile(decay, x0, seed):
    rng = random.Random(seed)
    x1 = x0 + TILE_LENGTH
    cracks, holes, sites = [], [], []
    width = 0.002 + 0.02 * decay ** 1.5

    def keep(c, kind):
        cracks.append(c)
        sites.append((kind, c))

    # Transverse cracks, closer as the pavement ages.
    spacing = 12.0 * (1.0 - decay) ** 2 + 1.2
    x = x0 + rng.uniform(0.3, spacing)
    while x < x1 - 0.2:
        z_start = -3.0 if rng.random() < 0.7 else rng.uniform(-3.0, 0.0)
        z_end = 3.0 if rng.random() < 0.7 else rng.uniform(0.0, 3.0)
        lean = rng.gauss(0.0, 0.15)
        keep(crack(rng, wander(rng, [x, z_start], [x + lean, z_end]), width, decay), "transverse")
        x += spacing * rng.uniform(0.7, 1.3)
    # Longitudinal cracks along the lane joint and the wheel paths.
    if decay > 0.15:
        for z in [0.0] + WHEEL_PATHS:
            if rng.random() < decay:
                a = rng.uniform(x0, x1 - 1.0)
                b = min(x1, a + rng.uniform(1.0, TILE_LENGTH * decay * 1.5))
                keep(crack(rng, wander(rng, [a, z + rng.gauss(0, 0.05)], [b, z + rng.gauss(0, 0.05)]),
                           width * 0.8, decay), "longitudinal")
    # Edge cracks a few centimetres in from each edge.
    if decay > 0.3:
        for z in (-2.9, 2.9):
            a = rng.uniform(x0, x0 + 1.0)
            keep(crack(rng, wander(rng, [a, z], [min(x1, a + TILE_LENGTH * decay), z + rng.gauss(0, 0.05)]),
                       width, decay), "edge")
    # Block cracking: cracks across the blocks between the others.
    if decay > 0.45:
        for _ in range(int(14 * (decay - 0.45))):
            xa = rng.uniform(x0, x1)
            za = rng.uniform(-3.0, 3.0)
            angle = rng.choice([0.0, math.pi / 2]) + rng.gauss(0, 0.1)
            span = rng.uniform(0.8, 2.0)
            xb = min(x1, max(x0, xa + math.cos(angle) * span))
            zb = min(3.0, max(-3.0, za + math.sin(angle) * span))
            keep(crack(rng, wander(rng, [xa, za], [xb, zb]), width * 0.7, decay), "block")
    # Fatigue (alligator) cracking: a net of small cells in the wheel paths.
    if decay > 0.4:
        cell = 0.22
        for z_path in WHEEL_PATHS:
            if rng.random() > decay:
                continue
            a = rng.uniform(x0, x1 - 1.0)
            run = min(x1 - a, rng.uniform(0.8, 3.0) * decay)
            cols = int(run / cell) + 1
            rows = 3
            grid = [[(a + i * cell + rng.uniform(-0.05, 0.05),
                      z_path - cell + j * cell + rng.uniform(-0.05, 0.05))
                     for j in range(rows)] for i in range(cols)]
            for i in range(cols):
                for j in range(rows):
                    for (di, dj) in ((1, 0), (0, 1)):
                        if i + di < cols and j + dj < rows and rng.random() < 0.4 + decay * 0.5:
                            p, q = grid[i][j], grid[i + di][j + dj]
                            keep(crack(rng, wander(rng, list(p), list(q), 0.04, 0.006),
                                       width * 0.6, decay), "fatigue")
    # Potholes where the fatigue cracking has broken out.
    if decay > 0.6:
        for _ in range(int(6 * (decay - 0.6)) + 1):
            cx = rng.uniform(x0 + 0.6, x1 - 0.6)
            cz = rng.choice(WHEEL_PATHS) + rng.gauss(0, 0.1)
            r = rng.uniform(0.15, 0.4)
            outline = []
            for k in range(14):
                t = 2 * math.pi * k / 14
                rr = r * (0.75 + 0.5 * rng.random())
                outline.append([round(cx + rr * math.cos(t), 4), round(cz + rr * math.sin(t), 4)])
            depth = rng.uniform(0.06, 0.15)
            holes.append({"outline_m": outline, "depth_m": round(depth, 3),
                          "fill_m": round(depth * (0.3 + 0.5 * decay), 3), "centre": [cx, cz]})
    return {
        "version": 1,
        "origin_m": [x0, -ROAD_WIDTH / 2],
        "size_m": [TILE_LENGTH, ROAD_WIDTH],
        "top_m": TOP,
        "thickness_m": THICKNESS,
        "base_m": BASE,
        "surface": "asphalt",
        "cracks": cracks,
        "holes": [{k: v for k, v in h.items() if k != "centre"} for h in holes],
        "made_by": {"maker": "docs/sites/make_road.py", "decay": decay, "seed": seed},
    }, sites, holes


def ground_obj(x0, x1):
    """Grass verges either side of the road, and soil under it (drawn by the
    tiles), as 1 m quads."""
    lines = ["# Verges and roadbed for docs/sites/road.json (make_road.py)", "usemtl verge"]
    vertices, faces = [], []
    z_edges = [-ROAD_WIDTH / 2 - VERGE + i * 1.0 for i in range(int(ROAD_WIDTH + 2 * VERGE) + 1)]
    xs = [x0 - 1.0 + i for i in range(int(x1 - x0 + 2) + 1)]
    index = {}
    for i, x in enumerate(xs):
        for j, z in enumerate(z_edges):
            index[(i, j)] = len(vertices) + 1
            vertices.append((x, 0.0, z))
    for i in range(len(xs) - 1):
        for j in range(len(z_edges) - 1):
            a, b = index[(i, j)], index[(i + 1, j)]
            c, d = index[(i + 1, j + 1)], index[(i, j + 1)]
            # Counter-clockwise seen from above (+Y), Z south.
            faces.append((a, d, c, b))
    for v in vertices:
        lines.append(f"v {v[0]:.3f} {v[1]:.3f} {v[2]:.3f}")
    for f in faces:
        lines.append("f " + " ".join(str(k) for k in f))
    return "\n".join(lines) + "\n"


def main():
    x0 = -TILE_LENGTH * len(DECAYS) / 2
    plants = []
    rng = random.Random(7)
    tiles = []
    for n, decay in enumerate(DECAYS):
        start = x0 + n * TILE_LENGTH
        data, sites, holes = tile(decay, start, 100 + n)
        name = f"road-{int(decay * 10)}.json"
        (HERE / name).write_text(json.dumps(data, indent=1) + "\n")
        tiles.append(name)
        # Weeds set by hand in the wider cracks: a few on young pavement,
        # more as it opens.
        herbs = ["hypochaeris-radicata", "trifolium-repens", "dactylis-glomerata",
                 "holcus-lanatus", "cirsium-vulgare", "verbascum-thapsus"]
        open_sites = [c for kind, c in sites if kind in ("transverse", "longitudinal", "edge", "block")]
        rng.shuffle(open_sites)
        for c in open_sites[: int(3 + 10 * decay)]:
            k = rng.randrange(len(c["points_m"]))
            x, z = c["points_m"][k]
            plants.append({"species": rng.choice(herbs), "at": [x, z],
                           "seed": rng.randrange(1, 9), "years": 3})
        for h in holes:
            plants.append({"species": "rubus-armeniacus" if rng.random() < 0.5 else "alnus-rubra",
                           "at": [round(h["centre"][0], 3), round(h["centre"][1], 3)],
                           "seed": 1, "years": 4})
    obj = ground_obj(x0, x0 + TILE_LENGTH * len(DECAYS))
    (HERE / "road.obj").write_text(obj)
    site = {
        "mesh": "road.obj",
        "materials": {"verge": {"as": "soil", "colour": [0.20, 0.28, 0.12]}},
        "pavements": tiles,
        "plants": plants,
    }
    (HERE / "road.json").write_text(json.dumps(site, indent=1) + "\n")
    print(f"wrote {len(tiles)} tiles and {len(plants)} plants")


if __name__ == "__main__":
    main()
