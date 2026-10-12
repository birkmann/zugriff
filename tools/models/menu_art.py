"""Renders the menus' pictures from the game's own models, with Blender run headless:

    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/menu_art.py [icons] [operators]

- icons (`assets/ui/icons/<name>.png`, 512×192): the buy menu's item pictures. Each weapon prop seen from its right
  side, muzzle to the right, in white with studio shading and cavities, on transparent ground; the menu tints them.
  Equipment the game has no model for (vest, helmet, kit, flash and smoke grenades) is built here from primitives.
- operators (`assets/ui/operator-ksk.png`, `operator-opfor.png`, 900×1400): a soldier of each side in its idle pose,
  a rifle fitted to both hands at low ready, lit key/rim/fill with Cycles on transparent ground, for the lobby.
"""

import math
import os
import re
import sys
from pathlib import Path

import bpy
from mathutils import Euler, Matrix, Vector

ROOT = Path(__file__).resolve().parents[2]
MODELS = ROOT / "assets" / "models"
OUT = ROOT / "assets" / "ui"


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def load(path):
    before = set(bpy.context.scene.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    return [o for o in bpy.context.scene.objects if o not in before]


def bounds(objs):
    pts = [o.matrix_world @ Vector(c) for o in objs if o.type == "MESH" for c in o.bound_box]
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return lo, hi


def posed_bounds():
    """The bounds of every mesh as posed (skinned), not of its rest shape."""
    deps = bpy.context.evaluated_depsgraph_get()
    pts = []
    for o in bpy.context.scene.objects:
        # Helpers (the soldiers carry a small icosphere) are not part of the figure.
        if o.type != "MESH" or o.hide_render or o.name.startswith("Icosphere"):
            continue
        e = o.evaluated_get(deps)
        m = e.to_mesh()
        pts.extend(e.matrix_world @ v.co for v in m.vertices)
        e.to_mesh_clear()
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return lo, hi


def camera(location, rotation, ortho=None, lens=50.0):
    scene = bpy.context.scene
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.location = location
    cam.rotation_euler = rotation
    if ortho:
        cam.data.type = "ORTHO"
        cam.data.ortho_scale = ortho
    else:
        cam.data.lens = lens
    return cam


# --- Icons -----------------------------------------------------------------------------------------------------------

ICON_W, ICON_H = 512, 192
# name: (model, least width in metres across the picture)
PROPS = {"rifle": ("m4.glb", 0), "ak": ("ak.glb", 0), "sniper": ("sniper.glb", 0), "shotgun": ("shotgun.glb", 0), "smg": ("smg.glb", 0.78), "mg": ("mg.glb", 0), "pistol": ("pistol.glb", 0.6), "he": ("grenade.glb", 0.36)}


def workbench():
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.render.resolution_x, scene.render.resolution_y = ICON_W, ICON_H
    scene.render.film_transparent = True
    scene.render.image_settings.color_mode = "RGBA"
    shading = scene.display.shading
    shading.light = "STUDIO"
    shading.color_type = "SINGLE"
    shading.single_color = (1.0, 1.0, 1.0)
    shading.show_cavity = True
    shading.cavity_type = "BOTH"
    shading.curvature_ridge_factor = 1.4
    shading.curvature_valley_factor = 1.4
    scene.display.render_aa = "32"


def side_view(objs, fill=0.9, min_width=0.0):
    """An orthographic camera on +X looking at the objects' side: +Y (the muzzle) to the right, +Z up. `min_width`
    (metres across the picture) keeps small things small, so a pistol does not come out as big as a rifle."""
    lo, hi = bounds(objs)
    c = (lo + hi) / 2
    w, h = hi.y - lo.y, hi.z - lo.z
    scale = max(max(w, h * ICON_W / ICON_H) / fill, min_width)
    camera((c.x + 5.0, c.y, c.z), (math.pi / 2, 0.0, math.pi / 2), ortho=scale)


def render(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    bpy.context.scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    print("wrote", path)


def prim(kind, **kw):
    getattr(bpy.ops.mesh, f"primitive_{kind}_add")(**kw)
    o = bpy.context.active_object
    return o


def bevel(o, width=0.004, segments=3):
    m = o.modifiers.new("bevel", "BEVEL")
    m.width = width
    m.segments = segments
    return o


def canister(radius, height, bands):
    """A hand grenade canister standing on Z, its spoon and ring on the +Y side (which ends up to the right)."""
    parts = [bevel(prim("cylinder", radius=radius, depth=height, vertices=48, location=(0, 0, height / 2)), 0.003)]
    for z in bands:
        parts.append(prim("torus", major_radius=radius, minor_radius=0.0025, location=(0, 0, z)))
    parts.append(bevel(prim("cylinder", radius=radius * 0.55, depth=0.018, vertices=32, location=(0, 0, height + 0.009)), 0.002))
    spoon = bevel(prim("cube", size=1, location=(0, radius + 0.004, height * 0.62)), 0.0015)
    spoon.scale = (0.016, 0.005, height * 0.8)
    spoon.rotation_euler = (math.radians(-6), 0, 0)
    parts.append(spoon)
    ring = prim("torus", major_radius=0.014, minor_radius=0.0022, location=(0, radius * 0.9, height + 0.022), rotation=(0, math.pi / 2, 0))
    parts.append(ring)
    return parts


def vest(x=0.0):
    """A plate carrier from the front (seen from +X: its front faces the camera)."""
    parts = []
    body = bevel(prim("cube", size=1, location=(x, 0, 0.0)), 0.02, 4)
    body.scale = (0.06, 0.34, 0.42)
    parts.append(body)
    for sy in (-1, 1):
        strap = bevel(prim("cube", size=1, location=(x, sy * 0.11, 0.27)), 0.01, 3)
        strap.scale = (0.05, 0.07, 0.16)
        parts.append(strap)
    for i in range(3):
        pouch = bevel(prim("cube", size=1, location=(x + 0.05, -0.11 + i * 0.11, -0.08)), 0.008, 3)
        pouch.scale = (0.05, 0.095, 0.16)
        parts.append(pouch)
        flap = bevel(prim("cube", size=1, location=(x + 0.08, -0.11 + i * 0.11, 0.0)), 0.004, 2)
        flap.scale = (0.01, 0.09, 0.02)
        parts.append(flap)
    patch = bevel(prim("cube", size=1, location=(x + 0.04, 0, 0.12)), 0.004, 2)
    patch.scale = (0.02, 0.16, 0.07)
    parts.append(patch)
    cummer = bevel(prim("cube", size=1, location=(x - 0.01, 0, -0.19)), 0.01, 3)
    cummer.scale = (0.05, 0.4, 0.07)
    parts.append(cummer)
    return parts


def helmet(y=0.0):
    """A combat helmet in profile, facing +Y."""
    shell = prim("uv_sphere", radius=0.13, segments=64, ring_count=32, location=(0, y, 0))
    shell.scale = (1.0, 1.12, 0.92)
    cut = prim("cube", size=1, location=(0, y, -0.2))
    cut.scale = (1, 1, 0.4)
    m = shell.modifiers.new("cut", "BOOLEAN")
    m.object = cut
    m.operation = "DIFFERENCE"
    bpy.context.view_layer.objects.active = shell
    bpy.ops.object.modifier_apply(modifier="cut")
    bpy.data.objects.remove(cut)
    sol = shell.modifiers.new("solid", "SOLIDIFY")
    sol.thickness = 0.008
    rim = prim("torus", major_radius=0.13, minor_radius=0.006, location=(0, y, 0.0))
    rim.scale = (1.0, 1.12, 1.0)
    mount = bevel(prim("cube", size=1, location=(0, y + 0.135, 0.06)), 0.004, 2)
    mount.scale = (0.05, 0.02, 0.04)
    rail = bevel(prim("cube", size=1, location=(0.12, y, 0.03)), 0.003, 2)
    rail.scale = (0.03, 0.12, 0.018)
    return [shell, rim, mount, rail]


def vest_helmet():
    """Vest and helmet side by side: the helmet to the right of (and a little in front of) the vest."""
    parts = vest(-0.2)
    for p in helmet(0.0):
        p.location = (p.location.x + 0.2, (p.location.y + 0.52), p.location.z - 0.05)
        p.scale = p.scale * 1.5
        p.location.y = 0.52 + (p.location.y - 0.52) * 1.5
    return parts + [o for o in bpy.context.scene.objects if o.type == "MESH" and o not in parts]


def kit():
    """The defuse kit: a hard case with a carry handle, latches and a grip band."""
    case = bevel(prim("cube", size=1, location=(0, 0, 0.07)), 0.012, 4)
    case.scale = (0.1, 0.34, 0.14)
    lid = bevel(prim("cube", size=1, location=(0.0, 0, 0.12)), 0.004, 2)
    lid.scale = (0.104, 0.344, 0.012)
    parts = [case, lid]
    for sy in (-1, 1):
        latch = bevel(prim("cube", size=1, location=(0.05, sy * 0.1, 0.11)), 0.003, 2)
        latch.scale = (0.012, 0.04, 0.05)
        parts.append(latch)
    handle = prim("torus", major_radius=0.05, minor_radius=0.009, location=(0, 0, 0.14), rotation=(math.pi / 2, 0, math.pi / 2))
    parts.append(handle)
    band = bevel(prim("cube", size=1, location=(0.0, 0, 0.05)), 0.003, 2)
    band.scale = (0.106, 0.346, 0.02)
    parts.append(band)
    return parts


def icons():
    out = OUT / "icons"
    for name, (file, least) in PROPS.items():
        reset()
        workbench()
        objs = load(MODELS / file)
        side_view(objs, 0.9, least)
        render(out / f"{name}.png")
    for name, build, least in [
        ("flash", lambda: canister(0.022, 0.1, [0.02, 0.05, 0.08]), 0.36),
        ("smoke", lambda: canister(0.03, 0.12, [0.03, 0.09]), 0.36),
        ("vest", lambda: vest(), 0),
        ("helmet", lambda: helmet(), 0.95),
        ("vesthelm", vest_helmet, 0),
        ("kit", kit, 0.95),
    ]:
        reset()
        workbench()
        objs = build()
        bpy.context.view_layer.update()
        side_view(objs, 0.86, least)
        render(out / f"{name}.png")


# --- Operators -------------------------------------------------------------------------------------------------------


def area_light(name, location, target, energy, size, color):
    light = bpy.data.lights.new(name, "AREA")
    light.energy = energy
    light.size = size
    light.color = color
    o = bpy.data.objects.new(name, light)
    bpy.context.scene.collection.objects.link(o)
    o.location = location
    o.rotation_euler = (Vector(target) - Vector(location)).to_track_quat("-Z", "Y").to_euler()
    return o


def bone(arm, pattern):
    """The first bone whose name matches (the two rigs name their bones differently)."""
    rx = re.compile(pattern)
    return next(b for b in arm.pose.bones if rx.search(b.name))


def empty(name, location):
    o = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(o)
    o.location = location
    return o


# Contact points measured on each prop in Blender space (+Y muzzle, +Z up).
# The rifles have different origins and handguard lengths; sharing these points
# put the AK support hand on its barrel and left the M4 trigger hand behind its grip.
HOLDS = {
    "m4.glb": {"butt": (-0.028, -0.145, 0.065), "R": (-0.028, 0.115, -0.020), "L": (-0.028, 0.365, 0.075)},
    "ak.glb": {"butt": (0.016, -0.180, 0.030), "R": (0.016, 0.075, -0.012), "L": (0.016, 0.325, 0.075)},
}


def frame(forward, up_hint):
    """A rotation whose +Y is `forward` and whose +Z is as close to `up_hint` as it can be."""
    y = forward.normalized()
    x = y.cross(up_hint).normalized()
    z = x.cross(y)
    return Matrix((x, y, z)).transposed()


def finger_chain(first):
    return [first, first.children[0], first.children[0].children[0]]


def grasp_center(w, chain):
    """Centre of the curled middle finger, rather than the back of the palm."""
    return sum([w @ b.head for b in chain] + [w @ chain[-1].tail], Vector()) / 4


def low_ready(arm, rifle):
    """Fit each hand independently to the rifle, then solve only the two arm bones.

    The retargeted idle provides curled fingers, not a reliable weapon orientation.
    Derive a grasp frame from each hand's knuckles: the right wraps the pistol grip;
    the left cups the handguard. Anchor the butt ahead of the right shoulder and
    point the muzzle down/forward. Leave the clavicles and torso in their idle pose.
    """
    w = arm.matrix_world.copy()
    hand = {s: bone(arm, rf"({s}_Hand|{'Left' if s == 'L' else 'Right'}Hand)_\d+$") for s in "LR"}
    mid = {s: bone(arm, rf"({s}_Mid1|{'Left' if s == 'L' else 'Right'}HandMiddle1)_\d+$") for s in "LR"}
    index = {s: bone(arm, rf"({s}_Index1|{'Left' if s == 'L' else 'Right'}HandIndex1)_\d+$") for s in "LR"}
    fore = {s: bone(arm, rf"({s}_Forearm|{'Left' if s == 'L' else 'Right'}ForeArm)_\d+$") for s in "LR"}
    upper = {s: bone(arm, rf"({s}_Upperarm|{'Left' if s == 'L' else 'Right'}Arm)_\d+$") for s in "LR"}
    shoulder = {s: w @ upper[s].head for s in "LR"}
    contacts = HOLDS[rifle]
    rot = frame(Vector((0.75, -0.55, -0.45)), Vector((0, 0, 1)))
    loc = shoulder["R"] + Vector((0.025, -0.20, -0.055)) - rot @ Vector(contacts["butt"])
    gun_transform = Matrix.Translation(loc) @ rot.to_4x4()
    gun = load(MODELS / rifle)
    for root in (o for o in gun if o.parent is None):
        root.matrix_world = gun_transform @ root.matrix_world

    for s in "LR":
        chain = finger_chain(mid[s])
        center = grasp_center(w, chain)
        wrist = w @ hand[s].head
        row = w @ index[s].head - w @ mid[s].head
        source = frame(w @ mid[s].head - wrist, row) if s == "R" else frame(row, w.to_3x3() @ (chain[1].head - mid[s].head))
        relative = rot @ source.inverted()
        grip = loc + rot @ Vector(contacts[s])
        goal = grip + relative @ (wrist - center)
        hand_rotation = (relative @ (w @ hand[s].matrix).to_3x3().normalized()).to_quaternion()

        # Keep elbows below the shoulders. A two-bone chain avoids the unnatural
        # clavicle lift caused by including the shoulder in the previous solve.
        ik = fore[s].constraints.new("IK")
        ik.use_tail = True
        ik.target = empty(f"ik-{s}", goal)
        ik.chain_count = 2
        ik.use_stretch = False
        pole = empty(f"pole-{s}", shoulder[s] + Vector((-0.18 if s == "R" else 0.16, 0.04, -0.42)))
        ik.pole_target = pole
        best = None
        for deg in range(-180, 180, 10):
            ik.pole_angle = math.radians(deg)
            bpy.context.view_layer.update()
            distance = (w @ fore[s].head - pole.location).length
            if best is None or distance < best[0]:
                best = (distance, deg)
        ik.pole_angle = math.radians(best[1])
        # Imported rigs can offset the hand head from the forearm tail.
        for _ in range(10):
            bpy.context.view_layer.update()
            error = w @ hand[s].head - goal
            if error.length < 0.002:
                break
            ik.target.location -= error
        orient = empty(f"grip-{s}", goal)
        orient.rotation_mode = "QUATERNION"
        orient.rotation_quaternion = hand_rotation
        constraint = hand[s].constraints.new("COPY_ROTATION")
        constraint.target = orient
        constraint.target_space = "WORLD"
        constraint.owner_space = "WORLD"
        bpy.context.view_layer.update()

        # Rotation can shift the curled fingers on the imported, scaled rigs.
        # Finish the solve against the actual posed contact, not just the wrist.
        for _ in range(12):
            error = grasp_center(w, chain) - grip
            if error.length < 0.002:
                break
            ik.target.location -= error
            bpy.context.view_layer.update()

        if s == "R":
            # The resting index finger follows the receiver outside the trigger guard.
            for j, finger in enumerate(finger_chain(index[s])):
                head = w @ finger.head
                direction = (w.to_3x3() @ (finger.tail - finger.head)).normalized()
                aim = (rot @ Vector((0, 1, -0.04 * j))).normalized()
                delta = direction.rotation_difference(aim).to_matrix().to_4x4()
                finger.matrix = w.inverted() @ Matrix.Translation(head) @ delta @ Matrix.Translation(-head) @ w @ finger.matrix
                bpy.context.view_layer.update()
        error = (grasp_center(w, chain) - grip).length
        print(f"  {s} grip error: {error * 1000:.1f} mm; elbow pole {best[1]}°")
        if error > 0.005:
            raise RuntimeError(f"{rifle}: {s} hand missed its grip by {error:.3f} m")


def operator(side, soldier, rifle, key_color, rim_color):
    reset()
    scene = bpy.context.scene
    objs = load(MODELS / soldier)
    arm = next((o for o in objs if o.type == "ARMATURE"), None)
    # The clips come in as "<clip>_<armature>" and as NLA tracks: the idle alone.
    idle = next((a for a in bpy.data.actions if a.name.startswith("idle")), None)
    if arm and idle:
        arm.animation_data_create()
        for t in arm.animation_data.nla_tracks:
            t.mute = True
        arm.animation_data.action = idle
        scene.frame_set(int(idle.frame_range[0]) + 8)
    bpy.context.view_layer.update()
    if arm:
        low_ready(arm, rifle)
    bpy.context.view_layer.update()
    lo, hi = posed_bounds()
    height = hi.z - lo.z
    print(side, "height", height)
    scene.render.engine = "CYCLES"
    scene.cycles.samples = int(os.environ.get("SAMPLES", "256"))
    scene.cycles.use_denoising = True
    try:
        prefs = bpy.context.preferences.addons["cycles"].preferences
        prefs.compute_device_type = "OPTIX"
        prefs.get_devices()
        for d in prefs.devices:
            d.use = True
        scene.cycles.device = "GPU"
    except Exception as e:  # CPU is fine, only slower.
        print("no GPU:", e)
    scene.render.resolution_x, scene.render.resolution_y = 900, 1400
    scene.render.film_transparent = True
    scene.view_settings.view_transform = "AgX"
    scene.view_settings.look = "AgX - Punchy"
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.05, 0.06, 0.07, 1)
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.6
    scene.world = world
    mid = Vector(((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, lo.z + height * 0.5))
    chest = Vector((mid.x, mid.y, lo.z + height * 0.7))
    # The soldiers face -Y. A three-quarter view from their front left, the lens a little below the chest, the whole
    # figure filling the frame's height.
    lens = 70.0
    sensor = 36.0
    aspect = 900 / 1400
    vfov = 2 * math.atan(sensor / 2 / lens) if aspect < 1 else 2 * math.atan(sensor / aspect / 2 / lens)
    dist = (height / 0.9) / 2 / math.tan(vfov / 2)
    direction = Vector((float(os.environ.get("CAM_SIDE", "0.14")), -1.0, 0.0)).normalized()
    cam = camera(mid + direction * dist + Vector((0, 0, -height * 0.08)), (0, 0, 0), lens=lens)
    cam.data.sensor_fit = "VERTICAL"
    cam.data.sensor_height = sensor
    cam.rotation_euler = (mid - cam.location).to_track_quat("-Z", "Y").to_euler()
    area_light("key", (2.6, -2.6, 2.8), chest, 1100, 1.4, key_color)
    area_light("fill", (-2.8, -2.2, 1.0), chest, 120, 2.5, (0.75, 0.82, 1.0))
    area_light("rim", (-1.8, 2.2, 2.6), chest, 2600, 0.8, rim_color)
    area_light("rim2", (2.0, 2.0, 1.8), chest, 1600, 0.8, rim_color)
    render(OUT / f"operator-{side}.png")


if __name__ == "__main__":
    args = [a for a in sys.argv[sys.argv.index("--") + 1 :]] if "--" in sys.argv else sys.argv[1:]
    what = [a for a in args if a in ("icons", "operators")] or ["icons", "operators"]
    if "icons" in what:
        icons()
    if "operators" in what:
        operator("ksk", "soldier-blue.glb", "m4.glb", (1.0, 0.93, 0.84), (0.55, 0.75, 1.0))
        operator("opfor", "soldier-red.glb", "ak.glb", (1.0, 0.9, 0.8), (1.0, 0.62, 0.3))
