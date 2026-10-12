"""Builds ZUGRIFF's models (first-person viewmodels, weapon props, characters) from the sources fetch.py downloaded
into .cache/ (sources.json has their authors and licences), with Blender run headless:

    ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/build.py [name ...]

(Adapted from Nexus Sim's `apps/bevy-spike/bevy/tools/infantry/build.py`, which first built vm-rifle, vm-pistol,
vm-hands, the M4, pistol, knife and grenade props and the soldiers; those recipes are kept here unchanged.)

Each recipe below imports a source, tidies it (junk meshes out, clips cut to start at zero and renamed, textures
shrunk to JPEG, meshes decimated to a budget), puts it in the frame the game expects, adds the named markers the
game looks up, and writes a GLB into ../../assets/models:
- viewmodels (`vm-*.glb`): arms and weapon in the camera's frame: the eye at the origin, looking along -Z (glTF), +Y
  up. Clips `idle`, `fire`, `reload`, `reload_empty`, `draw`, `walk` where the pack has them. Markers: `sight-eye`
  (where the eye goes aimed down the sights; the sight line runs along -Z through it), `muzzle`, `ejection`.
  (vm-ak, vm-sniper, vm-shotgun, vm-smg, vm-mg come from ccransh's FPS packs; see vm_pack and swap_in.)
- props (`knife.glb`, `grenade.glb`, `m4.glb`, `pistol.glb`, `ak.glb`, `sniper.glb`, `shotgun.glb`, `smg.glb`,
  `mg.glb`): a weapon on its own, its grip at the origin, the muzzle or blade along -Z, a `muzzle` marker, for the
  hands (first and third person).
- soldiers (`soldier-*.glb`) and the hostage (`hostage.glb`): skinned characters with the Universal Animation
  Library's clips retargeted onto them.
It also writes CREDITS.md next to the models and prints what it built against the budget.
"""

import json
import math
import os
import re
import sys
from pathlib import Path

import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

HERE = Path(__file__).resolve().parent
CACHE = HERE / ".cache"
OUT = HERE.parents[1] / "assets" / "models"
SOURCES = json.loads((HERE / "sources.json").read_text())


# ---- helpers ----------------------------------------------------------------------------------------------------


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = 30


def source(name: str) -> Path:
    found = sorted((CACHE / name).rglob("*.gltf")) + sorted((CACHE / name).rglob("*.glb"))
    if not found:
        sys.exit(f"{name}: not fetched (run fetch.py)")
    return found[0]


def load(path: Path, bind_pose: bool = False) -> list:
    before = set(bpy.data.objects)
    # The skins' own inverse bind matrices, not the importer's guess at a bind pose (which blows up the pistol pack's
    # arms, whose mesh and skeleton nodes carry different scales). `bind_pose` takes the bind pose as the rest instead
    # of the nodes' own transforms (which, for the hostage, are a frame of its walk, not a standing pose).
    bpy.ops.import_scene.gltf(filepath=str(path), guess_original_bind_pose=bind_pose)
    return [o for o in bpy.data.objects if o not in before]


def remove(objs):
    for o in list(objs):
        bpy.data.objects.remove(o, do_unlink=True)


def without(objs, junk) -> list:
    """Removes the objects `junk` picks out (a stray icosphere and the like); returns the rest."""
    gone = [o for o in objs if junk(o)]
    keep = [o for o in objs if not junk(o)]
    remove(gone)
    return keep


def armature(objs=None):
    return next(o for o in (objs or bpy.data.objects) if o.type == "ARMATURE")


def meshes(objs=None):
    return [o for o in (objs or bpy.data.objects) if o.type == "MESH"]


def shrink_textures(size: int):
    """Every image no larger than `size` px (the exporter writes them as JPEG)."""
    for im in bpy.data.images:
        if im.size[0] == 0:
            continue
        w, h = im.size
        k = min(1.0, size / max(w, h))
        if k < 1.0:
            im.scale(max(1, int(w * k)), max(1, int(h * k)))


def decimate(objs, target_tris: int):
    """Collapses the meshes together to about `target_tris` triangles, keeping their skin weights."""
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in objs)
    if tris <= target_tris:
        return tris
    ratio = target_tris / tris
    for o in objs:
        m = o.modifiers.new("decimate", "DECIMATE")
        m.ratio = ratio
        m.use_collapse_triangulate = True
        # Before the armature modifier, applied on the rest shape.
        while o.modifiers.find("decimate") > 0:
            bpy.context.view_layer.objects.active = o
            bpy.ops.object.modifier_move_up(modifier="decimate")
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.modifier_apply(modifier="decimate")
    return sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in objs)


def keyframe_range(action):
    ks = [k.co.x for fc in action.fcurves for k in fc.keyframe_points]
    return (min(ks), max(ks)) if ks else (0.0, 0.0)


def clear_nla():
    """Drops the tracks the importer made for the source's own animations (the recipe makes its own)."""
    for o in bpy.data.objects:
        if o.animation_data:
            for t in list(o.animation_data.nla_tracks):
                o.animation_data.nla_tracks.remove(t)
            o.animation_data.action = None


def cut(arm, action_name: str, out_name: str, frames=None):
    """A copy of an action named `out_name`, shifted to start at frame 0 (from `frames`, or its own first key)."""
    src = bpy.data.actions[action_name]
    a = src.copy()
    a.name = out_name
    lo, hi = frames or keyframe_range(src)
    for fc in a.fcurves:
        # By index from the end: removing a point invalidates references to the others.
        pts = fc.keyframe_points
        for i in reversed(range(len(pts))):
            if pts[i].co.x < lo - 1e-3 or pts[i].co.x > hi + 1e-3:
                pts.remove(pts[i], fast=True)
        fc.update()
        for k in fc.keyframe_points:
            k.co.x -= lo
            k.handle_left.x -= lo
            k.handle_right.x -= lo
    a.use_fake_user = True
    stash(arm, a)
    return a


def still(arm, action_name: str, frame: float, out_name: str):
    """A one-pose action: the armature as `action_name` has it at `frame`."""
    arm.animation_data_create()
    arm.animation_data.action = bpy.data.actions[action_name]
    bpy.context.scene.frame_set(int(frame), subframe=frame % 1)
    a = bpy.data.actions.new(out_name)
    a.use_fake_user = True
    arm.animation_data.action = a
    for pb in arm.pose.bones:
        for path in ("location", "rotation_quaternion", "scale"):
            pb.keyframe_insert(path, frame=0)
            pb.keyframe_insert(path, frame=1)
    arm.animation_data.action = None
    stash(arm, a)
    return a


def stash(arm, action):
    """Keeps an action on the armature as an NLA track, so the exporter writes it as its own animation."""
    arm.animation_data_create()
    track = arm.animation_data.nla_tracks.new()
    track.name = action.name
    track.strips.new(action.name, 0, action)
    track.mute = True


def drop_actions(keep: set):
    for a in list(bpy.data.actions):
        if a.name not in keep:
            bpy.data.actions.remove(a)


def drop_rest_channels(arm, actions) -> tuple:
    """Takes out the curves of each bone property (location, rotation, scale) that stays at the rest in every one of
    `actions`, and puts those bones at the rest, so that (exported with `bone_channels=False`) the animations only
    carry what moves: Bevy evaluates every channel of a playing clip each frame, and the retargeted and baked clips
    key every bone (fingers, face, twist bones) whether it moves or not. A property is dropped from all clips or
    none, so no clip leaves a bone where another put it. Returns the curves (kept, dropped)."""
    import re

    rest = {"location": (0, 0, 0), "rotation_quaternion": (1, 0, 0, 0), "rotation_euler": (0, 0, 0),
            "scale": (1, 1, 1)}
    pat = re.compile(r'^pose\.bones\["(.+)"\]\.(\w+)$')
    moving = set()
    curves = []
    for a in actions:
        for fc in a.fcurves:
            m = pat.match(fc.data_path)
            if not m or m.group(2) not in rest:
                continue
            key = (m.group(1), m.group(2))
            curves.append((a, fc, key))
            want = rest[key[1]][fc.array_index]
            if any(abs(k.co.y - want) > 1e-4 for k in fc.keyframe_points):
                moving.add(key)
    dropped = [(a, fc) for a, fc, key in curves if key not in moving]
    for a, fc in dropped:
        a.fcurves.remove(fc)
    # And what moves in one clip has curves in all (at the rest where a clip has none), for the same reason.
    for a in actions:
        have = {(fc.data_path, fc.array_index) for fc in a.fcurves}
        end = keyframe_range(a)[1]
        for bone, prop in moving:
            path = f'pose.bones["{bone}"].{prop}'
            for i, v in enumerate(rest[prop]):
                if (path, i) not in have:
                    fc = a.fcurves.new(path, index=i, action_group=bone)
                    fc.keyframe_points.insert(0, v)
                    fc.keyframe_points.insert(end, v)
    for pb in arm.pose.bones:
        for prop in ("location", "rotation_quaternion", "rotation_euler", "scale"):
            if (pb.name, prop) not in moving:
                setattr(pb, prop, rest[prop])
    return len(curves) - len(dropped), len(dropped)


def pose_at(arm, action_name: str, frame: float):
    arm.animation_data_create()
    arm.animation_data.action = bpy.data.actions[action_name]
    bpy.context.scene.frame_set(int(frame))


def marker(name: str, arm, bone: str, world: Vector):
    """An empty named `name` riding on `bone`, at `world` (in the current pose)."""
    e = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(e)
    e.parent = arm
    e.parent_type = "BONE"
    e.parent_bone = bone
    bpy.context.view_layer.update()
    e.matrix_world = Matrix.Translation(world)
    return e


def into_camera_frame(arm, eye: Vector, forward: Vector):
    """Moves the armature (and all it carries) so that `eye` is at the origin looking along +Y in Blender, which
    the exporter turns into -Z in glTF: the camera's forward in Bevy."""
    f = forward.normalized()
    turn = f.to_track_quat("Y", "Z").to_matrix().to_4x4().inverted()
    arm.matrix_world = turn @ Matrix.Translation(-eye) @ arm.matrix_world
    bpy.context.view_layer.update()


def export(name: str, only=None, bone_channels: bool = True):
    """Writes the scene (or just `only` and what hangs from it) as models/<name>.glb. Without `bone_channels`, bones
    with no curves in an action get no channels in its animation (see drop_rest_channels)."""
    path = OUT / f"{name}.glb"
    if only is not None:
        bpy.ops.object.select_all(action="DESELECT")
        for o in only:
            o.select_set(True)
    bpy.context.scene.frame_set(0)
    for o in bpy.data.objects:
        if o.animation_data:
            o.animation_data.action = None
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        export_yup=True,
        export_apply=False,
        export_animations=True,
        export_animation_mode="NLA_TRACKS",
        export_nla_strips_merged_animation_name="",
        export_force_sampling=True,
        export_optimize_animation_size=True,
        export_optimize_animation_keep_anim_armature=bone_channels,
        export_image_format="JPEG",
        export_jpeg_quality=86,
        export_extras=True,
        export_cameras=False,
        export_lights=False,
        use_selection=only is not None,
    )
    print(f"{name}.glb {path.stat().st_size / 1e6:.2f} MB")
    return path


# ---- recipes ----------------------------------------------------------------------------------------------------


def sleeves(mat_name: str, u_to: float, cuff: float = 0.025, seed: int = 5):
    """Paints sleeves over bare forearms: in material `mat_name`'s colour and normal maps, everything left of
    `u_to` (the wrist, where the glove begins) becomes olive-drab ripstop with a darker, ribbed cuff; the skin of the
    hands (right of it) stays. The other packs' arms wear such sleeves; the M4 pack's are bare to the elbow."""
    import numpy as np

    mat = bpy.data.materials[mat_name]
    ims = [n.image for n in mat.node_tree.nodes if n.type == "TEX_IMAGE" and n.image]
    colour = next(im for im in ims if "diffuse" in im.name.lower() or "basecolor" in im.name.lower())
    nmap = next((n.image for n in mat.node_tree.nodes if n.type == "TEX_IMAGE" and n.image and "normal" in n.image.name.lower()), None)
    for im, is_normal in ((colour, False), (nmap, True)):
        if im is None:
            continue
        w, h = im.size
        px = np.empty(w * h * 4, dtype=np.float32)
        im.pixels.foreach_get(px)
        px = px.reshape(h, w, 4)
        u = (np.arange(w) + 0.5) / w
        rng = np.random.default_rng(seed)
        # Ripstop: a faint grid every ~6 px, fibre noise, and large soft blotches of wear.
        yy, xx = np.mgrid[0:h, 0:w]
        grid = ((xx % 6 == 0) | (yy % 6 == 0)).astype(np.float32)
        fibre = rng.random((h, w)).astype(np.float32)
        coarse = rng.random((h // 64 + 2, w // 64 + 2)).astype(np.float32)
        blot = np.kron(coarse, np.ones((64, 64), dtype=np.float32))[:h, :w]
        mask = np.clip((u_to - u) / 0.004, 0, 1)[None, :].repeat(h, 0)
        band = ((u > u_to - cuff) & (u < u_to))[None, :].repeat(h, 0)
        if is_normal:
            fab = np.zeros((h, w, 3), dtype=np.float32)
            fab[..., 0] = 0.5 + (fibre - 0.5) * 0.06 + grid * 0.03
            fab[..., 1] = 0.5 + (np.roll(fibre, 1, 0) - 0.5) * 0.06
            fab[..., 2] = 1.0
            rib = np.sin(xx * 1.6)[..., None] * 0.18
            fab[..., 0:1] = np.where(band[..., None], 0.5 + rib, fab[..., 0:1])
        else:
            base = np.array([0.062, 0.058, 0.036], dtype=np.float32)  # linear olive drab (sRGB ≈ 70, 67, 53)
            k = (0.85 + 0.2 * blot + 0.1 * (fibre - 0.5) - 0.12 * grid)[..., None]
            fab = base * k
            fab = np.where(band[..., None], fab * 0.62, fab)
        px[..., :3] = px[..., :3] * (1 - mask[..., None]) + fab * mask[..., None]
        im.pixels.foreach_set(px.ravel())
        im.update()


def vm_rifle():
    """The M4 pack: arms, rifle, and its clips (all on one timeline; cut apart here)."""
    reset()
    objs = load(source("rifle"))
    objs = without(objs, lambda o: o.name.startswith("Icosphere"))
    sleeves("Hand_D", 0.855)
    arm = armature(objs)
    act = lambda n: next(a.name for a in bpy.data.actions if a.name.startswith(n + "_") and arm.name in a.name)
    clips = {
        "draw": act("Draw"),
        "fire": act("Fire"),
        "reload": act("Reload"),
        "reload_empty": act("Reload_Empty"),
        "holster": act("Holster"),
    }
    draw_end = keyframe_range(bpy.data.actions[clips["draw"]])[1]
    # The eye: the pack's camera joint, at the end of the draw (the rest of the clips start and end there).
    pose_at(arm, clips["draw"], draw_end)
    cam = arm.matrix_world @ arm.pose.bones["FPS_Camera_j_01"].matrix
    eye = cam.to_translation()
    main = next(o for o in objs if o.name.startswith("Main_M4"))
    pts = [main.matrix_world @ v.co for v in main.data.vertices]
    muzzle = min(pts, key=lambda p: p.y)
    top = max(p.z for p in pts)
    into_camera_frame(arm, eye, Vector((0, -1, 0)))
    clear_nla()
    for name, a in clips.items():
        cut(arm, a, name)
    still(arm, "draw", keyframe_range(bpy.data.actions["draw"])[1], "idle")
    drop_actions({"idle", *clips})
    pose_at(arm, "idle", 0)
    # Markers on the rifle's bone, placed in the camera frame.
    main_bone = main.parent_bone or "M4_Root_00"
    back = Matrix.Translation(-eye)
    turn = Vector((0, -1, 0)).to_track_quat("Y", "Z").to_matrix().to_4x4().inverted()
    to_cam = turn @ back
    m = to_cam @ muzzle
    marker("muzzle", arm, main_bone, m)
    marker("ejection", arm, main_bone, to_cam @ Vector((muzzle.x + 0.03, eye.y - 0.25, top - 0.05)))
    # Aimed down the sights: the eye over the rear sight, on the line of the bore's top.
    sight = Vector((m.x, 0.0, (to_cam @ Vector((0, 0, top))).z + 0.035))
    marker("sight-eye", arm, main_bone, sight)
    shrink_textures(1024)
    finish_uv([o for o in meshes(objs) if not is_arms(o) and not o.data.name.startswith("Hand_Mesh")])
    export("vm-rifle")


def vm_pistol():
    """The pistol pack: arms, a sub-compact pistol and five clips. Its origin is the eye, looking along -Y."""
    reset()
    objs = load(source("pistol"))
    objs = without(objs, lambda o: o.name.startswith("Icosphere"))
    arm = armature(objs)
    act = lambda n: next(a.name for a in bpy.data.actions if f"FPS_Pistol_{n}_" in a.name)
    clips = {"idle": act("Idle"), "walk": act("Walk"), "fire": act("Fire"), "reload": act("Reload_easy"), "reload_empty": act("Reload_full")}
    # The eye: the pack's head camera joint at the start of the idle.
    pose_at(arm, clips["idle"], 0)
    eye = (arm.matrix_world @ arm.pose.bones["Head_Cam_014"].matrix).to_translation()
    into_camera_frame(arm, eye, Vector((0, -1, 0)))
    clear_nla()
    for name, a in clips.items():
        cut(arm, a, name)
    drop_actions(set(clips))
    pose_at(arm, "idle", 0)
    # The pistol's parts: the small meshes (the arms are the big one).
    parts = sorted(meshes(objs), key=lambda o: len(o.data.polygons))[:-1]
    dg = bpy.context.evaluated_depsgraph_get()
    pts = [o.evaluated_get(dg).matrix_world @ v.co for o in parts for v in o.evaluated_get(dg).data.vertices]
    muzzle = min(pts, key=lambda p: p.y)
    top = max(p.z for p in pts)
    bone = "PBody_058"
    marker("muzzle", arm, bone, muzzle)
    # (In the camera frame now: forward is +Y, so behind the muzzle is -Y.)
    marker("ejection", arm, bone, Vector((muzzle.x + 0.02, muzzle.y - 0.1, top - 0.01)))
    marker("sight-eye", arm, bone, Vector((muzzle.x, muzzle.y - 0.3, top + 0.008)))
    shrink_textures(1024)
    decimate([max(meshes(objs), key=lambda o: len(o.data.polygons))], 12000)
    finish_uv(parts)
    export("vm-pistol")


def place(objs, grip: Vector, forward: Vector, up: Vector, scale: float = 1.0):
    """Puts loose meshes in the props' frame: `grip` at the origin, `forward` along +Y and `up` along +Z in Blender
    (-Z and +Y in glTF), scaled by `scale`, and applies it to their data."""
    f = forward.normalized()
    u = (up - f * up.dot(f)).normalized()
    r = f.cross(u)
    basis = Matrix((r, f, u)).to_4x4()  # rows: where each world axis goes
    m = Matrix.Scale(scale, 4) @ basis @ Matrix.Translation(-grip)
    for o in objs:
        o.matrix_world = m @ o.matrix_world
    bpy.context.view_layer.update()
    for o in objs:
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)


def loose(objs):
    """Unparents meshes keeping where they are (and bakes a skin's current pose into a rigid mesh)."""
    for o in objs:
        w = o.matrix_world.copy()
        for mod in list(o.modifiers):
            if mod.type == "ARMATURE":
                bpy.context.view_layer.objects.active = o
                bpy.ops.object.modifier_apply(modifier=mod.name)
                w = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = w
        for g in list(o.vertex_groups):
            o.vertex_groups.remove(g)


def world_points(objs):
    dg = bpy.context.evaluated_depsgraph_get()
    return [o.evaluated_get(dg).matrix_world @ v.co for o in objs for v in o.evaluated_get(dg).data.vertices]


def finish_uv(parts):
    """A second UV map, `finish` (TEXCOORD_1), on the weapon's parts: where each vertex is on the weapon seen from its
    side, in metres — u from the back end forward, v from the bottom up — as it is posed now, pointing along +Y. The
    game paints its finishes (cosmetic paint schemes) through it; the arms don't get one, which is how the game tells
    the weapon's meshes from theirs. A reload's spare magazine (`spare-*`) takes the map of the part it copies."""
    dg = bpy.context.evaluated_depsgraph_get()
    # Rounds and shells stay brass.
    parts = [o for o in parts if not any(k in o.name.lower() or k in o.data.name.lower() for k in ("bullet", "shell", "12ge", "cartridge"))]
    # Only the paintable parts have a second map (some packs bring spare ones: the weapons' maps never use them).
    for o in bpy.context.scene.objects:
        if o.type == "MESH":
            for name in [lay.name for lay in o.data.uv_layers][1:]:
                o.data.uv_layers.remove(o.data.uv_layers[name])
    main = [o for o in parts if not o.name.startswith("spare-")]
    # The weapon's extent: the body (the longest part) and the parts on it, not a pack's spare magazine held elsewhere.
    box = {o: world_points([o]) for o in main}
    mid = lambda ps, k: sum(getattr(p, k) for p in ps) / len(ps)
    body = max(main, key=lambda o: max(p.y for p in box[o]) - min(p.y for p in box[o]))
    b = box[body]
    y0, y1, z0, z1, cx = min(p.y for p in b), max(p.y for p in b), min(p.z for p in b), max(p.z for p in b), mid(b, "x")
    on = [o for o in main if box[o] and y0 - 0.05 <= mid(box[o], "y") <= y1 + 0.05 and abs(mid(box[o], "x") - cx) < 0.12
          and z0 - 0.3 <= mid(box[o], "z") <= z1 + 0.1]
    pts = [p for o in on for p in box[o]]
    back, bottom = min(p.y for p in pts), min(p.z for p in pts)
    if os.environ.get("FINISH_DEBUG"):
        for o in main:
            q = box[o]
            print(f"    {'*' if o in on else ' '} {o.name}: y {min(p.y for p in q):.3f}..{max(p.y for p in q):.3f} z {min(p.z for p in q):.3f}..{max(p.z for p in q):.3f} x~{mid(q, 'x'):.3f}")
    done = {}
    for o in main + [o for o in parts if o.name.startswith("spare-")]:
        me = o.data
        if "finish" in me.uv_layers:
            me.uv_layers.remove(me.uv_layers["finish"])
        first = me.uv_layers[0].name if me.uv_layers else None
        if first is None:
            me.uv_layers.new(name="UVMap")
        lay = me.uv_layers.new(name="finish")
        src = done.get(o.name[len("spare-"):]) if o.name.startswith("spare-") else None
        if src is not None and len(src) == len(me.loops):
            for i, uv in enumerate(src):
                lay.data[i].uv = uv
            continue
        e = o.evaluated_get(dg)
        em = e.data
        if len(em.vertices) != len(me.vertices):
            em = e.to_mesh()
        pos = [e.matrix_world @ v.co for v in em.vertices]
        uvs = [(pos[l.vertex_index].y - back, pos[l.vertex_index].z - bottom) for l in me.loops]
        for i, uv in enumerate(uvs):
            lay.data[i].uv = uv
        done[o.name] = uvs
        me.uv_layers.active = me.uv_layers[0]
    print(f"  finish uv: {len(parts)} parts, {max(p.y for p in pts) - back:.3f} m long, {max(p.z for p in pts) - bottom:.3f} m tall")


def prop_m4():
    """The M4 on its own (for soldiers' hands): the rifle pack's rifle meshes, as held at rest."""
    reset()
    objs = load(source("rifle"))
    arm = armature(objs)
    draw = next(a.name for a in bpy.data.actions if a.name.startswith("Draw_") and arm.name in a.name)
    pose_at(arm, draw, keyframe_range(bpy.data.actions[draw])[1])
    grip = (arm.matrix_world @ arm.pose.bones["RightHand_05"].matrix).to_translation()
    rifle = [o for o in meshes(objs) if "M4" in o.name and "Mag2" not in o.name or o.name.startswith("Bullet5") and o.parent and o.parent.name == "Bullet5_56x45mm"]
    loose(rifle)
    remove([o for o in bpy.data.objects if o not in rifle])
    main = next(o for o in rifle if o.name.startswith("Main_M4"))
    pts = [main.matrix_world @ v.co for v in main.data.vertices]
    muzzle = min(pts, key=lambda p: p.y)
    place(rifle, grip, Vector((0, -1, 0)), Vector((0, 0, 1)))
    add_point("muzzle", Vector((0, grip.y - muzzle.y, muzzle.z - grip.z)))
    finish_uv(rifle)
    shrink_textures(512)
    export("m4", only=[o for o in bpy.data.objects])


def prop_pistol():
    """The pistol on its own, as held at rest."""
    reset()
    objs = load(source("pistol"))
    objs = without(objs, lambda o: o.name.startswith("Icosphere"))
    arm = armature(objs)
    idle = next(a.name for a in bpy.data.actions if "FPS_Pistol_Idle_" in a.name)
    pose_at(arm, idle, 0)
    grip = (arm.matrix_world @ arm.pose.bones["PBody_058"].matrix).to_translation()
    parts = sorted(meshes(objs), key=lambda o: len(o.data.polygons))[:-1]
    loose(parts)
    remove([o for o in bpy.data.objects if o not in parts])
    pts = world_points(parts)
    muzzle = min(pts, key=lambda p: p.y)
    place(parts, grip, Vector((0, -1, 0)), Vector((0, 0, 1)))
    add_point("muzzle", Vector((0, grip.y - muzzle.y, muzzle.z - grip.z)))
    finish_uv(parts)
    shrink_textures(512)
    export("pistol", only=list(bpy.data.objects))


def add_point(name: str, at: Vector):
    e = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(e)
    e.location = at
    return e


def prop_knife():
    """The Ka-Bar "Classic": 30 cm, the grip at the origin, the blade forward, its edge down."""
    reset()
    objs = load(source("knife"))
    knife = next(o for o in meshes(objs) if any("Classic" in (s.material.name if s.material else "") for s in o.material_slots))
    loose([knife])
    remove([o for o in bpy.data.objects if o is not knife])
    pts = world_points([knife])
    lo = min(p.y for p in pts)
    hi = max(p.y for p in pts)
    length = hi - lo
    # The handle is the thicker end (the blade is thin): compare the thickness in the last fifth at each end.
    thick = lambda sel: max((p.z for p in pts if sel(p)), default=0) - min((p.z for p in pts if sel(p)), default=0)
    handle_low = thick(lambda p: p.y < lo + length * 0.2) > thick(lambda p: p.y > hi - length * 0.2)
    handle_end = lo if handle_low else hi
    forward = Vector((0, 1, 0)) if handle_low else Vector((0, -1, 0))
    cx = sum(p.x for p in pts) / len(pts)
    cz = sum(p.z for p in pts) / len(pts)
    grip = Vector((cx, handle_end + (forward.y) * length * 0.18, cz))
    # Lying flat: the blade's width is across x; stand it up with the width vertical.
    place([knife], grip, forward, Vector((1, 0, 0)), 0.30 / length)
    shrink_textures(512)
    export("knife", only=[knife])


def olive(image, strength: float = 1.0):
    """Turns an image's blue paint (the M69 is the blue practice round) olive drab, leaving the metal as it is."""
    px = list(image.pixels)
    for i in range(0, len(px), 4):
        r, g, b = px[i], px[i + 1], px[i + 2]
        if b > r + 0.08 and b > g:
            lum = 0.3 * r + 0.59 * g + 0.11 * b
            px[i], px[i + 1], px[i + 2] = lum * 0.62, lum * 0.7, lum * 0.42
    image.pixels[:] = px


def prop_grenade():
    """The M69: 10 cm tall, its middle at the origin, the fuze up, repainted olive drab."""
    reset()
    objs = load(source("grenade"))
    g = meshes(objs)
    loose(g)
    remove([o for o in bpy.data.objects if o not in g])
    pts = world_points(g)
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    place(g, (lo + hi) / 2, Vector((0, 1, 0)), Vector((0, 0, 1)), 0.10 / (hi.z - lo.z))
    shrink_textures(512)
    for im in bpy.data.images:
        if "baseColor" in im.name and im.size[0] > 0:
            olive(im)
    export("grenade", only=g)


def vm_hands():
    """Empty hands for the knife and the grenade: the pistol pack's arms without the pistol, the left arm folded away
    out of view, and `hand-r` where the pistol's grip was (the knife and grenade hang from it)."""
    reset()
    objs = load(source("pistol"))
    objs = without(objs, lambda o: o.name.startswith("Icosphere"))
    arm = armature(objs)
    ms = sorted(meshes(objs), key=lambda o: len(o.data.polygons))
    objs = without(objs, lambda o: o in ms[:-1])
    idle = next(a.name for a in bpy.data.actions if "FPS_Pistol_Idle_" in a.name)
    walk = next(a.name for a in bpy.data.actions if "FPS_Pistol_Walk_" in a.name)
    pose_at(arm, idle, 0)
    eye = (arm.matrix_world @ arm.pose.bones["Head_Cam_014"].matrix).to_translation()
    into_camera_frame(arm, eye, Vector((0, -1, 0)))
    clear_nla()
    for name, a in (("idle", idle), ("walk", walk)):
        act = cut(arm, a, name)
        # The left arm shrunk to nothing at the shoulder, behind the eye.
        for axis in range(3):
            fc = act.fcurves.new('pose.bones["Arm_L_02"].scale', index=axis, action_group="Arm_L_02")
            fc.keyframe_points.insert(0, 0.001)
    drop_actions({"idle", "walk"})
    pose_at(arm, "idle", 0)
    grip = (arm.matrix_world @ arm.pose.bones["PBody_058"].matrix).to_translation()
    marker("hand-r", arm, "PBody_058", grip)
    shrink_textures(1024)
    decimate([ms[-1]], 12000)
    export("vm-hands")


def knife_rig():
    """bumstrum's animated first-person knife (both arms, the knife in the right hand), in the game's frame: one
    timeline, the knife carried by an animated IK goal. The knife's motion is baked onto a bone of its own under the
    right wrist (so it moves with the clips like the rest of the rig), the goals go, the rig is brought from
    centimetres to metres and put in the camera frame with the eye where the knife rests low left of the middle and
    the slashes stay right of it. Returns (armature, knife mesh, the timeline's action)."""
    reset()
    objs = load(source("knife-fp-b"))
    objs = without(objs, lambda o: o.name.startswith("Icosphere"))
    arm = armature(objs)
    knife = next(o for o in objs if o.name.startswith("knife_knife"))
    timeline = arm.animation_data.action
    sc = bpy.context.scene
    last = int(keyframe_range(timeline)[1])
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    wrist = arm.data.edit_bones["R_wrist_025"]
    bone = arm.data.edit_bones.new("Knife_Bone")
    bone.head, bone.tail, bone.parent = wrist.head, wrist.head + Vector((0, 0, 5)), wrist
    bpy.ops.object.mode_set(mode="OBJECT")
    pb = arm.pose.bones["Knife_Bone"]
    pb.rotation_mode = "QUATERNION"
    for f in range(0, last + 1):
        sc.frame_set(f)
        pb.matrix = arm.matrix_world.inverted() @ knife.matrix_world
        bpy.context.view_layer.update()
        for path in ("location", "rotation_quaternion", "scale"):
            pb.keyframe_insert(path, frame=f)
    sc.frame_set(0)
    eye, forward = Vector((-6.0, 56.0, 24.0)), Vector((0.0, -1.0, -0.12))
    turn = forward.normalized().to_track_quat("Y", "Z").to_matrix().to_4x4().inverted()
    arm.matrix_world = turn @ Matrix.Scale(0.01, 4) @ Matrix.Translation(-eye) @ arm.matrix_world
    bpy.context.view_layer.update()
    # Onto its bone only now, in the final frame (the exporter misplaces a bone child moved with its armature).
    world = knife.matrix_world.copy()
    knife.parent, knife.parent_type, knife.parent_bone = arm, "BONE", "Knife_Bone"
    bpy.context.view_layer.update()
    knife.matrix_world = world
    remove([o for o in bpy.data.objects if o.type == "EMPTY" and (o.name == "knife" or "_Goal" in o.name or "_Pole" in o.name)])
    return arm, knife, timeline


def vm_knife():
    """The knife in first person: `knife_rig`, its timeline cut into the game's clips (at 30 fps): the idle, the slash
    (from the raised knife, so the blade meets at about the sim's strike time), the reverse-grip stab and the draw."""
    arm, _, timeline = knife_rig()
    clear_nla()
    clips = {"idle": (0, 41), "slash": (45, 60), "stab": (82, 100), "draw": (121, 135)}
    for name, frames in clips.items():
        cut(arm, timeline.name, name, frames)
    drop_actions(set(clips))
    pose_at(arm, "idle", 0)
    shrink_textures(1024)
    export("vm-knife")


def reach(arm, side: str, target: Vector, pole: Vector):
    """Two-bone IK in the armature's current pose: `side`'s ("R"/"L") wrist to `target` (world), the elbow bent
    towards `pole`. The upper arm and the forearm turn about their heads (the knife rig's bone tails don't point
    along the limbs, so Blender's IK constraint would pull on the wrong points); the hand keeps its turn in the world."""
    names = {"R": ("R_arm_023", "R_elbow_024", "R_wrist_025"), "L": ("L_arm_01", "L_elbow_02", "L_wrist_03")}[side]
    upper, fore, hand = (arm.pose.bones[n] for n in names)
    mw = arm.matrix_world
    head = lambda pb: mw @ pb.head
    hand_world = (mw @ hand.matrix).to_quaternion()
    s, e, w = head(upper), head(fore), head(hand)
    a, b = (e - s).length, (w - e).length
    to = target - s
    d = min(max(to.length, abs(a - b) + 1e-4), (a + b) * 0.999)
    u = to.normalized()
    v = (pole - s) - u * (pole - s).dot(u)
    v = v.normalized() if v.length > 1e-6 else Vector((0, 0, -1))
    cos = (a * a + d * d - b * b) / (2 * a * d)
    elbow = s + a * (cos * u + math.sqrt(max(0.0, 1 - cos * cos)) * v)
    wrist = s + u * d

    def turn(pb, frm: Vector, to_: Vector):
        q = frm.rotation_difference(to_)
        h = head(pb)
        m = mw.inverted() @ Matrix.Translation(h) @ q.to_matrix().to_4x4() @ Matrix.Translation(-h) @ mw @ pb.matrix
        pb.matrix = m
        bpy.context.view_layer.update()

    turn(upper, e - s, elbow - s)
    turn(fore, head(hand) - head(fore), wrist - head(fore))
    m = hand.matrix.copy()
    world = mw @ m
    loc = world.to_translation()
    hand.matrix = mw.inverted() @ (Matrix.Translation(loc) @ hand_world.to_matrix().to_4x4() @ Matrix.Diagonal(world.to_scale().to_4d()))
    bpy.context.view_layer.update()


def smooth_path(keys, t: float) -> Vector:
    """The point at `t` along `keys` [(t, Vector)], eased in and out between keys."""
    if t <= keys[0][0]:
        return keys[0][1].copy()
    for (t0, p0), (t1, p1) in zip(keys, keys[1:]):
        if t <= t1:
            k = (t - t0) / max(t1 - t0, 1e-6)
            k = k * k * (3 - 2 * k)
            return p0.lerp(p1, k)
    return keys[-1][1].copy()


def vm_grenade():
    """The grenades in first person: the knife rig's arms without the knife, a grenade in the right fist, and clips
    made here (at 30 fps) with `reach`: the idle and the draw are the knife's; `pin`/`pin_lob` bring the right hand
    in, the left pulls the pin and drops away, the right winds up high (overhand) or low (underhand) and holds;
    `throw`/`lob` swing it forward, let go and follow through out of view. The game hides `grenade` once thrown."""
    arm, knife, timeline = knife_rig()
    remove([knife])
    sc = bpy.context.scene
    arm.animation_data.action = timeline
    sc.frame_set(0)
    bpy.context.view_layer.update()
    mw = arm.matrix_world
    pb = arm.pose.bones
    rest = {n: (mw @ pb[n].head).copy() for n in ("R_elbow_024", "R_wrist_025", "L_elbow_02", "L_wrist_03")}
    # The grenade in the right fist, riding the wrist.
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(OUT / "grenade.glb"))
    grenade = next(o for o in bpy.data.objects if o not in before and o.type == "MESH")
    remove([o for o in bpy.data.objects if o not in before and o is not grenade])
    grenade.name = "grenade"
    palm = (mw @ pb["R_wrist_025"].head).lerp(mw @ pb["R_middle1_034"].head, 0.55) + Vector((0.0, 0.0, 0.02))
    grenade.parent, grenade.parent_type, grenade.parent_bone = arm, "BONE", "R_wrist_025"
    bpy.context.view_layer.update()
    grenade.matrix_world = Matrix.Translation(palm) @ Matrix.Rotation(math.radians(-20), 4, "X")
    clear_nla()
    for name, frames in (("idle", (0, 41)), ("draw", (121, 135))):
        cut(arm, timeline.name, name, frames)
    V = Vector
    r0, l0 = rest["R_wrist_025"], rest["L_wrist_03"]
    # Wound up: the fist by the right edge of the view at eye height (overhand), or low right (underhand).
    up_r, low_r = V((0.21, 0.3, -0.05)), V((0.19, 0.34, -0.33))
    l_out = V((-0.32, 0.42, -0.34))
    pull = [(0.0, l0), (0.2, V((0.04, 0.42, -0.17))), (0.33, V((-0.1, 0.36, -0.22))), (0.5, l_out)]
    clips = {
        "pin": ({"R": [(0.0, r0), (0.2, V((0.13, 0.42, -0.19))), (0.5, up_r)], "L": pull}, 0.5),
        "pin_lob": ({"R": [(0.0, r0), (0.2, V((0.13, 0.42, -0.19))), (0.5, low_r)], "L": pull}, 0.5),
        "throw": ({"R": [(0.0, up_r), (0.12, V((0.14, 0.46, 0.04))), (0.3, V((0.04, 0.5, -0.26))), (0.5, V((0.16, 0.28, -0.48)))],
                   "L": [(0.0, l_out), (0.5, V((-0.3, 0.3, -0.48)))]}, 0.5),
        "lob": ({"R": [(0.0, low_r), (0.15, V((0.12, 0.5, -0.22))), (0.3, V((0.1, 0.52, -0.06))), (0.5, V((0.16, 0.3, -0.46)))],
                 "L": [(0.0, l_out), (0.5, V((-0.3, 0.3, -0.48)))]}, 0.5),
    }
    limbs = {"R": ("R_arm_023", "R_elbow_024", "R_wrist_025"), "L": ("L_arm_01", "L_elbow_02", "L_wrist_03")}
    for name, (paths, length) in clips.items():
        act = still(arm, timeline.name, 0, name)
        arm.animation_data.action = act
        for f in range(0, int(round(length * 30)) + 1):
            t = f / 30.0
            # From the idle pose each frame (the keys at 0 and 1 hold it), then the wrists where the paths say.
            sc.frame_set(0)
            for side, keys in paths.items():
                reach(arm, side, smooth_path(keys, t), rest[limbs[side][1].replace("_02", "_02")] if side == "L" else rest["R_elbow_024"])
            for side in paths:
                for n in limbs[side]:
                    for path in ("location", "rotation_quaternion"):
                        pb[n].keyframe_insert(path, frame=f + 2 if f else 0)
        # The idle hold keys at frame 1 would pull the second frame back: move the clip's keys onto 0.. (frame f+2 -> f).
        for fc in act.fcurves:
            if any(n in fc.data_path for side in paths for n in limbs[side]):
                pts = fc.keyframe_points
                for i in reversed(range(len(pts))):
                    if abs(pts[i].co.x - 1.0) < 1e-3:
                        pts.remove(pts[i], fast=True)
                for k in pts:
                    if k.co.x >= 2.0:
                        k.co.x -= 1.0
                        k.handle_left.x -= 1.0
                        k.handle_right.x -= 1.0
                fc.update()
        arm.animation_data.action = None
        # Stashed again: `still` stashed it two frames long, and the exporter samples the strip's range.
        for t in list(arm.animation_data.nla_tracks):
            if t.name == name:
                arm.animation_data.nla_tracks.remove(t)
        stash(arm, act)
    drop_actions({"idle", "draw", *clips})
    pose_at(arm, "idle", 0)
    shrink_textures(1024)
    export("vm-grenade")


# ---- more long guns: FPS packs on the same kind of rig (arms, weapon bones, clips on one armature) -----------------


def pack_action(arm, clip: str) -> str:
    """The imported action for a pack's animation `clip` (the importer names them `Rig|<clip>_<armature>`)."""
    for a in bpy.data.actions:
        base = a.name.split("|", 1)[-1]
        if base == f"{clip}_{arm.name}" or base == clip:
            return a.name
    sys.exit(f"no clip {clip}: {[a.name for a in bpy.data.actions]}")


def is_arms(o) -> bool:
    """The arms' skin (weighted to the forearms), as opposed to the weapon's parts. (The importer gives every skinned
    mesh a group per joint, so this looks at the weights.)"""
    groups = {g.index for g in o.vertex_groups if any(k in g.name for k in ("UpArm", "UpperArm", "Forearm"))}
    return any(e.group in groups and e.weight > 0.1 for v in o.data.vertices for e in v.groups)


def pack_weapon(objs):
    """The weapon's parts (every mesh but the arms), the longest (front to back) first: the body."""
    def extent(o):
        ys = [p.y for p in world_points([o])]
        return max(ys) - min(ys)
    return sorted([o for o in meshes(objs) if not is_arms(o)], key=lambda o: -extent(o))


def bore(pts, length_hint=None):
    """The weapon's muzzle (the middle of its front-most centimetre), its length, and the points by the front sight,
    for a weapon pointing along +Y (the camera frame)."""
    front = max(p.y for p in pts)
    back = min(p.y for p in pts)
    length = front - back
    tip = [p for p in pts if p.y > front - 0.01 * length]
    muzzle = sum(tip, Vector()) / len(tip)
    muzzle.y = front
    return muzzle, back, length


def vm_pack(name: str, src: str, clips: dict, bone: str, eye_bone: str | None = None, eye_node: str | None = None,
            length: float | None = None, shift=(0.0, 0.0, 0.0), arms_tris: int = 9000, weapon_tris: int = 10000,
            maps: int = 1024, sight=None, eject_at: float = 0.4, drop=lambda o: False, swap=None):
    """A viewmodel from an FPS pack whose arms and weapon share one armature: `clips` maps the game's clip names to
    the pack's; the eye is the pack's head-camera bone `eye_bone` (or the static camera node `eye_node`) in the
    first frame of the idle, looking along -Y; the markers ride on the weapon's body bone `bone`. `sight(pts, muzzle)`
    gives the sight line's height (default: the top of the front sight, the highest point near the muzzle);
    `eject_at` is where the ejection port is, as a fraction of the weapon's length from its back. The rig is scaled
    about the eye so the weapon is its real `length` (as vm-rifle's M4 is: the view from the eye is the same, and the
    markers are where the muzzle really is), then moved by `shift` (metres: right, forward, up) to sit in the view
    as vm-rifle does. `maps` caps the normal and metal/roughness maps' size (the colour maps get 1024). `swap(arm,
    objs)`, if given, puts another weapon in the pack's hands before anything else (and returns the objects)."""
    reset()
    objs = load(source(src))
    objs = without(objs, lambda o: o.name.startswith("Icosphere") or o.type in ("CAMERA", "LIGHT") or drop(o))
    arm = armature(objs)
    if swap:
        objs = swap(arm, objs)
    acts = {k: pack_action(arm, v) for k, v in clips.items()}
    pose_at(arm, acts["idle"], 0)
    if eye_bone:
        eye = (arm.matrix_world @ arm.pose.bones[eye_bone].matrix).to_translation()
    else:
        eye = bpy.data.objects[eye_node].matrix_world.to_translation()
    k = 1.0 / arm.matrix_world.to_scale().x
    turn = Vector((0, -1, 0)).to_track_quat("Y", "Z").to_matrix().to_4x4().inverted()
    arm.matrix_world = turn @ Matrix.Scale(k, 4) @ Matrix.Translation(-eye) @ arm.matrix_world
    bpy.context.view_layer.update()
    clear_nla()
    for out, a in acts.items():
        cut(arm, a, out)
    drop_actions(set(acts))
    pose_at(arm, "idle", 0)
    parts = pack_weapon(objs)
    have = bore(world_points(parts[:1]))[2]
    arm.matrix_world = Matrix.Translation(Vector(shift)) @ Matrix.Scale((length or have) / have, 4) @ arm.matrix_world
    bpy.context.view_layer.update()
    pts = world_points(parts[:1])
    _, back, length = bore(pts)
    # The muzzle: the front of the whole weapon (a shroud or a brake may be a part of its own).
    whole = world_points([o for o in parts if not o.name.startswith("spare-")])
    muzzle = bore(whole)[0]
    near = [p for p in whole if p.y > muzzle.y - 0.18 * length]
    top = sight(pts, muzzle) if sight else max(p.z for p in near)
    marker("muzzle", arm, bone, muzzle)
    marker("ejection", arm, bone, Vector((muzzle.x + 0.025, back + eject_at * length, muzzle.z + 0.01)))
    # Aimed: the eye on the sight line, in the eye's plane (as vm-rifle has it).
    marker("sight-eye", arm, bone, Vector((muzzle.x, 0.0, top)))
    print(f"{name}: weapon {length:.2f} m, muzzle {tuple(round(c, 3) for c in muzzle)}, sight line z {top:.3f}")
    shrink_textures(1024)
    for im in bpy.data.images:
        if im.size[0] > maps and ("normal" in im.name or "metallic" in im.name):
            im.scale(maps, max(1, im.size[1] * maps // im.size[0]))
    arms = [o for o in meshes(objs) if is_arms(o)]
    decimate(arms, arms_tris)
    decimate(parts, weapon_tris)
    finish_uv(parts)
    export(name)


def prop_pack(name: str, src: str, idle: str, bone: str, hand: str, length: float, tex: int = 512, tris: int = 12000,
              drop=lambda o: False, swap=None, grip_at=None):
    """A pack's weapon on its own: its parts as posed in the idle, turned back to the weapon bone's rest (modelled
    straight, pointing along -Y), the right hand's grip at the origin, scaled to the real `length`. The grip is where
    the `hand` bone (the wrist) is, or with `grip_at` = (bone, back, down) that far (metres) behind and below the
    bone (the trigger), for a pack whose idle holds the weapon elsewhere than by its grip."""
    reset()
    objs = load(source(src))
    objs = without(objs, lambda o: o.name.startswith("Icosphere") or o.type in ("CAMERA", "LIGHT") or drop(o))
    arm = armature(objs)
    if swap:
        objs = swap(arm, objs)
    pose_at(arm, pack_action(arm, idle), 0)
    posed = arm.matrix_world @ arm.pose.bones[bone].matrix
    rest = arm.matrix_world @ arm.data.bones[bone].matrix_local
    grip = (arm.matrix_world @ arm.pose.bones[grip_at[0] if grip_at else hand].matrix).to_translation()
    parts = [o for o in pack_weapon(objs) if not o.name.startswith("spare-")]
    loose(parts)
    remove([o for o in bpy.data.objects if o not in parts])
    back = rest @ posed.inverted()
    for o in parts:
        o.matrix_world = back @ o.matrix_world
    grip = back @ grip
    bpy.context.view_layer.update()
    pts = world_points(parts[:1])
    front = min(p.y for p in pts)
    have = max(p.y for p in pts) - front
    tip = [p for p in pts if p.y < front + 0.01 * have]
    if grip_at:
        grip += Vector((0, grip_at[1], -grip_at[2])) * have / length
    # The grip on the bore's vertical plane (the hand bone is at the wrist, off to the side).
    grip.x = sum(p.x for p in tip) / len(tip)
    place(parts, grip, Vector((0, -1, 0)), Vector((0, 0, 1)), length / have)
    got = bore(world_points(parts[:1]))[2]
    muzzle = bore(world_points(parts))[0]
    add_point("muzzle", muzzle)
    print(f"{name}: {got:.2f} m long, muzzle {tuple(round(c, 3) for c in muzzle)}")
    decimate(parts, tris)
    finish_uv(parts)
    shrink_textures(tex)
    export(name, only=list(bpy.data.objects))


def swap_in(src: str, idle: str, pack_len: float, trigger: str, body: str, bones=(), spare=None, skip=(), bore=None, measure=None,
            upright=False):
    """For a pack whose weapon is the wrong one: a `swap` (see vm_pack) that takes the pack's weapon meshes out and
    puts the static model `src` (modelled pointing along -Y, +Z up, at its real size) in its place, riding on the
    pack's bones. It is lined up in the first frame of the pack's `idle` clip: along the old weapon's barrel, as big
    against the old weapon (`pack_len` metres long in reality) as it really is, its trigger where the pack's `trigger`
    bone is, its bore on the line through the `body` bone. `bones` sends parts (by mesh name prefix) to other bones
    than `body` (a bolt to the bolt's bone, a magazine to the magazine's), `spare` (from bone, to bone, prefixes)
    copies parts from one bone to another (the magazine a reload brings in; the copies are named `spare-*`, and the
    props leave them out), and parts named with a prefix in `skip` stay out. The parts are skinned rigidly to their
    bones, bound at the rest pose, as the pack's weapon was (glTF importers disagree about meshes parented to
    bones)."""

    def swap(arm, objs):
        import numpy as np

        pose_at(arm, pack_action(arm, idle), 0)
        arms = [o for o in meshes(objs) if is_arms(o)]
        old = pack_weapon(objs)
        pts = np.array([tuple(p) for p in world_points(old[:1])])
        # The bore's direction: the long axis of the old weapon's front quarter (the barrel); up is as near +Z as
        # square to it allows (the whole weapon's axes are skewed by its magazine and grip). `bore` overrides it for
        # packs where that goes wrong: a direction, or the mesh (by name prefix) whose own long axis is the bore.
        if isinstance(bore, tuple):
            f = Vector(bore).normalized()
        else:
            ref = pts if bore is None else np.array([tuple(p) for p in world_points([o for o in old if o.data.name.startswith(bore)])])
            front = ref[:, 1].min()
            seg = ref[ref[:, 1] < front + 0.25 * np.ptp(ref[:, 1])] if bore is None else ref
            w, v = np.linalg.eigh(np.cov((seg - seg.mean(0)).T))
            f = Vector(v[:, 2]).normalized()
        f = -f if f.y > 0 else f
        u = Vector((0, 0, 1))
        if upright:
            # The pack holds its weapon canted: its up is square to the bore and to the weapon's thinnest axis (its
            # sides), on the side away from the grip (the weapon's bulk hangs below the bore).
            allp = np.array([tuple(p) for p in world_points([o for o in old if not o.name.startswith("spare-")])])
            w2, v2 = np.linalg.eigh(np.cov((allp - allp.mean(0)).T))
            side = next(Vector(v2[:, i]).normalized() for i in range(3) if abs(Vector(v2[:, i]).normalized().dot(f)) < 0.5)
            u = f.cross(side).normalized()
            # Up points from the whole weapon's middle towards the bore's part (the slide sits over the grip).
            top = Vector(tuple(ref.mean(0) - allp.mean(0))) if bore is not None and not isinstance(bore, tuple) else Vector((0, 0, 1))
            if u.dot(top) < 0:
                u = -u
        u = (u - f * u.dot(f)).normalized()
        turn = Matrix((((-f).cross(u)), -f, u)).transposed().to_4x4()  # model -Y to f, +Z to u
        # How long the old weapon is (its longest part, or the parts `measure` names), against `pack_len`.
        if measure:
            pts = np.array([tuple(p) for p in world_points([o for o in old if o.data.name.startswith(tuple(measure))])])
        along = pts @ np.array(tuple(f))
        units = (along.max() - along.min()) / pack_len  # the pack's units per metre
        W = arm.matrix_world
        posed = lambda b: W @ arm.pose.bones[b].matrix
        rest = lambda b: W @ arm.data.bones[b].matrix_local
        trig = posed(trigger).to_translation()
        right = (-f).cross(u)
        trig -= right * (trig - posed(body).to_translation()).dot(right)  # onto the bore's line
        remove(old)
        new = load(source(src))
        parts = [o for o in new if o.type == "MESH" and not o.data.name.startswith(("Cube", *skip))]
        # A prepared model (guns.py) marks its trigger and its sights with empties.
        points = {o.name.split(".")[0]: o.matrix_world.to_translation() for o in new if o.type == "EMPTY"}
        loose(parts)
        remove([o for o in new if o not in parts])
        if "trigger_point" in points:
            pivot = points["trigger_point"]
        else:
            tp = world_points([o for o in parts if o.data.name.startswith("trigger")])
            pivot = Vector((sum(p.x for p in tp) / len(tp), sum(p.y for p in tp) / len(tp), max(p.z for p in tp)))
        m = Matrix.Translation(trig) @ turn @ Matrix.Scale(units, 4) @ Matrix.Translation(-pivot)
        # The sights, riding the body (the game aims along them).
        for k in ("rear", "front"):
            if f"sight_{k}" in points:
                marker(f"sight-{k}", arm, body, m @ points[f"sight_{k}"])
        # And a point 5 cm above the rear sight, square up in the gun's own frame: the game stands the aimed gun upright
        # by it (the packs' muzzle markers can sit off the bore, which leant the pistols by up to 10°).
        if "sight_rear" in points:
            marker("sight-up", arm, body, m @ (points["sight_rear"] + Vector((0.0, 0.0, 0.05))))
        for o in parts:
            o.matrix_world = m @ o.matrix_world
        bpy.context.view_layer.update()
        bone_of = {o: next((b for pre, b in bones if o.data.name.startswith(pre)), body) for o in parts}
        copies = []
        if spare:
            frm, to, pres = spare
            for o in [o for o in parts if o.data.name.startswith(pres)]:
                c = o.copy()
                c.data = o.data.copy()
                c.name = "spare-" + o.name
                bpy.context.scene.collection.objects.link(c)
                c.matrix_world = posed(to) @ posed(frm).inverted() @ o.matrix_world
                bone_of[c] = to
                copies.append(c)
        # Into each bone's rest: where the part would be with the bone at rest, so that the idle pose puts it here.
        for o in parts + copies:
            b = bone_of[o]
            o.data.transform(rest(b) @ posed(b).inverted() @ o.matrix_world)
            o.matrix_world = Matrix.Identity(4)
            o.parent = arm
            o.matrix_parent_inverse = arm.matrix_world.inverted()
            g = o.vertex_groups.new(name=b)
            g.add(range(len(o.data.vertices)), 1.0, "REPLACE")
            mod = o.modifiers.new("skin", "ARMATURE")
            mod.object = arm
        bpy.context.view_layer.update()
        return [arm] + parts + copies + arms

    return swap


def vm_ak():
    """The AK-74M pack (ccransh; bumstrum's arms, as the pistol pack has)."""
    vm_pack("vm-ak", "ak", {"idle": "AK_Idle", "draw": "AK_Draw", "walk": "AK_Walk", "fire": "AK_Shot",
                            "reload": "AK_Reload", "reload_empty": "AK_Reload_full"}, "PBody_058", eye_node="Camera",
            length=0.943, shift=(0.06, 0.0, -0.005), eject_at=0.45)


def prop_ak():
    prop_pack("ak", "ak", "AK_Idle", "PBody_058", "Hand_R_037", 0.943, drop=lambda o: o.type == "MESH" and o.data.name.startswith("Mag_2"))


def scope_line(pts, muzzle):
    """A scoped rifle's sight line: the scope tube's axis. Its top is a typical highest point across slices of the
    rifle's middle (the scope spans about half of them; the 65th percentile is the tube, not the turrets or the
    bells, nor the receiver in front of or behind the scope), and the tube is about 30 mm across."""
    front = max(p.y for p in pts)
    back = min(p.y for p in pts)
    n = 24
    tops = []
    for i in range(n):
        lo = back + (0.25 + 0.5 * i / n) * (front - back)
        hi = lo + 0.5 * (front - back) / n
        zs = [p.z for p in pts if lo <= p.y < hi]
        if zs:
            tops.append(max(zs))
    tops.sort()
    return tops[int(len(tops) * 0.65)] - 0.015


# The sniper pack's own rifle is an improvised one (an oil filter for a suppressor, taped stock); bumstrum's "Sniper"
# (a synthetic-stock bolt-action with a scope, as a G22 or SSG 3000 looks) takes its place, the bolt on the pack's bolt
# bone (so the bolt cycle works it) and the magazine on the magazine bone.
SNIPER_INTO_PACK = swap_in("sniper-rifle", "SRifle_Idle", 1.1, "Trigger_058", "Body_057",
                           bones=[(("bolt", "boltrear"), "Bolt_059"), (("mag_", "bullet"), "Mag_060")],
                           skip=("shell",))


def vm_sniper():
    """The sniper rifle pack's arms and clips (ccransh; bumstrum's arms) with bumstrum's rifle (SNIPER_INTO_PACK). It
    has no head-camera bone; its camera node is the eye. No draw clip; `fire` is the hip shot with its bolt cycle."""
    vm_pack("vm-sniper", "sniper", {"idle": "SRifle_Idle", "walk": "SRifle_Walk", "fire": "SRifle_Shot_nosight",
                                    "reload": "SRifle_Reload", "reload_empty": "SRifle_Reload_Full"}, "Body_057",
            eye_node="Camera", length=1.02, shift=(0.03, 0.0, -0.04), sight=scope_line, eject_at=0.42,
            swap=SNIPER_INTO_PACK)


def prop_sniper():
    # (The pack's idle holds the rifle with the right hand up by the stock's comb.)
    prop_pack("sniper", "sniper", "SRifle_Idle", "Body_057", "Hand_R_037", 1.02, swap=SNIPER_INTO_PACK,
              grip_at=("Trigger_058", 0.18, 0.03))


def vm_shotgun():
    """The Remington 870 pack (ccransh; bumstrum's arms): pump action, no draw clip, one reload clip that loads
    shell after shell."""
    vm_pack("vm-shotgun", "shotgun", {"idle": "SG_FPS_Idle", "walk": "SG_FPS_Walk", "fire": "SG_FPS_Shot",
                                      "reload": "SG_FPS_Reload"}, "Body_058", eye_bone="Head_Cam_014", length=1.0,
            shift=(0.05, -0.05, -0.06), eject_at=0.4)


def prop_shotgun():
    prop_pack("shotgun", "shotgun", "SG_FPS_Idle", "Body_058", "Hand_R_038", 1.0,
              drop=lambda o: o.type == "MESH" and o.data.name.startswith("12ge"))


def vm_smg():
    """The SMG pack (ccransh; Masterpiece Arms 30 SST by eNse7en, bumstrum's arms)."""
    vm_pack("vm-smg", "smg", {"idle": "KDW_DPose_Idle", "draw": "KDW_Draw", "walk": "KDW_Walk", "fire": "KDW_Shot",
                                  "reload": "KDW_Reload_fast", "reload_empty": "KDW_Reload_full"}, "PBody_058",
            eye_node="Camera", shift=(0.03, 0.06, -0.015), maps=512, eject_at=0.5)


def prop_smg():
    # Without the spare magazine (the reload's).
    prop_pack("smg", "smg", "KDW_DPose_Idle", "PBody_058", "Hand_R_00", 0.5, tris=8000,
              drop=lambda o: o.type == "MESH" and o.data.name.endswith("0.001_Material_457_0"))


# bumstrum's LMG (a Minimi) in the AK pack's hands, for want of an animated machine-gun pack. The box and its belt
# ride on the AK's magazine bone (a copy on the spare magazine's bone), so the AK's reload swaps boxes.
MINIMI_INTO_AK = swap_in("mg", "AK_Idle", 0.943, "Trigger_060", "PBody_058",
                       bones=[(("mag", "bulletholder", "bullet"), "Pmag_062")],
                       spare=("Pmag_062", "Pmag2_063", ("mag", "bulletholder", "bullet")))


def vm_mg():
    """The LMG in the AK pack's hands, with the AK's clips (see MINIMI_INTO_AK)."""
    vm_pack("vm-mg", "ak", {"idle": "AK_Idle", "draw": "AK_Draw", "walk": "AK_Walk", "fire": "AK_Shot",
                            "reload": "AK_Reload_full"}, "PBody_058", eye_node="Camera", length=1.04,
            shift=(0.06, 0.0, 0.0), eject_at=0.45, swap=MINIMI_INTO_AK)


def prop_mg():
    prop_pack("mg", "ak", "AK_Idle", "PBody_058", "Hand_R_037", 1.04, swap=MINIMI_INTO_AK)


# ---- soldiers: the animation library's clips on the models' own skeletons -----------------------------------------

# The Universal Animation Library's bones (Unreal's mannequin names) and each soldier rig's name for them, as a
# pattern matched against the imported bone names (they carry numeric suffixes from the export).
UAL_BONES = [
    "pelvis", "spine_01", "spine_02", "spine_03", "neck_01", "Head",
    "clavicle_l", "upperarm_l", "lowerarm_l", "hand_l", "clavicle_r", "upperarm_r", "lowerarm_r", "hand_r",
    "thigh_l", "calf_l", "foot_l", "ball_l", "thigh_r", "calf_r", "foot_r", "ball_r",
] + [f"{f}_{i:02d}_{s}" for s in "lr" for f in ("thumb", "index", "middle", "ring", "pinky") for i in (1, 2, 3)]

MIXAMO = {
    "pelvis": "Hips", "spine_01": "Spine", "spine_02": "Spine1", "spine_03": "Spine2", "neck_01": "Neck", "Head": "Head",
    **{f"clavicle_{s}": f"{S}Shoulder" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"upperarm_{s}": f"{S}Arm" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"lowerarm_{s}": f"{S}ForeArm" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"hand_{s}": f"{S}Hand" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"thigh_{s}": f"{S}UpLeg" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"calf_{s}": f"{S}Leg" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"foot_{s}": f"{S}Foot" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"ball_{s}": f"{S}ToeBase" for s, S in (("l", "Left"), ("r", "Right"))},
    **{f"{f}_{i:02d}_{s}": f"{S}Hand{F}{i}" for s, S in (("l", "Left"), ("r", "Right")) for f, F in (("thumb", "Thumb"), ("index", "Index"), ("middle", "Middle"), ("ring", "Ring"), ("pinky", "Pinky")) for i in (1, 2, 3)},
}

CC = {
    "pelvis": "CC_Base_Hip", "spine_01": "CC_Base_Waist", "spine_02": "CC_Base_Spine01", "spine_03": "CC_Base_Spine02",
    "neck_01": "CC_Base_NeckTwist01", "Head": "CC_Base_Head",
    **{f"clavicle_{s}": f"CC_Base_{S}_Clavicle" for s, S in (("l", "L"), ("r", "R"))},
    **{f"upperarm_{s}": f"CC_Base_{S}_Upperarm" for s, S in (("l", "L"), ("r", "R"))},
    **{f"lowerarm_{s}": f"CC_Base_{S}_Forearm" for s, S in (("l", "L"), ("r", "R"))},
    **{f"hand_{s}": f"CC_Base_{S}_Hand" for s, S in (("l", "L"), ("r", "R"))},
    **{f"thigh_{s}": f"CC_Base_{S}_Thigh" for s, S in (("l", "L"), ("r", "R"))},
    **{f"calf_{s}": f"CC_Base_{S}_Calf" for s, S in (("l", "L"), ("r", "R"))},
    **{f"foot_{s}": f"CC_Base_{S}_Foot" for s, S in (("l", "L"), ("r", "R"))},
    **{f"ball_{s}": f"CC_Base_{S}_ToeBase" for s, S in (("l", "L"), ("r", "R"))},
    **{f"{f}_{i:02d}_{s}": f"CC_Base_{S}_{F}{i}" for s, S in (("l", "L"), ("r", "R")) for f, F in (("thumb", "Thumb"), ("index", "Index"), ("middle", "Mid"), ("ring", "Ring"), ("pinky", "Pinky")) for i in (1, 2, 3)},
}


def bone_named(arm, name: str):
    """The bone whose imported name is `name` plus the exporter's suffix (`_12`), or a namespace (`ns:name_12`)."""
    import re

    pat = re.compile(rf"^(?:[^:]*:)?{re.escape(name)}(?:_\d+)?$")
    return next((b.name for b in arm.data.bones if pat.match(b.name)), None)


def retarget(src, dst, mapping: dict, action, out_name: str, fps_step: float = 1.0, limb_dirs: bool = False):
    """Bakes `action` (on the library's rig `src`) onto `dst` as `out_name`: each mapped bone turns in the world as
    the library's does from its rest, after its rest has been turned to point the way the library's rest bone points
    (so an A-pose and a T-pose agree); the hips move as the library's, scaled to the legs' length. Bones without a
    counterpart keep their rest. `limb_dirs` measures where a bone points toward its children's heads (the mapped
    ones, else any) rather than along it: for rigs whose bones' tails don't follow the limbs (as the importer makes
    them for the hostage's rig)."""
    pairs = []
    for s_name, d_name in mapping.items():
        sb = bone_named(src, s_name)
        db = bone_named(dst, d_name)
        if sb and db:
            pairs.append((sb, db))
    by_dst = {d: s for s, d in pairs}
    Ws, Wd = src.matrix_world, dst.matrix_world
    rot = lambda m: m.to_3x3().normalized().to_quaternion()
    # Rest rotations in the world, and the turn that points each target rest bone along its source.
    rest_s = {s: rot(Ws @ src.data.bones[s].matrix_local) for s, _ in pairs}
    rest_d = {d: rot(Wd @ dst.data.bones[d].matrix_local) for _, d in pairs}
    mapped = {s for s, _ in pairs} | {d for _, d in pairs}

    def dirs(arm, W, b):
        bone = arm.data.bones[b]
        kids = [c for c in bone.children if c.name in mapped] or list(bone.children)
        if limb_dirs and kids:
            to = sum((c.head_local for c in kids), Vector()) / len(kids)
            return (W.to_3x3() @ (to - bone.head_local)).normalized()
        return (W.to_3x3() @ (bone.tail_local - bone.head_local)).normalized()
    # Only the arms differ between an A-pose and a T-pose; elsewhere the rigs' bones point by different conventions
    # (a hip bone up or forward), and both rests stand straight anyway.
    arm_bone = lambda s: any(k in s for k in ("clavicle", "upperarm", "lowerarm", "hand", "thumb", "index", "middle", "ring", "pinky"))
    align = {d: dirs(dst, Wd, d).rotation_difference(dirs(src, Ws, s)) if arm_bone(s) else Quaternion() for s, d in pairs}
    hips_s, hips_d = bone_named(src, "pelvis"), bone_named(dst, mapping["pelvis"])
    leg = lambda arm, W, b: (W @ arm.data.bones[b].head_local).z
    scale = leg(dst, Wd, hips_d) / max(1e-6, leg(src, Ws, hips_s))
    hip_rest_s = (Ws @ src.data.bones[hips_s].matrix_local).to_translation()
    hip_rest_d = (Wd @ dst.data.bones[hips_d].matrix_local).to_translation()

    src.animation_data_create()
    src.animation_data.action = action
    lo, hi = keyframe_range(action)
    out = bpy.data.actions.new(out_name)
    out.use_fake_user = True
    dst.animation_data_create()
    dst.animation_data.action = out
    order = [b for b in dst.data.bones]  # parents before children
    f = lo
    while f <= hi + 1e-3:
        bpy.context.scene.frame_set(int(f), subframe=f % 1)
        world = {}
        for b in order:
            parent_world = world.get(b.parent.name) if b.parent else Wd
            rel = (b.parent.matrix_local.inverted() @ b.matrix_local) if b.parent else b.matrix_local
            rest_world = parent_world @ rel
            s = by_dst.get(b.name)
            if s is None:
                world[b.name] = rest_world
                continue
            ms = Ws @ src.pose.bones[s].matrix
            delta = rot(ms) @ rest_s[s].inverted()
            q = delta @ align[b.name] @ rest_d[b.name]
            loc = rest_world.to_translation()
            if b.name == hips_d:
                loc = hip_rest_d + (ms.to_translation() - hip_rest_s) * scale
            sc = rest_world.to_scale()
            want = Matrix.LocRotScale(loc, q, sc)
            world[b.name] = want
            basis = rest_world.inverted() @ want
            pb = dst.pose.bones[b.name]
            pb.rotation_mode = "QUATERNION"
            pb.rotation_quaternion = basis.to_quaternion()
            pb.keyframe_insert("rotation_quaternion", frame=f - lo)
            if b.name == hips_d:
                pb.location = basis.to_translation()
                pb.keyframe_insert("location", frame=f - lo)
        f += fps_step
    dst.animation_data.action = None
    stash(dst, out)
    return out


def hold_rifle(dst, mapping: dict, clips: list):
    """Both hands onto a rifle held at the right shoulder, baked into `clips`: two-bone IK on each arm toward points
    on the chest (so they follow the torso through each clip). A `weapon-socket` marker on the chest, at the right
    hand's grip, facing forward, is where the game hangs what is in the hands."""
    bone = lambda k: bone_named(dst, mapping[k])
    W = dst.matrix_world
    head = lambda k: W @ dst.data.bones[bone(k)].head_local
    # The characters face -Y in Blender (+Z in glTF), up is +Z, their right is -X.
    fwd, up, right = Vector((0, -1, 0)), Vector((0, 0, 1)), Vector((-1, 0, 0))
    shoulder_r, shoulder_l = head("upperarm_r"), head("upperarm_l")
    width = (shoulder_r - shoulder_l).length
    grip_r = shoulder_r + fwd * 0.2 - up * 0.17 - right * width * 0.1
    grip_l = shoulder_r + fwd * 0.52 - up * 0.1 - right * width * 0.45
    chest = bone("spine_03")
    targets = {}
    for side, at in (("r", grip_r), ("l", grip_l)):
        e = marker(f"ik-{side}", dst, chest, at)
        targets[side] = e
    # The knife and the grenade are in the right hand itself: `hand-socket`, pointing along the hand.
    hand = dst.data.bones[bone("hand_r")]
    along = (W.to_3x3() @ (hand.tail_local - hand.head_local)).normalized()
    hs = marker("hand-socket", dst, hand.name, W @ hand.tail_local)
    hs.matrix_world = Matrix.Translation(W @ hand.tail_local) @ Vector((0, 1, 0)).rotation_difference(along).to_matrix().to_4x4()
    socket = marker("weapon-socket", dst, chest, grip_r)
    # Facing forward, up: the props are modelled grip at the origin, muzzle along +Y (Blender), up +Z.
    socket.matrix_world = Matrix.Translation(grip_r) @ Matrix.Rotation(math.pi, 4, "Z") @ Matrix.Identity(4)
    for side in "rl":
        fore = dst.pose.bones[bone(f"lowerarm_{side}")]
        ik = fore.constraints.new("IK")
        ik.target = targets[side]
        ik.chain_count = 2
    bpy.context.view_layer.objects.active = dst
    bpy.ops.object.mode_set(mode="POSE")
    bpy.ops.pose.select_all(action="SELECT")
    for name in clips:
        act = bpy.data.actions[name]
        dst.animation_data.action = act
        lo, hi = keyframe_range(act)
        bpy.ops.nla.bake(frame_start=int(lo), frame_end=int(math.ceil(hi)), only_selected=False, visual_keying=True, clear_constraints=False, use_current_action=True, bake_types={"POSE"})
    bpy.ops.object.mode_set(mode="OBJECT")
    for side in "rl":
        fore = dst.pose.bones[bone(f"lowerarm_{side}")]
        for c in list(fore.constraints):
            fore.constraints.remove(c)
    remove(targets.values())
    dst.animation_data.action = None


# The library clips each soldier gets, by the name the game plays them under.
SOLDIER_CLIPS = {
    "idle": "Idle_Loop",
    "walk": "Walk_Loop",
    "run": "Jog_Fwd_Loop",
    "sprint": "Sprint_Loop",
    "crouch_idle": "Crouch_Idle_Loop",
    "crouch_walk": "Crouch_Fwd_Loop",
    "jump": "Jump_Loop",
    "death": "Death01",
    "hit": "Hit_Chest",
    "knife": "Punch_Jab",
    "throw": "Spell_Simple_Shoot",
}


def ual_path() -> Path:
    return next((CACHE / "anims").rglob("UAL1_Standard.glb"))


# ---- one mesh, one material: the characters' parts joined and their maps baked into an atlas ---------------------
#
# Bevy skins every glTF primitive (a mesh's part in one material) as an entity of its own: it works out all the joint
# matrices for each, and draws each on its own. A soldier in 23 parts cost 23 times the joints and the draws; joined,
# one. Each source map's own UV layout (so no new seams) gets a tile of one atlas, and Cycles bakes each part's maps
# from its own material into it: the base colour and the metallic/roughness/occlusion through the emission (exact, whatever the source's
# nodes, factors or UV maps are), the normal map as a tangent-space normal bake in the atlas's UVs.


def _socket_source(sock):
    """What feeds a shader socket: the linked output socket, or None (then its own value counts)."""
    return sock.links[0].from_socket if sock.is_linked else None


def _principled(mat):
    return next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)


def _output(mat):
    outs = [n for n in mat.node_tree.nodes if n.type == "OUTPUT_MATERIAL"]
    return next((n for n in outs if n.is_active_output), outs[0])


def _occlusion(mat):
    """The glTF occlusion input of a material (the importer's `glTF Material Output` group), if it has one."""
    for n in mat.node_tree.nodes:
        if n.type == "GROUP" and n.node_tree and n.node_tree.name.startswith("glTF") and "Occlusion" in n.inputs:
            return n.inputs["Occlusion"]
    return None


def _emit(mat, channels):
    """Rewires `mat` to emit what `channels` gives, a list of (socket or None, constant) per R, G, B (a single entry
    with a colour socket: that colour), for an EMIT bake of it."""
    nt = mat.node_tree
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Strength"].default_value = 1.0
    if len(channels) == 1:
        src, value = channels[0]
        if src is not None:
            nt.links.new(src, em.inputs["Color"])
        else:
            em.inputs["Color"].default_value = value
    else:
        comb = nt.nodes.new("ShaderNodeCombineColor")
        for i, (src, value) in enumerate(channels):
            if src is not None:
                nt.links.new(src, comb.inputs[i])
            else:
                comb.inputs[i].default_value = value
        nt.links.new(comb.outputs["Color"], em.inputs["Color"])
    nt.links.new(em.outputs["Emission"], _output(mat).inputs["Surface"])


def _skyline(sides, size):
    """Places squares of `sides` px (biggest first) in a `size` px square, each as low and then as far left as it
    goes on the skyline the placed ones make; their lower left corners in the given order, or None if they don't fit."""
    order = sorted(range(len(sides)), key=lambda i: -sides[i])
    sky = [(0, size, 0)]  # (x, width, height) segments, left to right
    at = [None] * len(sides)
    for i in order:
        s = sides[i]
        best = None
        for j, (x, _, _) in enumerate(sky):
            if x + s > size:
                break
            # The height the square would rest at over the segments it spans.
            h, k, reach = 0, j, x
            while reach < x + s:
                h = max(h, sky[k][2])
                reach += sky[k][1]
                k += 1
            if h + s <= size and (best is None or (h, x) < (best[0], best[1])):
                best = (h, x)
        if best is None:
            return None
        h, x = best
        at[i] = (x, h)
        # The new segment, and what's left of the ones it covers.
        new = []
        for (sx, sw, sh) in sky:
            if sx + sw <= x or sx >= x + s:
                new.append((sx, sw, sh))
                continue
            if sx < x:
                new.append((sx, x - sx, sh))
            if sx + sw > x + s:
                new.append((x + s, sx + sw - x - s, sh))
        new.append((x, s, h + s))
        new.sort()
        merged = []
        for seg in new:
            if merged and merged[-1][2] == seg[2]:
                merged[-1] = (merged[-1][0], merged[-1][1] + seg[1], seg[2])
            else:
                merged.append(seg)
        sky = merged
    return at


def _areas(obj, layer):
    """Per material index: the faces' area in the world and in the UV `layer`."""
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.transform(obj.matrix_world)
    uv = bm.loops.layers.uv[layer.name]
    world, flat = {}, {}
    for f in bm.faces:
        world[f.material_index] = world.get(f.material_index, 0) + f.calc_area()
        pts = [l[uv].uv for l in f.loops]
        a = 0.5 * abs(sum(pts[i].x * pts[i - 1].y - pts[i - 1].x * pts[i].y for i in range(len(pts))))
        flat[f.material_index] = flat.get(f.material_index, 0) + a
    bm.free()
    return world, flat


def _bake_into(obj, mats, image, kind: str):
    """Bakes `kind` (EMIT or NORMAL) of `obj` into `image`, through its active UV map (the atlas)."""
    for m in mats:
        nt = m.node_tree
        for n in nt.nodes:
            n.select = False
        t = nt.nodes.get("atlas-target") or nt.nodes.new("ShaderNodeTexImage")
        t.name = "atlas-target"
        t.image = image
        t.select = True
        nt.nodes.active = t
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    if kind == "NORMAL":
        bpy.ops.object.bake(type="NORMAL", normal_space="TANGENT")
    else:
        bpy.ops.object.bake(type="EMIT")
    image.pack()


def atlas(objs, name: str, size: int, normal_size: int | None = None, orm_size: int | None = None,
          favour: dict | None = None, gap_px: int = 8, ref_px: int = 1024):
    """Joins `objs` (skinned to the same armature) into one mesh in one material `name`, whose maps are baked from
    the parts' own materials into atlases: base colour `size` px, normal `normal_size` px (if any part has a normal
    map), metallic/roughness/occlusion `orm_size` px (if any part's vary; otherwise constants). `favour` scales the
    tile of a material (matched by a substring of its name), `gap_px` is the space between tiles, `ref_px` the map
    size the sources count as (bigger maps count as that big; smaller ones get smaller tiles). Returns the joined
    object."""
    scene = bpy.context.scene
    arm = objs[0].find_armature()
    # Sources a good deal bigger than their share of the atlas only slow the bake down.
    shrink_textures(2 * size)
    # Join (the biggest keeps its name and modifiers).
    body = max(objs, key=lambda o: len(o.data.polygons))
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
    me = body.data
    mats = [s.material for s in body.material_slots if s.material]
    # The parts' maps read their own UV maps (the first unless a node says otherwise), the atlas is a new one.
    src_uv = me.uv_layers[0].name
    for m in mats:
        for n in m.node_tree.nodes:
            if n.type == "TEX_IMAGE" and not n.inputs["Vector"].is_linked:
                uvn = m.node_tree.nodes.new("ShaderNodeUVMap")
                uvn.uv_map = src_uv
                m.node_tree.links.new(uvn.outputs["UV"], n.inputs["Vector"])
    me.uv_layers.active = me.uv_layers[0]
    lay = me.uv_layers.new(name="atlas", do_init=True)
    me.uv_layers.active = lay
    # Each source map gets a square tile of the atlas and keeps its own layout in it (its authors packed it; the
    # parts' islands, repacked, fill no more than 40% of a square: they are concave, many and small), cropped to
    # what the parts use of it. Materials that share their maps share a tile. The tiles are as big as the maps (up
    # to `ref_px`) times one scale, the biggest at which they all fit; no map denser on the body than four times the
    # median (by area): a whole map for an eyeball would take as much as the jacket's.
    src = me.uv_layers[src_uv]
    world, uv0 = _areas(body, src)
    groups = {}
    for i, sl in enumerate(body.material_slots):
        ims = frozenset(n.image.name for n in sl.material.node_tree.nodes if n.type == "TEX_IMAGE" and n.image)
        groups.setdefault(ims or i, []).append(i)
    tiles = []
    for key, idx in groups.items():
        faces = [p for p in me.polygons if p.material_index in idx]
        if not faces:
            continue
        uvs = [src.data[li].uv for p in faces for li in p.loop_indices]
        lo = Vector((min(u.x for u in uvs), min(u.y for u in uvs)))
        side = max(max(u.x for u in uvs) - lo.x, max(u.y for u in uvs) - lo.y, 1e-3)
        mats_g = [body.material_slots[i].material for i in idx]
        res = max((n.image.size[0] for m in mats_g for n in m.node_tree.nodes if n.type == "TEX_IMAGE" and n.image), default=0)
        area_w = sum(world.get(i, 0) for i in idx)
        area_uv = sum(uv0.get(i, 0) for i in idx)
        tiles.append({"idx": idx, "faces": faces, "lo": lo, "side": side, "world": area_w, "names": [m.name for m in mats_g],
                      # A map's pixels across the part's UVs, and per metre of the body.
                      "need": (min(res, ref_px) if res else 16 / side) * side,
                      "density": (min(res, ref_px) if res else 16) * math.sqrt(area_uv / max(area_w, 1e-9))})
    textured = sorted((t for t in tiles if t["density"] > 0), key=lambda t: t["density"])
    half, acc = sum(t["world"] for t in textured) / 2, 0.0
    median = next(t["density"] for t in textured if (acc := acc + t["world"]) >= half)
    for t in tiles:
        t["need"] *= min(1.0, 4 * median / t["density"]) if t["density"] > 0 else 1.0
        t["need"] *= next((f for key, f in (favour or {}).items() if any(key in n for n in t["names"])), 1.0)
    lo_s, hi_s = 0.0, 8.0
    for _ in range(40):
        mid = (lo_s + hi_s) / 2
        if _skyline([max(4, int(t["need"] * mid)) + gap_px for t in tiles], size + gap_px) is None:
            hi_s = mid
        else:
            lo_s = mid
    sizes = [max(4, int(t["need"] * lo_s)) for t in tiles]
    spots = _skyline([sz + gap_px for sz in sizes], size + gap_px)
    lay = me.uv_layers["atlas"]
    print(f"{name}: atlas tiles (px, texel density against the sources' at {ref_px} px):")
    for t, sz, (x, y) in zip(tiles, sizes, spots):
        for p in t["faces"]:
            for li in p.loop_indices:
                lay.data[li].uv = (Vector((x, y)) + (src.data[li].uv - t["lo"]) / t["side"] * sz) / size
        print(f"  {'+'.join(t['names']):28s} {sz:5d} px at ({x}, {y})  {sz / (t['side'] * ref_px):.2f}")
    # Bake at the rest pose.
    # (Props have no armature: then there is no pose to rest.)
    pose = arm.data.pose_position if arm else None
    if arm:
        arm.data.pose_position = "REST"
    bpy.context.view_layer.update()
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 8
    scene.cycles.use_denoising = False
    bake = scene.render.bake
    bake.target = "IMAGE_TEXTURES"
    bake.use_clear = True
    bake.margin = 2 * gap_px
    bake.margin_type = "EXTEND"
    bake.use_selected_to_active = False

    def new_image(suffix, px, data):
        im = bpy.data.images.new(f"{name}_{suffix}", px, px, alpha=False)
        im.colorspace_settings.name = "Non-Color" if data else "sRGB"
        return im

    principled = {m: _principled(m) for m in mats}
    has_normal = any(p and p.inputs["Normal"].is_linked for p in principled.values())
    normal = None
    if has_normal and normal_size:
        normal = new_image("normal", normal_size, True)
        _bake_into(body, mats, normal, "NORMAL")
    # Metallic, roughness, occlusion: baked if any part's are mapped or they differ between parts.
    consts = set()
    varies = False
    for m, p in principled.items():
        occ = _occlusion(m)
        if p is None:
            continue
        varies |= p.inputs["Roughness"].is_linked or p.inputs["Metallic"].is_linked or bool(occ and occ.is_linked)
        consts.add((round(p.inputs["Roughness"].default_value, 3), round(p.inputs["Metallic"].default_value, 3)))
    varies |= len(consts) > 1
    orm = None
    if varies and orm_size:
        orm = new_image("orm", orm_size, True)
        for m, p in principled.items():
            occ = _occlusion(m)
            _emit(m, [(_socket_source(occ) if occ else None, occ.default_value if occ and not occ.is_linked else 1.0),
                      (_socket_source(p.inputs["Roughness"]), p.inputs["Roughness"].default_value),
                      (_socket_source(p.inputs["Metallic"]), p.inputs["Metallic"].default_value)])
        _bake_into(body, mats, orm, "EMIT")
    base = new_image("baseColor", size, False)
    for m, p in principled.items():
        _emit(m, [(_socket_source(p.inputs["Base Color"]), tuple(p.inputs["Base Color"].default_value))])
    _bake_into(body, mats, base, "EMIT")
    if arm:
        arm.data.pose_position = pose
    # The one material, reading the atlases through the only UV map left.
    rough, metal = next(iter(consts)) if consts else (0.8, 0.0)
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    p = _principled(mat)
    tex = lambda im: (lambda n: (setattr(n, "image", im), n)[1])(nt.nodes.new("ShaderNodeTexImage"))
    nt.links.new(tex(base).outputs["Color"], p.inputs["Base Color"])
    if normal:
        nm = nt.nodes.new("ShaderNodeNormalMap")
        nt.links.new(tex(normal).outputs["Color"], nm.inputs["Color"])
        nt.links.new(nm.outputs["Normal"], p.inputs["Normal"])
    if orm:
        sep = nt.nodes.new("ShaderNodeSeparateColor")
        nt.links.new(tex(orm).outputs["Color"], sep.inputs["Color"])
        nt.links.new(sep.outputs["Green"], p.inputs["Roughness"])
        nt.links.new(sep.outputs["Blue"], p.inputs["Metallic"])
        from io_scene_gltf2.blender.com.gltf2_blender_material_helpers import create_settings_group, get_gltf_node_name
        group = bpy.data.node_groups.get(get_gltf_node_name()) or create_settings_group(get_gltf_node_name())
        g = nt.nodes.new("ShaderNodeGroup")
        g.node_tree = group
        nt.links.new(sep.outputs["Red"], g.inputs["Occlusion"])
    else:
        p.inputs["Roughness"].default_value = rough
        p.inputs["Metallic"].default_value = metal
    me.materials.clear()
    me.materials.append(mat)
    for poly in me.polygons:
        poly.material_index = 0
    for n in [l.name for l in me.uv_layers if l.name != "atlas"]:
        me.uv_layers.remove(me.uv_layers[n])
    me.uv_layers["atlas"].name = "UVMap"
    me.uv_layers[0].active = True
    me.uv_layers[0].active_render = True
    for m in mats:
        if m.users == 0:
            bpy.data.materials.remove(m)
    for im in list(bpy.data.images):
        if im.users == 0:
            bpy.data.images.remove(im)
    tris = sum(len(p.vertices) - 2 for p in me.polygons)
    print(f"{name}: joined into one mesh, {tris} tris; atlas {size} px"
          + (f", normal {normal_size} px" if normal else "") + (f", ORM {orm_size} px" if orm else ", constant roughness/metal"))
    return body


def soldier(name: str, src_name: str, mapping: dict, clips: dict, tris: int, tex: int, hold: bool = True,
            bind_pose: bool = False, limb_dirs: bool = False, atlas_px: int | None = None, favour: dict | None = None):
    """A character with the library's `clips` retargeted onto it (see retarget), `hold` a rifle in its hands (baked
    into the clips, with the sockets; see hold_rifle), decimated to `tris`, textures to `tex` px; with `atlas_px`,
    joined into one mesh in one material whose maps are baked into an atlas that big (see atlas; normal map the
    same size, metallic/roughness/occlusion half)."""
    reset()
    lib = load(ual_path())
    src = armature(lib)
    remove([o for o in lib if o.type == "MESH"])
    objs = load(source(src_name), bind_pose)
    dst = armature(objs)
    clear_nla()
    if bind_pose:
        # Standing at the rest: no pose left over from the source's own animation.
        for pb in dst.pose.bones:
            pb.rotation_mode = "QUATERNION"
            pb.location, pb.rotation_quaternion, pb.scale = (0, 0, 0), (1, 0, 0, 0), (1, 1, 1)
        bpy.context.view_layer.update()
    # 1.8 m tall, standing on the origin.
    pts = world_points([o for o in objs if o.type == "MESH" and o.parent])
    height = max(p.z for p in pts) - min(p.z for p in pts)
    dst.matrix_world = Matrix.Scale(1.8 / height, 4) @ dst.matrix_world
    bpy.context.view_layer.update()
    print(f"{name}: {height:.2f} m tall as modelled")
    lib_action = lambda n: next(a for a in bpy.data.actions if a.name == n or a.name.startswith(n + "_") and src.name in a.name)
    made = set()
    for out, clip in clips.items():
        retarget(src, dst, mapping, lib_action(clip), out, limb_dirs=limb_dirs)
        made.add(out)
    remove([o for o in bpy.data.objects if o in lib or o.name.startswith("Icosphere")])
    drop_actions(made)
    if hold:
        hold_rifle(dst, mapping, [c for c in made if c not in ("death", "knife", "throw")])
    body = [o for o in meshes() if o.parent]
    decimate(body, tris)
    if atlas_px:
        atlas(body, name, atlas_px, normal_size=atlas_px, orm_size=atlas_px // 2, favour=favour)
    else:
        shrink_textures(tex)
    kept, dropped = drop_rest_channels(dst, [bpy.data.actions[c] for c in made])
    print(f"{name}: {kept} curves animate, {dropped} at rest throughout dropped")
    export(name, bone_channels=False)


# (Joined and atlased for the browser's sake: the parts were 23 skinned meshes in 10 materials for blue, 7 for red.)
def soldier_blue(clips=SOLDIER_CLIPS):
    soldier("soldier-blue", "soldier-blue", CC, clips, 14500, 1024, atlas_px=2048)


def soldier_red(clips=SOLDIER_CLIPS):
    soldier("soldier-red", "soldier-red", MIXAMO, clips, 12000, 1024, atlas_px=1024)


# Renderpeople's rig (Nathan): the library's bones by their names there, after the rig's prefix.
RP_PREFIX = "rp_nathan_animated_003_walking_"
RP = {k: RP_PREFIX + v for k, v in {
    "pelvis": "hip", "spine_01": "spine_01", "spine_02": "spine_02", "spine_03": "spine_03", "neck_01": "neck",
    "Head": "head",
    **{f"clavicle_{s}": f"shoulder_{s}" for s in "lr"},
    **{f"upperarm_{s}": f"upperarm_{s}" for s in "lr"},
    **{f"lowerarm_{s}": f"lowerarm_{s}" for s in "lr"},
    **{f"hand_{s}": f"hand_{s}" for s in "lr"},
    **{f"thigh_{s}": f"upperleg_{s}" for s in "lr"},
    **{f"calf_{s}": f"lowerleg_{s}" for s in "lr"},
    **{f"foot_{s}": f"foot_{s}" for s in "lr"},
    **{f"ball_{s}": f"ball_{s}" for s in "lr"},
    **{f"{f}_{i:02d}_{s}": f"{f}_{i:02d}_{s}" for s in "lr" for f in ("thumb", "index", "middle", "ring", "pinky") for i in (1, 2, 3)},
}.items()}

# A hostage moves, crouches, flinches and dies; it neither stabs nor throws.
HOSTAGE_CLIPS = {k: v for k, v in SOLDIER_CLIPS.items() if k not in ("knife", "throw")}


def hostage(clips=HOSTAGE_CLIPS):
    """A civilian (Renderpeople's Nathan: T-shirt and jeans) with the soldiers' library clips, empty-handed (no rifle
    hold, no sockets); scaled to the soldiers' 1.8 m."""
    soldier("hostage", "hostage", RP, clips, 12000, 1024, hold=False, bind_pose=True, limb_dirs=True)


# ---- environment props ------------------------------------------------------------------------------------------
#
# The maps' props (`props/<id>.glb`), from Poly Haven's CC0 models (fetch.py's "polyhaven" sources). Each is one piece
# of a source (`keep` picks its parts by name), in metres, Y up, its front along +Z (Blender's -Y: Poly Haven's models
# face that way), the middle of its base at the origin (the bottom of its bounding box at y = 0). The parts are joined,
# welded and decimated to a budget (3000 triangles; 6000 for plants), their maps shrunk (1024 px, 512 for small
# things) and written as JPEG, q86 (PNG for a colour map with alpha: leaves, a grille). Parts in several materials get
# one, baked into an atlas (see atlas) unless one of them is alpha-masked. A second UV map (TEXCOORD_1) is a
# non-overlapping unwrap into 0..1 for the level lightmap: the game bakes every placed prop into its map's atlas
# through it. props.json next to them has what the game and the baker need: triangles, bytes, the size, the surface
# area and how much of the lightmap square its islands fill (for the texel budget), a collision box, the mean albedo,
# which maps it suits and how it mounts.
#
#     ~/.local/bin/uv run --python 3.11 --with "bpy==4.2.*" python tools/models/build.py props
#     ~/.local/bin/uv run ... build.py barrel_01 street_lamp_01     # just these

PROPS_OUT = OUT / "props"
HAFEN, ALTSTADT, FLIEGERHORST, ALL = "hafen", "altstadt", "fliegerhorst", "all"
SMALL, LARGE = 512, 1024


def _prop(theme, tex=SMALL, tris=3000, **kw):
    """A prop's recipe: `theme` the maps it suits; `tex` its maps' size; `tris` its budget. Optional: `src` (the
    source, if not the prop's own name), `keep` (a regex the kept parts' names match in full), `plant` (for a row of
    plants: which one, counted from the left, -1 the last), `rot` (degrees about Blender's X, Y, Z, first), `scale`,
    `swap` ({texture set: another set's maps}), `alpha` ({material: (colour map or None, alpha map)}), `atlas`
    (False: keep the materials apart), `origin` ("foot": the middle of what stands on the ground, not of the bounding
    box), `collide` ("box": the bounding box; None: nothing to collide with; (x, z): a box that wide and deep, as tall
    as the prop), `mount` (floor, wall: its back against a wall, ceiling: its top against one, water: floats, flush:
    set into the floor), `waterline` (metres above its bottom), `note`."""
    return {"theme": theme, "tex": tex, "tris": tris, **kw}


PROPS = {
    # Street and yard.
    "concrete_road_barrier": _prop([HAFEN, FLIEGERHORST], LARGE),
    "concrete_road_barrier_02": _prop([HAFEN, FLIEGERHORST], LARGE),
    "metal_trash_can": _prop([ALTSTADT, HAFEN], keep=r"metal_trash_can(_handle_(left|right))?",
                             note="the galvanised can, without its lid; the rusty one left out"),
    "trashbag": _prop([ALL], collide=None),
    "fire_hydrant": _prop([ALTSTADT], keep=r"fire_hydrant(_cap_0[123])?", note="the painted one, without its chain"),
    "street_lamp_01": _prop([ALTSTADT], LARGE, origin="foot", collide=(0.24, 0.24), note="lantern on a post"),
    "street_lamp_02": _prop([ALTSTADT], mount="wall", collide=None, note="wall lantern on a bracket"),
    "security_light": _prop([FLIEGERHORST, HAFEN], mount="wall", collide=None),
    "security_camera_01": _prop([FLIEGERHORST, HAFEN], mount="wall", collide=None),
    "painted_wooden_bench": _prop([ALTSTADT], LARGE),
    "wooden_picnic_table": _prop([ALTSTADT, FLIEGERHORST], LARGE),
    "modular_street_seating": _prop([ALTSTADT], LARGE, keep=r"arm_rest_0[12]|back_support_[lr]|crossbar|"
                                    r"legs_(single|double)|seat|seat_back|suspended_support_01",
                                    note="the assembled bench with a back; the loose connectors left out"),
    "exterior_aircon_unit": _prop([ALTSTADT, HAFEN], keep=r"exterior_aircon_unit", atlas=False,
                                  alpha={"exterior_aircon_unit_02": ("exterior_aircon_unit_02_diff_1k.jpg",
                                                                     "exterior_aircon_unit_01_opacity_1k.jpg")},
                                  note="the clean one; the fan grille alpha-masked, a second material"),
    "power_box_01": _prop([ALL], mount="wall", note="door open"),
    "utility_box_01": _prop([ALTSTADT, HAFEN]),
    "utility_box_02": _prop([ALTSTADT, HAFEN], LARGE),
    "rollershutter_door": _prop([HAFEN, FLIEGERHORST], keep=r"rollershutter_door", mount="wall"),
    "rollershutter_door_graffiti": _prop([ALTSTADT, HAFEN], src="rollershutter_door",
                                         keep=r"rollershutter_door_graffiti", mount="wall"),
    "hanging_industrial_lamp": _prop([FLIEGERHORST, HAFEN], mount="ceiling", collide=None),
    "mounted_fluorescent_lights": _prop([FLIEGERHORST, ALL], keep=r"mounted_fluorescent_lights_b", mount="ceiling",
                                        collide=None, note="the twin-tube fitting"),
    "water_manhole_cover": _prop([ALL], mount="flush", collide=None),
    # Military and storage.
    "old_military_crate": _prop([FLIEGERHORST], keep=r"old_military_crate_(\w+_)?a", note="the closed one"),
    "wooden_military_crate": _prop([FLIEGERHORST, HAFEN], LARGE),
    "ammo_box": _prop([FLIEGERHORST]),
    "metal_jerrycan_green": _prop([FLIEGERHORST, ALL]),
    "propane_tank": _prop([ALL]),
    "portable_generator": _prop([FLIEGERHORST, HAFEN]),
    "old_tyre": _prop([HAFEN, FLIEGERHORST], rot=(90, 0, 0), note="lying flat"),
    "cardboard_box_01": _prop([ALL]),
    "cement_bag": _prop([ALTSTADT, HAFEN], collide=None),
    "plastic_crate_01": _prop([HAFEN, ALL]),
    "steel_frame_shelves_01": _prop([FLIEGERHORST, HAFEN], LARGE, scale=0.1, note="scaled to a tenth: the source is ten times too big"),
    "worn_metal_rack": _prop([HAFEN, FLIEGERHORST], LARGE),
    "industrial_storage_cart": _prop([FLIEGERHORST, HAFEN], LARGE),
    "hand_truck": _prop([HAFEN, FLIEGERHORST]),
    "metal_tool_chest": _prop([FLIEGERHORST], note="lid open"),
    "metal_office_desk": _prop([FLIEGERHORST], LARGE),
    "drawer_cabinet": _prop([FLIEGERHORST, ALTSTADT], LARGE),
    "schoolchair_01": _prop([FLIEGERHORST, ALTSTADT]),
    "wetfloorsign_01": _prop([FLIEGERHORST, ALTSTADT]),
    # Barrels.
    "barrel_01": _prop([ALL], swap={"explosive": "oxide_green"}, note="olive paint: the oxide_green texture set"),
    "barrel_02": _prop([HAFEN, ALL]),
    "barrel_03": _prop([HAFEN, ALL]),
    "wooden_barrels_01": _prop([ALTSTADT, HAFEN], keep=r"wooden_barrels_01_barrel01",
                               note="one of the set's small barrels"),
    "wine_barrel_01": _prop([ALTSTADT]),
    # Harbour.
    "ocean_buoy": _prop([HAFEN], LARGE, mount="water", waterline=0.81),
    "lifebuoy": _prop([HAFEN], mount="wall", collide=None),
    # Plants.
    "shrub_01": _prop([ALTSTADT, ALL], tris=2500, plant=-1, collide=None, note="one weed of the source's row"),
    "shrub_02": _prop([ALTSTADT, FLIEGERHORST], tris=6000, keep=r"shrub_02_b", collide=None,
                      note="one bush of the set's four"),
    "tree_small_02": _prop([ALTSTADT, FLIEGERHORST], LARGE, tris=6000, origin="foot", collide=(0.3, 0.3),
                           cards={"leaves": "leaves", "twigs": 300, "wood": 2600, "cell": 0.3},
                           note="leaves as cluster cards rendered from the source's own"),
    "fir_sapling": _prop([FLIEGERHORST, ALL], tris=6000, keep=r"fir_sapling_a", origin="foot", collide=None,
                         cards={"leaves": "twigs", "twigs": 0, "wood": 1000, "cell": 0.1, "sparse": 0.01},
                         note="the tallest of three; needles as cluster cards; stands in for shrub_04, a 22 cm sprig"),
    "planter_box_01": _prop([ALTSTADT]),
    # Building parts.
    "modular_metal_gutter": _prop([ALTSTADT, ALL], keep=r"modular_metal_gutter", mount="wall", collide=None,
                                  note="a straight 1.5 m length of gutter"),
    "modular_metal_gutter_downpipe": _prop([ALTSTADT, ALL], src="modular_metal_gutter",
                                           keep=r"modular_metal_gutter_section", mount="wall", collide=None,
                                           note="a straight 1 m length of downpipe"),
    "modular_pipes": _prop([HAFEN, FLIEGERHORST], keep=r"pipe_200cm_metal", rot=(0, 90, 0), collide=None,
                           note="a straight 2 m steel pipe, lying along X"),
    # The look pass (2026-09-29): parked cars under covers, planters and pots, lanterns and lamps, yard clutter.
    "covered_car": _prop([ALTSTADT, HAFEN, FLIEGERHORST], LARGE, tris=5000, note="a car under a tarpaulin"),
    "planter_box_02": _prop([ALTSTADT]),
    "planter_box_03": _prop([ALTSTADT]),
    "potted_plant_02": _prop([ALTSTADT], tris=5000, collide=None),
    "wooden_crate_02": _prop([ALL]),
    "lantern_01": _prop([ALTSTADT], collide=None),
    "industrial_wall_lamp": _prop([HAFEN, FLIEGERHORST], mount="wall", collide=None),
    "barrel_stove": _prop([HAFEN]),
    "wooden_bucket_01": _prop([ALTSTADT], collide=None),
    "metal_jerrycan": _prop([ALL], collide=None),
    # Office dressing (the Fliegerhorst's Stab).
    "hanging_picture_frame_01": _prop([ALL], mount="wall", collide=None),
    "wall_clock": _prop([ALL], mount="wall", collide=None),
    "korean_fire_extinguisher_01": _prop([ALL], collide=None, note="an extinguisher standing on the floor"),
    "fire_alarm": _prop([ALL], mount="wall", collide=None),
    "steel_frame_shelves_02": _prop([ALL], LARGE),
    # Street clutter and life (2026-09-30).
    "street_rat": _prop([ALL], tris=1500, collide=None, note="for the rats that scurry about (game-side, not baked)"),
    "can_rusted": _prop([ALL], tris=500, collide=None),
    "plastic_bottle_gallon": _prop([ALL], tris=900, collide=None),
    "wine_bottle": _prop([ALTSTADT], src="wine_bottles_01", keep=r"wine_bottles_01_bordeaux", tris=600, collide=None),
    "compost_bags": _prop([ALTSTADT, HAFEN], src="compost_bags", keep=r"compost_bags_floorstacked", tris=2000),
    "compost_bag_leaning": _prop([ALL], src="compost_bags", keep=r"compost_bags_leaning", tris=1200, collide=None),
    "plastic_crate_02": _prop([ALL]),
    "plastic_monobloc_chair_01": _prop([ALL], tris=1500, collide=None),
    "wooden_broom": _prop([ALL], keep=r"wooden_broom", tris=1500, collide=None, rot=(0, 14, 0), note="leaning"),
    "stone_01": _prop([ALL], tris=1200, collide=None),
    "standing_chalkboard_01": _prop([ALTSTADT], tris=1200, collide=None),
    "watering_can_metal_01": _prop([ALTSTADT], tris=1200, collide=None),
    "rock_moss_01": _prop([ALL], src="rock_moss_set_01", keep=r"rock_moss_set_01_rock01", tris=1200, collide=None),
    "rock_moss_03": _prop([ALL], src="rock_moss_set_01", keep=r"rock_moss_set_01_rock03", tris=1200, collide=None),
}
PLANTS = {"shrub_01", "shrub_02", "tree_small_02", "fir_sapling", "potted_plant_02"}


def _select(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def _edit(obj, fn):
    """Runs `fn` in edit mode on all of `obj`."""
    _select(obj)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    fn()
    bpy.ops.object.mode_set(mode="OBJECT")


def _tris(obj) -> int:
    return sum(len(p.vertices) - 2 for p in obj.data.polygons)


def _join(objs):
    body = max(objs, key=lambda o: len(o.data.polygons))
    if len(objs) > 1:
        bpy.ops.object.select_all(action="DESELECT")
        for o in objs:
            o.select_set(True)
        bpy.context.view_layer.objects.active = body
        bpy.ops.object.join()
    return body


def _weld(obj):
    """Merges the vertices glTF keeps apart at UV seams and hard edges (so the decimator sees closed surfaces), and
    drops the imported custom normals (the shading is redone by angle after decimating)."""
    _edit(obj, lambda: bpy.ops.mesh.remove_doubles(threshold=1e-4))
    _select(obj)
    if obj.data.has_custom_normals:
        bpy.ops.mesh.customdata_custom_splitnormals_clear()


def _clean(obj):
    """Drops what decimating leaves behind: zero-area faces and loose bits (they would wreck the lightmap unwrap)."""
    def ops():
        bpy.ops.mesh.dissolve_degenerate(threshold=1e-5)
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.mesh.delete_loose(use_verts=True, use_edges=True, use_faces=False)

    _edit(obj, ops)


def _keep_plant(obj, which: int):
    """Of a row of plants along X in one mesh, keeps the `which`th (split where the loose parts' middles leave a gap
    along X)."""
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.faces.ensure_lookup_table()
    seen, islands = set(), []
    for f in bm.faces:
        if f.index in seen:
            continue
        stack, isl = [f], []
        seen.add(f.index)
        while stack:
            g = stack.pop()
            isl.append(g.index)
            for v in g.verts:
                for h in v.link_faces:
                    if h.index not in seen:
                        seen.add(h.index)
                        stack.append(h)
        xs = [v.co.x for i in isl for v in bm.faces[i].verts]
        islands.append((min(xs), max(xs), isl))
    islands.sort(key=lambda t: t[0])
    plants, reach = [], None
    for lo, hi, isl in islands:
        if reach is None or lo > reach + 0.03:
            plants.append([])
        plants[-1].extend(isl)
        reach = hi if reach is None else max(reach, hi)
    keep = set(plants[which])
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.index not in keep], context="FACES")
    bm.to_mesh(obj.data)
    bm.free()
    print(f"  plant {which} of {len(plants)}: {_tris(obj)} tris")


def _swap(swap: dict):
    """Points every image of a texture set at another set's maps (`{"explosive": "oxide_green"}`)."""
    for im in list(bpy.data.images):
        for a, b in swap.items():
            path = Path(bpy.path.abspath(im.filepath))
            other = path.with_name(path.name.replace(f"_{a}_", f"_{b}_"))
            if f"_{a}_" in path.name and other.exists():
                # (The importer packs its images, so pointing this one elsewhere and reloading changes nothing.)
                new = bpy.data.images.load(str(other))
                new.colorspace_settings.name = im.colorspace_settings.name
                im.user_remap(new)


def _texture_dir(obj) -> Path:
    im = next(n.image for s in obj.material_slots for n in s.material.node_tree.nodes if n.type == "TEX_IMAGE" and n.image)
    return Path(bpy.path.abspath(im.filepath)).parent


def _set_alpha(mat, colour: Path | None, alpha: Path, cutoff: float = 0.5):
    """An alpha mask for `mat` from its own map `alpha` (Poly Haven's 1k colour maps are JPEGs, without it), wired
    the way the glTF importer wires MASK (1 - (alpha < cutoff)), which the exporter writes back as MASK; `colour`
    replaces the colour map."""
    nt = mat.node_tree
    p = _principled(mat)
    tex = p.inputs["Base Color"].links[0].from_node
    if colour:
        tex.image = bpy.data.images.load(str(colour), check_existing=True)
    for link in list(p.inputs["Alpha"].links):
        nt.links.remove(link)
    a = nt.nodes.new("ShaderNodeTexImage")
    a.image = bpy.data.images.load(str(alpha), check_existing=True)
    a.image.colorspace_settings.name = "Non-Color"
    if tex.inputs["Vector"].is_linked:
        nt.links.new(tex.inputs["Vector"].links[0].from_socket, a.inputs["Vector"])
    lt = nt.nodes.new("ShaderNodeMath")
    lt.operation = "LESS_THAN"
    lt.inputs[1].default_value = cutoff
    sub = nt.nodes.new("ShaderNodeMath")
    sub.operation = "SUBTRACT"
    sub.inputs[0].default_value = 1.0
    nt.links.new(a.outputs["Color"], lt.inputs[0])
    nt.links.new(lt.outputs["Value"], sub.inputs[1])
    nt.links.new(sub.outputs["Value"], p.inputs["Alpha"])


def _has_alpha(mat) -> bool:
    p = _principled(mat)
    return bool(p and p.inputs["Alpha"].is_linked)


def _occlusion_from_arm(mat):
    """Poly Haven's `arm` map packs occlusion (R), roughness (G) and metal (B); the importer reads only G and B.
    This wires R to the glTF occlusion too, so the exporter writes one ORM texture for all three."""
    from io_scene_gltf2.blender.com.gltf2_blender_material_helpers import create_settings_group, get_gltf_node_name

    p = _principled(mat)
    r = p.inputs["Roughness"]
    sep = r.links[0].from_node if r.is_linked else None
    if not sep or sep.type != "SEPARATE_COLOR" or not sep.inputs["Color"].is_linked:
        return
    tex = sep.inputs["Color"].links[0].from_node
    if tex.type != "TEX_IMAGE" or not tex.image or "_arm_" not in Path(tex.image.filepath).name or _occlusion(mat):
        return
    nt = mat.node_tree
    group = bpy.data.node_groups.get(get_gltf_node_name()) or create_settings_group(get_gltf_node_name())
    g = nt.nodes.new("ShaderNodeGroup")
    g.node_tree = group
    nt.links.new(sep.outputs["Red"], g.inputs["Occlusion"])


def _bounds(obj):
    import numpy as np

    co = np.empty(len(obj.data.vertices) * 3)
    obj.data.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    return co, co.min(0), co.max(0)


def _to_origin(obj, how: str | None):
    """The middle of the base (of the bounding box, or with "foot" of what stands on the ground: the lowest 5%) to
    the origin, the bottom of the bounding box at z = 0."""
    co, lo, hi = _bounds(obj)
    if how == "foot":
        foot = co[co[:, 2] < lo[2] + 0.05 * (hi[2] - lo[2])]
        mid = (foot.min(0) + foot.max(0)) / 2
    else:
        mid = (lo + hi) / 2
    obj.data.transform(Matrix.Translation(Vector((-mid[0], -mid[1], -lo[2]))))
    obj.data.update()


def _lightmap_uv(obj, margin: float, angle: float = 66.0):
    """A second UV map, `lightmap`: Smart UV Project, packed into 0..1 with `margin` (UV units) between islands.
    The first stays the one the maps are read through (TEXCOORD_0); this one is written after it (TEXCOORD_1)."""
    me = obj.data
    if "lightmap" in me.uv_layers:
        me.uv_layers.remove(me.uv_layers["lightmap"])
    first = me.uv_layers[0].name
    lay = me.uv_layers.new(name="lightmap")
    me.uv_layers.active = lay
    def ops():
        # (Margins "ADD"ed in UV units: Blender 4.2's "FRACTION" margins overshoot the square, to 3x on some props.)
        bpy.ops.uv.smart_project(angle_limit=math.radians(angle), margin_method="ADD", island_margin=margin,
                                 area_weight=0.0, correct_aspect=True, scale_to_bounds=False)
        bpy.ops.uv.select_all(action="SELECT")
        bpy.ops.uv.pack_islands(udim_source="ACTIVE_UDIM", rotate=True, margin_method="ADD", margin=margin,
                                shape_method="CONCAVE")

    _edit(obj, ops)
    # Faces the unwrap folded onto others (decimating can flip a triangle within a flat patch): cut them loose, each
    # an island of its own, and unwrap again.
    tool = bpy.context.scene.tool_settings
    for _ in range(3):
        _select(obj)
        bpy.ops.object.mode_set(mode="EDIT")
        tool.use_uv_select_sync = True
        bpy.ops.mesh.select_mode(type="FACE")
        bpy.ops.mesh.select_all(action="DESELECT")
        bpy.ops.uv.select_overlap()
        bpy.ops.object.mode_set(mode="OBJECT")
        folded = sum(f.select for f in me.polygons)
        if not folded:
            break
        print(f"  lightmap: {folded} faces overlap; cutting them loose")
        if not me.has_custom_normals:  # (keep the shading as it is across the cuts)
            me.normals_split_custom_set([tuple(c.normal) for c in me.loops])
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.edge_split(type="EDGE")
        bpy.ops.object.mode_set(mode="OBJECT")
        _edit(obj, ops)
    tool.use_uv_select_sync = False
    me.uv_layers.active = me.uv_layers[first]
    me.uv_layers[first].active_render = True
    import numpy as np

    uv = np.empty(len(me.loops) * 2)
    me.uv_layers["lightmap"].data.foreach_get("uv", uv)  # (edit mode rebuilt the layers: `lay` is stale)
    lo, hi = uv.min(), uv.max()
    if lo < 0 or hi > 1:  # the packer left some out: scale the lot back in (the layout stays as it is)
        uv = (uv - lo) / (hi - lo) * (1 - 2 * margin) + margin
        me.uv_layers["lightmap"].data.foreach_set("uv", uv)
    assert uv.min() >= -1e-4 and uv.max() <= 1 + 1e-4, f"lightmap UVs outside 0..1: {uv.min()} {uv.max()}"
    # How much of the square the islands fill (the baker's chart for the prop wants area / fill texels' worth).
    me.calc_loop_triangles()
    tl = np.empty(len(me.loop_triangles) * 3, dtype=np.int64)
    me.loop_triangles.foreach_get("loops", tl)
    t = uv.reshape(-1, 2)[tl.reshape(-1, 3)]
    e1, e2 = t[:, 1] - t[:, 0], t[:, 2] - t[:, 0]
    return float(np.abs(e1[:, 0] * e2[:, 1] - e1[:, 1] * e2[:, 0]).sum() / 2)


def _srgb(c: float) -> float:
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def _linear(c):
    import numpy as np

    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def _albedo(obj, samples: int = 40000):
    """The surface's mean colour as sRGB: the base colour maps sampled over it by area (masked-out texels left
    out), averaged in linear light."""
    import numpy as np

    me = obj.data
    me.calc_loop_triangles()
    n = len(me.loop_triangles)
    tri_loops = np.empty(n * 3, dtype=np.int64)
    me.loop_triangles.foreach_get("loops", tri_loops)
    tri_loops = tri_loops.reshape(-1, 3)
    mat_idx = np.empty(n, dtype=np.int64)
    me.loop_triangles.foreach_get("material_index", mat_idx)
    area = np.empty(n)
    me.loop_triangles.foreach_get("area", area)
    uv = np.empty(len(me.loops) * 2)
    me.uv_layers[0].data.foreach_get("uv", uv)
    uv = uv.reshape(-1, 2)
    rng = np.random.default_rng(7)
    pick = rng.choice(n, size=samples, p=area / area.sum())
    b = rng.random((samples, 2))
    flip = b.sum(1) > 1
    b[flip] = 1 - b[flip]
    w = np.stack([1 - b[:, 0] - b[:, 1], b[:, 0], b[:, 1]], 1)
    at = (uv[tri_loops[pick]] * w[:, :, None]).sum(1)
    total, weight = np.zeros(3), 0.0
    for i, slot in enumerate(obj.material_slots):
        sel = mat_idx[pick] == i
        if not sel.any():
            continue
        p = _principled(slot.material)
        src = _socket_source(p.inputs["Base Color"])
        if src is None or src.node.type != "TEX_IMAGE":
            rgb = np.tile(np.array(p.inputs["Base Color"].default_value[:3]), (sel.sum(), 1))
            alpha = np.ones(sel.sum())
        else:
            im = src.node.image
            w_, h_ = im.size
            px = np.empty(w_ * h_ * 4, dtype=np.float32)
            im.pixels.foreach_get(px)
            px = px.reshape(h_, w_, 4)
            x = (np.mod(at[sel, 0], 1) * w_).astype(int).clip(0, w_ - 1)
            y = (np.mod(at[sel, 1], 1) * h_).astype(int).clip(0, h_ - 1)
            rgb = px[y, x, :3].astype(float)
            if not im.is_float:
                rgb = _linear(rgb)
            alpha = px[y, x, 3].astype(float)
            asrc = _socket_source(p.inputs["Alpha"])
            if asrc is not None:
                # 1 - (a < cutoff), a from its own map (its red) or the colour map's alpha.
                sock = asrc
                while sock.node.type == "MATH":
                    sock = next(i for i in sock.node.inputs if i.is_linked).links[0].from_socket
                am, channel = sock.node.image, 3 if sock.name == "Alpha" else 0
                aw, ah = am.size
                apx = np.empty(aw * ah * 4, dtype=np.float32)
                am.pixels.foreach_get(apx)
                apx = apx.reshape(ah, aw, 4)
                ax = (np.mod(at[sel, 0], 1) * aw).astype(int).clip(0, aw - 1)
                ay = (np.mod(at[sel, 1], 1) * ah).astype(int).clip(0, ah - 1)
                alpha = (apx[ay, ax, channel] >= 0.5).astype(float)
            else:
                alpha = np.ones(sel.sum())
        total += (rgb * alpha[:, None]).sum(0)
        weight += alpha.sum()
    mean = total / max(weight, 1e-9)
    return [round(_srgb(float(c)), 3) for c in mean]


def _area(obj) -> float:
    import numpy as np

    a = np.empty(len(obj.data.polygons))
    obj.data.polygons.foreach_get("area", a)
    return float(a.sum())


def _export_prop(name: str, obj) -> Path:
    PROPS_OUT.mkdir(parents=True, exist_ok=True)
    path = PROPS_OUT / f"{name}.glb"
    # Every map re-encoded (an untouched source JPEG would go in as it is: Poly Haven's are 0.5 to 1 MB at 1k).
    import numpy as np

    for im in bpy.data.images:
        if im.size[0] and not im.is_dirty:
            px = np.empty(im.size[0] * im.size[1] * 4, dtype=np.float32)
            im.pixels.foreach_get(px)
            im.pixels.foreach_set(px)  # (dirty now: the exporter encodes it instead of copying the file)
    # Colour maps with alpha go out as PNG, everything else as JPEG.
    rgba = set()
    for s in obj.material_slots:
        if _has_alpha(s.material):
            src = _socket_source(_principled(s.material).inputs["Base Color"])
            if src is not None and src.node.type == "TEX_IMAGE":
                rgba.add(src.node.image.name)
    for im in bpy.data.images:
        im.file_format = "PNG" if im.name in rgba else "JPEG"
    _select(obj)
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_materials="EXPORT",
        export_image_format="JPEG",  # (a map feeding alpha goes out as PNG all the same)
        export_image_quality=86,
        export_animations=False,
        export_skins=False,
        export_morph=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
    )
    return path


def _glb_check(path: Path) -> dict:
    """Triangles, materials and images of the written GLB (its JSON), and that every primitive has TEXCOORD_1."""
    sys.path.insert(0, str(HERE))
    from stats import gltf

    g = gltf(path)
    acc = g["accessors"]
    prims = [p for m in g["meshes"] for p in m["primitives"]]
    assert all("TEXCOORD_1" in p["attributes"] for p in prims), f"{path.name}: no TEXCOORD_1"
    tris = sum(acc[p["indices"]]["count"] // 3 for p in prims)
    images = [(im.get("mimeType"), g["bufferViews"][im["bufferView"]]["byteLength"]) for im in g.get("images", [])]
    modes = [m.get("alphaMode", "OPAQUE") for m in g.get("materials", [])]
    return {"tris": tris, "materials": len(g.get("materials", [])), "images": images, "alpha": modes}


def env_prop(name: str) -> dict:
    """Builds props/<name>.glb (see the section's comment and _prop) and returns its props.json entry."""
    spec = PROPS[name]
    reset()
    objs = load(source(spec.get("src", name)))
    parts = meshes(objs)
    loose(parts)
    remove([o for o in objs if o.type != "MESH"])
    if "keep" in spec:
        pattern = re.compile(spec["keep"])
        parts = without(parts, lambda o: not pattern.fullmatch(o.name))
    assert parts, f"{name}: nothing matches {spec.get('keep')}"
    for o in parts:
        if o.data.users > 1:
            o.data = o.data.copy()
        o.data.transform(o.matrix_world)
        o.matrix_world = Matrix.Identity(4)
    body = _join(parts)
    body.name = name
    _select(body)
    bpy.ops.object.material_slot_remove_unused()
    m = Matrix.Scale(spec.get("scale", 1.0), 4) @ Euler([math.radians(a) for a in spec.get("rot", (0, 0, 0))]).to_matrix().to_4x4()
    body.data.transform(m)
    if "plant" in spec:
        _keep_plant(body, spec["plant"])
    _weld(body)
    if spec.get("swap"):
        _swap(spec["swap"])
    here = _texture_dir(body)
    for mat_name, (colour, alpha) in spec.get("alpha", {}).items():
        _set_alpha(bpy.data.materials[mat_name], here / colour if colour else None, here / alpha)
    for s in body.material_slots:
        mat = s.material
        _occlusion_from_arm(mat)
        # Glass (BLEND) goes opaque: a lamp's pane in the atlas, not a material of its own to sort and blend.
        if name not in PLANTS and mat.name not in spec.get("alpha", {}) and _has_alpha(mat):
            for link in list(_principled(mat).inputs["Alpha"].links):
                mat.node_tree.links.remove(link)
        # The plants' own alpha maps (their JPEG colour maps' alpha is empty), where there is one.
        colour = _socket_source(_principled(mat).inputs["Base Color"])
        if name in PLANTS and colour is not None and colour.node.type == "TEX_IMAGE":
            stem = Path(bpy.path.abspath(colour.node.image.filepath))
            mask = stem.with_name(stem.name.replace("_diff_", "_alpha_"))
            if mask.exists() and mask != stem:
                _set_alpha(mat, None, mask)
    source_tris = _tris(body)
    if spec.get("cards"):
        body = _tree(name, body, spec)
    else:
        decimate([body], spec["tris"])
        _clean(body)
    mats = [s.material for s in body.material_slots]
    if len(mats) > 1 and spec.get("atlas", True) and not any(_has_alpha(mt) for mt in mats):
        tex = spec["tex"]
        body = atlas([body], name, tex, tex, tex)
    if not spec.get("cards"):  # (cards carry their own normals)
        _select(body)
        bpy.ops.object.shade_smooth_by_angle(angle=math.radians(40), keep_sharp_edges=False)
    _to_origin(body, spec.get("origin"))
    shrink_textures(spec["tex"])
    fill = _lightmap_uv(body, 0.004 if name in PLANTS else 0.02)
    path = _export_prop(name, body)
    check = _glb_check(path)
    _, lo, hi = _bounds(body)
    size = [float(hi[0] - lo[0]), float(hi[2] - lo[2]), float(hi[1] - lo[1])]  # glTF x, y (up), z
    collide = spec.get("collide", "box")
    if collide == "box":
        collide = [size[0] / 2, size[1] / 2, size[2] / 2]
    elif collide is not None:
        collide = [collide[0] / 2, size[1] / 2, collide[1] / 2]
    entry = {
        "tris": check["tris"],
        "bytes": path.stat().st_size,
        "size": [round(v, 3) for v in size],
        "area": round(_area(body), 3),
        "lightmap_fill": round(fill, 3),
        "collide": [round(v, 3) for v in collide] if collide else None,
        "albedo": _albedo(body),
        "theme": spec["theme"],
        "mount": spec.get("mount", "floor"),
    }
    if "waterline" in spec:
        entry["waterline"] = spec["waterline"]
    print(f"{name}.glb: {source_tris} -> {check['tris']} tris, {entry['bytes'] / 1e3:.0f} kB, size {entry['size']}, "
          f"materials {check['materials']} {check['alpha']}, images {check['images']}")
    return entry


def props(names=None):
    """Builds the props (all, or `names`) and merges their entries into props/props.json."""
    names = names or list(PROPS)
    PROPS_OUT.mkdir(parents=True, exist_ok=True)
    index = PROPS_OUT / "props.json"
    entries = json.loads(index.read_text()) if index.exists() else {}
    for name in names:
        print(f"== {name}")
        entries[name] = env_prop(name)
    # (Pieces modelled by arch.py keep their entries.)
    entries = {k: entries[k] for k in PROPS if k in entries} | {k: v for k, v in entries.items() if v.get("made")}
    index.write_text(json.dumps(entries, indent=1) + "\n")


# ---- the tree: leaves as cluster cards ----------------------------------------------------------------------------
#
# The tree's two million triangles are thirty thousand modelled leaves. Decimated, they would melt; thinned out, they
# would be a few big leaves. Instead a few clumps of the tree's own leaves are rendered (colour and alpha, from the
# side) into the tiles of one map, and the canopy is rebuilt from crossed cards carrying those tiles, one pair per
# cell of a grid where the leaves are. The trunk and the main branches are decimated and baked into a map of their
# own (the branches read their maps through a second, tiled UV map, which the atlas can't carry).


def _rebake(obj, name: str, size: int):
    """Bakes `obj`'s materials (whatever their nodes and UV maps) into one material `name` through a fresh unwrap:
    base colour, tangent-space normal, occlusion/roughness/metal."""
    import numpy as np

    me = obj.data
    src_uv = me.uv_layers[0].name
    mats = [s.material for s in obj.material_slots]
    for mt in mats:
        for n in mt.node_tree.nodes:
            if n.type == "TEX_IMAGE" and not n.inputs["Vector"].is_linked:
                uvn = mt.node_tree.nodes.new("ShaderNodeUVMap")
                uvn.uv_map = src_uv
                mt.node_tree.links.new(uvn.outputs["UV"], n.inputs["Vector"])
    lay = me.uv_layers.new(name="bake")
    me.uv_layers.active = lay
    _edit(obj, lambda: bpy.ops.uv.smart_project(angle_limit=math.radians(66), margin_method="ADD",
                                                island_margin=0.01, area_weight=0.0, correct_aspect=True,
                                                scale_to_bounds=True))
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 8
    scene.cycles.use_denoising = False
    bake = scene.render.bake
    bake.target = "IMAGE_TEXTURES"
    bake.use_clear = True
    bake.margin = 8
    bake.margin_type = "EXTEND"
    bake.use_selected_to_active = False

    def img(suffix, data):
        im = bpy.data.images.new(f"{name}_{suffix}", size, size, alpha=False)
        im.colorspace_settings.name = "Non-Color" if data else "sRGB"
        return im

    normal = img("normal", True)
    _bake_into(obj, mats, normal, "NORMAL")
    orm = img("orm", True)
    for mt in mats:
        p = _principled(mt)
        occ = _occlusion(mt)
        _emit(mt, [(_socket_source(occ) if occ else None, occ.default_value if occ and not occ.is_linked else 1.0),
                   (_socket_source(p.inputs["Roughness"]), p.inputs["Roughness"].default_value),
                   (_socket_source(p.inputs["Metallic"]), p.inputs["Metallic"].default_value)])
    _bake_into(obj, mats, orm, "EMIT")
    base = img("baseColor", False)
    for mt in mats:
        p = _principled(mt)
        _emit(mt, [(_socket_source(p.inputs["Base Color"]), tuple(p.inputs["Base Color"].default_value))])
    _bake_into(obj, mats, base, "EMIT")
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    p = _principled(mat)
    tex = lambda im: (lambda n: (setattr(n, "image", im), n)[1])(nt.nodes.new("ShaderNodeTexImage"))
    nt.links.new(tex(base).outputs["Color"], p.inputs["Base Color"])
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(tex(normal).outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], p.inputs["Normal"])
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(tex(orm).outputs["Color"], sep.inputs["Color"])
    nt.links.new(sep.outputs["Green"], p.inputs["Roughness"])
    nt.links.new(sep.outputs["Blue"], p.inputs["Metallic"])
    from io_scene_gltf2.blender.com.gltf2_blender_material_helpers import create_settings_group, get_gltf_node_name

    group = bpy.data.node_groups.get(get_gltf_node_name()) or create_settings_group(get_gltf_node_name())
    g = nt.nodes.new("ShaderNodeGroup")
    g.node_tree = group
    nt.links.new(sep.outputs["Red"], g.inputs["Occlusion"])
    me.materials.clear()
    me.materials.append(mat)
    for n in [l.name for l in me.uv_layers if l.name != "bake"]:
        me.uv_layers.remove(me.uv_layers[n])
    me.uv_layers["bake"].name = "UVMap"
    me.uv_layers[0].active_render = True
    return mat


def _dilate(px, steps: int = 16):
    """Spreads the colour of opaque texels into the transparent ones around them (so mipmaps don't pull in black)."""
    import numpy as np

    rgb, a = px[..., :3].copy(), px[..., 3]
    filled = a > 0.02
    rgb[~filled] = 0
    for _ in range(steps):
        acc = np.zeros_like(rgb)
        cnt = np.zeros(a.shape)
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            f = np.roll(filled, (dy, dx), (0, 1))
            acc += np.roll(rgb, (dy, dx), (0, 1)) * f[..., None]
            cnt += f
        grow = ~filled & (cnt > 0)
        rgb[grow] = acc[grow] / cnt[grow, None]
        filled = filled | grow
    out = px.copy()
    out[..., :3] = rgb
    return out


def _leaf_tiles(leaves, centres, side: float, tile: int, name: str):
    """Renders the leaves inside a `side` m cube around each of `centres` (seen along +Y, unlit colour and alpha)
    into the tiles of one square map, two by two; returns the image."""
    import numpy as np

    scene = bpy.context.scene
    for o in scene.objects:
        o.hide_render = o is not leaves
    mat = leaves.material_slots[0].material
    nt = mat.node_tree
    p = _principled(mat)
    colour = _socket_source(p.inputs["Base Color"])
    alpha = _socket_source(p.inputs["Alpha"])
    em = nt.nodes.new("ShaderNodeEmission")
    nt.links.new(colour, em.inputs["Color"])
    tr = nt.nodes.new("ShaderNodeBsdfTransparent")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(alpha, mix.inputs["Fac"])
    nt.links.new(tr.outputs[0], mix.inputs[1])
    nt.links.new(em.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], _output(mat).inputs["Surface"])
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 16
    scene.cycles.use_denoising = False
    scene.cycles.max_bounces = 0
    scene.cycles.transparent_max_bounces = 64
    scene.render.film_transparent = True
    scene.view_settings.view_transform = "Standard"
    scene.render.resolution_x = scene.render.resolution_y = tile
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    world = bpy.data.worlds.new("black")
    world.color = (0, 0, 0)
    scene.world = world
    cam = bpy.data.objects.new("leafcam", bpy.data.cameras.new("leafcam"))
    scene.collection.objects.link(cam)
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = side
    scene.camera = cam
    n = int(math.ceil(math.sqrt(len(centres))))
    atlas_px = np.zeros((n * tile, n * tile, 4), dtype=np.float32)
    tmp = Path(bpy.app.tempdir or "/tmp") / f"{name}_tile.png"
    for i, c in enumerate(centres):
        cam.location = Vector(c) + Vector((0, -side, 0))
        cam.rotation_euler = (math.radians(90), 0, 0)
        cam.data.clip_start = side / 2
        cam.data.clip_end = side * 1.5
        scene.render.filepath = str(tmp)
        bpy.ops.render.render(write_still=True)
        im = bpy.data.images.load(str(tmp))
        px = np.empty(tile * tile * 4, dtype=np.float32)
        im.pixels.foreach_get(px)
        bpy.data.images.remove(im)
        px = px.reshape(tile, tile, 4)
        # Round clumps, not squares: the alpha fades out towards the tile's rim (a cut-off branch would show the
        # card's straight edge).
        g = (np.arange(tile) + 0.5) / tile * 2 - 1
        d = np.sqrt(g[None, :] ** 2 + g[:, None] ** 2)
        # (a ragged rim: low-frequency noise on the radius)
        coarse = np.random.default_rng(i).random((6, 6))
        k = np.linspace(0, 5, tile)
        i0 = np.floor(k).astype(int).clip(0, 4)
        f = k - i0
        rows = coarse[i0] * (1 - f)[:, None] + coarse[i0 + 1] * f[:, None]
        noise = rows[:, i0] * (1 - f)[None, :] + rows[:, i0 + 1] * f[None, :]
        px[..., 3] *= np.clip((0.95 - d + 0.35 * (noise - 0.5)) / 0.25, 0, 1)
        x, y = i % n, i // n
        atlas_px[y * tile:(y + 1) * tile, x * tile:(x + 1) * tile] = px
    bpy.data.objects.remove(cam, do_unlink=True)
    for o in scene.objects:
        o.hide_render = False
    scene.render.film_transparent = False
    atlas_px = _dilate(atlas_px)
    im = bpy.data.images.new(f"{name}_leaves", n * tile, n * tile, alpha=True)
    im.pixels.foreach_set(atlas_px.ravel())
    im.file_format = "PNG"
    im.pack()
    return im, n


def _tree(name: str, body, spec):
    """A tree: the trunk and main branches decimated and rebaked, the leaves as cluster cards (see above). `cards`:
    `leaves` names the leaves' material, `twigs` the smallest branch (triangles) kept, `wood` the wood's budget,
    `cell` the grid's cell (m), `sparse` the emptiest cell that gets cards (against the fullest)."""
    opt = spec["cards"]
    import bmesh
    import numpy as np

    rng = np.random.default_rng(3)
    names = [s.material.name for s in body.material_slots]
    leaf_i = next(i for i, n in enumerate(names) if opt["leaves"] in n)
    # Split the leaves off.
    _select(body)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="DESELECT")
    body.active_material_index = leaf_i
    bpy.ops.object.material_slot_select()
    bpy.ops.mesh.separate(type="SELECTED")
    bpy.ops.object.mode_set(mode="OBJECT")
    leaves = next(o for o in bpy.context.selected_objects if o is not body)
    _select(leaves)
    bpy.ops.object.material_slot_remove_unused()
    wood = body
    # The wood: drop the twigs (branch islands of under 300 triangles: the cards hide them), decimate, rebake.
    bm = bmesh.new()
    bm.from_mesh(wood.data)
    bm.faces.ensure_lookup_table()
    seen, drop = set(), []
    branch_i = next((i for i, s in enumerate(wood.material_slots) if "branch" in s.material.name), -1)
    for f in bm.faces:
        if f.index in seen:
            continue
        stack, isl = [f], []
        seen.add(f.index)
        while stack:
            g = stack.pop()
            isl.append(g)
            for e in g.edges:
                for h in e.link_faces:
                    if h.index not in seen:
                        seen.add(h.index)
                        stack.append(h)
        if isl[0].material_index == branch_i and len(isl) < opt["twigs"]:
            drop.extend(isl)
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    bm.to_mesh(wood.data)
    bm.free()
    _select(wood)
    bpy.ops.object.material_slot_remove_unused()
    decimate([wood], opt["wood"])
    _rebake(wood, f"{name}_bark", 512)
    # Where the leaves are: their faces' middles and areas, binned into cells.
    me = leaves.data
    nf = len(me.polygons)
    ctr = np.empty(nf * 3)
    me.polygons.foreach_get("center", ctr)
    ctr = ctr.reshape(-1, 3)
    area = np.empty(nf)
    me.polygons.foreach_get("area", area)
    cell = opt["cell"]
    keys = np.floor(ctr / cell).astype(np.int64)
    uniq, inv = np.unique(keys, axis=0, return_inverse=True)
    inv = inv.ravel()
    cell_area = np.bincount(inv, weights=area)
    cell_ctr = np.stack([np.bincount(inv, weights=area * ctr[:, k]) for k in range(3)], 1) / cell_area[:, None]
    budget_cards = (spec["tris"] - _tris(wood)) // 4  # two crossed quads per cell
    order = np.argsort(-cell_area)
    chosen = order[:budget_cards]
    chosen = chosen[cell_area[chosen] > cell_area[order[0]] * opt.get("sparse", 0.03)]
    # Four clumps for the tiles: cells of middling density (the densest are a solid mass of leaves, which on a card
    # reads as a disc), far apart.
    ranked = order[:len(chosen)]
    dense = ranked[len(ranked) // 4:len(ranked) // 2]
    picks = [dense[0]]
    for _ in range(3):
        d = np.min([np.linalg.norm(cell_ctr[dense] - cell_ctr[p], axis=1) for p in picks], axis=0)
        picks.append(dense[int(np.argmax(d))])
    side = cell * 1.5
    image, n = _leaf_tiles(leaves, [tuple(cell_ctr[p]) for p in picks], side, 256, name)
    bpy.data.objects.remove(leaves, do_unlink=True)
    # The cards: two vertical quads per chosen cell, crossed at a random yaw, sized by how full the cell is.
    full = np.sqrt(np.clip(cell_area[chosen] / np.percentile(cell_area[chosen], 80), 0.35, 1.0))
    verts, faces, uvs = [], [], []
    for c, k in zip(cell_ctr[chosen], full):
        s = side * k / 2
        yaw = rng.uniform(0, math.pi)
        for j in range(2):
            a = yaw + j * math.pi / 2
            ax = np.array([math.cos(a), math.sin(a), 0.0])
            tilt = rng.normal(0, 0.15)
            up = np.array([0.0, 0.0, 1.0]) * math.cos(tilt) + np.cross(ax, [0, 0, 1]) * math.sin(tilt)
            t = rng.integers(0, 4)
            u0, v0 = (t % n) / n, (t // n) / n
            mirror = rng.random() < 0.5
            base = len(verts)
            for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                verts.append(tuple(c + ax * s * sx + up * s * sy))
                uu = (sx + 1) / 2
                uvs.append((u0 + ((1 - uu) if mirror else uu) / n, v0 + (sy + 1) / 2 / n))
            faces.append((base, base + 1, base + 2, base + 3))
    cme = bpy.data.meshes.new(f"{name}_cards")
    cme.from_pydata(verts, [], faces)
    uvl = cme.uv_layers.new(name="UVMap")
    for poly in cme.polygons:
        for li in poly.loop_indices:
            uvl.data[li].uv = uvs[cme.loops[li].vertex_index]
    cards = bpy.data.objects.new(f"{name}_cards", cme)
    bpy.context.scene.collection.objects.link(cards)
    leaf = bpy.data.materials.new(f"{name}_leaves")
    leaf.use_nodes = True
    nt = leaf.node_tree
    p = _principled(leaf)
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = image
    nt.links.new(t.outputs["Color"], p.inputs["Base Color"])
    lt = nt.nodes.new("ShaderNodeMath")
    lt.operation = "LESS_THAN"
    lt.inputs[1].default_value = 0.5
    sub = nt.nodes.new("ShaderNodeMath")
    sub.operation = "SUBTRACT"
    sub.inputs[0].default_value = 1.0
    nt.links.new(t.outputs["Alpha"], lt.inputs[0])
    nt.links.new(lt.outputs["Value"], sub.inputs[1])
    nt.links.new(sub.outputs["Value"], p.inputs["Alpha"])
    p.inputs["Roughness"].default_value = 0.8
    p.inputs["Metallic"].default_value = 0.0
    cme.materials.append(leaf)
    # The canopy's normals point out of it (from its middle), so the cards shade as one soft mass.
    mid = cell_ctr[chosen].mean(0)
    cme.update()
    norms = []
    for li in range(len(cme.loops)):
        v = np.array(cme.vertices[cme.loops[li].vertex_index].co) - mid
        v[2] *= 0.6
        norms.append(tuple(v / max(np.linalg.norm(v), 1e-6)))
    _select(wood)
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(60), keep_sharp_edges=False)
    cards.select_set(True)
    bpy.ops.object.join()
    tree = wood
    # (The join kept the bark's mesh; the cards' custom normals come across with them.)
    me = tree.data
    lo = len(me.loops) - len(norms)
    cur = [tuple(l.normal) for l in me.loops]
    cur[lo:] = norms
    me.normals_split_custom_set(cur)
    print(f"  tree: {len(chosen)} cells of {len(uniq)} -> {len(faces)} cards; {_tris(wood)} tris in all")
    return tree


RECIPES = {
    "soldier-blue": soldier_blue,
    "soldier-red": soldier_red,
    "hostage": hostage,
    "vm-rifle": vm_rifle,
    "vm-pistol": vm_pistol,
    "vm-hands": vm_hands,
    "vm-knife": vm_knife,
    "vm-grenade": vm_grenade,
    "m4": prop_m4,
    "pistol": prop_pistol,
    "knife": prop_knife,
    "grenade": prop_grenade,
    "vm-ak": vm_ak,
    "ak": prop_ak,
    "vm-sniper": vm_sniper,
    "sniper": prop_sniper,
    "vm-shotgun": vm_shotgun,
    "shotgun": prop_shotgun,
    "vm-smg": vm_smg,
    "smg": prop_smg,
    "vm-mg": vm_mg,
    "mg": prop_mg,
}


def credits():
    """assets/models/CREDITS.md: who made what the on-foot models are built from, and under which licence."""
    uses = {
        "rifle": "vm-rifle.glb (arms, M4 and clips), m4.glb",
        "pistol": "vm-pistol.glb (arms, pistol and clips), vm-hands.glb, pistol.glb",
        "knife": "knife.glb",
        "knife-fp-b": "vm-knife.glb (arms, knife and clips), vm-grenade.glb (arms, idle and draw; its throw clips made here)",
        "grenade": "grenade.glb (repainted olive)",
        "soldier-blue": "soldier-blue.glb (decimated, joined, maps baked into one atlas, retargeted clips, rifle hold)",
        "soldier-red": "soldier-red.glb (joined, maps baked into one atlas, retargeted clips, rifle hold)",
        "anims": "the soldiers' and the hostage's clips (retargeted onto their skeletons)",
        "ak": "vm-ak.glb (arms, AK-74M and clips), ak.glb; vm-mg.glb (arms and clips)",
        "sniper": "vm-sniper.glb (arms and clips; its own rifle replaced)",
        "sniper-rifle": "vm-sniper.glb (the rifle, on the sniper pack's rig), sniper.glb",
        "shotgun": "vm-shotgun.glb (arms, Remington 870 and clips), shotgun.glb",
        "smg": "vm-smg.glb (arms, SMG and clips), smg.glb",
        "mg": "vm-mg.glb (the LMG, on the AK pack's rig), mg.glb",
        "hostage": "hostage.glb (decimated, retargeted clips)",
    }
    import guns
    uses.update(guns.uses())
    lines = [
        "# On-foot models: credits",
        "",
        "Built by `tools/models/build.py` (first by Nexus Sim's `tools/infantry/build.py`) from these sources (the models",
        "were changed: cut, re-posed, decimated, retextured, re-animated, parts swapped between packs). Each is used under",
        "the licence shown.",
        "",
        "| Source | Author | Licence | Used in |",
        "|---|---|---|---|",
    ]
    for key, src in SOURCES.items():
        if src.get("source") == "polyhaven":
            continue
        lines.append(f"| [{src['title']}]({src['url']}) | {src['author']} | {src['license']} | {uses.get(key, '')} |")
    lines += [
        "",
        "## Environment props",
        "",
        "`props/*.glb`, built by `tools/models/build.py props` from Poly Haven models (cut to one piece, decimated,",
        "maps shrunk or baked into one atlas, a lightmap UV added). Poly Haven's assets are CC0: no attribution is",
        "required, but their authors are named here all the same.",
        "",
        "| Prop | Source | Author | Licence |",
        "|---|---|---|---|",
    ]
    for name, spec in PROPS.items():
        src = SOURCES[spec.get("src", name)]
        note = f" ({spec['note']})" if spec.get("note") else ""
        lines.append(f"| {name}.glb{note} | [{src['title']}]({src['url']}) | {src['author']} | {src['license']} |")
    lines += [
        "",
        "## Modelled here (arch.py)",
        "",
        "The facade pieces (`win_*`, `door_town_*`, `shop_town_*`, `awning_*`, `balcony_iron`), the street furniture",
        "(`cafe_set*`, `market_stall_*`, `planter_stone`, `sign_bakery`), `container40_*`/`container20_*`, `fuel_tank_green`,",
        "`crate_pine`/`crate_old`, `sandbags_1m` and the `truck_*` semis are modelled by `tools/models/arch.py` and shaded with",
        "the Poly Haven and ambientCG CC0 textures listed in `assets/textures/CREDITS.md`. The vehicles `veh_tornado`,",
        "`veh_nh90`, `veh_leopard`, `veh_patriot` and `veh_f124` are this workspace's own Nexus Sim models",
        "(`apps/bevy-spike/bevy/assets/models`), decimated and rebaked by arch.py.",
    ]
    (OUT / "CREDITS.md").write_text("\n".join(lines) + "\n")


def main():
    # `props` builds all the environment props, a prop's name just that one; they aren't in the default set.
    args = sys.argv[1:]
    # Every gun's own model (guns.py): prep-, measure-, vm- and prop recipes.
    import guns
    RECIPES.update(guns.recipes(sys.modules[__name__]))
    wanted_props = list(PROPS) if "props" in args else [a for a in args if a in PROPS]
    wanted = [a for a in args if a in RECIPES] or ([] if wanted_props else list(RECIPES))
    # `--clips idle,walk`: only those clips on the soldiers (for trying a change quickly).
    only = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--clips=")), None)
    for name in wanted:
        print(f"== {name}")
        if only and (name.startswith("soldier") or name == "hostage"):
            RECIPES[name]({k: v for k, v in SOLDIER_CLIPS.items() if k in only.split(",")})
        else:
            RECIPES[name]()
    if wanted_props:
        props(wanted_props)
    credits()


if __name__ == "__main__":
    main()
