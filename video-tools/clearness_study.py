#!/usr/bin/env python3
"""Character-clarity study: what makes actors readable at low resolution.

Compares reference-capture frames with (optionally) a PNG rendered from one
of our own --wgpu-demo dumps. Metrics are structural, not impressionistic:

- luma bands: pure-black negative space / dim carpet / mid noise / bright ink
- bright blobs (actors, FX, labels): interior luma/saturation vs the local
  surrounding ring -> actor-vs-ground contrast and saturation separation
- hue concentration: single-hue dominance inside a blob
- text-on-black: share of white-text pixels with a black backing plate

Usage:
  python clearness_study.py <image1> <image2> ... [--labels a,b,c]
"""
import argparse
import os
import sys

import cv2
import numpy as np

BLACK_LUMA = 10.0
BRIGHT_LUMA = 140.0
WHITE_LUMA = 180.0
WHITE_SAT = 70.0


def luma_of(bgr):
    f = bgr.astype(np.float32)
    return 0.114 * f[..., 0] + 0.587 * f[..., 1] + 0.299 * f[..., 2]


def hue_share(bgr, mask, bins=24):
    """Share of the largest hue bin among sufficiently saturated pixels."""
    if mask.sum() < 50:
        return 0.0
    hsv = cv2.cvtColor(bgr, cv2.COLOR_BGR2HSV)
    h, s = hsv[..., 0].astype(np.int32), hsv[..., 1].astype(np.int32)
    sel = mask & (s > 90)
    if sel.sum() < 50:
        return 0.0
    hist = np.bincount((h[sel] * bins // 180).clip(0, bins - 1), minlength=bins)
    return float(hist.max()) / float(max(sel.sum(), 1))


def backing_plate_share(bgr):
    """Share of white-text pixels that have a near-black local backing."""
    luma, hsv = luma_of(bgr), cv2.cvtColor(bgr, cv2.COLOR_BGR2HSV)
    white = (luma > WHITE_LUMA) & (hsv[..., 1] < WHITE_SAT)
    if white.sum() < 40:
        return None
    tight_dark = cv2.erode((luma < 22).astype(np.uint8), np.ones((5, 9), np.uint8))
    backed = cv2.dilate(tight_dark, np.ones((11, 25), np.uint8)) > 0
    return float((white & backed).sum()) / float(white.sum())


def analyze(path):
    img = cv2.imread(path)
    if img is None:
        print(f"!! unreadable {path}")
        return None
    luma, hsv = luma_of(img), cv2.cvtColor(img, cv2.COLOR_BGR2HSV)
    sat = hsv[..., 1].astype(np.float32)
    h, w = luma.shape
    out = {"path": os.path.basename(path), "px": f"{w}x{h}"}

    total = float(luma.size)
    out["black%"] = 100.0 * float((luma < BLACK_LUMA).mean())
    carpet = (luma >= BLACK_LUMA) & (luma < 60.0)
    out["carpet%"] = 100.0 * float(carpet.mean())
    out["mid%"] = 100.0 * float(((luma >= 60.0) & (luma < BRIGHT_LUMA)).mean())
    out["bright%"] = 100.0 * float((luma >= BRIGHT_LUMA).mean())
    out["p50"] = float(np.median(luma))
    out["carpet_sat_p50"] = float(np.median(sat[carpet])) if carpet.sum() else 0.0

    bright = (luma >= BRIGHT_LUMA).astype(np.uint8)
    grown = cv2.dilate(bright, np.ones((7, 7), np.uint8))
    n, labels, stats, _ = cv2.connectedComponentsWithStats(grown)
    blobs = []
    for i in range(1, n):
        x, y, bw, bh, area = stats[i]
        if not (400 <= area <= 80000):
            continue
        sel = (labels == i) & (bright > 0) if False else None
        mask = (labels == i)
        ys, xs = np.where(mask)
        lum_sel, sat_sel = luma[mask], sat[mask]
        med_l, med_s = float(np.median(lum_sel)), float(np.median(sat_sel))
        ring = np.zeros_like(bright, dtype=bool)
        r = 48
        x0, x1 = max(0, x - r), min(w, x + bw + r)
        y0, y1 = max(0, y - r), min(h, y + bh + r)
        ring[y0:y1, x0:x1] = True
        ring &= ~(mask | (luma >= BRIGHT_LUMA))
        if ring.sum() < 800:
            continue
        ring_med_l = float(np.median(luma[ring]))
        ring_med_s = float(np.median(sat[ring]))
        inner_mask = mask & (luma >= BRIGHT_LUMA) & (hsv[..., 1] > 90)
        blobs.append(
            {
                "box": (int(x), int(y), int(bw), int(bh)),
                "luma": med_l,
                "sat": med_s,
                "dL": med_l - ring_med_l,
                "dS": med_s - ring_med_s,
                "hue1": hue_share(img, inner_mask),
            }
        )
    blobs.sort(key=lambda b: -b["dL"])
    out["blobs"] = blobs[:5]
    out["plate%"] = backing_plate_share(img)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("images", nargs="+")
    ap.add_argument("--labels", default=",".join(os.path.basename(p) for p in []))
    a = ap.parse_args()
    names = a.labels.split(",") if a.labels else []
    for p, nm in zip(a.images, names + [None] * len(a.images)):
        r = analyze(p)
        if not r:
            continue
        tag = nm or ""
        print(
            f"{tag:22} p50={r['p50']:5.1f} black%={r['black%']:4.1f} "
            f"carpet%={r['carpet%']:4.1f} mid%={r['mid%']:4.1f} bright%={r['bright%']:4.1f} "
            f"carpSat={r['carpet_sat_p50']:4.0f} plate%={r['plate%'] if r['plate%'] is None else round(r['plate%'],2)}"
        )
        for b in r["blobs"]:
            x, y, bw, bh = b["box"]
            print(
                f"    blob@{x},{y} {bw}x{bh}  luma={b['luma']:5.1f} sat={b['sat']:4.1f} "
                f"dL={b['dL']:+6.1f} dS={b['dS']:+6.1f} hue1={b['hue1']:.2f}"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
