"""Layout scripts → Valve 220 .map files (TrenchBroom opens them too).

Units are map units (inches), Z up. A 72-unit player is 1.83 m; 39.37 units are a metre.

    m = Map("zg_example")
    m.block((-512, -512, -16), (512, 512, 0), "dev/floor")
    m.point("info_player_ksk", (0, 0, 40), angle=90)
    m.save("assets/maps/zg_example.map")
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field

Vec = tuple[float, float, float]

M = 39.37  # units per metre


def _sub(a: Vec, b: Vec) -> Vec:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _add(a: Vec, b: Vec) -> Vec:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _mul(a: Vec, s: float) -> Vec:
    return (a[0] * s, a[1] * s, a[2] * s)


def _dot(a: Vec, b: Vec) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a: Vec, b: Vec) -> Vec:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _norm(a: Vec) -> Vec:
    n = math.sqrt(_dot(a, a))
    return (a[0] / n, a[1] / n, a[2] / n)


# Fixed-point output instead of %g. Six significant digits shift sloped planes by up to ~0.05 units in large maps,
# which breaks coplanar face hiding. Off by default so existing maps (and their baked lightmaps) stay byte-identical;
# the zg_* layout scripts turn it on.
PRECISE = False

# Texture offsets per plane: every `mat/` face that isn't fitted gets a shift derived from its texture and plane, so
# parallel walls of neighbouring buildings don't show the same stains in the same places. Coplanar faces share the
# shift (a wall split around its windows stays continuous). UVs only: lightmaps are unaffected. Off by default (as
# PRECISE, so older maps stay byte-identical); the zg_* layout scripts turn it on.
VARY = False


def _vary(texture: str, n: Vec, point: Vec) -> tuple[float, float]:
    import zlib

    d = round(_dot(n, point))
    key = f"{texture}|{round(n[0], 3)},{round(n[1], 3)},{round(n[2], 3)}|{d}"
    h = zlib.crc32(key.encode())
    return (float(h % 512), float((h >> 9) % 512))


def _fmt(x: float) -> str:
    r = round(x, 4)
    if r == int(r):
        return str(int(r))
    return f"{r:.4f}".rstrip("0").rstrip(".") if PRECISE else f"{r:g}"


def _axes(n: Vec) -> tuple[Vec, Vec]:
    """Paraxial texture axes for a face normal (world-aligned, like the editor's default)."""
    ax, ay, az = abs(n[0]), abs(n[1]), abs(n[2])
    if az >= ax and az >= ay:
        return (1, 0, 0), (0, -1, 0)
    if ax >= ay:
        return (0, 1, 0), (0, 0, -1)
    return (1, 0, 0), (0, 0, -1)


@dataclass
class Face:
    normal: Vec
    point: Vec
    texture: str
    scale: float = 0.25
    # Optional: a separate vertical scale and texel shifts (u, v), e.g. to fit a texture to a crate's face.
    scale_v: float | None = None
    shift: tuple[float, float] = (0.0, 0.0)

    def line(self) -> str:
        n = _norm(self.normal)
        # Three points whose (p0 − p1) × (p2 − p1) points along n: p0 = p1 + a, p2 = p1 + n × a.
        seed = (0.0, 0.0, 1.0) if abs(n[2]) < 0.9 else (1.0, 0.0, 0.0)
        a = _mul(_norm(_cross(n, seed)), 64)
        b = _cross(n, a)
        p1 = self.point
        pts = (_add(p1, a), p1, _add(p1, b))
        u, v = _axes(n)
        s = " ".join("( " + " ".join(_fmt(c) for c in p) + " )" for p in pts)
        uu = " ".join(_fmt(c) for c in u)
        vv = " ".join(_fmt(c) for c in v)
        sv = self.scale if self.scale_v is None else self.scale_v
        shift = self.shift
        if VARY and self.texture.startswith("mat/") and self.scale_v is None and shift == (0.0, 0.0):
            shift = _vary(self.texture, n, p1)
        su0, sv0 = (_fmt(shift[0]), _fmt(shift[1]))
        return f"{s} {self.texture} [ {uu} {su0} ] [ {vv} {sv0} ] 0 {_fmt(self.scale)} {_fmt(sv)}"


@dataclass
class Brush:
    faces: list[Face]
    # Map-space bounds when known (boxes, hulls), for layout checks such as sightlines.
    bounds: tuple[Vec, Vec] | None = None

    def text(self) -> str:
        return "{\n" + "\n".join(f.line() for f in self.faces) + "\n}\n"


# World size of the material sets: texture scale per `mat/<set>` (one tile spans 512 × scale units). Faces built by
# the helpers below use these unless a scale is passed.
TEX_SCALE: dict[str, float] = {
    # About 0.077 × the set's real-world repeat in metres (see assets/textures/CREDITS.md).
    "mat/concrete_wall": 0.19,
    "mat/concrete_floor": 0.23,
    "mat/concrete_painted": 0.3,
    "mat/plaster_white": 0.17,
    "mat/plaster_ochre": 0.154,
    "mat/plaster_old": 0.17,
    "mat/bricks090": 0.12,
    "mat/brick_dark": 0.19,
    "mat/cobblestone": 0.09,
    "mat/paving": 0.123,
    "mat/asphalt": 0.25,
    "mat/gravel": 0.115,
    "mat/wood_planks": 0.14,
    "mat/wood_beam": 0.12,
    "mat/wood_crate": 0.108,
    "mat/metal_corrugated": 0.1,
    "mat/metal_painted_red": 0.17,
    "mat/metal_painted_blue": 0.17,
    "mat/metal_painted_green": 0.17,
    "mat/metal_plate": 0.07,
    "mat/metal_rust": 0.1,
    "mat/roof_tiles": 0.22,
    "mat/tiles_floor": 0.154,
    "mat/ceiling_tiles": 0.3,
    "mat/dirt": 0.25,
    "mat/grass": 0.11,
    # Variants and trim sets (2026-09-29).
    "mat/concrete_wall_b": 0.21,
    "mat/concrete_wall_c": 0.154,
    "mat/plaster_b": 0.23,
    "mat/plaster_c": 0.11,
    "mat/brick_b": 0.154,
    "mat/asphalt_b": 0.23,
    "mat/concrete_slabs": 0.35,
    "mat/paving_b": 0.154,
    "mat/metal_corrugated_b": 0.086,
    "mat/planks_b": 0.115,
    "mat/roof_b": 0.115,
    "mat/tiles_floor_b": 0.115,
    "mat/dirt_b": 0.154,
    "mat/glass_window": 0.1,
    "mat/window_frame": 0.077,
    "mat/metal_dark": 0.077,
    "mat/rubber": 0.058,
    "mat/door_steel": 0.19,
    "mat/rollershutter": 0.154,
    "mat/vent_grille": 0.077,
    "mat/sign_plate": 0.09,
    "mat/lamp_glass": 0.077,
    "mat/container_red": 0.154,
    "mat/container_blue": 0.154,
    "mat/container_green": 0.154,
    "mat/container_grey": 0.154,
    "mat/container_orange": 0.154,
    # Poly Haven sets for the look pass (2026-09-29): 0.077 × the source's tile in metres.
    "mat/plaster_patched": 0.177,
    "mat/plaster_red": 0.154,
    "mat/plaster_cream": 0.154,
    "mat/plaster_pink": 0.154,
    "mat/plaster_sky": 0.154,
    "mat/plaster_sage": 0.154,
    "mat/plaster_worn": 0.139,
    "mat/plaster_mossy": 0.139,
    "mat/plaster_blue": 0.154,
    "mat/stone_rustic": 0.117,
    "mat/stone_rubble": 0.154,
    "mat/sandstone_bricks": 0.154,
    "mat/stone_blocks_grey": 0.23,
    "mat/wood_door": 0.146,
    "mat/wood_door_b": 0.154,
    "mat/wood_old": 0.154,
    "mat/wood_shutter": 0.077,
    "mat/roof_clay": 0.19,
    "mat/cobble_mossy": 0.154,
    "mat/stone_pavers": 0.154,
    "mat/concrete_pavers": 0.148,
    "mat/metal_rusty_paint": 0.17,
    "mat/metal_corrugated_rust": 0.154,
    "mat/container_sheet": 0.154,
    "mat/metal_grate": 0.0385,
    "mat/factory_wall": 0.23,
    "mat/concrete_dirty": 0.23,
    "mat/concrete_cracked": 0.077,
    "mat/concrete_hangar": 0.154,
    "mat/concrete_precast": 0.122,
    # Ground sets (2026-09-30): height in the ORM blue, laid with a world-space overlay by the level shader.
    "mat/gnd_setts": 0.185,
    "mat/gnd_setts_old": 0.185,
    "mat/gnd_setts_moss": 0.115,
    "mat/gnd_flagstone": 0.139,
    "mat/gnd_dust": 0.308,
    "mat/gnd_mud": 0.154,
    "mat/gnd_asphalt": 0.154,
    "mat/gnd_asphalt_damaged": 0.169,
    "mat/gnd_concrete": 0.162,
    "mat/gnd_concrete_damaged": 0.385,
    "mat/gnd_leaves": 0.154,
}


def tex_scale(name: str, scale: float | None = None) -> float:
    """The texture scale for `name`: `scale` if given, else its TEX_SCALE entry, else 0.25."""
    if scale is not None:
        return scale
    return TEX_SCALE.get(name.lower(), 0.25)


def _pick(tex: str | dict[str, str], side: str) -> str:
    if isinstance(tex, str):
        return tex
    return tex.get(side) or tex.get("sides" if side in ("+x", "-x", "+y", "-y") else side) or tex.get("all", "dev/wall")


def box(lo: Vec, hi: Vec, tex: str | dict[str, str], scale: float | None = None) -> Brush:
    """An axis-aligned block. `tex` is one texture or a dict by side: top, bottom, sides (or ±x/±y).

    Without `scale`, each face uses its texture's TEX_SCALE entry (0.25 for textures not listed)."""
    def f(n: Vec, p: Vec, side: str) -> Face:
        name = _pick(tex, side)
        return Face(n, p, name, tex_scale(name, scale))

    return Brush([
        f((-1, 0, 0), lo, "-x"),
        f((0, -1, 0), lo, "-y"),
        f((0, 0, -1), lo, "bottom"),
        f((1, 0, 0), hi, "+x"),
        f((0, 1, 0), hi, "+y"),
        f((0, 0, 1), hi, "top"),
    ], bounds=(lo, hi))


def fit(b: Brush, tiles: tuple[float, float] = (1.0, 1.0)) -> Brush:
    """Scales and shifts each face's texture so it spans the face `tiles` times (a crate texture filling each side).

    Works on axis-aligned faces of a brush with known bounds."""
    assert b.bounds is not None
    lo, hi = b.bounds
    corners = [(x, y, z) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]
    for f in b.faces:
        n = _norm(f.normal)
        u, v = _axes(n)
        on = [c for c in corners if abs(_dot(n, _sub(c, f.point))) < 0.01]
        if len(on) < 3:
            continue
        us = [_dot(c, u) for c in on]
        vs = [_dot(c, v) for c in on]
        su = (max(us) - min(us)) / (512 * tiles[0])
        sv = (max(vs) - min(vs)) / (512 * tiles[1])
        if su < 1e-6 or sv < 1e-6:
            continue
        f.scale, f.scale_v = su, sv
        f.shift = (-min(us) / su, -min(vs) / sv)
    return b


def _hull_tex(tex, n: Vec) -> str:
    if callable(tex):
        return tex(n)
    if isinstance(tex, str):
        return tex
    if n[2] > 0.3:
        return tex.get("top") or tex.get("all", "dev/wall")
    if n[2] < -0.3:
        return tex.get("bottom") or tex.get("all", "dev/wall")
    ax = 0 if abs(n[0]) >= abs(n[1]) else 1
    key = ("+" if n[ax] > 0 else "-") + "xy"[ax]
    return tex.get(key) or tex.get("sides") or tex.get("all", "dev/wall")


def hull(points: list[Vec], tex, scale: float | None = None) -> Brush:
    """The convex hull of `points` as one brush. `tex`: a texture, a dict (top/bottom for faces tilted more than
    ~17° from vertical, sides or ±x/±y for the rest) or a function of the face normal."""
    pts: list[Vec] = []
    for p in points:
        if not any(abs(p[0] - q[0]) + abs(p[1] - q[1]) + abs(p[2] - q[2]) < 1e-6 for q in pts):
            pts.append(tuple(float(c) for c in p))
    planes: list[tuple[Vec, float, Vec]] = []
    n_pts = len(pts)
    for i in range(n_pts):
        for j in range(i + 1, n_pts):
            for k in range(j + 1, n_pts):
                c = _cross(_sub(pts[j], pts[i]), _sub(pts[k], pts[i]))
                if math.sqrt(_dot(c, c)) < 1e-6:
                    continue
                nrm = _norm(c)
                d = _dot(nrm, pts[i])
                dist = [_dot(nrm, p) - d for p in pts]
                if all(x <= 1e-4 for x in dist):
                    pass
                elif all(x >= -1e-4 for x in dist):
                    nrm, d = _mul(nrm, -1), -d
                else:
                    continue
                if any(_dot(nrm, q[0]) > 1 - 1e-7 and abs(d - q[1]) < 1e-4 for q in planes):
                    continue
                planes.append((nrm, d, pts[i]))
    lo = tuple(min(p[a] for p in pts) for a in range(3))
    hi = tuple(max(p[a] for p in pts) for a in range(3))
    # Largest faces first: the compiler splits other faces by this brush's planes in order, and a big face's plane is
    # the likeliest to separate a neighbouring face at once (fewer, larger lightmap charts).
    planes.sort(key=lambda q: -_plane_area(q[0], q[1], pts))
    faces = []
    for nrm, _, p in planes:
        name = _hull_tex(tex, nrm)
        faces.append(Face(nrm, p, name, tex_scale(name, scale)))
    return Brush(faces, bounds=(lo, hi))


def _plane_area(n: Vec, d: float, pts: list[Vec]) -> float:
    """Area of the convex polygon formed by the points lying on plane n·p = d."""
    on = [p for p in pts if abs(_dot(n, p) - d) < 1e-3]
    if len(on) < 3:
        return 0.0
    u = _norm(_cross(n, (0.0, 0.0, 1.0) if abs(n[2]) < 0.9 else (1.0, 0.0, 0.0)))
    v = _cross(n, u)
    q = sorted({(round(_dot(p, u), 6), round(_dot(p, v), 6)) for p in on})
    if len(q) < 3:
        return 0.0

    def half(seq):
        h: list = []
        for p in seq:
            while len(h) >= 2 and (h[-1][0] - h[-2][0]) * (p[1] - h[-2][1]) - (h[-1][1] - h[-2][1]) * (p[0] - h[-2][0]) <= 0:
                h.pop()
            h.append(p)
        return h

    ring = half(q)[:-1] + half(q[::-1])[:-1]
    return abs(sum(ring[i][0] * ring[(i + 1) % len(ring)][1] - ring[(i + 1) % len(ring)][0] * ring[i][1] for i in range(len(ring)))) / 2


def prism(center: tuple[float, float], radius: float, z0: float, z1: float, sides: int, tex,
          rot: float = 0.0, axis: str = "z", scale: float | None = None) -> Brush:
    """A regular prism (one convex brush): `sides` faces around `center`, `radius` to the corners, from z0 to z1 along
    `axis`. For axis "x" the center is (y, z) and z0..z1 run along x; for "y" it is (x, z) along y.
    `rot` (degrees) turns the polygon; 0 puts a corner on the first in-plane axis."""
    ring = []
    for i in range(sides):
        a = math.radians(rot) + 2 * math.pi * i / sides
        ring.append((center[0] + radius * math.cos(a), center[1] + radius * math.sin(a)))
    pts: list[Vec] = []
    for c0, c1 in ring:
        for t in (z0, z1):
            if axis == "z":
                pts.append((c0, c1, t))
            elif axis == "x":
                pts.append((t, c0, c1))
            else:
                pts.append((c0, t, c1))
    return hull(pts, tex, scale)


def ramp(lo: Vec, hi: Vec, rise: str, tex: str, scale: float = 0.25) -> Brush:
    """A wedge filling the box lo..hi whose top slopes up towards `rise` (+x, -x, +y or -y)."""
    (x0, y0, z0), (x1, y1, z1) = lo, hi
    axis = {"+x": 0, "-x": 0, "+y": 1, "-y": 1}[rise]
    up = rise[0] == "+"
    length = (x1 - x0) if axis == 0 else (y1 - y0)
    h = z1 - z0
    # Slope normal: tilted back against the rising direction.
    d = [0.0, 0.0, 0.0]
    d[axis] = 1.0 if up else -1.0
    n = _norm((-d[0] * h, -d[1] * h, length))
    low_edge = [x0, y0, z0]
    if not up:
        low_edge[axis] = x1 if axis == 0 else y1
    faces = [Face((0, 0, -1), lo, tex, scale), Face(n, tuple(low_edge), tex, scale)]
    # The tall end and the two sides.
    tall = [0.0, 0.0, 0.0]
    tall[axis] = 1.0 if up else -1.0
    faces.append(Face(tuple(tall), hi if up else lo, tex, scale))
    other = 1 - axis
    s = [0.0, 0.0, 0.0]
    s[other] = 1.0
    faces.append(Face(tuple(s), hi, tex, scale))
    s[other] = -1.0
    faces.append(Face(tuple(s), lo, tex, scale))
    return Brush(faces)


def stairs(lo: Vec, hi: Vec, rise: str, steps: int, tex: str | dict[str, str], clip: bool = True) -> list[Brush]:
    """Steps filling lo..hi rising towards `rise`, plus a player-clip ramp over their nosings so walking up is smooth."""
    (x0, y0, z0), (x1, y1, z1) = lo, hi
    axis = 0 if rise[1] == "x" else 1
    up = rise[0] == "+"
    out: list[Brush] = []
    span = (x1 - x0) if axis == 0 else (y1 - y0)
    for i in range(steps):
        top = z0 + (z1 - z0) * (i + 1) / steps
        start = span * i / steps if up else 0.0
        end = span if up else span * (steps - i) / steps
        if axis == 0:
            out.append(box((x0 + start, y0, z0), (x0 + end, y1, top), tex))
        else:
            out.append(box((x0, y0 + start, z0), (x1, y0 + end, top), tex))
    if clip:
        # The ramp through the nosings starts one tread before the stairs and reaches the top at the last nosing.
        tread = span / steps
        shift = -tread if up else tread
        if axis == 0:
            out.append(ramp((x0 + shift, y0, z0), (x1 + shift, y1, z1), rise, "tools/clip"))
        else:
            out.append(ramp((x0, y0 + shift, z0), (x1, y1 + shift, z1), rise, "tools/clip"))
    return out


@dataclass
class Entity:
    classname: str
    props: dict[str, str] = field(default_factory=dict)
    brushes: list[Brush] = field(default_factory=list)

    def text(self) -> str:
        kv = "".join(f'"{k}" "{v}"\n' for k, v in {"classname": self.classname, **self.props}.items())
        return "{\n" + kv + "".join(b.text() for b in self.brushes) + "}\n"


class Map:
    def __init__(self, name: str, **props: str):
        self.name = name
        self.world = Entity("worldspawn", {"mapversion": "220", "message": name, **props})
        self.entities: list[Entity] = []

    def add(self, *brushes: Brush | list[Brush]) -> None:
        for b in brushes:
            self.world.brushes.extend(b if isinstance(b, list) else [b])

    def block(self, lo: Vec, hi: Vec, tex: str | dict[str, str] = "dev/wall", scale: float | None = None) -> None:
        self.add(box(lo, hi, tex, scale))

    def room(self, lo: Vec, hi: Vec, wall: float = 16, tex: str | dict[str, str] = "dev/wall", floor: str = "dev/floor",
             ceiling: str | None = "dev/ceiling") -> None:
        """A closed room's shell: floor, walls and (optionally) a ceiling, `wall` thick, inside lo..hi."""
        (x0, y0, z0), (x1, y1, z1) = lo, hi
        w = wall
        self.block((x0 - w, y0 - w, z0 - w), (x1 + w, y1 + w, z0), floor)
        if ceiling:
            self.block((x0 - w, y0 - w, z1), (x1 + w, y1 + w, z1 + w), ceiling)
        self.block((x0 - w, y0 - w, z0), (x0, y1 + w, z1), tex)
        self.block((x1, y0 - w, z0), (x1 + w, y1 + w, z1), tex)
        self.block((x0, y0 - w, z0), (x1, y0, z1), tex)
        self.block((x0, y1, z0), (x1, y1 + w, z1), tex)

    def brush_entity(self, classname: str, *brushes: Brush, **props: str) -> None:
        self.entities.append(Entity(classname, {k: str(v) for k, v in props.items()}, list(brushes)))

    def volume(self, classname: str, lo: Vec, hi: Vec, **props: str) -> None:
        self.brush_entity(classname, box(lo, hi, "tools/trigger"), **props)

    def point(self, classname: str, origin: Vec, angle: float | None = None, **props: str) -> None:
        p = {"origin": " ".join(_fmt(c) for c in origin)}
        if angle is not None:
            p["angle"] = _fmt(angle)
        p.update({k: str(v) for k, v in props.items()})
        self.entities.append(Entity(classname, p))

    def text(self) -> str:
        return "// Game: ZUGRIFF\n// Format: Valve\n" + self.world.text() + "".join(e.text() for e in self.entities)

    def save(self, path: str) -> None:
        # A last pass over the brushes before writing (the kit sets one: showing hidden faces that can be seen).
        hook = getattr(self, "before_save", None)
        if hook:
            hook()
        with open(path, "w") as f:
            f.write(self.text())
        n = len(self.world.brushes) + sum(len(e.brushes) for e in self.entities)
        print(f"wrote {path}: {n} brushes, {len(self.entities)} entities")
