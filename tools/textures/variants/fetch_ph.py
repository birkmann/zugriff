import sys, json, os, urllib.request
UA = "Mozilla/5.0 (X11; Linux x86_64) zugriff-texture-fetch/1.0 (CC0 asset pipeline)"
C = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".cache") + "/ph"
def get(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=120).read()
for pid in sys.argv[1:]:
    d = os.path.join(C, pid); os.makedirs(d, exist_ok=True)
    files = json.loads(get(f"https://api.polyhaven.com/files/{pid}"))
    info = json.loads(get(f"https://api.polyhaven.com/info/{pid}"))
    json.dump({"authors": info.get("authors"), "dimensions": info.get("dimensions"), "name": info.get("name")}, open(os.path.join(d, "info.json"), "w"))
    for key in ["Diffuse", "nor_gl", "Rough", "AO", "Metal", "Displacement"]:
        if key not in files: continue
        if key == "Displacement": continue
        e = files[key]["2k"]["jpg"]; url = e["url"]; fn = os.path.join(d, os.path.basename(url))
        if not os.path.exists(fn):
            open(fn, "wb").write(get(url))
    print(pid, sorted(k for k in files if k not in ("blend","gltf","mtlx")), info.get("dimensions"), list(info.get("authors",{}).keys()))
