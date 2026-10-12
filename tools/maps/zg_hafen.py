"""zg_hafen: bomb defusal in a Kiel-style naval harbour.

North: the quay with the KSK start, a grey frigate alongside. South: the harbour gate with the OPFOR start.
West lane: the container yard with the long container lane ("Lange") to site A (the west end of the quay, a stacked
site office as the high ground). Mid: the harbour road past the customs house under the big red gantry crane ("Kran").
East lane: through the dark clinker warehouse ("Lagerhalle") to site B (the loading docks at the east end of the quay).

    python3 tools/maps/zg_hafen.py
"""

import os
import random
import sys

sys.path.insert(0, os.path.dirname(__file__))
from zmap import Map, box, hull, prism, stairs  # noqa: E402
from zkit import CLIP, NODRAW, TRIM, Kit, T, container_paint, places, props_info, spawns  # noqa: E402

m = Map("zg_hafen")
k = Kit(m)
k.container_props = True
k.facade_style = "industrial"
rng = random.Random(7)

QY = 2150  # quay edge
WATER = -96
XW, XE, YS = -2700, 2700, -2050  # playable bounds (west, east, south)
LX0, LX1 = -2150, -1700  # long lane

ASPH, CONC, PAVE = T("gnd_asphalt"), T("gnd_concrete"), T("concrete_slabs")
BRICK = T("brick_dark")
PAINT = ["red", "blue", "green", "metal_rust", "metal_corrugated", "red", "blue"]


def ground(lo, hi, tex):
    k.ground(lo[0], lo[1], hi[0], hi[1], tex, 700)


def paint_line(x0, y0, x1, y1, w=8):
    """A painted road line (a thin raised strip)."""
    if abs(x1 - x0) >= abs(y1 - y0):
        k.dblk((min(x0, x1), y0 - w / 2, 0), (max(x0, x1), y0 + w / 2, 0.5), {"top": T("plaster_white"), "sides": NODRAW, "bottom": NODRAW})
    else:
        k.dblk((x0 - w / 2, min(y0, y1), 0), (x0 + w / 2, max(y0, y1), 0.5), {"top": T("plaster_white"), "sides": NODRAW, "bottom": NODRAW})


def row(x0, y0, axis, lengths, z=0, gap=8, colors=None, doors="+"):
    """Containers in a row along `axis` starting at (x0, y0)."""
    u = x0 if axis == "x" else y0
    for i, L in enumerate(lengths):
        c = (colors[i % len(colors)] if colors else rng.choice(PAINT))
        if axis == "x":
            k.container(u, y0, z, L, "x", c, doors)
        else:
            k.container(x0, u, z, L, "y", c, doors)
        u += L + gap
    return u


def filler(x0, y0, x1, y1, z1, axis="x", z0=0):
    """Unreachable container mass: plain blocks tiled in container footprints (tops and outer sides painted)."""
    if axis == "x":
        y = y0
        while y < y1 - 1:
            x = x0
            yb = min(y + 288, y1)
            while x < x1 - 1:
                L = rng.choice((480, 960))
                xb = min(x + L, x1)
                c = rng.choice(PAINT)
                tex = container_paint(c)
                k.blk((x, y, z0), (xb, yb, z1), {"sides": tex, "top": tex, "bottom": NODRAW})
                x = xb
            y = yb
    else:
        x = x0
        while x < x1 - 1:
            y = y0
            xb = min(x + 288, x1)
            while y < y1 - 1:
                L = rng.choice((480, 960))
                yb = min(y + L, y1)
                c = rng.choice(PAINT)
                tex = container_paint(c)
                k.blk((x, y, z0), (xb, yb, z1), {"sides": tex, "top": tex, "bottom": NODRAW})
                y = yb
            x = xb


# ---------------------------------------------------------------------------------------------------------------
# Ground: asphalt yard, concrete quay, paved gate area, the quay wall and the water.
ground((-650, YS - 60), (XE, -1300), PAVE)
ground((-2150, -1830), (-650, -1300), ASPH)
ground((LX0, -1300), (-450, 1000), ASPH)
ground((-450, -1300), (950, 450), ASPH)
ground((950, -1300), (XE, 550), CONC)  # warehouse floor + forecourt
ground((-450, 450), (950, 1650), ASPH)
ground((950, 550), (XE, 1650), CONC)
ground((XW, 1000), (-450, 1650), ASPH)
ground((XW - 400, 1650), (XE + 60, QY), CONC)
# Quay wall down to the water, steel coping on its edge, and the water itself.
k.blk((XW - 400, QY - 16, WATER - 16), (XE + 60, QY, -16), {"sides": NODRAW, "+y": T("concrete_dirty"), "top": NODRAW, "bottom": NODRAW})
k.dblk((XW - 400, QY - 20, 0), (XE + 60, QY + 2, 3), {"sides": T("metal_rust"), "top": T("metal_rust"), "bottom": NODRAW, "-y": NODRAW})
# Water: a strip along the quay (the frigate hides the rest) and open water past its ends.
# Water: a flat brush below the quay (the client draws dev/water dark and glossy); the frigate hides the rest.
for (x0, y0, x1, y1) in ((-3000, QY, 3000, QY + 400), (-3000, QY + 400, -2500, 2950), (1500, QY + 400, 3000, 2950)):
    k.ground(x0, y0, x1, y1, "dev/water", 620, WATER - 16, WATER)
# Rubber fenders on the quay wall.
for x in range(-2500, 2600, 400):
    k.add(prism((QY + 14, -40), 14, x - 48, x + 48, 8, TRIM, rot=22.5, axis="x"))
# Keep players on the quay.
k.clip((XW - 16, QY + 2, 0), (XE + 16, QY + 18, 800))

# ---------------------------------------------------------------------------------------------------------------
# West: the container yard. Outer mass, the long lane's west wall, and the block between the lane and mid.
LX0, LX1 = -2150, -1700  # long lane
filler(XW, YS - 50, LX0 - 96, 1000, 204, "y")
# Lane west wall: real containers, two high, doors facing the lane.
y = -1780
for L in (480, 480, 240, 480, 480, 240, 480):
    c = rng.choice(PAINT)
    k.container(LX0 - 96, y, 0, L, "y", c)
    if y > -1500:
        k.container(LX0 - 96, y + (0 if L == 480 else 0), 102, L, "y", rng.choice(PAINT))
    y += L + 8
# A few three-high stacks over the outer mass for the skyline.
for (x, y0_, L) in ((-2500, -1200, 480), (-2380, 300, 480), (-2620, -300, 240)):
    k.container(x, y0_, 204, L, "y", rng.choice(PAINT))
# South path wall (containers along x), south of the path "Sued".
filler(LX0 - 96, YS - 50, -650, -1926, 204, "x")
row(-2150, -1926, "x", [480, 480, 480, 240], z=0, colors=["blue", "red", "green", "blue"])
row(-2150, -1926, "x", [480, 480, 240], z=102, colors=["green", "metal_rust", "red"])
# Block between the long lane and mid (x -1700..-450, y -1350..1000), with the cross alley at y -250..-50.
BX0, BX1 = LX1, -450
for (ya, yb) in ((-1350, -250), (-50, 1000)):
    # Lane-facing wall (east side of the lane) and mid-facing wall, real containers.
    yy = ya + 8
    while yb - yy >= 240:
        L = 480 if yb - yy >= 488 else 240
        k.container(BX0, yy, 0, L, "y", rng.choice(PAINT), doors="-")
        k.container(BX0, yy, 102, L, "y", rng.choice(PAINT), doors="-")
        k.container(BX1 - 96, yy, 0, L, "y", rng.choice(PAINT))
        if not (ya < 0 and yy > -700):
            k.container(BX1 - 96, yy, 102, L, "y", rng.choice(PAINT))
        yy += L + 8
    filler(BX0 + 96, ya, BX1 - 96, yb, 204, "x")
# The cross alley walls (containers along x facing the alley), a unit proud of the mass behind.
for yy, d in ((-345, "+"), (-51, "-")):
    row(BX0 + 104, yy, "x", [480, 480], z=0, gap=16, doors=d)
    row(BX0 + 104, yy, "x", [480, 480], z=102, gap=16, doors=d)
# The block's south face (on the south path) and north face (towards A): real containers, two high.
for yy in (-1351, 905):
    row(BX0 + 96, yy, "x", [480, 480], z=0, gap=12)
    row(BX0 + 96, yy, "x", [480, 480], z=102, gap=12)
# Three-high accents in the block.
k.container(-1300, 400, 204, 480, "x", "blue")
k.container(-1100, -900, 204, 480, "x", "red")
# Cover in the long lane: a lone container, crates, a pallet stack.
k.container(LX0 + 40, -700, 0, 240, "y", "red")
k.crate(LX1 - 60, -1100, 0, 56)
k.crate(LX1 - 60, -1044, 0, 48)
k.crate(LX1 - 70, -1070, 56, 40)
k.crate(LX0 + 80, 350, 0, 64)
k.pallet(LX0 + 150, 330, 0, 5)
k.crate(LX1 - 50, 700, 0, 48)
for y in (-1200, 600):
    k.lamp(LX1 - 20, y, 0, 300, "-x")
paint_line(LX0 + 225, -1600, LX0 + 225, 900, 6)

# ---------------------------------------------------------------------------------------------------------------
# South: OPFOR start at the harbour gate, the perimeter wall, the gatehouse.
wall_tex = {"sides": T("concrete_dirty"), "top": T("concrete_dirty"), "bottom": NODRAW, "-y": NODRAW}
k.blk((-650, YS - 24, 0), (950, YS, 144), wall_tex)
k.blk((950, YS - 24, 0), (XE + 20, YS, 144), wall_tex)
k.blk((-650, YS - 30, 144), (XE + 20, YS + 6, 152), T("concrete_dirty"))
for x in range(-600, XE, 256):
    k.dblk((x, YS, 0), (x + 20, YS + 8, 144), T("concrete_dirty"))
# Closed sliding gate in the wall (steel) and the barrier arm.
k.blk((-300, YS - 60, 0), (300, YS - 30, 4), T("metal_plate"))
k.fence(-300, YS - 44, 300, YS - 44, 0, 120, 100, T("metal_painted_blue"), clip_h=800)
k.blk((-340, YS - 70, 0), (-300, YS - 20, 150), T("concrete_dirty"))
k.blk((300, YS - 70, 0), (340, YS - 20, 150), T("concrete_dirty"))
# Gatehouse (Pfoertner): a small brick hut with a flat roof and windows.
k.solid_building(380, YS + 30, 640, YS + 250, 0, 128, T("bricks090"), sides="nw", bays=110, floors=[44], win_w=64,
                 win_h=52, door=("w", YS + 190, 56, 96), plinth=T("concrete_dirty"), cornice=T("concrete_dirty"),
                 top=T("concrete_dirty"), sill=T("concrete_dirty"), frame=T("metal_painted_blue"))
k.blk((370, YS + 20, 128), (650, YS + 260, 140), {"sides": T("metal_painted_blue"), "top": T("gravel"), "bottom": TRIM})
# Barrier arm (up), with its post.
k.blk((220, YS + 250, 0), (250, YS + 280, 44), T("metal_painted_red"))
k.bar((235, YS + 265, 44), (240, YS + 270, 220), 5, T("metal_painted_red"))
# Parked truck at the gate (cover and landmark).
k.truck(-560, -1560, 0, "-y", "metal_painted_red", "container", "blue")
k.crate(470, -1500, 0, 56)
k.crate(530, -1510, 0, 48)
k.crate(500, -1500, 56, 44)
for x in (-200, 200):
    k.lamp(x, -1320, 0, 280, "-y")
paint_line(-420, -1330, 900, -1330, 8)
# West side of the OPFOR yard: containers with the south path exit y -1800..-1400.
row(-746, -1394, "y", [240], z=0, colors=["green"])
row(-746, -1394, "y", [240], z=102, colors=["red"])

# ---------------------------------------------------------------------------------------------------------------
# Mid: the harbour road with a rail siding and freight wagons, the customs house at its north end.
for x in (380, 436):
    k.dblk((x - 3, -1300, 0), (x + 3, 1640, 3), {"top": T("metal_rust"), "sides": NODRAW, "bottom": NODRAW})


def wagon(y0, length, color):
    x0, x1 = 360, 456
    if f"wagon_{color}" in props_info():
        # The modelled wagon (arch.py) over the brush one's shape (invisible collision).
        k.blk((x0 + 4, y0, 16.25), (x1 - 4, y0 + length, 30), NODRAW)
        k.blk((x0, y0 + 8, 30), (x1, y0 + length - 8, 150), NODRAW)
        sc = length / (props_info()[f"wagon_{color}"]["size"][2] * 39.37)
        k.prop(f"wagon_{color}", (x0 + x1) / 2, y0 + length / 2, 0.25, yaw=90, scale=round(sc, 3), solid=False)
        return
    k.blk((x0 + 4, y0, 16.25), (x1 - 4, y0 + length, 30), {"sides": TRIM, "top": NODRAW, "bottom": NODRAW})
    k.blk((x0, y0 + 8, 30), (x1, y0 + length - 8, 150), {"sides": T(f"metal_painted_{color}"), "top": T("metal_rust"), "bottom": NODRAW})
    for yy in (y0 + 70, y0 + length - 70):
        k.wheel(408 - 34, yy, 0, 16, 6, axis="x")
        k.wheel(408 + 34, yy, 0, 16, 6, axis="x")
    for yy in range(int(y0) + 90, int(y0 + length) - 60, 140):
        for face, out in ((x0, -1), (x1, 1)):
            k.panel("y", yy, yy + 6, face, out, 36, 146, 2, T(f"metal_painted_{color}"), show=("front", "ends"))


wagon(-1000, 560, "red")
wagon(-340, 520, "green")
# Customs house ("Zoll"): brick, two storeys, hipped look from a flat roof with parapet.
k.solid_building(-250, 110, 200, 420, 0, 256, T("bricks090"), sides="nsew", bays=90, floors=[40, 160], win_w=44,
                 win_h=64, door=("s", -25, 64, 100), plinth=T("concrete_dirty"), cornice=T("concrete_dirty"),
                 top=T("gravel"), sill=T("concrete_dirty"), frame=T("plaster_white"), cornice_h=16, cross="frame")
k.blk((-262, 98, 256), (212, 110, 280), T("concrete_dirty"))
k.blk((-262, 420, 256), (212, 432, 280), T("concrete_dirty"))
k.blk((-262, 110, 256), (-250, 420, 280), T("concrete_dirty"))
k.blk((200, 110, 256), (212, 420, 280), T("concrete_dirty"))
k.blk((60, 250, 280), (140, 330, 340), {"sides": T("bricks090"), "top": TRIM, "bottom": NODRAW})
# Cover on the road: a stack of pipes, crates, a van.
for i, (py, pz) in enumerate(((-620, 18.25), (-584, 18.25), (-548, 18.25), (-602, 50), (-566, 50))):
    k.add(prism((py, pz), 18, -300, 60, 6, lambda n: T("metal_rust") if abs(n[0]) < 0.5 else TRIM, rot=0, axis="x"))
k.blk((-280, -650, 0), (-270, -520, 8), T("wood_beam"))
k.blk((40, -650, 0), (50, -520, 8), T("wood_beam"))
k.van(-200, 20, 0, "+x", "metal_painted_blue")
k.crate(180, -100, 0, 56)
k.crate(236, -110, 0, 48)
for y in (-500,):
    k.lamp(-420, y, 0, 300, "+x")
paint_line(-40, -1200, -40, -700, 8)
paint_line(-40, -500, -40, 0, 8)
# East strip along the warehouse: a skip, pallets, crates.
k.blk((600, -800, 0), (760, -700, 70), {"sides": T("metal_painted_green"), "top": T("metal_rust"), "bottom": NODRAW})
k.pallet(870, -480, 0, 6, "y")
k.pallet(870, -420, 0, 3, "y")
k.crate(880, 200, 0, 56)
k.crate(880, 144, 0, 48)

# ---------------------------------------------------------------------------------------------------------------
# East: the warehouse ("Lagerhalle"), dark clinker brick. x 1000..2600, y -1050..450. Walls 24, eaves 400.
WX0, WX1, WY0, WY1, WH, WT = 1000, 2600, -1050, 450, 400, 24
brick = {"sides": BRICK, "top": NODRAW, "bottom": NODRAW}
SDOOR, NDOOR = (1250, 1480), (1300, 1530)
DOCKS = ((1850, 2030), (2250, 2430))
WDOOR = (-300, -160)
# South wall: big door and a small door (x 2250..2350); high windows.
win_hi = [(1640, 1768, 250, 370), (1980, 2108, 250, 370), (2440, 2568, 250, 370), (1060, 1188, 250, 370)]
k.wall("x", WX0, WX1, WY0, WY0 + WT, 0, WH, brick, [(*SDOOR, 0, 192), (2250, 2350, 0, 112)] + win_hi)
# North wall: roller door, two closed dock doors, high windows.
win_hn = [(1060, 1188, 260, 370), (1620, 1748, 260, 370), (2080, 2200, 260, 370), (2460, 2568, 260, 370)]
k.wall("x", WX0, WX1, WY1 - WT, WY1, 0, WH, brick, [(*NDOOR, 0, 192)] + [(a_, b_, 48, 240) for a_, b_ in DOCKS] + win_hn)
# West wall: side door to mid; east wall: windows only.
win_w = [(v, v + 128, 250, 370) for v in (-850, -490, 230)]
win_e = []  # the east wall faces the boundary wall (filled gap), no windows
k.wall("y", WY0 + WT, WY1 - WT, WX0, WX0 + WT, 0, WH, brick, [(*WDOOR, 0, 112)] + win_w)
k.wall("y", WY0 + WT, WY1 - WT, WX1 - WT, WX1, 0, WH, brick, win_e)
GREEN = T("metal_painted_green")
for u0, u1, v0, v1 in win_hi:
    k.window("x", WY0, -1, u0, u1, v0, v1, depth=WT, back=None, frame=GREEN, sill=T("concrete_dirty"))
for u0, u1, v0, v1 in win_hn:
    k.window("x", WY1, 1, u0, u1, v0, v1, depth=WT, back=None, frame=GREEN, sill=T("concrete_dirty"))
for v0, v1, a_, b_ in win_w:
    k.window("y", WX0, -1, v0, v1, a_, b_, depth=WT, back=None, frame=GREEN, sill=T("concrete_dirty"))
for v0, v1, a_, b_ in win_e:
    k.window("y", WX1, 1, v0, v1, a_, b_, depth=WT, back=None, frame=GREEN, sill=T("concrete_dirty"))
for u0, u1 in DOCKS:
    k.door_leaf("x", WY1, 1, u0, u1, 48, 240, depth=10, tex=T("metal_corrugated"), frame=T("metal_painted_blue"))
# Exterior articulation: plinth, string course, cornice, pilasters between the openings.
openings = {("x", -1): [SDOOR, (2250, 2350)], ("x", 1): [NDOOR, *DOCKS], ("y", -1): [WDOOR], ("y", 1): []}
for axis, a0, a1, face, out in (("x", WX0, WX1, WY0, -1), ("x", WX0, WX1, WY1, 1), ("y", WY0, WY1, WX0, -1), ("y", WY0, WY1, WX1, 1)):
    ext = 8 if axis == "x" else 0
    k.band(axis, a0 - ext, a1 + ext, face, out, 0, 28, 6, T("concrete_dirty"))
    k.band(axis, a0 - ext - 6, a1 + ext + 6, face, out, WH - 24, WH, 12, BRICK, show=("front", "bottom", "ends"))
    k.band(axis, a0 - ext, a1 + ext, face, out, 220, 230, 5, BRICK, show=("front", "top"))
    wins = {("x", -1): win_hi, ("x", 1): win_hn, ("y", -1): win_w, ("y", 1): win_e}[(axis, out)]
    for u in range(int(a0) + 180, int(a1) - 60, 200):
        blocked = [o for o in openings[(axis, out)] + [(w[0], w[1]) for w in wins] if o[0] - 40 < u + 16 and u < o[1] + 24]
        if blocked:
            continue
        k.band(axis, u, u + 32, face, out, 28, WH - 24, 8, BRICK)
k.band("x", SDOOR[0] - 16, SDOOR[1] + 16, WY0, -1, 192, 208, 8, T("concrete_dirty"))
k.band("x", NDOOR[0] - 16, NDOOR[1] + 16, WY1, 1, 192, 222, 14, T("metal_painted_blue"))
k.band("y", WDOOR[0] - 16, WDOOR[1] + 16, WX0, -1, 112, 124, 6, T("concrete_dirty"))
k.band("x", 2234, 2366, WY0, -1, 112, 124, 6, T("concrete_dirty"))
# Name board over the north roller door.
k.panel("x", NDOOR[0], NDOOR[1], WY1, 1, 250, 300, 3, GREEN, show=("front", "top", "bottom", "ends"))
k.panel("x", NDOOR[0] + 12, NDOOR[1] - 12, WY1 + 3, 1, 262, 288, 1, T("plaster_white"))
for x in (WX0 + 40, WX1 - 40):
    for yface, out in ((WY0, -1), (WY1, 1)):
        k.dblk((x - 4, yface + out * 12 - 4, 0), (x + 4, yface + out * 12 + 4, WH - 10), {"sides": T("metal_plate"), "top": NODRAW, "bottom": NODRAW})
# Ceiling and a shallow gabled roof over the hall; trusses under the ceiling.
k.blk((WX0, WY0, WH), (WX1, WY1, WH + 16), {"sides": BRICK, "bottom": T("metal_corrugated"), "top": NODRAW})
# Flat roof behind a brick parapet with a stepped gable in the middle of the north and south fronts.
for yface, out in ((WY0, -1), (WY1, 1)):
    ya, yb = sorted((yface, yface - out * 20))
    k.blk((WX0, ya, WH + 16), (WX1, yb, WH + 48), {"sides": BRICK, "top": T("concrete_dirty"), "bottom": NODRAW})
    for i, (xa, xb, h) in enumerate(((1500, 2100, 96), (1620, 1980, 150), (1720, 1880, 196))):
        k.blk((xa, ya, WH + 48), (xb, yb, WH + 48 + h), {"sides": BRICK, "top": T("concrete_dirty"), "bottom": NODRAW})
for xface, out in ((WX0, -1), (WX1, 1)):
    xa, xb = sorted((xface, xface - out * 20))
    k.blk((xa, WY0 + 20, WH + 16), (xb, WY1 - 20, WH + 48), {"sides": BRICK, "top": T("concrete_dirty"), "bottom": NODRAW})
for x in range(WX0 + 250, WX1 - 100, 380):
    k.dblk((x, WY0 + WT, WH - 28), (x + 12, WY1 - WT, WH), {"sides": T("metal_painted_blue"), "top": NODRAW, "bottom": T("metal_painted_blue"), "+y": NODRAW, "-y": NODRAW})
# Interior: pillars, racks, an office on a mezzanine along the east wall.
for x in (1450, 1950):
    for y in (-650, -150, 300):
        k.blk((x - 14, y - 14, 0), (x + 14, y + 14, WH), {"sides": T("concrete_dirty"), "top": NODRAW, "bottom": NODRAW})


def rack(x0, y0, x1, y1, h=256):
    """A pallet rack: a solid core (plated back), blue uprights and red beams on the long faces, a few crates."""
    along_x = (x1 - x0) > (y1 - y0)
    k.blk((x0 + 8, y0 + 8, 0), (x1 - 8, y1 - 8, h - 8), {"sides": T("metal_plate"), "top": NODRAW, "bottom": NODRAW})
    L = (x1 - x0) if along_x else (y1 - y0)
    n = max(1, int(L // 192))
    faces = (("x", y0 + 8, -1), ("x", y1 - 8, 1)) if along_x else (("y", x0 + 8, -1), ("y", x1 - 8, 1))
    for axis, face, out in faces:
        for i in range(n + 1):
            u = (x0 if along_x else y0) + 8 + (L - 16) * i / n
            k.panel(axis, u - 4, u + 4, face, out, 0, h, 8, T("metal_painted_blue"))
        for lz in (88, 176):
            k.panel(axis, (x0 if along_x else y0) + 8, (x1 if along_x else y1) - 8, face, out, lz - 7, lz, 8, T("metal_painted_red"), show=("front", "top"))
    for i in range(n):
        u = (x0 if along_x else y0) + 8 + (L - 16) * (i + 0.5) / n
        for lz in (0, 88):
            sz = rng.choice((44, 52, 0))
            if not sz:
                continue
            side = rng.choice((0, 1))
            if along_x:
                k.crate(u, (y0 - sz / 2 + 4) if side == 0 else (y1 + sz / 2 - 4), lz, sz)
            else:
                k.crate((x0 - sz / 2 + 4) if side == 0 else (x1 + sz / 2 - 4), u, lz, sz)


rack(1560, -860, 1680, -100)
rack(1560, 60, 1680, 330)
rack(1150, -860, 1270, -420)
rack(2020, -860, 2140, -320)
# Office mezzanine (x 2250..2576, y -450..300) at z 160, stairs up from the south, railings, the office box.
MZ = 160
EX = WX1 - WT
k.blk((2250, -450, MZ - 12), (EX, 300, MZ), {"top": T("metal_plate"), "sides": T("metal_painted_blue"), "bottom": T("metal_plate")})
for x, y in ((2256, -444), (2256, 294), (2256, -80)):
    k.blk((x - 6, y - 6, 0), (x + 6, y + 6, MZ - 12), {"sides": T("metal_painted_blue"), "top": NODRAW, "bottom": NODRAW})
k.add(stairs((2266, -770, 0), (2360, -450, MZ), "+y", 10, {"top": T("metal_plate"), "sides": T("metal_painted_blue"), "bottom": NODRAW}))
k.railing(2250, -450, 2250, 300, MZ, 42, T("metal_painted_blue"))
k.railing(2360, -450, EX, -450, MZ, 42, T("metal_painted_blue"))
OZ = 290
k.wall("y", -100, 300, 2380, 2392, MZ, OZ, {"sides": T("plaster_white"), "top": T("plaster_white"), "bottom": NODRAW}, [(-40, 40, MZ, MZ + 100), (100, 260, MZ + 40, MZ + 100)])
k.wall("x", 2380, EX, -112, -100, MZ, OZ, {"sides": T("plaster_white"), "top": T("plaster_white"), "bottom": NODRAW})
k.blk((2380, -112, OZ), (EX, 300, OZ + 8), {"sides": T("plaster_white"), "top": T("metal_plate"), "bottom": T("ceiling_tiles")})
k.pane((2384, 100, MZ + 40), (2388, 260, MZ + 100))
k.blk((2420, 180, MZ), (2520, 230, MZ + 30), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})  # desk
# Crates and a forklift in the hall aisles.
k.forklift(1820, -500, 0, "+y")
k.crate(1330, -250, 0, 56)
k.crate(1386, -240, 0, 48)
k.crate(1360, -245, 56, 40)
k.pallet(2000, 150, 0, 4)
k.crate(2100, -960, 0, 64)
k.crate(1100, 350, 0, 48)
k.crate(1150, 380, 0, 40)

# Forecourt south of the warehouse ("Vorplatz"): trucks at the docks, a raised dock along the south wall.
k.blk((1600, WY0 - 180, 0), (2150, WY0, 48), {"top": CONC, "sides": T("concrete_dirty"), "bottom": NODRAW})
k.blk((1600, WY0 - 184, 44), (2150, WY0 - 180, 48), T("metal_rust"))
k.add(stairs((1540, WY0 - 180, 0), (1600, WY0 - 60, 48), "+x", 3, {"top": CONC, "sides": T("concrete_dirty"), "bottom": NODRAW}))
k.truck(2350, -1990, 0, "-y", "metal_painted_blue", "container", "red")
k.crate(1300, -1650, 0, 64)
k.crate(1240, -1640, 0, 48)
k.pallet(1060, -1500, 0, 4)
k.lamp(1100, -1900, 0, 300, "+y")
# East boundary wall of the forecourt and B (a brick wall with the neighbour's granary behind).
k.blk((XE, YS, 0), (XE + 24, QY - 10, 200), {"sides": T("bricks090"), "top": T("concrete_dirty"), "bottom": NODRAW})
# The strip between the warehouse and the boundary wall is closed off (no hidden flank).
k.blk((WX1, WY0 + 1, 0), (XE, WY1 - 1, 200), {"sides": T("bricks090"), "top": NODRAW, "bottom": NODRAW})
for y in range(YS, QY - 100, 300):
    k.blk((XE - 8, y, 0), (XE, y + 32, 200), T("bricks090"))
k.blk((XE - 10, YS, 200), (XE + 30, QY - 10, 210), T("concrete_dirty"))

# ---------------------------------------------------------------------------------------------------------------
# Site B: loading docks north of the warehouse, the quay's east end.
k.blk((1750, WY1, 0), (XE, WY1 + 200, 48), {"top": CONC, "sides": T("concrete_dirty"), "bottom": NODRAW})
k.blk((1750, WY1 + 196, 40), (XE, WY1 + 204, 48), T("metal_rust"))
k.add(stairs((1640, WY1 + 20, 0), (1750, WY1 + 180, 48), "+x", 3, {"top": CONC, "sides": T("concrete_dirty"), "bottom": NODRAW}))
k.railing(1760, WY1 + 196, 1840, WY1 + 196, 48, 40, T("metal_painted_blue"))
for x in (1950, 2350):
    k.blk((x - 60, WY1 + 200, 0), (x + 60, WY1 + 214, 40), TRIM)  # dock bumpers
# Cover on B: containers (one climbable via crates), crate stacks, a trailer.
k.container(2554, 900, 0, 480, "y", "red", doors="-")
k.crate(2500, 1000, 0, 48)
k.crate(2500, 1060, 0, 40)
k.container(1500, 1500, 0, 240, "x", "metal_rust")
k.container(1500, 1500, 102, 240, "x", "red")
k.crate(1800, 1050, 0, 64)
k.crate(1864, 1050, 0, 48)
k.crate(1830, 1052, 64, 48)
k.crate(2150, 850, 0, 56)
k.pallet(2100, 1300, 0, 5)
k.pallet(2160, 1300, 0, 2)
k.forklift(2300, 1250, 0, "-x")
k.crate(1350, 900, 0, 48)
k.crate(1300, 920, 0, 40)
# B's west side: containers between the crane area and B, with the gap y 700..1000.
k.container(1150, 452, 0, 240, "y", "red")
k.container(1150, 452, 102, 240, "y", "metal_rust")
k.container(1150, 1000, 0, 480, "y", "metal_rust")
k.container(1150, 1000, 102, 480, "y", "red")
k.container(1150, 1488, 0, 160, "y", "red")
for x in (1400, 2300):
    k.lamp(x, 1600, 0, 300, "-y")

# ---------------------------------------------------------------------------------------------------------------
# Crane area ("Kran"): the gantry crane over the rails, containers under it.
for x in (-600, 600):
    k.dblk((x - 4, 450, 0), (x + 4, QY - 24, 3), {"top": T("metal_rust"), "sides": NODRAW, "bottom": NODRAW})
k.container(-400, 850, 0, 480, "x", "red")
k.container(-400, 850, 102, 480, "x", "blue")
k.container(160, 1150, 0, 240, "x", "green")
k.crate(-520, 1250, 0, 56)
k.crate(-464, 1260, 0, 48)
k.crate(700, 700, 0, 64)
k.crate(760, 690, 0, 48)
# Jersey barriers between the crane area and the KSK start.
for x0 in (-700, -250, 250):
    k.add(hull([(x0, 1600, 0), (x0 + 320, 1600, 0), (x0, 1632, 0), (x0 + 320, 1632, 0),
                (x0, 1610, 32), (x0 + 320, 1610, 32), (x0, 1622, 32), (x0 + 320, 1622, 32)], T("concrete_dirty")))
# The gantry: four lattice legs, portal girders, a boom, trolley, cab and a hanging container.
RED, STEEL = T("metal_rusty_paint"), T("metal_plate")
CZ = 600  # girder bottom


def lattice_leg(cx, cy, z0, z1, w=64):
    h = w / 2
    for dx in (-h, h):
        for dy in (-h, h):
            k.blk((cx + dx - 5, cy + dy - 5, z0), (cx + dx + 5, cy + dy + 5, z1), {"sides": RED, "top": RED, "bottom": NODRAW})
    step = 192
    zz = z0 + 24
    flip = False
    while zz + step <= z1:
        for (ax, ay, bx, by) in ((-h, -h, h, -h), (h, -h, h, h), (h, h, -h, h), (-h, h, -h, -h)):
            a = (cx + ax, cy + ay, zz + (step if flip else 0))
            b = (cx + bx, cy + by, zz + (0 if flip else step))
            k.bar(a, b, 4, RED)
        zz += step
        flip = not flip


def girder(x0, y0, x1, y1, z0, h=64, w=48):
    """A plated box girder along x or y with a few stiffener ribs."""
    along_x = abs(x1 - x0) > abs(y1 - y0)
    if along_x:
        k.blk((x0, y0 - w / 2, z0), (x1, y0 + w / 2, z0 + h), RED)
    else:
        k.blk((x0 - w / 2, y0, z0), (x0 + w / 2, y1, z0 + h), RED)


for x in (-600, 600):
    for y in (650, 1450):
        k.blk((x - 48, y - 48, 0), (x + 48, y + 48, 24), {"sides": STEEL, "top": STEEL, "bottom": NODRAW})  # bogie
        k.wheel(x, y - 30, 0, 12, 8, axis="x")
        lattice_leg(x, y, 24, CZ)
    girder(x, 650, x, 1450, CZ, 72, 64)  # sill beams along y
girder(-900, 650, 900, 650, CZ + 72, 80, 64)
girder(-900, 1450, 900, 1450, CZ + 72, 80, 64)
# Boom: a deck across the portal (plated), overhanging west over A's side.
girder(-1200, 1050, 1000, 1050, CZ + 152, 72, 110)
# A lattice A-frame on top of the boom.
for x in (-1100, -500, 100, 700):
    k.bar((x, 1050, CZ + 224), (x + 300, 1050, CZ + 330), 6, RED)
    k.bar((x + 300, 1050, CZ + 330), (x + 600, 1050, CZ + 224), 6, RED)
k.blk((-1100, 1044, CZ + 326), (1000, 1056, CZ + 336), RED)
k.blk((-940, 640, CZ + 72), (-860, 1460, CZ + 152), {"sides": RED, "top": RED, "bottom": RED})
k.blk((860, 640, CZ + 72), (940, 1460, CZ + 152), {"sides": RED, "top": RED, "bottom": RED})
# Machine house on top, the operator cab under the boom, trolley and spreader with a container.
k.blk((600, 980, CZ + 236), (900, 1120, CZ + 330), {"sides": T("plaster_white"), "top": STEEL, "bottom": NODRAW})
k.blk((620, 978, CZ + 290), (880, 982, CZ + 310), TRIM)
k.blk((180, 990, CZ + 70), (280, 1110, CZ + 152), {"sides": T("plaster_white"), "top": STEEL, "bottom": STEEL})
k.pane((178, 995, CZ + 90), (182, 1105, CZ + 140))
k.blk((-120, 980, CZ + 128), (40, 1120, CZ + 152), STEEL)  # trolley
for dx in (-90, 10):
    k.bar((dx - 20, 1030, CZ + 128), (dx - 20, 1050, 420), 3, TRIM)
    k.bar((dx + 10, 1070, CZ + 128), (dx + 10, 1050, 420), 3, TRIM)
k.blk((-280, 1010, 408), (200, 1090, 420), {"sides": T("metal_painted_blue"), "top": T("metal_painted_blue"), "bottom": T("metal_painted_blue")})
k.container(-280, 1002, 306, 480, "x", "green")
# Stairs up a leg (visual ladder cage).
for z in range(40, CZ, 48):
    k.blk((-560, 690, z), (-548, 720, z + 4), STEEL)

# ---------------------------------------------------------------------------------------------------------------
# KSK start on the quay, bollards along the edge, lamp posts.
for x in range(-2500, 2600, 500):
    k.bollard(x, QY - 40)
for x in (-900, 0, 900):
    k.lamp(x, QY - 120, 0, 320, "-y")
k.crate(-600, 2000, 0, 48)
k.crate(640, 1780, 0, 56)
# Gangway to the ship (visual, blocked by the quay clip).
k.add(hull([(80, QY + 20, 40), (200, QY + 20, 40), (80, QY + 320, 170), (200, QY + 320, 170),
            (80, QY + 20, 46), (200, QY + 20, 46), (80, QY + 320, 176), (200, QY + 320, 176)], STEEL))
k.blk((80, QY - 40, 0), (200, QY + 20, 46), {"sides": STEEL, "top": STEEL, "bottom": NODRAW})
k.railing(80, QY - 40, 80, QY + 20, 46, 40, T("metal_painted_blue"))
k.railing(200, QY - 40, 200, QY + 20, 46, 40, T("metal_painted_blue"))

# ---------------------------------------------------------------------------------------------------------------
# Site A: the west end of the quay. Stacked site office with outside stairs (the high ground), cover.
AX0, AX1 = -2600, -1150
row(-2700, 1000, "y", [480], z=0, colors=["green"])
k.container(-2600, 1600, 0, 480, "x", "metal_corrugated", doors="+")  # lower row (balcony on top)
k.container(-2600, 1700, 0, 480, "x", "metal_corrugated", doors="+")
k.container(-2600, 1700, 102, 480, "x", "blue", doors="+")  # upper office container
# Office windows (with white frames) and a door on the upper container's south face.
for x in (-2540, -2440, -2340):
    k.flat_window("x", 1700, -1, x, x + 64, 145, 185, frame=T("plaster_white"), cross="frame")
k.panel("x", -2250, -2206, 1700, -1, 106, 190, 1, T("metal_plate"))
k.panel("x", -2560, -2300, 1700, -1, 190, 200, 1, T("plaster_white"))  # name strip
k.railing(-2600, 1604, -2125, 1604, 102, 40, T("metal_painted_blue"))
k.add(stairs((-2120, 1604, 0), (-1990, 1696, 102), "-x", 6, {"top": T("metal_plate"), "sides": T("metal_painted_blue"), "bottom": NODRAW}))
# Container wall on A's east side (to the crane area), gap y 1150..1450.
k.container(-1246, 1000, 0, 150, "y", "green")
k.container(-1246, 1450, 0, 200, "y", "blue")
k.container(-1246, 1450, 102, 200, "y", "green")
k.container(-1150, 450, 0, 480, "y", "blue")
k.container(-1150, 450, 102, 480, "y", "green")
k.container(-1150, 938, 0, 160, "y", "blue")
# Cover on A.
k.container(-1850, 1350, 0, 240, "y", "blue")
k.crate(-1650, 1500, 0, 64)
k.crate(-1586, 1500, 0, 48)
k.crate(-1620, 1500, 64, 48)
k.crate(-2400, 1300, 0, 56)
k.crate(-2344, 1310, 0, 48)
k.pallet(-1500, 1850, 0, 5)
k.pallet(-1440, 1850, 0, 2)
k.crate(-1790, 1650, 0, 48)  # climb onto the lone container (102): 48 then crouch-jump
# A two-high stack at the lane exit (corners to fight around) and a harbour master's hut by the water.
k.container(-2110, 1250, 0, 240, "x", "green")
k.container(-2110, 1250, 102, 240, "x", "blue")
k.solid_building(-1400, 1930, -1180, 2080, 0, 120, T("plaster_white"), sides="nswe", bays=90, floors=[46], win_w=56,
                 win_h=44, door=("w", 2005, 44, 90), plinth=T("concrete_dirty"), top=T("gravel"), sill=T("plaster_white"),
                 frame=T("metal_painted_blue"), cross="frame", door_tex=T("metal_painted_blue"))
k.blk((-1412, 1918, 120), (-1168, 2092, 130), {"sides": T("metal_painted_blue"), "top": T("gravel"), "bottom": TRIM})
# A motor boat on a trailer (landmark "Boot").
BX, BY = -2400, 1380
k.add(hull([(BX - 170, BY - 50, 34), (BX + 120, BY - 55, 34), (BX + 210, BY, 50), (BX + 120, BY + 55, 34), (BX - 170, BY + 50, 34),
            (BX - 160, BY - 58, 90), (BX + 130, BY - 62, 90), (BX + 240, BY, 100), (BX + 130, BY + 62, 90), (BX - 160, BY + 58, 90)],
           lambda n: T("plaster_white") if n[2] < 0.9 else T("wood_planks")))
k.dblk((BX - 160, BY - 58.5, 78), (BX + 130, BY - 57, 88), {"-y": T("metal_painted_blue"), "sides": NODRAW, "top": NODRAW, "bottom": NODRAW})
k.blk((BX - 120, BY - 40, 90), (BX + 20, BY + 40, 140), {"sides": T("plaster_white"), "top": T("plaster_white"), "bottom": NODRAW})
k.panel("x", BX - 110, BX + 10, BY - 40, -1, 110, 132, 1, TRIM)
k.blk((BX - 200, BY - 30, 14), (BX + 260, BY + 30, 30), {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW})
for wx in (BX - 60, BX + 20):
    k.wheel(wx, BY - 40, 0.25, 14, 8, axis="y")
    k.wheel(wx, BY + 40, 0.25, 14, 8, axis="y")
for x in (-1500,):
    k.lamp(x, 1150, 0, 300, "+y")

# ---------------------------------------------------------------------------------------------------------------
# The frigate alongside (unreachable backdrop). Faces turned away from the quay are not drawn.
# The catalogue's F124 (arch.py veh_f124) where it is built: the brush ship below stays as invisible geometry (the
# same sightlines and cover as before), drawn by the model.
SHIP = "veh_f124" in props_info()
GREY = NODRAW if SHIP else T("concrete_dirty")  # light naval grey superstructure
HULL = NODRAW if SHIP else TRIM  # dark grey hull
PAINT_RED, WHITE, DIRTY, DARK, CORR = ((NODRAW,) * 5 if SHIP else (T("metal_painted_red"), T("plaster_white"), T("concrete_dirty"), TRIM, T("metal_corrugated")))


def ship_tex(n):
    if n[1] > 0.5:
        return NODRAW
    if n[2] < -0.3:
        return NODRAW
    return HULL


HY0, HY1 = QY + 330, QY + 930
HYM = (HY0 + HY1) / 2
DECK = 140
SX0, SX1, BOW = -2500, 1500, 2350  # stern, end of the parallel hull, bow tip
k.add(hull([(SX0, HY0, DECK), (SX1, HY0, DECK), (SX0, HY1, DECK), (SX1, HY1, DECK), (BOW, HYM, DECK + 50),
            (SX0, HY0 + 30, WATER - 16), (SX1, HY0 + 30, WATER - 16), (SX0, HY1 - 30, WATER - 16),
            (SX1, HY1 - 30, WATER - 16), (BOW - 250, HYM, WATER - 16)], lambda n: NODRAW if n[2] > 0.9 else ship_tex(n), scale=1.0))
# The deck: plates over the parallel part (split along x by the kit), the bow's own top.
k.blk((SX0, HY0, DECK - 2), (SX1, HY0 + 120, DECK), {"top": GREY, "sides": NODRAW, "bottom": NODRAW})  # deck edge strip (the rest is above every eye)
k.add(hull([(SX1 + 2, HY0 + 1, DECK + 1), (SX1 + 2, HY1 - 1, DECK + 1), (BOW, HYM, DECK + 49), (SX1 + 2, HY0 + 1, DECK + 3), (SX1 + 2, HY1 - 1, DECK + 3), (BOW, HYM, DECK + 51)],
           lambda n: GREY if n[2] > 0.3 else NODRAW))
# Boot topping (a dark band at the waterline) and the deck edge.
k.dblk((SX0 + 20, HY0 + 18, WATER - 4), (SX1, HY0 + 26, WATER + 22), {"-y": PAINT_RED, "sides": NODRAW, "top": NODRAW, "bottom": NODRAW})
k.dblk((SX0, HY0 - 3, DECK), (SX1, HY0 + 1, DECK + 28), {"-y": GREY, "sides": NODRAW, "top": GREY, "bottom": NODRAW})
# Hull number and a row of scuttles on the quay side.
for i, x in enumerate((1150, 1230, 1310)):
    k.panel("x", x, x + 60, HY0 + 12, -1, 20, 120, 1, WHITE)
for x in range(-2300, 1400, 150):
    k.panel("x", x, x + 14, HY0 + 8, -1, 70, 84, 1, DIRTY)


def sup(x0, x1, y0, y1, z0, z1, tex=GREY):
    k.blk((x0, y0, z0), (x1, y1, z1), {"sides": tex, "top": tex, "bottom": NODRAW, "+y": NODRAW})


# Hangar and aft superstructure, main superstructure, bridge, funnel, mast, forward gun.
sup(-1500, -200, HY0 + 60, HY1 - 60, DECK, DECK + 250)
sup(-200, 1100, HY0 + 70, HY1 - 70, DECK, DECK + 290)
sup(600, 1150, HY0 + 90, HY1 - 90, DECK + 290, DECK + 420)  # bridge
k.panel("x", 600, 1150, HY0 + 90, -1, DECK + 360, DECK + 400, 1, DARK)  # bridge windows
k.panel("x", -1400, -300, HY0 + 60, -1, DECK + 20, DECK + 200, 1, CORR)  # hangar door
sup(0, 400, HY0 + 170, HY1 - 170, DECK + 290, DECK + 500)  # funnel
k.blk((20, HY0 + 190, DECK + 500), (380, HY1 - 190, DECK + 518), {"sides": DARK, "top": DARK, "bottom": NODRAW, "+y": NODRAW})
sup(760, 1000, HY0 + 220, HY1 - 220, DECK + 420, DECK + 900)  # mast
sup(730, 1030, HY0 + 200, HY1 - 200, DECK + 900, DECK + 980)
k.post(880, HYM, DECK + 980, DECK + 1300, 10, GREY)
k.panel("x", 780, 980, HY0 + 220, -1, DECK + 700, DECK + 820, 1, DARK)  # radar face
for x in range(-100, 1000, 180):
    k.panel("x", x, x + 60, HY0 + 70, -1, DECK + 170, DECK + 205, 1, DARK)
k.add(prism((1500, HYM), 80, DECK + 20, DECK + 90, 8, {"sides": GREY, "top": GREY, "bottom": NODRAW}, rot=22.5))
k.bar((1560, HYM, DECK + 60), (1900, HYM, DECK + 85), 12, GREY)
k.blk((-1100, HY0 + 20, DECK + 190), (-800, HY0 + 60, DECK + 240), {"sides": PAINT_RED, "top": PAINT_RED, "bottom": NODRAW, "+y": NODRAW})

if SHIP:
    # Scaled to the brush hull's length, its waterline at the water, the bow towards +x.
    sc = (BOW - SX0) / (props_info()["veh_f124"]["size"][2] * 39.37)
    k.prop("veh_f124", (SX0 + BOW) / 2, HYM, WATER + props_info()["veh_f124"]["bottom"] * sc * 39.37, yaw=0, scale=round(sc, 4), solid=False)

# ---------------------------------------------------------------------------------------------------------------
# Backdrop: the granary east of B, an office block east of the forecourt, the port office south of the gate.
k.solid_building(XE + 30, 700, XE + 600, 2000, 0, 600, T("bricks090"), sides="w", bays=150, floors=[140, 300, 460],
                 win_w=48, win_h=72, plinth=T("concrete_dirty"), cornice=T("bricks090"), top=NODRAW, sill=T("concrete_dirty"),
                 frame=TRIM, cross=False)
k.gable_roof(XE + 30, 700, XE + 600, 2000, 600, 200, axis="y", tex=T("roof_tiles"), gable=T("bricks090"), over=16, hide="e")
k.solid_building(XE + 30, -1700, XE + 400, -800, 0, 340, T("plaster_old"), sides="w", bays=170, floors=[60, 200],
                 win_w=56, win_h=80, plinth=T("concrete_dirty"), cornice=T("concrete_dirty"), top=T("gravel"),
                 sill=T("concrete_dirty"), frame=T("plaster_white"), cross=False)
k.solid_building(-900, YS - 450, 1400, YS - 60, 0, 400, T("concrete_dirty"), sides="n", bays=190, floors=[180, 300],
                 win_w=120, win_h=72, plinth=T("concrete_dirty"), cornice=T("concrete_dirty"), top=T("gravel"),
                 sill=T("concrete_dirty"), frame=TRIM, cross=False)
# A far container gantry silhouette over the water to the west.
for y in (2900, 3300):
    k.blk((-3400, y - 20, WATER), (-3360, y + 20, 900), {"sides": RED, "top": RED, "bottom": NODRAW, "-x": NODRAW})
    k.blk((-3000, y - 20, WATER), (-2960, y + 20, 900), {"sides": RED, "top": RED, "bottom": NODRAW, "-x": NODRAW})
k.blk((-3420, 2860, 900), (-2940, 3340, 960), {"sides": RED, "top": RED, "bottom": NODRAW})
k.blk((-3200, 2300, 960), (-3160, 3500, 1000), {"sides": RED, "top": RED, "bottom": NODRAW})

# ---------------------------------------------------------------------------------------------------------------
# Keep players in: clip walls on the open boundaries and a lid.
k.clip((XW - 40, YS - 40, 0), (XW, QY + 18, 900))
k.clip((XE, YS - 40, 0), (XE + 40, QY + 18, 900))
k.clip((XW, YS - 40, 0), (XE, YS - 24, 900))
k.clip((XW, YS - 40, 900), (XE, QY + 18, 916))

# ---------------------------------------------------------------------------------------------------------------
# Spawns, buy zones, sites, callouts, sun.
spawns(m, "info_player_opfor", [(-240, -1700, 0), (-120, -1780, 0), (0, -1700, 0), (120, -1780, 0), (240, -1700, 0)], 90)
spawns(m, "info_player_ksk", [(-260, 1900, 0), (-130, 1980, 0), (0, 1900, 0), (130, 1980, 0), (260, 1900, 0)], 270)
m.volume("func_buyzone", (-650, YS, 0), (650, -1320, 160), team="opfor")
m.volume("func_buyzone", (-750, 1650, 0), (750, QY, 160), team="ksk")
m.volume("func_bomb_target", (-2000, 1150, 0), (-1350, 1800, 128), site="A")
m.volume("func_bomb_target", (1350, 700, 0), (2050, 1400, 128), site="B")
dm = [(-2000, -1500, 90), (-1900, -600, 90), (-1950, 500, 270), (-2300, 1540, 0), (-1500, 2000, 0), (-1000, 1300, 0),
      (-300, 1300, 270), (700, 1300, 180), (900, -200, 90), (-100, -900, 90), (500, -1700, 180), (1300, -1600, 90),
      (1800, -300, 90), (2400, 0, 180), (2000, 1600, 270), (1400, 2000, 180)]
for x, y, a in dm:
    m.point("info_player_dm", (x, y, 4), angle=a)
places(m, [
    ("OPFOR-Start", (-650, YS, 0), (650, -1300, 400)),
    ("Süd", (-2150, -1830, 0), (-650, -1350, 400)),
    ("Lange", (LX0, -1830, 0), (LX1, 1000, 400)),
    ("Querweg", (LX1, -350, 0), (-450, 50, 400)),
    ("Mitte", (-450, -1300, 0), (950, 450, 400)),
    ("Gleis", (340, -1300, 0), (480, 450, 400)),
    ("Zoll", (-450, 0, 0), (360, 450, 400)),
    ("Hallenseite", (480, -1300, 0), (950, 450, 400)),
    ("Kran", (-1150, 450, 0), (1150, 1650, 400)),
    ("KSK-Start", (-750, 1650, 0), (750, QY, 400)),
    ("Kai West", (-1150, 1650, 0), (-750, QY, 400)),
    ("Kai Ost", (750, 1650, 0), (1150, QY, 400)),
    ("A", (AX0 - 100, 1000, 0), (-1150, QY, 400)),
    ("Büro", (-2600, 1600, 0), (-1990, 1800, 400)),
    ("Vorplatz", (650, YS, 0), (XE, WY0, 400)),
    ("Lagerhalle", (WX0, WY0, 0), (WX1, WY1, 460)),
    ("Empore", (2250, -450, MZ - 12), (WX1, 300, 460)),
    ("B", (1150, WY1, 0), (XE, QY, 400)),
    ("Rampe", (1640, WY1, 0), (XE, WY1 + 200, 400)),
])
m.point("light_environment", (0, 0, 800), pitch="-38", yaw="55", color="255 238 214", ambient="122 138 164")

print("hidden tops:", k.hide_tops(180))

# ---------------------------------------------------------------------------------------------------------------
# Dressing: models and markings that make the harbour look worked in (clear of the solids; against walls and edges,
# so the lanes stay as they were).
placed = 0
# OPFOR yard: road barriers along the perimeter wall, a bin and bags by the gatehouse.
placed += k.props_at("concrete_road_barrier", [(-590, YS + 34, 90), (-470, YS + 34, 90), (560, YS + 300, 90)])
placed += k.props_at("metal_trash_can", [(340, YS + 60, 90)])
placed += k.props_at("trashbag", [(310, YS + 50, 30), (300, YS + 90, 200)])
# Mid road: kerbside clutter along the warehouse's west wall.
placed += k.props_at("hand_truck", [(WX0 - 30, -620, 180)])
placed += k.props_at("propane_tank", [(WX0 - 20, -420, 0), (WX0 - 22, -395, 40), (WX0 - 45, -410, 10)])
placed += k.props_at("old_tyre", [(WX0 - 40, 80, 0), (WX0 - 42, 82, 30)])
placed += k.props_at("metal_jerrycan_green", [(WX0 - 18, 150, 180), (WX0 - 18, 168, 175)])
# Warehouse: shelving along the walls, lamps under the ceiling.
placed += k.props_at("steel_frame_shelves_01", [(WX0 + WT + 12, y, 0) for y in (-900, -780, 60, 180, 300)])
placed += k.props_at("worn_metal_rack", [(x, WY0 + WT + 14, 90) for x in (1650, 1760, 2000, 2110)])
placed += k.props_at("hanging_industrial_lamp", [(x, y, 0) for x in (1440, 1820, 2200) for y in (-800, -300, 200)],
                     z=WH - 56)
placed += k.props_at("industrial_storage_cart", [(1200, -450, 90)])
placed += k.props_at("portable_generator", [(2480, -980, 180)])
# Quay: buoys riding in the water past the quay wall, a lifebuoy stand's worth of kit.
for x, y in ((-1800, QY + 520), (1900, QY + 640)):
    k.prop("ocean_buoy", x, y, WATER - 32, yaw=x % 360)
    placed += 1
placed += k.props_at("wooden_military_crate", [(-1080, 1700, 0), (1080, 1720, 180)])
# Site A: kit by the office wall.
placed += k.props_at("barrel_01", [(-1965, 1640, 0), (-1962, 1668, 50)])
placed += k.props_at("barrel_03", [(-1960, 1700, 20)])
placed += k.props_at("utility_box_02", [(-2300, 1580, 270)])
print(f"harbour dressing: {placed} models")
# Signs and markings.
k.decal("sign_hafenbetrieb_nord", ((NDOOR[0] + NDOOR[1]) / 2, WY1 + 4.5, 275), "+y", size=(4.6, 0.46))
k.decal("stencil_halle_1", (2050, WY0, 300), "-y", size=(3.4, 1.04))
k.decal("sign_betreten_verboten", (2200, WY0, 150), "-y")
k.decal("sign_rauchen_verboten", (2400, WY0, 150), "-y")
k.decal("sign_kai_4", (2500, WY1, 290), "+y", size=(1.6, 0.69))
k.decal("sign_notausgang", (WX0, (WDOOR[0] + WDOOR[1]) / 2, 140), "-x")
k.decal("stop_line", (0, YS + 420, 0), "+z", size=(4.0, 1.9))
for y in (-1000, -400, 200):
    k.decal("floor_arrow", (160, y, 0), "+z")
for x in (-600, -200, 200, 600):
    k.decal("hazard_stripes", (x, QY - 30, 0), "+z", size=(3.0, 0.29))
k.dress(budget=200)
k.finish()

# Sightline check: the spawns must not see each other.
seen = [(o, c) for o in [(-240 + 120 * i, -1700 - 80 * (i % 2), 64) for i in range(5)]
        for c in [(-260 + 130 * i, 1900 + 80 * (i % 2), 64) for i in range(5)] if k.visible(o, c)]
if seen:
    print("WARNING: spawns see each other:", seen[:4])

# The south-west corner by the gate: ground.
k.ground(XW, YS, -2000, -1600, ASPH, 640)
k.safety_floor(XW - 200, YS - 200, XE + 200, 3200)
m.save(os.path.join(os.path.dirname(__file__), "../../assets/maps/zg_hafen.map"))
