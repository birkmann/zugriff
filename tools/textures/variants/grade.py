"""Grade colour maps for the new ZUGRIFF sets and write the job list (set, color png, normal, ao, rough, metal)."""
import json, os, sys
import numpy as np
from PIL import Image
C = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".cache") + ""
G = C + "/graded"
os.makedirs(G, exist_ok=True)

def ph(pid, kind):
    d = f"{C}/ph/{pid}"
    pats = {"col": ["_diff_2k", "_Diffuse_2k"], "nor": ["_nor_gl_2k"], "ao": ["_ao_2k", "_AO_2k"],
            "rough": ["_rough_2k", "_Rough_2k"], "metal": ["_metal_2k", "_Metal_2k"]}[kind]
    for p in pats:
        f = f"{d}/{pid}{p}.jpg"
        if os.path.exists(f):
            return f
    return "-"

def acg(aid, kind):
    k = {"col": "Color", "nor": "NormalGL", "ao": "AmbientOcclusion", "rough": "Roughness", "metal": "Metalness"}[kind]
    f = f"{C}/acg/{aid}/{aid}_2K-JPG_{k}.jpg"
    return f if os.path.exists(f) else "-"

def mean(a):
    im = Image.fromarray(np.clip(a, 0, 255).round().astype(np.uint8)).resize((64, 64), Image.BOX)
    return np.asarray(im, dtype=np.float64).reshape(-1, 3).mean(0)

def flatten(a, sigma_frac, strength):
    """Removes large-scale shading: divides by a wrap-around Gaussian blur of the luma (keeps tiling)."""
    h, w, _ = a.shape
    luma = (a * [0.2126, 0.7152, 0.0722]).sum(-1)
    fy = np.fft.fftfreq(h)[:, None]; fx = np.fft.fftfreq(w)[None, :]
    s = sigma_frac * h
    g = np.exp(-2 * (np.pi * s) ** 2 * (fx ** 2 + fy ** 2))
    blur = np.real(np.fft.ifft2(np.fft.fft2(luma) * g))
    return a * ((luma.mean() / np.maximum(blur, 1)) ** strength)[..., None]

def grade(src, target=None, desat=0.0, contrast=1.0, flat=None):
    a = np.asarray(Image.open(src).convert("RGB"), dtype=np.float64)
    src_mean = mean(a)
    if flat:
        a = flatten(a, *flat)
    luma = (a * [0.2126, 0.7152, 0.0722]).sum(-1, keepdims=True)
    a = luma + (a - luma) * (1 - desat)
    if contrast != 1.0:
        m = a.reshape(-1, 3).mean(0)
        a = m + (a - m) * contrast
    if target is not None:
        t = np.array(target, dtype=np.float64)
        for _ in range(6):
            a = a * (t / mean(a))
            a = np.clip(a, 0, 255)
    return np.clip(a, 0, 255).round().astype(np.uint8), src_mean

# set: (source fn, id, target mean, desat, contrast, metal override)
SETS = {
    "concrete_wall_b": (ph, "concrete_wall_008", (126, 125, 121), 0.8, 1.0, None),
    "concrete_wall_c": (ph, "concrete_wall_006", (116, 113, 107), 0.7, 1.0, None),
    "plaster_b": (ph, "beige_wall_001", (184, 174, 154), 0.3, 1.0, None, (0.02, 0.9)),
    "plaster_c": (ph, "grey_plaster_02", (168, 167, 162), 0.75, 1.0, None),
    "brick_b": (ph, "medieval_red_brick", None, 0.0, 1.0, None),
    "asphalt_b": (ph, "asphalt_02", (70, 70, 68), 0.5, 1.0, None),
    "concrete_slabs": (ph, "large_square_pattern_01", (125, 125, 121), 0.6, 1.0, None),
    "paving_b": (ph, "rectangular_paving", (128, 128, 125), 0.8, 1.0, None),
    "metal_corrugated_b": (ph, "corrugated_iron", (104, 114, 102), 1.0, 1.4, "-"),
    "planks_b": (ph, "wood_planks_grey", (88, 84, 78), 0.0, 1.0, None),
    "roof_b": (ph, "grey_roof_tiles_02", (80, 82, 86), 0.6, 1.0, None),
    "tiles_floor_b": (ph, "floor_tiles_08", (128, 128, 125), 0.85, 1.0, None),
    "dirt_b": (ph, "rocky_trail", (136, 120, 100), 0.0, 1.0, None),
    "window_frame": (acg, "Plastic010", (192, 192, 189), 0.8, 1.0, None),
    "metal_dark": (acg, "Metal028", (60, 62, 64), 1.0, 1.0, "-"),
    "rubber": (acg, "Rubber004", (40, 40, 40), 1.0, 1.0, None),
    "door_steel": (ph, "blue_metal_plate", (96, 106, 96), 1.0, 1.0, None),
    "rollershutter": (ph, "painted_metal_shutter", (130, 131, 132), 0.9, 1.0, None),
    "sign_plate": (acg, "PaintedMetal012", (196, 192, 182), 0.3, 0.5, None),
}
only = sys.argv[1:]
jobs = json.load(open(G + "/../jobs.json")) if os.path.exists(G + "/../jobs.json") else {}
for name, spec in SETS.items():
    fn, sid, target, desat, contrast, metal = spec[:6]
    flat = spec[6] if len(spec) > 6 else None
    if only and name not in only:
        continue
    img, sm = grade(fn(sid, "col"), target, desat, contrast, flat)
    out = f"{G}/{name}_color.png"
    Image.fromarray(img).save(out)
    m = mean(img.astype(np.float64))
    jobs[name] = {"color": out, "normal": fn(sid, "nor"), "ao": fn(sid, "ao"), "rough": fn(sid, "rough"),
                  "metal": metal if metal is not None else fn(sid, "metal"), "src_mean": [int(round(x)) for x in sm],
                  "mean": [int(round(x)) for x in m]}
    print(name, sid, "src", jobs[name]["src_mean"], "->", jobs[name]["mean"], "metal", jobs[name]["metal"].split("/")[-1])
json.dump(jobs, open(G + "/../jobs.json", "w"), indent=1)
