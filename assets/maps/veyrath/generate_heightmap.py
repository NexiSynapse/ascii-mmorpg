"""Create the deterministic grayscale heightmap used for Veyrath's terrain pass.

Azgaar's Image Converter treats black as ocean and luminosity as elevation. The
image is an authoring input, not a game texture; the editor still regenerates
cells, rivers, burgs, states, and biomes from it when the conversion is
completed.
"""

from pathlib import Path
import math

import numpy as np
from PIL import Image, ImageDraw, ImageFilter


WIDTH, HEIGHT = 1600, 1100
SEED = 731204
OUT = Path(__file__).with_name("veyrath-heightmap.png")


def ridge_field(x, y, points, amplitude, width):
    field = np.zeros_like(x, dtype=np.float32)
    for px, py, strength in points:
        distance = ((x - px) ** 2 + (y - py) ** 2) / (2.0 * width * width)
        field += amplitude * strength * np.exp(-distance)
    return field


def make_noise(rng, width, height, small_width, small_height):
    small = rng.integers(0, 256, size=(small_height, small_width), dtype=np.uint8)
    image = Image.fromarray(small, mode="L").resize((width, height), Image.Resampling.BICUBIC)
    return np.asarray(image, dtype=np.float32) / 255.0 - 0.5


def main():
    rng = np.random.default_rng(SEED)
    x, y = np.meshgrid(np.arange(WIDTH, dtype=np.float32), np.arange(HEIGHT, dtype=np.float32))

    # One broad mainland leaves an open southern/eastern sea. The small angular
    # perturbation keeps the coast from reading as a perfect ellipse while the
    # two notches create useful bays for future ports and marsh outlets.
    nx = (x - 800.0) / 680.0
    ny = (y - 520.0) / 450.0
    radius = np.sqrt(nx * nx + ny * ny)
    angle = np.arctan2(ny, nx)
    coast_wobble = (
        0.035 * np.sin(angle * 5.0 + 0.7)
        + 0.022 * np.sin(angle * 11.0 - 1.2)
        + 0.012 * np.sin(angle * 19.0 + 2.1)
    )
    land = radius < 1.0 + coast_wobble

    # Recess the east-facing coast and the south-central shore into sheltered
    # bays without splitting the mainland.
    east_bay = ((x - 1390.0) / 170.0) ** 2 + ((y - 620.0) / 125.0) ** 2 < 1.0
    south_bay = ((x - 850.0) / 230.0) ** 2 + ((y - 900.0) / 90.0) ** 2 < 1.0
    land &= ~(east_bay | south_bay)

    # Low-frequency relief noise supplies natural variation without turning the
    # planned basins into a noisy per-cell texture.
    noise = 5.0 * make_noise(rng, WIDTH, HEIGHT, 32, 22)
    fine_noise = 1.5 * make_noise(rng, WIDTH, HEIGHT, 96, 66)

    # Keep the inhabited interior well above the converter's darkest ocean
    # colors. The shoreline still fades to black, but the central basin and fen
    # remain land after auto-assignment by luminosity.
    height = 86.0 + 16.0 * (1.0 - y / HEIGHT) + noise + fine_noise

    # Broken northern mountain crescent: high peaks north of the heartland and
    # two deliberate gaps for the principal passes.
    mountain_points = []
    for px in np.linspace(260.0, 1280.0, 33):
        py = 235.0 + 0.00048 * (px - 800.0) ** 2
        gap = (abs(px - 570.0) < 60.0) or (abs(px - 1035.0) < 55.0)
        if not gap:
            strength = 0.82 + 0.18 * math.sin(px * 0.055)
            mountain_points.append((px, py, strength))
    height += ridge_field(x, y, mountain_points, 106.0, 57.0)

    # Western Gallowmere uplands and the southern Greyfall valley shoulders.
    western_points = [(305, 360, 0.8), (390, 420, 1.0), (485, 470, 0.9), (520, 555, 0.72)]
    height += ridge_field(x, y, western_points, 45.0, 105.0)
    southern_points = [(470, 760, 0.7), (640, 820, 0.8), (1030, 785, 0.78), (1190, 730, 0.62)]
    height += ridge_field(x, y, southern_points, 28.0, 82.0)

    # Keep the Veyrath Heartland readable as a low central basin and the eastern
    # Hollow Fen as a broad, nearly level floodplain.
    heartland = np.exp(-(((x - 790.0) / 310.0) ** 2 + ((y - 515.0) / 210.0) ** 2))
    fen = np.exp(-(((x - 1160.0) / 250.0) ** 2 + ((y - 525.0) / 250.0) ** 2))
    height -= 8.0 * heartland
    height -= 6.0 * fen

    # Four shallow inland basins give the regenerated drainage room to form
    # notable lakes while remaining above the black ocean value.
    for px, py, rx, ry in [(600, 420, 48, 35), (820, 335, 54, 34), (1080, 505, 66, 42), (870, 735, 52, 36)]:
        basin = np.exp(-(((x - px) / rx) ** 2 + ((y - py) / ry) ** 2))
        height -= 15.0 * basin

    # Fade the outer shoreline toward zero so the converter gets a clean ocean
    # boundary rather than a hard bright ring.
    shoreline = np.clip((1.02 - radius) / 0.085, 0.0, 1.0)
    height *= shoreline
    height[~land] = 0.0
    height[land & (radius < 0.95)] = np.maximum(height[land & (radius < 0.95)], 68.0)
    height = np.clip(height, 0.0, 230.0).astype(np.uint8)

    # A tiny blur suppresses single-pixel color islands while retaining the
    # ridge/basin structure that the converter uses for elevation.
    image = Image.fromarray(height, mode="L").filter(ImageFilter.GaussianBlur(radius=1.1))
    image.save(OUT, optimize=True)

    # Add a small legend-free preview beside the input for quick human review.
    preview = image.resize((800, 550), Image.Resampling.LANCZOS)
    preview.save(OUT.with_name("veyrath-heightmap-preview.png"), optimize=True)
    print(f"wrote {OUT} ({WIDTH}x{HEIGHT})")
    print(f"land pixels: {int(land.sum())} ({land.mean():.1%})")


if __name__ == "__main__":
    main()
