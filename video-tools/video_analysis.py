#!/usr/bin/env python3
"""Robust video-understanding pipeline for coding agents (adaptive, motion-aware).

Built for this project's ASCII-game reference captures (portrait phone clips,
30 fps, smooth game animations). Design follows community-proven practice:

* Individual signals: frame differences, histogram distance (scene cuts),
  dense optical flow magnitude + direction (smooth/continuous motion),
  variance-of-Laplacian blur, freeze intervals, and per-second audio RMS
  (no STT -- see LIMITATIONS).
* Adaptive sampling: a full-rate measurement pass first, then flagged
  intervals (scene cuts, sustained motion, subtle change ramps, freezes
  ending) are re-sampled densely -- default 3 fps snapshots per user spec,
  capped -- instead of fixed-FPS whole-video coverage.
* Agent ergonomics: contact sheets with timestamp labels, compact per-second
  JSON metrics, per-frame CSV, event lists, and a `zoom` command that dumps
  ANY interval at up to native fps with a per-pair diff CSV for iterative
  investigation.

LIMITATIONS (state these honestly in reports):
* No speech-to-text: audio is summarized as per-second RMS only (whisper not
  installed); dialogue content stays unknown.
* Optical flow runs on downscaled frames (<=360 px wide) for speed: 1-2 px
  UI twinkles may under-detect; use `zoom` for ground truth on an interval.
* Subtle-change detection is a heuristic ramp detector; zoom over any
  suspicious timestamp when in doubt.
* Phone-capture keyframe quirks can shift timestamps by about one frame.
"""
from __future__ import annotations

import argparse
import csv
import json
import math
import re
import subprocess
from pathlib import Path

import cv2
import numpy as np

FFMPEG = "ffmpeg"
FFPROBE = "ffprobe"

# Measurement thresholds (tuned on 30fps portrait game captures).
DIFF_PCT_CUT = 22.0          # absdiff pixels (delta > 8) fraction => scene cut
HIST_CUT = 0.35              # Bhattacharyya luma histogram distance => cut
FREEZE_DIFF = 0.05           # absdiff_pct below this => frozen
FREEZE_MIN_S = 0.40
MOTION_Q = 1.5               # flow_mag > median * MOTION_Q => active motion
MOTION_MIN_S = 0.33
SUBTLE_HIST = (0.02, 0.14)   # mid-range histogram distance = slow morph band
DENSE_FPS = 3.0              # user spec: snapshots 2-3 times a second
DENSE_MAX_FRAMES = 30        # per flagged interval
DENSE_MAX_INTERVALS = 20
SHEET_TILE_W = 240
SHEET_COLS = 4
# correlation of flow-magnitude over time within a band => easing label
EASE_DECAY_CORR = -0.45


# ----------------------------------------------------------------- helpers
def probe(path: Path) -> dict:
    cmd = [FFPROBE, "-v", "error", "-select_streams", "v:0",
           "-show_entries", "stream=width,height,r_frame_rate,nb_frames",
           "-show_entries", "format=duration",
           "-of", "json", str(path)]
    data = json.loads(subprocess.run(cmd, capture_output=True, text=True,
                                     check=True).stdout)
    st = data["streams"][0]
    num, den = st["r_frame_rate"].split("/")
    fps = float(num) / float(den)
    audio = subprocess.run(
        [FFPROBE, "-v", "error", "-select_streams", "a",
         "-show_entries", "stream=codec_name", "-of", "json", str(path)],
        capture_output=True, text=True, check=True).stdout
    has_audio = bool(json.loads(audio).get("streams"))
    return {
        "width": int(st["width"]), "height": int(st["height"]),
        "fps": fps, "frames": int(st["nb_frames"]),
        "duration": float(data["format"]["duration"]), "has_audio": has_audio,
    }


def audio_rms_per_second(path: Path) -> list[dict]:
    """Per-second overall RMS (dB) via ffmpeg astats; silence is omitted."""
    cmd = [FFMPEG, "-v", "info", "-i", str(path),
           "-af", "astats=metadata=1:reset=1,"
                  "ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-",
           "-f", "null", "-"]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    rows: list[tuple[float, float]] = []
    t = None
    for line in proc.stderr.splitlines():
        m = re.search(r"pts_time:([\d.]+)", line)
        if m:
            t = float(m.group(1))
        m = re.search(r"RMS_level=(-?[\d.]+|-inf)", line)
        if m and t is not None:
            val = m.group(1)
            if val != "-inf" and float(val) > -85:
                rows.append((t, float(val)))
    buckets: dict[int, list[float]] = {}
    for sec, db in rows:
        buckets.setdefault(int(sec), []).append(db)
    return [{"sec": s, "rms_db": round(max(v), 1)}
            for s, v in sorted(buckets.items())]


class VideoReader:
    """Sequential decoder that yields (index, timestamp, downscaled frame)."""

    def __init__(self, path: Path, analysis_max_w: int = 360):
        self.cap = cv2.VideoCapture(str(path))
        if not self.cap.isOpened():
            raise RuntimeError(f"cannot open {path}")
        self.analysis_max_w = analysis_max_w

    def frames(self):
        idx = 0
        fps = self.cap.get(cv2.CAP_PROP_FPS) or 30.0
        while True:
            ok, frame = self.cap.read()
            if not ok:
                break
            h, w = frame.shape[:2]
            if w > self.analysis_max_w:
                scale = self.analysis_max_w / w
                frame = cv2.resize(frame, None, fx=scale, fy=scale,
                                   interpolation=cv2.INTER_AREA)
            yield idx, idx / fps, frame
            idx += 1
        self.cap.release()


_FLOW = None


def flow_engine():
    """DIS optical flow when available, Farneback fallback (community default)."""
    global _FLOW
    if _FLOW is None:
        try:
            inst = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_FAST)
            _FLOW = ("dis", inst)
        except Exception:
            _FLOW = ("farneback", None)
    return _FLOW


def flow_mag_gray(prev_gray: np.ndarray,
                  gray: np.ndarray) -> tuple[float, list[int]]:
    """Mean flow magnitude (px at analysis scale) + 8-bin direction histogram."""
    kind, inst = flow_engine()
    try:
        if kind == "dis":
            flow = inst.calc(prev_gray, gray, None)
        else:
            flow = cv2.calcOpticalFlowFarneback(
                prev_gray, gray, None, 0.5, 2, 21, 2, 5, 1.1, 0)
    except cv2.error:
        return 0.0, [0] * 8
    mag, ang = cv2.cartToPolar(flow[..., 0], flow[..., 1])
    moving = mag > 0.3
    if not moving.any():
        return 0.0, [0] * 8
    dirs = (np.degrees(ang[moving]) // 45).astype(np.uint8) % 8
    return float(mag.mean()), np.bincount(dirs, minlength=8).tolist()


def luma_hist(gray: np.ndarray) -> np.ndarray:
    hist = cv2.calcHist([gray], [0], None, [32], [0, 256])
    cv2.normalize(hist, hist)
    return hist


# ------------------------------------------------------------ main analyze
def analyze_video(path: Path, outdir: Path, dense_fps: float) -> dict:
    meta = probe(path)
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "_", path.stem)
    vdir = outdir / stem
    vdir.mkdir(parents=True, exist_ok=True)

    rows: list[dict] = []
    prev_gray = None
    prev_hist = None
    prev_small = None
    blur_window: list[float] = []
    reader = VideoReader(path)

    for idx, ts, small in reader.frames():
        gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
        hist = luma_hist(gray)
        lap_var = float(cv2.Laplacian(gray, cv2.CV_64F).var())
        if prev_gray is not None:
            # Color-aware diff (max channel): luma-only diffs are blind to
            # hue morphs (damage flashes, color pulses) since hue rotation
            # preserves luminance.
            cdiff = cv2.absdiff(prev_small, small).max(axis=2)
            absdiff_pct = float((cdiff > 8).mean() * 100.0)
            absdiff2_pct = float((cdiff > 2).mean() * 100.0)
            bh = float(cv2.compareHist(prev_hist, hist,
                                       cv2.HISTCMP_BHATTACHARYYA))
            fmag, fdirs = flow_mag_gray(prev_gray, gray)
            rows.append({"idx": idx, "t": round(ts, 3),
                         "absdiff_pct": round(absdiff_pct, 2),
                         "absdiff2_pct": round(absdiff2_pct, 2),
                         "hist": round(bh, 4), "flow": round(fmag, 3),
                         "dir": fdirs, "blur": round(lap_var, 1),
                         "luma": round(float(gray.mean()), 1)})
        else:
            rows.append({"idx": idx, "t": round(ts, 3), "absdiff_pct": 0.0,
                         "absdiff2_pct": 0.0,
                         "hist": 0.0, "flow": 0.0, "dir": [0] * 8,
                         "blur": round(lap_var, 1),
                         "luma": round(float(gray.mean()), 1)})
        prev_hist = hist
        prev_gray = gray
        prev_small = small
        blur_window.append(lap_var)
        if len(blur_window) > 61:
            blur_window.pop(0)

    fps = meta["fps"]

    # --- per-second aggregation keeps the agent-facing JSON compact
    per_second: list[dict] = []
    for sec in range(math.ceil(meta["duration"])):
        sel = [r for r in rows if int(r["t"]) == sec]
        if not sel:
            continue
        per_second.append({
            "sec": sec,
            "absdiff_avg": round(sum(r["absdiff_pct"] for r in sel) / len(sel), 2),
            "absdiff_max": round(max(r["absdiff_pct"] for r in sel), 2),
            "absdiff2_avg": round(sum(r["absdiff2_pct"] for r in sel) / len(sel), 2),
            "flow_avg": round(sum(r["flow"] for r in sel) / len(sel), 3),
            "flow_max": round(max(r["flow"] for r in sel), 3),
            "hist_max": round(max(r["hist"] for r in sel), 4),
            "blur_min": round(min(r["blur"] for r in sel), 1),
        })

    # --- events
    scene_cuts: list[float] = []
    freezes: list[list[float]] = []
    motion_bands: list[dict] = []
    subtle_events: list[list[float]] = []
    blur_events: list[list[float]] = []
    freeze_start = subtle_start_idx = motion_start_idx = None
    flow_med = float(np.median([r["flow"] for r in rows])) or 0.001
    blur_med = float(np.median([r["blur"] for r in rows if r["blur"] > 0]) or 0)
    blur_thresh = blur_med * 0.5
    blur_start: float | None = None
    blur_last: float | None = None

    for i, r in enumerate(rows):
        # scene cut: histogram break WITH some pixel movement. The old
        # "absdiff8 > 22" alternative branch false-fired every ~0.27 s on
        # fast scrolling/panning captures (continuous motion); a real cut
        # always changes the global luma histogram (white flash included).
        if i > 0 and rows[i - 1]["absdiff_pct"] > 0.5 \
                and rows[i - 1]["hist"] > HIST_CUT:
            scene_cuts.append(round(r["t"], 2))
        if r["absdiff_pct"] < FREEZE_DIFF and r["absdiff2_pct"] < 0.2:
            if freeze_start is None:
                freeze_start = r["t"]
        elif freeze_start is not None:
            if r["t"] - freeze_start >= FREEZE_MIN_S:
                freezes.append([round(freeze_start, 2), round(r["t"], 2)])
            freeze_start = None
        if r["flow"] > max(0.12, flow_med * MOTION_Q):
            if motion_start_idx is None:
                motion_start_idx = i
        elif motion_start_idx is not None:
            start_t = rows[motion_start_idx]["t"]
            if r["t"] - start_t >= MOTION_MIN_S \
                    and max((x["flow"] for x in rows[motion_start_idx:i]),
                            default=0.0) >= 0.25:
                seg = rows[motion_start_idx:i]
                mags = np.array([x["flow"] for x in seg])
                corr = 0.0
                if len(mags) > 3 and mags.max() > 1e-4:
                    corr = float(np.corrcoef(np.arange(len(mags)), mags)[0, 1])
                ease = ("ease-out" if corr < EASE_DECAY_CORR
                        else "ease-in" if corr > 0.5 else "linear/oscillating")
                motion_bands.append({
                    "start": round(start_t, 2), "end": round(r["t"], 2),
                    "peak_flow": round(float(mags.max()), 3), "ease": ease,
                    "dirs": _merge_dirs(seg)})
            motion_start_idx = None
        # Subtle / ramping change: pixels move a little (>2) but nothing
        # jumps (>8) and no histogram cut — slow fades, gentle UI pulses,
        # ambient scroll, and color morphs all live here (a pure luma ramp
        # never clears the >8-per-pixel threshold).
        if 1.0 < r["absdiff2_pct"] < 101 and r["absdiff_pct"] < 8 \
                and r["hist"] < SUBTLE_HIST[1]:
            if subtle_start_idx is None:
                subtle_start_idx = i
        elif subtle_start_idx is not None:
            start_t = rows[subtle_start_idx]["t"]
            if r["t"] - start_t >= 0.5:
                subtle_events.append([round(start_t, 2), round(r["t"], 2)])
            subtle_start_idx = None
        # Sustained blur: relative to the video's own blur median (a rolling
        # window median adapts to a sustained shift and never fires), and
        # only over MOVING content — static flat frames have low Laplacian
        # variance by content, not by blur.
        if blur_thresh > 0 and r["blur"] < blur_thresh and r["flow"] > 0.1:
            if blur_start is None:
                blur_start = r["t"]
            blur_last = r["t"]
        elif blur_start is not None:
            if blur_last - blur_start >= 0.33:
                blur_events.append([round(blur_start, 2), round(blur_last, 2)])
            blur_start = blur_last = None

    # flush events that run to the end of the video
    if freeze_start is not None and rows and \
            rows[-1]["t"] - freeze_start >= FREEZE_MIN_S:
        freezes.append([round(freeze_start, 2), round(rows[-1]["t"], 2)])
    if blur_start is not None and blur_last is not None and \
            blur_last - blur_start >= 0.33:
        blur_events.append([round(blur_start, 2), round(blur_last, 2)])
    if motion_start_idx is not None:
        seg = rows[motion_start_idx:]
        start_t = seg[0]["t"]
        if seg and seg[-1]["t"] - start_t >= MOTION_MIN_S:
            mags = np.array([x["flow"] for x in seg])
            corr = 0.0
            if len(mags) > 3 and mags.max() > 1e-4:
                corr = float(np.corrcoef(np.arange(len(mags)), mags)[0, 1])
            ease = ("ease-out" if corr < EASE_DECAY_CORR
                    else "ease-in" if corr > 0.5 else "linear/oscillating")
            motion_bands.append({
                "start": round(start_t, 2), "end": round(seg[-1]["t"], 2),
                "peak_flow": round(float(mags.max()), 3), "ease": ease,
                "dirs": _merge_dirs(seg)})
    if subtle_start_idx is not None:
        start_t = rows[subtle_start_idx]["t"]
        if rows[-1]["t"] - start_t >= 0.5:
            subtle_events.append([round(start_t, 2), round(rows[-1]["t"], 2)])

    # dedupe/merge scene cuts closer than 0.25 s (flash = spike + decay)
    merged_cuts: list[float] = []
    for t in scene_cuts:
        if not merged_cuts or t - merged_cuts[-1] > 0.25:
            merged_cuts.append(t)
    scene_cuts = merged_cuts[:30]

    # --- dense adaptive snapshots for flagged intervals (user spec: 2-3/s)
    flagged = _flagged_intervals(scene_cuts, freezes, motion_bands,
                                 subtle_events, blur_events, meta["duration"])
    dense_meta = []
    for start, end, kinds in flagged:
        s = max(0.0, start - 0.5)
        e = min(meta["duration"], end + 0.5)
        n = min(DENSE_MAX_FRAMES, max(2, int((e - s) * dense_fps)))
        sheet, stamps = _dense_contact_sheet(path, s, e, n, vdir)
        dense_meta.append({"start": round(s, 2), "end": round(e, 2),
                           "kinds": kinds, "frames": n,
                           "sheet": sheet, "timestamps": stamps})

    overview_sheet = _overview_sheet(path, meta, vdir)
    audio = audio_rms_per_second(path) if meta["has_audio"] else []

    result = {
        "video": path.name, "meta": meta,
        "flow_threshold": round(flow_med, 3),
        "scene_cuts": scene_cuts, "freezes": freezes,
        "motion_bands": motion_bands, "subtle_changes": subtle_events,
        "blur_events": blur_events, "per_second": per_second,
        "audio_rms_per_sec": audio, "dense_intervals": dense_meta,
        "overview_sheet": overview_sheet,
        "LIMITATIONS": "no STT; flow at <=360px; subtle-change heuristic; "
                       "timestamps +-1 frame",
    }
    (vdir / "metrics.json").write_text(json.dumps(result, indent=1),
                                       encoding="utf-8")
    with (vdir / "per_frame.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=["idx", "t", "absdiff_pct",
                                           "absdiff2_pct", "hist",
                                           "flow", "blur", "luma", "dir"])
        w.writeheader()
        for r in rows:
            w.writerow({**r, "dir": " ".join(map(str, r["dir"]))})
    _summary_md(result, vdir)
    return result


def _merge_dirs(seg: list[dict]) -> list[int]:
    tot = np.zeros(8, dtype=np.int64)
    for x in seg:
        tot += np.array(x["dir"])
    return tot.tolist()


def _flagged_intervals(cuts, freezes, bands, subtles, blurs, duration):
    """Compose agent-inspection windows. Merge ONLY within a kind, so a
    back-to-back freeze+cut chain does not swallow the whole video."""
    per_kind: dict[str, list[list[float]]] = {
        "scene_cut": [[t, min(duration, t + 2.0)] for t in cuts],
        "freeze": [],
        "freeze_end": [[b, min(duration, b + 1.5)] for _, b in freezes],
        "subtle_change": [[a, b] for a, b in subtles],
        "blur": [[a, b] for a, b in blurs],
    }
    for a, b in freezes:
        per_kind["freeze"].append([a, b])
    for band in bands:
        kind = f"motion:{band['ease']}"
        per_kind.setdefault(kind, []).append([band["start"], band["end"]])
    merged_kinds: list[tuple[float, float, set]] = []
    for kind, spans in per_kind.items():
        spans.sort()
        cur = None
        for s, e in spans:
            if cur and s <= cur[1] + 0.4:
                cur[1] = max(cur[1], e)
            else:
                if cur:
                    merged_kinds.append((cur[0], cur[1], {kind}))
                cur = [s, e]
        if cur:
            merged_kinds.append((cur[0], cur[1], {kind}))
    merged_kinds.sort(key=lambda f: (f[0], f[2]))
    # Combine touching windows from different kinds (e.g. a cut at the end
    # of a motion band) so the agent gets ONE dense sheet covering both.
    combined: list[list] = []
    for s, e, kinds in merged_kinds:
        # Merge cross-kind windows only on REAL overlap (>=0.3 s): touching
        # windows (a freeze ending where a cut begins) stay separate so
        # back-to-back events don't chain into one whole-video blob.
        if combined and s <= combined[-1][1] - 0.3:
            combined[-1][1] = max(combined[-1][1], e)
            combined[-1][2] |= kinds
        else:
            combined.append([s, e, set(kinds)])
    out = []
    for s, e, kinds in combined[:DENSE_MAX_INTERVALS]:
        # Bound every window: back-to-back events (freeze chains, ambient
        # motion) must not swallow the whole video into one sheet — the
        # agent zooms into the clipped tail on demand.
        out.append((s, min(e, s + 5.0), sorted(kinds)))
    return out


def _decode_segment(path: Path, s: float, e: float, fps: float):
    """Decode frames in [s, e] at fps as decoded BMP bytes -> images."""
    cmd = [FFMPEG, "-v", "error", "-ss", f"{s:.3f}", "-i", str(path),
           "-t", f"{e - s + 0.2:.3f}", "-vf", f"fps={fps}",
           "-f", "image2pipe", "-vcodec", "bmp", "-"]
    raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    frames, off = [], 0
    while off + 6 < len(raw):
        size = int.from_bytes(raw[off + 2:off + 6], "little")
        img = cv2.imdecode(np.frombuffer(raw[off:off + size], np.uint8),
                           cv2.IMREAD_COLOR)
        if img is not None:
            frames.append(img)
        off += size
    return frames


def _tile(img: np.ndarray, label: str) -> np.ndarray:
    h, w = img.shape[:2]
    scale = SHEET_TILE_W / w
    tile = cv2.resize(img, (SHEET_TILE_W, int(h * scale)))
    bar = np.full((22, SHEET_TILE_W, 3), 20, np.uint8)
    cv2.putText(bar, label, (4, 16), cv2.FONT_HERSHEY_SIMPLEX, 0.45,
                (255, 255, 255), 1, cv2.LINE_AA)
    return np.vstack([tile, bar])


def _write_sheet(tiles: list[np.ndarray], path: Path,
                 cols: int = SHEET_COLS) -> None:
    rows = [tiles[i:i + cols] for i in range(0, len(tiles), cols)]
    grid = []
    for r in rows:
        while len(r) < cols:
            r.append(np.zeros_like(r[0]))
        grid.append(np.hstack(r))
    cv2.imwrite(str(path), np.vstack(grid))


def _dense_contact_sheet(path: Path, s: float, e: float, n: int,
                         vdir: Path) -> tuple[str, list[float]]:
    seg_fps = DENSE_FPS * 2
    seg = _decode_segment(path, s, e, seg_fps)
    if not seg:
        return "EMPTY", []
    step = max(1, int(round(len(seg) / n)))
    picked_idx = list(range(0, len(seg), step))[:n]
    picked = [seg[i] for i in picked_idx]
    stamps = [round(s + i / seg_fps, 2) for i in picked_idx]
    tiles = [_tile(img, f"{t:06.2f}s") for img, t in zip(picked, stamps)]
    name = f"dense_{s:06.2f}_{e:06.2f}.png"
    _write_sheet(tiles, vdir / name)
    return name, stamps


def _overview_sheet(path: Path, meta: dict, vdir: Path) -> str:
    want = [round(x, 2) for x in np.arange(0.5, meta["duration"], 2.0)][:24]
    tiles = []
    for t in want:
        seg = _decode_segment(path, t, t + 0.05, 20.0)
        if seg:
            tiles.append(_tile(seg[0], f"{t:06.2f}s"))
    name = "overview.png"
    _write_sheet(tiles, vdir / name)
    return name


def _summary_md(result: dict, vdir: Path) -> None:
    r = result
    m = r["meta"]
    lines = [
        f"# {r['video']}",
        f"- {m['width']}x{m['height']} @ {m['fps']:.0f} fps, "
        f"{m['duration']:.1f}s, {m['frames']} frames, "
        f"audio={'yes' if m['has_audio'] else 'no'}",
        f"- flow threshold (median): {r['flow_threshold']}",
        f"- scene cuts ({len(r['scene_cuts'])}): {r['scene_cuts']}",
        f"- freezes ({len(r['freezes'])}): {r['freezes']}",
        f"- motion bands ({len(r['motion_bands'])}): "
        + "; ".join(f"{b['start']}-{b['end']}s peak {b['peak_flow']} "
                    f"{b['ease']}" for b in r["motion_bands"]),
        f"- subtle changes ({len(r['subtle_changes'])}): "
        f"{r['subtle_changes']}",
        f"- blur events ({len(r['blur_events'])}): {r['blur_events']}",
        f"- dense snapshot sheets: "
        f"{[d['sheet'] for d in r['dense_intervals']]}",
        f"- LIMITATIONS: {r['LIMITATIONS']}",
    ]
    (vdir / "SUMMARY.md").write_text("\n".join(lines), encoding="utf-8")


# ------------------------------------------------------------------- zoom
def zoom_interval(path: Path, start: float, end: float, fps: float,
                  outdir: Path) -> Path:
    meta = probe(path)
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "_", path.stem)
    zdir = outdir / f"{stem}_zoom_{start:.2f}_{end:.2f}"
    zdir.mkdir(parents=True, exist_ok=True)
    if fps <= 0 or fps > meta["fps"]:
        fps = meta["fps"]
    fd = zdir / "frames"
    fd.mkdir(exist_ok=True)
    subprocess.run([FFMPEG, "-v", "error", "-ss", f"{start:.3f}",
                    "-i", str(path), "-t", f"{end - start:.3f}",
                    "-vf", f"fps={fps}", "-start_number", "0",
                    str(fd / "f_%04d.png")], check=True)
    files = sorted(fd.glob("f_*.png"))
    with (zdir / "diffs.csv").open("w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["frame_a", "frame_b", "t_a", "t_b", "absdiff_pct",
                    "hist", "flow"])
        prev = None
        for f in files:
            img = cv2.imread(str(f))
            idx = int(f.stem.split("_")[1])
            t = start + idx / fps
            gray = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY)
            if prev is not None:
                diff = cv2.absdiff(gray, prev[0])
                bh = float(cv2.compareHist(luma_hist(prev[0]), luma_hist(gray),
                                           cv2.HISTCMP_BHATTACHARYYA))
                fm, _ = flow_mag_gray(cv2.resize(prev[0], (360, 640)),
                                      cv2.resize(gray, (360, 640)))
                w.writerow([idx - 1, idx, f"{prev[1]:.3f}", f"{t:.3f}",
                            f"{(diff > 8).mean() * 100:.2f}", f"{bh:.4f}",
                            f"{fm:.3f}"])
            prev = (gray, t)
    return zdir


# -------------------------------------------------------------------- cli
def main() -> None:
    ap = argparse.ArgumentParser(description="Video understanding pipeline")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("analyze", help="full pass + adaptive dense snapshots")
    a.add_argument("videos", nargs="+")
    a.add_argument("--outdir", default=str(Path(__file__).parent / "out"))
    a.add_argument("--dense-fps", type=float, default=DENSE_FPS)
    z = sub.add_parser("zoom", help="dump any interval at up to native fps")
    z.add_argument("video")
    z.add_argument("start", type=float)
    z.add_argument("end", type=float)
    z.add_argument("--fps", type=float, default=0.0)
    z.add_argument("--outdir", default=str(Path(__file__).parent / "out"))
    args = ap.parse_args()
    outdir = Path(args.outdir)
    if args.cmd == "analyze":
        outdir.mkdir(parents=True, exist_ok=True)
        catalog = []
        for v in args.videos:
            p = Path(v)
            print(f"analyzing {p} ...", flush=True)
            catalog.append(analyze_video(p, outdir, args.dense_fps))
        (outdir / "CATALOG.json").write_text(
            json.dumps(catalog, indent=1), encoding="utf-8")
        print("done:", ", ".join(c["video"] for c in catalog))
    else:
        d = zoom_interval(Path(args.video), args.start, args.end, args.fps,
                          outdir)
        print("zoom written to", d)


if __name__ == "__main__":
    main()
