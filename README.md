# Chromazen

Minimal native Rust painting application focused on brush performance.

Implemented:

- `winit` native window
- `wgpu` renderer
- `egui` artwork gallery and editor controls
- persistent editable artworks with automatic background saving
- full-resolution flattened PNG export
- 4000 × 4000 paint texture
- bundled charcoal brush using the original stamp PNG
- pressure-sensitive brush size/opacity on macOS via AppKit tablet and
  pressure events
- mouse/fallback input remains full-size and fully opaque
- instanced GPU brush stamping with dedicated paint and eraser blend pipelines
- ordered GPU smudging within the selected paint layer
- always-on centripetal Catmull–Rom stroke smoothing for fast, sparse input
- transparent paint layers composited over a configurable Background color
- chronological GPU undo/redo for strokes, layer changes, and Background
  color changes with a bounded 256 MiB history
- wheel zoom, pan, clear, fit, 100% zoom

The app opens to the gallery. Use **New Artwork** to create a 4000 × 4000
artwork named `Untitled`, or select an existing thumbnail to continue editing
with its layers intact. Artwork titles do not need to be unique. Gallery cards
can be renamed or permanently deleted.

Artwork changes save automatically after a short idle period. Use
`Command-S` on macOS or `Control-S` on Windows and Linux to save immediately.
Returning to the gallery and closing the app wait for pending changes to save;
a failed save is shown and must be retried or the navigation cancelled.
Artworks are stored in the platform application-data directory under
`chromazen/artworks`. Each artwork uses a private, versioned Chromazen format.
Use **Export PNG…** to flatten the current live layers over the Background and
write an opaque PNG at the canvas's native dimensions.

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

Set `active_brush = "pencil"` in `config.toml` for the Brush tool. The Eraser
and Smudge selections are stored as `eraser_brush` and `smudge_brush`. The
preset's `stamp` path is resolved relative to `brush.toml`; invalid presets
fall back to the bundled charcoal brush.

Controls:

- Left drag: use the selected tool on the selected paint layer
- `B`: select Brush
- `E`: select Eraser; erasing makes the selected layer transparent to reveal
  lower layers and the Background
- `S`: select Smudge; smudging drags colors already on the selected layer
- `Shift-Tab`: cycle through Brush, Eraser, and Smudge
- Click an inactive tool button to select it; click the active tool again to
  open the shared floating brush library. Each tool remembers its own brush.
- `Tab`: show or hide the sidebar
- Hold `Shift` and left-drag vertically on the canvas to resize the brush; drag
  up to increase its size or down to decrease it
- Hold `Option` (`Alt` outside macOS) to activate the eyedropper, then
  left-click or drag on the canvas to sample its visible composited color
- Wheel: zoom around cursor
- Middle/right drag or Space + left drag: pan
- Save artwork immediately: `Command-S` on macOS; `Control-S` on Windows and Linux
- Export PNG: `Command-Shift-E` on macOS; `Control-Shift-E` on Windows and Linux
- Undo: `Command-Z` on macOS; `Control-Z` on Windows and Linux
- Redo: `Command-Shift-Z` on macOS; `Control-Y` on Windows;
  `Control-Shift-Z` or `Control-Y` on Linux
- On macOS and Windows, create, save, export, and gallery actions are available
  from the native **File** menu; Undo and Redo are available from **Edit**
- Use the minimal egui panels for brush controls and adding, selecting, or
  deleting layers
- Click a layer's eye to show or hide it; hidden selected layers cannot be painted on
- Click the selected layer again to rename it or adjust its opacity
- Drag a paint layer row to reorder it; the Background remains fixed at the bottom
- Select **Background** in the Layers panel to change its color; it cannot be
  painted on or deleted
- On macOS and Windows, use the native **Settings** menu to save, reload, reset,
  or open the configuration folder
- Edit brush behavior in each preset's `brush.toml`
- Use **Reload** after editing TOML externally, or **Open config folder** to
  locate the files
