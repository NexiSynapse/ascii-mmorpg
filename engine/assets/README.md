# Engine font asset

`LiberationMono-Regular.ttf` (312 KB) — monospace UI font rasterized at
startup into the presenter's GPU glyph atlas (`src/present.rs`).

- Source: system package (`/usr/share/fonts/liberation/LiberationMono-Regular.ttf`).
- License: SIL Open Font License 1.1 (Liberation fonts). Embedding and
  redistribution with this notice is permitted; see
  https://scripts.sil.org/OFL for the full text.
- Why embedded instead of fontconfig: deterministic startup on any machine,
  including headless CI and fresh clones without system fonts.
