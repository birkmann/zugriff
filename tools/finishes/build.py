"""Paints the weapon finishes (cosmetic paint schemes) into assets/finishes/<name>.png:

    ~/.local/bin/uv run --with numpy --with pillow python tools/finishes/build.py [name ...]

The game (game/src/finish.rs) lays them over a weapon through its `finish` UV map: a weapon seen from its side, in
metres, u from the back end forward and v from the bottom up (see tools/models/build.py `finish_uv`). Two kinds:

- tiles (square, seamless): a pattern repeating every so many metres (the catalogue in finish.rs says how many),
  the same density on a pistol as on a machine gun: camouflage, carbon, brushed metal;
- wraps (4:1): one design stretched over the whole weapon, back end at the left, muzzle at the right, bottom to top.

All patterns are made here, from noise and geometry; nothing is copied from any game's skins. The alpha channel is
the paint's coverage (a wrap may leave the bare weapon showing through).
"""

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parents[2] / "assets" / "finishes"
TILE = 512
WRAP = (2048, 512)


def rng(seed):
    return np.random.default_rng(seed)


def periodic_noise(n, scale, seed, aspect=(1.0, 1.0)):
    """Seamless smooth noise on an n×n torus: white noise low-passed in the frequency domain (so it wraps), about
    `scale` features across; `aspect` stretches it (x, y). Normalised to 0..1."""
    r = rng(seed)
    white = r.standard_normal((n, n))
    fy = np.fft.fftfreq(n)[:, None] * n / aspect[1]
    fx = np.fft.fftfreq(n)[None, :] * n / aspect[0]
    f = np.sqrt(fx * fx + fy * fy)
    spec = np.fft.fft2(white) * np.exp(-((f / scale) ** 2))
    out = np.real(np.fft.ifft2(spec))
    out -= out.min()
    return out / max(out.max(), 1e-9)


def rgb(h):
    h = h.lstrip("#")
    return np.array([int(h[i:i + 2], 16) for i in (0, 2, 4)], dtype=np.float32)


def layers(base, spots, n=TILE):
    """A camouflage: the base colour, then each (colour, mask) painted over it in order."""
    img = np.zeros((n, n, 3), np.float32) + rgb(base)
    for col, mask in spots:
        img[mask] = rgb(col)
    return img


def save(name, img, alpha=None):
    OUT.mkdir(parents=True, exist_ok=True)
    a = np.clip(img, 0, 255).astype(np.uint8)
    if alpha is None:
        Image.fromarray(a, "RGB").save(OUT / f"{name}.png", optimize=True)
    else:
        al = (np.clip(alpha, 0, 1) * 255).astype(np.uint8)
        Image.fromarray(np.dstack([a, al]), "RGBA").save(OUT / f"{name}.png", optimize=True)
    print(f"{name}.png {(OUT / f'{name}.png').stat().st_size / 1e3:.0f} kB")


def grain(img, amount, seed):
    """A little paint grain, so flat colour doesn't look printed."""
    g = rng(seed).standard_normal(img.shape[:2])[..., None] * amount
    return img + g


# ---- tiles ------------------------------------------------------------------------------------------------------


def flecktarn():
    """Five-colour spot camouflage: clusters of small, blotchy dots, darker colours in smaller clusters."""
    n = TILE
    spots = []
    for i, (col, cl, dot, th) in enumerate([("#4f3d2c", 6, 42, 0.58), ("#27301f", 7, 46, 0.6), ("#1a1a16", 8, 54, 0.66)]):
        clusters = periodic_noise(n, cl, 10 + i)
        dots = periodic_noise(n, dot, 20 + i)
        spots.append((col, (clusters * 0.62 + dots * 0.38) > th))
    light = periodic_noise(n, 5, 30) * 0.6 + periodic_noise(n, 40, 31) * 0.4 > 0.56
    img = layers("#687148", [("#4c5732", light)] + spots)
    save("flecktarn", grain(img, 3.0, 1))


def tropentarn():
    """Three-colour desert spot camouflage: sand ground, sparse olive-brown and brown dots."""
    n = TILE
    a = periodic_noise(n, 6, 40) * 0.6 + periodic_noise(n, 44, 41) * 0.4 > 0.57
    b = periodic_noise(n, 7, 42) * 0.6 + periodic_noise(n, 50, 43) * 0.4 > 0.62
    img = layers("#a8966f", [("#7b7050", a), ("#5c4632", b)])
    save("tropentarn", grain(img, 3.0, 2))


def schneetarn():
    """Winter: white with soft grey drifts and a few dark twig-like flecks."""
    n = TILE
    drift = periodic_noise(n, 4, 50, aspect=(2.0, 1.0))
    img = np.zeros((n, n, 3), np.float32) + rgb("#e9eae8")
    img = img * (1 - 0.18 * (drift > 0.55)[..., None]) + rgb("#aeb3b6") * 0.18 * (drift > 0.55)[..., None]
    grey = periodic_noise(n, 9, 51) * 0.6 + periodic_noise(n, 40, 52) * 0.4 > 0.6
    img[grey] = rgb("#9ca2a6")
    dark = periodic_noise(n, 12, 53) * 0.5 + periodic_noise(n, 70, 54) * 0.5 > 0.7
    img[dark] = rgb("#5d6468")
    save("schneetarn", grain(img, 2.5, 3))


def stadttarn():
    """Urban digital: square pixels in greys and a slate blue, in clusters of several sizes."""
    n = TILE
    cell = 16
    m = n // cell
    base = periodic_noise(m, 6, 60)
    fine = periodic_noise(m, 20, 61)
    v = base * 0.7 + fine * 0.3
    pal = [rgb("#c9cbcc"), rgb("#8f9497"), rgb("#5a6268"), rgb("#3c4957"), rgb("#1f2327")]
    idx = np.clip((v - v.min()) / (np.ptp(v) + 1e-9) * len(pal), 0, len(pal) - 1e-3).astype(int)
    small = np.stack([pal[i] for i in idx.ravel()]).reshape(m, m, 3)
    img = np.kron(small, np.ones((cell, cell, 1)))
    save("stadttarn", grain(img, 2.0, 4))


def tigerfell():
    """Tiger stripes: black bands across the weapon (along v), wavy and tapering, on a burnt orange."""
    n = TILE
    y, x = np.mgrid[0:n, 0:n] / n
    warp = periodic_noise(n, 3, 70) - 0.5
    warp2 = periodic_noise(n, 8, 71) - 0.5
    # Bands run up and down the weapon: they vary along x (u, along the barrel).
    phase = x * 7 + warp * 1.6 + warp2 * 0.35
    band = np.abs(np.sin(phase * math.pi))
    width = 0.28 + 0.22 * periodic_noise(n, 4, 72)
    stripes = band < width
    img = np.zeros((n, n, 3), np.float32)
    tone = periodic_noise(n, 5, 73)[..., None]
    img[:] = rgb("#d9741c") * (0.85 + 0.25 * tone)
    img[stripes] = rgb("#15120f")
    edge = (band < width + 0.05) & ~stripes
    img[edge] = rgb("#7a3a10")
    save("tigerfell", grain(img, 3.5, 5))


def karbon():
    """Carbon fibre: a 2×2 twill of dark tows, each with a sheen across its fibres."""
    n = TILE
    y, x = np.mgrid[0:n, 0:n]
    tow = n // 16
    cx, cy = (x // tow), (y // tow)
    over = ((cx + cy) // 2) % 2 == 0
    fx = (x % tow) / tow
    fy = (y % tow) / tow
    sheen = np.where(over, np.sin(fy * math.pi), np.sin(fx * math.pi))
    fibre = np.where(over, np.sin(x * 2.7), np.sin(y * 2.7)) * 0.5 + 0.5
    v = 18 + 38 * sheen ** 2 + 8 * fibre
    img = np.dstack([v, v * 1.02, v * 1.08])
    save("karbon", img)


def brushed(name, col, seed, streak=1.0):
    """A metal (the finish sets it metallic): the colour with fine streaks along the barrel."""
    n = TILE
    s = periodic_noise(n, 90, seed, aspect=(0.08, 1.0))
    s2 = periodic_noise(n, 30, seed + 1, aspect=(0.2, 1.0))
    k = 1.0 + streak * (0.1 * (s - 0.5) + 0.06 * (s2 - 0.5))
    img = rgb(col) * k[..., None]
    save(name, img)


def kobalt():
    brushed("kobalt", "#2448a8", 80)


def gold():
    brushed("gold", "#e2b35a", 82)


def damast():
    """Damascus steel: folded layers, dark and bright bands warped into ripples."""
    n = TILE
    y, x = np.mgrid[0:n, 0:n] / n
    w1 = periodic_noise(n, 3, 90) - 0.5
    w2 = periodic_noise(n, 9, 91) - 0.5
    t = y * 10 + w1 * 3.0 + w2 * 0.8 + np.sin(x * 2 * math.pi * 2) * 0.35
    layer = np.sin(t * 2 * math.pi) * 0.5 + 0.5
    layer = layer ** 1.6
    v = 70 + 120 * layer
    img = np.dstack([v, v * 1.01, v * 1.04])
    save("damast", grain(img, 2.0, 6))


# ---- wraps ------------------------------------------------------------------------------------------------------


def uv_grid(w, h):
    y, x = np.mgrid[0:h, 0:w]
    return x / w, 1.0 - y / h  # u along, v up


def lerp_cols(t, stops):
    """Colour along `t` (0..1) through `stops` [(t, "#hex")]."""
    t = np.clip(t, 0, 1)
    out = np.zeros(t.shape + (3,), np.float32)
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        k = np.clip((t - t0) / max(t1 - t0, 1e-6), 0, 1)[..., None]
        seg = (t >= t0) & (t <= t1)
        out[seg] = (rgb(c0) * (1 - k) + rgb(c1) * k)[seg]
    return out


def neonpuls():
    """Neon pulse: a violet body fading to magenta and cyan, halftone dots growing toward the muzzle, a bright pulse
    line (a heartbeat) running along the weapon, thin glowing edge lines."""
    w, h = WRAP
    u, v = uv_grid(w, h)
    img = lerp_cols(u * 0.8 + v * 0.2, [(0, "#2a1450"), (0.35, "#5b1a8c"), (0.65, "#d0258f"), (1.0, "#27c7d8")])
    # Halftone: a dot grid whose dots grow with u (and a slow wave).
    pitch = 18
    yy, xx = np.mgrid[0:h, 0:w]
    gx = (xx % pitch) - pitch / 2
    gy = ((yy + (xx // pitch % 2) * pitch / 2) % pitch) - pitch / 2
    r = np.sqrt(gx * gx + gy * gy)
    size = (0.15 + 0.55 * np.clip(u * 1.3 - 0.2 + 0.15 * np.sin(v * 9), 0, 1)) * pitch / 2
    dots = r < size
    img[dots] = img[dots] * 0.55 + rgb("#150828") * 0.45
    pil = Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))
    glow = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(glow)
    # The pulse line: flat, a spike, flat — twice along the weapon.
    mid = h * 0.52
    pts = []
    for x in range(0, w + 1, 8):
        t = x / w
        yv = 0.0
        for c in (0.3, 0.72):
            dt = (t - c) * 40
            if -1.2 < dt < 1.8:
                yv += (-0.9 * math.exp(-((dt - 0.2) ** 2) * 8) + 0.35 * math.exp(-((dt + 0.6) ** 2) * 6) + 0.25 * math.exp(-((dt - 1.2) ** 2) * 5))
        pts.append((x, mid + yv * h * 0.33))
    d.line(pts, fill=(80, 255, 240), width=9, joint="curve")
    d.line([(0, h * 0.14), (w, h * 0.14)], fill=(255, 60, 200), width=4)
    d.line([(0, h * 0.88), (w, h * 0.88)], fill=(255, 60, 200), width=4)
    blur = glow.filter(ImageFilter.GaussianBlur(10))
    out = np.asarray(pil, np.float32) + np.asarray(blur, np.float32) * 0.9 + np.asarray(glow, np.float32) * 0.9
    save("neonpuls", out)


def glutlack():
    """Glowing lacquer: a racing livery. Deep red into orange along the weapon, a broad white band with black pin
    stripes slanting across the middle, chevrons toward the muzzle, a dark underside."""
    w, h = WRAP
    u, v = uv_grid(w, h)
    img = lerp_cols(u, [(0, "#8e1111"), (0.5, "#d4301a"), (1.0, "#f27a1a")])
    img *= (0.8 + 0.2 * v)[..., None]
    s = u - (v - 0.5) * 0.18  # slanted coordinate
    band = (s > 0.42) & (s < 0.55)
    img[band] = rgb("#f2efe8")
    for c in (0.405, 0.565, 0.58):
        img[np.abs(s - c) < 0.0035] = rgb("#111111")
    # Chevrons toward the muzzle.
    for k in range(4):
        c = 0.74 + k * 0.045
        chev = np.abs((u - c) + np.abs(v - 0.5) * 0.12) < 0.008
        img[chev & (v > 0.25) & (v < 0.75)] = rgb("#f7e7c9")
    img[v < 0.16] = img[v < 0.16] * 0.25 + rgb("#1a1a1a") * 0.75
    save("glutlack", img)


def graffiti():
    """Street tags: loops and slashes of black spray on off-white, a few red strokes, with drips; tiles every
    0.5 m so it reads the same on every weapon."""
    n = TILE * 2
    r = rng(99)
    pil = Image.new("RGB", (n, n), (232, 229, 220))
    d = ImageDraw.Draw(pil)

    def stroke(col, width, pts):
        # Drawn three times over the tile's edges so it wraps.
        for ox in (-n, 0, n):
            for oy in (-n, 0, n):
                d.line([(x + ox, y + oy) for x, y in pts], fill=col, width=width, joint="curve")

    for i in range(26):
        x, y = r.uniform(0, n, 2)
        ang = r.uniform(0, 2 * math.pi)
        pts = []
        for t in range(40):
            ang += r.normal(0, 0.35) + (0.25 if i % 3 == 0 else 0)
            x += math.cos(ang) * 14
            y += math.sin(ang) * 14
            pts.append((x, y))
        col = (170, 20, 24) if i % 7 == 0 else (18, 18, 20)
        stroke(col, int(r.uniform(12, 26)), pts)
        if r.uniform() < 0.5:
            dx, dy = pts[-1]
            stroke(col, 6, [(dx, dy), (dx, dy + r.uniform(20, 70))])
    img = np.asarray(pil.filter(ImageFilter.GaussianBlur(1.2)), np.float32)
    speck = rng(98).uniform(size=img.shape[:2]) > 0.996
    img[speck] = (20, 20, 20)
    save("graffiti", img)


def polarlicht():
    """Northern lights: a night-blue body with green and violet curtains rising from the bottom, a few stars."""
    w, h = WRAP
    u, v = uv_grid(w, h)
    img = lerp_cols(v, [(0, "#040a18"), (1, "#0b1f3a")])
    n1 = np.asarray(Image.fromarray((periodic_noise(512, 6, 120) * 255).astype(np.uint8)).resize((w, h)), np.float32) / 255
    curtain = np.sin(u * 23 + n1 * 6) * 0.5 + 0.5
    height = 0.35 + 0.4 * n1
    fall = np.clip(1 - np.abs(v - height) / 0.3, 0, 1) ** 2
    green = fall * curtain ** 3
    violet = np.clip(1 - np.abs(v - height - 0.22) / 0.2, 0, 1) ** 2 * (1 - curtain) ** 2
    img += green[..., None] * rgb("#2cf29a") * 0.9 + violet[..., None] * rgb("#a24cf0") * 0.7
    stars = rng(121).uniform(size=(h, w)) > 0.9985
    img[stars & (v > 0.5)] = (230, 240, 255)
    save("polarlicht", img)


FINISHES = {
    "flecktarn": flecktarn, "tropentarn": tropentarn, "schneetarn": schneetarn, "stadttarn": stadttarn,
    "tigerfell": tigerfell, "karbon": karbon, "kobalt": kobalt, "gold": gold, "damast": damast,
    "neonpuls": neonpuls, "glutlack": glutlack, "graffiti": graffiti, "polarlicht": polarlicht,
}

if __name__ == "__main__":
    for name in sys.argv[1:] or list(FINISHES):
        FINISHES[name]()
