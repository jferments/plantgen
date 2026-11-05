#!/usr/bin/env python3
"""Write docs/sites/moss-wall.obj and moss-wall.json: a garden wall and a
shed for growing mosses on (`plantc moss`, plantgen::mosses).

A brick garden wall running east to west, its bricks standing 6 mm proud
of recessed mortar joints and a concrete coping on top that overhangs both
faces; a hedge (kept clear) shading the west half of its south face. East
of it, a small rendered shed with a gable roof of stepped tile courses
(each course's lower edge stands proud of the one below, as real tiles
do), ridge tiles, eaves overhanging the walls, a door kept clear and a
concrete step. The frame is PlantGen's: +Y up, +X east, -Z north, metres.

Sizes are those of common British bricks (215 x 65 mm, 10 mm joints) and
plain roof tiles laid at about a 200 mm gauge; the rest by eye.

Run from the repository's root: python3 docs/sites/make_moss_wall.py
"""

import json
import math
from pathlib import Path

HERE = Path(__file__).resolve().parent


class Mesh:
    def __init__(self):
        self.v = []
        self.index = {}
        self.groups = {}

    def vert(self, p):
        key = tuple(round(c, 5) for c in p)
        if key not in self.index:
            self.v.append(p)
            self.index[key] = len(self.v)
        return self.index[key]

    def solid(self, material, tris):
        """A closed solid's triangles, turned outward by its volume's sign."""
        vol = sum(
            a[0] * (b[1] * c[2] - b[2] * c[1])
            - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
            for a, b, c in tris
        )
        if vol < 0:
            tris = [(a, c, b) for a, b, c in tris]
        faces = self.groups.setdefault(material, [])
        for tri in tris:
            faces.append([self.vert(p) for p in tri])

    def obj(self):
        lines = ["# docs/sites/make_moss_wall.py"]
        lines += [f"v {x:.5f} {y:.5f} {z:.5f}" for x, y, z in self.v]
        for material, faces in self.groups.items():
            lines.append(f"usemtl {material}")
            lines += ["f " + " ".join(str(i) for i in f) for f in faces]
        return "\n".join(lines) + "\n"


def hexahedron(c):
    """The 12 triangles of a hexahedron from its 8 corners, indexed as
    bits (x, y, z) of 0..7."""
    quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]]
    tris = []
    for q in quads:
        a, b, cc, d = (c[i] for i in q)
        tris += [(a, b, cc), (a, cc, d)]
    return tris


def box(x0, y0, z0, x1, y1, z1):
    return hexahedron([(x, y, z) for z in (z0, z1) for y in (y0, y1) for x in (x0, x1)])


def main():
    mesh = Mesh()
    # Ground.
    gx0, gx1, gz0, gz1 = -7.0, 8.0, -5.0, 5.0
    mesh.solid("ground", box(gx0, -0.3, gz0, gx1, 0.0, gz1))

    # The garden wall: a mortar core 6 mm back from each face, bricks
    # standing out of it.
    wx0, wx1 = -5.0, 1.0
    half = 0.1125
    recess = 0.006
    course, brick_h, brick_l, joint = 0.075, 0.065, 0.215, 0.01
    courses = 16
    height = courses * course
    mesh.solid("mortar", box(wx0 + recess, 0.0, -half + recess, wx1 - recess, height, half - recess))
    for k in range(courses):
        y0 = k * course + joint * 0.5
        y1 = y0 + brick_h
        offset = 0.0 if k % 2 == 0 else (brick_l + joint) * 0.5
        for z0, z1 in ((half - recess, half), (-half, -half + recess)):
            x = wx0 - offset
            while x < wx1:
                a = max(x + joint * 0.5, wx0)
                b = min(x + brick_l + joint * 0.5, wx1)
                if b - a > 0.02:
                    mesh.solid("brick", box(a, y0, z0, b, y1, z1))
                x += brick_l + joint
    # The coping, overhanging each face by 37 mm.
    mesh.solid("coping", box(wx0 - 0.03, height, -0.15, wx1 + 0.03, height + 0.06, 0.15))
    # A hedge shading the west half of the wall's south face.
    mesh.solid("hedge", box(-5.2, 0.0, 0.9, -2.0, 2.4, 2.1))

    # The shed.
    sx0, sx1, sz0, sz1 = 2.5, 6.0, -1.5, 1.5
    eaves = 2.0
    mesh.solid("render", box(sx0, 0.0, sz0, sx1, eaves, sz1))
    mesh.solid("door", box(3.6, 0.15, sz1, 4.4, 1.95, sz1 + 0.04))
    mesh.solid("step", box(3.4, 0.0, sz1, 4.6, 0.15, sz1 + 0.4))
    pitch = math.radians(35.0)
    over = 0.25
    run = (sz1 - sz0) * 0.5 + over
    rise = run * math.tan(pitch)
    ridge_y = eaves + (sz1 - sz0) * 0.5 * math.tan(pitch)
    eaves_y = ridge_y - rise
    rx0, rx1 = sx0 - 0.15, sx1 + 0.15
    zc = (sz0 + sz1) * 0.5
    # The roof deck: one closed prism, 0.1 m thick at the eaves.
    deck = 0.1
    prism = [
        (rx0, eaves_y - deck, zc - run), (rx1, eaves_y - deck, zc - run),
        (rx0, eaves_y, zc - run), (rx1, eaves_y, zc - run),
        (rx0, ridge_y, zc), (rx1, ridge_y, zc),
        (rx0, eaves_y, zc + run), (rx1, eaves_y, zc + run),
        (rx0, eaves_y - deck, zc + run), (rx1, eaves_y - deck, zc + run),
    ]
    p = prism
    tris = []

    def quad(a, b, c, d):
        tris.extend([(a, b, c), (a, c, d)])

    # Faces of the deck: two slopes, two fascias, the soffit, two gables.
    quad(p[2], p[3], p[5], p[4])
    quad(p[4], p[5], p[7], p[6])
    quad(p[0], p[1], p[3], p[2])
    quad(p[6], p[7], p[9], p[8])
    quad(p[8], p[9], p[1], p[0])
    tris.extend([(p[0], p[2], p[4]), (p[0], p[4], p[6]), (p[0], p[6], p[8])])
    tris.extend([(p[1], p[5], p[3]), (p[1], p[7], p[5]), (p[1], p[9], p[7])])
    mesh.solid("deck", tris)
    # Tile courses, stepped: each a wedge on the deck whose lower edge
    # stands two tiles' thickness proud and whose upper edge one, so every
    # course steps down onto the one below it.
    slope = run / math.cos(pitch)
    gauge = 0.2
    thick = 0.014
    tile_w, gap = 0.265, 0.008
    n_courses = int(slope / gauge)
    for side in (1.0, -1.0):
        # Down the slope from the ridge toward the eaves on this side.
        down = (0.0, -math.sin(pitch), side * math.cos(pitch))
        normal = (0.0, math.cos(pitch), side * math.sin(pitch))
        top = (0.0, ridge_y, zc)

        def at(x, s, h):
            return (
                x,
                top[1] + down[1] * s + normal[1] * h,
                top[2] + down[2] * s + normal[2] * h,
            )

        for k in range(n_courses):
            s1 = slope - k * gauge  # the course's lower edge
            s0 = max(s1 - gauge - 0.02, 0.0)
            offset = 0.0 if k % 2 == 0 else (tile_w + gap) * 0.5
            x = rx0 - offset
            while x < rx1:
                a = max(x + gap * 0.5, rx0)
                b = min(x + tile_w + gap * 0.5, rx1)
                if b - a > 0.03:
                    corners = []
                    for zz in (0, 1):  # 0 lower edge, 1 upper
                        for yy in (0, 1):  # 0 on the deck, 1 its top
                            for xx in (a, b):
                                s = s1 if zz == 0 else s0
                                h = 0.0 if yy == 0 else (2.0 * thick if zz == 0 else thick)
                                corners.append(at(xx, s, h + 0.0005))
                    mesh.solid("tile", hexahedron(corners))
                x += tile_w + gap
    # Ridge tiles: a capping prism along the ridge.
    w = 0.13
    cap = [
        (rx0, ridge_y + 0.01, zc - w), (rx1, ridge_y + 0.01, zc - w),
        (rx0, ridge_y + 0.09, zc), (rx1, ridge_y + 0.09, zc),
        (rx0, ridge_y + 0.01, zc + w), (rx1, ridge_y + 0.01, zc + w),
        (rx0, ridge_y - 0.04, zc), (rx1, ridge_y - 0.04, zc),
    ]
    c = cap
    tris = []
    quad(c[0], c[1], c[3], c[2])
    quad(c[2], c[3], c[5], c[4])
    quad(c[4], c[5], c[7], c[6])
    quad(c[6], c[7], c[1], c[0])
    tris.extend([(c[0], c[2], c[4]), (c[0], c[4], c[6]), (c[1], c[5], c[3]), (c[1], c[7], c[5])])
    mesh.solid("ridge", tris)

    (HERE / "moss-wall.obj").write_text(mesh.obj())
    site = {
        "mesh": "moss-wall.obj",
        "materials": {
            "ground": {"as": "soil", "colour": [0.10, 0.12, 0.06]},
            "mortar": {"as": "built", "colour": [0.42, 0.40, 0.36]},
            "brick": {"as": "built", "colour": [0.30, 0.10, 0.06]},
            "coping": {"as": "built", "colour": [0.40, 0.39, 0.36]},
            "hedge": {"as": "wood", "clear": True, "colour": [0.05, 0.10, 0.04]},
            "render": {"as": "built", "colour": [0.55, 0.52, 0.45]},
            "door": {"as": "wood", "clear": True, "colour": [0.20, 0.12, 0.07]},
            "step": {"as": "built", "colour": [0.38, 0.37, 0.34]},
            "deck": {"as": "wood", "clear": True, "colour": [0.25, 0.20, 0.14]},
            "tile": {"as": "built", "soil_m": 0.002, "colour": [0.32, 0.13, 0.07]},
            "ridge": {"as": "built", "colour": [0.30, 0.12, 0.07]},
        },
        "plants": [],
    }
    (HERE / "moss-wall.json").write_text(json.dumps(site, indent=2) + "\n")
    tri_count = sum(len(f) for f in mesh.groups.values())
    print(f"wrote moss-wall.obj ({len(mesh.v)} vertices, {tri_count} triangles) and moss-wall.json")


if __name__ == "__main__":
    main()
