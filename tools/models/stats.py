"""Prints what a GLB holds (for checking the characters against the game's budget): meshes, primitives, materials,
skins' joints, animations, the named sockets and the bones they hang from, triangles, images, file size.

    python3 tools/models/stats.py assets/models/soldier-blue.glb [...]

(Plain Python, no Blender: it reads the GLB's JSON chunk.)
"""

import json
import struct
import sys
from pathlib import Path

SOCKETS = ("weapon-socket", "hand-socket")


def gltf(path: Path) -> dict:
    data = path.read_bytes()
    magic, _version, _length = struct.unpack_from("<III", data, 0)
    assert magic == 0x46546C67, f"{path}: not a GLB"
    n, kind = struct.unpack_from("<II", data, 12)
    assert kind == 0x4E4F534A
    return json.loads(data[20:20 + n])


def stats(path: Path) -> dict:
    g = gltf(path)
    nodes = g.get("nodes", [])
    parent = {c: i for i, n in enumerate(nodes) for c in n.get("children", [])}
    prims = [p for m in g.get("meshes", []) for p in m["primitives"]]
    acc = g.get("accessors", [])
    tris = 0
    for p in prims:
        if "indices" in p:
            tris += acc[p["indices"]]["count"] // 3
        else:
            tris += acc[p["attributes"]["POSITION"]]["count"] // 3
    skinned = [i for i, n in enumerate(nodes) if "skin" in n and "mesh" in n]
    sockets = {}
    for i, n in enumerate(nodes):
        if n.get("name") in SOCKETS:
            p = parent.get(i)
            sockets[n["name"]] = nodes[p].get("name") if p is not None else None
    images = []
    for im in g.get("images", []):
        view = g["bufferViews"][im["bufferView"]] if "bufferView" in im else None
        images.append((im.get("name"), im.get("mimeType"), view["byteLength"] if view else None))
    return {
        "file": path.name,
        "MB": round(path.stat().st_size / 1e6, 2),
        "meshes": len(g.get("meshes", [])),
        "primitives": len(prims),
        "skinned mesh nodes": len(skinned),
        "materials": len(g.get("materials", [])),
        "joints": [len(s["joints"]) for s in g.get("skins", [])],
        "tris": tris,
        "animations": [a.get("name") for a in g.get("animations", [])],
        "sockets (parent bone)": sockets,
        "chest bones": [n.get("name") for n in nodes if "Spine02" in n.get("name", "") or "Spine2_" in n.get("name", "")],
        "images": images,
    }


if __name__ == "__main__":
    for arg in sys.argv[1:]:
        s = stats(Path(arg))
        print(f"== {s.pop('file')}")
        for k, v in s.items():
            print(f"  {k}: {v}")
