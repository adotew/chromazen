# minipaint-rs

Minimal native Rust port of the `minipaint` brush path for painting performance tests.

Implemented:

- `winit` native window
- `wgpu` renderer
- `egui` controls/stats overlay
- 4000 × 4000 paint texture
- single brush tool using the original charcoal stamp PNG
- pressure-sensitive brush size/opacity on macOS via AppKit tablet/pressure events
- mouse/fallback input remains full-size and fully opaque
- instanced GPU brush stamping with the same core stamp shader/blend approach as `minipaint`
- wheel zoom, pan, clear, fit, 100% zoom

Run:

```bash
cargo run --release
```

Controls:

- Left drag: paint
- Wheel: zoom around cursor
- Middle/right drag or Space + left drag: pan
- Use the egui panel for brush size/color, pressure readout, and stats
