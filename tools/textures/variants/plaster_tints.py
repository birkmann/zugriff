"""Pastel facade plasters (cream, pink, pale blue, sage) from Poly Haven's red_plaster_weathered: its luminance detail
(fine streaks, mottling) kept, its colour replaced, so every house colour has the same well-behaved grain. Normal and
roughness from the same set.

    python3 tools/textures/variants/plaster_tints.py
"""
import os
import subprocess

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
C = os.path.join(HERE, "..", ".cache")
T = os.path.join(HERE, "..", "..", "..", "assets", "textures")
BIN = os.path.join(HERE, "..", "..", "..", "..", "..", "target", "release", "zg-tex")
SRC = os.path.join(C, "ph", "red_plaster_weathered")

COLOURS = {"plaster_cream": (214, 198, 164), "plaster_pink": (206, 160, 138), "plaster_sky": (160, 180, 186), "plaster_sage": (170, 178, 146)}


def lin(c):
    c = c / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055) * 255


def find(key):
    return next(os.path.join(SRC, f) for f in sorted(os.listdir(SRC)) if key in f.lower())


def main():
    src = lin(np.asarray(Image.open(find("_diff_")).convert("RGB")).astype(np.float64))
    lum = src @ np.array([0.2126, 0.7152, 0.0722])
    detail = lum / lum.mean()
    detail = 1.0 + (detail - 1.0) * 0.7  # a little calmer than the source
    for name, rgb in COLOURS.items():
        col = lin(np.array(rgb, dtype=np.float64))[None, None, :] * detail[..., None]
        png = os.path.join(C, "graded", f"{name}_color.png")
        img = srgb(col).astype(np.uint8)
        Image.fromarray(img).save(png)
        d = os.path.join(T, name)
        os.makedirs(d, exist_ok=True)
        for args in (["color", png, f"{d}/color.ktx2"], ["normal", find("_nor_gl_"), f"{d}/normal.ktx2"],
                     ["orm", find("_ao_"), find("_rough_"), "-", f"{d}/orm.ktx2"]):
            subprocess.run([BIN] + args, check=True, capture_output=True)
        print(f"{name}: mean sRGB {tuple(int(v) for v in img.reshape(-1, 3).mean(axis=0))}")


main()
