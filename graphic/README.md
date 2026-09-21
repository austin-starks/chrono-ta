# README animation

This Remotion composition explains the semantic difference between upstream
`ta` and `chrono-ta`: observation-count windows versus elapsed-time windows.

```bash
npm ci
npm run check
npm run render-gif
```

The committed artifact is `out/window-semantics.gif`. The build renders an
opaque MP4 first, then creates a deterministic 10 fps palette GIF with FFmpeg.
It fails unless the delivered file is 1200x600, contains at least 70 frames,
and lasts approximately eight seconds. The source stays in the repository so
the animation can evolve with the crate's behavior.
