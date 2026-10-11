# ADR 0003: Keep a plain ANSI fallback

The alpha ships a plain ANSI renderer as the default terminal backend. Native
Notcurses support stays behind a feature boundary until the pinned bindings are
verified on both Windows Terminal and a Linux terminal. This keeps gameplay,
frame dumps, bots, and CI usable without a native library while preserving the
small `Terminal` interface for the eventual adapter.

For detailed analysis of Notcurses performance on Windows ConPTY vs. the ANSI streaming
renderer, see [Notcurses Research Findings](../NOTCURSES_RESEARCH_FINDINGS.md).
