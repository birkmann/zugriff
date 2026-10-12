"""zg_altstadt: bomb defusal in a southern German old town.

South: the town gate ("Unteres Tor") with the OPFOR start. North: the town hall square ("Rathausplatz") with the KSK
start. West lane: the long, narrow "Lange Gasse" (one long sightline) to site A on the church square ("Kirchplatz",
onion-dome tower, fountain). Mid: "Marktstraße" to the market square ("Markt"), with the "Schmiedgasse" cross alley to
the Lange Gasse, "Kirchgasse" to A, the "Rathausgasse" under an arch to the KSK start and the "Bäckergasse" to B.
East lane: "Mauergasse" and the back yards ("Hinterhof") through the arched brewery gate ("Brauereitor") to site B in
the brewery yard ("Brauerei": barrels, carts, a loading ramp, the brewhouse chimney).

    python3 tools/maps/zg_altstadt.py
"""

import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(__file__))
from zmap import Map, hull, prism, stairs  # noqa: E402
from zkit import NODRAW, TRIM, Kit, T, arch, frustum, house, onion, places, props_info, spawns  # noqa: E402

m = Map("zg_altstadt")
k = Kit(m)
k.facade_style = "town"
rng = random.Random(11)

XW, XE, YS, YN = -2700, 2500, -2400, 2100
COBBLE, PAVE, DIRT, GRAVEL, GRASS = T("gnd_setts"), T("gnd_flagstone"), T("gnd_dust"), T("gravel"), T("grass")
OLD, MOSS = T("gnd_setts_old"), T("gnd_setts_moss")
PLASTERS = [T("plaster_cream"), T("plaster_white"), T("plaster_red"), T("plaster_ochre"), T("plaster_pink"),
            T("plaster_cream"), T("plaster_sky"), T("plaster_sage"), T("plaster_ochre")]
SHUTTERS = [T("metal_painted_green"), T("wood_beam"), T("metal_painted_red"), None, T("metal_painted_green")]

# ---------------------------------------------------------------------------------------------------------------
# Walkable areas (streets, squares, yards) with their ground.
W = {
    "OPFOR-Start": ((-600, -2400), (600, -1800), COBBLE),
    "Südgasse": ((-1700, -2150), (-600, -1900), COBBLE),
    "Lange Gasse": ((-1700, -1900), (-1480, 300), COBBLE),
    "Schmiedgasse": ((-1480, -950), (-250, -800), COBBLE),
    "Marktstraße": ((-250, -1800), (250, -600), COBBLE),
    "Markt": ((-600, -600), (600, 200), OLD),
    "Kirchgasse": ((-1100, -150), (-600, 50), COBBLE),
    "Kirchgasse Nord": ((-1100, 50), (-900, 300), COBBLE),
    "Kirchplatz": ((-2300, 300), (-900, 1450), OLD),
    "Pfarrgasse": ((-900, 1150), (-600, 1450), MOSS),
    "Rathausplatz": ((-600, 1300), (1200, YN), PAVE),
    "Rathausgasse": ((300, 200), (550, 900), COBBLE),
    "Winkel": ((100, 900), (550, 1100), COBBLE),
    "Rathausgasse Nord": ((100, 1100), (350, 1300), COBBLE),
    "Bäckergasse": ((600, 0), (900, 200), COBBLE),
    "Bäckergasse Nord": ((700, 200), (900, 650), COBBLE),
    "Mauergasse": ((600, -2150), (1500, -1900), MOSS),
    "Hinterhof": ((1200, -1900), (1900, -300), DIRT),
    "Brauereitor": ((1400, -300), (1650, 200), COBBLE),
    "Brauerei": ((900, 200), (2300, 1400), GRAVEL),
}
for name, (lo, hi, tex) in W.items():
    k.ground(lo[0], lo[1], hi[0], hi[1], tex, 640)

# Cables strung across the lanes (power, phone), eaves-high between the facades: two or three per lane, a few
# doubled. (Drawn by the client, zg_wire; nothing collides with them.)
for name, (lo, hi, _) in W.items():
    if "gasse" not in name.lower() and "straße" not in name.lower():
        continue
    along_x = (hi[0] - lo[0]) > (hi[1] - lo[1])
    length = (hi[0] - lo[0]) if along_x else (hi[1] - lo[1])
    for i in range(max(1, int(length // 450))):
        u = (lo[0] if along_x else lo[1]) + length * (i + 0.3 + 0.4 * rng.random())
        z0, z1 = rng.randint(200, 250), rng.randint(200, 250)
        for j in range(2 if rng.random() < 0.4 else 1):
            dz = j * 14
            a, b = ((u, lo[1] + 1, z0 - dz), (u + rng.randint(-60, 60), hi[1] - 1, z1 - dz)) if along_x else ((lo[0] + 1, u, z0 - dz), (hi[0] - 1, u + rng.randint(-60, 60), z1 - dz))
            m.point("zg_wire", a, end=f"{b[0]:.1f} {b[1]:.1f} {b[2]:.1f}", sag=f"{0.25 + 0.3 * rng.random():.2f}")

# ---------------------------------------------------------------------------------------------------------------
# House blocks: rows of houses along the streets they face.


def row_x(x0, x1, y0, y1, face, h=(260, 420), depth=None, **kw):
    """Houses side by side along x between x0..x1, occupying y0..y1, facing `face` ("s" or "n")."""
    x = x0
    while x < x1 - 1:
        w = min(x1 - x, rng.choice((300, 340, 380, 420, 460)))
        if x1 - (x + w) < 200:
            w = x1 - x
        make_house(x, y0, x + w, y1, face, rng.randint(*h) // 10 * 10, **kw)
        x += w


def row_y(y0, y1, x0, x1, face, h=(260, 420), **kw):
    y = y0
    while y < y1 - 1:
        w = min(y1 - y, rng.choice((300, 340, 380, 420, 460)))
        if y1 - (y + w) < 200:
            w = y1 - y
        make_house(x0, y, x1, y + w, face, rng.randint(*h) // 10 * 10, **kw)
        y += w


SQUARES = ("Markt", "Marktstraße", "Rathausplatz", "Kirchplatz", "OPFOR-Start")


def street_sides(x0, y0, x1, y1):
    """The sides of a footprint that border a walkable area (they need a drawn facade), and those on a square."""
    out, shop = "", ""
    width = {}
    for name, (lo, hi, _) in W.items():
        for side, hit in (("n", abs(lo[1] - y1) < 1 and min(hi[0], x1) - max(lo[0], x0) > 20),
                          ("s", abs(hi[1] - y0) < 1 and min(hi[0], x1) - max(lo[0], x0) > 20),
                          ("e", abs(lo[0] - x1) < 1 and min(hi[1], y1) - max(lo[1], y0) > 20),
                          ("w", abs(hi[0] - x0) < 1 and min(hi[1], y1) - max(lo[1], y0) > 20)):
            if hit and side not in out:
                out += side
            if hit and name in SQUARES and side not in shop:
                shop += side
            if hit:
                wd = (hi[1] - lo[1]) if side in "ns" else (hi[0] - lo[0])
                width[side] = max(width.get(side, 0), wd)
    street_sides.width = width
    return out, shop


def make_house(x0, y0, x1, y1, faces, eaves, plaster=None, timber=None, gable=None, door=True, sides=None, brick=False):
    """One house; `faces` are the street sides (they get windows); ridge parallel to the main street side unless
    `gable` (gable end to the street)."""
    plaster = plaster or (T("bricks090") if brick else rng.choice(PLASTERS))
    timber = (rng.random() < 0.4) if timber is None else timber
    main = faces[0]
    along_x = main in "ns"
    gable = (rng.random() < 0.3) if gable is None else gable
    ridge = ("y" if along_x else "x") if gable else ("x" if along_x else "y")
    width = (y1 - y0) if ridge == "x" else (x1 - x0)
    rise = min(width * 0.5, 260)
    shut = rng.choice(SHUTTERS) if not timber else rng.choice((None, T("metal_painted_green")))
    d = None
    if door:
        uc = ((x0 + x1) / 2 if along_x else (y0 + y1) / 2) + rng.choice((-60, 0, 60))
        d = (main, uc, 48, 96)
    # Facades on every side that borders a street; shop fronts on squares (some houses).
    auto, shopsides = street_sides(x0, y0, x1, y1)
    vis = "".join(dict.fromkeys((sides or faces) + auto))
    shops = shopsides[:1] if (shopsides and rng.random() < 0.6 and min(x1 - x0, y1 - y0) >= 250) else ""
    # The roof slope turned away from every street is only seen from above: not drawn.
    opp = {"n": "s", "s": "n", "e": "w", "w": "e"}
    hide = "".join(opp[f] for f in vis if opp[f] not in vis and ((ridge == "x") == (f in "ns")))
    # A slope whose eave faces only a narrow alley can't be seen from the ground either (the eye is below its plane).
    widths = street_sides.width
    for f in vis:
        if (ridge == "x") == (f in "ns") and widths.get(f, 9999) <= 300 and f not in hide:
            hide += f
    # Neighbours stand a unit apart, so their roofs and walls don't split each other's faces into many charts.
    g = 0.5
    sx0, sy0, sx1, sy1 = (x0 + (0 if "w" in vis else g), y0 + (0 if "s" in vis else g),
                          x1 - (0 if "e" in vis else g), y1 - (0 if "n" in vis else g))
    # A dark seam fills the gap at the low-end joint of each street facade (the neighbour covers the other end).
    for side in vis:
        if side in "ns" and "w" not in vis:
            yf = y1 if side == "n" else y0
            ya, yb = (yf - 16, yf) if side == "n" else (yf, yf + 16)
            k.dblk((x0 - 0.5, ya, 0), (x0 + 0.5, yb, eaves - 1), {"sides": NODRAW, "top": NODRAW, "bottom": NODRAW,
                                                                 ("+y" if side == "n" else "-y"): TRIM})
        if side in "ew" and "s" not in vis:
            xf = x1 if side == "e" else x0
            xa, xb = (xf - 16, xf) if side == "e" else (xf, xf + 16)
            k.dblk((xa, y0 - 0.5, 0), (xb, y0 + 0.5, eaves - 1), {"sides": NODRAW, "top": NODRAW, "bottom": NODRAW,
                                                                 ("+x" if side == "e" else "-x"): TRIM})
    x0, y0, x1, y1 = sx0, sy0, sx1, sy1
    over = {sd: (18 if sd in vis else 0) for sd in "nsew"}
    house(k, x0, y0, x1, y1, eaves, plaster, sides=vis, ridge=ridge, rise=rise, timber=timber, over=over,
          over_end=12 if gable else 0,
          bays=rng.choice((150, 165, 180)), win_w=40, win_h=58, shutters=shut, door=d, plinth=T("stone_rustic"),
          quoins=T("stone_rustic") if rng.random() < 0.55 else None, rafters=T("wood_old"),
          chimney=rng.random() < 0.6, hide=hide, shops=shops,
          awning=rng.choice((T("metal_painted_red"), T("metal_painted_green"), T("metal_painted_blue"))))
    return ridge


# South: the town wall with the gate tower; houses backing onto it facing the Gassen.
WALL_T = T("stone_rubble")
k.blk((XW - 60, YS - 60, 0), (XE + 60, YS, 320), {"sides": WALL_T, "top": NODRAW, "bottom": NODRAW, "-y": NODRAW})
for x in range(-980, 1000, 90):
    if -270 < x < 270:
        continue
    k.blk((x, YS - 60, 320), (x + 44, YS - 10, 360), {"sides": WALL_T, "top": T("concrete_wall"), "bottom": NODRAW, "-y": NODRAW})
k.blk((-1000, YS - 64, 300), (1000, YS - 8, 320), {"sides": T("concrete_wall"), "top": NODRAW, "bottom": NODRAW, "-y": NODRAW})
# The gate tower ("Unteres Tor"), a closed gate under a round arch, a clock and a steep roof.
TX0, TX1 = -260, 260
k.blk((TX0, YS - 200, 0), (-120, YS + 40, 700), {"sides": WALL_T, "top": NODRAW, "bottom": NODRAW})
k.blk((120, YS - 200, 0), (TX1, YS + 40, 700), {"sides": WALL_T, "top": NODRAW, "bottom": NODRAW})
arch(k, "x", -120, 120, YS - 200, YS + 40, 160, 700, {"sides": WALL_T, "bottom": WALL_T, "top": NODRAW}, n=8)
k.blk((-120, YS - 60, 0), (120, YS - 40, 280), {"sides": T("wood_planks"), "top": NODRAW, "bottom": NODRAW})
for x in (-90, -30, 30, 90):
    k.panel("x", x - 4, x + 4, YS - 40, 1, 20, 250, 2, TRIM)
k.band("x", TX0 - 8, TX1 + 8, YS + 40, 1, 0, 30, 6, T("concrete_wall"))
k.band("x", TX0, TX1, YS + 40, 1, 470, 480, 4, T("concrete_wall"), show=("front", "top"))
for x in (TX0 + 4, TX1 - 24):
    k.panel("x", x, x + 20, YS + 40, 1, 30, 700, 4, T("concrete_wall"))  # quoins
k.add(prism((0, YS + 41), 44, 540, 541.5, 12, {"all": T("plaster_white")}, axis="y"), deco=True)  # clock face
k.panel("x", -3, 3, YS + 42.5, 1, 540, 575, 1, TRIM)
k.panel("x", -3, 22, YS + 42.5, 1, 538, 544, 1, TRIM)
k.flat_window("x", YS + 40, 1, -30, 30, 350, 430, frame=T("concrete_wall"), cross="frame")
k.gable_roof(TX0, YS - 200, TX1, YS + 40, 700, 300, axis="x", tex=T("roof_clay"), gable=WALL_T, over=16)
# Houses south of the Gassen, backing onto the wall.
row_x(-2700, -1700, -2300, -1900, "n", (260, 340))
row_x(-1700, -600, YS, -2150, "n", (240, 320))
row_x(600, 1500, YS, -2150, "n", (240, 320))
row_x(1500, 2500, YS, -1900, "n", (260, 340), sides="nw")
# West of the Lange Gasse (facing east).
row_y(-1900, 300, -2300, -1700, "e", (300, 420))
# Between the Lange Gasse and the Marktstraße.
row_y(-1900, -950, -1480, -1150, "w", (300, 400), sides="wn")
make_house(-1150, -1900, -600, -1560, "s", 330, sides="sn")
make_house(-1150, -1560, -600, -1250, "n", 360, sides="en")
make_house(-1150, -1250, -600, -950, "n", 300, sides="n")
make_house(-600, -1800, -250, -1380, "e", 380, sides="es")
make_house(-600, -1380, -250, -950, "e", 340, sides="en")
make_house(-1480, -800, -1100, -450, "w", 360, sides="ws")
make_house(-1480, -450, -1100, -150, "w", 320, sides="w")
make_house(-1100, -800, -600, -450, "s", 380, sides="s")
make_house(-1100, -450, -600, -150, "n", 330, sides="n")
make_house(-600, -800, -250, -600, "s", 300, sides="se")
make_house(-1480, -150, -1100, 300, "w", 380, sides="wne")
make_house(-900, 50, -600, 300, "w", 320, sides="wn")
# East of the Marktstraße and the market.
make_house(250, -1800, 600, -1400, "w", 360, sides="ws")
make_house(250, -1400, 600, -1000, "w", 400, sides="w")
make_house(250, -1000, 600, -600, "w", 340, sides="wn")
row_y(-1500, -600, 800, 1200, "e", (280, 360), sides="e")
make_house(600, -1900, 1200, -1500, "s", 320, sides="se")
make_house(600, -600, 1200, -300, "w", 340, sides="we")
make_house(600, -300, 1000, 0, "w", 380, sides="wn")
make_house(1000, -300, 1400, 0, "e", 340, sides="en")
make_house(900, 0, 1400, 200, "w", 300, sides="wn", brick=True)
# East of the back yard: the brewery's back buildings.
make_house(1900, -1900, 2300, -1100, "w", 380, sides="w", brick=True, timber=False)
make_house(1900, -1100, 2300, -300, "w", 420, sides="wn", brick=True, timber=False)
# West of the church square, north of the market.
row_y(300, 1450, -2700, -2300, "e", (320, 420))
make_house(-900, 300, -600, 720, "w", 340, sides="w")
make_house(-900, 720, -600, 1150, "w", 380, sides="ws")
make_house(-600, 200, -150, 600, "s", 400, sides="s")
make_house(-150, 200, 300, 700, "s", 360, sides="se")
make_house(-600, 900, -150, 1300, "n", 380, sides="n")
make_house(-150, 700, 100, 1300, "e", 340, sides="ne")
make_house(100, 700, 300, 900, "e", 320, sides="en")
make_house(350, 1100, 550, 1300, "w", 300, sides="wsn")
make_house(550, 200, 700, 1300, "w", 360, sides="w", gable=False)
k.crate(500, 1050, 0, 48)
k.crate(150, 950, 0, 56)
make_house(700, 650, 900, 1300, "e", 320, sides="en")
# Filling the block north of the Pfarrgasse, between the church and the Rathausplatz, and the corner east of the
# Brauereitor (both were open to the void below).
make_house(-1250, 1450, -900, YN, "s", 360, sides="s")
make_house(-900, 1450, -600, YN, "s", 380, sides="se")
make_house(1650, -300, 2300, 200, "w", 360, sides="wn", brick=True, timber=False)
# North of the brewery yard: the malthouse.
make_house(1200, 1400, 2270, YN, "s", 460, sides="sw", brick=True, timber=False, gable=False)

# ---------------------------------------------------------------------------------------------------------------
# The arch house over the Rathausgasse (a passage under an upper floor).
AY0, AY1 = 450, 650
arch(k, "x", 300, 550, AY0, AY1, 150, 260, {"sides": T("plaster_ochre"), "bottom": T("plaster_ochre"), "top": NODRAW}, n=6, rise=90)
k.solid_building(300, AY0, 550, AY1, 260, 430, T("plaster_ochre"), sides="ns", bays=125, floors=[40], win_w=40,
                 win_h=56, shutters=T("metal_painted_green"), sill=T("plaster_white"), cross=False)
k.gable_roof(300, AY0, 550, AY1, 430, 120, axis="y", tex=T("roof_clay"), gable=T("plaster_ochre"), over=18)
for yf, out in ((AY0, -1), (AY1, 1)):
    k.band("x", 296, 554, yf, out, 250, 262, 5, T("concrete_wall"))
# A flying arch ("Schwibbogen") across the Schmiedgasse.
arch(k, "x", -700, -520, -950, -800, 250, 320, {"sides": T("plaster_cream"), "bottom": T("plaster_cream"), "top": T("roof_clay")}, n=6, rise=50)

# ---------------------------------------------------------------------------------------------------------------
# Church on the north side of the square: nave, onion-dome tower, portal with steps (A's high ground).
NX0, NX1, NY0, NY1, NH = -2150, -1250, 1500, 1850, 450
CH = T("plaster_white")
k.solid_building(NX0, NY0, NX1, NY1, 0, NH, CH, sides="sew", bays=225, floors=[160], win_w=64, win_h=190,
                 plinth=T("concrete_wall"), plinth_h=40, cornice=T("concrete_wall"), cornice_h=16, frame=T("concrete_wall"),
                 cross="frame", sill=T("concrete_wall"))
k.gable_roof(NX0, NY0, NX1, NY1, NH, 260, axis="x", tex=T("roof_clay"), gable=CH, over=20, ridge=TRIM)
for x in (NX0, NX1 - 30):
    k.panel("x", x, x + 30, NY0, -1, 40, NH - 16, 4, T("concrete_wall"))
# Buttresses along the south wall.
for x in (-1950, -1500):
    k.blk((x - 20, NY0 - 40, 0), (x + 20, NY0, 280), {"sides": T("concrete_wall"), "top": T("concrete_wall"), "bottom": NODRAW})
    k.add(hull([(x - 20, NY0 - 40, 280), (x + 20, NY0 - 40, 280), (x - 20, NY0, 280), (x + 20, NY0, 280),
                (x - 20, NY0, 330), (x + 20, NY0, 330)], T("concrete_wall")))
# Portal: a round-arched door, a stone surround, a landing and steps down to the square.
PX = -1700
if not k.facade_piece("door_church", "x", NY0, -1, PX - 70, PX + 70, 48):
    k.panel("x", PX - 70, PX + 70, NY0, -1, 48, 230, 1, T("wood_planks"))
    arch(k, "x", PX - 90, PX + 90, NY0 - 16, NY0, 230, 330, {"sides": T("concrete_wall"), "bottom": T("concrete_wall"), "top": T("concrete_wall")}, n=6)
    for x in (PX - 90, PX + 70):
        k.blk((x, NY0 - 16, 48), (x + 20, NY0, 230), {"sides": T("concrete_wall"), "top": NODRAW, "bottom": NODRAW})
k.blk((PX - 170, NY0 - 110, 0), (PX + 170, NY0, 48), {"sides": T("concrete_wall"), "top": T("paving"), "bottom": NODRAW})
k.add(stairs((PX - 170, NY0 - 200, 0), (PX + 170, NY0 - 110, 48), "+y", 3, {"sides": T("concrete_wall"), "top": T("paving"), "bottom": NODRAW}))
k.railing(PX - 170, NY0 - 110, PX - 170, NY0, 48, 36, TRIM)
k.railing(PX + 170, NY0 - 110, PX + 170, NY0, 48, 36, TRIM)
# Tower: square shaft with belfry openings, an octagonal drum and the copper onion.
TX0, TX1, TY0, TY1 = -2420, -2150, 1540, 1810
TC = ((TX0 + TX1) / 2, (TY0 + TY1) / 2)
k.solid_building(TX0, TY0, TX1, TY1, 0, 1000, CH, sides="se", bays=300, floors=[380, 760], win_w=44, win_h=110,
                 plinth=T("concrete_wall"), plinth_h=40, cornice=T("concrete_wall"), cornice_h=20, frame=T("concrete_wall"),
                 cross="frame", sill=T("concrete_wall"))
for axis, a0, a1, face, out in (("x", TX0, TX1, TY0, -1), ("y", TY0, TY1, TX1, 1)):
    for zb in (300, 600):
        k.band(axis, a0, a1, face, out, zb, zb + 12, 4, T("concrete_wall"), show=("front", "top"))
frustum(k, TC[0], TC[1], 1000, 150, 1030, 150, 8, {"sides": T("concrete_wall"), "top": NODRAW, "bottom": NODRAW})
frustum(k, TC[0], TC[1], 1030, 120, 1130, 120, 8, {"sides": CH, "top": NODRAW, "bottom": NODRAW})
onion(k, TC[0], TC[1], 1130, 110, 360, T("metal_painted_green"), tip=TRIM)
# Churchyard wall east of the church and a lime tree.
k.blk((NX1, NY0 - 20, 0), (-900, NY0, 56), {"sides": T("plaster_cream"), "top": T("concrete_wall"), "bottom": NODRAW})
k.tree(-1050, 1650, 0, 480, 150)
k.tree(-2480, 1300, 0, 420, 120)

# ---------------------------------------------------------------------------------------------------------------
# Site A: the church square. Fountain, market stalls, a parked car, benches.
FX, FY = -1575, 930
# The modelled fountain (arch.py) where built: the brush one stays as invisible collision, its water surface drawn.
FOUNTAIN = "fountain_town" in props_info()
FS = NODRAW if FOUNTAIN else T("concrete_wall")
if FOUNTAIN:
    # (all-nodraw collision, and the water surface as a thin slab of its own: a drawn top on the collision brush
    # would have `expose` give its hidden sides the water's texture)
    k.add(prism((FX, FY), 120, 0, 33.5, 8, NODRAW, rot=22.5))
    k.add(prism((FX, FY), 118, 33.5, 34, 8, lambda n: "dev/water" if n[2] > 0.9 else NODRAW, rot=22.5), deco=True)
else:
    k.add(prism((FX, FY), 120, 0, 34, 8, {"sides": FS, "top": "dev/water", "bottom": NODRAW}, rot=22.5))
if FOUNTAIN:
    k.prop("fountain_town", FX, FY, 0, yaw=22.5, solid=False)
for i in range(8):
    a0 = math.radians(22.5 + 45 * i)
    a1 = math.radians(22.5 + 45 * (i + 1))
    pts = []
    for a in (a0, a1):
        for r in (106, 122):
            for z in (34, 44):
                pts.append((FX + r * math.cos(a), FY + r * math.sin(a), z))
    k.add(hull(pts, FS))
k.add(prism((FX, FY), 18, 34, 150, 8, {"sides": FS, "top": NODRAW, "bottom": NODRAW}, rot=22.5))
if not FOUNTAIN:
    frustum(k, FX, FY, 150, 24, 170, 60, 8, T("concrete_wall"))
    frustum(k, FX, FY, 170, 60, 180, 60, 8, T("concrete_wall"))
    k.add(prism((FX, FY), 14, 180, 250, 6, {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW}))
    frustum(k, FX, FY, 250, 16, 270, 4, 6, T("metal_painted_green"))


def stall(x, y, color, axis="x"):
    """A market stall: a counter, four poles and a sloping awning (the modelled stall where it is built: the same
    collision, drawn by the model)."""
    L, D = 180, 90
    model = f"market_stall_{color}"
    if model in props_info():
        lo, hi = ((x - L / 2, y - D / 2, 0.25), (x + L / 2, y + D / 2, 40)) if axis == "x" else ((x - D / 2, y - L / 2, 0.25), (x + D / 2, y + L / 2, 40))
        k.blk(lo, hi, NODRAW)
        k.prop(model, x, y, 0, yaw=270 if axis == "x" else 180, scale=1.25, solid=False)
        k.clip((x - 100, y - 60, 40), (x + 100, y + 60, 125)) if axis == "x" else k.clip((x - 60, y - 100, 40), (x + 60, y + 100, 125))
        return
    if axis == "x":
        k.blk((x - L / 2, y - D / 2, 0.25), (x + L / 2, y + D / 2, 40), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
        for px in (x - L / 2 + 4, x + L / 2 - 4):
            for py in (y - D / 2 + 4, y + D / 2 - 4):
                k.post(px, py, 40, 100 if py < y else 120, 4, TRIM)
        k.add(hull([(x - L / 2 - 10, y - D / 2 - 20, 98), (x + L / 2 + 10, y - D / 2 - 20, 98), (x - L / 2 - 10, y + D / 2 + 10, 122),
                    (x + L / 2 + 10, y + D / 2 + 10, 122), (x - L / 2 - 10, y - D / 2 - 20, 101), (x + L / 2 + 10, y - D / 2 - 20, 101),
                    (x - L / 2 - 10, y + D / 2 + 10, 125), (x + L / 2 + 10, y + D / 2 + 10, 125)], T(f"metal_painted_{color}")), deco=True)
    else:
        k.blk((x - D / 2, y - L / 2, 0.25), (x + D / 2, y + L / 2, 40), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
        for px in (x - D / 2 + 4, x + D / 2 - 4):
            for py in (y - L / 2 + 4, y + L / 2 - 4):
                k.post(px, py, 40, 100 if px < x else 120, 4, TRIM)
        k.add(hull([(x - D / 2 - 20, y - L / 2 - 10, 98), (x - D / 2 - 20, y + L / 2 + 10, 98), (x + D / 2 + 10, y - L / 2 - 10, 122),
                    (x + D / 2 + 10, y + L / 2 + 10, 122), (x - D / 2 - 20, y - L / 2 - 10, 101), (x - D / 2 - 20, y + L / 2 + 10, 101),
                    (x + D / 2 + 10, y - L / 2 - 10, 125), (x + D / 2 + 10, y + L / 2 + 10, 125)], T(f"metal_painted_{color}")), deco=True)
    k.clip((x - 100, y - 60, 40), (x + 100, y + 60, 125)) if axis == "x" else k.clip((x - 60, y - 100, 40), (x + 60, y + 100, 125))


stall(-1950, 700, "red")
stall(-1250, 1200, "blue", "y")
k.car(-2150, 1100, 0, "+y", "metal_painted_blue")
k.bench(-1575, 700, 0, "x")
k.bench(-1850, 1300, 0, "x")
k.crate(-1350, 700, 0, 48)
k.crate(-1300, 690, 0, 40)
k.crate(-1330, 700, 48, 36)
for x, y in ((-2250, 380), (-950, 380), (-2250, 1380)):
    k.lamp(x, y, 0, 220, "+y", T("metal_painted_green"))
k.barrel(-1050, 1050, 0, 14, 38, T("metal_painted_green"))
k.barrel(-1030, 1080, 0, 14, 38, T("metal_painted_green"))

# ---------------------------------------------------------------------------------------------------------------
# Mid: the market square with stalls, a maypole and cover; the Marktstraße with crates and a cart.
stall(-350, -300, "green")
stall(350, -300, "red")
stall(-350, 20, "blue")
k.add(prism((200, -100), 8, 0, 700, 8, {"sides": T("plaster_white"), "top": TRIM, "bottom": NODRAW}), deco=True)  # maypole
k.clip((190, -110, 0), (210, -90, 900))
for z in (300, 420, 540):
    k.dblk((140, -104, z), (260, -96, z + 10), T("metal_painted_blue"))
k.add(prism((200, -100), 60, 640, 650, 8, {"sides": T("plaster_ochre"), "top": T("plaster_ochre"), "bottom": T("plaster_ochre")}), deco=True)


def cart(x, y, axis="x"):
    """A wooden hand cart with two spoked wheels (the modelled cart where built, over the same collision)."""
    carts = [c for c in ("hand_cart", "hand_cart_sacks", "hand_cart_barrels") if c in props_info()]
    if carts:
        lo, hi = ((x - 70, y - 40, 22), (x + 70, y + 40, 52)) if axis == "x" else ((x - 40, y - 70, 22), (x + 40, y + 70, 52))
        k.blk(lo, hi, NODRAW)
        k.clip((x - 75, y - 50, 0), (x + 75, y + 50, 52)) if axis == "x" else k.clip((x - 50, y - 75, 0), (x + 50, y + 75, 52))
        # (the model's shafts point along its -z, i.e. its back is its front: yaw so the shafts go +x / +y as before)
        k.prop(carts[int((x * 7 + y * 3) % len(carts))], x + (40 if axis == "x" else 0), y + (40 if axis == "y" else 0), 0,
               yaw=180 if axis == "x" else 270, scale=1.2, solid=False)
        return
    if axis == "x":
        k.blk((x - 70, y - 40, 22), (x + 70, y + 40, 52), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
        for yy in (y - 46, y + 46):
            k.add(prism((x, 24), 24, yy - 3, yy + 3, 8, T("wood_beam"), rot=22.5, axis="y"), deco=True)
        k.dblk((x + 70, y - 30, 40), (x + 150, y - 26, 44), T("wood_beam"))
        k.dblk((x + 70, y + 26, 40), (x + 150, y + 30, 44), T("wood_beam"))
    else:
        k.blk((x - 40, y - 70, 22), (x + 40, y + 70, 52), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
        for xx in (x - 46, x + 46):
            k.add(prism((y, 24), 24, xx - 3, xx + 3, 8, T("wood_beam"), rot=22.5, axis="x"), deco=True)
        k.dblk((x - 30, y + 70, 40), (x - 26, y + 150, 44), T("wood_beam"))
        k.dblk((x + 26, y + 70, 40), (x + 30, y + 150, 44), T("wood_beam"))
    k.clip((x - 75, y - 50, 0), (x + 75, y + 50, 52)) if axis == "x" else k.clip((x - 50, y - 75, 0), (x + 50, y + 75, 52))


cart(-120, -1300, "y")
k.crate(150, -1500, 0, 56)
k.crate(170, -1444, 0, 48)
k.crate(160, -1470, 56, 40)
k.crate(-180, -900, 0, 48)
k.barrel(480, 120, 0, 14, 38, T("wood_beam"), hoop=TRIM)
k.barrel(510, 150, 0, 14, 38, T("wood_beam"), hoop=TRIM)
k.bench(-520, -450, 0, "y")
for x, y in ((-560, -560), (560, 160), (0, -1780)):
    k.lamp(x, y, 0, 220, "+y", T("metal_painted_green"))
# Lange Gasse: a few doorsteps and a rain barrel for cover along the long sightline.
k.blk((-1700, -1300, 0), (-1660, -1180, 18), {"sides": T("concrete_wall"), "top": T("concrete_wall"), "bottom": NODRAW})
k.barrel(-1510, -600, 0, 14, 38, T("wood_beam"), hoop=TRIM)
k.crate(-1660, -250, 0, 40)
k.crate(-1520, 120, 0, 48)
# Kirchgasse and Schmiedgasse props.
k.crate(-1060, -100, 0, 44)
k.barrel(-940, 260, 0, 14, 38, T("metal_painted_blue"))

# ---------------------------------------------------------------------------------------------------------------
# OPFOR start: inside the town gate. A well, crates, a hay cart.
k.add(prism((-420, -2250), 40, 0, 40, 8, {"sides": T("concrete_wall"), "top": "dev/water", "bottom": NODRAW}, rot=22.5))
for x in (-452, -388):
    k.post(x, -2250, 40, 150, 6, T("wood_beam"), solid=True)
k.dblk((-460, -2254, 150), (-380, -2246, 158), T("wood_beam"))
k.dblk((-470, -2290, 158), (-370, -2210, 162), T("roof_clay"))
cart(420, -2250, "y")
k.crate(470, -2000, 0, 56)
k.crate(430, -1960, 0, 48)

# ---------------------------------------------------------------------------------------------------------------
# East lane: Mauergasse and the back yards: sheds, a woodpile, a garden fence, washing line posts.
k.blk((1200, -1800, 0), (1420, -1600, 110), {"sides": T("wood_planks"), "top": NODRAW, "bottom": NODRAW})  # shed
k.add(hull([(1190, -1810, 110), (1430, -1810, 110), (1190, -1590, 140), (1430, -1590, 140),
            (1190, -1810, 116), (1430, -1810, 116), (1190, -1590, 146), (1430, -1590, 146)], {"top": T("metal_corrugated"), "all": T("wood_beam")}))
k.blk((1700, -1300, 0.25), (1880, -1240, 64), {"sides": T("wood_beam"), "top": T("wood_beam"), "bottom": NODRAW})  # woodpile
k.blk((1720, -1240, 0.25), (1880, -1200, 40), {"sides": T("wood_beam"), "top": T("wood_beam"), "bottom": NODRAW})
k.ground(1250, -1150, 1550, -700, GRASS, 640, -16, 0.5)
k.fence(1250, -700, 1550, -700, 0, 48, 60, T("wood_planks"), rails=(0.5, 0.95), clip_h=48, mesh=False)
k.fence(1550, -1150, 1550, -700, 0, 48, 60, T("wood_planks"), rails=(0.5, 0.95), clip_h=48, mesh=False)
k.tree(1400, -950, 0, 360, 110)
for x in (1300, 1800):
    k.post(x, -500, 0, 140, 6, T("wood_beam"), solid=True)
k.crate(1800, -1700, 0, 56)
k.crate(1840, -1650, 0, 44)
k.barrel(1250, -400, 0, 14, 38, T("wood_beam"), hoop=TRIM)
cart(1600, -600, "x")

# ---------------------------------------------------------------------------------------------------------------
# The brewery: gatehouse arch, brewhouse with a loading ramp, the chimney, barrels, carts.
GX0, GX1 = 1400, 1650
k.solid_building(1400, -100, 1650, 200, 240, 420, T("bricks090"), sides="ns", bays=125, floors=[40], win_w=40, win_h=60,
                 sill=T("concrete_wall"), frame=T("concrete_wall"), cross="frame")
arch(k, "x", GX0, GX1, -100, 200, 150, 240, {"sides": T("bricks090"), "bottom": T("bricks090"), "top": NODRAW}, n=6, rise=90)
k.gable_roof(1400, -100, 1650, 200, 420, 140, axis="y", tex=T("roof_clay"), gable=T("bricks090"), over=16)
for yf, out in ((-100, -1), (200, 1)):
    k.band("x", 1396, 1654, yf, out, 0, 24, 4, T("concrete_wall"))
    k.panel("x", 1440, 1610, yf, out, 290, 330, 2, T("wood_planks"), show=("front", "top", "bottom", "ends"))  # sign board
# Open gate leaves against the passage walls.
k.blk((GX0, 200, 0.25), (GX0 + 8, 330, 190), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
k.blk((GX1 - 8, 200, 0.25), (GX1, 330, 190), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
# Brewhouse (east side of the yard): brick, tall, with a loading ramp along its west front.
BH = T("bricks090")
k.solid_building(2300, -300, 2800, 1600, 0, 520, BH, sides="wn", bays=190, floors=[80, 300], win_w=64, win_h=110,
                 plinth=T("concrete_wall"), cornice=T("concrete_wall"), cornice_h=16, sill=T("concrete_wall"),
                 frame=T("metal_painted_green"), cross="frame", door=("w", 800, 120, 150), door_tex=T("wood_planks"))
k.gable_roof(2300, -300, 2800, 1600, 520, 220, axis="y", tex=T("roof_clay"), gable=BH, over=20, hide="e")
k.blk((2150, 400, 0), (2300, 1200, 48), {"sides": T("concrete_wall"), "top": T("wood_planks"), "bottom": NODRAW})
k.add(stairs((2150, 1200, 0), (2300, 1290, 48), "-y", 3, {"sides": T("concrete_wall"), "top": T("wood_planks"), "bottom": NODRAW}))
k.dblk((2140, 400, 180), (2300, 1200, 186), {"sides": T("metal_corrugated"), "top": NODRAW, "bottom": T("metal_corrugated")})  # canopy
for y in (420, 800, 1180):
    k.post(2146, y, 48, 180, 6, T("wood_beam"), solid=True)
# Chimney (round brick stack) behind the brewhouse.
k.add(prism((2560, 1450), 70, 520, 1250, 12, {"sides": BH, "top": NODRAW, "bottom": NODRAW}), deco=True)
k.add(prism((2560, 1450), 78, 1250, 1290, 12, {"sides": T("concrete_wall"), "top": TRIM, "bottom": T("concrete_wall")}), deco=True)
k.add(prism((2560, 1450), 76, 900, 915, 12, {"sides": T("concrete_wall"), "top": T("concrete_wall"), "bottom": T("concrete_wall")}), deco=True)


def keg_stack(x, y, axis="x", n=3):
    """Barrels lying in a pyramid (3-2-1)."""
    r = 18
    for level in range(n):
        for i in range(n - level):
            off = (i - (n - level - 1) / 2) * 2 * r
            if axis == "x":
                k.keg(x, y + off, level * r * 1.7, r, 44, "x")
            else:
                k.keg(x + off, y, level * r * 1.7, r, 44, "y")
    k.clip((x - 24, y - n * r, 0), (x + 24, y + n * r, n * r * 1.7 + 6)) if axis == "x" else k.clip((x - n * r, y - 24, 0), (x + n * r, y + 24, n * r * 1.7 + 6))


keg_stack(1500, 1250, "x")
keg_stack(2050, 520, "y")
keg_stack(1100, 400, "x", 2)
for (x, y) in ((1850, 900), (1880, 935), (1840, 960), (1200, 1000), (1230, 1030)):
    k.barrel(x, y, 0, 15, 40, T("wood_beam"), hoop=TRIM, lid=T("wood_planks"))
cart(1650, 700, "y")
k.crate(1300, 700, 0, 64)
k.crate(1300, 764, 0, 48)
k.crate(1310, 730, 64, 48)
k.crate(2080, 1150, 0, 56)
k.crate(2080, 1100, 0.0, 48)
k.van(1150, 1300, 0, "+x", "metal_painted_green")
# Beer-garden tables by the malthouse.
for x in (1500, 1800):
    k.blk((x - 60, 1340, 0.25), (x + 60, 1370, 30), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
for x, y in ((950, 250), (2250, 250), (950, 1350)):
    k.lamp(x, y, 0, 220, "+y", T("metal_painted_green"))

# ---------------------------------------------------------------------------------------------------------------
# KSK start: the town-hall square. Town hall facade with a stepped gable, arcade and clock; a lime tree; benches.
RH = T("plaster_ochre")
k.solid_building(-600, YN, 1200, YN + 400, 0, 460, RH, sides="s", bays=150, floors=[200, 330], win_w=56, win_h=90,
                 plinth=T("concrete_wall"), plinth_h=40, cornice=T("concrete_wall"), cornice_h=16, sill=T("concrete_wall"),
                 frame=T("plaster_white"), cross="frame", shutters=None)
# Arcade on the ground floor: arches in the facade.
for i, x in enumerate(range(-450, 1100, 200)):
    k.panel("x", x, x + 120, YN, -1, 0, 180, 1, TRIM)
    # Shops and doors set into the arcade's back wall.
    if i % 3 == 1:
        k.facade_piece("door_town_wood", "x", YN - 1, -1, x + 34, x + 86, 0)
    else:
        k.facade_piece(("shop_town_20_green", "shop_town_20_brown", "shop_town_20_red")[i % 3], "x", YN - 1, -1, x + 12, x + 108, 24)
    arch(k, "x", x, x + 120, YN - 2, YN, 120, 180, {"-y": RH, "sides": NODRAW, "bottom": T("concrete_wall"), "top": NODRAW}, n=6, deco=True)
    k.panel("x", x - 14, x, YN, -1, 0, 184, 3, T("concrete_wall"))
    k.panel("x", x + 120, x + 134, YN, -1, 0, 184, 3, T("concrete_wall"))
# Stepped gable in the middle.
for i, (xa, xb) in enumerate(((0, 600), (60, 540), (120, 480), (180, 420), (240, 360))):
    z0 = 460 + i * 70
    k.blk((xa, YN, z0), (xb, YN + 60, z0 + 70), {"sides": RH, "top": T("concrete_wall"), "bottom": NODRAW, "+y": NODRAW})
k.add(prism((300, YN - 1), 50, 560, 561.5, 12, {"all": T("plaster_white")}, axis="y"), deco=True)
k.panel("x", 297, 303, YN - 1.5, -1, 560, 600, 1, TRIM)
k.gable_roof(-600, YN, 1198, YN + 400, 460, 200, axis="x", tex=T("roof_clay"), gable=RH, over=20, hide="n", over_end=0)
k.tree(-350, 1650, 0, 440, 140)
k.bench(-350, 1500, 0, "x")
k.bench(900, 1500, 0, "x")
k.crate(700, 1800, 0, 48)
k.crate(760, 1810, 0, 40)
for x in (-500, 1100):
    k.lamp(x, 1950, 0, 240, "-y", T("metal_painted_green"))

# ---------------------------------------------------------------------------------------------------------------
# Keep players in: clip walls around the playable area and a lid.
k.clip((XW - 40, YS - 40, 0), (XW, YN + 40, 1000))
k.clip((XE, YS - 40, 0), (XE + 40, YN + 40, 1000))
k.clip((XW, YS - 40, 0), (XE, YS, 1000))
k.clip((XW, YN, 0), (XE, YN + 40, 1000))
k.clip((XW, YS - 40, 1000), (XE, YN + 40, 1016))

# ---------------------------------------------------------------------------------------------------------------
spawns(m, "info_player_opfor", [(-240, -2150, 0), (-120, -2230, 0), (0, -2150, 0), (120, -2230, 0), (240, -2150, 0)], 90)
spawns(m, "info_player_ksk", [(0, 1800, 0), (130, 1880, 0), (260, 1800, 0), (390, 1880, 0), (520, 1800, 0)], 270)
m.volume("func_buyzone", (-600, YS, 0), (600, -1850, 160), team="opfor")
m.volume("func_buyzone", (-600, 1500, 0), (1200, YN, 160), team="ksk")
m.volume("func_bomb_target", (-1900, 650, 0), (-1250, 1300, 128), site="A")
m.volume("func_bomb_target", (1300, 500, 0), (2000, 1150, 128), site="B")
dm = [(-1600, -1700, 90), (-1590, -500, 90), (-2000, 500, 0), (-1300, 1350, 270), (-750, 1300, 0), (0, 1500, 270),
      (425, 1000, 270), (-400, -100, 0), (400, -500, 90), (0, -1200, 90), (-900, -870, 0), (1000, -2000, 0),
      (1500, -1000, 90), (1200, 600, 0), (2000, 900, 180), (800, 400, 270)]
for x, y, a in dm:
    m.point("info_player_dm", (x, y, 4), angle=a)
places(m, [(name, (lo[0], lo[1], 0), (hi[0], hi[1], 400)) for name, (lo, hi, _) in W.items()] + [
    ("OPFOR-Start", (-600, YS, 0), (600, -1800, 400)),
    ("A", (-1950, 600, 0), (-1200, 1350, 400)),
    ("Brunnen", (FX - 200, FY - 200, 0), (FX + 200, FY + 200, 400)),
    ("Kirche", (PX - 200, NY0 - 220, 0), (PX + 200, NY0, 400)),
    ("KSK-Start", (-600, 1500, 0), (1200, YN, 400)),
    ("Bogen", (300, AY0 - 50, 0), (550, AY1 + 50, 400)),
    ("B", (1250, 450, 0), (2050, 1200, 400)),
    ("Rampe", (2140, 380, 0), (2300, 1300, 400)),
])
m.point("light_environment", (0, 0, 900), pitch="-30", yaw="25", color="255 214 168", ambient="118 126 150")

print("hidden tops:", k.hide_tops(250))
# Street name plates on a facade at each end of the lanes and squares that have one.
PLATES = {"Lange Gasse": "street_lange_gasse", "Marktstraße": "street_marktstrasse", "Kirchgasse": "street_kirchgasse",
          "Schmiedgasse": "street_schmiedgasse", "Mauergasse": "street_mauergasse", "Bäckergasse": "street_baeckergasse",
          "Rathausplatz": "street_rathausplatz", "Kirchplatz": "street_kirchplatz"}
for area, plate in PLATES.items():
    lo, hi, _ = W[area]
    placed = 0
    for f in k.facades:
        if placed >= 2:
            break
        # A facade bordering the area (its face on the area's edge, facing into it), the plate near its end.
        if f["axis"] == "x":
            if not ((abs(f["face"] - lo[1]) < 2 and f["out"] > 0) or (abs(f["face"] - hi[1]) < 2 and f["out"] < 0)):
                continue
            a0, a1 = max(f["a0"], lo[0]), min(f["a1"], hi[0])
        else:
            if not ((abs(f["face"] - lo[0]) < 2 and f["out"] > 0) or (abs(f["face"] - hi[0]) < 2 and f["out"] < 0)):
                continue
            a0, a1 = max(f["a0"], lo[1]), min(f["a1"], hi[1])
        if a1 - a0 < 80:
            continue
        for u in (a0 + 30, a1 - 30):
            if any(w[0] - 25 < u < w[1] + 25 and w[2] < 150 for w in f["wins"] + f["doors"]):
                continue
            at = (u, f["face"], 118) if f["axis"] == "x" else (f["face"], u, 118)
            k.decal(plate, at, ("+" if f["out"] > 0 else "-") + ("y" if f["axis"] == "x" else "x"))
            placed += 1
            break
k.dress(style="town", budget=260)
k.finish()
seen = [(o, c) for o in [(-240 + 120 * i, -2150 - 80 * (i % 2), 64) for i in range(5)]
        for c in [(130 * i, 1800 + 80 * (i % 2), 64) for i in range(5)] if k.visible(o, c)]
if seen:
    print("WARNING: spawns see each other:", seen[:4])
k.safety_floor(XW - 200, YS - 200, XE + 200, YN + 200)
m.save(os.path.join(os.path.dirname(__file__), "../../assets/maps/zg_altstadt.map"))
