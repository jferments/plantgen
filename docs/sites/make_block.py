#!/usr/bin/env python3
"""Write docs/sites/block.obj, block-road-*.json and block-walk.json crack
tiles, and block.json: a city block's worth of mixed ground for testing
the site field (`plantc field`, plantgen::site_field) and stands on it.

North of a cracked two-lane street, a strip of woodland on uneven ground
that falls away northward, with boulders and a fallen log. South of it, a
cracked sidewalk behind a curb, a picket fence, two utility poles with
their wires, an old house with a mossy south roof (soil gathered on it),
windows and a door kept clear, a hollow where water gathers, and a rocky
knoll to the south-east with boulders half sunk in it.

PlantGen's own test maker, as make_road.py (whose crack tiles it reuses):
a host makes its own meshes and hands them over. Sizes are chosen by eye.

Run from the repository's root: python3 docs/sites/make_block.py
"""

import json
import math
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import make_road  # noqa: E402

X0, X1 = -20.0, 20.0
Z0, Z1 = -18.0, 18.0
STEP = 0.5
WALK_TOP = 0.12
WALK = (3.15, 5.0)  # the sidewalk's Z, behind a 0.15 m curb


class Mesh:
    def __init__(self):
        self.v = []
        self.index = {}
        self.groups = {}

    def vert(self, p):
        # Corners at one place are written once.
        key = tuple(round(c, 4) for c in p)
        if key not in self.index:
            self.v.append(p)
            self.index[key] = len(self.v)
        return self.index[key]

    def solid(self, material, tris):
        """A closed solid's triangles (corner positions), turned outward
        by the sign of its volume."""
        vol = sum(
            a[0] * (b[1] * c[2] - b[2] * c[1])
            - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
            for a, b, c in tris
        )
        if vol < 0:
            tris = [(a, c, b) for a, b, c in tris]
        self.add(material, tris)

    def add(self, material, tris):
        faces = self.groups.setdefault(material, [])
        for tri in tris:
            faces.append([self.vert(p) for p in tri])

    def obj(self):
        lines = ["# docs/sites/make_block.py"]
        lines += [f"v {x:.4f} {y:.4f} {z:.4f}" for x, y, z in self.v]
        for material, faces in self.groups.items():
            lines.append(f"usemtl {material}")
            lines += ["f " + " ".join(str(i) for i in f) for f in faces]
        return "\n".join(lines) + "\n"


def box(x0, y0, z0, x1, y1, z1):
    c = [(x, y, z) for z in (z0, z1) for y in (y0, y1) for x in (x0, x1)]
    quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]]
    tris = []
    for a, b, cc, d in quads:
        tris += [(c[a], c[b], c[cc]), (c[a], c[cc], c[d])]
    return tris


def prism(centre, radius, length, axis, sides=8):
    """A closed n-sided prism round `axis` ('x' or 'y') from `centre`."""
    cx, cy, cz = centre
    ring = []
    for k in range(sides):
        a = 2 * math.pi * k / sides
        u, w = radius * math.cos(a), radius * math.sin(a)
        ring.append((u, w))

    def at(u, w, t):
        if axis == "y":
            return (cx + u, cy + t, cz + w)
        return (cx + t, cy + u, cz + w)

    tris = []
    for k in range(sides):
        (u0, w0), (u1, w1) = ring[k], ring[(k + 1) % sides]
        a, b = at(u0, w0, 0), at(u1, w1, 0)
        c, d = at(u1, w1, length), at(u0, w0, length)
        tris += [(a, b, c), (a, c, d)]
        tris.append((at(0, 0, 0), b, a))
        tris.append((at(0, 0, length), d, c))
    return tris


def rock(rng, centre, size):
    """A lumpy closed boulder: an icosahedron split once, its corners
    pushed in and out."""
    t = (1 + 5 ** 0.5) / 2
    v = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t),
         (0, -1, -t), (0, 1, -t), (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
    v = [tuple(c / math.sqrt(1 + t * t) for c in p) for p in v]
    f = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4),
         (11, 10, 2), (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8),
         (3, 8, 9), (4, 9, 5), (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
    mid = {}

    def middle(i, j):
        key = (min(i, j), max(i, j))
        if key not in mid:
            p = [(a + b) / 2 for a, b in zip(v[i], v[j])]
            n = math.sqrt(sum(c * c for c in p))
            v.append(tuple(c / n for c in p))
            mid[key] = len(v) - 1
        return mid[key]

    faces = []
    for a, b, c in f:
        ab, bc, ca = middle(a, b), middle(b, c), middle(c, a)
        faces += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
    sx, sy, sz = size
    pts = []
    for p in v:
        r = 1 + rng.uniform(-0.18, 0.18)
        pts.append((centre[0] + p[0] * sx * r, centre[1] + p[1] * sy * r, centre[2] + p[2] * sz * r))
    return [(pts[a], pts[b], pts[c]) for a, b, c in faces]


def height(x, z, rng_bumps):
    h = 0.0
    if z < -4.0:
        # Woodland ground falling away northward, uneven.
        d = -4.0 - z
        h = -0.12 * d
        for (bx, bz, a, s) in rng_bumps:
            h += a * math.exp(-((x - bx) ** 2 + (z - bz) ** 2) / (2 * s * s))
        h *= min(1.0, d / 2.0)
    elif z > 6.0:
        d = z - 6.0
        # A rocky knoll to the south-east and a hollow by the house.
        knoll = 3.0 * math.exp(-((x - 14) ** 2 + (z - 14) ** 2) / (2 * 4.5 ** 2))
        hollow = -0.6 * math.exp(-((x + 1.0) ** 2 + (z - 12.0) ** 2) / (2 * 1.8 ** 2))
        h = (knoll + hollow) * min(1.0, d / 2.0)
        # Level under the house.
        if -13.0 <= x <= -3.0 and 8.0 <= z <= 16.0:
            h = 0.0
    return h


def main():
    rng = random.Random(11)
    mesh = Mesh()
    bumps = [(rng.uniform(X0, X1), rng.uniform(Z0, -5), rng.uniform(-0.5, 0.7), rng.uniform(1.5, 4))
             for _ in range(14)]

    # Terrain: a sheet of soil facing up, the woodland's darker.
    nx, nz = int((X1 - X0) / STEP), int((Z1 - Z0) / STEP)
    grid = [[(X0 + i * STEP, height(X0 + i * STEP, Z0 + k * STEP, bumps), Z0 + k * STEP)
             for i in range(nx + 1)] for k in range(nz + 1)]
    for k in range(nz):
        for i in range(nx):
            a, b, c, d = grid[k][i], grid[k][i + 1], grid[k + 1][i + 1], grid[k + 1][i]
            material = "woodland" if a[2] < -4.0 else "ground"
            # Up is +Y: (a, d, c) runs counter-clockwise seen from above.
            mesh.add(material, [(a, d, c), (a, c, b)])

    # The street's crack tiles, eight of them at decay 0.3 to 0.8.
    tiles = []
    for n in range(8):
        decay = round(0.3 + 0.5 * ((n * 5) % 8) / 7, 2)
        data, _, _ = make_road.tile(decay, X0 + 5.0 * n, 200 + n)
        data["made_by"] = {"maker": "docs/sites/make_block.py", "decay": decay, "seed": 200 + n}
        name = f"block-road-{n}.json"
        (HERE / name).write_text(json.dumps(data, indent=1) + "\n")
        tiles.append(name)

    # The sidewalk: a concrete tile with joints every 1.5 m, some cracked
    # through, behind a curb.
    walk_rng = random.Random(5)
    cracks = []
    x = X0 + 1.5
    while x < X1:
        if walk_rng.random() < 0.6:
            pts = make_road.wander(walk_rng, (x, WALK[0]), (x, WALK[1]))
            cracks.append(make_road.crack(walk_rng, pts, walk_rng.uniform(0.004, 0.015), 0.7))
        x += 1.5
    for _ in range(4):
        x = walk_rng.uniform(X0 + 1, X1 - 1)
        pts = make_road.wander(walk_rng, (x, WALK[0] + 0.3), (x + walk_rng.uniform(1, 3), WALK[1] - 0.2))
        cracks.append(make_road.crack(walk_rng, pts, walk_rng.uniform(0.003, 0.01), 0.6))
    walk = {
        "version": 1,
        "origin_m": [X0, WALK[0]],
        "size_m": [X1 - X0, WALK[1] - WALK[0]],
        "top_m": WALK_TOP,
        "thickness_m": 0.1,
        "base_m": 0.15,
        "surface": "concrete",
        "cracks": cracks,
        "holes": [],
        "traffic": [{"points_m": [[X0, 4.1], [X1, 4.1]], "spread_m": 0.4, "load": 0.3,
                     "track_m": 0.0, "body_m": 0.0}],
        "made_by": {"maker": "docs/sites/make_block.py", "seed": 5},
    }
    (HERE / "block-walk.json").write_text(json.dumps(walk, indent=1) + "\n")
    tiles.append("block-walk.json")
    mesh.solid("curb", box(X0, -0.1, 3.0, X1, WALK_TOP + 0.03, WALK[0]))

    # The old house: walls, a gable roof (its south side mossy), windows
    # and a door kept clear.
    hx0, hx1, hz0, hz1, eave, ridge = -12.0, -4.0, 9.0, 15.0, 2.8, 5.0
    mesh.solid("render", box(hx0, -0.2, hz0, hx1, eave, hz1))
    o = 0.35
    mid = (hz0 + hz1) / 2
    a0, a1 = (hx0 - o, eave, hz0 - o), (hx1 + o, eave, hz0 - o)
    b0, b1 = (hx0 - o, eave, hz1 + o), (hx1 + o, eave, hz1 + o)
    r0, r1 = (hx0 - o, ridge, mid), (hx1 + o, ridge, mid)
    north = [(a0, r0, r1), (a0, r1, a1)]
    south = [(b0, b1, r1), (b0, r1, r0)]
    ends = [(a0, b0, r0), (a1, r1, b1)]
    under = [(a0, a1, b1), (a0, b1, b0)]
    # One closed roof solid, written in two materials: wound outward as a
    # whole first.
    whole = north + south + ends + under
    vol = sum(a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
              + a[2] * (b[0] * c[1] - b[1] * c[0]) for a, b, c in whole)
    turn = (lambda t: [(a, c, b) for a, b, c in t]) if vol < 0 else (lambda t: t)
    mesh.add("roof", turn(north + ends + under))
    mesh.add("roof-moss", turn(south))
    for wx in (-11.0, -7.0):
        z = hz0 - 0.01
        mesh.add("glass", [((wx, 1.0, z), (wx + 1.2, 1.0, z), (wx + 1.2, 2.2, z)),
                           ((wx, 1.0, z), (wx + 1.2, 2.2, z), (wx, 2.2, z))])
    z = hz0 - 0.01
    mesh.add("door", [((-8.9, 0.0, z), (-8.0, 0.0, z), (-8.0, 2.1, z)),
                      ((-8.9, 0.0, z), (-8.0, 2.1, z), (-8.9, 2.1, z))])

    # A picket fence along the yard's front, with a gap for a gate.
    fz = 6.0
    x = -16.0
    while x <= -1.0 + 1e-9:
        mesh.solid("fence", box(x - 0.05, 0.0, fz - 0.05, x + 0.05, 1.2, fz + 0.05))
        x += 2.5
    for y in (0.3, 0.85):
        for (s, e) in ((-16.0, -9.5), (-8.5, -1.0)):
            mesh.solid("fence", box(s, y, fz + 0.05, e, y + 0.08, fz + 0.09))
    x = -15.9
    while x < -1.1:
        if not -9.5 < x < -8.5:
            mesh.solid("fence", box(x, 0.0, fz + 0.09, x + 0.08, 1.0, fz + 0.11))
        x += 0.15

    # Two utility poles and their wires along the sidewalk's back.
    poles = [(-15.0, 5.5), (5.0, 5.5)]
    for px, pz in poles:
        mesh.solid("pole", prism((px, -0.5, pz), 0.15, 10.0, "y"))
    for wy in (8.6, 9.0):
        (ax, az), (bx, bz) = poles
        mid_sag = wy - 0.35
        prev = None
        for k in range(9):
            t = k / 8
            px = ax + (bx - ax) * t
            py = wy - (wy - mid_sag) * 4 * t * (1 - t)
            p = (px, py, az + 0.16)
            if prev is not None:
                q, r = prev, p
                mesh.add("wire", [((q[0], q[1] - 0.006, q[2]), (r[0], r[1] - 0.006, r[2]), (r[0], r[1] + 0.006, r[2])),
                                  ((q[0], q[1] - 0.006, q[2]), (r[0], r[1] + 0.006, r[2]), (q[0], q[1] + 0.006, q[2]))])
            prev = p

    # Boulders: two in the woodland, three half sunk in the knoll.
    for (bx, bz, s) in [(-9.0, -9.0, (1.1, 0.7, 0.9)), (6.0, -12.0, (0.8, 0.5, 0.7)),
                        (12.0, 12.0, (1.4, 0.9, 1.1)), (15.5, 15.0, (0.9, 0.6, 0.8)),
                        (17.0, 10.5, (0.7, 0.5, 0.6))]:
        y = height(bx, bz, bumps) + 0.25 * s[1]
        mesh.solid("rock", rock(rng, (bx, y, bz), s))

    # A fallen log in the woodland.
    lx, lz = -2.0, -11.0
    ly = height(lx + 3, lz, bumps) + 0.2
    mesh.solid("log", prism((lx, ly, lz), 0.28, 6.0, "x"))

    (HERE / "block.obj").write_text(mesh.obj())
    site = {
        "mesh": "block.obj",
        "materials": {
            "ground": {"as": "soil", "colour": [0.22, 0.27, 0.12]},
            "woodland": {"as": "soil", "colour": [0.14, 0.11, 0.07]},
            "curb": {"as": "built", "colour": [0.45, 0.44, 0.41]},
            "render": {"as": "built", "colour": [0.62, 0.57, 0.48]},
            "roof": {"as": "built", "colour": [0.22, 0.22, 0.24]},
            "roof-moss": {"as": "built", "soil_m": 0.04, "colour": [0.20, 0.24, 0.12]},
            "glass": {"as": "built", "thin_m": 0.02, "clear": True, "colour": [0.30, 0.38, 0.42]},
            "door": {"as": "wood", "thin_m": 0.04, "clear": True, "colour": [0.28, 0.17, 0.11]},
            "fence": {"as": "wood", "colour": [0.70, 0.68, 0.62]},
            "pole": {"as": "wood", "colour": [0.26, 0.19, 0.12]},
            "wire": {"as": "built", "thin_m": 0.012, "colour": [0.10, 0.10, 0.10]},
            "rock": {"as": "rock", "colour": [0.34, 0.33, 0.31]},
            "log": {"as": "bark", "colour": [0.18, 0.13, 0.09]},
        },
        "pavements": tiles,
        # A few plants for `plantc site` to draw; stands come from the
        # field, not from here.
        "plants": [
            {"species": "hedera-helix", "at": [-10.5, 8.7], "seed": 1, "years": 8},
            {"species": "lonicera-ciliosa", "at": [-15.0, 5.85], "seed": 1, "years": 6},
            {"species": "alnus-rubra", "at": [-4.0, -9.0], "seed": 1, "years": 12},
        ],
    }
    (HERE / "block.json").write_text(json.dumps(site, indent=1) + "\n")
    print(f"wrote block.obj ({len(mesh.v)} corners), {len(tiles)} crack tiles, block.json")


if __name__ == "__main__":
    main()
