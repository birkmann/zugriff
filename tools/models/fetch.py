"""Downloads the ZUGRIFF models' sources (sources.json) into .cache/: the Sketchfab models through its API (a token in
SKETCHFAB_TOKEN or SKETCHFAB_API_KEY, or the workspace's .env), the Quaternius animation library from itch.io, the
environment props from Poly Haven (`"source": "polyhaven"`: the glTF with its 1k maps, no token needed; a `variant`
also fetches that texture set's 1k maps, e.g. a barrel's green paint instead of the default; foliage's and grilles'
alpha maps come along).

    python3 tools/models/fetch.py [name ...]     # all sources, or just these
"""

import http.cookiejar
import json
import os
import re
import sys
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
CACHE = HERE / ".cache"


def token() -> str:
    for k in ("SKETCHFAB_TOKEN", "SKETCHFAB_API_KEY"):
        if os.environ.get(k):
            return os.environ[k]
    env = HERE.parents[3] / ".env"
    if env.exists():
        for line in env.read_text().splitlines():
            m = re.match(r"\s*SKETCHFAB_(?:TOKEN|API_KEY)\s*=\s*['\"]?([^'\"\s]+)", line)
            if m:
                return m.group(1)
    sys.exit("no Sketchfab token: set SKETCHFAB_TOKEN")


def get(url: str, headers=None, data=None, opener=None) -> bytes:
    req = urllib.request.Request(url, data=data, headers={"User-Agent": "zugriff-models-fetch", **(headers or {})})
    return (opener or urllib.request.build_opener()).open(req, timeout=300).read()


def unzip(blob: bytes, into: Path):
    tmp = into.with_suffix(".zip")
    tmp.write_bytes(blob)
    with zipfile.ZipFile(tmp) as z:
        z.extractall(into)
    tmp.unlink()


def sketchfab(name: str, src: dict, key: str):
    into = CACHE / name
    if into.exists():
        print(f"{name}: cached")
        return
    info = json.loads(get(f"https://api.sketchfab.com/v3/models/{src['uid']}/download", {"Authorization": f"Token {key}"}))
    gltf = info.get("gltf") or {}
    print(f"{name}: {gltf.get('size', 0) / 1e6:.1f} MB")
    unzip(get(gltf["url"]), into)


def itch(name: str, src: dict):
    into = CACHE / name
    if into.exists():
        print(f"{name}: cached")
        return
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    page = get(src["itch"], opener=opener).decode()
    csrf = re.search(r'name="csrf_token" value="([^"]+)"', page).group(1)
    body = urllib.parse.urlencode({"csrf_token": csrf}).encode()
    dl = json.loads(get(src["itch"] + "/download_url", data=body, opener=opener))["url"]
    page = get(dl, opener=opener).decode()
    uploads = re.findall(r'data-upload_id="(\d+)".*?title="([^"]+)"', page, re.S)
    upload = next(u for u, f in uploads if f == src["file"])
    file = json.loads(get(f"{src['itch']}/file/{upload}?source=game_download", data=body, opener=opener))["url"]
    print(f"{name}: {src['file']}")
    unzip(get(file), into)


POLYHAVEN_RES = "1k"


def polyhaven(name: str, src: dict):
    """A Poly Haven model: the .gltf at 1k, its .bin and the textures it lists under "include" (into textures/, where
    the .gltf looks for them); with `variant`, that texture set's maps as well (diff, nor_gl, arm)."""
    into = CACHE / name
    done = into / ".complete"
    if done.exists():
        print(f"{name}: cached")
        return
    files = json.loads(get(f"https://api.polyhaven.com/files/{src['id']}"))
    entry = files["gltf"][POLYHAVEN_RES]["gltf"]
    into.mkdir(parents=True, exist_ok=True)
    total = 0
    blob = get(entry["url"])
    (into / f"{src['id']}.gltf").write_bytes(blob)
    total += len(blob)
    wanted = dict(entry.get("include", {}))
    # The 1k glTF's colour maps are JPEGs, without the leaves' (or a grille's) alpha: that comes as a map of its own.
    for kind, res in files.items():
        f = res.get(POLYHAVEN_RES, {}).get("jpg") if isinstance(res, dict) else None
        if f and re.search(r"(alpha|opacity|mask)$", kind, re.I):
            wanted[f"textures/{f['url'].rsplit('/', 1)[1]}"] = f
    for v in [src["variant"]] if src.get("variant") else []:
        for kind in ("diff", "nor_gl", "arm"):
            f = files.get(f"{v}_{kind}", {}).get(POLYHAVEN_RES, {}).get("jpg")
            if f:
                wanted[f"textures/{f['url'].rsplit('/', 1)[1]}"] = f
    for rel, f in wanted.items():
        out = into / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        blob = get(f["url"])
        out.write_bytes(blob)
        total += len(blob)
    done.write_text("")
    print(f"{name}: {total / 1e6:.1f} MB ({len(wanted) + 1} files)")


def main():
    CACHE.mkdir(exist_ok=True)
    sources = json.loads((HERE / "sources.json").read_text())
    only = sys.argv[1:]
    key = None
    for name, src in sources.items():
        if only and name not in only:
            continue
        if src.get("source") == "polyhaven":
            polyhaven(name, src)
        elif "uid" in src:
            key = key or token()
            sketchfab(name, src, key)
        else:
            itch(name, src)


if __name__ == "__main__":
    main()
