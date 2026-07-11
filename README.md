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
- optional centripetal Catmull–Rom stroke smoothing for fast, sparse input
- wheel zoom, pan, clear, fit, 100% zoom

Run:

```bash
cargo run --release
```

Settings are loaded from `config.toml` in the platform configuration directory. Use **Save settings** in the brush panel to create or update it atomically:

- Linux: `~/.config/minipaint/config.toml`
- macOS: `~/Library/Application Support/minipaint/config.toml`
- Windows: the user's roaming application-data directory

Custom brush presets can be installed under `brushes/<id>/` in that directory:

```text
brushes/pencil/
├── brush.toml
└── tip.png
```

Set `active_brush = "pencil"` in `config.toml`. The preset's `stamp` path is resolved relative to `brush.toml`; invalid presets fall back to the bundled charcoal brush.

Controls:

- Left drag: paint
- Wheel: zoom around cursor
- Middle/right drag or Space + left drag: pan
- Use the egui panel for brush size/color, stroke smoothing, pressure readout, and stats
