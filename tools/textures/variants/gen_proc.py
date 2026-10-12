"""Procedural trim sets: glass_window, lamp_glass, vent_grille. All tile seamlessly (FFT noise, integer periods)."""
import json, os
import numpy as np
from PIL import Image
C = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".cache") + ""
G = C + "/graded"
N = 1024
rng = np.random.default_rng(1234)
fy = np.fft.fftfreq(N)[:, None] * N
fx = np.fft.fftfreq(N)[None, :] * N

def noise(beta, fmax=None, sx=1.0, sy=1.0, seed=None):
    r = np.random.default_rng(seed) if seed is not None else rng
    w = r.standard_normal((N, N))
    f = np.sqrt((fx * sx) ** 2 + (fy * sy) ** 2)
    f[0, 0] = 1
    filt = f ** (-beta / 2 * 2)
    if fmax:
        filt *= np.exp(-(f / fmax) ** 2)
    filt[0, 0] = 0
    n = np.real(np.fft.ifft2(np.fft.fft2(w) * filt))
    return (n - n.mean()) / n.std()

def save(a, name):
    p = f"{G}/{name}.png"
    Image.fromarray(np.clip(a, 0, 255).round().astype(np.uint8)).save(p)
    return p

def mean(a):
    im = Image.fromarray(np.clip(a, 0, 255).round().astype(np.uint8)).resize((64, 64), Image.BOX)
    return [int(round(x)) for x in np.asarray(im, dtype=np.float64).reshape(-1, 3).mean(0)]

jobs = json.load(open(C + "/jobs.json"))
v = (np.arange(N)[:, None] + 0.5) / N * np.ones((1, N))
u = (np.arange(N)[None, :] + 0.5) / N * np.ones((N, 1))
flat_n = save(np.stack([np.full((N, N), 128.0), np.full((N, N), 128.0), np.full((N, N), 255.0)], -1), "flat_normal")

# --- glass_window: dark blue-grey pane, soft sky reflection, streaks and dirt.
base = np.array([34, 43, 53], dtype=np.float64)
sky = np.array([118, 136, 156], dtype=np.float64)
grad = 0.5 + 0.5 * np.cos(2 * np.pi * (v - 0.08))           # brighter near the top, wraps
clouds = noise(1.6, fmax=10, seed=1)                          # low-frequency reflection variation
diag = 0.5 + 0.5 * np.cos(2 * np.pi * (2 * u + 1 * v) + 0.6 * clouds)
refl = np.clip(0.14 + 0.36 * grad + 0.05 * clouds + 0.08 * (diag ** 6), 0, 1)
col = base + (sky - base) * refl[..., None]
streak = noise(1.0, fmax=90, sx=1.0, sy=12.0, seed=2)        # vertical rain streaks
streak = np.clip((streak - 1.0) / 2.0, 0, 1) * (0.4 + 0.6 * (1 - grad))
dirt = noise(1.2, fmax=60, seed=3)
dirt = np.clip((dirt - 0.6) / 2.5, 0, 1)
grime = np.clip(0.6 * streak + 0.3 * dirt, 0, 0.5)
dirt_col = np.array([84, 84, 82], dtype=np.float64)
col = col + (dirt_col - col) * grime[..., None]
col += 1.5 * noise(0.0, fmax=300, seed=4)[..., None]           # very fine speckle
rough = np.clip(20 + grime * 110 + 2 * noise(0.5, fmax=200, seed=5), 12, 120)
jobs["glass_window"] = {"color": save(col, "glass_window_color"), "normal": flat_n, "ao": "-",
                        "rough": save(np.repeat(rough[..., None], 3, -1), "glass_window_rough"), "metal": "-", "mean": mean(col)}

# --- lamp_glass: warm-white frosted plastic, near-uniform.
col = np.array([226, 222, 208], dtype=np.float64) + 2.0 * noise(0.8, fmax=200, seed=6)[..., None]
rough = 77 + 4 * noise(0.8, fmax=200, seed=7)
jobs["lamp_glass"] = {"color": save(np.broadcast_to(col, (N, N, 3)), "lamp_glass_color"), "normal": flat_n, "ao": "-",
                      "rough": save(np.repeat(rough[..., None], 3, -1), "lamp_glass_rough"), "metal": "-",
                      "mean": mean(np.broadcast_to(col, (N, N, 3)))}

# --- vent_grille: 16 horizontal louvre blades per repeat, dark painted steel.
SL = 16
p = (v * SL) % 1.0                                             # 0 at the top of each period
blade_end, edge = 0.80, 0.03
theta = np.radians(38)
# Blade face tilts down-and-out: normal points to -V (image down) in GL convention -> green below 128.
face = p < blade_end
ny = np.where(face, -np.sin(theta), 0.0)
nz = np.where(face, np.cos(theta), 1.0)
# Rounded lip at the lower edge of the blade: normal swings towards -V strongly.
lip = np.clip(1 - np.abs(p - blade_end) / edge, 0, 1) * face
ny = ny * (1 - lip) + (-0.9) * lip
nz = np.sqrt(np.clip(1 - ny ** 2, 0, 1))
# Rounded top edge of each blade (just below the gap), facing +V.
top = np.clip(1 - p / edge, 0, 1)
ny = ny * (1 - top) + 0.7 * top
nz = np.sqrt(np.clip(1 - ny ** 2, 0, 1))
grain_src = Image.open(f"{C}/acg/Metal028/Metal028_2K-JPG_NormalGL.jpg").convert("RGB").resize((N, N), Image.LANCZOS)
g = np.asarray(grain_src, dtype=np.float64) / 127.5 - 1
nx = g[..., 0] * 0.5
ny = ny + g[..., 1] * 0.5
l = np.sqrt(nx ** 2 + ny ** 2 + nz ** 2)
nor = np.stack([(nx / l + 1) * 127.5, (ny / l + 1) * 127.5, (nz / l + 1) * 127.5], -1)
# AO: gap is deep shadow, top of each blade is under the blade above.
ao = np.where(face, 0.55 + 0.45 * np.clip(p / 0.45, 0, 1), 0.12)
ao = np.where(face, ao, 0.12 + 0.2 * np.clip((p - blade_end) / (1 - blade_end), 0, 1) * 0)
ao_img = np.clip(ao * 255, 0, 255)
paint = np.array([74, 76, 78], dtype=np.float64)
tone = np.asarray(Image.open(f"{C}/acg/Metal028/Metal028_2K-JPG_Color.jpg").convert("L").resize((N, N), Image.LANCZOS), dtype=np.float64)
tone = 1 + (tone - tone.mean()) / 255 * 3.0
tone *= 1 + 0.06 * noise(1.5, fmax=30, seed=8)
shade = np.where(face, 0.78 + 0.22 * np.clip(p / 0.5, 0, 1), 0.22)     # the gap reads dark in the albedo too
col = paint[None, None, :] * (tone * shade)[..., None]
rough = np.where(face, 140, 200) + 10 * noise(1.0, fmax=100, seed=9)
jobs["vent_grille"] = {"color": save(col, "vent_grille_color"), "normal": save(nor, "vent_grille_normal"),
                       "ao": save(np.repeat(ao_img[..., None], 3, -1), "vent_grille_ao"),
                       "rough": save(np.repeat(rough[..., None], 3, -1), "vent_grille_rough"), "metal": "-", "mean": mean(col)}
for k in ["glass_window", "lamp_glass", "vent_grille"]:
    print(k, jobs[k]["mean"])
json.dump(jobs, open(C + "/jobs.json", "w"), indent=1)
