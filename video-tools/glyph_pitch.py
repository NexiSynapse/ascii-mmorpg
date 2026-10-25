"""Glyph-lattice pitch measurement for the reference captures.

Answers one question objectively: do different screen regions render on
different character-cell densities (the 'more dots in some places'
hypothesis)? Text on a lattice produces strong autocorrelation peaks at
the cell pitch; we measure the dominant pitch per region (whole frame,
top UI band, world band, bottom band) per frame.

Usage: python glyph_pitch.py <frame.png> [...] [--regions]
"""
import sys
import pathlib
import numpy as np
import cv2


def ink_mask(gray: np.ndarray) -> np.ndarray:
    """Binary ink mask: pixels notably brighter than the local dark bg."""
    blur = cv2.medianBlur(gray, 3)
    _, mask = cv2.threshold(blur, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    return mask


def dominant_period_1d(signal: np.ndarray, lo: int, hi: int) -> tuple[int, float]:
    """Dominant repeating period of a 1D ink-density profile, in pixels.

    Uses autocorrelation; returns (period, score 0..1) where score is the
    peak-to-mean ratio of the correlation outside the zero-lag zone.
    """
    profile = signal.astype(np.float64)
    profile -= profile.mean()
    if profile.std() < 1e-6:
        return 0, 0.0
    corr = np.correlate(profile, profile, mode="full")[len(profile) - 1 :]
    corr /= corr[0] if corr[0] != 0 else 1.0
    zone = corr[lo:hi]
    if zone.size == 0:
        return 0, 0.0
    peak_idx = int(np.argmax(zone)) + lo
    peak = corr[peak_idx]
    mean = float(zone.mean())
    score = (peak - mean) / (mean if mean != 0 else 1e-9)
    return peak_idx, float(max(0.0, score))


def region_pitch(mask: np.ndarray, lo: int = 4, hi: int = 40) -> dict:
    """Dominant x-pitch and y-pitch (px) of an ink mask region."""
    col_density = mask.sum(axis=0).astype(np.float64)
    row_density = mask.sum(axis=1).astype(np.float64)
    px, sx = dominant_period_1d(col_density, lo, hi)
    py, sy = dominant_period_1d(row_density, lo, hi)
    return {"x_pitch": px, "x_score": round(sx, 2), "y_pitch": py, "y_score": round(sy, 2)}


def analyze(path: str) -> None:
    img = cv2.imread(path)
    if img is None:
        print(f"!! cannot read {path}")
        return
    gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
    h, w = gray.shape
    mask = ink_mask(gray)
    # Region bands: top sixth (UI/title), middle half (world), bottom
    # sixth (log/controls) — plus the whole frame.
    regions = {
        "full": mask,
        "top_band": mask[: h // 6, :],
        "mid_band": mask[h // 6 : h - h // 6, :],
        "bot_band": mask[h - h // 6 :, :],
        "left_half": mask[:, : w // 2],
        "right_half": mask[:, w // 2 :],
    }
    ink_pct = 100.0 * mask.mean() / 255.0
    print(f"\n== {pathlib.Path(path).name}  ({w}x{h}, ink {ink_pct:.1f}%)")
    for name, region in regions.items():
        pitch = region_pitch(region)
        print(
            f"  {name:10s} x_pitch={pitch['x_pitch']:3d}px (s={pitch['x_score']:.1f}) "
            f"y_pitch={pitch['y_pitch']:3d}px (s={pitch['y_score']:.1f})"
        )


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    for path in args:
        analyze(path)
