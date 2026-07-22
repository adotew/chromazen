# Chromazen

Minimal native Rust painting application focused on brush performance.

Implemented:

- `winit` native window
- `wgpu` renderer
- `egui` controls/stats overlay
- 4000 × 4000 paint texture
- bundled charcoal brush using the original stamp PNG
- pressure-sensitive brush size/opacity on macOS via AppKit tablet and
  pressure events
- mouse/fallback input remains full-size and fully opaque
- instanced GPU brush stamping with dedicated paint and eraser blend pipelines
- always-on centripetal Catmull–Rom stroke smoothing for fast, sparse input
- transparent paint layers composited over a configurable Background color
- chronological GPU undo/redo for strokes, layer changes, and Background
  color changes with a bounded 256 MiB history
- wheel zoom, pan, clear, fit, 100% zoom

Run:

```bash
cargo run --release
```

Settings are loaded from `config.toml` in the platform configuration directory.
On macOS and Windows, use **Settings → Save Settings** in the native menu bar
to create or update it atomically:

- Linux: `~/.config/chromazen/config.toml`
- macOS: `~/Library/Application Support/chromazen/config.toml`
- Windows: the user's roaming application-data directory

Stroke smoothing is always enabled. Its global strength applies to every brush
preset and is configured in `config.toml`:

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

Set `active_brush = "pencil"` in `config.toml`. The preset's `stamp` path is
resolved relative to `brush.toml`; invalid presets fall back to the bundled
charcoal brush.

Controls:

- Left drag: use the selected tool on the selected paint layer
- `B`: select Brush
- `E`: select Eraser; erasing makes the selected layer transparent to reveal
  lower layers and the Background
- Wheel: zoom around cursor
- Middle/right drag or Space + left drag: pan
- Undo: `Command-Z` on macOS; `Control-Z` on Windows and Linux
- Redo: `Command-Shift-Z` on macOS; `Control-Y` on Windows;
  `Control-Shift-Z` or `Control-Y` on Linux
- On macOS and Windows, Undo and Redo are also available from the native
  **Edit** menu
- Use the minimal egui panels for brush controls and adding, selecting, or
  deleting layers
- Select **Background** in the Layers panel to change its color; it cannot be
  painted on or deleted
- On macOS and Windows, use the native **Settings** menu to save, reload, reset,
  or open the configuration folder
- Edit brush behavior in each preset's `brush.toml`
- Use **Reload** after editing TOML externally, or **Open config folder** to
  locate the files
