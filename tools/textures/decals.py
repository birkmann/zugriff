#!/usr/bin/env python3
"""ZUGRIFF decal atlas: builds assets/textures/decals/{color,normal}.ktx2 and decals.json.

    python3 tools/textures/decals.py [--preview DIR] [--no-ktx]

Sources:
- ambientCG CC0 decals and materials (leaks, floor damage, manhole, tyre tread, boot prints, moss, smears),
  downloaded once into tools/textures/.cache/decals/.
- Procedural decals and all signage drawn here with Pillow and numpy: cracks, oil, puddle, skid mark, German signs,
  stencils, floor markings, posters. Fonts: Barlow Condensed and Inter from assets/fonts (SIL OFL 1.1).

Layout: a 2048² atlas on a 128 px grid. Every decal owns a cell whose size is a multiple of 128 px and whose origin is on
that grid; its content sits inside with at least 16 px of transparent padding. The RGB of transparent texels is dilated
from the content (push-pull), so bilinear filtering and the alpha-weighted mips never pull in black or a neighbour.
Cell edges stay on texel and BC7 block boundaries down to mip 5, so the 2×2 mip filter never mixes two cells there.

Blend modes (decals.json "blend"):
- "alpha": straight-alpha over: out = mix(base, rgb, a).
- "multiply": out = base * mix(1, rgb, a). RGB is the multiply colour (white = no change), alpha its strength.

The atlas is encoded with `zg-tex rgba` (BC7 sRGB + straight alpha, alpha-weighted mips) and `zg-tex normal` (BC5, flat
where a decal has no relief). Intermediate PNGs go to tools/textures/.cache/decals/build/.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import subprocess
import sys
import urllib.request
import zipfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import numpy as np
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent  # tools/textures
APP = HERE.parent.parent  # apps/zugriff
REPO = APP.parent.parent
CACHE = HERE / ".cache" / "decals"
BUILD = CACHE / "build"
OUT = APP / "assets" / "textures" / "decals"
FONTS = APP / "assets" / "fonts"

ATLAS = 2048  # width
ATLAS_H = 2560  # height (grown from 2048 for the street plates, 2026-09-29)
GRID = 128
PAD = 16
SS = 4  # supersampling for everything drawn with Pillow

# ambientCG downloads: asset id → resolution of the JPG zip.
ACG = {
    "Leaking001": "2K-JPG",
    "Leaking003": "2K-JPG",
    "Leaking006": "2K-JPG",
    "Leaking007": "2K-JPG",
    "Leaking008": "2K-JPG",
    "Leaking009": "2K-JPG",
    "Leaking012B": "1K-JPG",
    "AsphaltDamage001": "2K-JPG",
    "TireTracks001": "2K-JPG",
    "ManholeCover004": "1K-JPG",
    "Footsteps004": "1K-JPG",
    "Moss002": "1K-JPG",
    "Smear003": "1K-JPG",
    "SurfaceImperfections003": "1K-JPG",
}

F_BOLD = FONTS / "BarlowCondensed-Bold.ttf"
F_SEMI = FONTS / "BarlowCondensed-SemiBold.ttf"
F_MED = FONTS / "BarlowCondensed-Medium.ttf"
F_INTER = FONTS / "Inter.ttf"

# Colours (sRGB 0..1), close to the RAL safety colours German signs use.
def rgb(*c: int) -> np.ndarray:
    return np.array(c, np.float32) / 255.0


RED = rgb(170, 30, 36)  # RAL 3001
GREEN = rgb(0, 123, 76)  # RAL 6032
YELLOW = rgb(242, 184, 0)  # RAL 1003
TRAFFIC_YELLOW = rgb(240, 196, 30)  # RAL 1023
WHITE = rgb(238, 237, 230)
BLACK = rgb(28, 28, 30)
BLUE = rgb(20, 72, 140)  # RAL 5005
NAVY = rgb(24, 42, 72)
ROAD_WHITE = rgb(226, 226, 216)


# ---------------------------------------------------------------------------------------------------------------------
# Sources


def fetch(asset: str) -> Path:
    res = ACG[asset]
    d = CACHE / asset
    if d.is_dir() and any(d.glob("*_Color.jpg")):
        return d
    d.mkdir(parents=True, exist_ok=True)
    url = f"https://ambientcg.com/get?file={asset}_{res}.zip"
    print(f"  download {url}", flush=True)
    req = urllib.request.Request(url, headers={"User-Agent": "zugriff-decals/1.0"})
    tmp = CACHE / f"{asset}.zip"
    with urllib.request.urlopen(req) as r, open(tmp, "wb") as f:
        shutil.copyfileobj(r, f)
    with zipfile.ZipFile(tmp) as z:
        for n in z.namelist():
            if n.endswith(".jpg") or n.endswith(".png"):
                z.extract(n, d)
    tmp.unlink()
    return d


def src(asset: str, kind: str) -> Path:
    return next(fetch(asset).glob(f"*_{kind}.jpg"))


def load(path: Path, mode: str = "RGB", box: tuple | None = None, size: tuple | None = None) -> np.ndarray:
    im = Image.open(path).convert(mode)
    if box:
        im = im.crop(box)
    if size:
        im = im.resize(size, Image.LANCZOS)
    return np.asarray(im, np.float32) / 255.0


# ---------------------------------------------------------------------------------------------------------------------
# Numeric helpers


def smoothstep(e0: float, e1: float, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def fbm(w: int, h: int, scale: float, seed: int, octaves: int = 4, persistence: float = 0.5, sx: float = 1.0, sy: float = 1.0) -> np.ndarray:
    """Fractal value noise, roughly 0..1 (mean 0.5). `scale` is the largest feature in px; sx/sy stretch it."""
    rng = np.random.default_rng(seed)
    out = np.zeros((h, w), np.float32)
    amp = 1.0
    for o in range(octaves):
        s = scale / (2**o)
        fx, fy = max(s * sx, 1.0), max(s * sy, 1.0)
        gw, gh = int(math.ceil(w / fx)) + 3, int(math.ceil(h / fy)) + 3
        g = rng.random((gh, gw), dtype=np.float32)
        up = np.asarray(Image.fromarray(g, "F").resize((int(gw * fx), int(gh * fy)), Image.BICUBIC))
        out += amp * up[:h, :w]
        amp *= persistence
    out = (out - out.mean()) / (out.std() + 1e-6)
    return np.clip(0.5 + 0.18 * out, 0.0, 1.0)


def blur(a: np.ndarray, sigma: float) -> np.ndarray:
    """Separable Gaussian blur of a 2-D float array (edges clamped)."""
    if sigma <= 0:
        return a
    r = max(1, int(sigma * 3))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2).astype(np.float32)
    k /= k.sum()
    p = np.pad(a, ((0, 0), (r, r)), mode="edge")
    t = sum(k[i] * p[:, i : i + a.shape[1]] for i in range(2 * r + 1))
    p = np.pad(t, ((r, r), (0, 0)), mode="edge")
    return sum(k[i] * p[i : i + a.shape[0], :] for i in range(2 * r + 1)).astype(np.float32)


def lum(c: np.ndarray) -> np.ndarray:
    return c[..., 0] * 0.2126 + c[..., 1] * 0.7152 + c[..., 2] * 0.0722


def solid(h: int, w: int, c: np.ndarray) -> np.ndarray:
    return np.broadcast_to(c, (h, w, 3)).astype(np.float32).copy()


def over(dst: np.ndarray, color: np.ndarray, alpha: np.ndarray) -> None:
    """Straight-alpha `color` over `dst` (h×w×4), in place. `color` is h×w×3 or a 3-vector."""
    a = alpha[..., None]
    da = dst[..., 3:4]
    oa = a + da * (1.0 - a)
    col = np.broadcast_to(color, dst[..., :3].shape)
    num = col * a + dst[..., :3] * da * (1.0 - a)
    dst[..., :3] = np.where(oa > 1e-6, num / np.maximum(oa, 1e-6), col)
    dst[..., 3:4] = oa


def rgba(h: int, w: int) -> np.ndarray:
    return np.zeros((h, w, 4), np.float32)


def height_to_normal(hgt: np.ndarray, strength: float) -> np.ndarray:
    """OpenGL-style tangent normal (0..1) from a height field, image rows going down = -V."""
    gx = np.zeros_like(hgt)
    gy = np.zeros_like(hgt)
    gx[:, 1:-1] = (hgt[:, 2:] - hgt[:, :-2]) * 0.5
    gy[1:-1, :] = (hgt[2:, :] - hgt[:-2, :]) * 0.5
    n = np.stack([-gx * strength, gy * strength, np.ones_like(hgt)], -1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return n * 0.5 + 0.5


def edge_feather(h: int, w: int, left=0.0, right=0.0, top=0.0, bottom=0.0) -> np.ndarray:
    """1 inside, smoothly 0 at the chosen borders over the given fractions of the size."""
    y = (np.arange(h, dtype=np.float32) + 0.5) / h
    x = (np.arange(w, dtype=np.float32) + 0.5) / w
    fx = np.ones(w, np.float32)
    fy = np.ones(h, np.float32)
    if left:
        fx *= smoothstep(0.0, left, x)
    if right:
        fx *= smoothstep(0.0, right, 1.0 - x)
    if top:
        fy *= smoothstep(0.0, top, y)
    if bottom:
        fy *= smoothstep(0.0, bottom, 1.0 - y)
    return fy[:, None] * fx[None, :]


def radial(h: int, w: int, seed: int, warp: float = 0.25, scale: float = 0.35) -> np.ndarray:
    """Distance from the centre in units of the half size, warped by noise: blob shapes (1 ≈ edge of the cell)."""
    y = (np.arange(h, dtype=np.float32) + 0.5) / h * 2 - 1
    x = (np.arange(w, dtype=np.float32) + 0.5) / w * 2 - 1
    d = np.sqrt(x[None, :] ** 2 + y[:, None] ** 2)
    n = fbm(w, h, max(w, h) * scale, seed, 4)
    return d * (1.0 + warp * (n - 0.5) * 2)


# ---------------------------------------------------------------------------------------------------------------------
# Drawing at SS× resolution


class Canvas:
    """A supersampled 8-bit coverage mask to draw vector shapes and text into."""

    def __init__(self, w: int, h: int):
        self.w, self.h = w, h
        self.im = Image.new("L", (w * SS, h * SS), 0)
        self.d = ImageDraw.Draw(self.im)

    def p(self, x: float, y: float) -> tuple:
        """Content px → SS px."""
        return (x * SS, y * SS)

    def rrect(self, x0, y0, x1, y1, r, fill=255):
        self.d.rounded_rectangle([x0 * SS, y0 * SS, x1 * SS - 1, y1 * SS - 1], radius=r * SS, fill=fill)

    def rect(self, x0, y0, x1, y1, fill=255):
        self.d.rectangle([x0 * SS, y0 * SS, x1 * SS - 1, y1 * SS - 1], fill=fill)

    def poly(self, pts, fill=255):
        self.d.polygon([self.p(x, y) for x, y in pts], fill=fill)

    def ellipse(self, cx, cy, rx, ry=None, fill=255):
        ry = rx if ry is None else ry
        self.d.ellipse([(cx - rx) * SS, (cy - ry) * SS, (cx + rx) * SS, (cy + ry) * SS], fill=fill)

    def stroke(self, pts, width, fill=255):
        """A polyline with round joints and caps."""
        self.d.line([self.p(x, y) for x, y in pts], fill=fill, width=int(round(width * SS)))
        for x, y in pts:
            self.ellipse(x, y, width / 2, fill=fill)

    def paste(self, mask: Image.Image, x: float, y: float, erase: bool = False):
        """Pastes an SS-resolution mask with its top left at content px (x, y)."""
        layer = Image.new("L", self.im.size, 0)
        layer.paste(mask, (int(round(x * SS)), int(round(y * SS))))
        a = np.asarray(self.im, np.int16)
        b = np.asarray(layer, np.int16)
        self.im = Image.fromarray(np.clip(a - b if erase else np.maximum(a, b), 0, 255).astype(np.uint8)).copy()
        self.d = ImageDraw.Draw(self.im)

    def text(self, s: str, font: Path, box: tuple, align: str = "center", stretch_y: float = 1.0, stencil: bool = False,
             spacing: float = 0.0, erase: bool = False, variation: str | None = None, height: float | None = None):
        """Draws `s` fitted into box (x0, y0, x1, y1) in content px. `height` fixes the cap height instead of fitting.
        Returns the drawn box."""
        x0, y0, x1, y1 = box
        m = text_mask(s, font, stencil=stencil, spacing=spacing, variation=variation)
        bw, bh = (x1 - x0) * SS, (y1 - y0) * SS
        mw, mh = m.size
        if height is not None:
            k = height * SS / mh
            kx, ky = k, k * stretch_y
            if mw * kx > bw:
                kx = bw / mw
        else:
            k = min(bw / mw, bh / (mh * stretch_y))
            kx, ky = k, k * stretch_y
        tw, th = max(1, int(round(mw * kx))), max(1, int(round(mh * ky)))
        m = m.resize((tw, th), Image.LANCZOS)
        ox = {"left": 0, "center": (bw - tw) / 2, "right": bw - tw}[align]
        oy = (bh - th) / 2
        self.paste(m, x0 + ox / SS, y0 + oy / SS, erase=erase)
        return (x0 + ox / SS, y0 + oy / SS, x0 + (ox + tw) / SS, y0 + (oy + th) / SS)

    def array(self) -> np.ndarray:
        return np.asarray(self.im.resize((self.w, self.h), Image.BOX), np.float32) / 255.0


_FONT_CACHE: dict = {}


def font(path: Path, size: int, variation: str | None = None) -> ImageFont.FreeTypeFont:
    key = (path, size, variation)
    if key not in _FONT_CACHE:
        f = ImageFont.truetype(str(path), size)
        if variation:
            f.set_variation_by_name(variation)
        _FONT_CACHE[key] = f
    return _FONT_CACHE[key]


def text_mask(s: str, path: Path, size: int = 400, stencil: bool = False, spacing: float = 0.0, variation: str | None = None) -> Image.Image:
    """Renders text tightly cropped to its ink (cap height included) as an L mask. `spacing` adds em-fractions."""
    f = font(path, size, variation)
    asc, desc = f.getmetrics()
    widths = [f.getlength(c) for c in s]
    total = int(sum(widths) + spacing * size * max(0, len(s) - 1)) + size
    im = Image.new("L", (total, asc + desc + size // 2), 0)
    d = ImageDraw.Draw(im)
    x = size // 2
    for c, cw in zip(s, widths):
        d.text((x, size // 4), c, font=f, fill=255)
        x += cw + spacing * size
    if stencil:
        im = stencilize(im, gap=max(2, int(size * 0.075)))
    bb = im.getbbox()
    return im.crop(bb) if bb else im


def stencilize(im: Image.Image, gap: int) -> Image.Image:
    """Cuts stencil bridges: every enclosed counter (O, A, B, 0, 4, 6, 8, 9, …) gets a gap to the outside, upwards
    when that is not much thicker than its thinnest straight exit, and on the opposite side too when symmetric (O, 0)."""
    im = im.copy()
    w, h = im.size
    marked = Image.new("L", (w + 2, h + 2), 0)
    marked.paste(im.point(lambda v: 255 if v >= 128 else 0), (1, 1))
    ImageDraw.floodfill(marked, (0, 0), 100)
    arr = np.asarray(marked).copy()
    label = 1
    cuts = []
    while True:
        zeros = np.argwhere(arr == 0)
        if len(zeros) == 0:
            break
        y, x = zeros[0]
        tmp = Image.fromarray(arr).copy()  # fromarray shares the buffer; drawing on it is lost
        ImageDraw.floodfill(tmp, (int(x), int(y)), label)
        arr = np.asarray(tmp).copy()
        comp = np.argwhere(arr == label)
        label += 1
        if label >= 100:
            break
        if len(comp) < (gap * gap):  # specks inside anti-aliased ink
            continue
        cy, cx = comp.mean(0)
        # Start from the component pixel closest to its centroid.
        j = np.argmin((comp[:, 0] - cy) ** 2 + (comp[:, 1] - cx) ** 2)
        sy, sx = comp[j]
        runs = []
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            yy, xx, ink, start = sy, sx, 0, None
            ok = False
            while 0 <= yy < h + 2 and 0 <= xx < w + 2:
                v = arr[yy, xx]
                if v == 255:
                    ink += 1
                    if start is None:
                        start = (yy, xx)
                elif v == 100:
                    ok = start is not None
                    break
                elif start is not None and v != arr[sy, sx]:
                    break  # ran into another counter
                yy += dy
                xx += dx
            if ok:
                runs.append((ink, (dy, dx), start, (yy, xx)))
        if not runs:
            continue
        runs.sort(key=lambda r: r[0])
        # Prefer a bridge at the top (the classic stencil A, B, 4, 6, 9) unless that is much thicker.
        up = [r for r in runs if r[1] == (-1, 0)]
        first = up[0] if up and up[0][0] <= runs[0][0] * 1.8 else runs[0]
        cuts.append(first)
        opp = [r for r in runs if r[1] == (-first[1][0], -first[1][1])]
        if opp and abs(opp[0][0] - first[0]) <= 0.1 * first[0]:  # symmetric counters (O, 0) get both
            cuts.append(opp[0])
    d = ImageDraw.Draw(im)
    for _, (dy, dx), (y0, x0), (y1, x1) in cuts:
        # Coordinates in `arr` are offset by the 1 px border.
        y0, x0, y1, x1 = y0 - 1, x0 - 1, y1 - 1, x1 - 1
        if dx == 0:
            d.rectangle([x0 - gap // 2, min(y0, y1), x0 + gap // 2, max(y0, y1)], fill=0)
        else:
            d.rectangle([min(x0, x1), y0 - gap // 2, max(x0, x1), y0 + gap // 2], fill=0)
    return im


def scratches(w: int, h: int, seed: int, n: int, length: tuple = (0.05, 0.3), width: tuple = (0.4, 1.2), angle=None) -> np.ndarray:
    """Thin random scratch lines (coverage 0..1)."""
    rng = np.random.default_rng(seed)
    c = Canvas(w, h)
    for _ in range(n):
        x, y = rng.random() * w, rng.random() * h
        a = rng.uniform(0, math.pi) if angle is None else angle + rng.normal(0, 0.25)
        L = rng.uniform(*length) * max(w, h)
        pts = [(x, y)]
        for _ in range(3):
            x += math.cos(a) * L / 3
            y += math.sin(a) * L / 3
            a += rng.normal(0, 0.12)
            pts.append((x, y))
        c.d.line([c.p(*p) for p in pts], fill=int(rng.uniform(120, 255)), width=max(1, int(rng.uniform(*width) * SS)))
    return c.array()


def paint_wear(alpha: np.ndarray, seed: int, amount: float = 0.35, grain: float = 0.25, scratch_n: int = 0, scale: float = 40.0) -> np.ndarray:
    """Weathers painted-on alpha: flaked patches, fine grain, optional scratches through the paint."""
    h, w = alpha.shape
    big = fbm(w, h, scale, seed, 5)
    flakes = smoothstep(1.0 - amount * 0.55, 1.0 - amount * 0.55 + 0.06, big)
    fine = fbm(w, h, 3.0, seed + 1, 2)
    out = alpha * (1.0 - flakes) * (1.0 - grain * smoothstep(0.45, 0.75, fine))
    if scratch_n:
        out *= 1.0 - 0.9 * scratches(w, h, seed + 2, scratch_n)
    return np.clip(out, 0, 1)


def plate_weather(col: np.ndarray, alpha: np.ndarray, seed: int, dirt: float = 0.25, scratch_n: int = 25) -> np.ndarray:
    """Weathers a painted metal or plastic plate's colour: mottling, grime gathering at the bottom and the edges,
    light scratches. Alpha is left alone (the plate is solid)."""
    h, w = alpha.shape
    mottle = fbm(w, h, max(w, h) * 0.3, seed, 5)
    col = col * (0.93 + 0.12 * mottle[..., None])
    inner = blur(alpha, max(w, h) * 0.03)
    edge = np.clip(1.0 - (inner - 0.5) * 2.2, 0, 1) * alpha
    y = np.linspace(0, 1, h, dtype=np.float32)[:, None]
    grime = np.clip(dirt * (0.4 * edge + 0.6 * y**3) * (0.6 + 0.8 * fbm(w, h, max(w, h) * 0.12, seed + 3, 4)), 0, 0.6)
    col = col * (1.0 - grime[..., None] * np.array([0.9, 0.92, 1.0], np.float32))
    col = col * (1.0 - 0.06) + 0.06 * lum(col)[..., None]  # a little sun fade
    if scratch_n:
        s = scratches(w, h, seed + 4, scratch_n, length=(0.02, 0.12), width=(0.3, 0.8))
        col = col + (0.75 - col) * (0.45 * s[..., None])
    return np.clip(col, 0, 1)


def screws(c: np.ndarray, w: int, h: int, inset: float, r: float) -> None:
    """Draws four grey screw heads with a slot shadow into an RGBA layer, in place."""
    cv = Canvas(w, h)
    sh = Canvas(w, h)
    for x, y in ((inset, inset), (w - inset, inset), (inset, h - inset), (w - inset, h - inset)):
        cv.ellipse(x, y, r)
        sh.ellipse(x + r * 0.2, y + r * 0.25, r * 1.15)
    over(c, BLACK, sh.array() * 0.35 * c[..., 3])
    head = cv.array()
    shade = fbm(w, h, 6, 7, 2)
    col = solid(h, w, rgb(150, 150, 146)) * (0.8 + 0.3 * shade[..., None])
    over(c, col, head)


# ---------------------------------------------------------------------------------------------------------------------
# Photo decals (ambientCG)


def photo_leak(asset: str, box, cw: int, ch: int, color: np.ndarray, contrast: float, gain: float, feather: dict, seed: int):
    col = load(src(asset, "Color"), "RGB", box, (cw, ch))
    a = load(src(asset, "Opacity"), "L", box, (cw, ch))
    L = lum(col)
    ref = float((L * a).sum() / max(a.sum(), 1e-3))
    t = np.clip((L / max(ref, 1e-3)) ** contrast, 0.4, 1.4)
    out = np.clip(color[None, None, :] * t[..., None], 0, 1)
    a = np.clip(a * gain, 0, 1) * edge_feather(ch, cw, **feather)
    return np.dstack([out, a]), None


def leak_water_a(cw, ch):
    # A tall slice of Leaking001: damp streaks running down from a sill or a pipe. Multiply: grey-brown water mark.
    return photo_leak("Leaking001", (620, 0, 1100, 1024), cw, ch, rgb(100, 93, 82), 1.2, 1.5, dict(left=0.18, right=0.18, top=0.03), 11)


def leak_window(cw, ch):
    # The whole of Leaking003, a wide band of streaks for under a window sill.
    return photo_leak("Leaking003", (40, 0, 2008, 1000), cw, ch, rgb(100, 94, 84), 1.2, 1.35, dict(left=0.08, right=0.08, top=0.02), 12)


def leak_rust_pipe(cw, ch):
    # Leaking006: a narrow run-off streak from a hole or pipe end, recoloured to rust.
    col = load(src("Leaking006", "Color"), "RGB", (64, 0, 448, 2048), (cw, ch))
    a = load(src("Leaking006", "Opacity"), "L", (64, 0, 448, 2048), (cw, ch))
    L = lum(col)
    rust = np.array([0.46, 0.22, 0.09], np.float32)
    dark = np.array([0.25, 0.13, 0.07], np.float32)
    t = np.clip((L - 0.25) / 0.5, 0, 1)[..., None]
    out = dark + (rust - dark) * t
    a = np.clip(a * 1.1, 0, 1) * edge_feather(ch, cw, left=0.1, right=0.1)
    return np.dstack([out, a]), None


def leak_rust_bolts(cw, ch):
    # Rust run-off below two bolts: streaks that spread as they run down, striated, with Leaking012B's drip lines
    # (a tiling CC0 material) as extra fine runs.
    y = (np.arange(ch, dtype=np.float32) + 0.5) / ch
    x = np.arange(cw, dtype=np.float32) + 0.5
    L = load(src("Leaking012B", "Color"), "L", (0, 0, 256, 1024), (cw, ch))
    drips = np.clip((0.985 - L) * 12.0, 0, 1)
    stri = fbm(cw, ch, 40, 5, 4, sx=0.12, sy=6.0)
    a = np.zeros((ch, cw), np.float32)
    for k, x0 in enumerate((0.3, 0.72)):
        wob = (fbm(1, ch, 80, 7 + k, 3)[:, 0] - 0.5) * 6
        yy = np.clip((y - 0.03) / 0.97, 0, 1)
        width = 2.5 + 16.0 * yy**0.8
        prof = np.exp(-(((x[None, :] - x0 * cw - wob[:, None]) / width[:, None]) ** 2))
        fade = smoothstep(0.0, 0.02, y - 0.02) * (1 - yy) ** 1.4
        a = np.maximum(a, prof * fade[:, None] * (0.45 + 0.75 * stri))
        spot = Canvas(cw, ch)
        spot.ellipse(x0 * cw, ch * 0.03, 5.5)
        a = np.maximum(a, spot.array() * 0.95)
    a = np.clip(a + drips * 0.6 * (1 - y[:, None]) ** 1.2 * np.clip(a * 3, 0, 1), 0, 1)
    a *= edge_feather(ch, cw, left=0.06, right=0.06)
    n = fbm(cw, ch, 20, 6, 3)[..., None]
    col = rgb(86, 38, 14) + (rgb(150, 70, 26) - rgb(86, 38, 14)) * n
    return np.dstack([col, np.clip(a, 0, 1)]), None


def rising(asset: str, box, cw, ch, color, gain, seed):
    """Leaking007/008/009 are streaks rising from a wall's foot: multiply-colour + strength."""
    col = load(src(asset, "Color"), "RGB", box, (cw, ch))
    a = load(src(asset, "Opacity"), "L", box, (cw, ch))
    L = lum(col)
    ref = float((L * a).sum() / max(a.sum(), 1e-3))
    t = np.clip((L / max(ref, 1e-3)), 0.5, 1.3)[..., None]
    out = np.clip(color * t, 0, 1)
    a = np.clip(a * gain, 0, 1) * edge_feather(ch, cw, left=0.06, right=0.06)
    return np.dstack([out, a]), None


def grime_base_a(cw, ch):
    return rising("Leaking007", (0, 60, 2048, 512), cw, ch, rgb(96, 88, 76), 1.25, 21)


def grime_base_b(cw, ch):
    return rising("Leaking009", (0, 120, 2048, 512), cw, ch, rgb(88, 82, 72), 1.35, 22)


def damp_base(cw, ch):
    # Leaking008 as rising damp: a cooler, darker wet band with a salt-stained tide line.
    layer = rising("Leaking008", (0, 0, 2048, 512), cw, ch, rgb(92, 92, 90), 1.2, 23)[0]
    out, base = layer[..., :3], layer[..., 3]
    y = np.linspace(0, 1, ch, dtype=np.float32)[:, None]
    band = smoothstep(0.25, 0.7, y) * (0.55 + 0.45 * fbm(cw, ch, 60, 24, 4))
    a = np.clip(np.maximum(base, band * 0.8) * edge_feather(ch, cw, left=0.05, right=0.05), 0, 1)
    return np.dstack([out, a]), None


def moss_base(cw, ch):
    # Moss002's colour, masked into clumps that grow up from the wall's foot.
    col = load(src("Moss002", "Color"), "RGB", (0, 0, 1024, 256), (cw, ch))
    col = np.clip(col * 0.85, 0, 1)
    y = np.linspace(0, 1, ch, dtype=np.float32)[:, None]
    n = fbm(cw, ch, 40, 31, 5, sx=1.6)
    field_ = y * 1.1 + (n - 0.5) * 1.6
    a = smoothstep(0.5, 0.68, field_)
    clumps = fbm(cw, ch, 10, 33, 3)
    a *= smoothstep(0.3, 0.5, clumps + y[:, :1] * 0.25)
    a *= 0.75 + 0.25 * fbm(cw, ch, 4, 32, 2)
    a *= edge_feather(ch, cw, left=0.08, right=0.08)
    return np.dstack([col, np.clip(a, 0, 1)]), None


def ivy(cw, ch, seed: int):
    """Ivy climbing a wall from its foot: woody stems wandering up and branching, dense five-lobed leaves along
    them (thicker low down, thinning out at the top), in dark and fresh greens, their relief as the normal."""
    rng = np.random.default_rng(seed)
    stems = Canvas(cw, ch)
    leaf_layers = [Canvas(cw, ch) for _ in range(4)]  # four shades
    heights = Canvas(cw, ch)

    def grow(x, y, length, width, depth):
        pts = [(x, y)]
        ang = -math.pi / 2 + rng.uniform(-0.3, 0.3)
        for _ in range(int(length / 6)):
            ang += rng.uniform(-0.35, 0.35) + (-math.pi / 2 - ang) * 0.12
            x += math.cos(ang) * 6
            y += math.sin(ang) * 6
            if not (0 < x < cw and 0 < y < ch):
                break
            pts.append((x, y))
            # Leaves along the stem, fewer towards the top.
            if rng.random() < 0.9 * (0.35 + 0.65 * y / ch):
                for _ in range(2):
                    lx = x + rng.uniform(-14, 14)
                    ly = y + rng.uniform(-10, 10)
                    r = rng.uniform(6, 11) * (0.7 + 0.5 * y / ch)
                    k = int(rng.integers(0, 4))
                    rot = rng.uniform(0, math.tau)
                    for lobe in range(5):
                        a = rot + lobe * math.tau / 5
                        leaf_layers[k].ellipse(lx + math.cos(a) * r * 0.45, ly + math.sin(a) * r * 0.45, r * 0.55)
                        heights.ellipse(lx + math.cos(a) * r * 0.45, ly + math.sin(a) * r * 0.45, r * 0.55, fill=int(120 + 100 * rng.random()))
            if depth < 3 and rng.random() < 0.05:
                grow(x, y, length * rng.uniform(0.3, 0.6), width * 0.7, depth + 1)
        stems.stroke(pts, width)

    for i in range(7):
        grow(cw * (0.1 + 0.8 * rng.random()), ch - 2, ch * rng.uniform(0.55, 1.0), 3.0, 0)
    c = rgba(ch, cw)
    over(c, rgb(58, 44, 30), stems.array())
    shades = [rgb(28, 52, 20), rgb(40, 70, 26), rgb(56, 88, 32), rgb(34, 60, 30)]
    for layer, col in zip(leaf_layers, shades):
        m = layer.array()
        mottle = 0.8 + 0.4 * fbm(cw, ch, 30, seed + 7, 3)
        over(c, np.clip(col * mottle[..., None], 0, 1), m)
    c[..., 3] = np.clip(c[..., 3], 0, 1)
    hgt = blur(heights.array(), 1.0)
    return c, height_to_normal(hgt, 5.0)


def ivy_a(cw, ch):
    return ivy(cw, ch, 41)


def ivy_b(cw, ch):
    return ivy(cw, ch, 42)


def grime_patch_a(cw, ch):
    # SurfaceImperfections003: vertical smudges; a soft, noisy blob of it.
    col = load(src("SurfaceImperfections003", "Color"), "RGB", (100, 100, 900, 900), (cw, ch))
    op = load(src("SurfaceImperfections003", "Opacity"), "L", (100, 100, 900, 900), (cw, ch))
    L = lum(col)
    d = radial(ch, cw, 41, warp=0.45)
    shape = smoothstep(0.95, 0.35, d)
    m = np.clip((1.0 - L) * 1.6, 0, 1) * np.clip(op * 1.3 + 0.2, 0, 1)
    a = np.clip(shape * (0.35 + 0.8 * m), 0, 0.9)
    out = np.clip(rgb(84, 78, 68) * (0.8 + 0.4 * L[..., None]), 0, 1)
    return np.dstack([out, a]), None


def grime_patch_b(cw, ch):
    # Smear003: dirty hand or tool smears, in a soft patch.
    col = load(src("Smear003", "Color"), "RGB", (100, 0, 900, 800), (cw, ch))
    L = lum(col)
    m = np.clip((L - 0.03) * 5.0, 0, 1)
    d = radial(ch, cw, 42, warp=0.5)
    shape = smoothstep(0.95, 0.3, d)
    a = np.clip(shape * (0.2 + 0.9 * m), 0, 0.85)
    out = solid(ch, cw, rgb(70, 64, 56))
    return np.dstack([out, a]), None


def floor_damage(cw, ch):
    col = load(src("AsphaltDamage001", "Color"), "RGB", None, (cw, ch))
    a = load(src("AsphaltDamage001", "Opacity"), "L", None, (cw, ch))
    n = load(src("AsphaltDamage001", "NormalGL"), "RGB", None, (cw, ch))
    a = a * edge_feather(ch, cw, left=0.04, right=0.04, top=0.04, bottom=0.04)
    return np.dstack([col * 0.92, a]), n


def manhole_cover(cw, ch):
    col = load(src("ManholeCover004", "Color"), "RGB", None, (cw, ch))
    a = load(src("ManholeCover004", "Opacity"), "L", None, (cw, ch))
    n = load(src("ManholeCover004", "NormalGL"), "RGB", None, (cw, ch))
    return np.dstack([col, a]), n


def footprints_mud(cw, ch):
    # Footsteps004: two lines of boot prints. The prints are the low areas of its displacement.
    img = Image.open(src("Footsteps004", "Displacement")).convert("L")
    w0, h0 = img.size
    box = (int(w0 * 0.1), 0, int(w0 * 0.9), h0)
    disp = load(src("Footsteps004", "Displacement"), "L", box, (cw, ch))
    n = load(src("Footsteps004", "NormalGL"), "RGB", box, (cw, ch))
    base = np.median(disp)
    m = smoothstep(0.02, 0.12, base - disp)
    rim = smoothstep(0.02, 0.08, disp - base) * 0.35
    a = np.clip(np.maximum(m * 0.85, rim), 0, 1) * (0.7 + 0.3 * fbm(cw, ch, 8, 51, 3))
    a *= edge_feather(ch, cw, top=0.04, bottom=0.12)
    col = rgb(66, 52, 38) + (rgb(96, 80, 60) - rgb(66, 52, 38)) * fbm(cw, ch, 10, 52, 3)[..., None]
    return np.dstack([col, np.clip(a, 0, 1)]), n


def tyre_track(cw, ch):
    # TireTracks001's tread: the lugs (low in its displacement) print as mud/rubber. The strip tiles, so it is doubled
    # along its length.
    box = (0, 40, 2048, 472)
    disp = load(src("TireTracks001", "Displacement"), "L", box)
    nrm = load(src("TireTracks001", "NormalGL"), "RGB", box)
    disp = np.concatenate([disp, disp], 1)
    nrm = np.concatenate([nrm, nrm], 1)
    disp = np.asarray(Image.fromarray(disp, "F").resize((cw, ch), Image.LANCZOS))
    nrm = np.stack([np.asarray(Image.fromarray(nrm[..., i].copy(), "F").resize((cw, ch), Image.LANCZOS)) for i in range(3)], -1)
    lug = smoothstep(0.52, 0.38, disp)
    streak = fbm(cw, ch, 60, 61, 4, sx=4.0, sy=0.25)
    along = 0.55 + 0.45 * smoothstep(0.25, 0.65, fbm(cw, ch, 300, 62, 2, sx=1.0, sy=0.2))
    a = np.clip(lug * (0.55 + 0.45 * streak) * along, 0, 1) * edge_feather(ch, cw, left=0.06, right=0.12, top=0.05, bottom=0.05)
    col = rgb(46, 40, 34) + (rgb(74, 64, 52) - rgb(46, 40, 34)) * fbm(cw, ch, 12, 63, 3)[..., None]
    return np.dstack([col, a]), np.clip(nrm, 0, 1)


# ---------------------------------------------------------------------------------------------------------------------
# Procedural grime


def crack_paths(rng, start, end, depth: int, width: float, jitter: float, out: list, bounds=None):
    """Midpoint-displaced crack from start to end, with side branches; appends (points, width) polylines."""
    pts = [np.array(start, np.float64), np.array(end, np.float64)]
    for _ in range(depth):
        new = [pts[0]]
        for a, b in zip(pts[:-1], pts[1:]):
            m = (a + b) / 2
            d = b - a
            n = np.array([-d[1], d[0]])
            m = m + n * rng.normal(0, jitter)
            new += [m, b]
        pts = new
    if bounds is not None:
        pts = [np.clip(q, bounds[0], bounds[1]) for q in pts]
    out.append((pts, width))
    return pts


def crack(cw, ch, seed: int, start, end, width: float, branches: int, net: bool = False):
    rng = np.random.default_rng(seed)
    polys: list = []
    lim = (np.array([cw * 0.05, ch * 0.03]), np.array([cw * 0.95, ch * 0.97]))
    main = crack_paths(rng, start, end, 7, width, 0.16, polys, lim)
    for _ in range(branches):
        i = rng.integers(len(main) // 6, len(main) * 5 // 6)
        p = main[i]
        d = main[min(i + 1, len(main) - 1)] - main[max(i - 1, 0)]
        ang = math.atan2(d[1], d[0]) + rng.choice([-1, 1]) * rng.uniform(0.5, 1.2)
        L = rng.uniform(0.15, 0.4) * max(cw, ch)
        q = p + np.array([math.cos(ang), math.sin(ang)]) * L
        q = np.clip(q, [cw * 0.08, ch * 0.08], [cw * 0.92, ch * 0.92])
        sub = crack_paths(rng, p, q, 5, width * rng.uniform(0.4, 0.65), 0.25, polys, lim)
        if net and rng.random() < 0.6:
            j = rng.integers(len(sub) // 3, len(sub))
            ang2 = ang + rng.choice([-1, 1]) * rng.uniform(0.6, 1.3)
            r = sub[j] + np.array([math.cos(ang2), math.sin(ang2)]) * L * 0.5
            r = np.clip(r, [cw * 0.08, ch * 0.08], [cw * 0.92, ch * 0.92])
            crack_paths(rng, sub[j], r, 4, width * 0.35, 0.3, polys, lim)
    core = Canvas(cw, ch)
    for pts, wd in polys:
        n = len(pts)
        for k in range(n - 1):
            t = k / max(n - 2, 1)
            wk = wd * (0.35 + 0.65 * math.sin(math.pi * min(max(t, 0.02), 0.98)) ** 0.6) * rng.uniform(0.75, 1.25)
            core.d.line([core.p(*pts[k]), core.p(*pts[k + 1])], fill=255, width=max(1, int(wk * SS)))
    m = core.array()
    halo = np.clip(blur(m, 2.2) * 1.6, 0, 1)
    chips = smoothstep(0.62, 0.7, fbm(cw, ch, 5, seed + 1, 3)) * np.clip(blur(m, 4.0) * 3, 0, 1)
    c = rgba(ch, cw)
    over(c, rgb(120, 116, 108), chips * 0.35)
    over(c, rgb(60, 56, 50), halo * 0.35)
    over(c, rgb(22, 20, 18), m * 0.95)
    hgt = -np.clip(m + 0.4 * halo, 0, 1)
    nrm = height_to_normal(blur(hgt, 0.7), 4.0)
    return c, nrm


def crack_wall_a(cw, ch):
    return crack(cw, ch, 71, (cw * 0.12, ch * 0.1), (cw * 0.85, ch * 0.9), 3.2, 3)


def crack_wall_b(cw, ch):
    return crack(cw, ch, 72, (cw * 0.55, ch * 0.05), (cw * 0.42, ch * 0.95), 3.6, 5)


def crack_floor(cw, ch):
    return crack(cw, ch, 73, (cw * 0.06, ch * 0.4), (cw * 0.94, ch * 0.62), 3.0, 5, net=True)


def oil_stain_a(cw, ch):
    d = radial(ch, cw, 81, warp=0.35, scale=0.3)
    body = smoothstep(0.85, 0.6, d)
    ring = np.exp(-(((d - 0.72) / 0.05) ** 2)) * 0.45
    tex = fbm(cw, ch, 18, 82, 4)
    inner = smoothstep(0.5, 0.0, d) * 0.2
    a = np.clip((body * (0.45 + 0.35 * tex) + ring + inner) * 0.95, 0, 0.92)
    col = rgb(20, 18, 16) + (rgb(40, 36, 30) - rgb(20, 18, 16)) * tex[..., None]
    # A few satellite drops.
    rng = np.random.default_rng(83)
    dots = Canvas(cw, ch)
    for _ in range(14):
        ang, r = rng.uniform(0, 2 * math.pi), rng.uniform(0.62, 0.9)
        dots.ellipse(cw / 2 + math.cos(ang) * r * cw / 2, ch / 2 + math.sin(ang) * r * ch / 2, rng.uniform(1.0, 3.5))
    a = np.maximum(a, dots.array() * 0.8)
    return np.dstack([col, a]), None


def oil_stain_b(cw, ch):
    # A drip trail: a line of drops and small pools, as under a parked vehicle.
    rng = np.random.default_rng(91)
    c = Canvas(cw, ch)
    x, y = cw * 0.2, ch * 0.3
    for _ in range(26):
        x += rng.uniform(2, 9)
        y += rng.normal(1.2, 3)
        c.ellipse(x, y, rng.uniform(1.5, 5.0), rng.uniform(1.5, 4.5))
    pool = radial(ch, cw, 92, warp=0.5, scale=0.25)
    yy, xx = np.mgrid[0:ch, 0:cw].astype(np.float32)
    blob = smoothstep(0.42, 0.28, np.sqrt(((xx - cw * 0.68) / cw) ** 2 + ((yy - ch * 0.62) / ch) ** 2) * (0.8 + 0.4 * fbm(cw, ch, 20, 93, 3)))
    m = np.clip(c.array() + blob, 0, 1)
    halo = np.clip(blur(m, 5) * 1.2, 0, 1) * 0.35
    tex = fbm(cw, ch, 10, 94, 3)
    a = np.clip(np.maximum(m * (0.7 + 0.25 * tex), halo), 0, 0.92)
    col = rgb(18, 16, 14) + (rgb(38, 34, 28) - rgb(18, 16, 14)) * tex[..., None]
    return np.dstack([col, a]), None


def puddle(cw, ch):
    d = radial(ch, cw, 101, warp=0.45, scale=0.28)
    water = smoothstep(0.66, 0.6, d)
    damp = smoothstep(0.92, 0.62, d) * (0.6 + 0.4 * fbm(cw, ch, 14, 102, 3))
    c = rgba(ch, cw)
    over(c, rgb(62, 60, 56), damp * 0.45)
    tex = fbm(cw, ch, 40, 103, 3)
    over(c, rgb(46, 52, 58) + (rgb(80, 88, 96) - rgb(46, 52, 58)) * tex[..., None], water * 0.72)
    return c, None


def skid_mark(cw, ch):
    y = (np.arange(ch, dtype=np.float32) + 0.5) / ch
    x = (np.arange(cw, dtype=np.float32) + 0.5) / cw
    wob = (fbm(cw, 1, 300, 111, 3)[0] - 0.5) * 0.15
    yc = 0.5 + wob[None, :]
    band = smoothstep(0.42, 0.34, np.abs(y[:, None] - yc))
    stri = fbm(cw, ch, 60, 112, 4, sx=6.0, sy=0.08)
    along = smoothstep(0.0, 0.25, x) * (0.55 + 0.45 * smoothstep(1.0, 0.55, x))
    a = np.clip(band * (0.35 + 0.65 * stri) * along[None, :] * 0.95, 0, 1)
    a *= edge_feather(ch, cw, left=0.02, right=0.02)
    col = solid(ch, cw, rgb(24, 23, 24))
    return np.dstack([col, a]), None


# ---------------------------------------------------------------------------------------------------------------------
# Signage and markings


def plate(cw, ch, color, radius, seed, dirt=0.25, scratch_n=25, border: tuple | None = None, screw=True):
    """A painted sign plate: returns (rgba layer, shape mask)."""
    shape = Canvas(cw, ch)
    shape.rrect(0, 0, cw, ch, radius)
    a = shape.array()
    c = rgba(ch, cw)
    over(c, color, a)
    if border:
        inset, width, bcol = border
        bc = Canvas(cw, ch)
        bc.rrect(inset, inset, cw - inset, ch - inset, max(radius - inset, 1))
        bc.rrect(inset + width, inset + width, cw - inset - width, ch - inset - width, max(radius - inset - width, 1), fill=0)
        over(c, bcol, bc.array())
    return c, a


def finish_plate(c, a, seed, dirt=0.25, scratch_n=25, screw_r=0.0, screw_inset=0.0):
    cw, ch = c.shape[1], c.shape[0]
    if screw_r:
        screws(c, cw, ch, screw_inset, screw_r)
    c[..., :3] = plate_weather(c[..., :3], a, seed, dirt, scratch_n)
    c[..., 3] = a
    return c


def sign_betreten_verboten(cw, ch):
    c, a = plate(cw, ch, RED, 10, 201, border=(8, 3, WHITE))
    t = Canvas(cw, ch)
    t.text("BETRETEN", F_BOLD, (30, 26, cw - 30, ch / 2 - 6), spacing=0.04)
    t.text("VERBOTEN", F_BOLD, (30, ch / 2 + 6, cw - 30, ch - 26), spacing=0.04)
    over(c, WHITE, t.array())
    return finish_plate(c, a, 201, dirt=0.3, screw_r=4.5, screw_inset=17), None


def running_man(cv: Canvas, x0, y0, s, width):
    """ISO-style running figure (to the right) in a box of size s at (x0, y0)."""
    P = lambda x, y: (x0 + x * s, y0 + y * s)
    cv.ellipse(*P(0.63, 0.14), 0.095 * s)
    cv.stroke([P(0.56, 0.31), P(0.44, 0.58)], width * 1.35)  # torso
    cv.stroke([P(0.57, 0.33), P(0.72, 0.44), P(0.84, 0.34)], width)  # front arm
    cv.stroke([P(0.54, 0.34), P(0.38, 0.36), P(0.27, 0.48)], width)  # back arm
    cv.stroke([P(0.45, 0.58), P(0.64, 0.68), P(0.61, 0.92), P(0.71, 0.93)], width)  # front leg
    cv.stroke([P(0.43, 0.6), P(0.34, 0.78), P(0.14, 0.8)], width)  # back leg


def sign_notausgang(cw, ch):
    c, a = plate(cw, ch, GREEN, 8, 211)
    t = Canvas(cw, ch)
    top, bot = 16, ch * 0.7
    s = bot - top
    # Arrow panel at the left, then figure, then the door.
    ax0 = 22
    t.stroke([(ax0 + 6, top + s / 2), (ax0 + s * 0.62, top + s / 2)], s * 0.14)
    t.poly([(ax0 + s * 0.52, top + s * 0.2), (ax0 + s * 0.9, top + s / 2), (ax0 + s * 0.52, top + s * 0.8)])
    t.rect(ax0 + s + 6, top, ax0 + s + 10, bot)  # divider
    fx = ax0 + s + 24
    running_man(t, fx, top + 2, s - 4, s * 0.085)
    dx = fx + s * 0.95
    t.rect(dx, top, dx + s * 0.62, bot)
    t.rect(dx + s * 0.07, top + s * 0.07, dx + s * 0.62 - s * 0.07, bot, fill=0)
    t.rect(dx + s * 0.07, bot - s * 0.07, dx + s * 0.2, bot)  # threshold
    t.text("NOTAUSGANG", F_BOLD, (22, ch * 0.73, cw - 22, ch - 12), spacing=0.06)
    over(c, WHITE, t.array())
    return finish_plate(c, a, 211, dirt=0.2, screw_r=0), None


def prohibition(cv_red: Canvas, cx, cy, r):
    ring = r * 0.16
    cv_red.ellipse(cx, cy, r)
    cv_red.ellipse(cx, cy, r - ring, fill=0)
    k = (r - ring * 0.6) / math.sqrt(2)
    cv_red.stroke([(cx - k, cy - k), (cx + k, cy + k)], ring * 0.95)


def sign_rauchen_verboten(cw, ch):
    c, a = plate(cw, ch, WHITE, 8, 221)
    r = cw * 0.42
    cx, cy = cw / 2, 16 + r
    blk = Canvas(cw, ch)
    # Cigarette with a glowing end and smoke.
    L, T = cx - r * 0.62, cy + r * 0.05
    blk.rect(L, T, cx + r * 0.42, T + r * 0.2)
    blk.rect(cx + r * 0.46, T, cx + r * 0.66, T + r * 0.2)
    for i, ox in enumerate((0.0, 0.12)):
        pts = [(L + r * (0.04 + ox), T - r * (0.06 + 0.1 * j) ) for j in range(0)]
        xs = L + r * (0.05 + ox)
        pts = [(xs + math.sin(j * 0.9 + i) * r * 0.05, T - r * 0.08 - j * r * 0.07) for j in range(7)]
        blk.stroke(pts, r * 0.045)
    red = Canvas(cw, ch)
    prohibition(red, cx, cy, r)
    over(c, BLACK, blk.array())
    over(c, RED, red.array())
    t = Canvas(cw, ch)
    ty = cy + r + 12
    t.text("RAUCHEN", F_BOLD, (16, ty, cw - 16, ty + (ch - ty - 20) / 2 - 2), spacing=0.03)
    t.text("VERBOTEN", F_BOLD, (16, ty + (ch - ty - 20) / 2 + 4, cw - 16, ch - 18), spacing=0.03)
    over(c, BLACK, t.array())
    return finish_plate(c, a, 221, dirt=0.25, screw_r=3.5, screw_inset=10), None


def sign_hochspannung(cw, ch):
    tri_h = cw * 0.88
    c = rgba(ch, cw)
    # Warning triangle: black rim, yellow field, black bolt with arrowhead.
    outer = Canvas(cw, ch)
    m = 4
    pts = [(cw / 2, m), (cw - m, tri_h), (m, tri_h)]
    outer.poly(pts)
    for p in pts:
        outer.ellipse(*p, 6)
    outer.d.line([outer.p(*pts[i]) for i in (0, 1, 2, 0)], fill=255, width=12 * SS)
    inner = Canvas(cw, ch)
    k = 0.16
    cxy = (cw / 2, (m + tri_h * 2) / 3)
    ipts = [(cxy[0] + (x - cxy[0]) * (1 - k * 1.25), cxy[1] + (y - cxy[1]) * (1 - k * 1.25)) for x, y in pts]
    inner.poly(ipts)
    bolt = Canvas(cw, ch)
    s = tri_h
    B = lambda x, y: (cw / 2 + (x - 0.5) * s, m + y * s)
    bolt.poly([B(0.56, 0.3), B(0.47, 0.57), B(0.555, 0.55), B(0.495, 0.74), B(0.465, 0.73), B(0.47, 0.87), B(0.565, 0.75), B(0.535, 0.755),
               B(0.6, 0.5), B(0.515, 0.52), B(0.6, 0.3)])
    oa = outer.array()
    over(c, BLACK, oa)
    over(c, YELLOW, inner.array())
    over(c, BLACK, bolt.array())
    tri_a = oa.copy()
    # The supplementary plate below.
    py = int(tri_h + 14)
    pc, pa = plate(cw, ch - py, YELLOW, 6, 231, border=(5, 3, BLACK))
    t = Canvas(cw, ch - py)
    ph = ch - py
    t.text("ACHTUNG", F_BOLD, (18, 14, cw - 18, 14 + ph * 0.3), spacing=0.04)
    t.text("HOCHSPANNUNG", F_BOLD, (16, 18 + ph * 0.3, cw - 16, 18 + ph * 0.62))
    t.text("LEBENSGEFAHR", F_SEMI, (40, 26 + ph * 0.62, cw - 40, ph - 16), spacing=0.05)
    over(pc, BLACK, t.array())
    c[py:, :, :3] = pc[..., :3]
    c[py:, :, 3] = pa
    a = c[..., 3].copy()
    return finish_plate(c, a, 231, dirt=0.25, scratch_n=20), None


def sign_lager_3(cw, ch):
    c, a = plate(cw, ch, BLUE, 6, 241, border=(5, 2, WHITE))
    t = Canvas(cw, ch)
    t.text("LAGER 3", F_BOLD, (24, 16, cw - 24, ch - 16), spacing=0.12)
    over(c, WHITE, t.array())
    return finish_plate(c, a, 241, dirt=0.3, screw_r=3.5, screw_inset=12), None


def spray(mask: np.ndarray, seed: int, color, wear: float, grain: float = 0.12, overspray: float = 0.08, max_a: float = 0.92, scratch_n: int = 20):
    """Stencil spray paint: slightly soft edges with overspray dots, flaking with wear."""
    h, w = mask.shape
    soft = blur(mask, 0.45)
    over_ = blur(mask, 3.0) * overspray * smoothstep(0.6, 0.9, fbm(w, h, 1.6, seed + 9, 2))
    a = np.maximum(soft, over_)
    a = paint_wear(a, seed, wear, grain, scratch_n)
    col = solid(h, w, color) * (0.95 + 0.07 * fbm(w, h, 30, seed + 5, 3))[..., None]
    return np.dstack([np.clip(col, 0, 1), np.clip(a * max_a, 0, 1)])


def stencil_text(cw, ch, text, color, seed, wear=0.35, spacing=0.08, stretch_y=1.0):
    t = Canvas(cw, ch)
    t.text(text, F_BOLD, (0, 0, cw, ch), stencil=True, spacing=spacing, stretch_y=stretch_y)
    return spray(t.array(), seed, color, wear), None


def stencil_halle_1(cw, ch):
    return stencil_text(cw, ch, "HALLE 1", BLACK, 251)


def stencil_halle_2(cw, ch):
    return stencil_text(cw, ch, "HALLE 2", BLACK, 252)


def site(cw, ch, letter, seed):
    disc = Canvas(cw, ch)
    r = cw / 2 - 2
    disc.ellipse(cw / 2, ch / 2, r)
    ring = Canvas(cw, ch)
    ring.ellipse(cw / 2, ch / 2, r)
    ring.ellipse(cw / 2, ch / 2, r - 9, fill=0)
    t = Canvas(cw, ch)
    t.text(letter, F_BOLD, (cw * 0.27, ch * 0.2, cw * 0.73, ch * 0.8), stencil=True)
    ink = np.clip(ring.array() + t.array(), 0, 1)
    c = rgba(ch, cw)
    base = spray(disc.array(), seed, YELLOW, 0.3, max_a=0.95)
    top = spray(ink, seed + 1, BLACK, 0.3, max_a=0.97)
    over(c, base[..., :3], base[..., 3])
    over(c, top[..., :3], top[..., 3] * np.clip(disc.array() * 1.2, 0, 1))
    return c, None


def site_a(cw, ch):
    return site(cw, ch, "A", 261)


def site_b(cw, ch):
    return site(cw, ch, "B", 262)


def hangar(n, seed):
    return lambda cw, ch: stencil_text(cw, ch, n, TRAFFIC_YELLOW, seed, wear=0.3, spacing=0.1)


def hazard_stripes(cw, ch):
    y, x = np.mgrid[0:ch, 0:cw].astype(np.float32)
    period = ch * 1.2
    ph = ((x + y) / period) % 1.0
    # Antialiased stripes: blend over a pixel's width.
    aa = 1.0 / period * 1.4
    stripe = smoothstep(0.5 - aa, 0.5 + aa, ph) * (1 - smoothstep(1.0 - aa, 1.0, ph)) + (1 - smoothstep(0.0, aa, ph)) * 0
    col = YELLOW[None, None, :] * (1 - stripe[..., None]) + BLACK[None, None, :] * stripe[..., None]
    col = col * (0.92 + 0.12 * fbm(cw, ch, 40, 271, 4))[..., None]
    a = paint_wear(np.ones((ch, cw), np.float32), 272, 0.35, 0.2, 40, scale=30)
    # Grime on the strip.
    g = fbm(cw, ch, 80, 273, 4)
    col = col * (1 - 0.3 * smoothstep(0.5, 0.8, g))[..., None]
    a *= edge_feather(ch, cw, top=0.02, bottom=0.02, left=0.004, right=0.004)
    return np.dstack([np.clip(col, 0, 1), np.clip(a * 0.95, 0, 1)]), None


def floor_arrow(cw, ch):
    t = Canvas(cw, ch)
    head = ch * 0.3
    sw = cw * 0.36
    t.poly([(cw / 2, 0), (cw, head), (cw / 2 + sw / 2, head * 0.86), (cw / 2 + sw / 2, ch), (cw / 2 - sw / 2, ch), (cw / 2 - sw / 2, head * 0.86), (0, head)])
    return road_paint(t.array(), 281, ROAD_WHITE), None


def road_paint(mask, seed, color, wear=0.5, tyre=True, along_x=False):
    """Worn floor paint; `along_x` when traffic runs along the image's x (lines), else along y (arrows, stop)."""
    h, w = mask.shape
    a = paint_wear(blur(mask, 0.4), seed, wear, 0.12, 0, scale=max(w, h) * 0.08)
    if tyre:
        # Rubber scuffs from traffic: darker, lengthwise streaks.
        sx, sy = (3.0, 0.15) if along_x else (0.15, 3.0)
        rub = smoothstep(0.55, 0.8, fbm(w, h, 60, seed + 3, 4, sx=sx, sy=sy))
        a *= 1 - 0.5 * rub
    col = solid(h, w, color) * (0.9 + 0.12 * fbm(w, h, 20, seed + 4, 3))[..., None]
    return np.dstack([np.clip(col, 0, 1), np.clip(a * 0.92, 0, 1)])


def stop_line(cw, ch):
    t = Canvas(cw, ch)
    bar = ch * 0.2
    t.text("STOP", F_BOLD, (cw * 0.12, 0, cw * 0.88, ch - bar - ch * 0.12), spacing=0.12)
    t.rect(0, ch - bar, cw, ch)
    return road_paint(t.array(), 291, ROAD_WHITE, wear=0.45), None


def taxiway_line(cw, ch):
    t = Canvas(cw, ch)
    t.rect(0, 0, cw, ch)
    y = Canvas(cw, ch)
    y.rect(0, ch / 3, cw, ch * 2 / 3)
    blk = road_paint(t.array(), 301, BLACK, wear=0.25, tyre=False)
    yel = road_paint(y.array(), 302, TRAFFIC_YELLOW, wear=0.4, along_x=True)
    c = rgba(ch, cw)
    over(c, blk[..., :3], blk[..., 3] * 0.85)
    over(c, yel[..., :3], yel[..., 3])
    c[..., 3] *= edge_feather(ch, cw, left=0.015, right=0.015)
    return c, None


def sign_parking(cw, ch):
    c, a = plate(cw, ch, BLUE, 14, 311, border=(7, 5, WHITE))
    t = Canvas(cw, ch)
    t.text("P", F_BOLD, (cw * 0.22, ch * 0.16, cw * 0.78, ch * 0.84))
    over(c, WHITE, t.array())
    return finish_plate(c, a, 311, dirt=0.25), None


def paper(cw, ch, seed, color, torn=True):
    """Paper: returns (rgba, shape). Slight creases, a torn corner."""
    shape = Canvas(cw, ch)
    shape.rect(0, 0, cw, ch)
    if torn:
        rng = np.random.default_rng(seed)
        pts = [(cw, ch * 0.9)]
        for i in range(1, 8):
            t = i / 8
            pts.append((cw - t * cw * 0.14 + rng.normal(0, 1.0), ch * 0.9 + t * ch * 0.1 + rng.normal(0, 1.0)))
        pts += [(cw - cw * 0.14, ch), (cw + 2, ch + 2)]
        shape.poly(pts, fill=0)
    a = shape.array()
    c = rgba(ch, cw)
    over(c, color, a)
    return c, a


def paper_finish(c, a, seed, fade=0.12):
    ch, cw = a.shape
    col = c[..., :3]
    col = col * (1 - fade) + fade * np.array([0.93, 0.9, 0.82], np.float32)  # sun-bleached
    y, x = np.mgrid[0:ch, 0:cw].astype(np.float32)
    crease = np.exp(-(((y - ch * 0.5) / 1.2) ** 2)) * 0.1 + np.exp(-(((x - cw * 0.5) / 1.2) ** 2)) * 0.07
    shade = 1.0 - crease - 0.05 * (fbm(cw, ch, 50, seed, 3) - 0.5) * 2
    stain = smoothstep(0.62, 0.8, fbm(cw, ch, 60, seed + 1, 4)) * 0.18
    col = col * shade[..., None] * (1 - stain[..., None] * np.array([0.3, 0.45, 0.7], np.float32))
    c[..., :3] = np.clip(col, 0, 1)
    # Tape at the top corners.
    tape = Canvas(cw, ch)
    for x0, ang in ((0.0, 0.5), (cw, -0.5)):
        cx, cy = x0 + (8 if x0 == 0 else -8), 8
        dx, dy = math.cos(ang) * 16, math.sin(ang) * 16
        nx, ny = -math.sin(ang) * 6, math.cos(ang) * 6
        tape.poly([(cx - dx - nx, cy - dy - ny), (cx + dx - nx, cy + dy - ny), (cx + dx + nx, cy + dy + ny), (cx - dx + nx, cy - dy + ny)])
    over(c, rgb(214, 204, 170), tape.array() * 0.7)
    c[..., 3] = np.maximum(a, tape.array() * 0.7)
    return c


def poster_stadtfest(cw, ch):
    c, a = paper(cw, ch, 321, rgb(236, 120, 40))
    rng = np.random.default_rng(322)
    art = rgba(ch, cw)
    for col, r, x, y in ((rgb(250, 200, 60), 0.34, 0.3, 0.3), (rgb(200, 40, 90), 0.26, 0.72, 0.22), (rgb(40, 90, 160), 0.2, 0.62, 0.52), (rgb(255, 238, 200), 0.1, 0.2, 0.55)):
        s = Canvas(cw, ch)
        s.ellipse(x * cw, y * ch, r * cw)
        over(art, col, s.array())
    conf = Canvas(cw, ch)
    for _ in range(40):
        x, y = rng.uniform(0, cw), rng.uniform(0, ch * 0.6)
        ang = rng.uniform(0, math.pi)
        conf.stroke([(x, y), (x + math.cos(ang) * 5, y + math.sin(ang) * 5)], 2.2)
    over(art, rgb(255, 250, 240), conf.array() * 0.9)
    over(c, art[..., :3], art[..., 3] * a)
    band = Canvas(cw, ch)
    band.rect(0, ch * 0.64, cw, ch)
    over(c, rgb(30, 30, 44), band.array() * a)
    t = Canvas(cw, ch)
    t.text("STADTFEST", F_BOLD, (12, ch * 0.665, cw - 12, ch * 0.8), spacing=0.02)
    t.text("12.–14. JULI", F_SEMI, (20, ch * 0.81, cw - 20, ch * 0.88), spacing=0.04)
    t.text("MARKTPLATZ · LIVE-MUSIK · FLOHMARKT", F_MED, (14, ch * 0.9, cw - 30, ch * 0.935))
    over(c, rgb(250, 246, 236), t.array() * a)
    t2 = Canvas(cw, ch)
    t2.text("EINTRITT FREI", F_BOLD, (cw * 0.14, ch * 0.03, cw * 0.5, ch * 0.07), align="left")
    over(c, rgb(30, 30, 44), t2.array() * a)
    return paper_finish(c, a, 323), None


def flyer_zimmer_frei(cw, ch):
    c, a = paper(cw, ch, 331, rgb(250, 248, 240), torn=False)
    # Tear-off tabs at the bottom, two of them gone.
    tabs_y = ch * 0.72
    n = 7
    tw = cw / n
    cut = Canvas(cw, ch)
    for i in range(n):
        cut.rect(i * tw, tabs_y, i * tw + 0.8, ch)
    for i in (2, 5):
        cut.rect(i * tw + 0.8, tabs_y + 3, (i + 1) * tw, ch)
    a = np.clip(a - cut.array(), 0, 1)
    t = Canvas(cw, ch)
    t.text("Zimmer frei", F_INTER, (10, 14, cw - 10, ch * 0.24), variation="Bold")
    t.text("Ferienwohnung in der Altstadt", F_INTER, (10, ch * 0.3, cw - 10, ch * 0.37), variation="Medium")
    t.text("ab 45 € / Nacht", F_INTER, (10, ch * 0.42, cw - 10, ch * 0.5), variation="SemiBold")
    t.text("ruhig · Frühstück möglich", F_INTER, (10, ch * 0.54, cw - 10, ch * 0.6), variation="Regular")
    for i in range(n):
        if i in (2, 5):
            continue
        m = text_mask("Tel. 0000 12345", F_INTER, 60, variation="Medium").rotate(90, expand=True)
        box_w, box_h = (tw - 3) * SS, (ch - tabs_y - 6) * SS
        k = min(box_w / m.size[0], box_h / m.size[1])
        m = m.resize((max(1, int(m.size[0] * k)), max(1, int(m.size[1] * k))), Image.LANCZOS)
        t.paste(m, i * tw + (tw - m.size[0] / SS) / 2, tabs_y + 3)
    dash = Canvas(cw, ch)
    for x in range(0, int(cw), 4):
        dash.rect(x, tabs_y - 0.5, x + 2, tabs_y + 0.5)
    over(c, rgb(30, 50, 110), t.array() * a)
    over(c, rgb(120, 120, 120), dash.array() * a * 0.6)
    c[..., 3] = a
    return paper_finish(c, a, 332, fade=0.08), None


def sign_hafenbetrieb_nord(cw, ch):
    c, a = plate(cw, ch, NAVY, 6, 341, border=(5, 2, WHITE))
    t = Canvas(cw, ch)
    # A made-up logo: a ring with two waves.
    r = ch * 0.34
    cx, cy = 18 + r, ch / 2
    t.ellipse(cx, cy, r)
    t.ellipse(cx, cy, r * 0.8, fill=0)
    for k, dy in enumerate((-0.18, 0.2)):
        pts = [(cx - r * 0.62 + i * r * 0.124, cy + dy * r + math.sin(i * 0.9) * r * 0.12) for i in range(11)]
        t.stroke(pts, r * 0.14)
    t.text("HAFENBETRIEB NORD", F_BOLD, (cx + r + 20, 18, cw - 24, ch - 18), align="left", spacing=0.1)
    over(c, WHITE, t.array())
    # Rust bleeding from the edges.
    rust = smoothstep(0.62, 0.78, fbm(cw, ch, 20, 342, 4)) * np.clip(1 - blur(a, 6) * 1.25 + 0.2, 0, 1) * a
    over(c, rgb(120, 58, 24), rust * 0.8)
    return finish_plate(c, a, 341, dirt=0.3, screw_r=3.5, screw_inset=11), None


def sign_kai_4(cw, ch):
    c, a = plate(cw, ch, WHITE, 6, 351, border=(5, 3, NAVY))
    t = Canvas(cw, ch)
    t.text("KAI 4", F_BOLD, (18, 14, cw - 18, ch - 14), spacing=0.1)
    over(c, NAVY, t.array())
    return finish_plate(c, a, 351, dirt=0.35, screw_r=3.2, screw_inset=11), None


def street_sign(cw, ch, name, seed):
    """A German street name plate: white on dark blue enamel, a thin white border, the name in condensed bold."""
    c, a = plate(cw, ch, NAVY, 5, seed, border=(4, 2, WHITE))
    t = Canvas(cw, ch)
    t.text(name, F_BOLD, (16, 14, cw - 16, ch - 14), spacing=0.02)
    over(c, WHITE, t.array())
    return finish_plate(c, a, seed, dirt=0.25, screw_r=2.5, screw_inset=9), None


STREETS = {"street_lange_gasse": "Lange Gasse", "street_marktstrasse": "Marktstraße", "street_kirchgasse": "Kirchgasse",
           "street_schmiedgasse": "Schmiedgasse", "street_mauergasse": "Mauergasse", "street_baeckergasse": "Bäckergasse",
           "street_rathausplatz": "Rathausplatz", "street_kirchplatz": "Kirchplatz"}


# ---------------------------------------------------------------------------------------------------------------------
# The catalogue


@dataclass
class Decal:
    name: str
    cell: tuple  # px, multiples of GRID
    width_m: float  # world width of the content rect; height follows its aspect
    blend: str
    surface: str
    tags: list
    fn: Callable
    content: tuple | None = None  # px; default: cell minus padding
    pos: tuple = field(default=(0, 0))


HAFEN, ALT, FLIEGER = "zg_hafen", "zg_altstadt", "zg_fliegerhorst"
CATALOGUE = [
    # Leaks and streaks
    Decal("leak_water_a", (256, 512), 0.7, "multiply", "wall", ["leak", "water", "streak", "tall"], leak_water_a),
    Decal("leak_window", (512, 256), 1.6, "multiply", "wall", ["leak", "water", "streak", "window", ALT], leak_window),
    Decal("leak_rust_pipe", (128, 512), 0.3, "alpha", "wall", ["leak", "rust", "streak", "pipe", "tall", HAFEN], leak_rust_pipe),
    Decal("leak_rust_bolts", (128, 512), 0.35, "alpha", "wall", ["leak", "rust", "streak", "metal", "tall", HAFEN], leak_rust_bolts),
    # Grime
    Decal("grime_base_a", (768, 256), 3.0, "multiply", "wall", ["grime", "dirt", "wall-base"], grime_base_a),
    Decal("grime_base_b", (512, 128), 1.6, "multiply", "wall", ["grime", "dirt", "wall-base"], grime_base_b),
    Decal("damp_base", (512, 128), 2.0, "multiply", "wall", ["damp", "water", "wall-base", ALT], damp_base),
    Decal("moss_base", (512, 128), 1.6, "alpha", "any", ["moss", "wall-base", ALT, HAFEN], moss_base),
    Decal("ivy_a", (256, 512), 1.7, "alpha", "wall", ["ivy", "plant", ALT], ivy_a),
    Decal("ivy_b", (256, 512), 1.7, "alpha", "wall", ["ivy", "plant", ALT], ivy_b),
    Decal("grime_patch_a", (256, 256), 1.0, "multiply", "any", ["grime", "dirt", "patch"], grime_patch_a),
    Decal("grime_patch_b", (256, 256), 0.8, "multiply", "any", ["grime", "smear", "patch"], grime_patch_b),
    # Cracks and damage
    Decal("crack_wall_a", (256, 256), 0.8, "alpha", "wall", ["crack"], crack_wall_a),
    Decal("crack_wall_b", (256, 512), 0.7, "alpha", "wall", ["crack", "tall"], crack_wall_b),
    Decal("crack_floor", (256, 256), 1.2, "alpha", "floor", ["crack"], crack_floor),
    Decal("floor_damage", (256, 256), 1.2, "alpha", "floor", ["crack", "damage", "asphalt"], floor_damage),
    # Floor stains
    Decal("oil_stain_a", (256, 256), 1.2, "alpha", "floor", ["oil", "stain", HAFEN, FLIEGER], oil_stain_a),
    Decal("oil_stain_b", (256, 256), 0.9, "alpha", "floor", ["oil", "stain", "drips", HAFEN, FLIEGER], oil_stain_b),
    Decal("puddle", (256, 256), 1.6, "alpha", "floor", ["water", "puddle", "damp"], puddle),
    Decal("tyre_track", (1024, 128), 3.2, "alpha", "floor", ["tyre", "mud", "track"], tyre_track),
    Decal("skid_mark", (1024, 128), 4.0, "alpha", "floor", ["tyre", "rubber", "skid"], skid_mark),
    Decal("manhole_cover", (256, 256), 1.1, "alpha", "floor", ["manhole", "cobblestone", ALT], manhole_cover),
    Decal("footprints_mud", (128, 512), 0.5, "alpha", "floor", ["footprints", "mud"], footprints_mud),
    # Signs
    Decal("sign_betreten_verboten", (512, 256), 0.5, "alpha", "wall", ["sign", "prohibition", "text"], sign_betreten_verboten, (448, 224)),
    Decal("sign_notausgang", (512, 256), 0.6, "alpha", "wall", ["sign", "exit", "text"], sign_notausgang),
    Decal("sign_rauchen_verboten", (256, 384), 0.25, "alpha", "wall", ["sign", "prohibition", "text"], sign_rauchen_verboten, (224, 320)),
    Decal("sign_hochspannung", (256, 384), 0.3, "alpha", "wall", ["sign", "warning", "electric", "text"], sign_hochspannung, (224, 352)),
    Decal("sign_lager_3", (512, 128), 0.8, "alpha", "wall", ["sign", "text", HAFEN], sign_lager_3),
    Decal("sign_parking", (256, 256), 0.6, "alpha", "wall", ["sign", "parking"], sign_parking),
    Decal("sign_hafenbetrieb_nord", (1024, 128), 3.1, "alpha", "wall", ["sign", "text", "company", HAFEN], sign_hafenbetrieb_nord),
    Decal("sign_kai_4", (256, 128), 1.0, "alpha", "wall", ["sign", "text", HAFEN], sign_kai_4),
    *[Decal(k, (256, 128), 0.75, "alpha", "wall", ["sign", "street", "text", ALT], (lambda cw, ch, n=v, sd=400 + i: street_sign(cw, ch, n, sd)))
      for i, (k, v) in enumerate(STREETS.items())],
    # Stencils and markings
    Decal("stencil_halle_1", (768, 256), 4.0, "alpha", "wall", ["stencil", "text", FLIEGER], stencil_halle_1),
    Decal("stencil_halle_2", (768, 256), 4.0, "alpha", "wall", ["stencil", "text", FLIEGER], stencil_halle_2),
    Decal("site_a", (256, 256), 1.2, "alpha", "any", ["stencil", "bombsite"], site_a),
    Decal("site_b", (256, 256), 1.2, "alpha", "any", ["stencil", "bombsite"], site_b),
    Decal("hangar_01", (256, 256), 1.6, "alpha", "wall", ["stencil", "number", FLIEGER], hangar("01", 263)),
    Decal("hangar_02", (256, 256), 1.6, "alpha", "wall", ["stencil", "number", FLIEGER], hangar("02", 264)),
    Decal("hangar_03", (256, 256), 1.6, "alpha", "wall", ["stencil", "number", FLIEGER], hangar("03", 265)),
    Decal("hazard_stripes", (1024, 128), 2.5, "alpha", "any", ["marking", "hazard", HAFEN, FLIEGER], hazard_stripes),
    Decal("floor_arrow", (128, 512), 0.5, "alpha", "floor", ["marking", "road", "arrow"], floor_arrow),
    Decal("stop_line", (512, 256), 3.0, "alpha", "floor", ["marking", "road", "text"], stop_line),
    Decal("taxiway_line", (1024, 128), 3.0, "alpha", "floor", ["marking", "taxiway", FLIEGER], taxiway_line),
    # Paper
    Decal("poster_stadtfest", (256, 384), 0.59, "alpha", "wall", ["poster", "paper", "text", ALT], poster_stadtfest, (224, 320)),
    Decal("flyer_zimmer_frei", (256, 256), 0.15, "alpha", "wall", ["poster", "paper", "text", ALT], flyer_zimmer_frei, (160, 224)),
]


# ---------------------------------------------------------------------------------------------------------------------
# Packing and assembly


def pack(decals: list[Decal]) -> None:
    """First-fit on the 128 px grid, biggest first; fails loudly if the set doesn't fit."""
    n, nh = ATLAS // GRID, ATLAS_H // GRID
    orders = [
        lambda d: (-d.cell[0] * d.cell[1], -d.cell[0], d.name),
        lambda d: (-d.cell[0], -d.cell[1], d.name),
        lambda d: (-d.cell[1], -d.cell[0], d.name),
    ]
    for order in orders:
        used = np.zeros((nh, n), bool)
        ok = True
        for d in sorted(decals, key=order):
            w, h = d.cell[0] // GRID, d.cell[1] // GRID
            spot = next(((x, y) for y in range(nh - h + 1) for x in range(n - w + 1) if not used[y : y + h, x : x + w].any()), None)
            if spot is None:
                ok = False
                break
            x, y = spot
            used[y : y + h, x : x + w] = True
            d.pos = (x * GRID, y * GRID)
        if ok:
            free = (~used).sum()
            print(f"packed {len(decals)} decals, {free} free {GRID}px cells of {n * nh}")
            return
    sys.exit("error: the decals don't fit the atlas")


def dilate(col: np.ndarray, a: np.ndarray) -> np.ndarray:
    """Push-pull: fills the RGB of transparent texels with a smooth continuation of the nearby opaque colour."""
    w = (a > 0.02).astype(np.float32)
    pyr = [(col * w[..., None], w)]
    while min(pyr[-1][1].shape) > 1:
        c, ww = pyr[-1]
        h2, w2 = (c.shape[0] + 1) // 2, (c.shape[1] + 1) // 2
        c = np.pad(c, ((0, h2 * 2 - c.shape[0]), (0, w2 * 2 - c.shape[1]), (0, 0)))
        ww = np.pad(ww, ((0, h2 * 2 - ww.shape[0]), (0, w2 * 2 - ww.shape[1])))
        c = c.reshape(h2, 2, w2, 2, 3).sum((1, 3))
        ww = ww.reshape(h2, 2, w2, 2).sum((1, 3))
        pyr.append((c, ww))
    c, ww = pyr[-1]
    filled = c / np.maximum(ww[..., None], 1e-6)
    for c, ww in reversed(pyr[:-1]):
        up = np.repeat(np.repeat(filled, 2, 0), 2, 1)[: c.shape[0], : c.shape[1]]
        own = c / np.maximum(ww[..., None], 1e-6)
        k = np.clip(ww, 0, 1)[..., None]
        filled = own * k + up * (1 - k)
    return np.where((a > 0.02)[..., None], col, filled)


def build(no_ktx: bool, preview: Path | None) -> None:
    for asset in ACG:
        fetch(asset)
    pack(CATALOGUE)
    color = np.zeros((ATLAS_H, ATLAS, 4), np.float32)
    normal = np.zeros((ATLAS_H, ATLAS, 3), np.float32)
    normal[...] = (0.5, 0.5, 1.0)
    meta = {}
    for d in CATALOGUE:
        cw, ch = d.content or (d.cell[0] - 2 * PAD, d.cell[1] - 2 * PAD)
        assert cw <= d.cell[0] - 2 * PAD and ch <= d.cell[1] - 2 * PAD, d.name
        print(f"  {d.name:24s} cell {d.cell[0]}×{d.cell[1]}  content {cw}×{ch}", flush=True)
        img, nrm = d.fn(cw, ch)
        img = np.clip(np.asarray(img, np.float32), 0, 1)
        assert img.shape == (ch, cw, 4), (d.name, img.shape)
        x0 = d.pos[0] + (d.cell[0] - cw) // 2
        y0 = d.pos[1] + (d.cell[1] - ch) // 2
        # The cell: content in the middle, transparent padding with dilated colour.
        cell = np.zeros((d.cell[1], d.cell[0], 4), np.float32)
        ox, oy = x0 - d.pos[0], y0 - d.pos[1]
        cell[oy : oy + ch, ox : ox + cw] = img
        cell[..., :3] = dilate(cell[..., :3], cell[..., 3])
        if d.blend == "multiply":
            # Keep fully transparent texels neutral-ish so any bleed can only lighten towards "no change".
            pass
        color[d.pos[1] : d.pos[1] + d.cell[1], d.pos[0] : d.pos[0] + d.cell[0]] = cell
        if nrm is not None:
            a = img[..., 3:4]
            n = np.asarray(nrm, np.float32)[..., :3] * a + np.array([0.5, 0.5, 1.0], np.float32) * (1 - a)
            normal[y0 : y0 + ch, x0 : x0 + cw] = n
        rect = [x0 / ATLAS, y0 / ATLAS_H, (x0 + cw) / ATLAS, (y0 + ch) / ATLAS_H]
        meta[d.name] = {
            "rect": [round(v, 6) for v in rect],
            "size": [d.width_m, round(d.width_m * ch / cw, 3)],
            "blend": d.blend,
            "surface": d.surface,
            "tags": d.tags,
        }
    BUILD.mkdir(parents=True, exist_ok=True)
    col8 = (np.clip(color, 0, 1) * 255 + 0.5).astype(np.uint8)
    Image.fromarray(col8, "RGBA").save(BUILD / "color.png")
    nrm8 = (np.clip(normal, 0, 1) * 255 + 0.5).astype(np.uint8)
    Image.fromarray(nrm8, "RGB").save(BUILD / "normal.png")
    OUT.mkdir(parents=True, exist_ok=True)
    doc = {
        "atlas": [ATLAS, ATLAS_H],
        "notes": {
            "rect": "u0, v0, u1, v1 in 0..1 with v down (image rows); the rect is the decal's content, padded by >= 16 px of transparent texels in its cell",
            "size": "suggested world size in metres (width, height) of the rect",
            "alpha": "out = mix(base, rgb, a), straight alpha",
            "multiply": "out = base * mix(1, rgb, a): rgb is the multiply colour, a its strength",
            "normal": "normal.ktx2 (BC5, OpenGL convention) has the same layout; flat where a decal has no relief",
        },
        "decals": meta,
    }
    (OUT / "decals.json").write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"wrote {OUT / 'decals.json'} ({len(meta)} decals)")
    if not no_ktx:
        cmd = ["cargo", "run", "--release", "-q", "-p", "zg_tex", "--"]
        subprocess.run(cmd + ["rgba", str(BUILD / "color.png"), str(OUT / "color.ktx2"), "--max", str(ATLAS_H)], cwd=REPO, check=True)
        subprocess.run(cmd + ["normal", str(BUILD / "normal.png"), str(OUT / "normal.ktx2"), "--max", str(ATLAS_H)], cwd=REPO, check=True)
    if preview:
        write_previews(color, meta, preview)


def write_previews(color: np.ndarray, meta: dict, out: Path) -> None:
    """The atlas composited over mid grey and over brick, with each decal's blend mode."""
    out.mkdir(parents=True, exist_ok=True)
    bricks = next((HERE / ".cache" / "acg" / "Bricks090").glob("*_Color.jpg"), None)
    bases = {"grey": np.full((ATLAS_H, ATLAS, 3), 0.5, np.float32)}
    if bricks:
        b = Image.open(bricks).convert("RGB").resize((1024, 1024), Image.LANCZOS)
        tile = Image.new("RGB", (ATLAS, ATLAS_H))
        for y in range(0, ATLAS_H, 1024):
            for x in range(0, ATLAS, 1024):
                tile.paste(b, (x, y))
        bases["brick"] = np.asarray(tile, np.float32) / 255.0
    mult = np.zeros((ATLAS_H, ATLAS), bool)
    for m in meta.values():
        if m["blend"] == "multiply":
            u0, v0, u1, v1 = (int(round(v * k_)) for v, k_ in zip(m["rect"], (ATLAS, ATLAS_H, ATLAS, ATLAS_H)))
            mult[v0 - PAD : v1 + PAD, u0 - PAD : u1 + PAD] = True
    a = color[..., 3:4]
    for name, base in bases.items():
        alpha_out = base * (1 - a) + color[..., :3] * a
        mul_out = base * (1 - a + a * color[..., :3])
        img = np.where(mult[..., None], mul_out, alpha_out)
        im = Image.fromarray((np.clip(img, 0, 1) * 255 + 0.5).astype(np.uint8))
        dr = ImageDraw.Draw(im)
        f = font(F_MED, 18)
        for k, m in meta.items():
            u0, v0, u1, v1 = (v * k_ for v, k_ in zip(m["rect"], (ATLAS, ATLAS_H, ATLAS, ATLAS_H)))
            dr.rectangle([u0 - 1, v0 - 1, u1, v1], outline=(0, 200, 255))
            dr.text((u0 + 2, v1 + 1), k, fill=(0, 220, 255), font=f)
        im.save(out / f"preview_{name}.png")
        print(f"preview {out / f'preview_{name}.png'}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--no-ktx", action="store_true", help="only write the PNGs and decals.json")
    ap.add_argument("--preview", type=Path, help="write preview_grey.png / preview_brick.png into this directory")
    args = ap.parse_args()
    build(args.no_ktx, args.preview)


if __name__ == "__main__":
    main()
