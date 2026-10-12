"""Architecture and prop kit for the ZUGRIFF maps, built on zmap.

Everything is made of convex brushes. Conventions:
- `m` is the zmap.Map being built; props add worldspawn brushes unless noted.
- Walls: `axis` "x" means the wall runs along x (thickness along y); "y" runs along y.
- Openings in walls are (u0, u1, v0, v1): u along the wall, v = z.
- Faces nobody can see use tools/nodraw (NODRAW) to keep the lightmap atlas small. "Nobody" includes the menu's
  flyover high above the map: before a map is written, `expose` gives every nodraw face that borders open air the
  brush's own texture, so roofs, stack tops, gable ends and panel edges are never holes from above. Only faces buried
  in other solid geometry stay nodraw.
"""

from __future__ import annotations

import json
import math
import os

import zmap
from zmap import Brush, Map, box, fit, hull, prism, ramp, stairs  # noqa: F401

zmap.PRECISE = True  # the zg_* maps have many sloped planes
zmap.VARY = True  # texture offsets per plane (see zmap.VARY)

NODRAW = "tools/nodraw"
CLIP = "tools/clip"
# Dark painted steel: frames, posts, bars, bumpers, grilles (was the dev/trim blockout grid).
TRIM = "mat/metal_dark"
GLASS = "dev/glass"
# Window glass seen from outside (a pane drawn on a facade), fitted once per window.
GLASS_WIN = "mat/glass_window"
FRAME = "mat/window_frame"
RUBBER = "mat/rubber"
SHUTTER = "mat/rollershutter"


# Props float this far above what they stand on, so they don't split the ground's (or the crate below's) faces into
# many small lightmap charts. Invisible at 0.25 units; walking and bullets don't notice.
LIFT = 0.25


def lift(z: float) -> float:
    return z + (LIFT * 2 if z > 0.5 else LIFT)


def _big_first(b: Brush) -> Brush:
    """Orders a box's faces largest first (see zmap.hull)."""
    lo, hi = b.bounds
    ext = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2])

    def area(f):
        n = f.normal
        ax = 0 if abs(n[0]) > 0.5 else (1 if abs(n[1]) > 0.5 else 2)
        a, c = [ext[i] for i in range(3) if i != ax]
        return a * c

    b.faces.sort(key=lambda f: -area(f))
    return b


def _tex(tex, side: str) -> str:
    if isinstance(tex, str):
        return tex
    return tex.get(side) or tex.get("all", "dev/wall")


_HERE = os.path.dirname(os.path.abspath(__file__))
_ASSETS = os.path.join(_HERE, "..", "..", "assets")
_PROPS: dict | None = None
_DECALS: dict | None = None


def props_info() -> dict:
    """assets/models/props/props.json: each model's size, collision box, surface area."""
    global _PROPS
    if _PROPS is None:
        try:
            _PROPS = json.load(open(os.path.join(_ASSETS, "models", "props", "props.json")))
        except OSError:
            _PROPS = {}
    return _PROPS


def decals_info() -> dict:
    """assets/textures/decals/decals.json: each decal's atlas cell and world size."""
    global _DECALS
    if _DECALS is None:
        try:
            _DECALS = json.load(open(os.path.join(_ASSETS, "textures", "decals", "decals.json")))["decals"]
        except OSError:
            _DECALS = {}
    return _DECALS


def hash_unit(*xs: float) -> float:
    """A stable pseudo-random number in 0..1 from coordinates."""
    import zlib

    return (zlib.crc32(",".join(f"{x:.2f}" for x in xs).encode()) % 10007) / 10007


# Look-alike sets a building's walls or roof may take instead, so a street of the same building type isn't one texture.
VARIANTS = {
    "mat/plaster_white": ("mat/plaster_white", "mat/plaster_b", "mat/plaster_c"),
    "mat/plaster_old": ("mat/plaster_old", "mat/plaster_c"),
    "mat/concrete_wall": ("mat/concrete_wall", "mat/concrete_wall_b", "mat/concrete_wall_c"),
    "mat/bricks090": ("mat/bricks090", "mat/brick_b"),
    "mat/roof_tiles": ("mat/roof_tiles", "mat/roof_tiles", "mat/roof_b"),
    "mat/metal_corrugated": ("mat/metal_corrugated", "mat/metal_corrugated_b", "mat/metal_corrugated_rust"),
    "mat/roof_clay": ("mat/roof_clay", "mat/roof_clay", "mat/roof_tiles"),
}


# The light a lamp model gives (it's day, so only the lamps that burn by day: indoor and area lighting). In the
# model's frame (metres, +y up, +z its front): where the light sits, how it shines (point, spot down its `dir`, or
# a rectangle of `size` facing `dir`), its power in the bake's watts and its colour. The bulb glows in the game.
# The light sits just outside the model (below the shade, in front of the glass): inside, the model's own shell
# swallowed it in the bake.
LAMP_LIGHTS = {
    "hanging_industrial_lamp": {"at": (0.0, -0.04, 0.0), "kind": "spot", "dir": (0.0, -1.0, 0.0), "angle": 120,
                                "power": 480, "color": (255, 210, 160)},
    "mounted_fluorescent_lights": {"at": (0.0, -0.02, 0.0), "kind": "area", "dir": (0.0, -1.0, 0.0),
                                   "size": (0.9, 0.08), "power": 10, "color": (236, 242, 255)},
    "security_light": {"at": (0.0, 0.3, 0.25), "kind": "spot", "dir": (0.0, -0.6, 1.0), "angle": 95,
                       "power": 100, "color": (255, 238, 214)},
}


def container_paint(color: str) -> str:
    """A container's painted corrugated sheet for a colour name (red, blue, green, or the older metal_rust and
    metal_corrugated, which become orange and grey)."""
    c = {"metal_rust": "orange", "metal_corrugated": "grey", "rust": "orange"}.get(color, color)
    return T(f"container_{c}") if c in ("red", "blue", "green", "grey", "orange") else T(color)


def variant(tex, *key):
    """`tex` or one of its look-alikes (VARIANTS), chosen by `key` (a building's corner, say)."""
    if not isinstance(tex, str) or tex not in VARIANTS:
        return tex
    opts = VARIANTS[tex]
    return opts[int(hash_unit(*key) * len(opts)) % len(opts)]


def T(name: str) -> str:
    """A material set name."""
    return name if "/" in name else f"mat/{name}"


def _unit(v):
    l = math.sqrt(sum(c * c for c in v)) or 1.0
    return (v[0] / l, v[1] / l, v[2] / l)


def _dot3(a, b) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _brush_vertices(planes) -> list:
    """A convex brush's corners: where three of its planes meet inside all the others."""
    out = []
    n = len(planes)
    for i in range(n):
        for j in range(i + 1, n):
            for k in range(j + 1, n):
                (a, da), (b, db), (c, dc) = planes[i], planes[j], planes[k]
                bc = (b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0])
                det = _dot3(a, bc)
                if abs(det) < 1e-9:
                    continue
                ca = (c[1] * a[2] - c[2] * a[1], c[2] * a[0] - c[0] * a[2], c[0] * a[1] - c[1] * a[0])
                ab = (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])
                p = tuple((da * bc[q] + db * ca[q] + dc * ab[q]) / det for q in range(3))
                if all(_dot3(m, p) <= dm + 0.05 for m, dm in planes) and not any(math.dist(p, o) < 0.01 for o in out):
                    out.append(p)
    return out


class Kit:
    """Collects glass panes (one func_glass entity) and checks sightlines on top of a Map."""

    def __init__(self, m: Map):
        self.m = m
        self.glass: list[Brush] = []
        self.deco: list[Brush] = []  # thin details: drawn, no collision, don't cut other faces
        self.solids: list[tuple[tuple, tuple]] = []  # boxes for sightline checks
        # Facades of solid buildings (for the dressing pass): dicts with axis, a0, a1, face, out, z0, z1, tex, wins, doors.
        self.facades: list[dict] = []
        # Ground tiles (x0, y0, x1, y1, z, tex), for scattering floor decals.
        self.grounds: list[tuple] = []
        # "town": windows and doors of flat facades are the modelled pieces (arch.py: win_town_*, door_town_*)
        # instead of panels; "office" / "industrial": their windows are win_office_* / win_factory_*; None: panels.
        self.facade_style: str | None = None
        # Containers as the modelled ones (arch.py: container40_*, container20_*) where built.
        self.container_props = False
        m.before_save = self.expose

    # ---- primitives -------------------------------------------------------------------------------------------
    def add(self, b: Brush | list[Brush], deco: bool = False) -> None:
        bs = b if isinstance(b, list) else [b]
        for x in bs:
            if deco:
                self.deco.append(x)
                continue
            self.m.add(x)
            if x.bounds is not None and not all(f.texture.startswith("tools/") for f in x.faces):
                self.solids.append(x.bounds)

    # Horizontal faces are charted with their map-x extent as the chart height; the atlas packs shelves by height, so
    # a few very long ones waste whole shelves. Boxes with drawn tops/bottoms longer than this in x are split.
    SPLIT_X = 640

    def blk(self, lo, hi, tex, scale=None, deco=False) -> Brush:
        dx = hi[0] - lo[0]
        flat_drawn = any(not _tex(tex, k_).startswith("tools/") for k_ in ("top", "bottom"))
        if dx > self.SPLIT_X * 1.25 and flat_drawn:
            n = math.ceil(dx / self.SPLIT_X)
            last = None
            for i in range(n):
                a = lo[0] + dx * i / n
                c = lo[0] + dx * (i + 1) / n
                last = _big_first(box((a, lo[1], lo[2]), (c, hi[1], hi[2]), tex, scale))
                self.add(last, deco)
            return last
        b = _big_first(box(lo, hi, tex, scale))
        self.add(b, deco)
        return b

    def dblk(self, lo, hi, tex, scale=None) -> Brush:
        """A decorative block (func_illusionary)."""
        return self.blk(lo, hi, tex, scale, deco=True)

    def ground(self, x0, y0, x1, y1, tex, size=384, z0=-16, z1=0, side=None):
        """Ground in tiles, so props standing on it only split their own tile's face."""
        nx = max(1, round((x1 - x0) / size))
        ny = max(1, round((y1 - y0) / size))
        for i in range(nx):
            for j in range(ny):
                a = (x0 + (x1 - x0) * i / nx, y0 + (y1 - y0) * j / ny)
                b = (x0 + (x1 - x0) * (i + 1) / nx, y0 + (y1 - y0) * (j + 1) / ny)
                self.m.add(_big_first(box((a[0], a[1], z0), (b[0], b[1], z1), {"top": tex, "sides": side or NODRAW, "bottom": NODRAW})))
                self.grounds.append((a[0], a[1], b[0], b[1], z1, tex))

    def safety_floor(self, x0, y0, x1, y1, z=-64) -> None:
        """An invisible floor under the whole map (collision only, nothing drawn or baked), so a gap somewhere
        never drops a player out of the world."""
        self.m.add(box((x0, y0, z - 16), (x1, y1, z), NODRAW))

    def clip(self, lo, hi) -> None:
        self.m.add(box(lo, hi, CLIP))

    # ---- placed models and decals ------------------------------------------------------------------------------------
    def blocked(self, lo, hi, margin: float = 4.0) -> bool:
        """Whether the box lo..hi (shrunk by `margin`) runs into a solid added so far."""
        return any(lo[0] + margin < b[0] and a[0] < hi[0] - margin and lo[1] + margin < b[1] and a[1] < hi[1] - margin
                   and lo[2] + margin < b[2] and a[2] < hi[2] - margin for a, b in self.solids)

    def props_at(self, model: str, spots, z: float = 0.0, scale: float = 1.0, solid: bool | None = None) -> int:
        """Places `model` at each (x, y, yaw) in `spots` that is clear of the solids so far. Returns how many."""
        info = props_info().get(model)
        if info is None:
            return 0
        sx, sy, sz = (c * zmap.M * scale / 2 for c in info.get("size", (0.5, 0.5, 0.5)))
        n = 0
        for x, y, yaw in spots:
            r = max(sx, sz)
            if self.blocked((x - r, y - r, z), (x + r, y + r, z + 2 * sy)):
                continue
            self.prop(model, x, y, z, yaw=yaw, scale=scale, solid=solid)
            n += 1
        return n

    def prop(self, model: str, x: float, y: float, z: float = 0.0, yaw: float = 0.0, scale: float = 1.0,
             solid: bool | None = None) -> None:
        """A model from assets/models/props (`prop_model`) standing with its base at (x, y, z) (map units), its front
        facing map angle `yaw` (degrees, 0 = +x, 90 = +y). Collision from props.json's box: solid (nodraw: stops
        players and bullets) for cover-sized things, player clip for small ones (`solid` overrides), none where
        props.json has none. The box floats LIFT up so it doesn't cut the ground's faces."""
        info = props_info().get(model)
        if info is None:
            print(f"warning: unknown prop {model}")
            info = {}
        area = float(info.get("area", 4.0)) * scale * scale
        # The model's lightmap square: finer than walls, about 0.08 m per luxel over the part of the square its
        # unwrap fills (`lightmap_fill`).
        fill = max(0.1, float(info.get("lightmap_fill", 0.5)))
        # (Big models, a ship or a jet, may take up to 192: at 96 a frigate's luxels were 1.5 m.)
        lmsize = max(8, min(192 if area > 300 else 96, math.ceil(math.sqrt(area / fill) / 0.08)))
        self.m.point("prop_model", (x, y, z), angle=yaw, model=model, scale=f"{scale:g}", lmsize=str(lmsize))
        lamp = LAMP_LIGHTS.get(model)
        if lamp:
            self._lamp_light(lamp, x, y, z, yaw, scale)
        col = info.get("collide")
        if not col:
            return
        hx, hy, hz = (c * zmap.M * scale for c in col)
        a = math.radians(yaw)
        ca, sa = abs(math.cos(a)), abs(math.sin(a))
        # The turned footprint's axis-aligned box (the model's depth, z, lies along the facing).
        ex, ey = ca * hz + sa * hx, sa * hz + ca * hx
        lo = (x - ex, y - ey, z + LIFT)
        hi = (x + ex, y + ey, z + 2 * hy)
        big = solid if solid is not None else (2 * hy > 24 and 2 * max(hx, hz) > 12)
        self.m.add(box(lo, hi, NODRAW if big else CLIP))
        if big:
            self.solids.append((lo, hi))

    def _lamp_light(self, lamp: dict, x: float, y: float, z: float, yaw: float, scale: float) -> None:
        """A `light_zg` entity for a lamp model placed at (x, y, z) turned `yaw`: model frame → map units."""
        a = math.radians(yaw)
        f = (math.cos(a), math.sin(a))  # the model's +z (front) on the map
        r = (-math.sin(a), math.cos(a))  # its +x

        def to_map(v, pos=True):
            k = zmap.M * scale if pos else 1.0
            return (r[0] * v[0] * k + f[0] * v[2] * k, r[1] * v[0] * k + f[1] * v[2] * k, v[1] * k)

        o = to_map(lamp["at"])
        d = to_map(lamp.get("dir", (0.0, -1.0, 0.0)), pos=False)
        props = {"kind": lamp["kind"], "power": f"{lamp['power'] * scale * scale:g}",
                 "color": " ".join(str(c) for c in lamp["color"]), "dir": " ".join(f"{c:.4f}" for c in d)}
        if "angle" in lamp:
            props["cone"] = str(lamp["angle"])
        if "size" in lamp:
            props["size"] = " ".join(f"{c * scale:g}" for c in lamp["size"])
            # The rectangle's long side, along the model's +x.
            props["along"] = " ".join(f"{c:.4f}" for c in to_map((1.0, 0.0, 0.0), pos=False))
        self.m.point("light_zg", (x + o[0], y + o[1], z + o[2]), **props)

    def decal(self, name: str, at, facing: str, size=None) -> None:
        """A decal from the atlas on a flat surface: centred at `at` (map units, a point on the surface), on a face
        looking towards `facing` (+x, -x, +y, -y for walls, +z for floors), `size` (w, h) in metres (default from
        decals.json). A thin func_illusionary panel just in front of the surface; only its front is drawn."""
        info = decals_info().get(name)
        if info is None:
            print(f"warning: unknown decal {name}")
            return
        u0, v0, u1, v1 = info["rect"]
        w, h = size or info.get("size", (1.0, 1.0))
        W, H = w * zmap.M, h * zmap.M
        ax = {"+x": 0, "-x": 0, "+y": 1, "-y": 1, "+z": 2}[facing]
        sign = -1.0 if facing[0] == "-" else 1.0
        n = [0.0, 0.0, 0.0]
        n[ax] = sign
        if ax == 2:
            # Floors: the image's top towards +y.
            R, D = (1.0, 0.0, 0.0), (0.0, -1.0, 0.0)
        else:
            # Walls, seen from in front (looking along -n): right = forward × up, down = -z.
            fx, fy = -n[0], -n[1]
            R, D = (fy, -fx, 0.0), (0.0, 0.0, -1.0)
        half = [abs(R[i]) * W / 2 + abs(D[i]) * H / 2 for i in range(3)]
        lo, hi = [at[i] - half[i] for i in range(3)], [at[i] + half[i] for i in range(3)]
        # Thickness along the normal: from LIFT/2 to LIFT in front of the surface.
        lo[ax], hi[ax] = sorted((at[ax] + sign * LIFT * 0.5, at[ax] + sign * LIFT))
        lo, hi = tuple(lo), tuple(hi)
        b = box(lo, hi, NODRAW)
        corners = [(x_, y_, z_) for x_ in (lo[0], hi[0]) for y_ in (lo[1], hi[1]) for z_ in (lo[2], hi[2])]
        rmin = min(zmap._dot(c, R) for c in corners)
        dmin = min(zmap._dot(c, D) for c in corners)
        for face in b.faces:
            if abs(zmap._norm(face.normal)[ax] - sign) > 1e-6:
                continue
            face.texture = f"decal/{name}"
            U, V = zmap._axes(zmap._norm(face.normal))
            # Texel = dot(p, axis) / scale + shift; negative scales keep the image the right way round.
            su = (1.0 if zmap._dot(U, R) > 0 else -1.0) * W / ((u1 - u0) * 512)
            sv = (1.0 if zmap._dot(V, D) > 0 else -1.0) * H / ((v1 - v0) * 512)
            face.scale, face.scale_v = su, sv
            du = zmap._dot(U, R)
            dv = zmap._dot(V, D)
            # dot(p, U) = du · dot(p, R): the left edge (rmin) gets u0, the top edge (dmin) v0.
            face.shift = (u0 * 512 - du * rmin / su, v0 * 512 - dv * dmin / sv)
        self.deco.append(b)

    def pane(self, lo, hi) -> None:
        self.glass.append(box(lo, hi, GLASS))

    def hide_tops(self, zmin: float, keep=None) -> int:
        """Once hid flat roofs and stack tops at or above `zmin` from the players; the menu's flyover sees them, so they
        stay drawn now (kept so the map scripts still read the same). Returns 0."""
        return 0
        n = 0
        for b in self.m.world.brushes + self.deco:
            for f in b.faces:
                nz = f.normal[2] / math.sqrt(sum(c * c for c in f.normal))
                if nz > 0.999 and f.point[2] >= zmin and not f.texture.startswith("tools/"):
                    if keep and keep(f, b):
                        continue
                    f.texture = NODRAW
                    n += 1
        return n

    # ---- dressing: what makes a facade look used ----------------------------------------------------------------
    def _decals(self, tag: str, surface: str = "wall") -> list[str]:
        return sorted(n for n, d in decals_info().items() if tag in d.get("tags", []) and d.get("surface", "any") in (surface, "any"))

    def _wall_prop(self, f: dict, model: str, u: float, z: float, stand: float = 0.0) -> None:
        """A model on a facade: at u along it, its base at z, its back against the wall (`stand` out from it)."""
        info = props_info().get(model)
        if info is None:
            return
        depth = info.get("size", [0.5, 0.5, 0.5])[2] * zmap.M
        t = f["face"] + f["out"] * (depth / 2 + stand)
        x, y = (u, t) if f["axis"] == "x" else (t, u)
        yaw = (90 if f["out"] > 0 else 270) if f["axis"] == "x" else (0 if f["out"] > 0 else 180)
        self.prop(model, x, y, z, yaw=yaw, solid=False)

    def _wall_foot_clutter(self, budget: int) -> int:
        """Small clusters of yard clutter (jerrycans, drums, crates, tyres) against tall solids along paved ground:
        candidate spots every 160 units a hand's width from a wall, a few of them used, none blocking anything."""
        junk = [n for n in ("metal_jerrycan", "metal_jerrycan_green", "wooden_crate_02", "old_tyre", "barrel_01", "barrel_02",
                            "plastic_crate_01", "plastic_crate_02", "cardboard_box_01", "propane_tank", "barrel_stove", "trashbag",
                            "plastic_bottle_gallon", "compost_bags", "can_rusted", "compost_bag_leaning") if n in props_info()]
        if not junk:
            return 0
        paved = ("asphalt", "concrete", "slabs", "paving", "pavers", "gnd_")
        grounds = [g for g in self.grounds if any(p_ in g[5] for p_ in paved)]
        tall = [(lo, hi) for lo, hi in self.solids if hi[2] - lo[2] > 90 and lo[2] < 8]
        placed = 0
        for lo, hi in tall:
            # Each side of the solid, spots every 200 units a little way out from it, facing away from it.
            for axis, face, out, a0, a1, yaw in (("x", lo[1], -1, lo[0], hi[0], 270), ("x", hi[1], 1, lo[0], hi[0], 90),
                                                ("y", lo[0], -1, lo[1], hi[1], 180), ("y", hi[0], 1, lo[1], hi[1], 0)):
                for u in range(int(a0) + 40, int(a1) - 40, 200):
                    if placed >= budget or hash_unit(u, face, 21) > 0.2:
                        continue
                    px, py = (u, face + out * 22) if axis == "x" else (face + out * 22, u)
                    g = next((g for g in grounds if g[0] < px < g[2] and g[1] < py < g[3]), None)
                    if g is None or self.blocked((px - 18, py - 18, g[4] + 1), (px + 18, py + 18, g[4] + 60), margin=2):
                        continue
                    for j in range(1 + int(hash_unit(px, py, 22) * 3)):
                        ox, oy = ((j - 1) * 30, 0) if axis == "x" else (0, (j - 1) * 30)
                        name = junk[int(hash_unit(px, py, j, 23) * len(junk)) % len(junk)]
                        self.prop(name, px + ox, py + oy, g[4], yaw=yaw + 30 * (hash_unit(j, px) - 0.5))
                    placed += 1
        print(f"yard clutter: {placed} clusters")
        return placed

    def dress(self, props: bool = True, budget: int = 120, style: str = "industrial") -> None:
        """Adds the wear of use to every recorded facade: grime along the base, streaks under some sills, the odd
        crack, downpipes at the ends of tall facades, and fixtures (AC units, meter boxes, lamps over doors, the odd
        camera). Deterministic: the same map always gets the same dressing. `budget` caps the placed models.
        `style` "town": wall lanterns over doors, fewer AC units, no cameras."""
        town = style == "town"
        grime = self._decals("grime")
        # Under sills: soft water streaks (the rust ones belong under pipes and bolts, not windows).
        leaks = [n for n in self._decals("leak") if decals_info()[n].get("blend") == "multiply"]
        cracks = self._decals("crack")
        used = 0

        def pick(names, *key):
            return names[int(hash_unit(*key) * len(names)) % len(names)] if names else None

        def at(f, u, z):
            t = f["face"]
            return (u, t, z) if f["axis"] == "x" else (t, u, z)

        facing = lambda f: ("+" if f["out"] > 0 else "-") + ("y" if f["axis"] == "x" else "x")  # noqa: E731
        for f in self.facades:
            a0, a1, z0, z1 = f["a0"], f["a1"], f["z0"], f["z1"]
            length, height = a1 - a0, z1 - z0
            if length < 64:
                continue
            blocked = f["doors"] + [w for w in f["wins"] if w[2] < z0 + 70]
            # Grime along the base, in overlapping patches.
            if grime:
                u = a0 + 30
                while u < a1 - 30:
                    w = 60 + 50 * hash_unit(u, f["face"])
                    if not any(d[0] - 10 < u < d[1] + 10 for d in blocked):
                        name = pick(grime, u, f["face"], 1)
                        self.decal(name, at(f, u, z0 + 18), facing(f), size=(w / zmap.M, 36 / zmap.M))
                    u += w * 0.8
            # Streaks under a third of the windows.
            for (u0, u1, v0, v1) in f["wins"]:
                if leaks and hash_unit(u0, v0, f["face"], 2) < 0.33 and v0 - z0 > 90:
                    w = (u1 - u0) * (0.5 + 0.3 * hash_unit(u1, v1))
                    h = min(v0 - z0 - 30, 60 + 60 * hash_unit(v0, u0))
                    self.decal(pick(leaks, u0, v0), at(f, (u0 + u1) / 2, v0 - 8 - h / 2), facing(f), size=(w / zmap.M, h / zmap.M))
            # A crack now and then.
            if cracks and hash_unit(a0, a1, f["face"], 3) < 0.45 and height > 120:
                u = a0 + length * (0.2 + 0.6 * hash_unit(a1, f["face"]))
                z = z0 + 40 + (height - 100) * hash_unit(a0, f["face"]) * 0.5
                if not any(w[0] - 20 < u < w[1] + 20 and w[2] - 30 < z < w[3] + 30 for w in f["wins"] + f["doors"]):
                    self.decal(pick(cracks, u, z), at(f, u, z), facing(f))
            # Downpipes at both ends of tall facades.
            if height > 180 and length > 280:
                for u in (a0 + 14, a1 - 14):
                    t = f["face"] + f["out"] * 5
                    if f["axis"] == "x":
                        self.add(prism((u, t), 3.2, z0, z1 - 4, 6, {"sides": TRIM, "top": NODRAW, "bottom": NODRAW}, axis="z"), deco=True)
                    else:
                        self.add(prism((t, u), 3.2, z0, z1 - 4, 6, {"sides": TRIM, "top": NODRAW, "bottom": NODRAW}, axis="z"), deco=True)
            if not props or used >= budget:
                continue
            # A lamp over each door.
            for (u0, u1, v0, v1) in f["doors"]:
                if town:
                    # A lantern beside every other door.
                    if used < budget and hash_unit(u0, f["face"], 11) < 0.5 and u1 + 40 < a1:
                        self._wall_prop(f, "street_lamp_02", u1 + 22, v1 - 20)
                        used += 1
                elif used < budget and "security_light" in props_info():
                    self._wall_prop(f, "security_light", (u0 + u1) / 2, v1 + 14)
                    used += 1
            # AC units beside some upper windows.
            for (u0, u1, v0, v1) in f["wins"]:
                if used < budget and v0 - z0 > 100 and hash_unit(u0, v0, f["face"], 4) < (0.04 if town else 0.12) and u1 + 60 < a1 - 20:
                    self._wall_prop(f, "exterior_aircon_unit", u1 + 36, v0 - 4)
                    used += 1
            # A meter or utility box on the ground on some longer facades, clear of doors and low windows.
            if length > 360 and hash_unit(a0, f["face"], 5) < 0.4 and used < budget:
                box_name = pick([n for n in ("power_box_01", "utility_box_01", "utility_box_02") if n in props_info()], a1, f["face"])
                if box_name:
                    for tries in range(4):
                        u = a0 + length * (0.15 + 0.7 * hash_unit(a0, tries, f["face"]))
                        if not any(b[0] - 50 < u < b[1] + 50 for b in blocked):
                            self._wall_prop(f, box_name, u, z0 + (40 if "power_box" in box_name else 0))
                            used += 1
                            break
            # Old town: a poster or a note by some doors, a planter or a bin at the foot of some walls.
            if town and length > 200:
                h = hash_unit(a0, f["face"], 12)
                u = a0 + length * (0.2 + 0.6 * hash_unit(a1, f["face"], 13))
                clear = not any(b[0] - 30 < u < b[1] + 30 for b in f["wins"] + f["doors"])
                if clear and h < 0.18:
                    self.decal("poster_stadtfest" if h < 0.1 else "flyer_zimmer_frei", at(f, u, z0 + 150), facing(f))
                elif clear and h < 0.3 and used < budget:
                    self._wall_prop(f, "planter_box_01" if h < 0.25 else "metal_trash_can", u, z0)
                    used += 1
            # Old town: a gutter of granite setts along the foot of each facade (drains the street), a hand wide.
            if town and "mat/stone_pavers" in zmap.TEX_SCALE:
                w = 22
                t0, t1 = sorted((f["face"], f["face"] + f["out"] * w))
                lo = (a0, t0, z0) if f["axis"] == "x" else (t0, a0, z0)
                hi = (a1, t1, z0 + 0.35) if f["axis"] == "x" else (t1, a1, z0 + 0.35)
                self.dblk(lo, hi, {"top": T("stone_pavers"), "sides": NODRAW, "bottom": NODRAW})
            # Old town: ivy climbing up from the foot of some walls, clear of the doors.
            if town and length > 150:
                ivies = [n for n in ("ivy_a", "ivy_b") if n in decals_info()]
                for i in range(2):
                    h = hash_unit(a0, f["face"], 30 + i)
                    u = a0 + 40 + (length - 80) * hash_unit(a1, f["face"], 32 + i)
                    if ivies and h < 0.3 and not any(d[0] - 60 < u < d[1] + 60 for d in f["doors"]):
                        name = ivies[int(h * 20) % len(ivies)]
                        tall = decals_info()[name]["size"][1] * zmap.M
                        self.decal(name, at(f, u, z0 + tall / 2), facing(f))
            # Old town: a trash corner at some facade ends (sacks, a bag, bottles, a can), a broom by some doors.
            if town and length > 250 and used < budget:
                h = hash_unit(a1, f["face"], 40)
                if h < 0.35:
                    end = a0 + 30 if h < 0.17 else a1 - 30
                    if not any(b[0] - 50 < end < b[1] + 50 for b in f["doors"]):
                        pile = [n for n in ("trashbag", "compost_bag_leaning", "cardboard_box_01", "wine_bottle", "can_rusted", "plastic_bottle_gallon") if n in props_info()]
                        for j in range(3 + int(hash_unit(end, 41) * 3)):
                            name = pile[int(hash_unit(end, j, 42) * len(pile)) % len(pile)]
                            du = (j - 2) * 16 + 8 * (hash_unit(j, end) - 0.5)
                            self._wall_prop(f, name, end + du, z0, stand=3 + 10 * hash_unit(end, j, 43))
                            used += 1
                for (u0, u1, v0, v1) in f["doors"]:
                    if hash_unit(u1, f["face"], 44) < 0.12 and "wooden_broom" in props_info() and used < budget:
                        self._wall_prop(f, "wooden_broom", u1 + 16, z0 + 1)
                        used += 1
            # Old town: pots and planters beside some front doors.
            if town:
                pots = [n for n in ("potted_plant_02", "planter_box_03", "wooden_bucket_01") if n in props_info()]
                for (u0, u1, v0, v1) in f["doors"]:
                    h = hash_unit(u0, f["face"], 14)
                    if pots and h < 0.5 and used < budget:
                        side = u1 + 26 if h < 0.25 else u0 - 26
                        if a0 + 20 < side < a1 - 20:
                            self._wall_prop(f, pots[int(h * 40) % len(pots)], side, z0)
                            used += 1
            # Yards (not the old town): a small cluster of clutter at the foot of some walls.
            if not town and length > 300 and hash_unit(a0, f["face"], 15) < 0.35 and used < budget:
                junk = [n for n in ("metal_jerrycan", "metal_jerrycan_green", "wooden_crate_02", "old_tyre", "barrel_01", "barrel_02",
                                    "plastic_crate_01", "cardboard_box_01", "propane_tank", "barrel_stove") if n in props_info()]
                u = a0 + length * (0.2 + 0.6 * hash_unit(a1, f["face"], 16))
                if junk and not any(b[0] - 60 < u < b[1] + 60 for b in f["doors"]):
                    for j in range(1 + int(hash_unit(u, 17) * 3)):
                        name = junk[int(hash_unit(u, j, 18) * len(junk)) % len(junk)]
                        self._wall_prop(f, name, u + (j - 1) * 34 + 8 * hash_unit(j, u), z0, stand=4 * hash_unit(u, j))
                        used += 1
            # A camera high on some corners.
            if not town and height > 200 and hash_unit(a1, f["face"], 6) < 0.25 and used < budget and "security_camera_01" in props_info():
                self._wall_prop(f, "security_camera_01", a1 - 30, z1 - 40)
                used += 1
        # Yards: clutter along the foot of walls that aren't facades (sheds, blast walls, container stacks).
        if not town:
            used += self._wall_foot_clutter(budget // 2)
        # Stones and mossy rocks on the grass (airfield, parks), clear of everything.
        rocks = [n for n in ("stone_01", "rock_moss_01", "rock_moss_03") if n in props_info()]
        for (x0, y0, x1, y1, z, tex) in self.grounds:
            if "grass" not in tex or not rocks:
                continue
            area = (x1 - x0) * (y1 - y0)
            for i in range(int(area / (700 * 700)) + 1):
                if hash_unit(x0, y0, i, 50) > 0.5 or used >= budget:
                    continue
                px = x0 + 60 + (x1 - x0 - 120) * hash_unit(x1, i, 51)
                py = y0 + 60 + (y1 - y0 - 120) * hash_unit(y1, i, 52)
                name = rocks[int(hash_unit(px, py, 53) * len(rocks)) % len(rocks)]
                sc = 0.25 + 0.3 * hash_unit(px, 54) if name.startswith("rock_moss") else 1.5 + 2.0 * hash_unit(py, 55)
                r = props_info()[name]["size"][0] * zmap.M * sc / 2 + 8
                if not self.blocked((px - r, py - r, z), (px + r, py + r, z + 60)):
                    self.prop(name, px, py, z - 4 * sc, yaw=hash_unit(px, py) * 360, scale=round(sc, 3), solid=False)
                    used += 1
        # Wear on paved ground: stains, cracks, puddles, the odd manhole; clear of anything standing there.
        paved = ("asphalt", "concrete_floor", "concrete_slabs", "paving", "concrete_painted", "cobblestone", "gnd_")
        roads = [n for n in ("oil_stain_a", "oil_stain_b", "crack_floor", "floor_damage", "puddle", "grime_patch_a", "grime_patch_b")
                 + (() if town else ("tyre_track", "skid_mark", "oil_stain_a")) if n in decals_info()]
        spots = 0
        for (x0, y0, x1, y1, z, tex) in self.grounds:
            if not any(p_ in tex for p_ in paved) or not roads:
                continue
            area = (x1 - x0) * (y1 - y0)
            # (Yards and aprons get more wear than the old town's lanes.)
            keep = 0.55 if town else 0.85
            for i in range(max(1, int(area / (420 * 420)) * (1 if town else 2))):
                if hash_unit(x0, y0, i, 7) > keep:
                    continue
                px = x0 + 40 + (x1 - x0 - 80) * hash_unit(x0, i, 8)
                py = y0 + 40 + (y1 - y0 - 80) * hash_unit(y0, i, 9)
                if any(lo[0] - 48 < px < hi[0] + 48 and lo[1] - 48 < py < hi[1] + 48 and lo[2] < z + 60 for lo, hi in self.solids):
                    continue
                name = roads[int(hash_unit(px, py) * len(roads)) % len(roads)]
                if "asphalt" in tex and hash_unit(py, px, 10) < 0.12 and "manhole_cover" in decals_info():
                    name = "manhole_cover"
                self.decal(name, (px, py, z), "+z")
                spots += 1
        print(f"floor wear: {spots} decals")
        # The bomb sites' letters, painted on the floor in the middle of each site.
        for e in self.m.entities:
            if e.classname == "func_bomb_target" and e.brushes and e.brushes[0].bounds:
                lo, hi = e.brushes[0].bounds
                name = "site_" + e.props.get("site", "a").lower()
                if name in decals_info():
                    self.decal(name, ((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, lo[2]), "+z", size=(2.4, 2.4))
        print(f"dressed {len(self.facades)} facades, {used} wall models")

    # ---- exposing hidden faces that can be seen -------------------------------------------------------------------
    # Textures that don't hide what is behind them (and brushes made only of these aren't shown either).
    SEE_THROUGH = ("tools/clip", "tools/playerclip", "tools/skip", "tools/hint", "tools/trigger", "tools/invisible", GLASS)

    def expose(self) -> int:
        """Nodraw faces of drawn brushes that border open air get the brush's texture. A face is sampled at its
        middle and near its corners, a unit and 12 units outside it; a sample inside another solid (drawn or nodraw,
        not clip or glass) is covered, and a face counts as open where both are free. Undersides with nothing below
        (the ground's) and decal edges stay hidden. Returns how many faces were shown."""
        solid_entities = {"worldspawn", "func_detail", "func_illusionary", "func_wall"}
        bodies = [b for e in [self.m.world] + self.m.entities if e.classname in solid_entities for b in e.brushes]
        seen = {id(b) for b in bodies}
        bodies += [b for b in self.deco if id(b) not in seen]
        planes = {}
        for b in bodies:
            planes[id(b)] = [(_unit(f.normal), _dot3(_unit(f.normal), f.point)) for f in b.faces]
        verts = {id(b): _brush_vertices(planes[id(b)]) for b in bodies}
        # A grid over the brushes' boxes.
        cell = 256.0
        grid: dict[tuple, list] = {}
        boxes = {}
        for b in bodies:
            vs = verts[id(b)]
            if not vs or all(f.texture in self.SEE_THROUGH or f.texture.startswith("tools/clip") for f in b.faces):
                continue
            lo = tuple(min(v[i] for v in vs) for i in range(3))
            hi = tuple(max(v[i] for v in vs) for i in range(3))
            boxes[id(b)] = (lo, hi)
            for gx in range(int(math.floor(lo[0] / cell)), int(math.floor(hi[0] / cell)) + 1):
                for gy in range(int(math.floor(lo[1] / cell)), int(math.floor(hi[1] / cell)) + 1):
                    grid.setdefault((gx, gy), []).append(b)

        def covered(p, own) -> bool:
            for o in grid.get((int(math.floor(p[0] / cell)), int(math.floor(p[1] / cell))), ()):
                if o is own:
                    continue
                lo, hi = boxes[id(o)]
                if not (lo[0] - 0.01 <= p[0] <= hi[0] + 0.01 and lo[1] - 0.01 <= p[1] <= hi[1] + 0.01 and lo[2] - 0.01 <= p[2] <= hi[2] + 0.01):
                    continue
                if all(_dot3(n, p) <= d + 0.01 for n, d in planes[id(o)]):
                    return True
            return False

        def underneath(p) -> bool:
            """Anything solid below this point? (If not, it is under the world: nobody sees up from there.)"""
            for o in grid.get((int(math.floor(p[0] / cell)), int(math.floor(p[1] / cell))), ()):
                lo, hi = boxes[id(o)]
                if lo[0] <= p[0] <= hi[0] and lo[1] <= p[1] <= hi[1] and lo[2] < p[2]:
                    return True
            return False

        shown = 0
        for b in bodies:
            drawn = [f.texture for f in b.faces if not f.texture.startswith("tools/") and f.texture != GLASS]
            # (Decals are thin overlays: their edges stay hidden.)
            if not drawn or not any(f.texture == NODRAW for f in b.faces) or any(t.startswith("decal/") for t in drawn):
                continue
            for i, f in enumerate(b.faces):
                if f.texture != NODRAW:
                    continue
                n, d = planes[id(b)][i]
                poly = [v for v in verts[id(b)] if abs(_dot3(n, v) - d) < 0.05]
                if len(poly) < 3:
                    continue
                c = tuple(sum(v[k] for v in poly) / len(poly) for k in range(3))
                samples = [c] + [tuple(c[k] + (v[k] - c[k]) * 0.8 for k in range(3)) for v in poly]
                outside = [tuple(p[k] + n[k] * 1.0 for k in range(3)) for p in samples]
                # Open where it touches, and still open a hand's width out: a slot narrower than that (between two
                # containers, behind a trim) can't be seen into.
                if not any(not covered(p, b) and not covered(tuple(p[k] + n[k] * 11.0 for k in range(3)), b) for p in outside):
                    continue
                # An underside with nothing below it faces the void under the map.
                if n[2] < -0.7 and not any(underneath(p) for p in outside):
                    continue
                # The brush's own look: for a top or bottom, what its sides wear; for a side, what its top wears.
                flat = abs(n[2]) > 0.7
                like = [g.texture for g in b.faces if g.texture in drawn and (abs(_unit(g.normal)[2]) > 0.7) != flat] or drawn
                f.texture = max(set(like), key=like.count)
                # A building's flat top in its wall's brick or plaster reads wrong from above: roof slabs instead.
                if n[2] > 0.7 and any(w in f.texture for w in ("brick", "plaster", "concrete_wall")):
                    f.texture = "mat/concrete_slabs"
                f.scale = zmap.tex_scale(f.texture) if f.texture.startswith("mat/") else f.scale
                shown += 1
        print(f"exposed {shown} hidden faces that border open air")
        return shown

    def finish(self) -> None:
        if self.glass:
            self.m.brush_entity("func_glass", *self.glass)
        if self.deco:
            self.m.brush_entity("func_illusionary", *self.deco)

    # ---- sightline check (boxes only, approximate) ---------------------------------------------------------------
    def visible(self, a, b, step: float = 16.0) -> bool:
        ax, ay, az = a
        bx, by, bz = b
        d = math.dist(a, b)
        n = max(2, int(d / step))
        lo_x, hi_x = min(ax, bx), max(ax, bx)
        lo_y, hi_y = min(ay, by), max(ay, by)
        cands = [s for s in self.solids if not (s[1][0] < lo_x or s[0][0] > hi_x or s[1][1] < lo_y or s[0][1] > hi_y)]
        for i in range(1, n):
            t = i / n
            p = (ax + (bx - ax) * t, ay + (by - ay) * t, az + (bz - az) * t)
            for lo, hi in cands:
                if lo[0] < p[0] < hi[0] and lo[1] < p[1] < hi[1] and lo[2] < p[2] < hi[2]:
                    return False
        return True

    # ---- walls ------------------------------------------------------------------------------------------------
    def wall(self, axis: str, a0, a1, t0, t1, z0, z1, tex, openings=(), scale=None) -> None:
        """A wall with rectangular holes. Openings in one column must share u0/u1 (stacked windows)."""
        cuts = sorted({a0, a1, *[c for o in openings for c in (o[0], o[1]) if a0 < c < a1]})
        cols = []
        for ua, ub in zip(cuts, cuts[1:]):
            holes = sorted((o[2], o[3]) for o in openings if o[0] <= ua + 1e-6 and o[1] >= ub - 1e-6)
            segs, z = [], z0
            for v0, v1 in holes:
                if v0 > z + 1e-6:
                    segs.append((z, min(v0, z1)))
                z = max(z, v1)
            if z < z1 - 1e-6:
                segs.append((z, z1))
            if cols and cols[-1][2] == segs and abs(cols[-1][1] - ua) < 1e-6:
                cols[-1] = (cols[-1][0], ub, segs)
            else:
                cols.append((ua, ub, segs))
        for ua, ub, segs in cols:
            for za, zb in segs:
                if zb - za < 0.5:
                    continue
                if axis == "x":
                    self.blk((ua, t0, za), (ub, t1, zb), tex, scale)
                else:
                    self.blk((t0, ua, za), (t1, ub, zb), tex, scale)

    def _span(self, axis, u0, u1, t0, t1, z0, z1, tex, scale=None, deco=False):
        if axis == "x":
            return self.blk((u0, t0, z0), (u1, t1, z1), tex, scale, deco)
        return self.blk((t0, u0, z0), (t1, u1, z1), tex, scale, deco)

    def band(self, axis, u0, u1, face, out, z0, z1, depth, tex, scale=None, deco=True, show=("front", "top", "ends")):
        """A strip proud of a wall face (plinth, cornice, string course): `face` is the wall face coordinate, `out`
        ±1 its outward direction, `depth` how far it stands out. Decorative (no collision) unless deco=False.
        Only the faces in `show` are drawn (front, top, bottom, ends)."""
        if deco:
            return self.panel(axis, u0, u1, face, out, z0, z1, depth, tex, show=show)
        t0, t1 = sorted((face, face + out * depth))
        return self._span(axis, u0, u1, t0, t1, z0, z1, tex, scale, deco)

    def panel(self, axis, u0, u1, face, out, v0, v1, depth, tex, show=("front",)):
        """A decorative slab on a wall face drawing only the faces in `show` (front, top, bottom, ends)."""
        t0, t1 = sorted((face, face + out * depth))
        fk = ("+" if out > 0 else "-") + ("y" if axis == "x" else "x")
        bk = ("-" if out > 0 else "+") + ("y" if axis == "x" else "x")
        ends = ("+x", "-x") if axis == "x" else ("+y", "-y")
        tx = {fk: tex if "front" in show else NODRAW, bk: NODRAW, "top": tex if "top" in show else NODRAW,
              "bottom": tex if "bottom" in show else NODRAW}
        for e in ends:
            tx[e] = tex if "ends" in show else NODRAW
        return self._span(axis, u0, u1, t0, t1, v0, v1, tx, deco=True)

    def facade_piece(self, model: str, axis, face, out, u0, u1, v0) -> bool:
        """A modelled window or door (arch.py) over the opening u0..u1 from v0 up, on the facade at `face` facing
        `out`: scaled to the opening's width, its back on the wall. False if the model isn't built."""
        info = props_info().get(model)
        if not info or "anchor" not in info:
            return False
        an = info["anchor"]
        scale = (u1 - u0) / (an["open"][0] * zmap.M)
        yaw = (90 if out > 0 else 270) if axis == "x" else (0 if out > 0 else 180)
        a = math.radians(yaw)
        right = (-math.sin(a), math.cos(a))  # the model's +x on the map
        off = an.get("open_x", 0.0) * scale * zmap.M
        uc = (u0 + u1) / 2
        t = face + out * (info["size"][2] * zmap.M * scale / 2 + 0.3)
        x, y = (uc, t) if axis == "x" else (t, uc)
        x, y = x - right[0] * off, y - right[1] * off
        self.prop(model, x, y, v0 - an["open_z"] * scale * zmap.M, yaw=yaw, scale=round(scale, 4), solid=False)
        return True

    # The town's shutter paints (the house kit's shutter textures) → the modelled windows' colours.
    TOWN_SHUTTERS = {"mat/metal_painted_green": "green", "mat/wood_beam": "brown", "mat/metal_painted_red": "red"}

    def town_window(self, axis, face, out, u0, u1, v0, v1, shutters=None) -> bool:
        """A town-house window piece: the house's shutter colour (or none: bare, blue or grey by the spot), a flower
        box on some ground- and first-floor windows."""
        # Tall openings (a church's) get the round-headed leaded window.
        if (v1 - v0) > 1.8 * (u1 - u0) and "win_church" in props_info():
            return self.facade_piece("win_church", axis, face, out, u0, u1, v0)
        h = hash_unit(u0, v0, face)
        colour = self.TOWN_SHUTTERS.get(shutters or "")
        if colour is None:
            colour = ("bare", "bare", "blue", "grey")[int(h * 4) % 4] if not shutters else "green"
        name = "win_town_bare" if colour == "bare" else f"win_town_{colour}"
        # An iron balcony under some upper-floor windows (none with flower boxes: the balcony has its own).
        balcony = v0 > 150 and hash_unit(v0, u0, face, 3) < 0.22 and "balcony_iron" in props_info()
        if not balcony and v0 < 220 and hash_unit(face, u1, v1) < 0.3 and f"{name}_flowers" in props_info():
            name += "_flowers"
        if balcony:
            bi = props_info()["balcony_iron"]
            scale = (u1 - u0 + 50) / (bi["size"][0] * zmap.M)
            yaw = (90 if out > 0 else 270) if axis == "x" else (0 if out > 0 else 180)
            t = face + out * (bi["size"][2] * zmap.M * scale / 2 + 0.3)
            uc = (u0 + u1) / 2
            x, y = (uc, t) if axis == "x" else (t, uc)
            self.prop("balcony_iron", x, y, v0 - 5 - bi["anchor"]["top"] * scale * zmap.M, yaw=yaw, scale=round(scale, 4), solid=False)
            # A pot or two on it.
            pots = [p_ for j, p_ in enumerate(("potted_plant_02", "planter_box_03")) if p_ in props_info() and hash_unit(u0, j, 9) < 0.6]
            for i, pot in enumerate(pots):
                off = (u1 - u0) * (0.55 if i == 0 else -0.55)
                t2 = face + out * 12
                px, py = (uc + off, t2) if axis == "x" else (t2, uc + off)
                self.prop(pot, px, py, v0 - 5, yaw=yaw, scale=0.6, solid=False)
        return self.facade_piece(name, axis, face, out, u0, u1, v0)

    def flat_window(self, axis, face, out, u0, u1, v0, v1, back=None, frame=None, sill=None, lintel=None,
                    cross=True, shutters=None, casing=None, reveal=5):
        """A window drawn on a facade without a hole: a glass pane, frame bars, sill and lintel (all decorative).
        cross: True (frame + mullion cross), "frame" (frame only) or False (just the pane).
        `casing` (a texture, usually the wall's): a reveal `reveal` units proud around the window, so the pane sits
        back in it and the bake shades its edges; without it the window is a flat sticker.
        With `facade_style` "town" it is a modelled window piece instead (town_window)."""
        if self.facade_style == "town" and back is None and self.town_window(axis, face, out, u0, u1, v0, v1, shutters):
            return
        if self.facade_style in ("office", "industrial") and back is None:
            # The modelled window of the style whose shape (width : height) is nearest the opening's.
            prefix = "win_office" if self.facade_style == "office" else "win_factory"
            shapes = {"tall": 1.0 / 1.4, "sq": 1.0, "wide": 1.6 / 1.3}
            aspect = (u1 - u0) / max(1.0, v1 - v0)
            shape = min(shapes, key=lambda k_: abs(math.log(shapes[k_] / aspect)))
            if self.facade_piece(f"{prefix}_{shape}", axis, face, out, u0, u1, v0):
                return
        pane = self.panel(axis, u0, u1, face, out, v0, v1, 1, back or GLASS_WIN)
        if back is None:
            fit(pane)
        if casing and reveal:
            w = 6
            self.panel(axis, u0 - w, u0, face, out, v0, v1, reveal, casing, show=("front", "ends"))
            self.panel(axis, u1, u1 + w, face, out, v0, v1, reveal, casing, show=("front", "ends"))
            self.panel(axis, u0 - w, u1 + w, face, out, v1, v1 + w, reveal, casing, show=("front", "bottom", "ends"))
            if not sill:
                self.panel(axis, u0 - w, u1 + w, face, out, v0 - 3, v0, reveal, casing, show=("front", "top", "ends"))
        # Some windows have their roller shutter part or all the way down (the same ones every build).
        h = hash_unit(u0, v0, face)
        if back is None and not shutters and h < 0.28:
            down = 0.35 + 0.65 * (h / 0.28)
            sh = self.panel(axis, u0, u1, face, out, v1 - (v1 - v0) * down, v1, 1.5, SHUTTER)
            fit(sh, (1.0, max(1.0, round((v1 - v0) * down / 24))))
        if cross == "frame":
            fr = frame or FRAME
            for a, b in ((u0 - 4, u0), (u1, u1 + 4)):
                self.panel(axis, a, b, face, out, v0, v1 + 4, 2, fr)
            self.panel(axis, u0, u1, face, out, v1, v1 + 4, 2, fr)
        elif cross:
            fr = frame or FRAME
            um, vm = (u0 + u1) / 2, v0 + (v1 - v0) * 0.62
            self.panel(axis, um - 2, um + 2, face, out, v0, v1, 2, fr)
            self.panel(axis, u0, u1, face, out, vm - 2, vm + 2, 2, fr)
            for a, b in ((u0 - 3, u0), (u1, u1 + 3)):
                self.panel(axis, a, b, face, out, v0, v1, 2, fr)
            self.panel(axis, u0 - 3, u1 + 3, face, out, v1, v1 + 3, 2, fr)
        if sill and not shutters:
            self.panel(axis, u0 - 5, u1 + 5, face, out, v0 - 6, v0, 3, sill)
        if lintel:
            self.panel(axis, u0 - 4, u1 + 4, face, out, v1 + 3, v1 + 11, 2, lintel)
        if shutters:
            w = (u1 - u0) / 2
            self.panel(axis, u0 - w - 4, u0 - 4, face, out, v0, v1, 3, shutters)
            self.panel(axis, u1 + 4, u1 + w + 4, face, out, v0, v1, 3, shutters)

    def window(self, axis, face, out, u0, u1, v0, v1, depth=12, frame=None, sill=None, glass=True, back=TRIM,
               cross=True, lintel=None):
        """Dressing for a window hole cut in a wall whose outer face is at `face` (outward `out`): a dark back plate
        `depth` in, a glass pane, frame bars, a sill and a lintel. The hole itself comes from wall(openings=...)."""
        inner = face - out * depth
        t0, t1 = sorted((inner, inner - out * 4))
        if back:
            self._span(axis, u0, u1, t0, t1, v0, v1, back)
        fr = frame or FRAME
        if cross:
            um, vm = (u0 + u1) / 2, v0 + (v1 - v0) * 0.62
            self.panel(axis, um - 1.5, um + 1.5, inner, out, v0, v1, 2, fr)
            self.panel(axis, u0, u1, inner, out, vm - 1.5, vm + 1.5, 2, fr)
        if glass:
            a, b = sorted((inner + out * 3, inner + out * 4))
            if axis == "x":
                self.pane((u0, a, v0), (u1, b, v1))
            else:
                self.pane((a, u0, v0), (b, u1, v1))
        if sill:
            self.panel(axis, u0 - 4, u1 + 4, face, out, v0 - 5, v0, 5, sill, show=("front", "top"))
        if lintel:
            self.panel(axis, u0 - 4, u1 + 4, face, out, v1, v1 + 8, 2, lintel)

    def door_leaf(self, axis, face, out, u0, u1, v0, v1, depth=8, tex=None, frame=None):
        """A closed (fake) door set `depth` into a hole: a leaf with a frame and a dark gap at the top."""
        inner = face - out * depth
        t0, t1 = sorted((inner, inner - out * 4))
        self._span(axis, u0, u1, t0, t1, v0, v1, tex or T("wood_planks"))
        if frame:
            self.panel(axis, u0 - 6, u0, face, out, v0, v1 + 6, 2, frame)
            self.panel(axis, u1, u1 + 6, face, out, v0, v1 + 6, 2, frame)
            self.panel(axis, u0, u1, face, out, v1, v1 + 6, 2, frame)

    def facade(self, axis, a0, a1, face, out, z0, z1, tex, openings, depth=12, back_core=TRIM, **win):
        """A facade skin of thickness `depth` in front of a solid core, windows as holes showing the dark core.

        `openings`: (u0, u1, v0, v1, kind) with kind "win" (frame + sill), "door" (fake door leaf), "open" (just a
        dark recess), "blind" (a shallow panel)."""
        t0, t1 = sorted((face, face - out * depth))
        holes = [o[:4] for o in openings]
        self.wall(axis, a0, a1, t0, t1, z0, z1, tex, holes)
        for u0, u1, v0, v1, kind in openings:
            if kind == "win":
                w = {k_: v for k_, v in win.items() if k_ != "door_tex"}
                self.window(axis, face, out, u0, u1, v0, v1, depth=depth, back=None, **w)
            elif kind == "door":
                self.door_leaf(axis, face, out, u0, u1, v0, v1, depth=depth - 4, tex=win.get("door_tex"),
                               frame=win.get("frame"))

    # ---- solid (inaccessible) buildings ---------------------------------------------------------------------
    def solid_building(self, x0, y0, x1, y1, z0, z1, tex, sides="nsew", core=TRIM, depth=12, bays=None,
                       floors=None, win_w=48, win_h=72, door=None, plinth=None, plinth_h=24, cornice=None,
                       cornice_h=12, top=None, frame=None, sill=None, lintel=None, glass=True, cross=True,
                       door_tex=None, pilasters=None, scale=None, flat=True, shutters=None):
        """A closed building: a dark core with facade skins (windows as recesses) on the listed sides.

        `bays` (window spacing along a facade) and `floors` (list of window sill heights) lay out the windows;
        `door` = (side, u_center, width, height) adds a fake door."""
        tex = variant(tex, x0, y0, x1)
        d = 0 if flat else depth
        if flat:
            skin = {"top": top or NODRAW, "bottom": NODRAW, "sides": NODRAW}
            for sd, key in (("s", "-y"), ("n", "+y"), ("w", "-x"), ("e", "+x")):
                if sd in sides:
                    skin[key] = tex
            self.blk((x0, y0, z0), (x1, y1, z1), skin, scale)
        else:
            self.blk((x0 + d, y0 + d, z0), (x1 - d, y1 - d, z1), {"sides": core, "top": top or NODRAW, "bottom": NODRAW})
        floors = floors if floors is not None else []
        spec = {"s": ("x", x0, x1, y0, -1), "n": ("x", x0, x1, y1, 1), "w": ("y", y0, y1, x0, -1),
                "e": ("y", y0, y1, x1, 1)}
        for side, (axis, a0, a1, face, out) in spec.items():
            ops = []
            if side in sides and bays:
                n = max(1, int((a1 - a0 - 2 * d) // bays))
                span = (a1 - a0) / n
                for i in range(n):
                    uc = a0 + span * (i + 0.5)
                    for fz in floors:
                        ops.append((uc - win_w / 2, uc + win_w / 2, z0 + fz, z0 + fz + win_h, "win"))
            if door and door[0] == side:
                _, uc, w, h = door
                ops = [o for o in ops if o[1] < uc - w / 2 - 8 or o[0] > uc + w / 2 + 8 or o[2] > z0 + h + 16]
                ops.append((uc - w / 2, uc + w / 2, z0, z0 + h, "door"))
            if side in sides:
                self.facades.append({"axis": axis, "a0": a0, "a1": a1, "face": face, "out": out, "z0": z0, "z1": z1,
                                     "tex": tex if isinstance(tex, str) else None,
                                     "wins": [o[:4] for o in ops if o[4] == "win"],
                                     "doors": [o[:4] for o in ops if o[4] == "door"]})
            # Skin ends: the x-facades span the full length, the y-facades fit between them.
            if axis == "x":
                lo, hi = a0, a1
            else:
                lo, hi = a0 + d, a1 - d
            if flat:
                if side not in sides:
                    continue
                for u0, u1, v0, v1, kind in ops:
                    if kind == "win":
                        self.flat_window(axis, face, out, u0, u1, v0, v1, frame=frame, sill=sill, lintel=lintel,
                                         cross=cross, shutters=shutters, casing=tex if isinstance(tex, str) else None)
                    else:
                        if self.facade_style == "town":
                            doors = ("door_town_brown", "door_town_green", "door_town_wood")
                            if self.facade_piece(doors[int(hash_unit(u0, face) * 3) % 3], axis, face, out, u0, u1, v0):
                                continue
                        self.panel(axis, u0, u1, face, out, v0, v1, 1, door_tex or T("wood_planks"))
                        if frame:
                            self.panel(axis, u0 - 6, u0, face, out, v0, v1 + 6, 3, frame)
                            self.panel(axis, u1, u1 + 6, face, out, v0, v1 + 6, 3, frame)
                            self.panel(axis, u0, u1, face, out, v1, v1 + 6, 3, frame)
            else:
                skin_tex = tex if side in sides else {"sides": tex, "top": NODRAW}
                self.facade(axis, lo, hi, face, out, z0, z1, skin_tex, ops, depth=d, frame=frame, sill=sill,
                            lintel=lintel, glass=glass, cross=cross, door_tex=door_tex)
                if side not in sides:
                    continue
            if plinth:
                self.band(axis, a0 - (4 if axis == "x" else 0), a1 + (4 if axis == "x" else 0), face, out, z0,
                          z0 + plinth_h, 4, plinth, show=("front", "top"))
            if cornice:
                self.band(axis, a0 - (10 if axis == "x" else 0), a1 + (10 if axis == "x" else 0), face, out,
                          z1 - cornice_h, z1, 10, cornice, show=("front", "bottom", "top"))
            if pilasters:
                tex_p, every, w = pilasters
                n = max(1, int((a1 - a0) // every))
                for i in range(n + 1):
                    uc = a0 + (a1 - a0) * i / n
                    ua, ub = max(a0, uc - w / 2), min(a1, uc + w / 2)
                    self.band(axis, ua, ub, face, out, z0 + (plinth_h if plinth else 0), z1 - (cornice_h if cornice else 0), 6, tex_p)

    # ---- roofs ----------------------------------------------------------------------------------------------
    def gable_roof(self, x0, y0, x1, y1, z, rise, axis="x", tex=None, gable=None, over=16, thick=8, ridge=None,
                   solid=True, under=None, hide=(), over_end=None, gable_sides="nsew", rafters=None):
        """A pitched roof over x0..x1 × y0..y1 from eaves height z; ridge along `axis`, `rise` above z.
        `gable`: texture of the triangular attic fill (the gable ends); `over`: eave overhang. `rafters` (a
        texture): the rafter ends under each drawn eave and a fascia board along its edge, in that texture."""
        tex = tex or T("roof_tiles")
        under = under or TRIM
        if rafters:
            self._rafters(x0, y0, x1, y1, z, rise, axis, over, hide, rafters)
        oe = (max(over.values()) if isinstance(over, dict) else over) if over_end is None else over_end
        if axis == "x":
            ym = (y0 + y1) / 2
            half = (y1 - y0) / 2
            slope = rise / half
            for sd, sgn, yedge in (("s", -1, y0), ("n", 1, y1)):
                ov = over.get(sd, 16) if isinstance(over, dict) else over
                ya, yb, ze = yedge + sgn * ov, ym, z - ov * slope
                pts = [(x0 - oe, ya, ze), (x1 + oe, ya, ze), (x0 - oe, yb, z + rise), (x1 + oe, yb, z + rise)]
                pts += [(p[0], p[1], p[2] + thick) for p in pts]
                tt = NODRAW if sd in hide else tex
                self.add(hull(pts, lambda n, tt=tt: tt if n[2] > 0.3 else (under if n[2] < -0.3 else NODRAW)))
            if gable:
                pts = [(x0, y0, z), (x1, y0, z), (x0, y1, z), (x1, y1, z), (x0, ym, z + rise), (x1, ym, z + rise)]
                self.add(hull(pts, lambda n: gable if abs(n[2]) < 0.3 and abs(n[0]) > 0.5 and ("e" if n[0] > 0 else "w") in gable_sides else NODRAW))
            if ridge:
                self.blk((x0 - oe, ym - 5, z + rise + thick - 4), (x1 + oe, ym + 5, z + rise + thick + 4), ridge)
        else:
            xm = (x0 + x1) / 2
            half = (x1 - x0) / 2
            slope = rise / half
            for sd, sgn, xedge in (("w", -1, x0), ("e", 1, x1)):
                ov = over.get(sd, 16) if isinstance(over, dict) else over
                xa, xb, ze = xedge + sgn * ov, xm, z - ov * slope
                pts = [(xa, y0 - oe, ze), (xa, y1 + oe, ze), (xb, y0 - oe, z + rise), (xb, y1 + oe, z + rise)]
                pts += [(p[0], p[1], p[2] + thick) for p in pts]
                tt = NODRAW if sd in hide else tex
                self.add(hull(pts, lambda n, tt=tt: tt if n[2] > 0.3 else (under if n[2] < -0.3 else NODRAW)))
            if gable:
                pts = [(x0, y0, z), (x0, y1, z), (x1, y0, z), (x1, y1, z), (xm, y0, z + rise), (xm, y1, z + rise)]
                self.add(hull(pts, lambda n: gable if abs(n[2]) < 0.3 and abs(n[1]) > 0.5 and ("n" if n[1] > 0 else "s") in gable_sides else NODRAW))
            if ridge:
                self.blk((xm - 5, y0 - oe, z + rise + thick - 4), (xm + 5, y1 + oe, z + rise + thick + 4), ridge)

    def _rafters(self, x0, y0, x1, y1, z, rise, axis, over, hide, tex, every=44, w=4, h=6):
        """Rafter ends under the overhang of each eave (sloped like the roof), and a fascia board at its edge."""
        half = ((y1 - y0) if axis == "x" else (x1 - x0)) / 2
        slope = rise / half
        sides = (("s", -1, y0), ("n", 1, y1)) if axis == "x" else (("w", -1, x0), ("e", 1, x1))
        a0, a1 = (x0, x1) if axis == "x" else (y0, y1)
        for sd, sgn, edge in sides:
            ov = over.get(sd, 16) if isinstance(over, dict) else over
            if ov < 6 or sd in hide:
                continue
            ze = z - ov * slope
            n = max(2, int((a1 - a0) // every))
            for i in range(n + 1):
                u = a0 + 6 + (a1 - a0 - 12) * i / n
                pts = []
                for uu in (u - w / 2, u + w / 2):
                    for t, zz in ((edge, z), (edge + sgn * (ov - 1), z - (ov - 1) * slope)):
                        for dz in (-h, -0.2):
                            pts.append((uu, t, zz + dz) if axis == "x" else (t, uu, zz + dz))
                self.add(hull(pts, lambda n_: tex if n_[2] < 0.9 else NODRAW), deco=True)
            # The fascia: a board along the eave's edge, as deep as the roof slab plus the rafters.
            t0, t1 = sorted((edge + sgn * (ov - 1.5), edge + sgn * (ov + 0.5)))
            lo = (a0 - 2, t0, ze - h) if axis == "x" else (t0, a0 - 2, ze - h)
            hi = (a1 + 2, t1, ze + 7) if axis == "x" else (t1, a1 + 2, ze + 7)
            self.dblk(lo, hi, {"sides": tex, "top": NODRAW, "bottom": tex})

    def chimney(self, x, y, z0, z1, w=32, tex=None):
        """A brick chimney with a cap (decorative, so it doesn't split the roof faces)."""
        tex = tex or T("bricks090")
        self.dblk((x - w / 2, y - w / 2, z0), (x + w / 2, y + w / 2, z1), {"sides": tex, "top": NODRAW, "bottom": NODRAW})
        self.dblk((x - w / 2 - 3, y - w / 2 - 3, z1), (x + w / 2 + 3, y + w / 2 + 3, z1 + 6), {"sides": TRIM, "top": TRIM, "bottom": TRIM})

    # ---- props ------------------------------------------------------------------------------------------------
    def crate(self, x, y, z, s=48, h=None, tex=None):
        """A crate with its texture fitted to each face; (x, y) is the centre. A cube (h = s) is the modelled crate
        (arch.py crate_pine / crate_old) over an invisible box of the same size, where those are built."""
        h = h or s
        z = lift(z)
        if tex is None and abs(h - s) < 0.5 and "crate_pine" in props_info():
            b = box((x - s / 2, y - s / 2, z), (x + s / 2, y + s / 2, z + h), NODRAW)
            self.add(b)
            k_ = hash_unit(x, y, z)
            model = "crate_old" if k_ < 0.3 and "crate_old" in props_info() else "crate_pine"
            self.prop(model, x, y, z, yaw=90 * int(k_ * 40 % 4), scale=round(s / zmap.M, 4), solid=False)
            return z + h
        b = fit(box((x - s / 2, y - s / 2, z), (x + s / 2, y + s / 2, z + h), tex or T("wood_crate")))
        b.faces[2].texture = NODRAW  # bottom
        self.add(b)
        return z + h

    def sandbag_wall(self, lo, hi, tex=None):
        """A sandbag wall over the box lo..hi: the modelled metre-long segments (arch.py sandbags_1m) tiled along
        its length, as many rows deep as fit, over an invisible box of the same size; a plain block of `tex` (dirt)
        where the model isn't built."""
        info = props_info().get("sandbags_1m")
        if info is None:
            self.blk(lo, hi, {"sides": tex or T("dirt"), "top": tex or T("dirt"), "bottom": NODRAW})
            return
        self.blk(lo, hi, NODRAW)
        along_x = (hi[0] - lo[0]) >= (hi[1] - lo[1])
        length = (hi[0] - lo[0]) if along_x else (hi[1] - lo[1])
        depth = (hi[1] - lo[1]) if along_x else (hi[0] - lo[0])
        seg = info["size"][0] * zmap.M
        n = max(1, round(length / seg))
        rows = max(1, round(depth / (info["size"][2] * zmap.M)))
        scale = (hi[2] - lo[2]) / (info["size"][1] * zmap.M)
        for r in range(rows):
            t = (lo[1] if along_x else lo[0]) + depth * (r + 0.5) / rows
            for i in range(n):
                u = (lo[0] if along_x else lo[1]) + length * (i + 0.5) / n
                x, y = (u, t) if along_x else (t, u)
                self.prop("sandbags_1m", x, y, lo[2], yaw=(90 if along_x else 0) + 180 * ((i + r) % 2), scale=round(scale, 4), solid=False)

    def crate_long(self, lo, hi, tex=None):
        """A crate of any size, texture fitted in whole tiles of about 48 units."""
        sx, sy, sz = hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]
        b = box(lo, hi, tex or T("wood_crate"))
        # fit per face with tiles by the longer side
        fit(b, (1.0, 1.0))
        for f in b.faces:
            n = f.normal
            if abs(n[0]) > 0.5:
                tu, tv = max(1, round(sy / 48)), max(1, round(sz / 48))
            elif abs(n[1]) > 0.5:
                tu, tv = max(1, round(sx / 48)), max(1, round(sz / 48))
            else:
                tu, tv = max(1, round(sx / 48)), max(1, round(sy / 48))
            f.scale /= tu
            f.scale_v /= tv
            f.shift = (f.shift[0] * tu, f.shift[1] * tv)
        self.add(b)

    def pallet(self, x, y, z, n=1, orient="x"):
        """A pallet stack (48 × 40, 6 per pallet): one block with the plank texture."""
        w, d = (48, 40) if orient == "x" else (40, 48)
        h = 6 * n
        z = lift(z)
        # The modelled pallets (arch.py) where built: a stack of empty ones, or a loaded one on a shorter stack.
        if "pallet" in props_info():
            yaw = 0 if orient == "x" else 90
            loaded = n >= 3 and hash_unit(x, y) < 0.6 and "pallet_boxes" in props_info()
            if loaded:
                # (the collision as tall as the load)
                h = 6 + props_info()["pallet_boxes" if hash_unit(y, x) < 0.5 else "pallet_sacks"]["size"][1] * zmap.M - 6
            self.blk((x - w / 2, y - d / 2, z), (x + w / 2, y + d / 2, z + h), NODRAW)
            stack = n if not loaded else 0
            for i in range(stack):
                self.prop("pallet", x, y, z - LIFT + 6 * i, yaw=yaw + 180 * (i % 2), solid=False)
            if loaded:
                load = "pallet_boxes" if hash_unit(y, x) < 0.5 else "pallet_sacks"
                self.prop(load, x, y, z - LIFT, yaw=yaw, solid=False)
            return z + h
        self.blk((x - w / 2, y - d / 2, z), (x + w / 2, y + d / 2, z + h), {"sides": T("wood_planks"), "top": T("wood_planks"), "bottom": NODRAW})
        return z + h

    def barrel(self, x, y, z, r=12, h=36, tex=None, hoop=None, lid=None):
        # A real model when there is one: wooden casks for wood, steel drums otherwise.
        wood = tex is not None and "wood" in tex
        choice = ["wine_barrel_01"] if wood else ["barrel_01", "barrel_02", "barrel_03"]
        choice = [c for c in choice if c in props_info()]
        if choice:
            name = choice[int(hash_unit(x, y) * len(choice)) % len(choice)]
            size = props_info()[name].get("size", [0.6, 0.9, 0.6])
            self.prop(name, x, y, z, yaw=hash_unit(y, x) * 360, scale=(h / zmap.M) / max(size[1], 0.1))
            return
        self._barrel_brush(x, y, z, r, h, tex, hoop, lid)

    def _barrel_brush(self, x, y, z, r=12, h=36, tex=None, hoop=None, lid=None):
        tex = tex or T("metal_painted_blue")
        z = lift(z)
        self.add(prism((x, y), r, z, z + h, 8, {"sides": tex, "top": lid or TRIM, "bottom": NODRAW}, rot=22.5))
        hp = hoop or TRIM
        for hz in (z + h * 0.72,):
            self.add(prism((x, y), r + 1, hz, hz + 3, 8, {"sides": hp, "top": NODRAW, "bottom": NODRAW}, rot=22.5), deco=True)
        return z + h

    def keg(self, x, y, z, r=18, length=44, axis="x"):
        """A wooden barrel lying on its side (octagonal), iron hoops."""
        z = lift(z)
        c = (y, z + r) if axis == "x" else (x, z + r)
        a0, a1 = ((x - length / 2, x + length / 2) if axis == "x" else (y - length / 2, y + length / 2))
        self.add(prism(c, r, a0, a1, 8, lambda n: T("wood_planks") if (abs(n[0]) < 0.5 if axis == "x" else abs(n[1]) < 0.5) else T("wood_beam"), rot=22.5, axis=axis))

    def container(self, x0, y0, z0, length=480, axis="x", color="red", doors="+", top=None, detail=True):
        """An ISO container (96 wide, 102 high): corrugated long sides, painted ends, corner posts and top rails,
        door bars on the door end. (x0, y0) is its low corner; `doors` "+" or "-" picks the door end."""
        paint = corr = container_paint(color)
        W, H = 96, 102
        if z0 > 0.5:
            z0 = lift(z0)
        # The modelled container (arch.py) where there is one for this length and colour: an invisible box of the
        # same size keeps the collision (and the sightlines) exactly as before.
        c = {"metal_rust": "orange", "metal_corrugated": "grey", "rust": "orange"}.get(color, color)
        model = {480: f"container40_{c}", 240: f"container20_{c}"}.get(length)
        if self.container_props and model in props_info():
            lo = (x0, y0, z0)
            hi = (x0 + length, y0 + W, z0 + H) if axis == "x" else (x0 + W, y0 + length, z0 + H)
            self.blk(lo, hi, NODRAW)
            yaw = (270 if doors == "+" else 90) if axis == "x" else (0 if doors == "+" else 180)
            self.prop(model, (lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, z0, yaw=yaw, solid=False)
            return hi
        if axis == "x":
            lo, hi = (x0, y0, z0), (x0 + length, y0 + W, z0 + H)
            tex = {"+y": corr, "-y": corr, "+x": paint, "-x": paint, "top": top or corr, "bottom": NODRAW}
        else:
            lo, hi = (x0, y0, z0), (x0 + W, y0 + length, z0 + H)
            tex = {"+x": corr, "-x": corr, "+y": paint, "-y": paint, "top": top or corr, "bottom": NODRAW}
        self.blk(lo, (hi[0], hi[1], hi[2]), tex)
        if not detail:
            return hi
        # painted corner posts on the long sides, top and bottom rails
        if axis == "x":
            for face, out in ((lo[1], -1), (hi[1], 1)):
                self.panel("x", lo[0], hi[0], face, out, z0 + H - 11, z0 + H, 1, paint)
                self.panel("x", lo[0], hi[0], face, out, z0, z0 + 9, 1, paint)
                for u in (lo[0], hi[0] - 7):
                    self.panel("x", u, u + 7, face, out + out * 0.0, z0 + 9, z0 + H - 11, 1, paint)
            ex = hi[0] if doors == "+" else lo[0]
            for yy in (lo[1] + 22, lo[1] + 40, lo[1] + 56, lo[1] + 74):
                self.panel("y", yy - 1.5, yy + 1.5, ex, 1 if doors == "+" else -1, z0 + 10, z0 + H - 10, 1.5, TRIM)
        else:
            for face, out in ((lo[0], -1), (hi[0], 1)):
                self.panel("y", lo[1], hi[1], face, out, z0 + H - 11, z0 + H, 1, paint)
                self.panel("y", lo[1], hi[1], face, out, z0, z0 + 9, 1, paint)
                for u in (lo[1], hi[1] - 7):
                    self.panel("y", u, u + 7, face, out, z0 + 9, z0 + H - 11, 1, paint)
            ey = hi[1] if doors == "+" else lo[1]
            for xx in (lo[0] + 22, lo[0] + 40, lo[0] + 56, lo[0] + 74):
                self.panel("x", xx - 1.5, xx + 1.5, ey, 1 if doors == "+" else -1, z0 + 10, z0 + H - 10, 1.5, TRIM)
        return hi

    def wheel(self, x, y, z, r=20, w=12, axis="y"):
        """A wheel lying along `axis` (the axle), centre (x, y) on the ground plane z (wheel touches z)."""
        if axis == "y":
            self.add(prism((x, z + r), r, y - w / 2, y + w / 2, 8, RUBBER, rot=0, axis="y"), deco=True)
        else:
            self.add(prism((y, z + r), r, x - w / 2, x + w / 2, 8, RUBBER, rot=0, axis="x"), deco=True)

    def truck(self, x, y, z=0, heading="+x", paint="metal_painted_blue", cargo="container", color="green"):
        """A semi truck: cab, chassis, wheels and a box or container trailer. (x, y) is the cab front centre;
        `heading` the way the cab faces (+x, -x, +y, -y)."""
        L = 640  # overall length
        sgn = 1 if heading[0] == "+" else -1
        ax = heading[1]

        def B(u0, u1, v0, v1, z0, z1, tex):
            # u along the truck (from the front, backwards), v across (centre 0).
            if ax == "x":
                xa, xb = sorted((x - sgn * u0, x - sgn * u1))
                self.blk((xa, y + v0, z + z0), (xb, y + v1, z + z1), tex)
            else:
                ya, yb = sorted((y - sgn * u0, y - sgn * u1))
                self.blk((x + v0, ya, z + z0), (x + v1, yb, z + z1), tex)

        # The modelled semi (arch.py truck_box_* / truck_flat_*) where built: invisible boxes where the brush cab,
        # chassis and cargo stood keep the collision; a container still stands on the flatbed as its own piece.
        colour = {"metal_painted_green": "green", "metal_painted_red": "red", "metal_painted_blue": "blue"}.get(paint, "green")
        model = f"truck_box_{colour}" if cargo == "box" else f"truck_flat_{colour}"
        if model not in props_info() and cargo == "box":
            model = "truck_box_green"
        if model in props_info():
            B(0, 88, -46, 46, 24, 118, NODRAW)
            B(88, 610, -48, 48, 14, 26, NODRAW)
            if cargo == "box":
                B(110, 610, -48, 48, 26, 150, NODRAW)
            info = props_info()[model]
            half = info["size"][2] * zmap.M / 2
            cx, cy = (x - sgn * (half - 6), y) if ax == "x" else (x, y - sgn * (half - 6))
            yaw = {"+x": 0, "-x": 180, "+y": 90, "-y": 270}[heading]
            self.prop(model, cx, cy, z, yaw=yaw, solid=False)
            if cargo == "container":
                if ax == "x":
                    self.container(min(x - sgn * 120, x - sgn * 600), y - 48, z + 26, 480, "x", color, doors="-" if sgn > 0 else "+")
                else:
                    self.container(x - 48, min(y - sgn * 120, y - sgn * 600), z + 26, 480, "y", color, doors="-" if sgn > 0 else "+")
            return L
        cab = T(paint)
        # Cab: lower body, window band, roof.
        B(0, 88, -46, 46, 24, 70, {"sides": cab, "top": cab, "bottom": NODRAW})
        B(8, 88, -46, 46, 70, 108, {"sides": TRIM, "top": cab, "bottom": NODRAW})
        B(0, 8, -46, 46, 70, 108, {"sides": TRIM, "top": cab, "bottom": NODRAW})
        B(4, 84, -44, 44, 108, 118, cab)
        B(-3, 0, -40, 40, 30, 56, TRIM)  # grille
        B(-5, 0, -46, 46, 16, 26, T("metal_plate"))  # bumper
        B(0, 600, -30, 30, 14, 26, TRIM)  # chassis
        for u in (40, 500, 560):
            for v in (-38, 38):
                if ax == "x":
                    self.wheel(x - sgn * u, y + v, z, 20, 14, axis="y")
                else:
                    self.wheel(x + v, y - sgn * u, z, 20, 14, axis="x")
        if cargo == "container":
            if ax == "x":
                x0 = min(x - sgn * 120, x - sgn * 600)
                self.container(x0, y - 48, z + 26, 480, "x", color, doors="-" if sgn > 0 else "+")
            else:
                y0 = min(y - sgn * 120, y - sgn * 600)
                self.container(x - 48, y0, z + 26, 480, "y", color, doors="-" if sgn > 0 else "+")
        elif cargo == "box":
            B(110, 610, -48, 48, 26, 150, {"sides": T("metal_corrugated"), "top": T("metal_plate"), "bottom": NODRAW})
        return L

    def van(self, x, y, z=0, heading="+x", paint="metal_painted_green"):
        z = lift(z)
        sgn = 1 if heading[0] == "+" else -1
        ax = heading[1]

        def B(u0, u1, v0, v1, z0, z1, tex):
            if ax == "x":
                xa, xb = sorted((x - sgn * u0, x - sgn * u1))
                self.blk((xa, y + v0, z + z0), (xb, y + v1, z + z1), tex)
            else:
                ya, yb = sorted((y - sgn * u0, y - sgn * u1))
                self.blk((x + v0, ya, z + z0), (x + v1, yb, z + z1), tex)

        colour = {"metal_painted_green": "green", "metal_painted_blue": "blue"}.get(paint, "white")
        if f"van_{colour}" in props_info():
            B(0, 200, -38, 38, 14, 92, NODRAW)
            cx, cy = (x - sgn * 100, y) if ax == "x" else (x, y - sgn * 100)
            yaw = {"+x": 0, "-x": 180, "+y": 90, "-y": 270}[heading]
            self.prop(f"van_{colour}", cx, cy, z - LIFT, yaw=yaw, solid=False)
            return
        p = T(paint)
        B(0, 36, -38, 38, 14, 44, {"sides": p, "top": p, "bottom": NODRAW})
        B(20, 200, -38, 38, 14, 60, {"sides": p, "top": p, "bottom": NODRAW})
        B(28, 200, -38, 38, 60, 92, {"sides": p, "top": p, "bottom": NODRAW})
        B(22, 60, -36, 36, 44, 80, {"sides": TRIM, "top": NODRAW, "bottom": NODRAW})
        B(-2, 0, -36, 36, 18, 30, TRIM)
        for u in (30, 160):
            for v in (-32, 32):
                if ax == "x":
                    self.wheel(x - sgn * u, y + v, z, 14, 10, axis="y")
                else:
                    self.wheel(x + v, y - sgn * u, z, 14, 10, axis="x")

    def car(self, x, y, z=0, heading="+x", paint="metal_painted_red"):
        z = lift(z)
        sgn = 1 if heading[0] == "+" else -1
        ax = heading[1]
        # A parked car under a tarpaulin (Poly Haven's covered_car) where it is built, over the brush car's shape.
        if "covered_car" in props_info():
            (xa, xb), (ya, yb) = (sorted((x, x - sgn * 170)), (y - 34, y + 34)) if ax == "x" else ((x - 34, x + 34), sorted((y, y - sgn * 170)))
            self.blk((xa, ya, z + 10), (xb, yb, z + 36), NODRAW)
            self.blk(((xa + xb) / 2 - (40 if ax == "x" else 30), (ya + yb) / 2 - (30 if ax == "x" else 40), z + 36),
                     ((xa + xb) / 2 + (40 if ax == "x" else 30), (ya + yb) / 2 + (30 if ax == "x" else 40), z + 56), NODRAW)
            yaw = {"+x": 0, "-x": 180, "+y": 90, "-y": 270}[heading]
            self.prop("covered_car", (xa + xb) / 2, (ya + yb) / 2, z - LIFT, yaw=yaw, solid=False)
            return

        def B(u0, u1, v0, v1, z0, z1, tex):
            if ax == "x":
                xa, xb = sorted((x - sgn * u0, x - sgn * u1))
                self.blk((xa, y + v0, z + z0), (xb, y + v1, z + z1), tex)
            else:
                ya, yb = sorted((y - sgn * u0, y - sgn * u1))
                self.blk((x + v0, ya, z + z0), (x + v1, yb, z + z1), tex)

        p = T(paint)
        B(0, 170, -34, 34, 10, 36, {"sides": p, "top": p, "bottom": NODRAW})
        B(50, 130, -30, 30, 36, 56, {"sides": TRIM, "top": p, "bottom": NODRAW})
        for u in (30, 140):
            for v in (-28, 28):
                if ax == "x":
                    self.wheel(x - sgn * u, y + v, z, 12, 8, axis="y")
                else:
                    self.wheel(x + v, y - sgn * u, z, 12, 8, axis="x")

    def forklift(self, x, y, z=0, heading="+x"):
        z = lift(z)
        sgn = 1 if heading[0] == "+" else -1
        ax = heading[1]

        def B(u0, u1, v0, v1, z0, z1, tex, deco=False):
            if ax == "x":
                xa, xb = sorted((x + sgn * u0, x + sgn * u1))
                self.blk((xa, y + v0, z + z0), (xb, y + v1, z + z1), tex, deco=deco)
            else:
                ya, yb = sorted((y + sgn * u0, y + sgn * u1))
                self.blk((x + v0, ya, z + z0), (x + v1, yb, z + z1), tex, deco=deco)

        body = T("metal_painted_red")
        B(-40, 40, -24, 24, 6, 44, {"sides": body, "top": body, "bottom": NODRAW})
        B(-40, -20, -24, 24, 44, 64, {"sides": body, "top": body, "bottom": NODRAW})  # counterweight
        B(-28, 14, -22, 22, 88, 91, TRIM, deco=True)  # roof guard
        B(10, 14, -22, 22, 44, 88, {"sides": TRIM, "top": NODRAW, "bottom": NODRAW}, deco=True)
        B(40, 44, -18, 18, 2, 110, {"sides": T("metal_plate"), "top": T("metal_plate"), "bottom": NODRAW})  # mast
        B(44, 90, -16, 16, 2, 5, {"sides": T("metal_plate"), "top": T("metal_plate"), "bottom": NODRAW}, deco=True)

    def bollard(self, x, y, z=0, tex=None):
        """A mooring bollard (quay)."""
        t = tex or TRIM
        z = lift(z)
        self.add(prism((x, y), 9, z, z + 22, 8, {"sides": t, "top": t, "bottom": NODRAW}, rot=22.5))
        self.add(prism((x, y), 13, z + 22, z + 27, 8, {"sides": t, "top": t, "bottom": t}, rot=22.5))

    def post(self, x, y, z0, z1, w=6, tex=TRIM, solid=False):
        self.blk((x - w / 2, y - w / 2, z0), (x + w / 2, y + w / 2, z1), {"sides": tex, "top": tex, "bottom": NODRAW},
                 deco=not solid)

    def lamp(self, x, y, z=0, h=240, arm="+x", tex=None):
        # Old-town lantern posts (the green ones) are a model at its own size; the tall yard masts stay brushes.
        if tex == T("metal_painted_green") and "street_lamp_01" in props_info():
            yaw = {"+x": 0, "+y": 90, "-x": 180, "-y": 270}[arm]
            self.prop("street_lamp_01", x, y, z, yaw=yaw, solid=False)
            return
        t = tex or TRIM
        self.post(x, y, z, z + h, 5, t)
        self.clip((x - 8, y - 8, z), (x + 8, y + 8, z + h))
        dx, dy = {"+x": (1, 0), "-x": (-1, 0), "+y": (0, 1), "-y": (0, -1)}[arm]
        ax0, ax1 = sorted((x, x + dx * 40))
        ay0, ay1 = sorted((y, y + dy * 40))
        if dx:
            self.dblk((ax0, y - 2, z + h - 6), (ax1, y + 2, z + h - 2), {"sides": t, "top": t, "bottom": t, "+x": NODRAW, "-x": NODRAW})
        else:
            self.dblk((x - 2, ay0, z + h - 6), (x + 2, ay1, z + h - 2), {"sides": t, "top": t, "bottom": t, "+y": NODRAW, "-y": NODRAW})
        hx, hy = x + dx * 40, y + dy * 40
        self.dblk((hx - 9, hy - 6, z + h - 12), (hx + 9, hy + 6, z + h - 2), {"sides": t, "top": t, "bottom": T("lamp_glass")})

    def bench(self, x, y, z=0, axis="x", L=96):
        if "painted_wooden_bench" in props_info():
            # Its back where the brush bench had it (+y or +x), facing away from it.
            self.prop("painted_wooden_bench", x, y, z, yaw=270 if axis == "x" else 180, solid=False)
            return
        if axis == "x":
            self.dblk((x - L / 2, y - 10, z + 16), (x + L / 2, y + 10, z + 19), T("wood_planks"))
            self.dblk((x - L / 2, y + 8, z + 19), (x + L / 2, y + 11, z + 34), T("wood_planks"))
            for u in (x - L / 2 + 8, x + L / 2 - 12):
                self.dblk((u, y - 9, z), (u + 4, y + 9, z + 16), TRIM)
            self.clip((x - L / 2, y - 10, z), (x + L / 2, y + 11, z + 34))
        else:
            self.dblk((x - 10, y - L / 2, z + 16), (x + 10, y + L / 2, z + 19), T("wood_planks"))
            self.dblk((x + 8, y - L / 2, z + 19), (x + 11, y + L / 2, z + 34), T("wood_planks"))
            for u in (y - L / 2 + 8, y + L / 2 - 12):
                self.dblk((x - 9, u, z), (x + 9, u + 4, z + 16), TRIM)
            self.clip((x - 10, y - L / 2, z), (x + 11, y + L / 2, z + 34))

    def fence(self, x0, y0, x1, y1, z=0, h=96, every=128, tex=None, rails=(0.12, 0.55, 0.97), clip_h=None,
              mesh=True):
        """A steel fence: posts, rails, a see-through mesh (glass), and a player clip up to `clip_h`."""
        t = tex or T("metal_painted_green")
        L = math.hypot(x1 - x0, y1 - y0)
        n = max(1, round(L / every))
        axis = "x" if abs(x1 - x0) >= abs(y1 - y0) else "y"
        for i in range(n + 1):
            px = x0 + (x1 - x0) * i / n
            py = y0 + (y1 - y0) * i / n
            self.post(px, py, z, z + h, 5, t)
        for r in rails:
            rz = z + h * r
            if axis == "x":
                a, b = sorted((x0, x1))
                self.dblk((a, y0 - 1.5, rz - 2), (b, y0 + 1.5, rz + 1), t)
            else:
                a, b = sorted((y0, y1))
                self.dblk((x0 - 1.5, a, rz - 2), (x0 + 1.5, b, rz + 1), t)
        if axis == "x":
            a, b = sorted((x0, x1))
            if mesh:
                self.pane((a, y0 - 0.5, z + 4), (b, y0 + 0.5, z + h - 4))
            self.clip((a, y0 - 4, z), (b, y0 + 4, z + (clip_h or 1024)))
        else:
            a, b = sorted((y0, y1))
            if mesh:
                self.pane((x0 - 0.5, a, z + 4), (x0 + 0.5, b, z + h - 4))
            self.clip((x0 - 4, a, z), (x0 + 4, b, z + (clip_h or 1024)))

    def railing(self, x0, y0, x1, y1, z, h=40, tex=None, every=64):
        t = tex or T("metal_painted_green")
        L = math.hypot(x1 - x0, y1 - y0)
        n = max(1, round(L / every))
        for i in range(n + 1):
            self.post(x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n, z, z + h, 3, t)
        if abs(x1 - x0) >= abs(y1 - y0):
            a, b = sorted((x0, x1))
            self.dblk((a - 2, y0 - 2, z + h - 3), (b + 2, y0 + 2, z + h), {"sides": t, "top": t, "bottom": NODRAW, "+x": NODRAW, "-x": NODRAW})
            self.clip((a - 2, y0 - 2, z), (b + 2, y0 + 2, z + h))
        else:
            a, b = sorted((y0, y1))
            self.dblk((x0 - 2, a - 2, z + h - 3), (x0 + 2, b + 2, z + h), {"sides": t, "top": t, "bottom": NODRAW, "+y": NODRAW, "-y": NODRAW})
            self.clip((x0 - 2, a - 2, z), (x0 + 2, b + 2, z + h))

    def ladder(self, x, y, z0, z1, face="+y", w=24, tex=None):
        """A ladder on a wall whose face is at (x, y) facing `face`: rails, rungs every 12, a func_ladder volume."""
        t = tex or T("metal_painted_green")
        d = {"+x": (1, 0), "-x": (-1, 0), "+y": (0, 1), "-y": (0, -1)}[face]
        if d[0]:
            a, b = sorted((x + d[0] * 2, x + d[0] * 6))
            for yy in (y - w / 2, y + w / 2 - 2):
                self.dblk((a, yy, z0), (b, yy + 2, z1 + 32), t)
            for rz in range(int(z0) + 10, int(z1) + 20, 12):
                self.dblk((a + 1, y - w / 2 + 2, rz), (b - 1, y + w / 2 - 2, rz + 1.5), t)
            va, vb = sorted((x, x + d[0] * 24))
            self.m.volume("func_ladder", (va, y - w / 2, z0), (vb, y + w / 2, z1 + 16))
        else:
            a, b = sorted((y + d[1] * 2, y + d[1] * 6))
            for xx in (x - w / 2, x + w / 2 - 2):
                self.dblk((xx, a, z0), (xx + 2, b, z1 + 32), t)
            for rz in range(int(z0) + 10, int(z1) + 20, 12):
                self.dblk((x - w / 2 + 2, a + 1, rz), (x + w / 2 - 2, b - 1, rz + 1.5), t)
            va, vb = sorted((y, y + d[1] * 24))
            self.m.volume("func_ladder", (x - w / 2, va, z0), (x + w / 2, vb, z1 + 16))

    def bar(self, p0, p1, w=6, tex=TRIM):
        """A straight beam between two points (the hull of two cubes)."""
        h = w / 2
        pts = []
        for p in (p0, p1):
            for dx in (-h, h):
                for dy in (-h, h):
                    for dz in (-h, h):
                        pts.append((p[0] + dx, p[1] + dy, p[2] + dz))
        dv = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2])
        ln = math.sqrt(dv[0] ** 2 + dv[1] ** 2 + dv[2] ** 2) or 1.0
        dv = (dv[0] / ln, dv[1] / ln, dv[2] / ln)
        b = hull(pts, lambda n: NODRAW if abs(n[0] * dv[0] + n[1] * dv[1] + n[2] * dv[2]) > 0.5 else tex)
        self.add(b, deco=True)
        return b

    def tree(self, x, y, z=0, h=360, r=110, trunk=None, leaves=None):
        """A tree: a model when there is one, else a stylised trunk and a crown of two octagonal frusta."""
        name = "tree_small_02" if "tree_small_02" in props_info() else None
        if name:
            size = props_info()[name].get("size", [4.0, 6.0, 4.0])
            self.prop(name, x, y, z, yaw=hash_unit(x, y) * 360, scale=min(1.7, (h / zmap.M) / max(size[1], 0.1)))
            return
        self.add(prism((x, y), 9, z, z + h * 0.45, 4, {"sides": trunk or T("wood_beam"), "top": NODRAW, "bottom": NODRAW}, rot=45), deco=True)
        lv = leaves or T("grass")
        pts = []
        for zz, rr in ((0.35, 0.55), (0.6, 1.0), (0.85, 0.75), (1.0, 0.25)):
            for i in range(8):
                a = math.pi / 8 + 2 * math.pi * i / 8
                pts.append((x + r * rr * math.cos(a), y + r * rr * math.sin(a), z + h * zz))
        lo_pts = [p for p in pts if p[2] <= z + h * 0.6 + 1e-6]
        hi_pts = [p for p in pts if p[2] >= z + h * 0.6 - 1e-6]
        self.add(hull(lo_pts, lv), deco=True)
        self.add(hull(hi_pts, lv), deco=True)

    def tank(self, x, y, z, r, h, tex=None, roof=None, sides=12):
        """An upright storage tank with a conical roof and a top ring. With `container_props` and a tank of the
        modelled one's size (r 130, h 250), the model (arch.py fuel_tank_green) over the same shape as an invisible
        collision."""
        tex = tex or T("metal_painted_green")
        z = lift(z)
        if self.container_props and "fuel_tank_green" in props_info() and abs(r - 130) < 15 and abs(h - 250) < 25:
            self.add(prism((x, y), r, z, z + h, sides, NODRAW, rot=180 / sides))
            self.add(hull([(x + (r + 4) * math.cos(math.pi * 2 * i / sides), y + (r + 4) * math.sin(math.pi * 2 * i / sides), z + h) for i in range(sides)]
                          + [(x, y, z + h + r * 0.25)], NODRAW))
            self.prop("fuel_tank_green", x, y, z, yaw=hash_unit(x, y) * 360, solid=False)
            return
        self.add(prism((x, y), r, z, z + h, sides, {"sides": tex, "top": NODRAW, "bottom": NODRAW}, rot=180 / sides))
        pts = []
        for i in range(sides):
            a = math.pi * 2 * i / sides + math.pi / sides
            pts.append((x + (r + 4) * math.cos(a), y + (r + 4) * math.sin(a), z + h))
        pts.append((x, y, z + h + r * 0.25))
        self.add(hull(pts, roof or T("metal_plate")))
        for hz in (z + h * 0.33, z + h * 0.66):
            self.add(prism((x, y), r + 1.5, hz, hz + 3, sides, TRIM, rot=180 / sides), deco=True)

    def htank(self, x0, x1, y, z, r, tex=None, axis="x", saddles=True):
        """A horizontal tank on saddles (fuel bowser style)."""
        tex = tex or T("metal_painted_green")
        z = lift(z)
        if axis == "x":
            self.add(prism((y, z + 20 + r), r, x0, x1, 12, lambda n: tex if abs(n[0]) < 0.5 else T("metal_plate"), rot=15, axis="x"))
            if saddles:
                for sx in (x0 + (x1 - x0) * 0.2, x1 - (x1 - x0) * 0.2):
                    self.blk((sx - 6, y - r * 0.8, z), (sx + 6, y + r * 0.8, z + 20 + r * 0.6), T("concrete_painted"))
        else:
            self.add(prism((y, z + 20 + r), r, x0, x1, 12, lambda n: tex if abs(n[1]) < 0.5 else T("metal_plate"), rot=15, axis="y"))
            if saddles:
                for sy in (x0 + (x1 - x0) * 0.2, x1 - (x1 - x0) * 0.2):
                    self.blk((y - r * 0.8, sy - 6, z), (y + r * 0.8, sy + 6, z + 20 + r * 0.6), T("concrete_painted"))


def places(m: Map, zones: list[tuple[str, tuple, tuple]]) -> None:
    for name, lo, hi in zones:
        m.volume("zg_place", lo, hi, name=name)


def spawns(m: Map, cls: str, pts: list[tuple[float, float, float]], angle: float) -> None:
    for p in pts:
        m.point(cls, (p[0], p[1], p[2] + 4), angle=angle)


# ---- old-town pieces (used by zg_altstadt, handy elsewhere) --------------------------------------------------------
def frustum(k: Kit, cx, cy, z0, r0, z1, r1, sides, tex, rot=None, deco=False):
    """A tapered prism between two regular polygons (radius r0 at z0, r1 at z1); r1 may be 0 (a pyramid)."""
    rot = (180 / sides) if rot is None else rot
    pts = []
    for z, r in ((z0, r0), (z1, r1)):
        if r <= 0.01:
            pts.append((cx, cy, z))
            continue
        for i in range(sides):
            a = math.radians(rot) + 2 * math.pi * i / sides
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a), z))
    b = hull(pts, tex)
    k.add(b, deco)
    return b


def onion(k: Kit, cx, cy, z, r, h, tex, tip=None):
    """A baroque onion dome: stacked octagonal frusta, a lantern, a small bulb and a spike."""
    prof = [(0.0, 1.0), (0.12, 1.28), (0.28, 1.38), (0.44, 1.18), (0.58, 0.72), (0.68, 0.34), (0.72, 0.28)]
    for (za, ra), (zb, rb) in zip(prof, prof[1:]):
        frustum(k, cx, cy, z + h * za, r * ra, z + h * zb, r * rb, 8, tex)
    frustum(k, cx, cy, z + h * 0.72, r * 0.28, z + h * 0.8, r * 0.28, 8, tip or tex)  # lantern
    frustum(k, cx, cy, z + h * 0.8, r * 0.28, z + h * 0.86, r * 0.42, 8, tex)
    frustum(k, cx, cy, z + h * 0.86, r * 0.42, z + h * 0.94, r * 0.1, 8, tex)
    frustum(k, cx, cy, z + h * 0.94, 2.5, z + h * 1.1, 1.0, 4, tip or TRIM, deco=True)


def arch(k: Kit, axis, u0, u1, t0, t1, z_spring, z_top, tex, n=8, rise=None, deco=False):
    """The solid above a round (or segmental, with `rise`) arch spanning u0..u1, through t0..t1, up to z_top.
    Built from `n` vertical strips whose undersides follow the arc."""
    r = (u1 - u0) / 2
    uc = (u0 + u1) / 2
    rz = r if rise is None else rise

    def zarc(u):
        x = (u - uc) / r
        return z_spring + rz * math.sqrt(max(0.0, 1 - x * x))

    for i in range(n):
        ua = u0 + (u1 - u0) * i / n
        ub = u0 + (u1 - u0) * (i + 1) / n
        za, zb = zarc(ua), zarc(ub)
        if axis == "x":
            pts = [(ua, t0, za), (ub, t0, zb), (ua, t1, za), (ub, t1, zb), (ua, t0, z_top), (ub, t0, z_top), (ua, t1, z_top), (ub, t1, z_top)]
        else:
            pts = [(t0, ua, za), (t0, ub, zb), (t1, ua, za), (t1, ub, zb), (t0, ua, z_top), (t0, ub, z_top), (t1, ua, z_top), (t1, ub, z_top)]
        k.add(hull(pts, tex), deco)


def brace(k: Kit, axis, face, out, u0, v0, u1, v1, w, tex, depth=3):
    """A diagonal timber on a wall face from (u0, v0) to (u1, v1), `w` wide; only its front is drawn."""
    du, dv = u1 - u0, v1 - v0
    ln = math.hypot(du, dv)
    nu, nv = -dv / ln * w / 2, du / ln * w / 2
    quad = [(u0 + nu, v0 + nv), (u0 - nu, v0 - nv), (u1 - nu, v1 - nv), (u1 + nu, v1 + nv)]
    pts = []
    for t in (face, face + out * depth):
        for u, v in quad:
            pts.append((u, t, v) if axis == "x" else (t, u, v))
    ax = 1 if axis == "x" else 0

    def tx(n):
        return tex if n[ax] * out > 0.9 else NODRAW
    k.add(hull(pts, tx), deco=True)


def house(k: Kit, x0, y0, x1, y1, eaves, plaster, sides="nsew", ridge="x", rise=None, roof=None, timber=False,
          floors=None, bays=110, win_w=40, win_h=56, shutters=None, door=None, plinth=None, chimney=True,
          beam=None, frame=None, over=18, sill=None, cornice=None, gable_timber=True, hide="", shops="",
          awning=None, over_end=0, quoins=None, rafters=None):
    """A town house: plaster walls with windows (and shutters), optional half-timbering, a pitched tile roof.
    `quoins` (a texture): dressed stones up the corners of plastered houses; `rafters` (a texture): rafter ends and
    a fascia under the eaves."""
    beam = beam or T("wood_beam")
    roof = variant(roof or T("roof_tiles"), x0, y1)
    plaster = variant(plaster, x0, y0, x1)
    if floors is None:
        floors = [z for z in range(44, int(eaves) - 80, 128)]
    if shops:
        floors = [f for f in floors if f > 100]
        door = None
    width = (y1 - y0) if ridge == "x" else (x1 - x0)
    rise = rise if rise is not None else width * 0.55
    k.solid_building(x0, y0, x1, y1, 0, eaves, plaster, sides=sides, bays=bays, floors=floors, win_w=win_w,
                     win_h=win_h, door=door, plinth=plinth, plinth_h=28, cornice=cornice, cornice_h=10,
                     top=NODRAW, sill=sill or (None if timber else T("plaster_white")), frame=frame,
                     cross="frame" if frame else False, shutters=shutters, door_tex=T("wood_planks"))
    k.gable_roof(x0, y0, x1, y1, eaves, rise, axis=ridge, tex=roof, gable=plaster, over=over, thick=8, hide=hide,
                 over_end=over_end, gable_sides=sides, under=rafters, rafters=rafters)
    if quoins and not timber:
        corner_stones(k, x0, y0, x1, y1, 28 if plinth else 0, eaves, sides, quoins)
    if not timber:
        # A string course at each upper floor and a moulded cornice under the eaves, in light stone: the plain
        # plaster boxes get their storeys.
        course = T("concrete_wall_b")
        spec = {"s": ("x", x0, x1, y0, -1), "n": ("x", x0, x1, y1, 1), "w": ("y", y0, y1, x0, -1), "e": ("y", y0, y1, x1, 1)}
        for side, (axis, a0, a1, face, out) in spec.items():
            if side not in sides:
                continue
            for fz in floors[1:]:
                k.band(axis, a0, a1, face, out, fz - 26, fz - 18, 3, course, show=("front", "top", "bottom"))
            k.band(axis, a0, a1, face, out, eaves - 14, eaves - 6, 4, course, show=("front", "top", "bottom"))
            k.band(axis, a0, a1, face, out, eaves - 6, eaves, 7, course, show=("front", "bottom"))
    if timber:
        spec = {"s": ("x", x0, x1, y0, -1), "n": ("x", x0, x1, y1, 1), "w": ("y", y0, y1, x0, -1),
                "e": ("y", y0, y1, x1, 1)}
        for side, (axis, a0, a1, face, out) in spec.items():
            if side not in sides:
                continue
            n = max(1, int((a1 - a0) // bays))
            span = (a1 - a0) / n
            levels = [0] + [fz - 18 for fz in floors[1:]] + [eaves - 12]
            for lz in levels:
                k.panel(axis, a0, a1, face, out, lz, lz + 12, 3, beam)
            for (za, zb) in zip(levels, levels[1:]):
                for i in range(n + 1):
                    u = a0 + span * i
                    ua, ub = max(a0, u - 6), min(a1, u + 6)
                    k.panel(axis, ua, ub, face, out, za + 12, zb, 3, beam)
                if n >= 2 and za > 0:
                    # braces in the end bays of the upper floors
                    brace(k, axis, face, out, a0 + 6, za + 12, a0 + span * 0.45, zb, 8, beam)
                    brace(k, axis, face, out, a1 - 6, za + 12, a1 - span * 0.45, zb, 8, beam)
            gable_side = (ridge == "x" and axis == "y") or (ridge == "y" and axis == "x")
            if gable_timber and gable_side:
                um = (a0 + a1) / 2
                k.panel(axis, um - 6, um + 6, face, out, eaves, eaves + rise - 20, 3, beam)
                k.panel(axis, a0 + (a1 - a0) * 0.18, a1 - (a1 - a0) * 0.18, face, out, eaves + rise * 0.35,
                        eaves + rise * 0.35 + 10, 3, beam)
    spec2 = {"s": ("x", x0, x1, y0, -1), "n": ("x", x0, x1, y1, 1), "w": ("y", y0, y1, x0, -1), "e": ("y", y0, y1, x1, 1)}
    for side in shops:
        axis, a0, a1, face, out = spec2[side]
        if k.facade_style == "town" and town_shop(k, axis, a0, a1, face, out, awning):
            continue
        # a shop: a door, a wide dark shop window with a frame, a sloping awning above
        k.panel(axis, a0 + 40, a0 + 88, face, out, 0, 100, 1, T("wood_planks"))
        fit(k.panel(axis, a0 + 120, a1 - 40, face, out, 24, 118, 1, GLASS_WIN))
        k.panel(axis, a0 + 114, a1 - 34, face, out, 118, 124, 3, T("concrete_wall"))
        k.panel(axis, a0 + 114, a1 - 34, face, out, 18, 24, 3, T("concrete_wall"))
        aw = awning or T("metal_painted_red")
        pts = []
        for u in (a0 + 110, a1 - 30):
            for (t, z) in ((face, 150), (face + out * 60, 128)):
                for dz in (0, 3):
                    pts.append((u, t, z + dz) if axis == "x" else (t, u, z + dz))
        k.add(hull(pts, lambda n: aw if n[2] > 0.3 or (abs(n[1 if axis == "x" else 0]) > 0.5) else NODRAW), deco=True)
    if chimney:
        cx = x0 + (x1 - x0) * 0.7
        cy = y0 + (y1 - y0) * 0.35
        k.chimney(cx, cy, eaves + rise * 0.5, eaves + rise + 50, 28, T("bricks090"))


def town_shop(k: Kit, axis, a0, a1, face, out, awning=None) -> bool:
    """A shop on a town facade from the modelled pieces (arch.py): the display window (the width variant nearest the
    space), a striped awning over it in the house's awning colour, a single door. False if they aren't built."""
    info = props_info()
    u0, u1 = a0 + 120, a1 - 40
    w_m = (u1 - u0) / zmap.M
    paints = ("green", "brown", "blue", "red")
    paint = paints[int(hash_unit(a0, face) * 4) % 4]
    ww = min((20, 30, 40), key=lambda w: abs(w / 10 - w_m))
    shop = f"shop_town_{ww}_{paint}"
    colour = {"mat/metal_painted_red": "red", "mat/metal_painted_green": "green", "mat/metal_painted_blue": "blue"}.get(awning or "", "ochre")
    aw_u0, aw_u1 = a0 + 110, a1 - 30
    aw_w = min((24, 34, 44), key=lambda w: abs(w / 10 - (aw_u1 - aw_u0) / zmap.M))
    aw = f"awning_{aw_w}_{colour}"
    if shop not in info or aw not in info or "door_town_green" not in info:
        return False
    k.facade_piece(shop, axis, face, out, u0, u1, 24)
    k.facade_piece("door_town_green", axis, face, out, a0 + 40, a0 + 88, 0)
    # The awning: scaled to its span, hung from 150 up the wall.
    ai = info[aw]
    scale = (aw_u1 - aw_u0) / (ai["size"][0] * zmap.M)
    yaw = (90 if out > 0 else 270) if axis == "x" else (0 if out > 0 else 180)
    uc = (aw_u0 + aw_u1) / 2
    t = face + out * (ai["size"][2] * zmap.M * scale / 2 + 0.3)
    x, y = (uc, t) if axis == "x" else (t, uc)
    k.prop(aw, x, y, 150 - ai["anchor"]["top"] * scale * zmap.M, yaw=yaw, scale=round(scale, 4), solid=False)
    # A bakery sign over some doors, café tables out front of some shops (where the square is clear).
    h = hash_unit(a1, face, 3)
    if h < 0.35 and "sign_bakery" in info:
        si = info["sign_bakery"]
        t = face + out * (si["size"][2] * zmap.M / 2 + 0.3)
        uc = a0 + 64
        x, y = (uc, t) if axis == "x" else (t, uc)
        k.prop("sign_bakery", x, y, 150, yaw=yaw, solid=False)
    if h > 0.4:
        sets = [n for n in ("cafe_set_red", "cafe_set_green", "cafe_set_ochre", "cafe_set") if n in info]
        n_sets = 1 if (u1 - u0) < 200 else 2
        for i in range(n_sets):
            uc = u0 + (u1 - u0) * (i + 0.5) / n_sets
            name = sets[int(hash_unit(uc, face) * len(sets)) % len(sets)]
            t = face + out * 110
            x, y = (uc, t) if axis == "x" else (t, uc)
            r = 55
            if not k.blocked((x - r, y - r, 0), (x + r, y + r, 100)):
                k.prop(name, x, y, 0, yaw=hash_unit(x, y) * 360)
        # A chalkboard menu by the door.
        if "standing_chalkboard_01" in info:
            t = face + out * 40
            uc = a0 + 110
            x, y = (uc, t) if axis == "x" else (t, uc)
            if not k.blocked((x - 20, y - 20, 0), (x + 20, y + 20, 60)):
                k.prop("standing_chalkboard_01", x, y, 0, yaw=yaw + 15, solid=False)
    return True


def corner_stones(k: Kit, x0, y0, x1, y1, z0, z1, sides, tex, h=22, long=30, short=18, depth=2.5):
    """Quoins: dressed stones up each corner of a facade in `sides`, long and short in turn, standing a little proud,
    each course a hair apart so the joints read."""
    spec = {"s": ("x", x0, x1, y0, -1), "n": ("x", x0, x1, y1, 1), "w": ("y", y0, y1, x0, -1), "e": ("y", y0, y1, x1, 1)}
    for side, (axis, a0, a1, face, out) in spec.items():
        if side not in sides:
            continue
        n = int((z1 - z0) // h)
        for i in range(n):
            za, zb = z0 + i * h + 0.6, z0 + (i + 1) * h - 0.6
            w = long if i % 2 == 0 else short
            for end, sgn in ((a0, 1), (a1, -1)):
                ua, ub = sorted((end, end + sgn * w))
                k.panel(axis, ua, ub, face, out, za, zb, depth, tex, show=("front", "top", "bottom", "ends"))


def vault(k: Kit, x0, x1, y0, y1, z0, rise, thick, tex, inner=None, n=12, ends=True):
    """A barrel vault shell along y spanning x0..x1 (inside), elliptical, `rise` high inside, `thick` thick.
    Built from vertical strips between the inner and outer arcs (each strip one convex brush)."""
    a = (x1 - x0) / 2
    xc = (x0 + x1) / 2
    A, Bz = a + thick, rise + thick
    inner = inner or tex

    def zin(x):
        t = (x - xc) / a
        return z0 + rise * math.sqrt(max(0.0, 1 - t * t)) if abs(t) < 1 else z0

    def zout(x):
        t = (x - xc) / A
        return z0 + Bz * math.sqrt(max(0.0, 1 - t * t))

    xs = [xc - A + (A - a) * i / 2 for i in range(2)] + [x0 + (x1 - x0) * i / n for i in range(n + 1)] + [x1 + (A - a) * (i + 1) / 2 for i in range(2)]
    xs = sorted(set(round(v, 4) for v in xs))
    for xa, xb in zip(xs, xs[1:]):
        za, zb = zin(xa), zin(xb)
        ta, tb = zout(xa), zout(xb)
        if ta - za < 0.5 and tb - zb < 0.5:
            continue
        pts = []
        for y in (y0, y1):
            pts += [(xa, y, za), (xb, y, zb), (xa, y, ta), (xb, y, tb)]
        k.add(hull(pts, lambda nn: tex if nn[2] > 0.05 and not (abs(nn[1]) > 0.9) else (inner if nn[2] < -0.05 else (tex if abs(nn[1]) > 0.9 else NODRAW))))
    return zin, zout


def vault_wall(k: Kit, x0, x1, y0, y1, zin, holes, tex, n=12):
    """Fills a vault's end (below the inner arc `zin`, between x0..x1, through y0..y1) except rectangular `holes`
    (u0, u1, z_top): each strip is filled from the hole's top (or the floor) up to the arc."""
    cuts = sorted({x0, x1, *[h[0] for h in holes], *[h[1] for h in holes]} | {x0 + (x1 - x0) * i / n for i in range(n + 1)})
    for xa, xb in zip(cuts, cuts[1:]):
        xm = (xa + xb) / 2
        zbot = 0.0
        for u0, u1, zt in holes:
            if u0 <= xm <= u1:
                zbot = zt
        za, zb = zin(xa), zin(xb)
        if min(za, zb) - zbot < 1:
            continue
        pts = []
        for y in (y0, y1):
            pts += [(xa, y, zbot), (xb, y, zbot), (xa, y, za), (xb, y, zb)]
        k.add(hull(pts, tex))
