"""Write docs/sites/house.obj: a test site for `plantc site` with a house
and two fences (python3 docs/sites/make_house.py).

+Y up, metres, -Z north. A lawn 16 m by 12 m with its top at 0; a house
6 m by 5 m, walls 2.7 m high under a gable roof that overhangs 0.3 m, its
south face at z = -1 with two windows and a door; a picket fence and a
post-and-wire fence along z = 2. Solids are closed boxes and prisms wound
counter-clockwise seen from outside. The windows, the door and the fence
wires are thin parts: open faces the site document gives a thickness.
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))

vertices = []
groups = {}


def face(material, *corners):
    start = len(vertices)
    vertices.extend(corners)
    groups.setdefault(material, []).append(
        [start + k + 1 for k in range(len(corners))]
    )


def box(material, low, high):
    """A closed box from low to high, each face counter-clockwise from outside."""
    (x0, y0, z0), (x1, y1, z1) = low, high
    p = [
        (x, y, z)
        for z in (z0, z1)
        for y in (y0, y1)
        for x in (x0, x1)
    ]
    # Index i: x bit 1, y bit 2, z bit 4.
    for quad in (
        (0, 2, 3, 1),  # -z
        (4, 5, 7, 6),  # +z
        (0, 4, 6, 2),  # -x
        (1, 3, 7, 5),  # +x
        (0, 1, 5, 4),  # -y
        (2, 6, 7, 3),  # +y
    ):
        face(material, *(p[i] for i in quad))


def gable(material, x0, x1, z0, z1, eave, ridge):
    """A closed prism roof: its ridge along x, over z0 to z1."""
    zm = (z0 + z1) / 2
    a, b, c = (z0, eave), (z1, eave), (zm, ridge)
    # Ends (triangles), the two slopes and the underside.
    face(material, (x0, a[1], a[0]), (x0, b[1], b[0]), (x0, c[1], c[0]))
    face(material, (x1, a[1], a[0]), (x1, c[1], c[0]), (x1, b[1], b[0]))
    face(material, (x0, a[1], a[0]), (x0, c[1], c[0]), (x1, c[1], c[0]), (x1, a[1], a[0]))
    face(material, (x0, b[1], b[0]), (x1, b[1], b[0]), (x1, c[1], c[0]), (x0, c[1], c[0]))
    face(material, (x0, a[1], a[0]), (x1, a[1], a[0]), (x1, b[1], b[0]), (x0, b[1], b[0]))


def panel_z(material, x0, x1, y0, y1, z):
    """An open rectangle in the plane z, facing +z."""
    face(material, (x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z))


# The lawn.
box("lawn", (-8.0, -1.0, -8.0), (8.0, 0.0, 4.0))

# The house: walls as one solid, its roof overhanging 0.3 m.
box("render", (-4.0, 0.0, -6.0), (2.0, 2.7, -1.0))
gable("roof", -4.3, 2.3, -6.3, -0.7, 2.7, 4.5)
# Two windows and a door on the south face, 1 cm proud of it.
for x0 in (-3.4, 0.6):
    panel_z("glass", x0, x0 + 1.0, 1.0, 2.2, -0.99)
panel_z("door", -1.2, -0.3, 0.0, 2.1, -0.99)

# A picket fence from x = -6 to -1 along z = 2: posts every 2.5 m, two
# rails, pickets 8 cm wide with 6 cm gaps.
for x in (-6.0, -3.5, -1.0):
    box("fence", (x - 0.05, 0.0, 1.95), (x + 0.05, 1.1, 2.05))
for y in (0.25, 0.8):
    box("fence", (-6.0, y, 1.92), (-1.0, y + 0.08, 1.95))
x = -5.93
while x + 0.08 <= -1.05:
    box("fence", (x, 0.05, 1.90), (x + 0.08, 1.0, 1.92))
    x += 0.14

# A post-and-wire fence from x = 0.5 to 6 along z = 2: posts every
# 2.75 m, four wires 1 cm ribbons at 0.3, 0.6, 0.9 and 1.2 m.
for x in (0.5, 3.25, 6.0):
    box("post", (x - 0.06, 0.0, 1.94), (x + 0.06, 1.4, 2.06))
for y in (0.3, 0.6, 0.9, 1.2):
    panel_z("wire", 0.5, 6.0, y - 0.005, y + 0.005, 2.065)

with open(os.path.join(HERE, "house.obj"), "w") as out:
    out.write(
        "# A test site for `plantc site`: a house with windows and a door,\n"
        "# a picket fence and a post-and-wire fence on a lawn. +Y up, metres.\n"
        "# Written by make_house.py.\n"
    )
    for x, y, z in vertices:
        out.write(f"v {x:.4f} {y:.4f} {z:.4f}\n")
    for material, faces in groups.items():
        out.write(f"usemtl {material}\n")
        for corners in faces:
            out.write("f " + " ".join(map(str, corners)) + "\n")
