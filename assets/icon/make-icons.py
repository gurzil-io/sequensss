"""Generates the icons for every platform from assets/icon/source.png (snake on a white background).

    python3 -m venv .venv && .venv/bin/pip install pillow numpy scipy
    .venv/bin/python assets/icon/make-icons.py
"""
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from scipy import ndimage as ndi

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
RES = ROOT / "android/app/src/main/res"


def cutout(src: Image.Image) -> Image.Image:
    """Snake cut out (white connected to the border → transparent, eyes kept), cropped."""
    rgb = np.asarray(src.convert("RGB")).astype(np.float32)
    light = rgb.min(axis=2)
    lab, _ = ndi.label(light > 230)
    border = np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))
    bg = np.isin(lab, border[border > 0])
    # Anti-aliased edge: black outline blended with white → alpha from lightness.
    near = ndi.binary_dilation(bg, iterations=3) & ~bg
    alpha = np.where(bg, 0.0, 1.0)
    alpha[near] = np.clip((250.0 - light[near]) / 210.0, 0.0, 1.0)
    a = alpha[..., None]
    color = np.where(a > 0, (rgb - (1 - a) * 255.0) / np.maximum(a, 1e-3), 0)
    out = np.dstack([np.clip(color, 0, 255), alpha * 255]).astype(np.uint8)
    img = Image.fromarray(out, "RGBA")
    return img.crop(img.getbbox())


def fit(snake: Image.Image, canvas: int, box: float) -> Image.Image:
    """Snake centered in a transparent square, its largest side = box × canvas."""
    s = box * canvas / max(snake.size)
    sn = snake.resize((round(snake.width * s), round(snake.height * s)), Image.LANCZOS)
    out = Image.new("RGBA", (canvas, canvas))
    out.alpha_composite(sn, ((canvas - sn.width) // 2, (canvas - sn.height) // 2))
    return out


def tile(snake: Image.Image, size: int, inset: float, radius: float, shadow: bool, box: float) -> Image.Image:
    """White rounded square (inset = margin on each side), snake at box × the square."""
    out = Image.new("RGBA", (size, size))
    m = round(size * inset)
    rect = (m, m, size - m - 1, size - m - 1)
    r = round((size - 2 * m) * radius)
    if shadow:
        sh = Image.new("RGBA", (size, size))
        ImageDraw.Draw(sh).rounded_rectangle((m, m + size // 100, size - m - 1, size - m - 1 + size // 100), r, fill=(0, 0, 0, 90))
        out.alpha_composite(sh.filter(ImageFilter.GaussianBlur(size / 80)))
    ImageDraw.Draw(out).rounded_rectangle(rect, r, fill="white")
    inner = size - 2 * m
    out.alpha_composite(fit(snake, inner, box), (m, m))
    return out


snake = cutout(Image.open(HERE / "source.png"))

# Desktop (window, Windows, Linux, README): full-size rounded square.
desk = tile(snake, 1024, 0.0, 0.18, shadow=False, box=0.86)
desk.save(HERE / "sequensss.png")
desk.resize((256, 256), Image.LANCZOS).save(HERE / "sequensss-256.png")
desk.save(HERE / "sequensss.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

# macOS: Big Sur grid (824 px square in 1024, ~22.5% radius, drop shadow).
tile(snake, 1024, 100 / 1024, 0.225, shadow=True, box=0.78).save(HERE / "sequensss.icns")

# Android: adaptive icon, transparent foreground (66/108 dp safe zone), white background.
for dpi, px in {"mdpi": 108, "hdpi": 162, "xhdpi": 216, "xxhdpi": 324, "xxxhdpi": 432}.items():
    d = RES / f"mipmap-{dpi}"
    d.mkdir(parents=True, exist_ok=True)
    fit(snake, px * 4, 0.58).resize((px, px), Image.LANCZOS).save(d / "ic_launcher_foreground.png")
print("OK")
