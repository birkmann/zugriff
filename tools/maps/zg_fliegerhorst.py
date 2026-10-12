"""zg_fliegerhorst: hostage rescue on a Luftwaffe air base.

South: the KSK start outside the perimeter fence (the rescue zone), two gaps in the fence. Across the grass: the long
taxiway ("Rollweg", the long sightline). West: the hardened aircraft shelter ("Shelter", a concrete vault with a jet
inside, front and rear doors). Mid: the base road ("Mitte") between the shelter and the fuel depot. East: the fuel
depot ("Tanklager": tanks inside blast walls) and the helicopter hardstand ("Vorfeld Ost"). North: the two-storey
operations building ("Stab"), four hostages upstairs; inside stairs and an outside fire escape. OPFOR start behind it
("Kasernenhof"). The control tower stands in the north-west corner.

    python3 tools/maps/zg_fliegerhorst.py
"""

import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(__file__))
from zmap import Map, hull, prism, stairs  # noqa: E402
from zkit import NODRAW, TRIM, Kit, T, fit, frustum, hash_unit, places, props_info, spawns, vault, vault_wall  # noqa: E402

m = Map("zg_fliegerhorst")
k = Kit(m)
k.container_props = True
k.facade_style = "office"
rng = random.Random(5)

XW, XE, YS, YN = -2500, 2500, -2500, 2100
FENCE_Y = -1600
ASPH, CONC, GRASS, GRAVEL = T("gnd_asphalt"), T("gnd_concrete"), T("grass"), T("gravel")
PAINT = T("plaster_white")


def paint(x0, y0, x1, y1):
    k.dblk((x0, y0, 0), (x1, y1, 0.5), {"top": PAINT, "sides": NODRAW, "bottom": NODRAW})


# ---------------------------------------------------------------------------------------------------------------
# Ground.
k.ground(-1300, YS, 300, FENCE_Y, GRASS, 640)  # KSK start outside the fence
k.ground(XW, FENCE_Y, XE, -1100, GRASS, 700)
k.ground(XW, -1100, XE, -700, ASPH, 640)  # taxiway
k.ground(-2200, -700, -800, -300, CONC, 700)  # shelter apron
k.ground(XW, -700, -2200, 1700, ASPH, 640)  # west road
k.ground(-800, -700, 300, 1400, ASPH, 640)  # mid road
k.ground(300, -700, 1600, 800, GRAVEL, 700)  # depot
k.ground(1600, -700, XE, 600, CONC, 700)  # east hardstand
k.ground(1500, 600, 1900, 1700, ASPH, 640)  # east yard
k.ground(-2200, 1100, 300, 1700, ASPH, 640)  # north road (behind the shelter)
k.ground(300, 1400, 1500, 1700, ASPH, 640)
k.ground(300, 1700, 1900, YN, CONC, 640)  # OPFOR yard
k.ground(-2200, -300, -2120, 1100, GRASS, 640)
k.ground(-880, -300, -800, 1100, GRASS, 640)
# Taxiway centre line and edge markings, the holding position bars at the shelter apron.
paint(XW, -904, XE, -896)
for x in range(XW, XE, 300):
    paint(x, -1096, x + 150, -1090)
    paint(x, -710, x + 150, -704)
for x in range(-1950, -1050, 60):
    paint(x, -690, x + 30, -660)
# Taxiway edge lights (small blue posts).
for x in range(XW + 150, XE, 400):
    for y in (-1130, -670):
        k.post(x, y, 0, 14, 5, T("metal_painted_blue"))

# ---------------------------------------------------------------------------------------------------------------
# Perimeter fence with two gaps; outside it the KSK vehicles and trees.
GAPS = [(-1100, -900), (0, 150)]
xs = [XW] + [g for gap in GAPS for g in gap] + [XE]
for a, b in zip(xs[0::2], xs[1::2]):
    k.fence(a, FENCE_Y, b, FENCE_Y, 0, 110, 128, T("metal_painted_green"), rails=(0.05, 0.5, 0.97), clip_h=1000, mesh=False)
    k.dblk((a, FENCE_Y - 1, 110), (b, FENCE_Y + 1, 112), TRIM)  # barbed wire strand
    k.dblk((a, FENCE_Y - 1, 120), (b, FENCE_Y + 1, 122), TRIM)
# Cut fence ends bent inward, an open barrier at the east gap.
k.blk((-1112, FENCE_Y - 10, 0), (-1100, FENCE_Y + 10, 120), T("metal_painted_green"))
k.blk((-900, FENCE_Y - 10, 0), (-888, FENCE_Y + 10, 120), T("metal_painted_green"))
k.blk((-20, FENCE_Y - 20, 0), (0, FENCE_Y + 20, 44), T("metal_painted_red"))
k.bar((-10, FENCE_Y, 44), (-5, FENCE_Y + 10, 260), 5, T("metal_painted_red"))
k.van(-900, -2250, 0, "+y", "metal_painted_green")
k.van(-300, -2350, 0, "+y", "metal_painted_green")
k.truck(250, -2000, 0, "+y", "metal_painted_green", "box")
for x, y in ((-1200, -2400), (-1150, -1900), (200, -2450), (-700, -2450)):
    k.tree(x, y, 0, 420, 130)
k.crate(-500, -1800, 0, 48)
k.crate(-560, -1790, 0, 40)
# Out of bounds outside the fence beyond the KSK meadow.
k.clip((-1300 - 16, YS, 0), (-1300, FENCE_Y, 1000))
k.clip((300, YS, 0), (316, FENCE_Y, 1000))
for x in range(XW + 200, -1400, 500):
    k.tree(x, -2000 + rng.randint(-200, 200), 0, 440, 140)
for x in range(700, XE, 500):
    k.tree(x, -2000 + rng.randint(-200, 200), 0, 440, 140)

# ---------------------------------------------------------------------------------------------------------------
# Hardened aircraft shelter: a concrete vault (x -2120..-880 outside), front doors south, a small rear door north.
SX0, SX1, SY0, SY1 = -2060, -940, -300, 1100
RISE, TH = 360, 60
SH = T("concrete_wall")
zin, zout = vault(k, SX0, SX1, SY0, SY1, 0, RISE, TH, SH, inner=T("concrete_wall_b"), n=12)
DOOR = (-1850, -1150, 290)
vault_wall(k, SX0, SX1, SY0, SY0 + 40, zin, [DOOR], SH)
vault_wall(k, SX0, SX1, SY1 - 40, SY1, zin, [(-1560, -1440, 120)], SH)
k.ground(SX0, SY0, SX1, SY1, T("concrete_hangar"), 640, -16, 0.01)
# Steel ribs follow the concrete vault. Service rails carry broad, neutral work lights.
for y in (-160, 140, 440, 740, 1020):
    xs = [SX0 + (SX1 - SX0) * i / 12 for i in range(13)]
    for xa, xb in zip(xs, xs[1:]):
        k.bar((xa, y, max(6, zin(xa) - 8)), (xb, y, max(6, zin(xb) - 8)), 10, TRIM)
for x in (-1780, -1220):
    h = zin(x) - 30
    k.bar((x, -160, h), (x, 1020, h), 5, TRIM)
    for y in (-80, 260, 600, 940):
        k.dblk((x - 5, y - 48, h - 5), (x + 5, y + 48, h), TRIM)
        m.point("light_zg", (x, y, h - 6), kind="area", power="350",
                color="236 242 255", dir="0 0 -1", size="2.3 0.18", along="0 1 0")
# Worn paint decals mark the aircraft bay without thin coplanar brush faces.
for y in (-220, 960):
    k.decal("taxiway_line", (-1500, y, 0.7), "+z", size=(17.3, 0.18))
for y in range(-260, 920, 100):
    k.decal("taxiway_line", (-1500, y + 27, 0.7), "+z", size=(0.16, 1.4))
for x in (-1840, -1160):
    k.decal("hazard_stripes", (x, 360, 0.7), "+z", size=(0.22, 30.0))
# Maintenance stations stay against the walls, leaving both exits and aisles open.
for x, yaw in ((-1970, 90), (-1030, 270)):
    for y in (170, 850):
        k.prop("steel_frame_shelves_01", x, y, 0.25, yaw=yaw, solid=True)
        k.prop("metal_tool_chest", x, y + 70, 0.25, yaw=yaw, solid=True)
        k.prop("korean_fire_extinguisher_01", x, y - 45, 0.25, yaw=yaw)
        k.prop("ammo_box", x, y, 36, yaw=yaw, solid=False)
    # Workbench with steel legs, a timber top and equipment.
    for y in (420, 530):
        for xx in (x - 18, x + 18):
            k.post(xx, y, 0, 36, 3, TRIM)
    k.blk((x - 23, 412, 36), (x + 23, 538, 40), T("wood_beam"))
    k.prop("metal_jerrycan_green", x, 450, 40, yaw=yaw, solid=False)
    k.prop("ammo_box", x, 505, 40, yaw=yaw, solid=False)
# The sliding door leaves, pushed open to the sides in front of the shelter.
for xa, xb in ((SX0 - 120, -1850), (-1150, SX1 + 120)):
    k.blk((xa, SY0 - 70, 0.25), (xb, SY0 - 30, 300), {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW})
k.blk((SX0 - 140, SY0 - 80, 0), (SX1 + 140, SY0 - 20, 6), {"sides": T("metal_rust"), "top": T("metal_rust"), "bottom": NODRAW})
# Front buttresses and the exhaust deflector at the rear.
for x in (SX0 - 60, SX1 + 20):
    k.blk((x, SY0 - 40, 0), (x + 40, SY0 + 60, 320), {"sides": SH, "top": SH, "bottom": NODRAW})
k.add(hull([(-1650, SY1, 0), (-1350, SY1, 0), (-1650, SY1 + 160, 0), (-1350, SY1 + 160, 0),
            (-1650, SY1, 200), (-1350, SY1, 200)], SH))


# Nexus MiG-29, with a fuselage collision hull and open space beneath the wings.
k.prop("veh_mig29", -1500, 410, 0.25, yaw=90, solid=True)
# Ground equipment and supplies sit outside the marked aircraft bay.
k.prop("pallet_boxes", -1940, 730, 0.25, yaw=90, solid=True)
k.prop("pallet", -1070, 710, 0.25, yaw=90)
k.prop("wooden_military_crate", -1070, 710, 6, yaw=90, solid=True)
for x in (-1800, -1200):
    k.lamp(x, -500, 0, 280, "+y")
# Sandbag walls on the apron (cover in front of the shelter).
for x0, y0, x1, y1 in ((-2150, -620, -1950, -560), (-1000, -560, -820, -500)):
    k.sandbag_wall((x0, y0, 0), (x1, y1, 40))

# ---------------------------------------------------------------------------------------------------------------
# West road with the control tower (north-west corner) and a fire truck.
TWX0, TWX1, TWY0, TWY1 = -2480, -2240, 1250, 1490
k.solid_building(TWX0, TWY0, TWX1, TWY1, 0, 620, T("concrete_painted"), sides="nsew", bays=120, floors=[60, 220, 380],
                 win_w=40, win_h=56, plinth=T("concrete_wall"), top=NODRAW, frame=TRIM, cross=False, door=("e", 1370, 64, 100),
                 door_tex=T("metal_painted_blue"))
k.blk((TWX0 - 70, TWY0 - 70, 620), (TWX1 + 70, TWY1 + 70, 640), {"sides": T("concrete_wall"), "top": T("concrete_wall"), "bottom": T("concrete_wall")})
k.blk((TWX0 - 60, TWY0 - 60, 640), (TWX1 + 60, TWY1 + 60, 740), {"sides": TRIM, "top": NODRAW, "bottom": NODRAW})
for x in range(TWX0 - 60, TWX1 + 61, 60):
    for yf, out in ((TWY0 - 60, -1), (TWY1 + 60, 1)):
        k.panel("x", x - 3, x + 3, yf, out, 640, 740, 2, T("concrete_painted"))
for y in range(TWY0 - 60, TWY1 + 61, 60):
    for xf, out in ((TWX0 - 60, -1), (TWX1 + 60, 1)):
        k.panel("y", y - 3, y + 3, xf, out, 640, 740, 2, T("concrete_painted"))
k.blk((TWX0 - 80, TWY0 - 80, 740), (TWX1 + 80, TWY1 + 80, 760), {"sides": T("concrete_wall"), "top": NODRAW, "bottom": T("concrete_wall")})
k.post((TWX0 + TWX1) / 2, (TWY0 + TWY1) / 2, 760, 980, 6, TRIM)
k.add(prism(((TWX0 + TWX1) / 2, (TWY0 + TWY1) / 2), 50, 790, 830, 8, {"sides": T("plaster_white"), "top": NODRAW, "bottom": T("plaster_white")}), deco=True)


def fire_truck(x, y):
    """An airfield crash tender: a long red body with a white roof bar and six wheels."""
    R = T("metal_painted_red")
    k.blk((x - 48, y - 260, 24), (x + 48, y + 260, 120), {"sides": R, "top": R, "bottom": NODRAW})
    k.blk((x - 44, y + 170, 120), (x + 44, y + 250, 150), {"sides": TRIM, "top": R, "bottom": NODRAW})
    k.dblk((x - 48, y - 200, 70), (x + 48, y + 150, 80), {"sides": T("plaster_white"), "top": NODRAW, "bottom": NODRAW})
    k.dblk((x - 20, y - 100, 120), (x + 20, y + 60, 132), {"sides": T("metal_plate"), "top": T("metal_plate"), "bottom": NODRAW})
    for yy in (y + 180, y - 60, y - 180):
        for s in (-1, 1):
            k.wheel(x + s * 46, yy, 0.25, 22, 12, axis="x")


fire_truck(-2350, 400)
k.blk((-2450, -400, 0.25), (-2300, -250, 60), {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW})
k.crate(-2300, 900, 0, 64)
k.crate(-2250, 950, 0, 48)
for y in (-200, 800):
    k.lamp(-2470, y, 0, 280, "+x")
# Earth berms along the west edge (backdrop), shaped as slopes.
k.add(hull([(XW, -700, 0), (XW, 1700, 0), (XW + 60, -700, 0), (XW + 60, 1700, 0), (XW, -700, 140), (XW, 1700, 140)],
           {"sides": GRASS, "top": GRASS, "bottom": NODRAW}))

# ---------------------------------------------------------------------------------------------------------------
# Mid road: concrete barriers, a fuel bowser truck, a guard post.
for x0, y0, ln, ax in ((-600, 0, 320, "x"), (-50, 400, 320, "y"), (-700, 900, 320, "x")):
    if "concrete_road_barrier" in props_info():
        # A row of precast barriers over the brush one's hull (invisible collision).
        pts = [(x0, y0, 0), (x0 + ln, y0, 0), (x0, y0 + 36, 0), (x0 + ln, y0 + 36, 0), (x0, y0 + 12, 36), (x0 + ln, y0 + 12, 36), (x0, y0 + 24, 36), (x0 + ln, y0 + 24, 36)]
        if ax != "x":
            pts = [(x0 + (p[1] - y0), y0 + (p[0] - x0), p[2]) for p in pts]
        k.add(hull(pts, NODRAW))
        seg = props_info()["concrete_road_barrier"]["size"][0] * 39.37
        n = max(1, round(ln / seg))
        for i in range(n):
            u = ln * (i + 0.5) / n
            x, y = (x0 + u, y0 + 18) if ax == "x" else (x0 + 18, y0 + u)
            k.prop("concrete_road_barrier", x, y, 0, yaw=(90 if ax == "x" else 0) + 180 * (i % 2), solid=False)
        continue
    if ax == "x":
        k.add(hull([(x0, y0, 0), (x0 + ln, y0, 0), (x0, y0 + 36, 0), (x0 + ln, y0 + 36, 0),
                    (x0, y0 + 12, 36), (x0 + ln, y0 + 12, 36), (x0, y0 + 24, 36), (x0 + ln, y0 + 24, 36)], T("concrete_painted")))
    else:
        k.add(hull([(x0, y0, 0), (x0, y0 + ln, 0), (x0 + 36, y0, 0), (x0 + 36, y0 + ln, 0),
                    (x0 + 12, y0, 36), (x0 + 12, y0 + ln, 36), (x0 + 24, y0, 36), (x0 + 24, y0 + ln, 36)], T("concrete_painted")))
# Fuel bowser: cab and a horizontal tank.
BX, BY = -300, 700
if "fuel_bowser" in props_info():
    # The modelled bowser (arch.py) over the brush one's shape, kept as invisible collision.
    k.blk((BX - 45, BY + 150, 20), (BX + 45, BY + 240, 110), NODRAW)
    k.blk((BX - 40, BY - 250, 20), (BX + 40, BY + 150, 34), NODRAW)
    k.add(prism((BX, 80), 46, BY - 240, BY + 140, 12, NODRAW, rot=15, axis="y"))
    k.prop("fuel_bowser", BX, BY - 5, 0, yaw=90, scale=1.12, solid=False)
else:
    k.blk((BX - 45, BY + 150, 20), (BX + 45, BY + 240, 110), {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW})
    k.panel("y", BX - 40, BX + 40, BY + 240, 1, 70, 104, 1, TRIM)
    k.blk((BX - 40, BY - 250, 20), (BX + 40, BY + 150, 34), {"sides": TRIM, "top": NODRAW, "bottom": NODRAW})
    k.add(prism((BX, 80), 46, BY - 240, BY + 140, 12, lambda n: T("metal_painted_green") if abs(n[1]) < 0.5 else T("metal_plate"), rot=15, axis="y"))
    for yy in (BY + 200, BY - 120, BY - 200):
        for s in (-1, 1):
            k.wheel(BX + s * 40, yy, 0.25, 18, 10, axis="x")
# Guard post (a small hut with a flat roof).
k.solid_building(100, -600, 280, -450, 0, 110, T("plaster_white"), sides="nsw", bays=80, floors=[44], win_w=60, win_h=40,
                 plinth=T("concrete_wall"), top=NODRAW, frame=TRIM, cross=False, door=("n", 230, 44, 90), door_tex=T("metal_painted_blue"))
k.blk((90, -610, 110), (290, -440, 122), {"sides": T("metal_painted_blue"), "top": NODRAW, "bottom": T("metal_painted_blue")})
k.lamp(-750, 400, 0, 280, "+x")
# More cover on the mid road: containers, a parked truck, a sandbag position.
k.container(-620, 260, 0, 240, "x", "green")
k.container(-20, 820, 0, 240, "y", "green")
k.container(-20, 820, 102, 240, "y", "metal_corrugated")
k.truck(-650, -150, 0, "+y", "metal_painted_green", "box")
for x0, y0, x1, y1 in ((-260, -420, -60, -380), (-260, -380, -220, -240), (-100, -380, -60, -240)):
    k.sandbag_wall((x0, y0, 0), (x1, y1, 44))
k.crate(150, 300, 0, 56)
k.crate(150, 356, 0, 48)
k.lamp(250, 1200, 0, 280, "-x")

# ---------------------------------------------------------------------------------------------------------------
# Fuel depot: three tanks inside blast walls, horizontal tanks, a pump house, pipes.
BW = T("concrete_wall")


def blast_wall(x0, y0, x1, y1, h=88):
    """A row of precast T-wall segments (arch.py t_wall) over the wall's box (kept as invisible collision); a plain
    concrete block where the model isn't built."""
    if "t_wall" not in props_info():
        k.blk((x0, y0, 0), (x1, y1, h), {"sides": BW, "top": T("concrete_painted"), "bottom": NODRAW})
        return
    k.blk((x0, y0, 0), (x1, y1, h), NODRAW)
    along_x = (x1 - x0) >= (y1 - y0)
    L = (x1 - x0) if along_x else (y1 - y0)
    seg = props_info()["t_wall"]["size"][0] * 39.37
    n = max(1, round(L / seg))
    sc = h / (props_info()["t_wall"]["size"][1] * 39.37)
    for i in range(n):
        u = (x0 if along_x else y0) + L * (i + 0.5) / n
        x, y = (u, (y0 + y1) / 2) if along_x else ((x0 + x1) / 2, u)
        # (a hair's turn each, as they stand when craned into place)
        k.prop("t_wall", x, y, 0, yaw=(0 if along_x else 90) + 180 * (i % 2) + 1.5 * (hash_unit(x, y) - 0.5), scale=round(sc, 3), solid=False)


blast_wall(420, -520, 1000, -480)
blast_wall(1150, -520, 1450, -480)
blast_wall(420, -480, 460, 250)
blast_wall(1410, -480, 1450, 100)
blast_wall(420, 420, 900, 460)
blast_wall(1100, 420, 1450, 460)
for (tx, ty) in ((650, -250), (1150, -230), (700, 180)):
    k.tank(tx, ty, 0, 130, 250, T("metal_painted_green"), sides=12)
    k.post(tx + 136, ty, 0, 280, 4, T("metal_plate"))  # ladder rail
k.htank(900, 1300, 250, 0, 40, T("plaster_white"), axis="x")
# pipes along the depot floor
for y in (-420, -400):
    k.add(prism((y, 14), 8, 500, 1350, 6, T("metal_rust"), axis="x"), deco=True)
k.clip((500, -430, 0), (1350, -390, 26))
k.solid_building(1200, 150, 1400, 380, 0, 140, T("bricks090"), sides="nws", bays=100, floors=[60], win_w=50, win_h=40,
                 plinth=T("concrete_wall"), top=NODRAW, frame=TRIM, cross=False, door=("w", 260, 48, 100), door_tex=T("metal_painted_red"))
k.blk((1190, 140, 140), (1410, 390, 150), {"sides": T("concrete_wall"), "top": NODRAW, "bottom": T("concrete_wall")})
k.crate(950, 300, 0, 56)
k.crate(1000, 330, 0, 48)
k.barrel(560, 350, 0, 14, 38, T("metal_painted_red"))
k.barrel(590, 380, 0, 14, 38, T("metal_painted_red"))
k.barrel(1350, -100, 0, 14, 38, T("metal_painted_blue"))

# ---------------------------------------------------------------------------------------------------------------
# East hardstand: a transport helicopter, a hangar front (backdrop), a fuel truck, lights.


def helicopter(x, y):
    """A medium transport helicopter silhouette (nose to +y): cabin, nose, tail boom, fin, rotor, skids/wheels."""
    G = T("metal_painted_green")
    k.blk((x - 60, y - 120, 30), (x + 60, y + 160, 150), {"sides": G, "top": G, "bottom": NODRAW})
    k.add(hull([(x - 60, y + 160, 30), (x + 60, y + 160, 30), (x - 60, y + 160, 150), (x + 60, y + 160, 150),
                (x - 40, y + 260, 50), (x + 40, y + 260, 50), (x - 36, y + 230, 120), (x + 36, y + 230, 120)], G))
    k.add(hull([(x - 30, y - 120, 90), (x + 30, y - 120, 90), (x - 30, y - 120, 150), (x + 30, y - 120, 150),
                (x - 10, y - 520, 110), (x + 10, y - 520, 110), (x - 10, y - 520, 140), (x + 10, y - 520, 140)], G))
    k.add(hull([(x - 5, y - 470, 110), (x + 5, y - 470, 110), (x - 5, y - 540, 120), (x + 5, y - 540, 120),
                (x - 5, y - 520, 240), (x + 5, y - 520, 240), (x - 5, y - 560, 240), (x + 5, y - 560, 240)], G))
    k.blk((x - 40, y - 60, 150), (x + 40, y + 100, 190), {"sides": G, "top": G, "bottom": NODRAW})
    k.post(x, y + 20, 190, 214, 10, TRIM)
    for a in (20, 110):
        r = math.radians(a)
        k.bar((x - 380 * math.cos(r), y + 20 - 380 * math.sin(r), 214), (x + 380 * math.cos(r), y + 20 + 380 * math.sin(r), 214), 5, TRIM)
    for s in (-1, 1):
        k.flat_window("y", x + s * 60, s, y + 10, y + 70, 90, 125, back=TRIM, cross=False)
        k.wheel(x + s * 50, y - 60, 0.25, 16, 10, axis="x")
    k.wheel(x, y + 200, 0.25, 12, 8, axis="x")


if "veh_nh90" in props_info():
    k.prop("veh_nh90", 2000, -120, 0.25, yaw=90, solid=True)
else:
    helicopter(2000, -150)
for x in range(1700, XE, 250):
    paint(x, 560, x + 120, 568)
paint(1780, -500, 2220, -492)
paint(1780, 200, 2220, 208)
k.truck(1750, 480, 0, "+x", "metal_painted_green", "box")
k.crate(2350, 300, 0, 56)
k.crate(2300, 340, 0, 48)
k.blk((2380, -600, 0.25), (2480, -500, 50), {"sides": T("metal_painted_red"), "top": T("metal_plate"), "bottom": NODRAW})
# Hangar front beyond the east edge (backdrop): corrugated walls, a big door, a vault roof.
vault(k, XE + 20, XE + 900, -700, 700, 420, 150, 30, T("metal_corrugated"), inner=NODRAW, n=6)
k.solid_building(XE + 20, -700, XE + 900, 700, 0, 420, T("metal_corrugated"), sides="w", bays=300, floors=[300], win_w=180,
                 win_h=60, top=NODRAW, frame=TRIM, cross=False)
fit(k.panel("y", -500, 300, XE + 20, -1, 0, 280, 2, T("rollershutter")), (8.0, 11.0))
for y in range(-500, 301, 100):
    k.panel("y", y - 2, y + 2, XE + 18, -1, 0, 280, 2, TRIM)
for x in (1650, 2400):
    k.lamp(x, -650, 0, 300, "+y")

# ---------------------------------------------------------------------------------------------------------------
# Operations building ("Stab"): x 300..1500, y 800..1400. Two floors (upper floor at 176), roof at 336.
OX0, OX1, OY0, OY1 = 300, 1500, 800, 1400
WT = 16
F2, F2T, RF = 160, 176, 336
PL = T("plaster_white")
INT = {"sides": T("plaster_b"), "top": NODRAW, "bottom": NODRAW}
EXT = {"sides": PL, "top": NODRAW, "bottom": NODRAW}
CY0, CY1 = 1060, 1200  # corridor
# Openings in the outer walls: (u0, u1, z0, z1).
south = [(860, 960, 0, 112)] + [(u, u + 90, 50, 120) for u in (380, 560, 1160, 1340)] + \
        [(u, u + 90, F2T + 40, F2T + 110) for u in (380, 560, 760, 960, 1160, 1340)]
north = [(860, 960, 0, 112)] + [(u, u + 90, 50, 120) for u in (1160, 1340)] + \
        [(u, u + 90, F2T + 40, F2T + 110) for u in (760, 960, 1160, 1340)]
west = [(CY0 + 20, CY1 - 20, 0, 112)] + [(v, v + 90, 50, 120) for v in (880,)] + [(v, v + 90, F2T + 40, F2T + 110) for v in (880, 1260)]
east = [(CY0 + 20, CY1 - 20, 0, 112), (CY0 + 20, CY1 - 20, F2T, F2T + 112)] + [(v, v + 90, 50, 120) for v in (880, 1260)] + \
       [(v, v + 90, F2T + 40, F2T + 110) for v in (880, 1260)]
wall_tex = {"-y": PL, "+y": T("plaster_white"), "sides": PL, "top": NODRAW, "bottom": NODRAW}
k.wall("x", OX0, OX1, OY0, OY0 + WT, 0, RF, {"sides": PL, "top": NODRAW, "bottom": NODRAW}, south)
k.wall("x", OX0, OX1, OY1 - WT, OY1, 0, RF, {"sides": PL, "top": NODRAW, "bottom": NODRAW}, north)
k.wall("y", OY0 + WT, OY1 - WT, OX0, OX0 + WT, 0, RF, {"sides": PL, "top": NODRAW, "bottom": NODRAW}, west)
k.wall("y", OY0 + WT, OY1 - WT, OX1 - WT, OX1, 0, RF, {"sides": PL, "top": NODRAW, "bottom": NODRAW}, east)
for u0, u1, v0, v1 in south:
    if v0 > 0:
        k.window("x", OY0, -1, u0, u1, v0, v1, depth=WT, back=None, frame=TRIM, sill=T("concrete_wall"))
for u0, u1, v0, v1 in north:
    if v0 > 0:
        k.window("x", OY1, 1, u0, u1, v0, v1, depth=WT, back=None, frame=TRIM, sill=T("concrete_wall"))
for u0, u1, v0, v1 in west:
    if v0 > 0:
        k.window("y", OX0, -1, u0, u1, v0, v1, depth=WT, back=None, frame=TRIM, sill=T("concrete_wall"))
for u0, u1, v0, v1 in east:
    if v0 > 0 and v1 - v0 < 100:
        k.window("y", OX1, 1, u0, u1, v0, v1, depth=WT, back=None, frame=TRIM, sill=T("concrete_wall"))
# Plinth, floor band, cornice/parapet, entrance canopies.
for axis, a0, a1, face, out in (("x", OX0, OX1, OY0, -1), ("x", OX0, OX1, OY1, 1), ("y", OY0, OY1, OX0, -1), ("y", OY0, OY1, OX1, 1)):
    ext = 4 if axis == "x" else 0
    k.band(axis, a0 - ext, a1 + ext, face, out, 0, 24, 4, T("concrete_wall"))
    k.band(axis, a0 - ext, a1 + ext, face, out, F2 - 4, F2T + 4, 3, T("concrete_painted"), show=("front", "top", "bottom"))
k.blk((OX0 - 8, OY0 - 8, RF), (OX1 + 8, OY1 + 8, RF + 16), {"sides": T("concrete_painted"), "top": NODRAW, "bottom": T("ceiling_tiles")})
for (x0, y0, x1, y1) in ((OX0 - 8, OY0 - 8, OX1 + 8, OY0 + 8), (OX0 - 8, OY1 - 8, OX1 + 8, OY1 + 8),
                         (OX0 - 8, OY0 + 8, OX0 + 8, OY1 - 8), (OX1 - 8, OY0 + 8, OX1 + 8, OY1 - 8)):
    k.blk((x0, y0, RF + 16), (x1, y1, RF + 48), {"sides": T("concrete_painted"), "top": T("concrete_wall"), "bottom": NODRAW})
for yf, out in ((OY0, -1), (OY1, 1)):
    ya, yb = sorted((yf, yf + out * 110))
    k.blk((840, ya, 128), (980, yb, 138), {"sides": T("concrete_painted"), "top": NODRAW, "bottom": T("concrete_painted")})
k.panel("x", 820, 1000, OY0, -1, 150, 190, 2, T("metal_painted_blue"), show=("front", "top", "bottom", "ends"))  # name board
k.panel("x", 830, 990, OY0 - 2, -1, 158, 182, 1, T("plaster_white"))
# Ground floor: floor, corridor walls with doors, room walls.
k.ground(OX0 + WT, OY0 + WT, OX1 - WT, OY1 - WT, T("tiles_floor_b"), 640, -16, 0.01)
g_doors_s = [(420, 490, 0, 112), (860, 960, 0, 112), (1250, 1320, 0, 112)]
g_doors_n = [(400, 470, 0, 112), (860, 960, 0, 112), (1250, 1320, 0, 112)]
u_doors_s = [(420, 490, F2T, F2T + 112), (860, 930, F2T, F2T + 112), (1250, 1320, F2T, F2T + 112)]
u_doors_n = [(560, 650, F2T, F2T + 112), (860, 930, F2T, F2T + 112)]
k.wall("x", OX0 + WT, OX1 - WT, CY0 - 8, CY0 + 8, 0, RF, INT, g_doors_s + u_doors_s + [(OX0 + WT, OX1 - WT, F2, F2T)])
k.wall("x", OX0 + WT, OX1 - WT, CY1 - 8, CY1 + 8, 0, RF, INT, g_doors_n + u_doors_n + [(OX0 + WT, OX1 - WT, F2, F2T)])
for x in (700, 1100):
    k.wall("y", OY0 + WT, CY0 - 8, x - 8, x + 8, 0, RF, INT, [(OY0 + WT, CY0 - 8, F2, F2T)])
k.wall("y", CY1 + 8, OY1 - WT, 700 - 8, 700 + 8, 0, RF, INT, [(CY1 + 8, OY1 - WT, F2, F2T)])
k.wall("y", CY1 + 8, OY1 - WT, 1100 - 8, 1100 + 8, 0, F2, INT)
# Upper floor slab (with the stairwell hole x 316..630, y 1280..1384) and ceilings.
SLAB = {"top": T("tiles_floor_b"), "bottom": T("ceiling_tiles"), "sides": T("plaster_b")}
k.blk((OX0 + WT, OY0 + WT, F2), (OX1 - WT, CY1 + 8, F2T), SLAB)
k.blk((OX0 + WT, CY1 + 8, F2), (OX1 - WT, 1280, F2T), SLAB)
k.blk((630, 1280, F2), (OX1 - WT, OY1 - WT, F2T), SLAB)
# Stairs up in the north-west room, a railing round the stairwell hole.
# (They start 46 units from the west wall, so a player fits in front of the first step.)
k.add(stairs((362, 1284, 0), (630, 1384, F2T), "+x", 11, {"top": T("concrete_painted"), "sides": T("concrete_wall"), "bottom": NODRAW}))
k.railing(316, 1280, 630, 1280, F2T, 40, TRIM)
# Outside fire escape on the east wall up to the upper door.
k.add(stairs((OX1 + 10, 760, 0), (OX1 + 110, 1060, F2T), "+y", 11, {"top": T("metal_plate"), "sides": T("metal_painted_green"), "bottom": NODRAW}))
k.blk((OX1 + 10, 1060, F2T - 8), (OX1 + 160, 1220, F2T), {"top": T("metal_plate"), "sides": T("metal_painted_green"), "bottom": T("metal_plate")})
for x, y in ((OX1 + 154, 1066), (OX1 + 154, 1214)):
    k.post(x, y, 0, F2T - 8, 8, T("metal_painted_green"), solid=True)
k.railing(OX1 + 160, 1060, OX1 + 160, 1220, F2T, 40, T("metal_painted_green"))
k.railing(OX1 + 10, 1220, OX1 + 160, 1220, F2T, 40, T("metal_painted_green"))
# Furniture: desks, lockers, a map table, chairs as blocks; hostage rooms with mattresses.
WOOD = {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW}
MET = {"sides": T("metal_painted_green"), "top": T("metal_painted_green"), "bottom": NODRAW}
PROPS = props_info()


def desks(lo, hi, chairs_side):
    """Office desks (Poly Haven's metal desk) side by side over the block lo..hi (its collision kept, invisible),
    chairs on `chairs_side` ("+y"/"-y"); the old plank block where the desk isn't built."""
    if "metal_office_desk" not in PROPS:
        k.blk(lo, hi, WOOD)
        return
    k.blk(lo, hi, NODRAW)
    n = max(1, int((hi[0] - lo[0]) // 80))
    yc = (lo[1] + hi[1]) / 2
    sgn = 1 if chairs_side == "+y" else -1
    for i in range(n):
        xc = lo[0] + (hi[0] - lo[0]) * (i + 0.5) / n
        # The desk's front (its drawers' side) towards the chair.
        k.prop("metal_office_desk", xc, yc, lo[2] - 0.25, yaw=90 if sgn > 0 else 270, solid=False)
        if "schoolchair_01" in PROPS:
            k.prop("schoolchair_01", xc + 10 * (hash_unit(xc) - 0.5), yc + sgn * 44, lo[2] - 0.25, yaw=(270 if sgn > 0 else 90) + 20 * (hash_unit(yc, xc) - 0.5))


def shelves(lo, hi, yaw):
    """Steel shelving along the block lo..hi (collision kept, invisible), facing `yaw`; the green block otherwise."""
    if "steel_frame_shelves_02" not in PROPS:
        k.blk(lo, hi, MET)
        return
    k.blk(lo, hi, NODRAW)
    along_x = (hi[0] - lo[0]) >= (hi[1] - lo[1])
    L = (hi[0] - lo[0]) if along_x else (hi[1] - lo[1])
    n = max(1, int(L // 24))
    for i in range(n):
        u = (lo[0] if along_x else lo[1]) + L * (i + 0.5) / n
        x, y = (u, (lo[1] + hi[1]) / 2) if along_x else ((lo[0] + hi[0]) / 2, u)
        k.prop("steel_frame_shelves_02", x, y, lo[2] - 0.25, yaw=yaw, solid=False)


desks((360, 830, 0.25), (560, 880, 30), "+y")  # guard desk
desks((1150, 830, 0.25), (1300, 880, 30), "+y")
shelves((1440, 830, 0.25), (1480, 1040, 80), 180)  # lockers / radio racks
shelves((1120, 1220, 0.25), (1300, 1260, 80), 90)
k.crate(1400, 1300, 0, 56)
k.crate(1440, 1250, 0, 48)
k.crate(800, 1300, 0, 48)
k.blk((800, 1260, F2T + 0.25), (1040, 1360, F2T + 30), WOOD)  # briefing table
k.blk((1200, 1260, F2T + 0.25), (1400, 1360, F2T + 30), WOOD)
desks((720, 830, F2T + 0.25), (840, 880, F2T + 30), "+y")  # office desk
# On the walls: pictures, a clock, fire alarms, extinguishers (their backs on the inner faces of the outer walls).
S_IN, N_IN = {"axis": "x", "face": OY0 + WT, "out": 1}, {"axis": "x", "face": OY1 - WT, "out": -1}
for f, u, z, model in ((S_IN, 715, 62, "hanging_picture_frame_01"), (S_IN, 1030, 95, "wall_clock"), (S_IN, 1100, 110, "fire_alarm"),
                       (S_IN, 700, F2T + 55, "hanging_picture_frame_01"), (S_IN, 1105, F2T + 55, "hanging_picture_frame_01"),
                       (N_IN, 520, 62, "hanging_picture_frame_01"), (N_IN, 660, 95, "wall_clock"),
                       (N_IN, 520, F2T + 55, "hanging_picture_frame_01"), (N_IN, 700, F2T + 100, "fire_alarm")):
    if model in PROPS:
        k._wall_prop(f, model, u, z)
for x, y, z in ((990, OY0 + WT + 12, 0), (600, OY1 - WT - 12, 0), (990, OY0 + WT + 12, F2T), (1450, CY0 - 30, F2T)):
    if "korean_fire_extinguisher_01" in PROPS:
        k.prop("korean_fire_extinguisher_01", x, y, z, yaw=90 if y < 1100 else 270)
for x0 in (340, 1140):
    k.blk((x0, 830, F2T + 0.25), (x0 + 110, 890, F2T + 8), {"sides": T("metal_painted_blue"), "top": T("metal_painted_blue"), "bottom": NODRAW})
k.crate(620, 1000, F2T, 48)
k.crate(1420, 1000, F2T, 48)

# ---------------------------------------------------------------------------------------------------------------
# North: the road behind the shelter and the OPFOR barracks yard with trucks; barracks blocks (backdrop).
k.truck(1600, 1950, 0, "-x", "metal_painted_green", "box")
k.truck(700, 1800, 0, "+x", "metal_painted_green", "container", "green")
k.crate(1700, 1780, 0, 56)
k.crate(1750, 1800, 0, 48)
k.solid_building(1900, 600, XE, YN, 0, 360, T("plaster_white"), sides="w", bays=180, floors=[60, 220], win_w=90, win_h=70,
                 plinth=T("concrete_wall"), top=NODRAW, cornice=T("concrete_painted"), frame=TRIM, cross="frame")
k.solid_building(-2200, 1700, 300, YN + 300, 0, 300, T("bricks090"), sides="s", bays=200, floors=[60, 190], win_w=90, win_h=64,
                 plinth=T("concrete_wall"), top=NODRAW, cornice=T("concrete_painted"), frame=TRIM, cross="frame")
k.solid_building(300, YN, 1900, YN + 300, 0, 320, T("plaster_white"), sides="s", bays=200, floors=[60, 200], win_w=90, win_h=64,
                 plinth=T("concrete_wall"), top=NODRAW, cornice=T("concrete_painted"), frame=TRIM, cross="frame")
for x in (-1600, -600, 600, 1400):
    k.lamp(x, 1680, 0, 280, "-y")
k.blk((-1000, 1450, 0.25), (-760, 1546, 102), {"sides": T("metal_painted_green"), "top": NODRAW, "bottom": NODRAW})  # a shelter container
k.crate(-400, 1500, 0, 56)

# ---------------------------------------------------------------------------------------------------------------
# Keep players in.
k.clip((XW - 40, FENCE_Y, 0), (XW, YN + 40, 1000))
k.clip((XE, FENCE_Y, 0), (XE + 40, YN + 40, 1000))
k.clip((XW, YN, 0), (XE, YN + 40, 1000))
k.clip((-1300, YS - 40, 0), (300, YS, 1000))
k.clip((XW, YS - 40, 1000), (XE, YN + 40, 1016))

# ---------------------------------------------------------------------------------------------------------------
spawns(m, "info_player_ksk", [(-700, -2100, 0), (-580, -2180, 0), (-460, -2100, 0), (-340, -2180, 0), (-220, -2100, 0)], 90)
spawns(m, "info_player_opfor", [(900, 1880, 0), (1020, 1960, 0), (1140, 1880, 0), (1260, 1960, 0), (1380, 1880, 0)], 270)
m.volume("func_buyzone", (-1300, YS, 0), (300, FENCE_Y - 20, 160), team="ksk")
m.volume("func_buyzone", (300, 1700, 0), (1900, YN, 160), team="opfor")
m.volume("func_hostage_rescue", (-1300, YS, 0), (300, FENCE_Y - 20, 160))
# Beside the mattresses (x 340..450 and 1140..1250, y 830..890), not on them.
for x, y in ((420, 930), (560, 960), (1240, 930), (1380, 960)):
    m.point("info_hostage", (x, y, F2T + 4), angle=90)
dm = [(-2300, -900, 0), (-1500, -500, 90), (-1250, 800, 270), (-2300, 1000, 270), (-1500, 1400, 0), (-300, 1300, 270),
      (-400, -200, 90), (0, -900, 180), (800, -800, 90), (900, 0, 90), (1900, -500, 90), (2300, 200, 180),
      (1700, 1200, 180), (600, 1100, 0), (1300, 1100, 180), (-600, -1300, 90)]
for x, y, a in dm:
    m.point("info_player_dm", (x, y, 4), angle=a)
places(m, [
    ("KSK-Start", (-1300, YS, 0), (300, FENCE_Y, 400)),
    ("Zaun", (XW, FENCE_Y, 0), (XE, -1100, 400)),
    ("Rollweg", (XW, -1100, 0), (XE, -700, 400)),
    ("Vorfeld", (-2200, -700, 0), (-800, -300, 400)),
    ("Shelter", (SX0, SY0, 0), (SX1, SY1, 500)),
    ("Westweg", (XW, -700, 0), (-2200, 1700, 400)),
    ("Tower", (XW, 1100, 0), (-2100, 1700, 400)),
    ("Mitte", (-800, -700, 0), (300, 1400, 400)),
    ("Tanklager", (300, -700, 0), (1600, 800, 400)),
    ("Vorfeld Ost", (1600, -700, 0), (XE, 600, 400)),
    ("Hof Ost", (1500, 600, 0), (1900, 1700, 400)),
    ("Nordstraße", (-2200, 1100, 0), (1500, 1700, 400)),
    ("Stab", (OX0, OY0, 0), (OX1, OY1, F2)),
    ("Stab OG", (OX0, OY0, F2), (OX1, OY1, RF)),
    ("Treppe", (316, 1200, 0), (700, OY1, RF)),
    ("Feuertreppe", (OX1, 760, 0), (OX1 + 170, 1230, 400)),
    ("OPFOR-Start", (300, 1700, 0), (1900, YN, 400)),
])
m.point("light_environment", (0, 0, 900), pitch="-58", yaw="70", color="255 246 232", ambient="125 140 168")

print("hidden tops:", k.hide_tops(200, keep=lambda f, b: False))
# ---------------------------------------------------------------------------------------------------------------
# Dressing: models and markings of a working air base (clear of the solids; along walls and edges).
placed = 0
# Shelter: its number over the door, kit along the inside walls.
k.decal("hangar_01", (-1500, SY0, 326), "-y", size=(1.5, 1.5))
k.decal("hazard_stripes", (-1500, SY0 - 12, 0.01), "+z", size=(7.0, 0.3))
placed += k.props_at("metal_tool_chest", [(-1960, -180, 90), (-1040, 40, 270)])
placed += k.props_at("portable_generator", [(-1960, 540, 90)])
placed += k.props_at("metal_jerrycan_green", [(-1965, 620, 90), (-1965, 640, 95), (-1965, 660, 85)])
placed += k.props_at("wooden_military_crate", [(-1040, 480, 270), (-1040, 530, 270)])
# East hardstand: the hangar's number, kit by the helicopter and the fuel truck.
k.decal("hangar_02", (XE + 20, 480, 180), "-x", size=(1.6, 1.6))
placed += k.props_at("portable_generator", [(2240, -420, 180)])
placed += k.props_at("old_military_crate", [(2380, 120, 0), (2390, 160, 10)])
placed += k.props_at("ammo_box", [(2330, 180, 30), (2345, 188, 60)], z=0)
placed += k.props_at("metal_jerrycan_green", [(1700, 420, 0), (1715, 420, 0)])
placed += k.props_at("propane_tank", [(2440, 380, 0)])
# Taxiway: tyre marks where the jets turn off.
for x in (-1600, -900, -200, 700, 1500):
    k.decal("skid_mark", (x, -880 + (x % 3) * 30, 0), "+z")
# OPFOR yard: barriers and clutter along its edges.
placed += k.props_at("concrete_road_barrier_02", [(420, 2040, 0), (560, 2040, 0), (1700, 2040, 0)])
placed += k.props_at("trashbag", [(1820, 1760, 20), (1840, 1790, 200)])
placed += k.props_at("metal_trash_can", [(1850, 1730, 180)])
placed += k.props_at("wooden_military_crate", [(340, 1760, 90), (340, 1820, 90)])
# The operations building: strip lights under the ground-floor ceiling.
placed += k.props_at("mounted_fluorescent_lights", [(x, y, 0) for x in range(420, 1450, 260) for y in (900, 1130, 1320)], z=F2 - 3)
print(f"air base dressing: {placed} models")
k.dress(budget=200)
k.finish()
seen = [(o, c) for o in [(-700 + 120 * i, -2100 - 80 * (i % 2), 64) for i in range(5)]
        for c in [(900 + 120 * i, 1880 + 80 * (i % 2), 64) for i in range(5)] if k.visible(o, c)]
if seen:
    print("WARNING: spawns see each other:", seen[:4])
# The north-west corner behind the barracks: ground.
k.ground(XW, 1500, -2200, YN, GRASS if "GRASS" in globals() else T("grass"), 640)
k.safety_floor(XW - 200, YS - 200, XE + 200, YN + 200)
m.save(os.path.join(os.path.dirname(__file__), "../../assets/maps/zg_fliegerhorst.map"))
