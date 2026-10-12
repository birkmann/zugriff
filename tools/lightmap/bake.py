"""Bakes a level's lightmap with Blender Cycles (headless, no Blender install needed).

    cargo run -p zugriff_sim --bin zg-compile -- assets/maps/<map>.map --bake-json /tmp/<map>.bake.json
    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/lightmap/bake.py \
        /tmp/<map>.bake.json assets/maps/<map>.lightmap.hdr [--samples 256]

The atlas holds irradiance (direct sun + sky + bounced light, no surface colour): the renderer multiplies it by each
surface's albedo. Surface albedos for the bounce come from ALBEDO below until the material library carries them.

Placed models (`props` in the bake input) are loaded from assets/models/props/<model>.glb at their place and baked into
their own square of the atlas through their second UV set; they shadow and bounce light onto the level too. Decal faces
(`decal/*`) are baked like any face but are invisible to rays, so their see-through parts cast no shadow.
"""

import json
import math
import os
import sys
import time

import bpy
import numpy as np
from mathutils import Vector

# Linear albedo of the dev textures (their sRGB base colours, see game/src/textures.rs).
ALBEDO_SRGB = {
    "dev/ground": (92, 88, 80),
    "dev/floor": (118, 116, 110),
    "dev/concrete": (150, 148, 142),
    "dev/wall": (196, 150, 96),
    "dev/plaster": (214, 206, 188),
    "dev/ceiling": (180, 180, 184),
    "dev/trim": (70, 76, 84),
    "dev/crate": (150, 112, 62),
    # Material sets: the mean sRGB of each set's colour image (assets/textures/<set>, see CREDITS.md there).
    "mat/concrete_wall": (100, 96, 92),
    "mat/concrete_floor": (86, 87, 84),
    "mat/concrete_painted": (149, 156, 147),
    "mat/plaster_white": (217, 213, 210),
    "mat/plaster_ochre": (139, 120, 77),
    "mat/plaster_old": (198, 196, 192),
    "mat/bricks090": (181, 146, 116),
    "mat/brick_dark": (97, 59, 29),
    "mat/cobblestone": (140, 138, 130),
    "mat/paving": (150, 150, 146),
    "mat/asphalt": (65, 65, 62),
    "mat/gravel": (127, 123, 111),
    "mat/wood_planks": (157, 120, 89),
    "mat/wood_beam": (96, 82, 73),
    "mat/wood_crate": (195, 170, 116),
    "mat/metal_corrugated": (153, 155, 158),
    "mat/metal_painted_red": (145, 42, 37),
    "mat/metal_painted_blue": (45, 72, 125),
    "mat/metal_painted_green": (66, 88, 61),
    "mat/metal_plate": (117, 124, 125),
    "mat/metal_rust": (103, 87, 80),
    "mat/roof_tiles": (143, 91, 67),
    "mat/tiles_floor": (211, 202, 194),
    "mat/ceiling_tiles": (244, 244, 244),
    "mat/dirt": (123, 105, 81),
    "mat/grass": (96, 109, 48),
    "mat/concrete_wall_b": (126, 125, 121),
    "mat/concrete_wall_c": (116, 113, 107),
    "mat/plaster_b": (184, 174, 154),
    "mat/plaster_c": (168, 167, 162),
    "mat/brick_b": (130, 99, 80),
    "mat/asphalt_b": (70, 70, 68),
    "mat/concrete_slabs": (125, 125, 121),
    "mat/paving_b": (128, 128, 125),
    "mat/metal_corrugated_b": (104, 114, 102),
    "mat/planks_b": (88, 84, 78),
    "mat/roof_b": (80, 82, 86),
    "mat/tiles_floor_b": (128, 128, 125),
    "mat/dirt_b": (136, 120, 100),
    "mat/glass_window": (63, 75, 88),
    "mat/window_frame": (192, 193, 189),
    "mat/metal_dark": (60, 62, 64),
    "mat/rubber": (40, 40, 40),
    "mat/door_steel": (96, 106, 96),
    "mat/rollershutter": (130, 131, 132),
    "mat/vent_grille": (58, 60, 62),
    "mat/sign_plate": (196, 192, 182),
    "mat/lamp_glass": (226, 222, 208),
    "mat/container_red": (128, 52, 44),
    "mat/container_blue": (44, 70, 115),
    "mat/container_green": (69, 88, 62),
    "mat/container_grey": (115, 119, 116),
    "mat/container_orange": (152, 89, 49),
    # Poly Haven sets for the look pass (2026-09-29).
    "mat/plaster_patched": (155, 137, 114),
    "mat/plaster_red": (149, 103, 82),
    "mat/plaster_cream": (213, 197, 163),
    "mat/plaster_pink": (205, 159, 137),
    "mat/plaster_sky": (159, 179, 185),
    "mat/plaster_sage": (169, 177, 145),
    "mat/gnd_setts": (122, 113, 93),
    "mat/gnd_setts_old": (121, 117, 106),
    "mat/gnd_setts_moss": (138, 125, 98),
    "mat/gnd_flagstone": (94, 86, 76),
    "mat/gnd_dust": (149, 115, 78),
    "mat/gnd_mud": (99, 82, 62),
    "mat/gnd_asphalt": (70, 57, 47),
    "mat/gnd_asphalt_damaged": (73, 56, 45),
    "mat/gnd_concrete": (138, 123, 103),
    "mat/gnd_concrete_damaged": (79, 67, 53),
    "mat/gnd_leaves": (114, 77, 52),
    "mat/plaster_worn": (117, 108, 92),
    "mat/plaster_mossy": (132, 127, 115),
    "mat/plaster_blue": (135, 165, 160),
    "mat/stone_rustic": (119, 102, 76),
    "mat/stone_rubble": (135, 119, 99),
    "mat/sandstone_bricks": (137, 128, 110),
    "mat/stone_blocks_grey": (86, 83, 75),
    "mat/wood_door": (65, 41, 26),
    "mat/wood_door_b": (76, 37, 14),
    "mat/wood_old": (115, 104, 92),
    "mat/wood_shutter": (119, 73, 37),
    "mat/roof_clay": (145, 80, 43),
    "mat/cobble_mossy": (119, 108, 76),
    "mat/stone_pavers": (100, 87, 75),
    "mat/concrete_pavers": (103, 94, 83),
    "mat/metal_rusty_paint": (109, 59, 44),
    "mat/metal_corrugated_rust": (87, 41, 24),
    "mat/container_sheet": (106, 38, 23),
    "mat/metal_grate": (130, 83, 50),
    "mat/factory_wall": (97, 118, 75),
    "mat/concrete_dirty": (137, 131, 119),
    "mat/concrete_cracked": (187, 174, 155),
    "mat/concrete_hangar": (51, 48, 45),
    "mat/concrete_precast": (151, 135, 116),
}


def linear(c: float) -> float:
    c /= 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def albedo(name: str) -> tuple[float, float, float, float]:
    r, g, b = ALBEDO_SRGB.get(name.lower(), (128, 128, 128))
    return (linear(r), linear(g), linear(b), 1.0)


SKY_STRENGTH = 0.42
# Bounced light a little weaker than physics says: rooms stay moody and the lamps' pools read (a darker world
# around them is what makes interiors look lit rather than flat).
BOUNCE_BOOST = 0.9


def _frame(d: Vector, along: Vector):
    """Euler rotation whose −Z is `d` and whose X lies along `along` (projected off `d`)."""
    from mathutils import Matrix

    z = -d.normalized()
    x = (along - along.dot(z) * z).normalized()
    y = z.cross(x)
    return Matrix((x, y, z)).transposed().to_euler()


PROPS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "assets", "models", "props")


def chart_labels(doc: dict, w: int, h: int) -> np.ndarray:
    """Each luxel's chart (1-based; 0 = none), rows as Blender stores them (bottom up). A face's chart is the box
    around its lightmap coordinates plus the 2-luxel pad (see crates/zugriff_sim/src/lightmap.rs); a model's is its
    square plus the pad."""
    labels = np.zeros((h, w), dtype=np.int32)
    n = 0
    boxes = []
    for f in doc["faces"]:
        us = [t[0] * w for t in f["lm"]]
        vs = [t[1] * h for t in f["lm"]]
        boxes.append((min(us) - 2.5, min(vs) - 2.5, max(us) + 2.5, max(vs) + 2.5))
    for pr in doc.get("props", []):
        x0, y0, x1, y1 = pr["rect"]
        boxes.append((x0 * w - 2, y0 * h - 2, x1 * w + 2, y1 * h + 2))
    for (x0, y0, x1, y1) in boxes:
        n += 1
        a, b = max(0, int(math.floor(x0 + 0.5))), min(w, int(math.ceil(x1 - 0.5)))
        c, d = max(0, int(math.floor(y0 + 0.5))), min(h, int(math.ceil(y1 - 0.5)))
        if a < b and c < d:
            # Top-down atlas rows → Blender's bottom-up rows.
            labels[h - d:h - c, a:b] = np.where(labels[h - d:h - c, a:b] == 0, n, labels[h - d:h - c, a:b])
    return labels


def labelled_blur(img: np.ndarray, labels: np.ndarray, sigma: float, radius: int) -> np.ndarray:
    """A Gaussian blur that only mixes luxels of the same chart (normalised by the weights that took part)."""
    acc = np.zeros_like(img)
    wsum = np.zeros(labels.shape, dtype=np.float32)
    hh, ww = labels.shape
    for dy in range(-radius, radius + 1):
        for dx in range(-radius, radius + 1):
            wgt = math.exp(-(dx * dx + dy * dy) / (2 * sigma * sigma))
            ys, yd = (slice(dy, hh), slice(0, hh - dy)) if dy >= 0 else (slice(0, hh + dy), slice(-dy, hh))
            xs, xd = (slice(dx, ww), slice(0, ww - dx)) if dx >= 0 else (slice(0, ww + dx), slice(-dx, ww))
            same = (labels[ys, xs] == labels[yd, xd]).astype(np.float32) * wgt
            acc[yd, xd] += img[ys, xs] * same[..., None]
            wsum[yd, xd] += same
    return acc / np.maximum(wsum, 1e-6)[..., None]


def to_blender(p):
    # Sim space is Y up (x, y, z); Blender is Z up.
    return (p[0], -p[2], p[1])


def main() -> None:
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:]
    src, out = args[0], args[1]
    samples = int(args[args.index("--samples") + 1]) if "--samples" in args else 128
    doc = json.load(open(src))
    atlas = doc["atlas"]
    w, h = atlas["width"], atlas["height"]
    t0 = time.time()

    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene

    image = bpy.data.images.new("lightmap", w, h, float_buffer=True, alpha=False)
    materials: dict[str, bpy.types.Material] = {}

    def bake_material(name: str, colour) -> bpy.types.Material:
        if name not in materials:
            mat = bpy.data.materials.new(name)
            mat.use_nodes = True
            nodes = mat.node_tree.nodes
            bsdf = nodes["Principled BSDF"]
            bsdf.inputs["Base Color"].default_value = colour
            bsdf.inputs["Roughness"].default_value = 1.0
            target = nodes.new("ShaderNodeTexImage")
            target.image = image
            nodes.active = target
            materials[name] = mat
        return materials[name]

    # The level as one mesh (decals as a second, which rays pass through), one material per surface, every face with
    # its own vertices.
    def level_mesh(label: str, faces: list) -> bpy.types.Object | None:
        if not faces:
            return None
        verts, polys, uvs, mats = [], [], [], []
        names: list[str] = []
        for f in faces:
            start = len(verts)
            verts.extend(to_blender(p) for p in f["points"])
            polys.append(list(range(start, start + len(f["points"]))))
            # Blender's UV origin is the image's bottom left; the atlas rows run top down like Bevy's.
            uvs.extend((t[0], 1.0 - t[1]) for t in f["lm"])
            if f["surface"] not in names:
                names.append(f["surface"])
            mats.append(names.index(f["surface"]))
        mesh = bpy.data.meshes.new(label)
        mesh.from_pydata(verts, [], polys)
        mesh.update()
        layer = mesh.uv_layers.new(name="lm")
        for i, loop in enumerate(mesh.loops):
            layer.data[i].uv = uvs[loop.vertex_index]
        for name in names:
            mesh.materials.append(bake_material(name, albedo(name)))
        for poly, m in zip(mesh.polygons, mats):
            poly.material_index = m
        o = bpy.data.objects.new(label, mesh)
        scene.collection.objects.link(o)
        return o

    decal_faces = [f for f in doc["faces"] if f["surface"].lower().startswith("decal/")]
    obj = level_mesh("level", [f for f in doc["faces"] if not f["surface"].lower().startswith("decal/")])
    decals = level_mesh("decals", decal_faces)
    if decals is not None:
        decals.visible_shadow = False
        decals.visible_diffuse = False
        decals.visible_glossy = False
        decals.visible_transmission = False
        decals.visible_volume_scatter = False
    polys = [p for o in (obj, decals) if o for p in o.data.polygons]

    # Placed models, each into its square.
    props = doc.get("props", [])
    info = {}
    if props:
        try:
            info = json.load(open(os.path.join(PROPS_DIR, "props.json")))
        except OSError:
            print("no props.json; props bake mid-grey")
    prop_objects = []
    # Each model is imported once (its meshes put into its own frame, its maps dropped: the bake only needs the
    # shapes and the lightmap UVs) and copied for every place it stands; importing a model per placement took
    # half an hour and 15 GB once the facades had hundreds of window pieces.
    templates = {}

    def template(model: str):
        if model in templates:
            return templates[model]
        path = os.path.join(PROPS_DIR, model + ".glb")
        if not os.path.exists(path):
            print(f"missing model {path}; skipped")
            templates[model] = None
            return None
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=path)
        parts = [o for o in bpy.data.objects if o not in before]
        meshes = []
        for o in parts:
            if o.type == "MESH":
                me = o.data.copy()
                me.transform(o.matrix_world)
                if len(me.uv_layers) < 2:
                    print(f"{model}: no lightmap UVs; skipped a part")
                    continue
                me.uv_layers.active = me.uv_layers[1]
                meshes.append(me)
        for o in parts:
            bpy.data.objects.remove(o, do_unlink=True)
        rgb = info.get(model, {}).get("albedo", (128, 128, 128))
        mat = bake_material("prop/" + model, (linear(rgb[0]), linear(rgb[1]), linear(rgb[2]), 1.0))
        for me in meshes:
            me.materials.clear()
            me.materials.append(mat)
        templates[model] = meshes
        return meshes

    # Every placement's copy in world space, then all of them joined into one object: Blender prepares a bake per
    # selected object over the whole atlas, and with hundreds of props that took an hour.
    from mathutils import Matrix

    placed = []
    for n, pr in enumerate(props):
        meshes = template(pr["model"])
        if not meshes:
            continue
        x0, y0, x1, y1 = pr["rect"]
        world = Matrix.Translation(to_blender(pr["origin"])) @ Matrix.Rotation(pr["yaw"], 4, "Z") @ Matrix.Scale(pr["scale"], 4)
        for me0 in meshes:
            me = me0.copy()
            lm = me.uv_layers[1]
            uv = np.empty(len(lm.data) * 2, dtype=np.float32)
            lm.data.foreach_get("uv", uv)
            uv = uv.reshape(-1, 2)
            # glTF's v runs down (the importer flips it); the atlas square is in top-down atlas coordinates.
            u, v = uv[:, 0], 1.0 - uv[:, 1]
            uv = np.stack([x0 + u * (x1 - x0), 1.0 - (y0 + v * (y1 - y0))], axis=1)
            lm.data.foreach_set("uv", uv.ravel())
            # Only the lightmap layer, named as the level's, so the join keeps it.
            for other in [l.name for l in me.uv_layers if l.name != lm.name]:
                me.uv_layers.remove(me.uv_layers[other])
            me.uv_layers[0].name = "lm"
            me.transform(world)
            o = bpy.data.objects.new(f"prop{n}", me)
            scene.collection.objects.link(o)
            placed.append(o)
    if placed:
        bpy.ops.object.select_all(action="DESELECT")
        for o in placed:
            o.select_set(True)
        bpy.context.view_layer.objects.active = placed[0]
        bpy.ops.object.join()
        placed[0].name = "props"
        prop_objects.append(placed[0])
    # (The imports' own maps aren't needed.)
    for im in list(bpy.data.images):
        if im.users == 0:
            bpy.data.images.remove(im)

    # Sun and sky.
    sun = doc["sun"]
    light = bpy.data.lights.new("sun", "SUN")
    light.energy = 4.0
    light.angle = math.radians(1.2)
    light.color = sun["color"]
    sun_obj = bpy.data.objects.new("sun", light)
    direction = Vector(to_blender(sun["direction"]))
    sun_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    scene.collection.objects.link(sun_obj)
    world = bpy.data.worlds.new("sky")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (*doc["sky"]["color"], 1.0)
    # The sky's share: on a clear day the open sky gives about a quarter of the sun's light on the ground, so
    # shadowed ground stays clearly darker than sunlit (a sky at full strength left the maps flat and grey).
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = SKY_STRENGTH
    scene.world = world

    # Lamps (`lights` in the bake input): bulbs, spots and strip lights, baked like the sun and the sky.
    for i, li in enumerate(doc.get("lights", [])):
        kind = {"point": "POINT", "spot": "SPOT", "area": "AREA"}.get(li["kind"], "POINT")
        lamp = bpy.data.lights.new(f"lamp{i}", kind)
        lamp.energy = li["power"]
        lamp.color = li["color"][:3]
        lamp.shadow_soft_size = 0.06
        if kind == "SPOT":
            lamp.spot_size = math.radians(li.get("cone", 90))
            lamp.spot_blend = 0.45
        if kind == "AREA":
            size = li.get("size") or [0.5, 0.1]
            lamp.shape = "RECTANGLE"
            lamp.size, lamp.size_y = size[0], size[1] if len(size) > 1 else size[0]
        o = bpy.data.objects.new(f"lamp{i}", lamp)
        o.location = to_blender(li["origin"])
        d = Vector(to_blender(li["dir"])).normalized()
        up = Vector(to_blender(li.get("along") or [1.0, 0.0, 0.0]))
        if abs(up.dot(d)) > 0.99 or up.length < 1e-6:
            up = Vector((1.0, 0.0, 0.0)) if abs(d.x) < 0.9 else Vector((0.0, 1.0, 0.0))
        # Lights shine down their −Z; an area light's X runs along its long side.
        o.rotation_euler = d.to_track_quat("-Z", "X").to_euler() if kind != "AREA" else _frame(d, up)
        scene.collection.objects.link(o)

    # Cycles, on the GPU when there is one.
    scene.render.engine = "CYCLES"
    scene.cycles.samples = samples
    scene.cycles.max_bounces = 4
    device = "CPU"
    try:
        prefs = bpy.context.preferences.addons["cycles"].preferences
        for kind in ("OPTIX", "CUDA", "HIP", "ONEAPI"):
            try:
                prefs.compute_device_type = kind
            except TypeError:
                continue
            prefs.get_devices()
            gpus = [d for d in prefs.devices if d.type == kind]
            if gpus:
                for d in prefs.devices:
                    d.use = d.type == kind
                scene.cycles.device = "GPU"
                device = kind
                break
    except Exception as e:  # noqa: BLE001
        print(f"gpu setup failed ({e}); baking on the CPU")
    print(f"baking {len(polys)} faces and {len(props)} models into {w}×{h} with {samples} samples on {device}", flush=True)

    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for o in [obj, decals, *prop_objects]:
        if o is not None:
            o.select_set(True)
    bpy.context.view_layer.objects.active = obj
    scene.render.bake.margin = 2
    scene.render.bake.margin_type = "EXTEND"
    # Direct light (sun and sky, crisp shadows) and bounced light are baked apart: the bounce is what's noisy (in
    # rooms it is all the light there is), so only it is smoothed, and only within each chart.
    def bake(passes: set) -> np.ndarray:
        print(f"  {'+'.join(sorted(passes)).lower()} pass at {time.time() - t0:.0f} s", flush=True)
        bpy.ops.object.bake(type="DIFFUSE", pass_filter=passes, margin=2, use_clear=True)
        px = np.empty(w * h * 4, dtype=np.float32)
        image.pixels.foreach_get(px)
        return px.reshape(h, w, 4)

    # The sun's direct light is the game's (real-time, with sharp shadows); the bake keeps what it can't do: the
    # sky, the lamps, and everything's bounce (the sun's included).
    bounce = bake({"INDIRECT"})
    sun_obj.hide_render = True
    direct = bake({"DIRECT"})
    sun_obj.hide_render = False
    # With the sun live, all that's baked is soft (sky, lamps, bounce): the whole map is smoothed within each chart,
    # which takes out the noise of the dim parts (the blotches in rooms and under vaults).
    labels = chart_labels(doc, w, h)
    light = direct[..., :3] + bounce[..., :3] * BOUNCE_BOOST
    total = direct.copy()
    total[..., :3] = np.where(labels[..., None] > 0, labelled_blur(light, labels, sigma=1.6, radius=4), light)
    image.pixels.foreach_set(total.ravel())

    image.filepath_raw = out
    image.file_format = "HDR"
    image.save()
    print(f"wrote {out} in {time.time() - t0:.1f} s (atlas hash {atlas['hash']})")


main()
