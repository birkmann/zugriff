"""Painted steel in the maps' colours (metal_painted_red/blue/green): ambientCG's PaintedMetal012 (clean white paint
with a few chips down to rusty steel) recoloured, the chips kept as they are. (The red and blue sets were
PaintedMetal004/002: scratched all over, which from a few metres reads as camouflage.)

    python3 tools/textures/variants/painted_metal.py
"""
import os
import subprocess

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
C = os.path.join(HERE, "..", ".cache")
T = os.path.join(HERE, "..", "..", "..", "assets", "textures")
BIN = os.path.join(HERE, "..", "..", "..", "..", "..", "target", "release", "zg-tex")
SRC = os.path.join(C, "acg", "PaintedMetal012", "PaintedMetal012_2K-JPG")

# Mean sRGB of the paint.
COLOURS = {"red": (150, 36, 30), "blue": (38, 70, 130), "green": (62, 88, 58)}


def lin(c):
    c = c / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055) * 255


def main():
    src = lin(np.asarray(Image.open(SRC + "_Color.jpg").convert("RGB")).astype(np.float64))
    lum = src @ np.array([0.2126, 0.7152, 0.0722])
    chroma = src.max(axis=2) - src.min(axis=2)
    # Paint: bright and colourless; chips: darker or rusty.
    paint = np.clip((lum - 0.22) / 0.12, 0, 1) * np.clip(1 - (chroma - 0.05) * 12, 0, 1)
    shade = np.clip(lum / np.percentile(lum[paint > 0.9], 60), 0.8, 1.08)
    for name, rgb in COLOURS.items():
        target = lin(np.array(rgb, dtype=np.float64))
        # Chips read as worn, darker paint with a little of the bare steel in them, not as rust spots: on a tank or
        # a door a few metres across, full-strength rust chips made a camouflage pattern.
        chip = target[None, None, :] * 0.8 + src * 0.08
        # (the paint's own shading, halved: the source's large-scale mottling read as blotches on big blocks)
        soft = 1.0 + (shade - 1.0) * 0.5
        col = target[None, None, :] * soft[..., None] * paint[..., None] + chip * (1 - paint[..., None])
        png = os.path.join(C, "graded", f"metal_painted_{name}_color.png")
        img = srgb(col).astype(np.uint8)
        Image.fromarray(img).save(png)
        d = os.path.join(T, f"metal_painted_{name}")
        os.makedirs(d, exist_ok=True)
        for args in (["color", png, f"{d}/color.ktx2"], ["normal", SRC + "_NormalGL.jpg", f"{d}/normal.ktx2"],
                     ["orm", SRC + "_AmbientOcclusion.jpg" if os.path.exists(SRC + "_AmbientOcclusion.jpg") else "-", SRC + "_Roughness.jpg",
                      SRC + "_Metalness.jpg" if os.path.exists(SRC + "_Metalness.jpg") else "-", f"{d}/orm.ktx2"]):
            subprocess.run([BIN] + args, check=True, capture_output=True)
        print(f"metal_painted_{name}: mean sRGB {tuple(int(v) for v in img.reshape(-1, 3).mean(axis=0))}")


main()
