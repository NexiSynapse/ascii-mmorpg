"""Per-band glyph pitch via autocorrelation of ink profiles. Tests uniform vs
variable glyph density (the 'more dots somewhere' hypothesis) and perspective
compression (pitch shrinking toward the top) on single frames."""
import sys, cv2, numpy as np

def best_lag(profile, lo, hi):
    p = profile - profile.mean()
    if p.std() < 1e-6:
        return 0, 0.0
    scores = []
    for lag in range(lo, hi):
        if lag >= len(p): break
        scores.append((float(np.dot(p[:-lag], p[lag:]) / (len(p) - lag)), lag))
    s, l = max(scores)
    return l, s / max(p.var() * len(p), 1e-9)

for path in sys.argv[1:]:
    img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
    h, w = img.shape
    ink = (img > 40).astype(np.float32)
    print(f"\n== {path.split('/')[-1]} ({w}x{h}) band: x-pitch, y-pitch, ink%")
    for b in range(8):
        y0, y1 = h * b // 8, h * (b + 1) // 8
        strip = ink[y0:y1]
        colp = strip.mean(axis=0)
        rowp = strip.mean(axis=1)
        xl, xs = best_lag(colp, 3, 14)
        yl, ys = best_lag(rowp, 5, 16)
        print(f"  band{b} y[{y0:4d}:{y1:4d}]  x-pitch={xl:2d} (conf {xs:4.1f})  y-pitch={yl:2d} (conf {ys:4.1f})  ink={strip.mean()*100:4.1f}%")
