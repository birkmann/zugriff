"""Architectural dressing for the ZUGRIFF maps, modelled here in Blender (not downloaded): window assemblies with
stone surrounds, sills and louvred shutters, flower boxes, panelled doors, a wrought-iron balcony. Each is built from
simple solids with small bevels (so edges catch the light), shaded with Poly Haven's CC0 texture sets (the ones in
`tools/textures/.cache/ph`, box-projected in world scale) plus paint, wear and grime made from noise, then baked into
one material (`build._rebake`) and written like the downloaded props: `assets/models/props/<name>.glb` with a
lightmap UV set, and an entry in `props.json`.

A wall-mounted piece stands with its back on the wall plane (Blender y = 0, its front towards -y, which the export
turns to +Z) and the bottom of its bounding box at z = 0. `anchor` in its entry says where the opening it dresses
sits: `open` = [width, height] and `open_z` (the opening's bottom above the model's bottom), in metres, so the map
kit can scale and place it over a window or door of any size.

    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/arch.py [name ...]
"""

import json
import math
import sys
from pathlib import Path

import bpy  # (first: bmesh only exists once bpy is loaded)
import bmesh  # noqa: E402
from mathutils import Matrix, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import build  # noqa: E402  (the prop pipeline: rebake, lightmap UVs, export, checks)

PH = HERE.parent / "textures" / ".cache" / "ph"
ACG = HERE.parent / "textures" / ".cache" / "acg"


# ---- materials -------------------------------------------------------------------------------------------------


def _ph(set_id: str, kind: str) -> Path | None:
    """A map of a Poly Haven set (by id) or, for an id like `acg:Concrete030`, of an ambientCG one."""
    if set_id.startswith("acg:"):
        key = {"diff": "_Color", "nor": "_NormalGL", "rough": "_Roughness", "ao": "_AmbientOcclusion"}[kind]
        found = sorted((ACG / set_id[4:]).glob(f"*{key}.jpg"))
        return found[0] if found else None
    for f in sorted((PH / set_id).glob("*")):
        n = f.name.lower()
        if kind == "diff" and "_diff_" in n:
            return f
        if kind == "nor" and "_nor_gl_" in n:
            return f
        if kind == "rough" and "_rough_2k" in n:
            return f
        if kind == "ao" and "_ao_" in n:
            return f
    return None


def _image(path: Path, data: bool):
    im = bpy.data.images.load(str(path), check_existing=True)
    im.colorspace_settings.name = "Non-Color" if data else "sRGB"
    return im


def _box_tex(nt, path: Path, data: bool, tile: float, offset=(0.0, 0.0, 0.0)):
    """An image read by box projection in object space, one repeat per `tile` metres."""
    coord = nt.nodes.new("ShaderNodeTexCoord")
    mapping = nt.nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (1 / tile, 1 / tile, 1 / tile)
    mapping.inputs["Location"].default_value = offset
    nt.links.new(coord.outputs["Object"], mapping.inputs["Vector"])
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = _image(path, data)
    tex.projection = "BOX"
    tex.projection_blend = 0.25
    nt.links.new(mapping.outputs["Vector"], tex.inputs["Vector"])
    return tex


def _noise(nt, scale: float, detail: float = 4.0, rough: float = 0.6):
    coord = nt.nodes.new("ShaderNodeTexCoord")
    n = nt.nodes.new("ShaderNodeTexNoise")
    n.inputs["Scale"].default_value = scale
    n.inputs["Detail"].default_value = detail
    n.inputs["Roughness"].default_value = rough
    nt.links.new(coord.outputs["Object"], n.inputs["Vector"])
    return n


def _ramp(nt, src, a: float, b: float):
    """`src` (a float socket) mapped from a..b to 0..1, clamped."""
    m = nt.nodes.new("ShaderNodeMapRange")
    m.inputs["From Min"].default_value = a
    m.inputs["From Max"].default_value = b
    m.clamp = True
    nt.links.new(src, m.inputs["Value"])
    return m.outputs["Result"]


def _mix(nt, fac, a, b):
    """Colour mix: `a` where fac is 0, `b` where 1. a/b: sockets or RGB tuples."""
    m = nt.nodes.new("ShaderNodeMix")
    m.data_type = "RGBA"
    nt.links.new(fac, m.inputs["Factor"])
    for sock, v in ((m.inputs[6], a), (m.inputs[7], b)):
        if isinstance(v, tuple):
            sock.default_value = (*v, 1.0)
        else:
            nt.links.new(v, sock)
    return m.outputs[2]


def _multiply(nt, a, b):
    m = nt.nodes.new("ShaderNodeMix")
    m.data_type = "RGBA"
    m.blend_type = "MULTIPLY"
    m.inputs["Factor"].default_value = 1.0
    for sock, v in ((m.inputs[6], a), (m.inputs[7], b)):
        if isinstance(v, tuple):
            sock.default_value = (*v, 1.0)
        else:
            nt.links.new(v, sock)
    return m.outputs[2]


def _new_mat(name: str):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    return mat, mat.node_tree, build._principled(mat)


def _grime(nt, colour, height: float, strength: float = 0.35):
    """Darkens `colour` towards the bottom of the piece (dirt splashes and damp) and in noisy patches."""
    coord = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Object"], sep.inputs["Vector"])
    low = _ramp(nt, sep.outputs["Z"], height, 0.0)
    patches = _ramp(nt, _noise(nt, 3.0, 6.0).outputs["Fac"], 0.5, 0.75)
    add = nt.nodes.new("ShaderNodeMath")
    add.operation = "MAXIMUM"
    nt.links.new(low, add.inputs[0])
    mul = nt.nodes.new("ShaderNodeMath")
    mul.operation = "MULTIPLY"
    nt.links.new(patches, mul.inputs[0])
    mul.inputs[1].default_value = 0.5
    nt.links.new(mul.outputs[0], add.inputs[1])
    k = nt.nodes.new("ShaderNodeMath")
    k.operation = "MULTIPLY"
    nt.links.new(add.outputs[0], k.inputs[0])
    k.inputs[1].default_value = strength
    return _mix(nt, k.outputs[0], colour, (0.05, 0.045, 0.035))


def textured(name: str, set_id: str, tile: float, tint=(1.0, 1.0, 1.0), rough_mul: float = 1.0, normal: float = 1.0,
             grime: float = 0.0, height: float = 1.0):
    """A Poly Haven set in world scale, tinted, with optional grime towards the bottom."""
    mat, nt, p = _new_mat(name)
    col = _box_tex(nt, _ph(set_id, "diff"), False, tile).outputs["Color"]
    col = _multiply(nt, col, tint)
    if grime:
        col = _grime(nt, col, height, grime)
    nt.links.new(col, p.inputs["Base Color"])
    r = _box_tex(nt, _ph(set_id, "rough"), True, tile)
    rm = nt.nodes.new("ShaderNodeMath")
    rm.operation = "MULTIPLY"
    rm.use_clamp = True
    nt.links.new(r.outputs["Color"], rm.inputs[0])
    rm.inputs[1].default_value = rough_mul
    nt.links.new(rm.outputs[0], p.inputs["Roughness"])
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nm.inputs["Strength"].default_value = normal
    nt.links.new(_box_tex(nt, _ph(set_id, "nor"), True, tile).outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], p.inputs["Normal"])
    p.inputs["Metallic"].default_value = 0.0
    return mat


def painted_wood(name: str, colour, wear: float = 0.15, grain: str = "wood_shutter", tile: float = 0.8,
                 rough: float = 0.55, grime: float = 0.3, height: float = 1.5):
    """Paint over wood: the paint's colour with a faint mottle, bare weathered wood where it has flaked (noise above
    a threshold), the grain showing through the normal map, grime towards the bottom."""
    mat, nt, p = _new_mat(name)
    # Where the paint has gone the wood is grey and weathered, not freshly orange.
    wood = _multiply(nt, _box_tex(nt, _ph(grain, "diff"), False, tile).outputs["Color"], (0.55, 0.5, 0.45))
    mottle = _noise(nt, 18.0, 3.0)
    paint = _mix(nt, _ramp(nt, mottle.outputs["Fac"], 0.3, 0.7), tuple(c * 0.85 for c in colour), tuple(min(1.0, c * 1.12) for c in colour))
    flake = _ramp(nt, _noise(nt, 9.0, 8.0, 0.7).outputs["Fac"], 0.8 - wear * 0.6, 0.82 - wear * 0.6)
    col = _mix(nt, flake, paint, wood)
    if grime:
        col = _grime(nt, col, height, grime)
    nt.links.new(col, p.inputs["Base Color"])
    rmix = nt.nodes.new("ShaderNodeMix")
    rmix.data_type = "FLOAT"
    nt.links.new(flake, rmix.inputs["Factor"])
    rmix.inputs[2].default_value = rough
    rmix.inputs[3].default_value = 0.85
    nt.links.new(rmix.outputs[0], p.inputs["Roughness"])
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nm.inputs["Strength"].default_value = 0.6
    nt.links.new(_box_tex(nt, _ph(grain, "nor"), True, tile).outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], p.inputs["Normal"])
    return mat


def plain(name: str, colour, rough: float = 0.6, metal: float = 0.0, mottle: float = 0.1, grime: float = 0.0,
          height: float = 1.0):
    """A flat colour with a slight mottle (paint, iron, terracotta, leaves)."""
    mat, nt, p = _new_mat(name)
    n = _noise(nt, 25.0, 3.0)
    col = _mix(nt, n.outputs["Fac"], tuple(c * (1 - mottle) for c in colour), tuple(min(1.0, c * (1 + mottle)) for c in colour))
    if grime:
        col = _grime(nt, col, height, grime)
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = rough
    p.inputs["Metallic"].default_value = metal
    return mat


def glass(name: str, curtain: bool):
    """Window glass as seen from a sunny street: no room is modelled behind it, so its colour carries the look of a
    reflection (the sky's pale blue above, the dark room and the facades opposite below) and, maybe, a net curtain;
    it is glossy, so the sun's highlight still slides across it live."""
    mat, nt, p = _new_mat(name)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Generated"], sep.inputs["Vector"])
    # Generated Z runs 0..1 up each pane (each pane is its own object until the join).
    up = _ramp(nt, sep.outputs["Z"], 0.25, 1.0)
    streak = _ramp(nt, _noise(nt, 2.5, 2.0).outputs["Fac"], 0.35, 0.8)
    sky = _mix(nt, streak, (0.08, 0.1, 0.13), (0.22, 0.27, 0.33))
    col = _mix(nt, up, (0.025, 0.028, 0.03), sky)
    if curtain:
        # A pale net curtain drawn behind the lower two thirds.
        cur = _ramp(nt, sep.outputs["Z"], 0.72, 0.66)
        folds = nt.nodes.new("ShaderNodeTexWave")
        folds.inputs["Scale"].default_value = 6.0
        folds.inputs["Distortion"].default_value = 1.5
        nt.links.new(coord.outputs["Generated"], folds.inputs["Vector"])
        # (seen through the glass: dimmed, and the reflection's darks still over it)
        cloth = _mix(nt, _ramp(nt, folds.outputs["Fac"], 0.0, 1.0), (0.1, 0.1, 0.09), (0.19, 0.18, 0.16))
        col = _mix(nt, _ramp(nt, cur, 0.0, 1.6), col, cloth)
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.08
    p.inputs["Metallic"].default_value = 0.0
    return mat


# ---- geometry --------------------------------------------------------------------------------------------------


def block(name: str, lo, hi, mat, bevel: float = 0.006, segments: int = 1):
    """A box from `lo` to `hi` (metres, Blender axes) with its edges bevelled by `bevel`."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    sx, sy, sz = (hi[i] - lo[i] for i in range(3))
    bmesh.ops.scale(bm, vec=(sx, sy, sz), verts=bm.verts)
    bmesh.ops.translate(bm, vec=Vector(((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2)), verts=bm.verts)
    b = min(bevel, 0.45 * min(sx, sy, sz))
    # (Bevels of a few millimetres on thin bars don't show, but where bars meet they left pinholes the bake turned
    # black.)
    if b > 0.0045:
        bmesh.ops.bevel(bm, geom=list(bm.edges), offset=b, segments=segments, affect="EDGES", profile=0.5)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


def rotated(ob, axis: str, degrees: float, pivot):
    """Turns `ob`'s mesh about an axis through `pivot`."""
    m = Matrix.Translation(Vector(pivot)) @ Matrix.Rotation(math.radians(degrees), 4, axis) @ Matrix.Translation(-Vector(pivot))
    ob.data.transform(m)
    return ob


def cylinder(name: str, centre, r: float, h: float, mat, axis: str = "Z", verts: int = 12):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=verts, radius1=r, radius2=r, depth=h)
    if axis == "X":
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.radians(90), 3, "Y"))
    elif axis == "Y":
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.radians(90), 3, "X"))
    bmesh.ops.translate(bm, vec=Vector(centre), verts=bm.verts)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


def blob(name: str, centre, r: float, mat, subdiv: int = 1, squash: float = 1.0):
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=subdiv, radius=r)
    bmesh.ops.scale(bm, vec=(1, 1, squash), verts=bm.verts)
    bmesh.ops.translate(bm, vec=Vector(centre), verts=bm.verts)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


class Rng:
    def __init__(self, seed: int):
        self.s = seed * 2654435761 % 2**32 or 1

    def __call__(self) -> float:
        self.s ^= (self.s << 13) & 0xFFFFFFFF
        self.s ^= self.s >> 17
        self.s ^= (self.s << 5) & 0xFFFFFFFF
        return (self.s & 0xFFFFFF) / 0x1000000

    def range(self, a, b):
        return a + (b - a) * self()


# ---- pieces ------------------------------------------------------------------------------------------------------

SHUTTER_COLOURS = {
    "green": (0.045, 0.1, 0.055),
    "brown": (0.12, 0.055, 0.025),
    "blue": (0.1, 0.15, 0.19),
    "grey": (0.3, 0.3, 0.27),
    "red": (0.24, 0.04, 0.03),
}


def shutter(name: str, x0: float, x1: float, z0: float, z1: float, y: float, mat, slats: bool = True):
    """A louvred shutter (stiles, rails and angled slats) lying open against the wall, from x0..x1, its back at
    y (towards -y)."""
    t = 0.035
    parts = []
    w = 0.05
    parts.append(block(name + "_s0", (x0, y - t, z0), (x0 + w, y, z1), mat, 0.004))
    parts.append(block(name + "_s1", (x1 - w, y - t, z0), (x1, y, z1), mat, 0.004))
    for za, zb in ((z0, z0 + 0.07), (z1 - 0.07, z1), ((z0 + z1) / 2 - 0.035, (z0 + z1) / 2 + 0.035)):
        parts.append(block(name + "_r", (x0 + w, y - t, za), (x1 - w, y, zb), mat, 0.004))
    if slats:
        mid = (z0 + z1) / 2
        for za, zb in ((z0 + 0.07, mid - 0.035), (mid + 0.035, z1 - 0.07)):
            n = max(3, int((zb - za) / 0.055))
            step = (zb - za) / n
            for i in range(n):
                zc = za + step * (i + 0.5)
                s = block(name + "_l", (x0 + w - 0.005, y - t * 0.8, zc - 0.004), (x1 - w + 0.005, y - t * 0.2, zc + step * 0.55), mat, 0.002)
                rotated(s, "X", -35, (0, y - t / 2, zc))
                parts.append(s)
    else:
        parts.append(block(name + "_p", (x0 + w, y - t * 0.6, z0 + 0.07), (x1 - w, y - 0.005, z1 - 0.07), mat, 0.003))
    return parts


def flower_box(name: str, x0: float, x1: float, z: float, y: float, rng: Rng, box_mat, leaf_mats, bloom_mats):
    """A planter on brackets under a sill, overflowing with geraniums (leaf blobs and small bright heads)."""
    parts = [block(name + "_box", (x0, y - 0.2, z - 0.18), (x1, y - 0.02, z), box_mat, 0.008)]
    n = int((x1 - x0) / 0.06)
    for i in range(n):
        x = x0 + 0.04 + (x1 - x0 - 0.08) * (i + rng()) / n
        yy = y - rng.range(0.05, 0.19)
        parts.append(blob(name + "_leaf", (x, yy, z + rng.range(0.0, 0.07)), rng.range(0.045, 0.075), leaf_mats[int(rng() * len(leaf_mats))], 1, 0.7))
    for i in range(int(n * 0.9)):
        x = x0 + 0.04 + (x1 - x0 - 0.08) * rng()
        yy = y - rng.range(0.04, 0.2)
        parts.append(blob(name + "_bloom", (x, yy, z + rng.range(0.06, 0.14)), rng.range(0.022, 0.035), bloom_mats[int(rng() * len(bloom_mats))], 1, 0.9))
    # A few trailing stems over the front.
    for i in range(3):
        x = x0 + (x1 - x0) * rng()
        parts.append(blob(name + "_trail", (x, y - 0.21, z - rng.range(0.05, 0.14)), rng.range(0.03, 0.045), leaf_mats[0], 1, 1.3))
    return parts


def window_town(name: str, shutters: str | None = None, flowers: bool = False, pediment: bool = False, curtain: bool = True,
                surround: str = "light", seed: int = 1, W: float = 1.02, H: float = 1.47):
    """A town-house window over a W × H opening (its bottom at z = 0 before the model is set on its bottom): a stone
    surround, a sill, a painted two-casement frame with glazing bars, glass, open louvred shutters, maybe a flower
    box and a pediment."""
    rng = Rng(seed)
    parts = []
    # Dressed limestone (a smooth concrete set, warmed): light grey-beige, or a warmer sandstone.
    stone = textured("stone", "acg:Concrete030", 1.6, tint=(1.12, 1.05, 0.92) if surround == "light" else (1.1, 0.95, 0.75),
                     grime=0.18, height=0.3)
    sill_mat = textured("sill", "acg:Concrete030", 1.2, tint=(1.05, 0.99, 0.88), grime=0.15, height=0.1)
    frame = plain("frame", (0.72, 0.7, 0.64), rough=0.45, mottle=0.06, grime=0.2, height=0.3)
    s = 0.14  # surround width
    d = 0.06  # how far it stands out
    # Surround: jambs, head, and the sill below (standing out further, wider).
    parts.append(block("jamb_l", (-W / 2 - s, -d, 0), (-W / 2, 0, H + s), stone, 0.01, 2))
    parts.append(block("jamb_r", (W / 2, -d, 0), (W / 2 + s, 0, H + s), stone, 0.01, 2))
    parts.append(block("head", (-W / 2, -d, H), (W / 2, 0, H + s), stone, 0.01, 2))
    # A keystone in the head.
    parts.append(block("key", (-0.07, -d - 0.02, H - 0.01), (0.07, 0, H + s + 0.03), stone, 0.008, 2))
    parts.append(block("sill", (-W / 2 - s - 0.04, -0.12, -0.07), (W / 2 + s + 0.04, 0, 0), sill_mat, 0.012, 2))
    parts.append(block("sill_drip", (-W / 2 - s - 0.02, -0.1, -0.1), (W / 2 + s + 0.02, 0, -0.07), sill_mat, 0.005))
    if pediment:
        cor = block("cornice", (-W / 2 - s - 0.06, -d - 0.07, H + s), (W / 2 + s + 0.06, 0, H + s + 0.07), stone, 0.012, 2)
        parts.append(cor)
        parts.append(block("cornice_top", (-W / 2 - s - 0.03, -d - 0.04, H + s + 0.07), (W / 2 + s + 0.03, 0, H + s + 0.1), stone, 0.01))
    # The frame, set back inside the surround, and the casements.
    fy = -0.025
    ft = 0.05
    parts.append(block("fr_l", (-W / 2, fy - 0.03, 0), (-W / 2 + ft, 0, H), frame, 0.004))
    parts.append(block("fr_r", (W / 2 - ft, fy - 0.03, 0), (W / 2, 0, H), frame, 0.004))
    parts.append(block("fr_t", (-W / 2, fy - 0.03, H - ft), (W / 2, 0, H), frame, 0.004))
    parts.append(block("fr_b", (-W / 2, fy - 0.035, 0), (W / 2, 0, 0.06), frame, 0.004))
    # Transom and the casements' meeting stiles.
    tz = H * 0.72
    parts.append(block("transom", (-W / 2, fy - 0.035, tz - 0.03), (W / 2, 0, tz + 0.03), frame, 0.004))
    parts.append(block("mull", (-0.03, fy - 0.035, 0.06), (0.03, 0, tz), frame, 0.004))
    # Glazing bars across each casement.
    for side in (-1, 1):
        cx0, cx1 = (-W / 2 + ft, -0.03) if side < 0 else (0.03, W / 2 - ft)
        for k in (1, 2):
            zc = 0.06 + (tz - 0.06) * k / 3
            parts.append(block("bar", (cx0, fy - 0.02, zc - 0.012), (cx1, fy + 0.005, zc + 0.012), frame, 0.003))
        # Casement frames (a little proud of the glass).
        parts.append(block("cs_l", (cx0, fy - 0.025, 0.06), (cx0 + 0.035, fy + 0.005, tz - 0.03), frame, 0.003))
        parts.append(block("cs_r", (cx1 - 0.035, fy - 0.025, 0.06), (cx1, fy + 0.005, tz - 0.03), frame, 0.003))
    # Glass: one pane per casement and the transom light, each its own object (the shader grades each up its height).
    g = glass("glass_c" if curtain else "glass", curtain)
    g2 = glass("glass_t", False)
    parts.append(block("pane_l", (-W / 2 + ft, fy + 0.005, 0.06), (-0.03, fy + 0.01, tz - 0.03), g, 0.0))
    parts.append(block("pane_r", (0.03, fy + 0.005, 0.06), (W / 2 - ft, fy + 0.01, tz - 0.03), g, 0.0))
    parts.append(block("pane_t", (-W / 2 + ft, fy + 0.005, tz + 0.03), (W / 2 - ft, fy + 0.01, H - ft), g2, 0.0))
    # A dark reveal inside the frame (the wall's thickness) so the glass reads as set back.
    rev = plain("reveal", (0.34, 0.32, 0.29), rough=0.9)
    parts.append(block("rev", (-W / 2, fy + 0.01, 0), (W / 2, 0, H), rev, 0.0))
    if shutters:
        sh = painted_wood("shutter_" + shutters, SHUTTER_COLOURS[shutters], wear=rng.range(0.08, 0.2))
        sw = W / 2 + 0.02
        parts += shutter("sh_l", -W / 2 - s - sw, -W / 2 - s - 0.01, 0.02, H + 0.02, -0.005, sh)
        parts += shutter("sh_r", W / 2 + s + 0.01, W / 2 + s + sw, 0.02, H + 0.02, -0.005, sh)
        # Hinge pins and the shutter dogs that hold them open.
        iron = plain("iron", (0.03, 0.03, 0.03), rough=0.5, metal=0.6)
        for x in (-W / 2 - s - 0.02, W / 2 + s + 0.02):
            for z in (0.2, H - 0.2):
                parts.append(block("hinge", (x - 0.012, -0.02, z - 0.02), (x + 0.012, 0, z + 0.02), iron, 0.003))
    if flowers:
        box = plain("planter", (0.36, 0.13, 0.06), rough=0.8, mottle=0.12, grime=0.3, height=0.2)
        leaves = [plain("leaf_a", (0.05, 0.14, 0.03), rough=0.7, mottle=0.25), plain("leaf_b", (0.08, 0.18, 0.04), rough=0.7, mottle=0.25)]
        blooms = [plain("bloom_r", (0.7, 0.04, 0.03), rough=0.6, mottle=0.2), plain("bloom_p", (0.75, 0.2, 0.35), rough=0.6, mottle=0.2),
                  plain("bloom_w", (0.8, 0.78, 0.72), rough=0.6, mottle=0.1)]
        if rng() < 0.6:
            blooms = blooms[:1] + blooms[:1] + blooms[1:]
        parts += flower_box("fb", -W / 2 + 0.02, W / 2 - 0.02, -0.07, -0.12, rng, box, leaves, blooms)
    return parts, {"open": [W, H], "open_z": 0.0}


def door_town(name: str, paint: str | None = "brown", seed: int = 2, W: float = 1.22, H: float = 2.44, double: bool = True):
    """A town-house front door over a W × H opening: a stone surround with a cornice, a panelled door (double or
    single) under a glazed transom with an iron grille, a step, a house number."""
    rng = Rng(seed)
    parts = []
    stone = textured("stone", "acg:Concrete030", 1.6, tint=(1.1, 1.03, 0.9), grime=0.25, height=0.5)
    step_mat = textured("step", "large_sandstone_blocks", 1.8, tint=(0.95, 0.92, 0.86), grime=0.3, height=0.2)
    if paint:
        leaf = painted_wood("door_" + paint, SHUTTER_COLOURS[paint], wear=rng.range(0.03, 0.1), grain="rough_pine_door", tile=2.0, height=1.2)
    else:
        leaf = textured("door_wood", "rough_pine_door", 2.2, tint=(0.8, 0.75, 0.7), grime=0.3, height=1.0)
    iron = plain("iron", (0.03, 0.03, 0.03), rough=0.45, metal=0.7)
    brass = plain("brass", (0.55, 0.4, 0.15), rough=0.3, metal=0.9)
    s, d = 0.2, 0.08
    parts.append(block("jamb_l", (-W / 2 - s, -d, 0.15), (-W / 2, 0, H + s), stone, 0.012, 2))
    parts.append(block("jamb_r", (W / 2, -d, 0.15), (W / 2 + s, 0, H + s), stone, 0.012, 2))
    parts.append(block("head", (-W / 2, -d, H), (W / 2, 0, H + s), stone, 0.012, 2))
    parts.append(block("cornice", (-W / 2 - s - 0.07, -d - 0.08, H + s), (W / 2 + s + 0.07, 0, H + s + 0.09), stone, 0.015, 2))
    parts.append(block("base_l", (-W / 2 - s - 0.02, -d - 0.02, 0.15), (-W / 2 + 0.01, 0, 0.45), stone, 0.01))
    parts.append(block("base_r", (W / 2 - 0.01, -d - 0.02, 0.15), (W / 2 + s + 0.02, 0, 0.45), stone, 0.01))
    # The step: the opening starts at 0; the step stands in front of it.
    parts.append(block("step", (-W / 2 - s - 0.05, -0.38, 0), (W / 2 + s + 0.05, 0, 0.15), step_mat, 0.015, 2))
    # Transom: glass behind an iron grille.
    tz = H - 0.5
    g = glass("glass_t", False)
    parts.append(block("transom_glass", (-W / 2, -0.03, tz + 0.04), (W / 2, -0.025, H), g, 0.0))
    parts.append(block("transom_bar", (-W / 2, -0.06, tz - 0.02), (W / 2, -0.02, tz + 0.06), leaf, 0.006))
    for i in range(1, 8):
        x = -W / 2 + W * i / 8
        parts.append(block("grille", (x - 0.008, -0.05, tz + 0.06), (x + 0.008, -0.034, H), iron, 0.003))
    parts.append(block("grille_h", (-W / 2, -0.05, (tz + H) / 2 - 0.008), (W / 2, -0.034, (tz + H) / 2 + 0.008), iron, 0.003))
    # Leaves: a frame with raised panels.
    leaves = [(-W / 2, 0.0), (0.0, W / 2)] if double else [(-W / 2, W / 2)]
    y0 = -0.04
    for a, b in leaves:
        parts.append(block("leaf", (a + 0.005, y0, 0.15), (b - 0.005, -0.01, tz - 0.02), leaf, 0.006))
        # Two tall panels over a short one, each a raised field inside a moulding.
        for (za, zb) in ((0.3, 0.9), (1.02, tz - 0.14)):
            parts.append(block("panel", (a + 0.09, y0 - 0.018, za), (b - 0.09, y0, zb), leaf, 0.012, 2))
            parts.append(block("field", (a + 0.13, y0 - 0.03, za + 0.04), (b - 0.13, y0 - 0.015, zb - 0.04), leaf, 0.01, 2))
    # Handle, knocker, a letter slot, the house number.
    hx = 0.0 if double else W / 2 - 0.15
    parts.append(cylinder("knob", (hx - 0.06 if double else hx, y0 - 0.05, 1.05), 0.025, 0.05, brass, "Y"))
    if double:
        parts.append(cylinder("knob2", (hx + 0.06, y0 - 0.05, 1.05), 0.025, 0.05, brass, "Y"))
    parts.append(block("slot", (-W / 4 - 0.12, y0 - 0.025, 0.95), (-W / 4 + 0.12, y0 - 0.015, 0.99), brass, 0.003))
    parts.append(block("number", (W / 2 + s + 0.08, -0.015, 1.7), (W / 2 + s + 0.24, 0, 1.86), plain("enamel", (0.05, 0.1, 0.35), rough=0.3), 0.004))
    return parts, {"open": [W, H], "open_z": 0.0}


def balcony(name: str, seed: int = 3, W: float = 1.7, D: float = 0.6):
    """A small balcony: a stone slab on two corbels with a wrought-iron railing; its top at the model's z = 0.14
    (it goes under a French window's sill)."""
    parts = []
    stone = textured("stone", "acg:Concrete030", 1.6, tint=(1.08, 1.02, 0.9), grime=0.3, height=0.2)
    iron = plain("iron", (0.025, 0.025, 0.028), rough=0.5, metal=0.6, mottle=0.2)
    parts.append(block("slab", (-W / 2, -D, 0.0), (W / 2, 0, 0.14), stone, 0.015, 2))
    for x in (-W / 2 + 0.2, W / 2 - 0.2):
        c = block("corbel", (x - 0.08, -D + 0.1, -0.35), (x + 0.08, 0, 0.0), stone, 0.012, 2)
        # Taper the corbel: pull its lower front edge in.
        for v in c.data.vertices:
            if v.co.z < -0.2 and v.co.y < -0.2:
                v.co.y = -0.18
        parts.append(c)
    h = 0.95
    # Top and bottom rails round three sides.
    for z, t in ((0.2, 0.02), (h, 0.035)):
        parts.append(block("rail_f", (-W / 2 + 0.02, -D + 0.02, z - t / 2), (W / 2 - 0.02, -D + 0.02 + t, z + t / 2), iron, 0.004))
        for x in (-W / 2 + 0.02, W / 2 - 0.02 - t):
            parts.append(block("rail_s", (x, -D + 0.02, z - t / 2), (x + t, -0.01, z + t / 2), iron, 0.004))
    # Balusters, every 11 cm.
    n = int((W - 0.04) / 0.11)
    for i in range(n + 1):
        x = -W / 2 + 0.02 + (W - 0.06) * i / n
        parts.append(block("bar", (x - 0.007, -D + 0.022, 0.14), (x + 0.007, -D + 0.036, h), iron, 0.002))
    for side in (-1, 1):
        m = int((D - 0.05) / 0.11)
        for i in range(1, m + 1):
            y = -D + 0.03 + (D - 0.05) * i / (m + 1)
            x = side * (W / 2 - 0.03)
            parts.append(block("bar_s", (x - 0.007, y - 0.007, 0.14), (x + 0.007, y + 0.007, h), iron, 0.002))
    # Scrolls: short diagonal pairs forming a band of X's between the rails.
    for i in range(n):
        xa = -W / 2 + 0.02 + (W - 0.06) * i / n
        xb = -W / 2 + 0.02 + (W - 0.06) * (i + 1) / n
        for flip in (0, 1):
            p = block("x", (xa, -D + 0.024, 0.28), (xb, -D + 0.034, 0.29), iron, 0.0)
            ang = math.degrees(math.atan2(0.14, xb - xa)) * (1 if flip else -1)
            rotated(p, "Y", ang, ((xa + xb) / 2, -D + 0.029, 0.285))
            parts.append(p)
    return parts, {"top": 0.14}


CONTAINER_COLOURS = {
    "red": (0.3, 0.035, 0.025),
    "blue": (0.025, 0.075, 0.2),
    "green": (0.06, 0.13, 0.07),
    "grey": (0.3, 0.31, 0.29),
    "orange": (0.55, 0.16, 0.03),
    "brown": (0.2, 0.07, 0.035),
}


def container_paint(name: str, colour, seed: int):
    """Container paint: the colour, mottled, with rust bleeding down from the top rail and the seams, rust dots,
    dust and grime towards the bottom; the box-profile sheet's dents in the normal map."""
    mat, nt, p = _new_mat(name)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    mottle = _noise(nt, 4.0, 4.0)
    paint = _mix(nt, _ramp(nt, mottle.outputs["Fac"], 0.3, 0.7), tuple(c * 0.82 for c in colour), tuple(min(1.0, c * 1.15) for c in colour))
    # Streaks: noise stretched tall (object coords scaled 14:1 in x/y against z), stronger towards the top.
    st_map = nt.nodes.new("ShaderNodeMapping")
    st_map.inputs["Scale"].default_value = (14.0, 14.0, 0.9)
    st_map.inputs["Location"].default_value = (seed * 1.7, seed * 0.3, 0.0)
    nt.links.new(coord.outputs["Object"], st_map.inputs["Vector"])
    st = nt.nodes.new("ShaderNodeTexNoise")
    st.inputs["Detail"].default_value = 3.0
    nt.links.new(st_map.outputs["Vector"], st.inputs["Vector"])
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Object"], sep.inputs["Vector"])
    top = _ramp(nt, sep.outputs["Z"], 1.2, 2.6)
    streak = _ramp(nt, st.outputs["Fac"], 0.58, 0.72)
    k = nt.nodes.new("ShaderNodeMath")
    k.operation = "MULTIPLY"
    nt.links.new(streak, k.inputs[0])
    nt.links.new(top, k.inputs[1])
    k2 = nt.nodes.new("ShaderNodeMath")
    k2.operation = "MULTIPLY"
    nt.links.new(k.outputs[0], k2.inputs[0])
    k2.inputs[1].default_value = 0.8
    rust = (0.16, 0.055, 0.02)
    col = _mix(nt, k2.outputs[0], paint, rust)
    # Rust spots.
    spots = _ramp(nt, _noise(nt, 30.0, 6.0, 0.8).outputs["Fac"], 0.7, 0.74)
    col = _mix(nt, spots, col, (0.12, 0.045, 0.018))
    # Dust settles lighter on the lower part.
    low = _ramp(nt, sep.outputs["Z"], 0.9, 0.0)
    dust = nt.nodes.new("ShaderNodeMath")
    dust.operation = "MULTIPLY"
    nt.links.new(low, dust.inputs[0])
    dust.inputs[1].default_value = 0.3
    col = _mix(nt, dust.outputs[0], col, (0.26, 0.23, 0.19))
    nt.links.new(col, p.inputs["Base Color"])
    rmix = nt.nodes.new("ShaderNodeMix")
    rmix.data_type = "FLOAT"
    nt.links.new(k2.outputs[0], rmix.inputs["Factor"])
    rmix.inputs[2].default_value = 0.5
    rmix.inputs[3].default_value = 0.85
    nt.links.new(rmix.outputs[0], p.inputs["Roughness"])
    p.inputs["Metallic"].default_value = 0.0
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nm.inputs["Strength"].default_value = 0.5
    nt.links.new(_box_tex(nt, _ph("box_profile_metal_sheet", "nor"), True, 2.0, (seed * 0.37, 0, 0)).outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], p.inputs["Normal"])
    return mat


def corrugated(name: str, a0: float, a1: float, z0: float, z1: float, plane: float, out: float, axis: str, mat,
               pitch: float = 0.278, depth: float = 0.036, vertical: bool = False):
    """A sheet of trapezoidal corrugation spanning a0..a1 along `axis` ("x" or "y") and z0..z1, its crests at
    `plane` and its troughs `depth` further in (towards -out). vertical=False: the ribs run up (profile along the
    axis); True: they run along the axis (profile up the height)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    span = (z1 - z0) if vertical else (a1 - a0)
    n = max(1, round(span / pitch))
    step = span / n
    prof = []  # (position along the profile, offset in)
    for i in range(n):
        s0 = i * step
        prof += [(s0, 0.0), (s0 + step * 0.25, 0.0), (s0 + step * 0.4, depth), (s0 + step * 0.85, depth), (s0 + step, 0.0)]
    rows = []
    for (t, d) in prof:
        t_off = plane - out * d
        if vertical:
            z = z0 + t
            pts = [(a0, t_off, z), (a1, t_off, z)] if axis == "x" else [(t_off, a0, z), (t_off, a1, z)]
        else:
            a = a0 + t
            pts = [(a, t_off, z0), (a, t_off, z1)] if axis == "x" else [(t_off, a, z0), (t_off, a, z1)]
        rows.append([bm.verts.new(p) for p in pts])
    for r0, r1 in zip(rows, rows[1:]):
        f = bm.faces.new((r0[0], r1[0], r1[1], r0[1]))
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    # Faces point outwards (towards +out).
    bm.normal_update()
    for f in bm.faces:
        nrm = f.normal
        comp = nrm.y if axis == "x" else nrm.x
        if comp * out < 0:
            f.normal_flip()
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


def container(name: str, colour: str, L: float = 12.19, seed: int = 41):
    """An ISO container, L long (x), 2.44 wide (y), 2.59 high: corner posts and castings, rails, corrugated sides
    and front, a roof, and the door end (+x) with two leaves, locking bars, cams, handles and hinges."""
    W, H = 2.44, 2.59
    parts = []
    paint = container_paint("paint", CONTAINER_COLOURS[colour], seed)
    dark = plain("steel", (0.04, 0.035, 0.03), rough=0.55, metal=0.3, mottle=0.3, grime=0.3, height=0.6)
    x0, x1, y0, y1 = -L / 2, L / 2, -W / 2, W / 2
    post = 0.16
    # Corner posts and castings.
    for x in (x0, x1 - post):
        for y in (y0, y1 - post):
            parts.append(block("post", (x, y, 0.12), (x + post, y + post, H - 0.12), paint, 0.01))
            for z in (0.0, H - 0.12):
                parts.append(block("casting", (x - 0.005, y - 0.005, z), (x + post + 0.005, y + post + 0.005, z + 0.12), dark, 0.008))
    # Rails: bottom and top along the sides and across the ends.
    for y, sgn in ((y0, -1), (y1, 1)):
        parts.append(block("rail_b", (x0 + post, min(y, y - sgn * 0.14), 0.0), (x1 - post, max(y, y - sgn * 0.14), 0.16), paint, 0.008))
        parts.append(block("rail_t", (x0 + post, min(y, y - sgn * 0.1), H - 0.1), (x1 - post, max(y, y - sgn * 0.1), H), paint, 0.008))
    for x, sgn in ((x0, -1), (x1, 1)):
        parts.append(block("end_b", (min(x, x - sgn * 0.14), y0 + post, 0.0), (max(x, x - sgn * 0.14), y1 - post, 0.2), paint, 0.008))
        parts.append(block("end_t", (min(x, x - sgn * 0.14), y0 + post, H - 0.24), (max(x, x - sgn * 0.14), y1 - post, H), paint, 0.008))
    # Corrugated long sides (ribs up), the front end (ribs up), the roof (shallow, ribs across).
    for y, out in ((y0 + 0.012, -1), (y1 - 0.012, 1)):
        parts.append(corrugated("side", x0 + post, x1 - post, 0.16, H - 0.1, y, out, "x", paint))
    parts.append(corrugated("front", y0 + post, y1 - post, 0.2, H - 0.24, x0 + 0.03, -1, "y", paint, pitch=0.3, depth=0.03))
    roof = corrugated("roof", x0 + post, x1 - post, y0 + 0.02, y1 - 0.02, 0, 1, "x", paint, pitch=0.5, depth=0.015)
    # (the roof was made standing; lay it down: its "t" is z)
    for v in roof.data.vertices:
        x, t, z = v.co
        v.co = (x, z, H - 0.02 + t)
    # (swapping two axes mirrors the sheet: turn its faces back up)
    bm = bmesh.new()
    bm.from_mesh(roof.data)
    bm.normal_update()
    bmesh.ops.reverse_faces(bm, faces=[f for f in bm.faces if f.normal.z < 0])
    bm.to_mesh(roof.data)
    bm.free()
    parts.append(roof)
    # The door end: two leaves, each a panel with shallow vertical ribs, four locking bars with cams and handles.
    dx = x1 - 0.02
    iron = plain("iron", (0.05, 0.045, 0.04), rough=0.5, metal=0.5, mottle=0.25)
    for ya, yb in ((y0 + post, 0.0), (0.0, y1 - post)):
        parts.append(block("leaf", (dx - 0.03, ya + 0.004, 0.2), (dx, yb - 0.004, H - 0.24), paint, 0.006))
        parts.append(corrugated("leaf_rib", ya + 0.06, yb - 0.06, 0.3, H - 0.34, dx + 0.018, 1, "y", paint, pitch=0.24, depth=0.018))
        for yy in (ya + (yb - ya) * 0.22, ya + (yb - ya) * 0.78):
            parts.append(cylinder("bar", (dx + 0.06, yy, (H - 0.04) / 2), 0.022, H - 0.5, iron, "Z", 8))
            for z in (0.3, H - 0.35):
                parts.append(block("cam", (dx + 0.02, yy - 0.05, z - 0.05), (dx + 0.08, yy + 0.05, z + 0.05), iron, 0.006))
            parts.append(block("handle", (dx + 0.06, yy - 0.02, 1.0), (dx + 0.13, yy + 0.02, 1.35), iron, 0.008))
        for z in (0.45, 1.3, 2.1):
            yh = ya + 0.02 if ya < 0 else yb - 0.07
            parts.append(block("hinge", (dx - 0.01, yh, z - 0.06), (dx + 0.03, yh + 0.05, z + 0.06), iron, 0.005))
    # The CSC plate and a placard on the door.
    plate = plain("plate", (0.55, 0.55, 0.5), rough=0.4, metal=0.4, mottle=0.1)
    parts.append(block("csc", (dx + 0.015, 0.18, 1.5), (dx + 0.025, 0.42, 1.68), plate, 0.003))
    return parts, {"box": [L, W, H]}


def shop_glass(name: str, seed: int):
    """A shop window seen from the street: the reflection's gradient, and behind it the display: shelves of goods
    (patches of colour in rows) fading into the dark shop."""
    mat, nt, p = _new_mat(name)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Generated"], sep.inputs["Vector"])
    up = _ramp(nt, sep.outputs["Z"], 0.35, 1.0)
    streak = _ramp(nt, _noise(nt, 2.0, 2.0).outputs["Fac"], 0.35, 0.8)
    sky = _mix(nt, streak, (0.06, 0.07, 0.09), (0.2, 0.24, 0.3))
    col = _mix(nt, up, (0.03, 0.03, 0.03), sky)
    # Goods: a Voronoi of colour cells in rows (shelves), shown in the lower two thirds.
    vor = nt.nodes.new("ShaderNodeTexVoronoi")
    vor.inputs["Scale"].default_value = 9.0
    vmap = nt.nodes.new("ShaderNodeMapping")
    vmap.inputs["Scale"].default_value = (3.0, 1.0, 1.6)
    vmap.inputs["Location"].default_value = (seed * 0.61, 0.0, 0.0)
    nt.links.new(coord.outputs["Generated"], vmap.inputs["Vector"])
    nt.links.new(vmap.outputs["Vector"], vor.inputs["Vector"])
    shelves = nt.nodes.new("ShaderNodeTexWave")
    shelves.wave_type = "BANDS"
    shelves.bands_direction = "Z"
    shelves.inputs["Scale"].default_value = 3.0
    shelves.inputs["Distortion"].default_value = 0.0
    nt.links.new(coord.outputs["Generated"], shelves.inputs["Vector"])
    goods_mask = _ramp(nt, shelves.outputs["Fac"], 0.35, 0.55)
    low = _ramp(nt, sep.outputs["Z"], 0.75, 0.6)
    k = nt.nodes.new("ShaderNodeMath")
    k.operation = "MULTIPLY"
    nt.links.new(goods_mask, k.inputs[0])
    nt.links.new(low, k.inputs[1])
    k2 = nt.nodes.new("ShaderNodeMath")
    k2.operation = "MULTIPLY"
    nt.links.new(k.outputs[0], k2.inputs[0])
    k2.inputs[1].default_value = 0.4
    # (muted: seen through glass in the shop's gloom, not a stained-glass window)
    grey = nt.nodes.new("ShaderNodeRGBToBW")
    nt.links.new(vor.outputs["Color"], grey.inputs["Color"])
    goods = _mix(nt, _ramp(nt, grey.outputs["Val"], 0.0, 3.0), vor.outputs["Color"], (0.5, 0.5, 0.5))
    goods = _multiply(nt, goods, (0.16, 0.14, 0.12))
    col = _mix(nt, k2.outputs[0], col, goods)
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.1
    return mat


def shopfront(name: str, W: float, paint: str = "green", seed: int = 51, H: float = 2.4):
    """A shop front over a W × H display window whose bottom is at z = 0: a panelled bulkhead below it, a painted
    frame with glazing bars, the window, a sign board over it. (The door is a door piece of its own.)"""
    rng = Rng(seed)
    parts = []
    wood = painted_wood("shop_" + paint, SHUTTER_COLOURS[paint], wear=rng.range(0.03, 0.08), grain="wood_shutter", tile=1.2, height=0.8)
    trim = painted_wood("shop_trim", (0.62, 0.58, 0.48), wear=0.04, grain="wood_shutter", tile=1.0, rough=0.5, grime=0.2, height=0.8)
    g = shop_glass("shop_glass", seed)
    d = 0.1
    # Bulkhead with two raised panels per bay.
    parts.append(block("bulk", (-W / 2 - 0.1, -d, -0.6), (W / 2 + 0.1, 0, 0.0), wood, 0.01))
    bays = max(1, round(W / 1.1))
    for i in range(bays):
        a = -W / 2 + W * i / bays + 0.08
        b = -W / 2 + W * (i + 1) / bays - 0.08
        parts.append(block("bpanel", (a, -d - 0.02, -0.5), (b, -d, -0.1), wood, 0.012, 2))
    # Frame: stiles, head, sill, mullions and a transom.
    parts.append(block("st_l", (-W / 2 - 0.1, -d, 0.0), (-W / 2 + 0.02, 0, H), wood, 0.008))
    parts.append(block("st_r", (W / 2 - 0.02, -d, 0.0), (W / 2 + 0.1, 0, H), wood, 0.008))
    parts.append(block("sill", (-W / 2 - 0.12, -d - 0.05, -0.02), (W / 2 + 0.12, 0, 0.06), trim, 0.01))
    parts.append(block("transom", (-W / 2, -d + 0.02, H - 0.5), (W / 2, 0, H - 0.44), wood, 0.006))
    for i in range(1, bays):
        x = -W / 2 + W * i / bays
        parts.append(block("mull", (x - 0.035, -d + 0.02, 0.06), (x + 0.035, 0, H), wood, 0.006))
    for i in range(bays * 2):
        x = -W / 2 + W * (i + 0.5) / (bays * 2)
        parts.append(block("tbar", (x - 0.012, -d + 0.03, H - 0.44), (x + 0.012, -d + 0.05, H), wood, 0.003))
    parts.append(block("glass", (-W / 2 + 0.02, -0.045, 0.06), (W / 2 - 0.02, -0.04, H - 0.5), g, 0.0))
    parts.append(block("glass_t", (-W / 2 + 0.02, -0.045, H - 0.44), (W / 2 - 0.02, -0.04, H), glass("glass_top", False), 0.0))
    # Fascia: the sign board with a moulded border and a cornice.
    parts.append(block("fascia", (-W / 2 - 0.15, -d - 0.02, H), (W / 2 + 0.15, 0, H + 0.55), wood, 0.01))
    parts.append(block("sign", (-W / 2 + 0.05, -d - 0.035, H + 0.1), (W / 2 - 0.05, -d - 0.02, H + 0.45), trim, 0.006))
    parts.append(block("cornice", (-W / 2 - 0.22, -d - 0.1, H + 0.55), (W / 2 + 0.22, 0, H + 0.65), wood, 0.012, 2))
    return parts, {"open": [W, H], "open_z": 0.0}


AWNINGS = {"red": ((0.45, 0.04, 0.03), (0.75, 0.72, 0.64)), "green": ((0.05, 0.16, 0.08), (0.72, 0.7, 0.6)),
           "blue": ((0.04, 0.09, 0.22), (0.75, 0.74, 0.68)), "ochre": ((0.5, 0.3, 0.06), (0.72, 0.68, 0.58))}


def awning(name: str, W: float, colour: str, seed: int = 71, D: float = 1.3, drop: float = 0.55):
    """A shop awning: striped canvas sloping out from the wall (its top at z = 0 before the model is set on its
    bottom), a scalloped valance, two iron arms."""
    a, b = AWNINGS[colour]
    mat, nt, p = _new_mat("canvas_" + colour)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    wave = nt.nodes.new("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = "X"
    wave.inputs["Scale"].default_value = W / 0.25 / 2
    wave.inputs["Distortion"].default_value = 0.0
    nt.links.new(coord.outputs["Generated"], wave.inputs["Vector"])
    stripes = _ramp(nt, wave.outputs["Fac"], 0.48, 0.52)
    col = _mix(nt, stripes, a, b)
    col = _grime(nt, col, -0.2, 0.25)
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.85
    iron = plain("iron", (0.03, 0.03, 0.03), rough=0.5, metal=0.6)
    parts = []
    # The canvas: a thin sloped sheet (a box sheared down to the front edge).
    sheet = block("canvas", (-W / 2, -D, -0.012), (W / 2, 0, 0.012), mat, 0.0)
    for v in sheet.data.vertices:
        v.co.z -= (-v.co.y / D) * drop
    parts.append(sheet)
    # Valance: a strip hanging from the front edge with scallops (a row of small half-discs).
    parts.append(block("valance", (-W / 2, -D - 0.012, -drop - 0.2), (W / 2, -D + 0.004, -drop), mat, 0.0))
    n = int(W / 0.25)
    for i in range(n):
        x = -W / 2 + W * (i + 0.5) / n
        c = cylinder("scallop", (x, -D - 0.004, -drop - 0.2), 0.11, 0.016, mat, "Y", 10)
        # (keep the lower half)
        for v in c.data.vertices:
            v.co.z = min(v.co.z, -drop - 0.2)
        parts.append(c)
    # Arms from the wall to the front corners, and the roller box at the top.
    for x in (-W / 2 + 0.05, W / 2 - 0.05):
        arm = block("arm", (x - 0.012, -D, -0.02), (x + 0.012, 0, 0.0), iron, 0.003)
        for v in arm.data.vertices:
            v.co.z -= (-v.co.y / D) * drop + 0.03
        parts.append(arm)
    parts.append(cylinder("roller", (0, -0.07, 0.03), 0.07, W + 0.04, iron, "X", 10))
    return parts, {"top": 0.1}


def torus(name: str, centre, R: float, r: float, mat, axis: str = "Y", major: int = 16, minor: int = 6, arc: float = 360.0):
    """A ring (or an arc of one), lying in the plane across `axis`."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rings = []
    n = major if arc >= 360 else major + 1
    for i in range(n):
        a = math.radians(arc) * i / major
        ring = []
        for j in range(minor):
            b = 2 * math.pi * j / minor
            x = (R + r * math.cos(b)) * math.cos(a)
            z = (R + r * math.cos(b)) * math.sin(a)
            y = r * math.sin(b)
            p = Vector((x, y, z))
            if axis == "Z":
                p = Vector((x, z, y))
            elif axis == "X":
                p = Vector((y, x, z))
            ring.append(bm.verts.new(p + Vector(centre)))
        rings.append(ring)
    pairs = list(zip(rings, rings[1:])) + ([(rings[-1], rings[0])] if arc >= 360 else [])
    for r0, r1 in pairs:
        for j in range(minor):
            bm.faces.new((r0[j], r0[(j + 1) % minor], r1[(j + 1) % minor], r1[j]))
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


def bistro_chair(name: str, x: float, y: float, turn: float, iron, slat):
    """A bistro chair (iron frame, wooden slat seat and back) standing at (x, y), facing `turn` degrees."""
    parts = []
    for dx, dy in ((-0.19, -0.19), (0.19, -0.19), (-0.19, 0.19), (0.19, 0.19)):
        parts.append(cylinder("leg", (dx, dy, 0.23), 0.011, 0.46, iron, "Z", 6))
    parts.append(torus("seat_ring", (0, 0, 0.46), 0.2, 0.012, iron, "Z", 14, 5))
    for i in range(5):
        yy = -0.17 + 0.085 * i
        parts.append(block("slat", (-0.2, yy - 0.03, 0.46), (0.2, yy + 0.03, 0.48), slat, 0.004))
    for dx in (-0.18, 0.18):
        parts.append(cylinder("post", (dx, 0.2, 0.68), 0.01, 0.44, iron, "Z", 6))
    for z in (0.72, 0.86):
        parts.append(block("back", (-0.19, 0.19, z - 0.03), (0.19, 0.22, z + 0.03), slat, 0.004))
    m = Matrix.Translation(Vector((x, y, 0))) @ Matrix.Rotation(math.radians(turn), 4, "Z")
    for p in parts:
        p.data.transform(m)
    return parts


def cafe_set(name: str, parasol: str | None, seed: int = 81):
    """A café table outside a shop: a round bistro table on a cast-iron foot, two or three chairs, maybe a parasol."""
    rng = Rng(seed)
    iron = plain("iron", (0.025, 0.03, 0.028), rough=0.45, metal=0.6, mottle=0.2)
    top = textured("marble", "acg:Concrete030", 0.6, tint=(1.25, 1.22, 1.15), rough_mul=0.5)
    slat = painted_wood("slat", (0.05, 0.12, 0.07), wear=0.08, tile=0.6, height=0.9)
    parts = [cylinder("top", (0, 0, 0.74), 0.34, 0.03, top, "Z", 24), cylinder("stem", (0, 0, 0.38), 0.03, 0.7, iron, "Z", 8)]
    for a in range(3):
        f = block("foot", (0, -0.025, 0.0), (0.3, 0.025, 0.05), iron, 0.006)
        rotated(f, "Z", 120 * a + 30, (0, 0, 0))
        parts.append(f)
    for i in range(3 if rng() < 0.5 else 2):
        a = math.radians(120 * i + rng.range(-20, 20) + 90)
        parts += bistro_chair("chair", math.cos(a) * 0.62, math.sin(a) * 0.62, math.degrees(a) + 90 + rng.range(-15, 15), iron, slat)
    if parasol:
        a, b = AWNINGS[parasol]
        canvas = plain("parasol_" + parasol, a, rough=0.85, mottle=0.08)
        edge = plain("parasol_edge", b, rough=0.85, mottle=0.05)
        parts.append(cylinder("pole", (0, 0, 1.25), 0.02, 2.5, plain("pole", (0.6, 0.58, 0.52), rough=0.5), "Z", 8))
        # Eight canvas panels (a shallow octagonal cone) and a valance.
        me = bpy.data.meshes.new("canopy")
        bm = bmesh.new()
        R, top_z, rim_z = 1.35, 2.62, 2.25
        apex = bm.verts.new((0, 0, top_z))
        rim = [bm.verts.new((R * math.cos(math.pi * 2 * i / 8), R * math.sin(math.pi * 2 * i / 8), rim_z)) for i in range(8)]
        for i in range(8):
            bm.faces.new((apex, rim[i], rim[(i + 1) % 8]))
        bm.to_mesh(me)
        bm.free()
        cone = bpy.data.objects.new("canopy", me)
        bpy.context.scene.collection.objects.link(cone)
        me.materials.append(canvas)
        # (a thickness so it casts and takes light on both sides)
        sol = cone.modifiers.new("solid", "SOLIDIFY")
        sol.thickness = 0.015
        build._select(cone)
        bpy.ops.object.modifier_apply(modifier="solid")
        parts.append(cone)
        for i in range(8):
            a0, a1 = math.pi * 2 * i / 8, math.pi * 2 * (i + 1) / 8
            p0 = Vector((R * math.cos(a0), R * math.sin(a0), rim_z))
            p1 = Vector((R * math.cos(a1), R * math.sin(a1), rim_z))
            v = block("valance", (0, -0.006, -0.16), ((p1 - p0).length, 0.006, 0.0), edge, 0.0)
            v.data.transform(Matrix.Translation(p0) @ Matrix.Rotation(math.atan2((p1 - p0).y, (p1 - p0).x), 4, "Z"))
            parts.append(v)
        parts.append(cylinder("base", (0, 0, 0.05), 0.22, 0.1, iron, "Z", 12))
    return parts, {}


PRODUCE = {"apples": (0.45, 0.04, 0.03), "oranges": (0.8, 0.28, 0.02), "lemons": (0.8, 0.65, 0.05),
           "cabbage": (0.18, 0.35, 0.08), "potatoes": (0.4, 0.28, 0.14), "tomatoes": (0.6, 0.05, 0.02)}


def market_stall(name: str, roof: str, seed: int = 91, W: float = 2.6, D: float = 1.4):
    """A market stall: a timber frame, a striped canvas roof sloping to the front, a counter of produce crates."""
    rng = Rng(seed)
    wood = textured("timber", "medieval_wood", 1.5, tint=(0.9, 0.8, 0.7), grime=0.3, height=0.6)
    crate_wood = textured("crate", "wood_shutter", 0.8, tint=(0.95, 0.9, 0.8))
    a, b = AWNINGS[roof]
    parts = []
    for x in (-W / 2, W / 2 - 0.08):
        for y, h in ((-D / 2, 2.1), (D / 2 - 0.08, 2.4)):
            parts.append(block("post", (x, y, 0), (x + 0.08, y + 0.08, h), wood, 0.008))
    # Counter: a board on trestles, crates of produce on it.
    parts.append(block("counter", (-W / 2, -D / 2 - 0.05, 0.8), (W / 2, -D / 2 + 0.6, 0.85), wood, 0.01))
    parts.append(block("skirt", (-W / 2, -D / 2 - 0.05, 0.1), (W / 2, -D / 2 - 0.02, 0.8), textured("skirt", "wood_shutter", 1.0, tint=(0.7, 0.55, 0.4)), 0.004))
    kinds = list(PRODUCE)
    n = int(W / 0.5)
    for i in range(n):
        x = -W / 2 + 0.05 + (W - 0.1) * (i + 0.5) / n
        kind = kinds[int(rng() * len(kinds))]
        parts.append(block("crate", (x - 0.22, -D / 2 + 0.02, 0.85), (x + 0.22, -D / 2 + 0.5, 1.0), crate_wood, 0.006))
        fruit = plain("fruit_" + kind, PRODUCE[kind], rough=0.45 if kind != "potatoes" else 0.8, mottle=0.2)
        for j in range(9):
            parts.append(blob("fruit", (x + rng.range(-0.18, 0.18), -D / 2 + rng.range(0.07, 0.45), 1.0 + rng.range(0.0, 0.04)), 0.04, fruit, 1, 0.9))
    # More crates stacked behind.
    for i in range(3):
        x = rng.range(-W / 2 + 0.3, W / 2 - 0.3)
        parts.append(block("crate2", (x - 0.22, D / 2 - 0.55, 0.0), (x + 0.22, D / 2 - 0.1, 0.3), crate_wood, 0.006))
    # The roof: striped canvas from the back (2.45) to the front (2.1), overhanging.
    mat, nt, p = _new_mat("stall_" + roof)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    wave = nt.nodes.new("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = "X"
    wave.inputs["Scale"].default_value = W / 0.3 / 2
    wave.inputs["Distortion"].default_value = 0.0
    nt.links.new(coord.outputs["Generated"], wave.inputs["Vector"])
    col = _mix(nt, _ramp(nt, wave.outputs["Fac"], 0.48, 0.52), a, b)
    nt.links.new(_grime(nt, col, 2.0, 0.15), p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.85
    sheet = block("roof", (-W / 2 - 0.15, -D / 2 - 0.35, -0.01), (W / 2 + 0.15, D / 2 + 0.1, 0.01), mat, 0.0)
    for v in sheet.data.vertices:
        t = (v.co.y + D / 2 + 0.35) / (D + 0.45)
        v.co.z += 2.05 + 0.4 * t
    parts.append(sheet)
    parts.append(block("valance", (-W / 2 - 0.15, -D / 2 - 0.36, 1.85), (W / 2 + 0.15, -D / 2 - 0.34, 2.06), mat, 0.0))
    return parts, {}


def planter_stone(name: str, seed: int = 101):
    """A square stone planter with a clipped box shrub (leaf clumps) in it."""
    rng = Rng(seed)
    stone = textured("stone", "acg:Concrete030", 1.2, tint=(1.05, 0.98, 0.88), grime=0.35, height=0.4)
    soil = plain("soil", (0.06, 0.04, 0.025), rough=0.95)
    leaves = [plain("leaf_a", (0.03, 0.09, 0.025), rough=0.75, mottle=0.3), plain("leaf_b", (0.05, 0.12, 0.03), rough=0.75, mottle=0.3)]
    parts = [block("box", (-0.4, -0.4, 0), (0.4, 0.4, 0.55), stone, 0.02, 2), block("rim", (-0.44, -0.44, 0.5), (0.44, 0.44, 0.6), stone, 0.015, 2),
             block("soil", (-0.34, -0.34, 0.55), (0.34, 0.34, 0.58), soil, 0.0)]
    for i in range(38):
        a = rng.range(0, math.tau)
        r = 0.32 * math.sqrt(rng())
        h = 0.62 + rng.range(0.0, 0.55) * (1 - r / 0.4)
        parts.append(blob("leaf", (math.cos(a) * r, math.sin(a) * r, h), rng.range(0.1, 0.16), leaves[i % 2], 2, 0.9))
    return parts, {}


def bakery_sign(name: str, seed: int = 111):
    """A hanging shop sign: a wrought-iron bracket from the wall, a gilded pretzel hanging from it (its back at the
    wall, y = 0)."""
    iron = plain("iron", (0.025, 0.025, 0.028), rough=0.5, metal=0.6, mottle=0.2)
    gold = plain("gilt", (0.6, 0.42, 0.12), rough=0.3, metal=0.9, mottle=0.15)
    parts = [block("plate", (-0.1, -0.02, -0.15), (0.1, 0, 0.15), iron, 0.004),
             block("arm", (-0.015, -0.9, 0.08), (0.015, -0.01, 0.11), iron, 0.003),
             block("strut", (-0.012, -0.5, -0.12), (0.012, -0.01, -0.09), iron, 0.003)]
    rotated(parts[-1], "X", -25, (0, -0.01, -0.1))
    parts.append(torus("scroll", (0, -0.45, -0.02), 0.09, 0.008, iron, "X", 14, 5, 270))
    for y in (-0.35, -0.75):
        parts.append(cylinder("chain", (0, y, -0.02), 0.006, 0.2, iron, "Z", 5))
    # The pretzel: a big loop and two crossing arms (rings and arcs), gilded.
    parts.append(torus("loop", (0, -0.55, -0.45), 0.24, 0.04, gold, "Y", 18, 8, 360))
    parts.append(torus("arm_l", (-0.1, -0.55, -0.4), 0.14, 0.035, gold, "Y", 12, 8, 180))
    parts.append(torus("arm_r", (0.1, -0.55, -0.4), 0.14, 0.035, gold, "Y", 12, 8, 180))
    return parts, {}


def blinds_glass(name: str, down: float, seed: int):
    """Office glass: the reflection's gradient over a room, venetian blinds lowered `down` of the way (pale slats in
    fine bands)."""
    mat, nt, p = _new_mat(name)
    coord = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Generated"], sep.inputs["Vector"])
    up = _ramp(nt, sep.outputs["Z"], 0.2, 1.0)
    streak = _ramp(nt, _noise(nt, 2.2, 2.0).outputs["Fac"], 0.35, 0.8)
    col = _mix(nt, up, (0.03, 0.035, 0.04), _mix(nt, streak, (0.08, 0.1, 0.12), (0.22, 0.26, 0.32)))
    slats = nt.nodes.new("ShaderNodeTexWave")
    slats.wave_type = "BANDS"
    slats.bands_direction = "Z"
    slats.inputs["Scale"].default_value = 28.0
    slats.inputs["Distortion"].default_value = 0.0
    nt.links.new(coord.outputs["Generated"], slats.inputs["Vector"])
    blind = _mix(nt, _ramp(nt, slats.outputs["Fac"], 0.3, 0.7), (0.2, 0.2, 0.19), (0.34, 0.34, 0.32))
    lowered = _ramp(nt, sep.outputs["Z"], 1.0 - down - 0.01, 1.0 - down + 0.01)
    col = _mix(nt, lowered, col, blind)
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.08
    return mat


def window_office(name: str, W: float, H: float, style: str = "office", seed: int = 121):
    """A plain modern or industrial window over a W × H opening (bottom at z = 0): a concrete sill, a frame set
    back in a reveal, glazing bars (office: a vertical split and a transom, pale aluminium; factory: a grid of
    small panes in dark steel), glass with blinds or grime."""
    rng = Rng(seed)
    parts = []
    concrete = textured("sill", "acg:Concrete030", 1.2, tint=(1.0, 0.98, 0.94), grime=0.3, height=0.1)
    if style == "office":
        frame = plain("alu", (0.55, 0.56, 0.56), rough=0.35, metal=0.6, mottle=0.05, grime=0.15, height=0.3)
        g = blinds_glass("glass_" + name, rng.range(0.0, 0.7), seed)
    else:
        frame = plain("steel", (0.05, 0.07, 0.06), rough=0.55, metal=0.3, mottle=0.2, grime=0.25, height=0.4)
        g = glass("glass_grimy", False)
    reveal = plain("reveal", (0.3, 0.29, 0.27), rough=0.9)
    parts.append(block("rev", (-W / 2, -0.005, 0), (W / 2, 0, H), reveal, 0.0))
    parts.append(block("sill", (-W / 2 - 0.05, -0.1, -0.06), (W / 2 + 0.05, 0, 0.0), concrete, 0.01, 2))
    f = 0.05
    fy = -0.03
    for lo, hi in (((-W / 2, fy - 0.02, 0), (-W / 2 + f, 0, H)), ((W / 2 - f, fy - 0.02, 0), (W / 2, 0, H)),
                   ((-W / 2, fy - 0.02, H - f), (W / 2, 0, H)), ((-W / 2, fy - 0.02, 0), (W / 2, 0, f))):
        parts.append(block("fr", lo, hi, frame, 0.003))
    if style == "office":
        parts.append(block("mull", (-0.025, fy - 0.015, f), (0.025, 0, H - f), frame, 0.003))
        tz = H * 0.78
        parts.append(block("transom", (-W / 2, fy - 0.015, tz - 0.025), (W / 2, 0, tz + 0.025), frame, 0.003))
    else:
        nx = max(2, round(W / 0.35))
        nz = max(2, round(H / 0.35))
        for i in range(1, nx):
            x = -W / 2 + W * i / nx
            parts.append(block("gx", (x - 0.012, fy - 0.01, f), (x + 0.012, 0, H - f), frame, 0.002))
        for j in range(1, nz):
            z = H * j / nz
            parts.append(block("gz", (-W / 2, fy - 0.01, z - 0.012), (W / 2, 0, z + 0.012), frame, 0.002))
    parts.append(block("glass", (-W / 2 + f, fy + 0.005, f), (W / 2 - f, fy + 0.01, H - f), g, 0.0))
    return parts, {"open": [W, H], "open_z": 0.0}


def fuel_tank(name: str, colour, R: float = 3.3, H: float = 6.35, seed: int = 141):
    """An upright fuel tank: a smooth painted shell with welded ring seams, a low cone roof, a caged ladder up one
    side, a railing round the roof, a manhole and pipe outlets with valves at the foot."""
    rng = Rng(seed)
    paint = container_paint("tank_paint", colour, seed)
    steel = plain("steel", (0.2, 0.2, 0.19), rough=0.45, metal=0.6, mottle=0.2, grime=0.3, height=1.0)
    rail = plain("rail", (0.55, 0.45, 0.06), rough=0.5, metal=0.2, mottle=0.15)
    parts = [cylinder("shell", (0, 0, H / 2), R, H, paint, "Z", 48)]
    for z in (0.02, H * 0.34, H * 0.67):
        parts.append(torus("seam", (0, 0, z + 0.02), R + 0.005, 0.02, paint, "Z", 48, 4))
    # Roof: a shallow cone.
    me = bpy.data.meshes.new("roof")
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=R + 0.08, radius2=0.35, depth=0.9)
    bmesh.ops.translate(bm, vec=Vector((0, 0, H + 0.45)), verts=bm.verts)
    bm.to_mesh(me)
    bm.free()
    roof = bpy.data.objects.new("roof", me)
    bpy.context.scene.collection.objects.link(roof)
    me.materials.append(paint)
    parts.append(roof)
    parts.append(cylinder("vent", (0, 0, H + 1.05), 0.2, 0.4, steel, "Z", 12))
    # Railing round the roof edge: posts and two rails.
    for i in range(24):
        a = math.tau * i / 24
        parts.append(cylinder("post", ((R - 0.1) * math.cos(a), (R - 0.1) * math.sin(a), H + 0.55), 0.02, 1.1, rail, "Z", 6))
    for z in (H + 0.6, H + 1.08):
        parts.append(torus("rail", (0, 0, z), R - 0.1, 0.022, rail, "Z", 48, 5))
    # The ladder: two stiles, rungs, a cage of hoops above head height, on the -y side (the front).
    ly = -R - 0.25
    for x in (-0.22, 0.22):
        parts.append(block("stile", (x - 0.025, ly - 0.02, 0.0), (x + 0.025, ly + 0.02, H + 1.1), steel, 0.0))
    for i in range(int(H / 0.3)):
        z = 0.3 + 0.3 * i
        parts.append(block("rung", (-0.22, ly - 0.012, z - 0.012), (0.22, ly + 0.012, z + 0.012), steel, 0.0))
    # The cage: hoops round the climber (open towards the tank) and three bars joining them.
    cy, cr = ly - 0.32, 0.36
    for i in range(int((H - 2.4) / 0.8) + 1):
        z = 2.4 + 0.8 * i
        hoop = torus("hoop", (0, 0, z), cr, 0.02, steel, "Z", 12, 4, 200)
        hoop.data.transform(Matrix.Translation(Vector((0, cy, 0))) @ Matrix.Rotation(math.radians(170), 4, "Z"))
        parts.append(hoop)
    for a in (math.radians(200), math.radians(270), math.radians(340)):
        x, y = cr * math.cos(a), cy + cr * math.sin(a)
        parts.append(block("cage", (x - 0.014, y - 0.014, 2.4), (x + 0.014, y + 0.014, H + 1.0), steel, 0.0))
    # A manhole and two outlets with valves near the foot.
    parts.append(cylinder("manhole", (R * 0.7, -R * 0.7, 0.8), 0.35, 0.12, paint, "Y", 20))
    valve = plain("valve", (0.5, 0.05, 0.03), rough=0.5)
    for a in (0.6, 2.3):
        # An outlet: a pipe out of the shell (radial), a flange, a valve body with its hand wheel on top.
        m = Matrix.Rotation(a, 4, "Z")
        pipe = cylinder("pipe", (R + 0.45, 0, 0.5), 0.12, 0.95, steel, "X", 12)
        flange = cylinder("flange", (R + 0.12, 0, 0.5), 0.2, 0.05, steel, "X", 12)
        body = block("valve", (R + 0.5, -0.14, 0.36), (R + 0.72, 0.14, 0.64), valve, 0.02)
        stem = cylinder("stem", (R + 0.61, 0, 0.75), 0.02, 0.24, steel, "Z", 6)
        wheel = torus("wheel", (R + 0.61, 0, 0.87), 0.14, 0.015, valve, "Z", 12, 4)
        for ob in (pipe, flange, body, stem, wheel):
            ob.data.transform(m)
            parts.append(ob)
    del rng
    return parts, {}


def wooden_crate(name: str, tone: str = "pine", seed: int = 151, S: float = 1.0):
    """A shipping crate S metres on a side: planked faces (a hair apart), a frame of battens round every edge, a
    diagonal brace on each side, nail heads; raw pine or old weathered wood."""
    rng = Rng(seed)
    if tone == "pine":
        wood = textured("pine", "wood_shutter", 0.9, tint=(1.25, 1.1, 0.85), grime=0.25, height=0.35)
        batten = textured("pine_b", "wood_shutter", 0.9, tint=(1.1, 0.95, 0.72), grime=0.3, height=0.35)
    else:
        wood = textured("old", "medieval_wood", 1.2, tint=(0.95, 0.88, 0.78), grime=0.3, height=0.35)
        batten = textured("old_b", "medieval_wood", 1.2, tint=(0.8, 0.72, 0.62), grime=0.35, height=0.35)
    iron = plain("nail", (0.1, 0.09, 0.08), rough=0.5, metal=0.5)
    parts = []
    t = 0.022  # plank thickness
    b = 0.07  # batten width
    h = S / 2
    # Core faces as planks: each side has 5 planks.
    n = 5
    for axis in ("x", "y"):
        for sgn in (-1, 1):
            for i in range(n):
                a0 = -h + S * i / n + 0.003
                a1 = -h + S * (i + 1) / n - 0.003
                if axis == "x":
                    parts.append(block("plank", (a0, sgn * h - (t if sgn > 0 else 0), 0.0), (a1, sgn * h + (0 if sgn > 0 else t), S), wood, 0.004))
                else:
                    parts.append(block("plank", (sgn * h - (t if sgn > 0 else 0), a0, 0.0), (sgn * h + (0 if sgn > 0 else t), a1, S), wood, 0.004))
    for i in range(n):
        a0 = -h + S * i / n + 0.003
        a1 = -h + S * (i + 1) / n - 0.003
        parts.append(block("lid", (a0, -h, S - t), (a1, h, S), wood, 0.004))
    # Battens round the edges of each vertical face (proud of it), and a diagonal brace.
    for axis in ("x", "y"):
        for sgn in (-1, 1):
            f = sgn * (h + 0.018)
            def bx(u0, u1, z0, z1):
                if axis == "x":
                    return block("bat", (u0, min(f, sgn * h), z0), (u1, max(f, sgn * h), z1), batten, 0.006)
                return block("bat", (min(f, sgn * h), u0, z0), (max(f, sgn * h), u1, z1), batten, 0.006)
            parts.append(bx(-h, h, 0.0, b))
            parts.append(bx(-h, h, S - b, S))
            parts.append(bx(-h, -h + b, b, S - b))
            parts.append(bx(h - b, h, b, S - b))
            # The brace, corner to corner.
            L = math.hypot(S - 2 * b, S - 2 * b)
            br = bx(-L / 2, L / 2, S / 2 - b / 2, S / 2 + b / 2)
            ang = 45 if (sgn > 0) == (axis == "x") else -45
            rotated(br, "Y" if axis == "x" else "X", ang if axis == "x" else -ang, (0, 0, S / 2))
            parts.append(br)
            # Nails at the batten corners.
            for u in (-h + b / 2, h - b / 2):
                for z in (b / 2, S - b / 2):
                    c = (u, f + sgn * 0.004, z) if axis == "x" else (f + sgn * 0.004, u, z)
                    parts.append(cylinder("nail", c, 0.008, 0.01, iron, "Y" if axis == "x" else "X", 6))
    del rng
    return parts, {}


def sandbags(name: str, seed: int = 161, L: float = 1.0, D: float = 0.55, courses: int = 4):
    """A metre of sandbag wall: `courses` rows of plump bags laid like bricks (every other row half a bag along),
    each a bevelled, slightly squashed and turned pillow in olive or tan cloth with a tied end."""
    rng = Rng(seed)
    cloths = [plain("bag_olive", (0.13, 0.12, 0.07), rough=0.95, mottle=0.25, grime=0.25, height=0.3),
              plain("bag_tan", (0.26, 0.21, 0.13), rough=0.95, mottle=0.25, grime=0.25, height=0.3)]
    parts = []
    bh = 0.26
    bl = 0.5
    for c in range(courses):
        z = c * bh * 0.95
        off = (bl / 2) if c % 2 else 0.0
        x = -L / 2 - off
        while x < L / 2 - 0.05:
            for yrow in ((-D / 4, D / 4) if D > 0.4 else (0.0,)):
                a, b = max(x, -L / 2), min(x + bl, L / 2)
                if b - a < 0.12:
                    continue
                cl = cloths[int(rng() * 2)]
                w = D / 2 if D > 0.4 else D
                bag = block("bag", (a + 0.01, yrow - w / 2 + 0.01, z), (b - 0.01, yrow + w / 2 - 0.01, z + bh), cl, 0.09, 3)
                # Squash and plump: the middle bulges, the top sags a little, a small random turn.
                for v in bag.data.vertices:
                    cx = (a + b) / 2
                    k = 1.0 - abs(v.co.x - cx) / ((b - a) / 2 + 1e-6)
                    v.co.y = yrow + (v.co.y - yrow) * (0.85 + 0.25 * k)
                    v.co.z -= 0.02 * k * (1 if v.co.z > z + bh / 2 else -0.5)
                rotated(bag, "Z", rng.range(-4, 4), ((a + b) / 2, yrow, z))
                parts.append(bag)
            x += bl
    return parts, {}


def wheel_set(name: str, x: float, y: float, r: float, w: float, tyre, hub, mudguard=None):
    """A wheel standing on z = 0 at (x, y), its axle along x: a treaded tyre, a hub with bolts, maybe a mudguard."""
    parts = [cylinder("tyre", (x, y, r), r, w, tyre, "X", 20)]
    # Tread: shallow blocks round the tyre.
    for i in range(28):
        a = math.tau * i / 28
        t = block("tread", (x - w / 2 + 0.02, -0.025, -0.02), (x + w / 2 - 0.02, 0.025, 0.0), tyre, 0.0)
        t.data.transform(Matrix.Translation(Vector((0, y, r))) @ Matrix.Rotation(a, 4, "X") @ Matrix.Translation(Vector((0, 0, r + 0.012))))
        parts.append(t)
    side = 1 if x > 0 else -1
    parts.append(cylinder("hub", (x + side * (w / 2 + 0.005), y, r), r * 0.55, 0.03, hub, "X", 16))
    parts.append(cylinder("cap", (x + side * (w / 2 + 0.03), y, r), r * 0.2, 0.05, hub, "X", 10))
    if mudguard is not None:
        guard = torus("guard", (0, 0, 0), r + 0.08, 0.02, mudguard, "X", 14, 4, 180)
        guard.data.transform(Matrix.Translation(Vector((x, y, r))) @ Matrix.Scale(1.0, 4))
        for v in guard.data.vertices:
            v.co.x = x + (v.co.x - x) * 1.0 + (0.0)
        parts.append(block("guard_top", (x - w / 2 - 0.03, y - r - 0.12, 2 * r + 0.05), (x + w / 2 + 0.03, y + r + 0.12, 2 * r + 0.08), mudguard, 0.0))
    return parts


def semi_truck(name: str, colour, trailer: str = "box", seed: int = 171):
    """A 16 m semi (its cab facing -y, the front of the cab at y = 0): a cab-over tractor with its windscreen,
    grille, lights and mirrors, chassis rails, and a trailer: a tarpaulin box body ("box") or a flatbed ("flat",
    for a container placed on it)."""
    paint = painted_wood("cab_paint", colour, wear=0.02, grain="acg:Concrete030", tile=2.0, rough=0.4, grime=0.35, height=1.2) if False else \
        plain("cab_paint", colour, rough=0.4, mottle=0.05, grime=0.35, height=1.3)
    dark = plain("dark", (0.025, 0.025, 0.025), rough=0.6, metal=0.2, mottle=0.2)
    steel = plain("steel", (0.12, 0.12, 0.12), rough=0.5, metal=0.5, mottle=0.2, grime=0.3, height=1.0)
    tyre = plain("tyre", (0.03, 0.03, 0.03), rough=0.9, mottle=0.1)
    hub = plain("hub", (0.3, 0.3, 0.28), rough=0.4, metal=0.6)
    g = glass("cab_glass", False)
    lamp = plain("lamp", (0.8, 0.8, 0.75), rough=0.1)
    W, cab_l = 2.45, 2.25
    parts = []
    # Cab body and roof.
    parts.append(block("cab", (-W / 2, -cab_l, 1.0), (W / 2, 0, 3.0), paint, 0.06, 2))
    parts.append(block("roof", (-W / 2 + 0.05, -cab_l + 0.05, 3.0), (W / 2 - 0.05, -0.1, 3.12), paint, 0.05, 2))
    # (the cab front is at -y: move everything so the front face is at y = -cab_l … build facing -y)
    parts.append(block("windscreen", (-W / 2 + 0.1, -cab_l - 0.005, 2.0), (W / 2 - 0.1, -cab_l + 0.02, 2.8), g, 0.0))
    parts.append(block("pillar", (-0.03, -cab_l - 0.01, 2.0), (0.03, -cab_l + 0.01, 2.8), dark, 0.0))
    for x in (-W / 2 - 0.005, W / 2 - 0.02):
        parts.append(block("side_win", (x, -cab_l + 0.15, 2.0), (x + 0.025, -1.0, 2.75), g, 0.0))
        parts.append(block("door_seam", (x, -1.0, 1.1), (x + 0.025, -0.97, 2.9), dark, 0.0))
    parts.append(block("visor", (-W / 2, -cab_l - 0.2, 2.82), (W / 2, -cab_l + 0.05, 2.9), paint, 0.02))
    # Grille, bumper, lights, steps.
    parts.append(block("grille", (-0.8, -cab_l - 0.015, 1.25), (0.8, -cab_l + 0.02, 1.85), dark, 0.01))
    for i in range(5):
        z = 1.3 + i * 0.11
        parts.append(block("slat", (-0.78, -cab_l - 0.03, z), (0.78, -cab_l - 0.01, z + 0.04), steel, 0.0))
    parts.append(block("bumper", (-W / 2 - 0.02, -cab_l - 0.1, 0.55), (W / 2 + 0.02, -cab_l + 0.3, 0.98), steel, 0.03))
    for x in (-0.95, 0.95):
        parts.append(block("headlight", (x - 0.2, -cab_l - 0.115, 0.75), (x + 0.2, -cab_l - 0.09, 0.92), lamp, 0.01))
        parts.append(block("step", (x - 0.25 if x < 0 else x - 0.1, -cab_l + 0.35, 0.45), (x + 0.1 if x < 0 else x + 0.25, -cab_l + 1.2, 0.5), steel, 0.0))
    # Mirrors on arms.
    for sx in (-1, 1):
        x = sx * (W / 2 + 0.22)
        parts.append(block("mirror_arm", (min(sx * W / 2, x), -cab_l + 0.18, 2.55), (max(sx * W / 2, x), -cab_l + 0.22, 2.58), dark, 0.0))
        parts.append(block("mirror", (x - 0.06, -cab_l + 0.12, 2.1), (x + 0.06, -cab_l + 0.22, 2.6), dark, 0.02))
    # Chassis rails and a fuel tank.
    L = 15.8
    for x in (-0.5, 0.4):
        parts.append(block("rail", (x, -cab_l, 0.55), (x + 0.1, L - cab_l, 0.85), dark, 0.0))
    parts.append(cylinder("fuel", (-W / 2 + 0.35, 0.8, 0.8), 0.3, 1.2, steel, "Y", 14))
    # Wheels: the steer axle, the tractor's drive axle, the trailer's three.
    for y in (-cab_l + 0.9, 1.9, 10.5, 11.8, 13.1):
        for x in (-W / 2 + 0.2, W / 2 - 0.2):
            parts += wheel_set("w", x, y, 0.52, 0.36, tyre, hub)
    if trailer == "box":
        tarp = plain("tarp", (0.12, 0.13, 0.07), rough=0.9, mottle=0.18, grime=0.3, height=1.5)
        parts.append(block("deck", (-W / 2, 0.4, 1.0), (W / 2, L - cab_l, 1.2), steel, 0.02))
        parts.append(block("box", (-W / 2 + 0.02, 0.45, 1.2), (W / 2 - 0.02, L - cab_l - 0.05, 3.75), tarp, 0.06, 2))
        n = int((L - cab_l - 0.5) / 1.3)
        for i in range(n + 1):
            y = 0.45 + (L - cab_l - 0.5) * i / n
            parts.append(block("rib", (-W / 2 - 0.01, y - 0.04, 1.2), (W / 2 + 0.01, y + 0.04, 3.77), tarp, 0.02))
        parts.append(block("rail_b", (-W / 2 - 0.02, 0.4, 1.2), (W / 2 + 0.02, L - cab_l, 1.35), steel, 0.01))
    else:
        parts.append(block("deck", (-W / 2, 0.4, 0.45), (W / 2, L - cab_l, 0.64), steel, 0.02))
        for x in (-W / 2, W / 2 - 0.08):
            parts.append(block("side_rail", (x, 0.4, 0.3), (x + 0.08, L - cab_l, 0.64), dark, 0.0))
    return parts, {"front": cab_l}


def spoked_wheel(name: str, x: float, y: float, r: float, wood, iron, spokes: int = 10):
    """A cart wheel standing at (x, y) on z = 0, its axle along x: an iron tyre, a felloe, a hub and spokes."""
    parts = [torus("tyre", (0, 0, 0), r - 0.012, 0.016, iron, "X", 24, 5), torus("felloe", (0, 0, 0), r - 0.045, 0.028, wood, "X", 24, 5),
             cylinder("hub", (0, 0, 0), 0.07, 0.16, wood, "X", 12)]
    for i in range(spokes):
        sp = block("spoke", (-0.015, -0.018, 0.05), (0.015, 0.018, r - 0.06), wood, 0.004)
        sp.data.transform(Matrix.Rotation(math.tau * i / spokes, 4, "X"))
        parts.append(sp)
    m = Matrix.Translation(Vector((x, y, r)))
    for p in parts:
        p.data.transform(m)
    return parts


def hand_cart(name: str, load: str = "empty", seed: int = 181):
    """A wooden hand cart (bed along y, shafts towards -y): a planked bed with low sides on two spoked wheels, two
    shafts resting on the ground, a prop leg; `load`: "empty", "sacks" or "barrels"."""
    rng = Rng(seed)
    wood = textured("cart", "medieval_wood", 1.2, tint=(0.9, 0.8, 0.68), grime=0.35, height=0.6)
    iron = plain("iron", (0.05, 0.045, 0.04), rough=0.55, metal=0.5, mottle=0.25)
    parts = []
    L, W, bz = 1.8, 1.1, 0.62
    # The bed tilts forward: the wheels are at the back third, the shafts touch the ground.
    for i in range(6):
        x0 = -W / 2 + W * i / 6 + 0.005
        parts.append(block("plank", (x0, -L / 2, bz), (x0 + W / 6 - 0.01, L / 2, bz + 0.04), wood, 0.004))
    for x in (-W / 2, W / 2 - 0.04):
        parts.append(block("side", (x, -L / 2, bz + 0.04), (x + 0.04, L / 2, bz + 0.3), wood, 0.006))
    for y in (-L / 2, L / 2 - 0.04):
        parts.append(block("end", (-W / 2, y, bz + 0.04), (W / 2, y + 0.04, bz + 0.28), wood, 0.006))
    for x in (-W / 2 + 0.1, W / 2 - 0.14):
        parts.append(block("shaft", (x, -L / 2 - 1.3, bz - 0.06), (x + 0.05, L / 2, bz), wood, 0.006))
    parts.append(block("axle", (-W / 2 - 0.1, 0.25, 0.43), (W / 2 + 0.1, 0.33, 0.49), iron, 0.004))
    for x in (-W / 2 - 0.08, W / 2 + 0.08):
        parts += spoked_wheel("wheel", x, 0.29, 0.46, wood, iron)
    # Tilt the whole cart down at the front so the shafts reach the ground.
    tilt = math.atan2(bz - 0.03, L / 2 + 1.3 + 0.29)
    for p in parts:
        if p.name.startswith(("tyre", "felloe", "hub", "spoke")):
            continue
        p.data.transform(Matrix.Translation(Vector((0, 0.29, 0.46))) @ Matrix.Rotation(tilt, 4, "X") @ Matrix.Translation(Vector((0, -0.29, -0.46))))
    if load == "sacks":
        sack = plain("sack", (0.36, 0.3, 0.2), rough=0.95, mottle=0.2)
        for i in range(5):
            c = (rng.range(-0.3, 0.3), rng.range(-0.5, 0.6), bz + 0.2 + (0.15 if i > 2 else 0))
            parts.append(blob("sack", c, 0.22, sack, 2, 0.55))
    elif load == "barrels":
        staves = textured("barrel", "medieval_wood", 0.8, tint=(0.85, 0.72, 0.58))
        for i, y in enumerate((-0.4, 0.3)):
            b = cylinder("barrel", (0, y, bz + 0.35), 0.28, 0.62, staves, "X", 16)
            parts.append(b)
            for dx in (-0.2, 0.2):
                parts.append(torus("hoop", (dx, y, bz + 0.35), 0.285, 0.012, iron, "X", 16, 4))
    return parts, {}


def panel_van(name: str, colour, seed: int = 191):
    """A panel van 5.1 m long (front towards -y, its nose at y = -L/2): a boxy body with rounded edges, a sloped
    bonnet and windscreen, cab side windows, sliding-door and rear-door seams, bumpers, lights, mirrors, four
    wheels under arches."""
    paint = plain("van_paint", colour, rough=0.35, mottle=0.05, grime=0.4, height=0.7)
    dark = plain("dark", (0.03, 0.03, 0.03), rough=0.6, metal=0.2, mottle=0.15)
    steel = plain("steel", (0.25, 0.25, 0.24), rough=0.4, metal=0.6)
    tyre = plain("tyre", (0.03, 0.03, 0.03), rough=0.9, mottle=0.1)
    hub = plain("hub", (0.45, 0.45, 0.43), rough=0.35, metal=0.7)
    g = glass("van_glass", False)
    lamp = plain("lamp", (0.85, 0.85, 0.8), rough=0.1)
    tail = plain("tail", (0.5, 0.03, 0.02), rough=0.2)
    L, W, H = 5.1, 1.95, 2.3
    y0, y1 = -L / 2, L / 2
    parts = []
    # The load box, and the cab ahead of it (lower bonnet, then the windscreen slope).
    parts.append(block("box", (-W / 2, y0 + 0.95, 0.45), (W / 2, y1, H), paint, 0.12, 3))
    parts.append(block("bonnet", (-W / 2 + 0.02, y0 + 0.05, 0.45), (W / 2 - 0.02, y0 + 1.1, 1.15), paint, 0.12, 3))
    # The windscreen: a slab sloping from the bonnet's back edge up to the roof, framed by A-pillars.
    def slope(ob, ya, za, yb, zb):
        for v in ob.data.vertices:
            t = (v.co.z - za) / (zb - za)
            v.co.y += (ya + (yb - ya) * t) - ya
    screen = block("windscreen", (-W / 2 + 0.1, y0 + 0.95, 1.15), (W / 2 - 0.1, y0 + 0.98, H - 0.08), g, 0.0)
    slope(screen, y0 + 0.95, 1.15, y0 + 1.25, H - 0.08)
    screen.data.transform(Matrix.Translation(Vector((0, -0.3, 0))))
    parts.append(screen)
    for sx in (-1, 1):
        pil = block("pillar", (sx * (W / 2 - 0.05) - 0.06, y0 + 0.94, 1.15), (sx * (W / 2 - 0.05) + 0.06, y0 + 1.0, H - 0.02), paint, 0.02)
        slope(pil, y0 + 0.94, 1.15, y0 + 1.24, H - 0.02)
        pil.data.transform(Matrix.Translation(Vector((0, -0.3, 0))))
        parts.append(pil)
    parts.append(block("roof_lip", (-W / 2 + 0.05, y0 + 0.9, H - 0.1), (W / 2 - 0.05, y0 + 1.1, H), paint, 0.05))
    for sx in (-1, 1):
        x = sx * (W / 2 + 0.005)
        parts.append(block("side_win", (min(x, x - sx * 0.02), y0 + 1.15, 1.3), (max(x, x - sx * 0.02), y0 + 1.95, H - 0.2), g, 0.0))
        for yy in (y0 + 2.0, y0 + 3.3, y0 + 1.05):
            parts.append(block("seam", (min(x, x - sx * 0.02), yy, 0.5), (max(x, x - sx * 0.02), yy + 0.02, H - 0.1), dark, 0.0))
        parts.append(block("rail", (min(x, x + sx * 0.02), y0 + 2.05, H - 0.35), (max(x, x + sx * 0.02), y0 + 3.3, H - 0.32), dark, 0.0))
        parts.append(block("mirror", (sx * (W / 2 + 0.12) - 0.05, y0 + 1.1, 1.55), (sx * (W / 2 + 0.12) + 0.05, y0 + 1.2, 1.85), dark, 0.02))
        # Wheel arches: dark insets on the sides.
        for wy in (y0 + 0.75, y1 - 0.9):
            parts.append(block("arch", (min(x, x - sx * 0.02), wy - 0.45, 0.25), (max(x, x - sx * 0.02), wy + 0.45, 0.8), dark, 0.05))
            parts += wheel_set("w", sx * (W / 2 - 0.2), wy, 0.36, 0.24, tyre, hub)
    parts.append(block("rear_seam", (-0.01, y1 - 0.005, 0.5), (0.01, y1 + 0.01, H - 0.1), dark, 0.0))
    parts.append(block("bumper_f", (-W / 2, y0 - 0.06, 0.35), (W / 2, y0 + 0.2, 0.62), dark, 0.04))
    parts.append(block("bumper_r", (-W / 2, y1 - 0.15, 0.35), (W / 2, y1 + 0.06, 0.55), dark, 0.04))
    parts.append(block("grille", (-0.55, y0 + 0.03, 0.7), (0.55, y0 + 0.06, 1.0), dark, 0.01))
    for sx in (-1, 1):
        parts.append(block("head", (sx * 0.75 - 0.18, y0 + 0.02, 0.8), (sx * 0.75 + 0.18, y0 + 0.06, 1.0), lamp, 0.01))
        parts.append(block("tail", (sx * (W / 2 - 0.1) - 0.08, y1 - 0.01, 0.7), (sx * (W / 2 - 0.1) + 0.08, y1 + 0.02, 1.2), tail, 0.01))
    del steel, seed
    return parts, {}


def pallet(name: str, load: str | None = None, seed: int = 201):
    """A Euro pallet (1.2 × 0.8 m... here 1.22 × 1.02 to match the maps' 48 × 40 units, 0.15 m high): bottom
    boards, nine blocks, stringer boards and top deck slats; `load`: None, "boxes" (cardboard boxes stacked a metre
    high under clear wrap) or "sacks"."""
    rng = Rng(seed)
    wood = textured("pallet", "wood_shutter", 0.7, tint=(1.3, 1.15, 0.9), grime=0.3, height=0.15)
    L, W, H = 1.22, 1.02, 0.15
    parts = []
    for y in (-W / 2, -0.05, W / 2 - 0.1):
        parts.append(block("bottom", (-L / 2, y, 0.0), (L / 2, y + 0.1, 0.022), wood, 0.003))
    for x in (-L / 2, -0.05, L / 2 - 0.1):
        for y in (-W / 2, -0.05, W / 2 - 0.1):
            parts.append(block("blk", (x, y, 0.022), (x + 0.1, y + 0.1, 0.1), wood, 0.004))
    for y in (-W / 2, -0.05, W / 2 - 0.1):
        parts.append(block("stringer", (-L / 2, y, 0.1), (L / 2, y + 0.1, 0.122), wood, 0.003))
    n = 7
    for i in range(n):
        x = -L / 2 + (L - 0.1) * i / (n - 1)
        parts.append(block("slat", (x, -W / 2, 0.122), (x + 0.1, W / 2, H), wood, 0.003))
    if load == "boxes":
        card = plain("card", (0.42, 0.3, 0.17), rough=0.85, mottle=0.12, grime=0.2, height=1.2)
        tape = plain("tape", (0.55, 0.45, 0.3), rough=0.5)
        rows = 3
        for zi in range(rows):
            z = H + zi * 0.33
            for xi in range(3):
                for yi in range(2):
                    x0 = -L / 2 + 0.02 + xi * 0.4
                    y0 = -W / 2 + 0.02 + yi * 0.49
                    bx = block("box", (x0, y0, z), (x0 + 0.39 - rng.range(0, 0.02), y0 + 0.48, z + 0.32), card, 0.008)
                    parts.append(bx)
                    parts.append(block("tape", (x0 + 0.17, y0 - 0.002, z + 0.2), (x0 + 0.22, y0 + 0.482, z + 0.322), tape, 0.0))
    elif load == "sacks":
        sack = plain("sack", (0.36, 0.31, 0.22), rough=0.95, mottle=0.25, grime=0.2, height=0.6)
        for zi in range(4):
            for j in range(2):
                c = (rng.range(-0.03, 0.03), -W / 4 + j * W / 2 + rng.range(-0.02, 0.02), H + 0.1 + zi * 0.19)
                sk = block("sack", (c[0] - 0.56, c[1] - 0.23, c[2] - 0.095), (c[0] + 0.56, c[1] + 0.23, c[2] + 0.095), sack, 0.07, 2)
                rotated(sk, "Z", rng.range(-3, 3), c)
                parts.append(sk)
    return parts, {}


def t_wall(name: str, seed: int = 211, W: float = 1.22, H: float = 2.24, D: float = 1.0):
    """A precast concrete T-wall segment (a blast wall's unit): a tapering slab on a T footing, a chamfered top, two
    lifting loops, a stencilled number; weathered concrete with rain streaks and dirt at the foot."""
    rng = Rng(seed)
    conc = textured("tconc", "concrete_wall_006", 1.6, tint=(1.12, 1.1, 1.05), grime=0.45, height=0.6)
    iron = plain("loop", (0.2, 0.12, 0.07), rough=0.7, metal=0.3, mottle=0.3)
    parts = []
    parts.append(block("foot", (-W / 2 + 0.005, -D / 2, 0), (W / 2 - 0.005, D / 2, 0.28), conc, 0.02, 2))
    slab = block("slab", (-W / 2 + 0.005, -0.14, 0.28), (W / 2 - 0.005, 0.14, H), conc, 0.02, 2)
    for v in slab.data.vertices:
        if v.co.z > H - 0.01:
            v.co.y *= 0.62
    parts.append(slab)
    # fillets where the slab meets the foot
    for sy in (-1, 1):
        f = block("fillet", (-W / 2 + 0.01, sy * 0.14 if sy > 0 else -0.3, 0.28), (W / 2 - 0.01, 0.3 if sy > 0 else -0.14, 0.4), conc, 0.0)
        for v in f.data.vertices:
            if v.co.z > 0.39 and abs(v.co.y) > 0.2:
                v.co.y = sy * 0.145
        parts.append(f)
    for x in (-0.3, 0.3):
        parts.append(torus("lift", (x, 0, H + 0.02), 0.06, 0.012, iron, "Y", 10, 4, 180))
    del rng
    return parts, {}


def fuel_bowser(name: str, colour, seed: int = 221):
    """An airfield fuel bowser (front towards -y): a cab-over cab, a chassis, an elliptical tank with dished ends,
    a walkway and handrail on top, hose reels and a pump cabinet at the back, six wheels."""
    paint = plain("bowser_paint", colour, rough=0.4, mottle=0.06, grime=0.4, height=1.0)
    dark = plain("dark", (0.03, 0.03, 0.03), rough=0.6, metal=0.2, mottle=0.15)
    steel = plain("steel", (0.35, 0.35, 0.34), rough=0.4, metal=0.6, mottle=0.1)
    tyre = plain("tyre", (0.03, 0.03, 0.03), rough=0.9, mottle=0.1)
    hub = plain("hub", (0.45, 0.45, 0.43), rough=0.35, metal=0.7)
    red = plain("red", (0.45, 0.03, 0.02), rough=0.4)
    g = glass("cab_glass", False)
    W, L = 2.45, 10.5
    y0 = -L / 2
    parts = []
    # Cab.
    parts.append(block("cab", (-W / 2, y0, 1.0), (W / 2, y0 + 2.0, 3.0), paint, 0.08, 2))
    parts.append(block("screen", (-W / 2 + 0.1, y0 - 0.005, 2.0), (W / 2 - 0.1, y0 + 0.02, 2.8), g, 0.0))
    parts.append(block("grille", (-0.8, y0 - 0.015, 1.2), (0.8, y0 + 0.02, 1.8), dark, 0.01))
    parts.append(block("bumper", (-W / 2, y0 - 0.1, 0.55), (W / 2, y0 + 0.2, 0.95), dark, 0.03))
    for sx in (-1, 1):
        parts.append(block("side_win", (sx * W / 2 - 0.013, y0 + 0.2, 2.0), (sx * W / 2 + 0.013, y0 + 1.2, 2.75), g, 0.0))
        parts.append(block("mirror", (sx * (W / 2 + 0.2) - 0.05, y0 + 0.15, 2.1), (sx * (W / 2 + 0.2) + 0.05, y0 + 0.25, 2.6), dark, 0.02))
        parts.append(block("beacon", (sx * 0.6 - 0.08, y0 + 0.8, 3.0), (sx * 0.6 + 0.08, y0 + 0.96, 3.12), plain("amber", (0.9, 0.45, 0.02), rough=0.2), 0.02))
    for x in (-0.5, 0.4):
        parts.append(block("rail", (x, y0, 0.6), (x + 0.1, L / 2, 0.9), dark, 0.0))
    # Tank: an elliptical cylinder along y with dished ends.
    tank = cylinder("tank", (0, 1.0, 2.05), 1.1, 6.6, paint, "Y", 28)
    for v in tank.data.vertices:
        v.co.x = v.co.x * 1.1
    parts.append(tank)
    for yy, sgn in ((1.0 - 3.3, -1), (1.0 + 3.3, 1)):
        cap = blob("end", (0, yy, 2.05), 1.1, paint, 3, 1.0)
        for v in cap.data.vertices:
            v.co.x *= 1.1
            v.co.y = yy + (v.co.y - yy) * 0.25
            if (v.co.y - yy) * sgn < 0:
                v.co.y = yy
        parts.append(cap)
    for yy in (-1.3, 1.0, 3.3):
        parts.append(torus("band", (0, yy, 2.05), 1.12, 0.03, paint, "Y", 28, 4))
        for v in parts[-1].data.vertices:
            v.co.x *= 1.1
    # Walkway and rail on top, manholes.
    parts.append(block("walk", (-0.35, -2.2, 3.15), (0.35, 4.2, 3.2), steel, 0.0))
    for x in (-0.33, 0.33):
        for yy in (-2.1, -0.6, 0.9, 2.4, 4.1):
            parts.append(block("post", (x - 0.015, yy - 0.015, 3.2), (x + 0.015, yy + 0.015, 3.9), plain("yellow", (0.7, 0.55, 0.05), rough=0.5), 0.0))
        parts.append(block("hand", (x - 0.02, -2.1, 3.88), (x + 0.02, 4.1, 3.92), plain("yellow", (0.7, 0.55, 0.05), rough=0.5), 0.0))
    for yy in (-0.8, 1.2, 3.0):
        parts.append(cylinder("manhole", (0, yy, 3.15), 0.25, 0.1, steel, "Z", 16))
    # Rear pump cabinet and hose reels.
    parts.append(block("cabinet", (-W / 2, 4.4, 0.9), (W / 2, L / 2, 2.2), paint, 0.04))
    hose = plain("hose", (0.04, 0.04, 0.04), rough=0.7, mottle=0.1)
    for sx in (-1, 1):
        # a hose reel: two flanges and the coiled hose between, in an open bay of the cabinet
        for dy in (0.02, 0.28):
            parts.append(cylinder("flange", (sx * 0.55, L / 2 - 0.3 + dy, 1.5), 0.3, 0.02, steel, "Y", 16))
        parts.append(cylinder("coil", (sx * 0.55, L / 2 - 0.15, 1.5), 0.24, 0.24, hose, "Y", 16))
    parts.append(block("bay", (-W / 2 + 0.05, L / 2 - 0.02, 1.1), (W / 2 - 0.05, L / 2 + 0.005, 1.95), dark, 0.0))
    parts.append(block("fire_ext", (W / 2 - 0.02, 4.0, 1.0), (W / 2 + 0.1, 4.2, 1.5), red, 0.02))
    for yy in (y0 + 1.0, 2.8, 4.1):
        for sx in (-1, 1):
            parts += wheel_set("w", sx * (W / 2 - 0.2), yy, 0.5, 0.35, tyre, hub)
    del seed
    return parts, {}


def box_wagon(name: str, colour, seed: int = 231, L: float = 14.0):
    """A covered freight wagon along y: an underframe, two bogies with wheelsets, buffers and couplers at the ends,
    a ribbed body with two big sliding doors a side, a curved roof, grab irons and a stencil panel."""
    paint = container_paint("wagon_paint", colour, seed)
    dark = plain("dark", (0.04, 0.035, 0.03), rough=0.6, metal=0.3, mottle=0.25, grime=0.4, height=1.0)
    steel = plain("steel", (0.3, 0.29, 0.27), rough=0.45, metal=0.6, mottle=0.2)
    roof_mat = plain("roof", (0.3, 0.3, 0.28), rough=0.6, metal=0.2, mottle=0.25, grime=0.3, height=3.8)
    W, zb, H = 2.8, 1.15, 3.9
    y0, y1 = -L / 2, L / 2
    parts = []
    # Underframe and headstocks.
    parts.append(block("frame", (-W / 2 + 0.1, y0 + 0.2, 0.95), (W / 2 - 0.1, y1 - 0.2, zb), dark, 0.02))
    for yy in (y0, y1 - 0.25):
        parts.append(block("headstock", (-W / 2, yy, 0.85), (W / 2, yy + 0.25, zb + 0.05), dark, 0.02))
    # Buffers and couplers.
    for yy, sgn in ((y0, -1), (y1, 1)):
        for x in (-0.87, 0.87):
            parts.append(cylinder("buffer", (x, yy + sgn * 0.3, 1.05), 0.1, 0.6, steel, "Y", 12))
            parts.append(cylinder("head", (x, yy + sgn * 0.6, 1.05), 0.2, 0.05, steel, "Y", 16))
        parts.append(block("hook", (-0.06, yy + sgn * 0.45 - 0.1, 0.95), (0.06, yy + sgn * 0.45 + 0.1, 1.1), dark, 0.01))
    # Bogies: side frames and wheels.
    for by in (y0 + 2.3, y1 - 2.3):
        for x in (-0.82, 0.82):
            parts.append(block("sideframe", (x - 0.06, by - 1.3, 0.35), (x + 0.06, by + 1.3, 0.75), dark, 0.03))
        for wy in (by - 0.9, by + 0.9):
            for x in (-0.72, 0.72):
                parts.append(cylinder("wheel", (x, wy, 0.46), 0.46, 0.14, steel, "X", 20))
            parts.append(cylinder("axle", (0, wy, 0.46), 0.08, 1.6, dark, "X", 8))
    # The body: ribbed sides, plain ends.
    for x, out in ((-W / 2 + 0.01, -1), (W / 2 - 0.01, 1)):
        parts.append(corrugated("side", y0 + 0.1, y1 - 0.1, zb, H - 0.25, x, out, "y", paint, pitch=0.45, depth=0.04))
        # Two sliding doors a side (proud of the ribs), their runners and handles.
        for dy in (-3.4, 3.4):
            d0, d1 = dy - 1.3, dy + 1.3
            dx = x + out * 0.06
            parts.append(block("door", (min(x, dx), d0, zb + 0.1), (max(x, dx), d1, H - 0.35), paint, 0.02))
            parts.append(block("runner", (min(x, x + out * 0.1), d0 - 0.3, H - 0.4), (max(x, x + out * 0.1), d1 + 0.3, H - 0.3), dark, 0.0))
            parts.append(block("handle", (min(dx, dx + out * 0.05), dy - 0.05, 1.8), (max(dx, dx + out * 0.05), dy + 0.05, 2.5), steel, 0.01))
        parts.append(block("stencil", (min(x, x + out * 0.01), -1.1, 2.9), (max(x, x + out * 0.01), 1.1, 3.35), plain("panel", (0.75, 0.73, 0.68), rough=0.5), 0.0))
    for yy in (y0 + 0.1, y1 - 0.12):
        parts.append(block("endwall", (-W / 2, yy, zb), (W / 2, yy + 0.02, H - 0.25), paint, 0.0))
    # Curved roof: a flattened half-cylinder along y.
    roof = cylinder("roof", (0, 0, H - 0.25), W / 2 + 0.03, L - 0.1, roof_mat, "Y", 24)
    for v in roof.data.vertices:
        v.co.z = H - 0.25 + max(0.0, v.co.z - (H - 0.25)) * 0.28
    parts.append(roof)
    del seed
    return parts, {}


def octagon(name: str, r_out: float, r_in: float, z0: float, z1: float, mat, sides: int = 8):
    """An octagonal ring (r_in 0: a solid prism) from z0 to z1, flat sides facing the axes."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    rot = math.pi / sides
    def ring(r, z):
        return [bm.verts.new((r * math.cos(rot + math.tau * i / sides), r * math.sin(rot + math.tau * i / sides), z)) for i in range(sides)]
    ob, ot = ring(r_out, z0), ring(r_out, z1)
    for i in range(sides):
        j = (i + 1) % sides
        bm.faces.new((ob[i], ob[j], ot[j], ot[i]))
    if r_in > 0:
        ib, it = ring(r_in, z0), ring(r_in, z1)
        for i in range(sides):
            j = (i + 1) % sides
            bm.faces.new((ib[j], ib[i], it[i], it[j]))
            bm.faces.new((ot[i], ot[j], it[j], it[i]))
    else:
        bm.faces.new(list(reversed(ob)))
        bm.faces.new(ot)
    bm.normal_update()
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(obj)
    me.materials.append(mat)
    return obj


def fountain(name: str, seed: int = 241, R: float = 3.1):
    """A town fountain: an octagonal basin of dressed sandstone (a plinth, panelled sides, a moulded rim), water in
    it, a central column on a pedestal with a bowl, four lion-head spouts (simple masks) and a finial."""
    stone = textured("fstone", "acg:Concrete030", 1.4, tint=(1.12, 1.02, 0.86), grime=0.4, height=0.9)
    dark_stone = textured("fstone_d", "acg:Concrete030", 1.4, tint=(0.9, 0.82, 0.7), grime=0.5, height=0.5)
    water = glass("water", False)
    bronze = plain("bronze", (0.12, 0.2, 0.15), rough=0.4, metal=0.7, mottle=0.3)
    parts = [octagon("plinth", R + 0.1, 0, 0.0, 0.12, dark_stone),
             octagon("wall", R, R - 0.25, 0.12, 0.8, stone),
             octagon("rim", R + 0.08, R - 0.32, 0.8, 0.95, stone),
             octagon("rim_top", R + 0.02, R - 0.28, 0.95, 1.0, stone),
             # (the basin's floor, deep under the map's own water surface, which the level shader ripples)
             octagon("floor", R - 0.25, 0, 0.12, 0.3, dark_stone)]
    # Panels on the basin's sides (raised fields).
    for i in range(8):
        a = math.pi / 8 + math.tau * i / 8 + math.pi / 8
        a = math.tau * i / 8
        c = Vector((math.cos(a), math.sin(a), 0)) * (R * math.cos(math.pi / 8) + 0.012)
        side = 2 * R * math.sin(math.pi / 8)
        pnl = block("panel", (-side * 0.35, -0.02, 0.25), (side * 0.35, 0.0, 0.68), stone, 0.015, 2)
        pnl.data.transform(Matrix.Translation(c) @ Matrix.Rotation(a + math.pi / 2, 4, "Z"))
        parts.append(pnl)
    # Central column: pedestal, shaft, bowl, finial.
    parts.append(octagon("ped", 0.55, 0, 0.12, 0.9, stone))
    parts.append(octagon("ped_cap", 0.62, 0, 0.9, 1.0, stone))
    parts.append(cylinder("shaft", (0, 0, 1.75), 0.22, 1.5, stone, "Z", 12))
    bowl = cylinder("bowl", (0, 0, 2.55), 0.9, 0.2, stone, "Z", 16)
    for v in bowl.data.vertices:
        if v.co.z < 2.55:
            v.co.x *= 0.5
            v.co.y *= 0.5
    parts.append(bowl)
    parts.append(cylinder("bowl_water", (0, 0, 2.63), 0.8, 0.02, water, "Z", 16))
    parts.append(octagon("fin_base", 0.25, 0, 2.65, 2.85, stone))
    parts.append(blob("finial", (0, 0, 3.1), 0.22, bronze, 2, 1.4))
    parts.append(cylinder("spike", (0, 0, 3.45), 0.03, 0.35, bronze, "Z", 6))
    # Spouts: bronze masks with a pipe, on the pedestal.
    for i in range(4):
        a = math.tau * i / 4
        d = Vector((math.cos(a), math.sin(a), 0))
        parts.append(blob("mask", tuple(d * 0.6 + Vector((0, 0, 0.75))), 0.12, bronze, 2, 1.1))
        pipe = cylinder("pipe", (0, 0, 0), 0.025, 0.25, bronze, "X", 6)
        pipe.data.transform(Matrix.Translation(d * 0.8 + Vector((0, 0, 0.72))) @ Matrix.Rotation(a, 4, "Z"))
        parts.append(pipe)
    del seed
    return parts, {}


def arch_strip(name: str, x0: float, x1: float, z_spring: float, depth: tuple, width: float, mat, n: int = 10):
    """A semicircular band over x0..x1 springing at z_spring: `width` wide (radially), from y depth[0] to depth[1]."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    xc, r = (x0 + x1) / 2, (x1 - x0) / 2
    rows = []
    for i in range(n + 1):
        a = math.pi * i / n
        c, s_ = math.cos(a), math.sin(a)
        rows.append([bm.verts.new((xc + (r + dr) * c, y, z_spring + (r + dr) * s_)) for dr in (0.0, width) for y in depth])
    for r0, r1 in zip(rows, rows[1:]):
        # inner, outer, front, back faces
        bm.faces.new((r0[0], r1[0], r1[1], r0[1]))
        bm.faces.new((r0[2], r0[3], r1[3], r1[2]))
        bm.faces.new((r0[0], r0[2], r1[2], r1[0]))
        bm.faces.new((r0[1], r1[1], r1[3], r0[3]))
    bm.normal_update()
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    return ob


def church_window(name: str, W: float = 1.6, H: float = 4.8, seed: int = 251):
    """A tall round-headed church window over a W × H opening: a stone surround with an arched head and a keystone,
    a deep sill, leaded glass in a diamond lattice with a tinted border."""
    stone = textured("stone", "acg:Concrete030", 1.6, tint=(1.1, 1.03, 0.9), grime=0.25, height=0.4)
    lead = plain("lead", (0.05, 0.05, 0.05), rough=0.5, metal=0.4)
    mat, nt, p = _new_mat("leaded")
    coord = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Generated"], sep.inputs["Vector"])
    streak = _ramp(nt, _noise(nt, 6.0, 3.0).outputs["Fac"], 0.3, 0.8)
    col = _mix(nt, streak, (0.06, 0.07, 0.08), (0.2, 0.22, 0.2))
    # a warm tinted border
    edge = _ramp(nt, _noise(nt, 40.0, 1.0).outputs["Fac"], 0.2, 0.9)
    col = _mix(nt, edge, col, (0.14, 0.12, 0.07))
    # Lead cames: a diamond lattice (two sets of thin bands crossing at ±30°), in object space.
    for ang in (math.radians(60), math.radians(-60)):
        m = nt.nodes.new("ShaderNodeMapping")
        m.inputs["Rotation"].default_value = (0.0, ang, 0.0)
        nt.links.new(coord.outputs["Object"], m.inputs["Vector"])
        w = nt.nodes.new("ShaderNodeTexWave")
        w.wave_type = "BANDS"
        w.bands_direction = "X"
        w.inputs["Scale"].default_value = 1.6
        w.inputs["Distortion"].default_value = 0.0
        nt.links.new(m.outputs["Vector"], w.inputs["Vector"])
        line = _ramp(nt, w.outputs["Fac"], 0.06, 0.02)
        col = _mix(nt, line, col, (0.02, 0.02, 0.02))
    nt.links.new(col, p.inputs["Base Color"])
    p.inputs["Roughness"].default_value = 0.12
    s_, d = 0.2, 0.12
    parts = []
    spring = H - W / 2
    parts.append(block("jamb_l", (-W / 2 - s_, -d, 0), (-W / 2, 0, spring), stone, 0.01, 2))
    parts.append(block("jamb_r", (W / 2, -d, 0), (W / 2 + s_, 0, spring), stone, 0.01, 2))
    parts.append(arch_strip("head", -W / 2, W / 2, spring, (-d, 0.0), s_, stone, 12))
    parts.append(block("key", (-0.1, -d - 0.03, H - 0.02), (0.1, 0.0, H + s_ + 0.04), stone, 0.01, 2))
    parts.append(block("sill", (-W / 2 - s_ - 0.05, -0.2, -0.12), (W / 2 + s_ + 0.05, 0, 0.0), stone, 0.015, 2))
    # The glass: rectangle up to the springing plus a half-disc above.
    parts.append(block("glass", (-W / 2, -0.06, 0), (W / 2, -0.055, spring), mat, 0.0))
    top = cylinder("glass_top", (0, -0.0575, spring), W / 2, 0.005, mat, "Y", 16)
    for v in top.data.vertices:
        v.co.z = spring + max(0.0, v.co.z - spring)
    parts.append(top)
    # A stone mullion splitting the light.
    parts.append(block("mullion", (-0.05, -0.1, 0), (0.05, -0.04, spring), stone, 0.01))
    # A dark reveal.
    parts.append(block("rev", (-W / 2, -0.05, 0), (W / 2, 0.0, spring), plain("rev", (0.15, 0.14, 0.13), rough=0.9), 0.0))
    del seed
    return parts, {"open": [W, H], "open_z": 0.0}


def church_door(name: str, W: float = 3.6, H: float = 4.6, seed: int = 261):
    """A church portal's double door under a round arch: heavy vertical boards with iron straps and studs, a stone
    surround, the arch's tympanum in stone."""
    wood = textured("oak", "medieval_wood", 1.8, tint=(0.75, 0.58, 0.42), grime=0.3, height=1.2)
    iron = plain("iron", (0.03, 0.03, 0.03), rough=0.5, metal=0.6, mottle=0.2)
    stone = textured("stone", "acg:Concrete030", 1.6, tint=(1.1, 1.03, 0.9), grime=0.3, height=0.6)
    spring = H - W / 2
    parts = []
    n = 12
    for i in range(n):
        x0 = -W / 2 + W * i / n + 0.004
        b = block("board", (x0, -0.1, 0.0), (x0 + W / n - 0.008, -0.02, spring), wood, 0.006)
        parts.append(b)
    parts.append(block("gap", (-0.015, -0.11, 0), (0.015, -0.09, spring), iron, 0.0))
    tymp = cylinder("tympanum", (0, -0.06, spring), W / 2, 0.08, stone, "Y", 20)
    for v in tymp.data.vertices:
        v.co.z = spring + max(0.0, v.co.z - spring)
    parts.append(tymp)
    for z in (0.5, 1.6, 2.7):
        if z < spring:
            for sx in (-1, 1):
                parts.append(block("strap", (sx * 0.06 if sx > 0 else -W / 2 + 0.05, -0.13, z), (W / 2 - 0.05 if sx > 0 else -0.06, -0.1, z + 0.08), iron, 0.005))
    for z in (0.3, 0.9, 1.5, 2.1):
        for i in range(n):
            if z < spring:
                parts.append(cylinder("stud", (-W / 2 + W * (i + 0.5) / n, -0.105, z), 0.018, 0.02, iron, "Y", 6))
    for sx in (-1, 1):
        parts.append(torus("ring", (sx * 0.25, -0.14, 1.2), 0.1, 0.012, iron, "Y", 12, 4))
    parts.append(arch_strip("arch", -W / 2, W / 2, spring, (-0.16, 0.0), 0.28, stone, 14))
    for x in (-W / 2 - 0.28, W / 2):
        parts.append(block("jamb", (x, -0.16, 0), (x + 0.28, 0, spring), stone, 0.012, 2))
    del seed
    return parts, {"open": [W, H], "open_z": 0.0}


# ---- the set ---------------------------------------------------------------------------------------------------

PIECES = {
    # Town-house windows (Altstadt): shutters in the town's colours, some with flowers, some bare with a pediment.
    "win_town_green": lambda n: window_town(n, "green", seed=11),
    "win_town_brown": lambda n: window_town(n, "brown", seed=12, curtain=False),
    "win_town_blue": lambda n: window_town(n, "blue", seed=13),
    "win_town_grey": lambda n: window_town(n, "grey", seed=14, curtain=False),
    "win_town_red": lambda n: window_town(n, "red", seed=15),
    "win_town_green_flowers": lambda n: window_town(n, "green", flowers=True, seed=16),
    "win_town_brown_flowers": lambda n: window_town(n, "brown", flowers=True, seed=17, curtain=False),
    "win_town_bare": lambda n: window_town(n, None, pediment=True, seed=18),
    "win_town_bare_flowers": lambda n: window_town(n, None, flowers=True, pediment=True, seed=19, surround="sandstone"),
    "door_town_brown": lambda n: door_town(n, "brown", seed=21),
    "door_town_green": lambda n: door_town(n, "green", seed=22, double=False),
    "door_town_wood": lambda n: door_town(n, None, seed=23),
    "balcony_iron": lambda n: balcony(n, seed=31),
    # Shop fronts (the window; the door is a door piece) and awnings, in three widths.
    **{f"shop_town_{w}_{c}": (lambda n, w=w, c=c, i=i: shopfront(n, w / 10, c, 51 + i)) for i, (w, c) in enumerate(
        [(w, c) for w in (20, 30, 40) for c in ("green", "brown", "blue", "red")])},
    **{f"awning_{w}_{c}": (lambda n, w=w, c=c, i=i: awning(n, w / 10, c, 71 + i)) for i, (w, c) in enumerate(
        [(w, c) for w in (24, 34, 44) for c in AWNINGS])},
    # Street furniture (Altstadt squares).
    "cafe_set": lambda n: cafe_set(n, None, 81),
    "cafe_set_red": lambda n: cafe_set(n, "red", 82),
    "cafe_set_green": lambda n: cafe_set(n, "green", 83),
    "cafe_set_ochre": lambda n: cafe_set(n, "ochre", 84),
    "market_stall_red": lambda n: market_stall(n, "red", 91),
    "market_stall_green": lambda n: market_stall(n, "green", 92),
    "market_stall_blue": lambda n: market_stall(n, "blue", 93),
    "planter_stone": lambda n: planter_stone(n, 101),
    "sign_bakery": lambda n: bakery_sign(n, 111),
    # Office and factory windows (Fliegerhorst, Hafen): three shapes each, picked by the opening's aspect.
    "win_office_tall": lambda n: window_office(n, 1.0, 1.4, "office", 121),
    "win_office_sq": lambda n: window_office(n, 1.1, 1.1, "office", 122),
    "win_office_wide": lambda n: window_office(n, 1.6, 1.3, "office", 123),
    "win_factory_tall": lambda n: window_office(n, 1.0, 1.4, "factory", 131),
    "win_factory_sq": lambda n: window_office(n, 1.1, 1.1, "factory", 132),
    "win_factory_wide": lambda n: window_office(n, 1.6, 1.3, "factory", 133),
    "fuel_tank_green": lambda n: fuel_tank(n, (0.07, 0.13, 0.08), seed=141),
    "sandbags_1m": lambda n: sandbags(n, 161),
    "t_wall": lambda n: t_wall(n, 211),
    "win_church": lambda n: church_window(n),
    "door_church": lambda n: church_door(n),
    "fountain_town": lambda n: fountain(n, 241),
    "fuel_bowser": lambda n: fuel_bowser(n, (0.06, 0.1, 0.05), 221),
    "wagon_red": lambda n: box_wagon(n, CONTAINER_COLOURS["brown"], 231),
    "wagon_green": lambda n: box_wagon(n, CONTAINER_COLOURS["green"], 232),
    "pallet": lambda n: pallet(n, None, 201),
    "pallet_boxes": lambda n: pallet(n, "boxes", 202),
    "pallet_sacks": lambda n: pallet(n, "sacks", 203),
    "van_green": lambda n: panel_van(n, (0.06, 0.1, 0.05), 191),
    "van_blue": lambda n: panel_van(n, (0.04, 0.09, 0.24), 192),
    "van_white": lambda n: panel_van(n, (0.62, 0.62, 0.6), 193),
    "hand_cart": lambda n: hand_cart(n, "empty", 181),
    "hand_cart_sacks": lambda n: hand_cart(n, "sacks", 182),
    "hand_cart_barrels": lambda n: hand_cart(n, "barrels", 183),
    "truck_box_green": lambda n: semi_truck(n, (0.06, 0.1, 0.05), "box", 171),
    "truck_flat_green": lambda n: semi_truck(n, (0.06, 0.1, 0.05), "flat", 172),
    "truck_flat_red": lambda n: semi_truck(n, (0.3, 0.03, 0.02), "flat", 173),
    "truck_flat_blue": lambda n: semi_truck(n, (0.03, 0.08, 0.22), "flat", 174),
    "crate_pine": lambda n: wooden_crate(n, "pine", 151),
    "crate_old": lambda n: wooden_crate(n, "old", 152),
    # ISO containers (Hafen): 40 ft and 20 ft in the port's colours.
    **{f"container40_{c}": (lambda n, c=c, i=i: container(n, c, 12.19, 41 + i)) for i, c in enumerate(CONTAINER_COLOURS)},
    **{f"container20_{c}": (lambda n, c=c, i=i: container(n, c, 6.06, 61 + i)) for i, c in enumerate(CONTAINER_COLOURS)},
}

# Bigger pieces get a bigger baked map.
TEX_SIZE = {n: 2048 for n in PIECES if n.startswith(("container40", "fuel_tank"))}
THEME = {n: ["hafen", "fliegerhorst"] for n in PIECES if n.startswith(("container", "win_office", "win_factory", "fuel_tank"))}
THEME |= {n: ["all"] for n in PIECES if n.startswith("crate")}
THEME["sandbags_1m"] = ["fliegerhorst", "hafen"]
THEME |= {n: ["fliegerhorst", "hafen"] for n in PIECES if n.startswith("truck")}
THEME |= {n: ["all"] for n in PIECES if n.startswith(("van", "pallet"))}
THEME["t_wall"] = ["fliegerhorst", "hafen"]
THEME["fuel_bowser"] = ["fliegerhorst"]
THEME |= {n: ["hafen"] for n in PIECES if n.startswith("wagon")}
TEX_SIZE |= {n: 2048 for n in PIECES if n.startswith(("truck", "fuel_bowser", "wagon"))}
MOUNT = {n: "floor" for n in PIECES if n.startswith(("container", "cafe", "market", "planter", "fuel_tank", "crate", "sandbags", "truck", "hand_cart", "van", "pallet", "t_wall", "fuel_bowser", "wagon", "fountain"))}
# Collision boxes (half width, half depth in metres; as tall as the piece) for the ones that stand in the way.
COLLIDE = {**{n: (0.7, 0.7) for n in PIECES if n.startswith("cafe")}, **{n: (1.35, 0.8) for n in PIECES if n.startswith("market")},
           "planter_stone": (0.44, 0.44)}


# ---- vehicles from the Nexus Sim catalogue (the workspace's own models, apps/bevy-spike) ------------------------

# Optional source material for rebuilding imported vehicles. Runtime GLBs are already bundled.
import os
NEXUS = Path(os.environ.get("ZUGRIFF_VEHICLE_SOURCES", str(HERE / "sources" / "vehicles")))

# name: (source model, triangle budget, collision (width, length) in metres or None, turn about the up axis in
# degrees so the nose points along the model's +Z front, maps).
VEHICLES = {
    "veh_mig29": ("mig29.glb", 65000, (3.8, 17.3), 0, ["fliegerhorst"]),
    "veh_tornado": ("tornado.glb", 40000, (3.2, 15.5), 0, ["fliegerhorst"]),
    "veh_nh90": ("nh90.glb", 24000, (3.0, 15.0), 0, ["fliegerhorst"]),
    "veh_leopard": ("leopard2a8.glb", 26000, (3.8, 9.5), 0, ["fliegerhorst", "hafen"]),
    "veh_patriot": ("patriot.glb", 24000, (3.0, 11.0), 0, ["fliegerhorst"]),
    "veh_f124": ("f124.glb", 60000, None, 0, ["hafen"]),
}


# A factor on the baked colour (linear) for vehicles whose paint reads too light here.
VEHICLE_TINT = {"veh_f124": 0.62}


def vehicle(name: str) -> dict:
    """A catalogue vehicle as a placed model: its parts joined, decimated to the budget, rebaked into one 2048 px
    material, a collision box round the hull (not the wings or rotor)."""
    src, budget, collide, turn, theme = VEHICLES[name]
    build.reset()
    objs = build.load(NEXUS / src)
    parts = build.meshes(objs)
    build.loose(parts)
    build.remove([o for o in objs if o.type != "MESH"])
    for o in parts:
        if o.data.users > 1:
            o.data = o.data.copy()
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)
    body = build._join(parts)
    # Left out: decals (serial, roundels, crest, flag: see-through overlays would bake as black patches), the
    # engines' flame, the crew and the cockpit's insides (a parked aircraft, its canopy opaque in the bake).
    drop = ("jet-", "crew-", "cockpit-", "display-", "hud-", "|interior", "muzzle-flash", "rotor-disc")

    def dropped(mt) -> bool:
        n = mt.name.lower()
        # (markings are named "kind:text"; everything else see-through but glass is an effect)
        return ":" in n or any(d in n for d in drop) or (mt.blend_method == "BLEND" and "glass" not in n and "canopy" not in n)

    bm = bmesh.new()
    bm.from_mesh(body.data)
    gone = {i for i, sl in enumerate(body.material_slots) if sl.material and dropped(sl.material)}
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.material_index in gone], context="FACES")
    # Strands (a whip antenna stretched by its skin, a cable): loose parts over 3 m long and under 8 cm across
    # would stretch the bounding box and the collision.
    seen = set()
    strands = []
    for f0 in bm.faces:
        if f0.index in seen:
            continue
        island, stack = [], [f0]
        seen.add(f0.index)
        while stack:
            f = stack.pop()
            island.append(f)
            for e in f.edges:
                for g in e.link_faces:
                    if g.index not in seen:
                        seen.add(g.index)
                        stack.append(g)
        vs = {v for f in island for v in f.verts}
        ext = sorted(max(v.co[i] for v in vs) - min(v.co[i] for v in vs) for i in range(3))
        if ext[2] > 3.0 and ext[1] < 0.08:
            strands += island
    bm.faces.ensure_lookup_table()
    if strands:
        print(f"{name}: dropped {len(strands)} faces of strands")
        bmesh.ops.delete(bm, geom=strands, context="FACES")
    bm.to_mesh(body.data)
    bm.free()
    body.name = name
    build._select(body)
    bpy.ops.object.material_slot_remove_unused()
    body.data.transform(Matrix.Rotation(math.radians(turn), 4, "Z"))
    # Unlit or emissive materials (lamps, the HUD glass) have no principled shader to bake from: give them one
    # with their colour.
    for sl in body.material_slots:
        mt = sl.material
        if mt is None or build._principled(mt) is not None:
            continue
        mt.use_nodes = True
        nt = mt.node_tree
        colour_src = None
        for n in nt.nodes:
            if n.type == "TEX_IMAGE":
                colour_src = n.outputs["Color"]
                break
        const = (0.5, 0.5, 0.5, 1.0)
        for n in nt.nodes:
            if n.type in ("EMISSION", "BACKGROUND", "RGB") and colour_src is None:
                sock = n.inputs[0] if n.type != "RGB" else None
                if sock is not None and sock.is_linked:
                    colour_src = sock.links[0].from_socket
                elif sock is not None:
                    const = tuple(sock.default_value)
        p = nt.nodes.new("ShaderNodeBsdfPrincipled")
        if colour_src is not None:
            nt.links.new(colour_src, p.inputs["Base Color"])
        else:
            p.inputs["Base Color"].default_value = const
        out = next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL"), None) or nt.nodes.new("ShaderNodeOutputMaterial")
        nt.links.new(p.outputs["BSDF"], out.inputs["Surface"])
    build._weld(body)
    src_tris = build._tris(body)
    import numpy as np

    co = np.empty(len(body.data.vertices) * 3)
    body.data.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    box_lo, box_hi = co.min(0) - 0.05, co.max(0) + 0.05
    build.decimate([body], budget)
    # Collapsing ten to one can shoot a vertex far out (a spike metres long): faces that leave the source's box go.
    bm = bmesh.new()
    bm.from_mesh(body.data)
    out = [f for f in bm.faces if any(not (box_lo[i] <= v.co[i] <= box_hi[i]) for v in f.verts for i in range(3))]
    if out:
        print(f"{name}: dropped {len(out)} faces decimation pushed out of the model's box")
        bmesh.ops.delete(bm, geom=out, context="FACES")
    bm.to_mesh(body.data)
    bm.free()
    build._clean(body)
    if not body.data.uv_layers:
        body.data.uv_layers.new(name="UVMap")
    build._rebake(body, name, 2048)
    bake_ao(body, name, 2048, distance=0.5, strength=0.7)
    if name in VEHICLE_TINT:
        # Darken the baked colour (the catalogue's naval grey is tuned for its own renderer; here it came out white).
        import numpy as np

        im = bpy.data.images[f"{name}_baseColor"]
        px = np.empty(im.size[0] * im.size[1] * 4, dtype=np.float32)
        im.pixels.foreach_get(px)
        px = px.reshape(-1, 4)
        px[:, :3] *= VEHICLE_TINT[name]
        im.pixels.foreach_set(px.ravel())
        im.update()
    build._select(body)
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(40), keep_sharp_edges=False)
    _, lo0, _ = build._bounds(body)
    zmin = float(lo0[2])
    build._to_origin(body, None)
    fill = build._lightmap_uv(body, 0.004)
    path = build._export_prop(name, body)
    check = build._glb_check(path)
    _, lo, hi = build._bounds(body)
    size = [float(hi[0] - lo[0]), float(hi[2] - lo[2]), float(hi[1] - lo[1])]
    entry = {
        "tris": check["tris"],
        "bytes": path.stat().st_size,
        "size": [round(v, 3) for v in size],
        "area": round(build._area(body), 3),
        "lightmap_fill": round(fill, 3),
        "collide": [round(collide[0] / 2, 3), round(size[1] / 2, 3), round(collide[1] / 2, 3)] if collide else None,
        "albedo": build._albedo(body),
        "theme": theme,
        # (the model's lowest point: for a ship, how far its keel is below the waterline, which is its y = 0)
        "bottom": round(zmin, 3),
        "mount": "floor",
        "made": "arch.py",
    }
    print(f"{name}.glb: {src_tris} -> {check['tris']} tris, {entry['bytes'] / 1e3:.0f} kB, size {entry['size']}")
    return entry


def bake_ao(obj, name: str, size: int, distance: float = 0.3, strength: float = 0.85):
    """Bakes ambient occlusion (Cycles, `distance` metres) through the object's UV map and multiplies it into its
    baked colour: props take the level's lightmap, which has one value per luxel, so the cavities between ribs,
    under sills and inside frames would otherwise be as bright as the faces."""
    import numpy as np

    scene = bpy.context.scene
    if scene.world is None:
        scene.world = bpy.data.worlds.new("ao")
    scene.world.light_settings.distance = distance
    scene.cycles.samples = 48
    img = bpy.data.images.new(f"{name}_ao", size, size, alpha=False)
    img.colorspace_settings.name = "Non-Color"
    mat = obj.material_slots[0].material
    nt = mat.node_tree
    for n in nt.nodes:
        n.select = False
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    t.select = True
    nt.nodes.active = t
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.bake(type="AO", margin=8)
    nt.nodes.remove(t)
    ao = np.empty(size * size * 4, dtype=np.float32)
    img.pixels.foreach_get(ao)
    ao = ao.reshape(-1, 4)[:, 0]
    base = bpy.data.images[f"{name}_baseColor"]
    px = np.empty(base.size[0] * base.size[1] * 4, dtype=np.float32)
    base.pixels.foreach_get(px)
    px = px.reshape(-1, 4)
    if base.size[0] != size:
        return
    k = 1.0 - strength * (1.0 - np.clip(ao, 0, 1))
    px[:, :3] *= k[:, None]
    base.pixels.foreach_set(px.ravel())
    base.update()
    bpy.data.images.remove(img)


def piece(name: str) -> dict:
    build.reset()
    parts, anchor = PIECES[name](name)
    body = build._join(parts)
    body.name = name
    build._select(body)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    # One UV map for the bake to unwrap from (the materials read by projection, so any will do).
    if not body.data.uv_layers:
        body.data.uv_layers.new(name="UVMap")
    tris_src = build._tris(body)
    build._rebake(body, name, TEX_SIZE.get(name, 1024))
    bake_ao(body, name, TEX_SIZE.get(name, 1024))
    build._select(body)
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(35), keep_sharp_edges=False)
    # Where the opening sits above the model's bottom (after it is set on its bottom).
    _, lo, hi = build._bounds(body)
    zmin = float(lo[2])
    xmid = float(lo[0] + hi[0]) / 2
    build._to_origin(body, None)
    # Corrugated sheet (containers, trucks, tanks): every rib is a strip thinner than a luxel; with the usual angle
    # limit each became its own island, and the lightmap came out as black stripes. At 88° a side is one island.
    ribbed = name.startswith(("container", "truck", "fuel_tank", "fuel_bowser", "wagon"))
    fill = build._lightmap_uv(body, 0.01, 88.0 if ribbed else 66.0)
    path = build._export_prop(name, body)
    check = build._glb_check(path)
    _, lo, hi = build._bounds(body)
    size = [float(hi[0] - lo[0]), float(hi[2] - lo[2]), float(hi[1] - lo[1])]
    anchor = dict(anchor)
    if "open_z" in anchor:
        anchor["open_z"] = round(anchor["open_z"] - zmin, 4)
        # The opening's middle along the model's +x from the middle of its bounding box (a house number on one
        # side shifts the box).
        anchor["open_x"] = round(-xmid, 4)
    if "top" in anchor:
        anchor["top"] = round(anchor["top"] - zmin, 4)
    entry = {
        "tris": check["tris"],
        "bytes": path.stat().st_size,
        "size": [round(v, 3) for v in size],
        "area": round(build._area(body), 3),
        "lightmap_fill": round(fill, 3),
        "collide": [COLLIDE[name][0], round(size[1] / 2, 3), COLLIDE[name][1]] if name in COLLIDE else None,
        "albedo": build._albedo(body),
        "theme": THEME.get(name, ["altstadt"]),
        "mount": MOUNT.get(name, "wall"),
        "anchor": anchor,
        "made": "arch.py",
    }
    print(f"{name}.glb: {tris_src} tris, {entry['bytes'] / 1e3:.0f} kB, size {entry['size']}, anchor {anchor}")
    return entry


def main():
    names = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [a for a in sys.argv[1:] if not a.startswith("-")]
    names = [n for n in names if n in PIECES or n in VEHICLES] or list(PIECES)
    index = build.PROPS_OUT / "props.json"
    entries = json.loads(index.read_text()) if index.exists() else {}
    for n in names:
        print(f"== {n}")
        entries[n] = vehicle(n) if n in VEHICLES else piece(n)
    index.write_text(json.dumps(entries, indent=1) + "\n")


if __name__ == "__main__":
    main()
