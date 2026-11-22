"""Generates the icons for every platform from assets/icon/source.png (snake on a white background),
and the in-app splash: a transparent snake plus the eye and connector pin positions measured
from the image (crates/seq-ui/src/splash_geom.rs), so the glows line up with the drawing.

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
UI = ROOT / "crates/seq-ui"
SPLASH_H = 720


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


def glow_cutout(src: Image.Image) -> Image.Image:
    """Snake for dark grounds: inside its black outline, opaque; outside, the soft glow painted
    on white becomes translucent color (white → transparent), so no light halo is left. Cropped."""
    rgb = np.asarray(src.convert("RGB")).astype(np.float32)
    body = ndi.binary_fill_holes(rgb.max(axis=2) < 140)
    body = ndi.binary_opening(body, iterations=2)
    # Over white, the least opaque color that explains a pixel has alpha = 1 - min(rgb).
    alpha = 1.0 - rgb.min(axis=2) / 255.0
    alpha = np.clip((alpha - 0.06) / 0.94, 0.0, 1.0)
    alpha[body] = 1.0
    a = alpha[..., None]
    color = np.where(a > 0, (rgb - (1 - a) * 255.0) / np.maximum(a, 1e-3), 0)
    out = np.dstack([np.clip(color, 0, 255), alpha * 255]).astype(np.uint8)
    img = Image.fromarray(out, "RGBA")
    return img.crop(Image.fromarray((alpha > 0.03).astype(np.uint8) * 255).getbbox())


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


def blobs(mask: np.ndarray, min_px: int, max_px: int) -> list[tuple[float, float, float]]:
    """(x, y, radius) of the connected components of `mask` with min_px..max_px pixels."""
    lab, n = ndi.label(mask)
    sizes = ndi.sum(mask, lab, range(1, n + 1))
    out = []
    for i, size in enumerate(sizes, 1):
        if min_px <= size <= max_px:
            y, x = ndi.center_of_mass(lab == i)
            out.append((x, y, float(np.sqrt(size / np.pi))))
    return out


def measure(snake: Image.Image) -> tuple[list, list]:
    """Eyes (bright yellow blobs) and connector pins (small orange dots on the dark face)."""
    px = np.asarray(snake).astype(np.int32)
    r, g, b, a = px[..., 0], px[..., 1], px[..., 2], px[..., 3]
    eyes = blobs((a == 255) & (r > 240) & (g > 220) & (b < 170), 300, 10**6)
    dots = blobs((a == 255) & (r > 150) & (g > 70) & (b < 100) & (r - b > 90), 15, 150)
    # The pins are the tightest group of five dots surrounded by the black face of the connector.
    dark = px[..., :3].max(axis=2) < 70
    yy, xx = np.ogrid[: dark.shape[0], : dark.shape[1]]
    dots = [(x, y, rad) for x, y, rad in dots if dark[(np.hypot(xx - x, yy - y) - rad - 6.5) ** 2 < 12].mean() > 0.5]
    groups = [sorted(dots, key=lambda q: np.hypot(q[0] - p[0], q[1] - p[1]))[:5] for p in dots]
    pins = min(groups, key=lambda g: max(np.hypot(q[0] - g[0][0], q[1] - g[0][1]) for q in g))
    assert len(eyes) == 2, eyes
    assert len(pins) == 5, pins
    # Pins light up clockwise (screen coordinates), starting from the top one.
    cx = sum(p[0] for p in pins) / 5
    cy = sum(p[1] for p in pins) / 5
    pins.sort(key=lambda p: (np.arctan2(p[1] - cy, p[0] - cx) + np.pi / 2) % (2 * np.pi))
    eyes.sort()
    return eyes, pins


def splash(snake: Image.Image) -> None:
    """Splash asset (seq-ui) and its geometry, normalised to the image (radii to its height)."""
    eyes, pins = measure(snake)
    s = SPLASH_H / snake.height
    sn = snake.resize((round(snake.width * s), SPLASH_H), Image.LANCZOS)
    (UI / "assets").mkdir(exist_ok=True)
    sn.save(UI / "assets/splash-snake.png", optimize=True)
    w, h = snake.size

    def pts(name: str, items: list) -> str:
        rows = "".join(f"    [{x / w:.4f}, {y / h:.4f}, {r / h:.4f}],\n" for x, y, r in items)
        return f"pub const {name}: [[f32; 3]; {len(items)}] = [\n{rows}];\n"

    (UI / "src/splash_geom.rs").write_text(
        "//! Generated by assets/icon/make-icons.py from assets/icon/source.png: do not edit.\n"
        "//! Positions in the splash image, `[x, y, radius]`: x and y as fractions of its width and\n"
        "//! height, the radius as a fraction of its height.\n\n"
        f"/// Width / height of the image.\npub const ASPECT: f32 = {w / h:.4f};\n"
        "/// Bright core of each eye, left to right.\n" + pts("EYES", eyes) +
        "/// Pins of the MIDI connector, in lighting order (clockwise from the top).\n" + pts("PINS", pins)
    )


src = Image.open(HERE / "source.png")
snake = cutout(src)

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

# In-app splash (seq-ui): drawn on the dark background.
splash(glow_cutout(src))
print("OK")
