# video-tools — reference-capture understanding pipeline

Robust video-understanding pipeline for coding agents, tuned for this
project's reference captures (`../../data/videos/*.mp4`: portrait phone
clips, 30 fps, smooth game animations). The game NEVER loads these files;
this tool exists to study them.

## Usage

```powershell
# Full pass: measurements + event detection + adaptive dense snapshots
python video_analysis.py analyze ..\..\data\videos\Video-47489.mp4

# All videos at once
python video_analysis.py analyze ..\..\data\videos\*.mp4

# Zoom into ANY interval at up to native fps (ground truth)
python video_analysis.py zoom <video> 12.4 15.0          # native 30 fps
python video_analysis.py zoom <video> 12.4 15.0 --fps 10 # explicit rate
```

Outputs land in `out/<video>/`:

| File | Purpose |
|---|---|
| `SUMMARY.md` | one-screen event summary (cuts, freezes, motion bands + easing labels, subtle changes, blur spans) |
| `metrics.json` | compact per-second aggregates + event lists + dense-interval metadata |
| `per_frame.csv` | every analyzed frame: `absdiff_pct` (>8/255, "jumps"), `absdiff2_pct` (>2/255, "micro-motion"), histogram distance, flow magnitude, blur, luma, 8-bin flow direction |
| `overview.png` | contact sheet, ~1 tile / 2 s |
| `dense_<a>_<b>.png` | flagged intervals sampled at 3 fps (user spec: 2-3 snapshots/s), timestamps on every tile |
| `out/CATALOG.json` | all videos in one file |

`zoom` writes native-fps frames + `diffs.csv` (per-pair absdiff / histogram /
flow) — iterate there when an interval is still uncertain.

## Method (and what NOT to trust)

Signals: color-aware per-pixel diffs (max channel — luma-only diffs are
blind to hue morphs), 32-bin luma histogram distance (cuts), dense optical
flow magnitude + 8-bin direction (DIS, Farneback fallback), variance-of-
Laplacian blur, freeze spans, per-second audio RMS.

Event rules (validated on a synthetic benchmark with known freeze / slow
hue morph / full-field ramp / continuous motion / white-flash hard cut /
subtle fade / sustained blur):

- scene cut = histogram break OR pixel-jump spike, AND real pixel movement
  (flat full-field ramps spike single-bin histograms across bin boundaries
  with no scene change);
- freeze = <0.2% of pixels change >2/255 sustained ≥0.4 s;
- motion band = flow > max(0.12, 1.5×video-median) sustained ≥0.33 s, with
  an ease label from the flow-vs-time correlation;
- subtle change = 1%-98% of pixels moving >2/255, nothing jumping >8,
  sustained ≥0.5 s (slow fades never clear an 8-level threshold);
- blur = laplacian variance < 0.5×video median while content moves.

Interval windows merge only on real overlap (>=0.3 s), are clipped to 5 s,
capped at 20/video — back-to-back events must not produce one whole-video
blob; zoom in for the clipped tail.

## Known limitations (report honestly)

1. No speech-to-text: audio is per-second RMS only (whisper not installed).
2. Optical flow + diffs run at ≤360 px wide: 1-2 px UI twinkles may
   under-detect; zoom for ground truth.
3. Sub-perceptual morphs (hue rotation < ~1°/frame, luma ramps < 2 levels/
   frame) round away in 8-bit video and can read as freezes — track the
   `subtle_changes` list and the per-second directions instead.
4. Phone-capture keyframe quantization can shift timestamps ±1 frame.
5. A video that is ONE sustained slow scroll (e.g. WhatsApp 39 PM) reports
   no discrete events: its per-frame motion sits below its own median-based
   motion threshold. Its content is in the overview sheet + per-second
   aggregates; use `zoom` over any timestamp that interests you.
5. `zoom` per-pair flow runs at full resolution — keep intervals short.
