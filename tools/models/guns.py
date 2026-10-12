"""Every gun its own model: a model of that very weapon (sources.json, fetched by fetch.py) put into the hands of one of
the animated first-person packs, as the sniper rifle and the LMG already are (build.py `swap_in`). Run through build.py:

    uv run --python 3.11 --with "bpy==4.2.*" python tools/models/build.py prep-<key> | measure-<key> | vm-<key> | <key>

- `prep-<key>` makes `.cache/<key>-prep/<key>.glb`: the model turned to point along -Y with +Z up, at its real length,
  its loose pieces sorted into `body`, `mag`, `bolt` and `trigger` meshes (by the boxes in `GUNS`), and empties at
  its trigger (`trigger_point`) and sights (`sight_rear`, `sight_front`). That is the form `swap_in` takes.
- `measure-<key>` renders the prepped model from its side with a 5 cm grid (metres from the muzzle along, from the
  bottom up), and each loose piece's number, into the scratch dir given by MEASURE_OUT: for reading off the boxes and
  points.
- `vm-<key>` / `<key>`: the first-person model on its family's rig (with the family's clips, the magazine on the
  magazine bone, the bolt or slide on the bolt's), and the gun on its own.

Coordinates in `GUNS` are in the prepped frame: y from the muzzle (0) back, z from the bottom (0) up, x across (0 on
the middle of the gun), in metres.
"""

import math
import os
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

# The packs a gun can be put in: the source, its clips, the bones, and how long its own weapon really is.
FAMILIES = {
    "ak": dict(src="ak", pack_len=0.943, clips={"idle": "AK_Idle", "draw": "AK_Draw", "walk": "AK_Walk", "fire": "AK_Shot", "reload": "AK_Reload", "reload_empty": "AK_Reload_full"},
               body="PBody_058", trigger="Trigger_060", mag="Pmag_062", spare="Pmag2_063", bolt="Rif_059", hand="Hand_R_037", eye=dict(eye_node="Camera"), shift=(0.06, 0.0, -0.005)),
    "smg": dict(src="smg", pack_len=0.51, clips={"idle": "KDW_DPose_Idle", "draw": "KDW_Draw", "walk": "KDW_Walk", "fire": "KDW_Shot", "reload": "KDW_Reload_fast", "reload_empty": "KDW_Reload_full"},
                body="PBody_058", trigger="Trigger_060", mag="Pmag_061", spare="Pmag_2_062", bolt="Rif_059", hand="Hand_R_00", eye=dict(eye_node="Camera"), shift=(0.03, 0.06, -0.015),
                # (Its frame, the longest part, is as tall as it is long, with the grip: the upper gives the bore.)
                bore="LP SST30 Bottom Frame001_Material #457_0_Material_459", measure=["LP SST30 Bottom Frame001_Material #457_0_Material_45"]),
    "pistol": dict(src="pistol", pack_len=0.198, clips={"idle": "FPS_Pistol_Idle", "walk": "FPS_Pistol_Walk", "fire": "FPS_Pistol_Fire", "reload": "FPS_Pistol_Reload_easy", "reload_empty": "FPS_Pistol_Reload_full"},
                   body="PBody_058", trigger="Trigger_060", mag="Pmag_061", spare=None, bolt="Rif_059", hand="PBody_058", eye=dict(eye_bone="Head_Cam_014"), shift=(0.0, 0.0, 0.0),
                   # (A pistol's front quarter is too short to find the bore in: the slide gives it.)
                   bore="xd_frame.001", upright=True),
    "sniper": dict(src="sniper", pack_len=1.1, clips={"idle": "SRifle_Idle", "walk": "SRifle_Walk", "fire": "SRifle_Shot_nosight", "reload": "SRifle_Reload", "reload_empty": "SRifle_Reload_Full"},
                   body="Body_057", trigger="Trigger_058", mag="Mag_060", spare=None, bolt="Bolt_059", hand="Hand_R_037", eye=dict(eye_node="Camera"), shift=(0.03, 0.0, -0.04),
                   grip_at=("Trigger_058", 0.18, 0.03)),
    "shotgun": dict(src="shotgun", pack_len=1.0, clips={"idle": "SG_FPS_Idle", "walk": "SG_FPS_Walk", "fire": "SG_FPS_Shot", "reload": "SG_FPS_Reload"},
                    body="Body_058", trigger="Trig_060", mag=None, spare=None, bolt=None, hand="Hand_R_038", eye=dict(eye_bone="Head_Cam_014"), shift=(0.05, -0.05, -0.06),
                    # (The front quarter's barrel and magazine tube tilt the found bore: the whole gun gives it.)
                    bore="main_remington"),
}

# key: the source (sources.json), the family, its real length (m); how the source lies (`forward`/`up`: its axes, as
# "+x", "-z" …); pieces to drop (by object or mesh name, lower case substrings); boxes (y0, y1, z0, z1) sorting loose
# pieces into mag and bolt; the trigger point and the sights (y, z, x: the rear sight's aim point, a notch's blades' top or an
# aperture's middle, and the front post's top, read off slices through each sight: a few mm off shows as a sight picture
# with no post in the notch); aimed, the eye's distance behind the rear sight (the game's copy is gear.rs OWN).
GUNS: dict = {
    # The foregrip and the laser under the handguard go: the rig's left hand holds the handguard there.
    # (Its long stock and tall optic sat it too close to the eye: held a little further out.)
    "g36": dict(family="ak", length=0.999, forward="+y", up="+z", mag=[(0.49, 0.62, 0.0, 0.15)], drop_box=[(0.28, 0.47, 0.04, 0.155)],
                trigger=(0.66, 0.145), sight_rear=(0.70, 0.285), sight_front=(0.42, 0.285), eye=0.12, shift=(0.06, 0.08, -0.005)),
    # A spare magazine and two rounds lie under the gun in the source.
    "hk416": dict(family="ak", length=0.915, forward="-x", up="+z", mag=[(0.41, 0.52, 0.045, 0.13)], drop_box=[(0.40, 0.72, -0.01, 0.045)],
                  trigger=(0.545, 0.125), sight_rear=(0.59, 0.2636, 0.0049), sight_front=(0.128, 0.2635, 0.001), eye=0.12),
    "akm": dict(family="ak", length=0.880, forward="+y", up="+z", mag=[(0.34, 0.52, 0.0, 0.14)],
                trigger=(0.553, 0.165), sight_rear=(0.419, 0.2544, 0.0016), sight_front=(0.043, 0.2494, 0.0004), eye=0.25),
    "awm": dict(family="sniper", length=1.30, forward="-y", up="+z", drop=["silent", "night", "case", "bullet"],
                mag=[(0.74, 0.85, 0.02, 0.10)], bolt=[(0.86, 0.95, 0.09, 0.15)], trigger=(0.88, 0.085)),
    # Without its bipod (the rig's hand is there).
    "g28": dict(family="ak", length=1.082, forward="+x", up="+z", weld=True, rough=0.75, shift=(0.06, 0.07, -0.005), mag=[(0.51, 0.645, 0.02, 0.17)], drop_box=[(0.28, 0.42, -0.01, 0.20)],
                trigger=(0.673, 0.165)),
    # Its long stock puts the scope's eyecup beside the eye: held a little further out.
    "svd": dict(family="ak", length=1.225, forward="+x", up="+z", drop=["internal"], mag=[(0.67, 0.79, 0.0, 0.12)], trigger=(0.873, 0.083),
                shift=(0.06, 0.08, -0.005)),
    "mg5": dict(family="ak", length=1.14, forward="+y", up="+z", mag=[(0.59, 0.70, 0.0, 0.12)],
                trigger=(0.75, 0.103), sight_rear=(0.836, 0.2158, 0.0042), sight_front=(0.17, 0.2162, 0.0009), eye=0.14),
    # Without its front leg, which hangs straight down in the source, and its night sight (it filled the view).
    "pkm": dict(family="ak", length=1.173, forward="-y", up="+z", mag=[(0.57, 0.68, 0.05, 0.21)], drop_box=[(0.02, 0.10, 0.0, 0.25), (0.76, 1.0, 0.31, 0.45)],
                trigger=(0.773, 0.19), sight_rear=(0.728, 0.3219, -0.0018), sight_front=(0.05, 0.316, -0.0003), eye=0.3),
    "mp7": dict(family="smg", length=0.638, forward="+x", up="+z", mag=[(0.22, 0.32, -0.01, 0.10)],
                trigger=(0.235, 0.20), sight_rear=(0.369, 0.3219, -0.002), sight_front=(0.1025, 0.3208, -0.0006), eye=0.15),
    "bizon": dict(family="smg", length=0.66, forward="-x", up="+z", mag=[(0.04, 0.30, 0.05, 0.125)],
                  trigger=(0.36, 0.093), sight_rear=(0.216, 0.1775, 0.003), sight_front=(0.046, 0.1814, 0.0006), eye=0.15),
    "mp5": dict(family="smg", length=0.68, forward="-y", up="+z", mag=[(0.18, 0.28, -0.01, 0.17)],
                trigger=(0.335, 0.137), sight_rear=(0.389, 0.2578, -0.0011), sight_front=(0.0465, 0.2595, -0.0001), eye=0.15),
    "ump": dict(family="smg", length=0.69, forward="+x", up="+z", mag=[(0.17, 0.27, -0.01, 0.20)],
                trigger=(0.34, 0.18), sight_rear=(0.4026, 0.3005, 0), sight_front=(0.0867, 0.3031, 0), eye=0.15),
    "usp": dict(family="pistol", length=0.194, forward="-y", up="+z", mag=[(0.11, 0.22, -0.01, 0.06)], bolt=[(0.0, 0.2, 0.10, 0.15)],
                trigger=(0.098, 0.092), sight_rear=(0.1682, 0.1388, -0.001), sight_front=(0.012, 0.1387, 0), eye=0.2),
    "mp443": dict(family="pistol", length=0.198, forward="-x", up="+z", mag=[(0.14, 0.22, -0.01, 0.06)], bolt=[(0.0, 0.2, 0.105, 0.16)],
                  trigger=(0.107, 0.097), sight_rear=(0.1783, 0.1445, -0.0004), sight_front=(0.0102, 0.1445, 0), eye=0.2),
    # A VP9 for the P30 (the only P30 model is 264k triangles); without its weapon light.
    "p30": dict(family="pistol", length=0.186, forward="-y", up="+z", mag=[(0.12, 0.2, -0.01, 0.06)], bolt=[(0.0, 0.2, 0.10, 0.16)],
                drop_box=[(-0.01, 0.055, 0.06, 0.095)], trigger=(0.094, 0.09), sight_rear=(0.1726, 0.1337, 0), sight_front=(0.0153, 0.1337, 0), eye=0.2),
    # The cylinder stays put (the pack's reload drops a magazine).
    "revolver": dict(family="pistol", length=0.292, forward="+y", up="+z", roll=20, trigger=(0.184, 0.07), sight_rear=(0.189, 0.1314, 0.0002), sight_front=(0.006, 0.1335, 0.0002), eye=0.2),
    "benelli": dict(family="shotgun", length=0.885, forward="+y", up="+z", trigger=(0.571, 0.096), sight_rear=(0.575, 0.1596, -0.0022),
                    sight_front=(0.07, 0.163, -0.0001), eye=0.2),
}


def uses():
    """For CREDITS.md: what each source is used in."""
    out = Path(__file__).resolve().parents[2] / "assets" / "models"
    return {g.get("src", k): f"vm-{k}.glb (on the {g['family']} pack's rig, its clips), {k}.glb" for k, g in GUNS.items() if (out / f"{k}.glb").exists()}


def _axis(s: str) -> Vector:
    v = {"x": Vector((1, 0, 0)), "y": Vector((0, 1, 0)), "z": Vector((0, 0, 1))}[s[1]]
    return v if s[0] == "+" else -v


def prep(b, key: str):
    """The source turned, scaled and sorted into the form swap_in takes (see the module's doc)."""
    g = GUNS[key]
    b.reset()
    objs = b.load(b.source(g.get("src", key)))
    drop = tuple(g.get("drop", ()))
    meshes = [o for o in objs if o.type == "MESH" and not any(d in o.name.lower() or d in o.data.name.lower() for d in drop)]
    b.loose(meshes)
    b.remove([o for o in bpy.data.objects if o not in meshes])
    # Apply each piece's transform, then turn the whole: forward to -Y, up to +Z.
    for o in meshes:
        o.data = o.data.copy()
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)
        # One map each, of one name, so that joining pieces keeps every piece's (the maps read the first).
        for name in [lay.name for lay in o.data.uv_layers][1:]:
            o.data.uv_layers.remove(o.data.uv_layers[name])
        if o.data.uv_layers:
            o.data.uv_layers[0].name = "UVMap"
    f, u = _axis(g["forward"]), _axis(g["up"])
    r = f.cross(u)
    # Rows: f to -Y, u to +Z, and so f × u to -X (a turn, not a mirror).
    rot = Matrix((-r, -f, u))
    for o in meshes:
        o.data.transform(rot.to_4x4())
    # A source rolled about its long axis (seen from behind, its sights lean to one side): stood upright by `roll`
    # (degrees, positive turns the top towards +x).
    if g.get("roll"):
        for o in meshes:
            o.data.transform(Matrix.Rotation(math.radians(g["roll"]), 4, "Y"))
    # A source turned a little about its up axis (seen from the side it looks right, but the muzzle is off to one
    # side): straighten its long axis onto Y.
    import numpy as np
    xy = np.array([(p.x, p.y) for p in b.world_points(meshes)])
    w, v = np.linalg.eigh(np.cov((xy - xy.mean(0)).T))
    ax = v[:, 1]
    yaw = math.atan2(ax[0], ax[1])
    yaw = (yaw + math.pi / 2) % math.pi - math.pi / 2  # (the axis either way)
    if 0.002 < abs(yaw) < 0.35:
        print(f"{key}: straightened by {math.degrees(yaw):.1f} deg")
        for o in meshes:
            o.data.transform(Matrix.Rotation(yaw, 4, "Z"))
    pts = b.world_points(meshes)
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    k = g["length"] / (hi.y - lo.y)
    # The muzzle (the front, the lowest y) at y = 0, so y is the distance back from it; the bottom at z = 0, the
    # middle at x = 0.
    move = Matrix.Scale(k, 4) @ Matrix.Translation(Vector((-(lo.x + hi.x) / 2, -lo.y, -lo.z)))
    for o in meshes:
        o.data.transform(move)
    # Loose pieces, sorted by where their middle is.
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    whole = bpy.context.view_layer.objects.active
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    if g.get("weld"):
        # A triangle soup (every face on its own): weld it, or it falls apart into faces, decimates badly and
        # shades flat.
        bpy.ops.mesh.remove_doubles(threshold=0.0002)
        # (Its faces also wind inside out: drawn double-sided, the lights then fall on the inside.)
        bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.mesh.separate(type="LOOSE")
    bpy.ops.object.mode_set(mode="OBJECT")
    pieces = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    groups = {"body": [], "mag": [], "bolt": [], "trigger": []}
    # Loose pieces in a `drop_box` (a spare magazine lying beside the gun, a bipod's legs) go.
    for o in list(pieces):
        ps = [v.co for v in o.data.vertices]
        c = sum(ps, Vector()) / len(ps)
        if any(y0 <= c.y <= y1 and z0 <= c.z <= z1 for (y0, y1, z0, z1) in g.get("drop_box", [])):
            pieces.remove(o)
            bpy.data.objects.remove(o)
    # (After dropping, the bottom may have risen: the boxes are in the frame measured before. But what was dropped may
    # have lain beside the gun: centre it across again.)
    # (On the muzzle's middle, not the box's: a charging handle or a sling mount on one side would pull the box.)
    tip = [v.co.x for o in pieces for v in o.data.vertices if v.co.y < 0.02 * g["length"]]
    cx = (min(tip) + max(tip)) / 2
    for o in pieces:
        o.data.transform(Matrix.Translation(Vector((-cx, 0.0, 0.0))))
    boxes = {"mag": g.get("mag", []), "bolt": g.get("bolt", []), "trigger": g.get("trigger_box", [])}
    for o in pieces:
        ps = [v.co for v in o.data.vertices]
        c = sum(ps, Vector()) / len(ps)
        name = next((n for n, bs in boxes.items() for (y0, y1, z0, z1) in bs if y0 <= c.y <= y1 and z0 <= c.z <= z1), "body")
        groups[name].append(o)
    if g.get("weld"):
        for o in pieces:
            o.data.shade_smooth()
            o.data.set_sharp_from_angle(angle=math.radians(35))
    for name, os_ in groups.items():
        if not os_:
            continue
        bpy.ops.object.select_all(action="DESELECT")
        for o in os_:
            o.select_set(True)
        bpy.context.view_layer.objects.active = os_[0]
        if len(os_) > 1:
            bpy.ops.object.join()
        o = bpy.context.view_layer.objects.active
        o.name = name
        o.data.name = name
    for name in ("trigger_point", "sight_rear", "sight_front"):
        at = g.get(name.replace("_point", ""))
        if at:
            e = bpy.data.objects.new(name, None)
            bpy.context.scene.collection.objects.link(e)
            e.location = Vector((at[2] if len(at) > 2 else 0.0, at[0], at[1]))
    # Guns are solid: some sources' materials come alpha-blended (the whole gun drawn see-through).
    for m in bpy.data.materials:
        m.blend_method = "OPAQUE"
        bsdf = next((n for n in (m.node_tree.nodes if m.node_tree else []) if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf:
            for link in list(bsdf.inputs["Alpha"].links):
                m.node_tree.links.remove(link)
            bsdf.inputs["Alpha"].default_value = 1.0
            # A source without a roughness map, too glossy for its finish (`rough`).
            if g.get("rough") and not bsdf.inputs["Roughness"].links:
                bsdf.inputs["Roughness"].default_value = g["rough"]
    b.shrink_textures(2048)
    out = b.CACHE / f"{key}-prep"
    out.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=str(out / f"{key}.glb"), export_format="GLB", export_yup=True, export_apply=False, export_image_format="JPEG", export_jpeg_quality=92, export_extras=True, export_cameras=False, export_lights=False)
    print(f"{key}: prepped, {g['length']:.3f} m, pieces: " + ", ".join(f"{n} {len(v)}" for n, v in groups.items()))


def measure(b, key: str):
    """The prepped model from its right side (the muzzle to the right), a 5 cm grid, each loose piece numbered, for
    reading off the boxes and points in GUNS: y (m back from the muzzle) runs right to left, z up."""
    out = Path(os.environ.get("MEASURE_OUT", "/tmp"))
    out.mkdir(parents=True, exist_ok=True)
    b.reset()
    objs = b.load(b.CACHE / f"{key}-prep" / f"{key}.glb")
    meshes = [o for o in objs if o.type == "MESH"]
    pts = b.world_points(meshes)
    L = max(p.y for p in pts)
    H = max(p.z for p in pts)
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.color_type = "TEXTURE"
    scene.render.resolution_x = 2000
    scene.render.resolution_y = int(2000 * (H + 0.1) / (L + 0.1))
    scene.render.film_transparent = False
    scene.world = bpy.data.worlds.new("w")
    scene.world.color = (0.8, 0.8, 0.8)
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = L + 0.1
    # From -X looking along +X (the gun's right side): the muzzle (y = 0) is to the right.
    cam.location = (-2.0, L / 2, H / 2)
    cam.rotation_euler = (math.pi / 2, 0, -math.pi / 2)
    # Grid lines every 5 cm (thin boxes behind the gun), heavier every 10.
    mat = bpy.data.materials.new("grid")
    for i in range(int(L / 0.05) + 3):
        y = i * 0.05
        bpy.ops.mesh.primitive_cube_add(size=1, location=(0.3, y, H / 2))
        c = bpy.context.object
        c.scale = (0.001, 0.0015 if i % 2 else 0.003, H + 0.1)
    for i in range(int(H / 0.05) + 3):
        z = i * 0.05
        bpy.ops.mesh.primitive_cube_add(size=1, location=(0.3, L / 2, z))
        c = bpy.context.object
        c.scale = (0.001, L + 0.1, 0.0015 if i % 2 else 0.003)
    del mat
    # Labels (cm) every 10 cm: along the bottom (back from the muzzle) and up the right edge.
    def label(t, y, z):
        cu = bpy.data.curves.new("t", type="FONT")
        cu.body = t
        cu.size = 0.018
        cu.align_x = "CENTER"
        ob = bpy.data.objects.new("t", cu)
        scene.collection.objects.link(ob)
        ob.location = (-0.2, y, z)
        ob.rotation_euler = (math.pi / 2, 0, -math.pi / 2)
    for i in range(int(L / 0.1) + 2):
        label(str(i * 10), i * 0.1, -0.035)
    for i in range(int(H / 0.1) + 2):
        label(str(i * 10), -0.025, i * 0.1 - 0.006)
    cam.data.ortho_scale = L + 0.16
    cam.location = (-2.0, L / 2 - 0.02, H / 2 - 0.02)
    scene.render.resolution_y = int(2000 * (H + 0.16) / (L + 0.16))
    path = out / f"measure-{key}.png"
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    # The pieces: their numbers and middles, printed (the render is too small for labels on the little ones).
    for o in meshes:
        ps = [o.matrix_world @ v.co for v in o.data.vertices]
        c = sum(ps, Vector()) / len(ps)
        print(f"  {o.name}: middle y {c.y:.3f} z {c.z:.3f}, y {min(p.y for p in ps):.3f}..{max(p.y for p in ps):.3f} z {min(p.z for p in ps):.3f}..{max(p.z for p in ps):.3f}")
    print(f"{key}: {L:.3f} m long, {H:.3f} m tall -> {path}")


def vm(b, key: str):
    g = GUNS[key]
    fam = FAMILIES[g["family"]]
    b.vm_pack(f"vm-{key}", fam["src"], fam["clips"], fam["body"], length=g["length"], shift=g.get("shift", fam["shift"]),
              eject_at=g.get("eject_at", 0.45), swap=_swap(b, key), **fam["eye"])


def prop(b, key: str):
    g = GUNS[key]
    fam = FAMILIES[g["family"]]
    b.prop_pack(key, fam["src"], fam["clips"]["idle"], fam["body"], fam["hand"], g["length"], swap=_swap(b, key),
                grip_at=fam.get("grip_at"))


def _swap(b, key):
    g = GUNS[key]
    fam = FAMILIES[g["family"]]
    bones = []
    if fam["mag"] and g.get("mag"):
        bones.append((("mag",), fam["mag"]))
    if fam["bolt"] and g.get("bolt"):
        bones.append((("bolt",), fam["bolt"]))
    spare = (fam["mag"], fam["spare"], ("mag",)) if fam["spare"] and g.get("mag") else None
    return b.swap_in(f"{key}-prep", fam["clips"]["idle"], fam["pack_len"], fam["trigger"], fam["body"], bones=bones, spare=spare,
                     bore=fam.get("bore"), measure=fam.get("measure"), upright=fam.get("upright", False))


def recipes(b):
    """The build.py recipes for every gun: prep-, measure-, vm- and the prop."""
    out = {}
    for key in GUNS:
        out[f"prep-{key}"] = (lambda k: lambda: prep(b, k))(key)
        out[f"measure-{key}"] = (lambda k: lambda: measure(b, k))(key)
        out[f"vm-{key}"] = (lambda k: lambda: vm(b, k))(key)
        out[key] = (lambda k: lambda: prop(b, k))(key)
    return out
