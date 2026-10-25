"""Global camera-motion measurement via cv2.phaseCorrelate on ink masks."""
import sys, glob, re, cv2, numpy as np

def load_ink(p):
    g = cv2.imread(p, cv2.IMREAD_GRAYSCALE)[150:1150]
    return (g > 40).astype(np.float32)

for d in sys.argv[1:]:
    frames = sorted(glob.glob(d + "/frames/*.png"), key=lambda p: int(re.search(r"(\d+)\.png$", p).group(1)))
    print(f"== {d.split('/')[-1]} ({len(frames)} frames)")
    # self-test on first frame
    a = load_ink(frames[0])
    t = np.roll(np.roll(a, -7, axis=1), -5, axis=0)  # content right 7, down 5
    (sx, sy), _ = cv2.phaseCorrelate(a, t)
    print(f"  selftest(expect +7,+5): {sx:+.2f},{sy:+.2f}")
    prev = None
    for i, f in enumerate(frames):
        cur = load_ink(f)
        if prev is not None:
            (sx, sy), resp = cv2.phaseCorrelate(prev, cur)
            if i <= 12 or i % 6 == 0:
                print(f"  f{i:03d} t={i/30.0:5.2f}s  shift=({sx:+6.2f},{sy:+6.2f})")
        prev = cur
