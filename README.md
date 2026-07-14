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
- always-on centripetal Catmull–Rom stroke smoothing for fast, sparse input
- wheel zoom, pan, clear, fit, 100% zoom

Run:

```bash
cargo run --release
```

Settings are loaded from `config.toml` in the platform configuration directory. Use **Save settings** in the brush panel to create or update it atomically:

- Linux: `~/.config/minipaint-rs/config.toml`
- macOS: `~/Library/Application Support/minipaint-rs/config.toml`
- Windows: the user's roaming application-data directory

Stroke smoothing is always enabled. Its global strength applies to every brush preset and is configured in `config.toml`:

```toml
[smoothing]
strength = 0.8 # greater than 0.0, up to 1.0
```

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
- Use the minimal egui panel for brush selection, size/color, and current settings
- Edit brush behavior in each preset's `brush.toml`
- Use **Reload** after editing TOML externally, or **Open config folder** to locate the files
