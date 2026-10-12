"""Container paint: Poly Haven's box-profile sheet (a red trapezoidal-rib sheet with rust spots, its ribs at a
container's pitch) recoloured into shipping-container colours, keeping its wear and rib shading. Normal and ORM come
from the same source. (Until 2026-09-29 the base was the graded corrugated iron, whose round waves and blotchy paint
read as camouflage.)

    python3 tools/textures/variants/containers.py      # writes assets/textures/container_<colour>/
"""
import os
import subprocess

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
C = os.path.join(HERE, "..", ".cache")
T = os.path.join(HERE, "..", "..", "..", "assets", "textures")
BIN = os.path.join(HERE, "..", "..", "..", "..", "..", "target", "release", "zg-tex")
SRC = os.path.join(C, "ph", "box_profile_metal_sheet")

# Mean sRGB colour of each paint.
COLOURS = {
    "red": (132, 44, 36),
    "blue": (36, 66, 118),
    "green": (66, 86, 58),
    "grey": (118, 121, 119),
    "orange": (158, 88, 42),
}


def srgb_to_linear(c):
    c = c / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055) * 255


def main() -> None:
    base = np.asarray(Image.open(os.path.join(SRC, "box_profile_metal_sheet_diff_2k.jpg")).convert("RGB")).astype(np.float64)
    lin = srgb_to_linear(base)
    lum = lin @ np.array([0.2126, 0.7152, 0.0722])
    shade = lum / lum.mean()
    # Where the sheet is far from its paint colour (rust, bare spots), keep some of the original.
    chroma = np.abs(lin - lin.mean(axis=(0, 1))).sum(axis=2)
    wear = np.clip((chroma - 0.05) * 4, 0, 0.6)[..., None]
    out_means = {}
    for name, rgb in COLOURS.items():
        target = srgb_to_linear(np.array(rgb, dtype=np.float64))
        paint = target[None, None, :] * shade[..., None]
        col = paint * (1 - wear) + lin * wear
        img = linear_to_srgb(col).astype(np.uint8)
        png = os.path.join(C, "graded", f"container_{name}_color.png")
        Image.fromarray(img).save(png)
        out_means[name] = tuple(int(v) for v in img.reshape(-1, 3).mean(axis=0))
        d = os.path.join(T, f"container_{name}")
        os.makedirs(d, exist_ok=True)
        for args in (["color", png, f"{d}/color.ktx2"],
                     ["normal", os.path.join(SRC, "box_profile_metal_sheet_nor_gl_2k.jpg"), f"{d}/normal.ktx2"],
                     ["orm", os.path.join(SRC, "box_profile_metal_sheet_ao_2k.jpg"), os.path.join(SRC, "box_profile_metal_sheet_rough_2k.jpg"), "-", f"{d}/orm.ktx2"]):
            subprocess.run([BIN] + args, check=True, capture_output=True)
    for name, m in out_means.items():
        print(f"container_{name}: mean sRGB {m}")


main()
