# Chromazen

Minimal native Rust painting application focused on brush performance.

Implemented:

- `winit` native window
- `wgpu` renderer
- `egui` artwork gallery and editor controls
- persistent editable artworks with automatic background saving
- full-resolution flattened PNG export
- custom canvas dimensions up to the renderer's safe texture and pixel limits
- per-artwork reference images arranged in the workspace outside or over the canvas
- bundled charcoal brush using the original stamp PNG
- pressure-sensitive brush size/opacity on macOS via AppKit events, Windows
  via Windows Ink pointer events, and Linux Wayland via the tablet-v2 protocol
- Linux X11, WinTab-only tablet configurations, and mouse fallback input remain
  full-size and fully opaque
- instanced GPU brush stamping with dedicated paint and eraser blend pipelines
- ordered GPU smudging within the selected paint layer
- always-on centripetal Catmull–Rom stroke smoothing for fast, sparse input
- transparent paint layers composited over a configurable Background color
- chronological GPU undo/redo for strokes, layer changes, and Background
  color changes with a bounded 256 MiB history
- wheel zoom, pan, clear, fit, 100% zoom
- non-destructive freehand canvas rotation with 90° snapping and view flipping
- destructive, undoable selected-layer move, scale, and rotation transforms
- visual, undoable canvas cropping and expansion without scaling paint

The app opens to the gallery. Use **New Artwork** to create a 4000 × 4000
artwork named `Untitled`, or select an existing thumbnail to continue editing
with its layers intact. Artwork titles do not need to be unique. Gallery cards
can be renamed or permanently deleted.

Artwork changes save automatically after a short idle period. Use
`Command-S` on macOS or `Control-S` on Windows and Linux to save immediately.
Returning to the gallery and closing the app wait for pending changes to save;
a failed save is shown and must be retried or the navigation cancelled.
Artworks are stored in the platform application-data directory under
`chromazen/artworks`. Each artwork uses a private, versioned Chromazen format. Imported reference
images are copied into that format, so their saved layout does not depend on the
original files remaining in place. References are workspace aids: they are not
included in gallery thumbnails or exported images. Use **Export PNG…** to
flatten the current live layers over the Background and write an opaque PNG at
the canvas's native dimensions.

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

Centripetal Catmull–Rom stroke smoothing is always enabled at 100% for every brush.

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

On macOS and Windows, Photoshop `.abr` files can be imported from
**File → Import Photoshop Brushes…**. Importing is a one-time conversion into
the same native `brush.toml` and `tip.png` layout, so ABR parsing never affects
painting or startup performance. Chromazen imports sampled tips from ABR
versions 1, 2, 6, 7, 9, and 10, including soft alpha, rectangular tip shape,
legacy names and spacing, PackBits compression, and 8- or 16-bit modern tips.
Modern imports use the file name plus an index and 5% spacing. Computed tips and
Photoshop-only dynamics such as dual
brushes, textures, scattering, and color dynamics are skipped or replaced by
Chromazen's pressure and spacing defaults.

Controls:

- Left drag: use the selected tool on the selected paint layer
- Pressure-sensitive pens are supported on macOS, Windows Ink, and Linux
  Wayland. Enable Windows Ink in the tablet driver on Windows. Linux X11 and
  WinTab-only configurations rely on pointer emulation and do not expose
  pressure to Chromazen.
- Use **File → Add Reference…** to import PNG and JPEG reference images where
  the native File menu is available; image files can also be dropped into the editor
- Drag an unlocked reference to move it; drag its lower-right handle to resize
  it while preserving its aspect ratio
- Right-click a reference to lock, unlock, or delete it.
- Locked references cannot be moved or resized, but retain their right-click menu
- Reference positions and sizes use canvas-relative workspace coordinates and
  are restored when the artwork is reopened
- `B`: select Brush
- `E`: select Eraser; erasing makes the selected layer transparent to reveal
  lower layers and the Background
- `S`: select Smudge; smudging drags colors already on the selected layer
- `T`: select Transform. The frame fits the painted pixels on the selected
  layer. Drag inside it to move, drag its square handles to scale freely, or
  drag the circular handle to rotate. Hold `Shift` while scaling to preserve
  the current aspect ratio. Press `T` again or `Enter` to bake the transform, or
  `Escape` to restore the original pixels; each returns to the previously
  selected paint tool. Applied transforms are undoable.
- `Shift-Tab`: cycle through Brush, Eraser, and Smudge
- Click an inactive tool button to select it; click the active tool again to
  open the shared floating brush library. Each tool remembers its own brush.
- On macOS and Windows, use **File → Import Photoshop Brushes…** to import one
  or more `.abr` files; the first imported brush is selected automatically.
- `Tab`: show or hide the Color and Layers panels
- Hold `Shift` and left-drag vertically on the canvas to resize the brush; drag
  up to increase its size or down to decrease it
- Hold `Option` (`Alt` outside macOS) to activate the eyedropper, then
  left-click or drag on the canvas to sample its visible composited color
- Wheel: zoom around cursor
- Middle/right drag or Space + left drag: pan
- Hold `R` and left-drag to rotate the canvas view freely; rotation snaps near each 90° increment
- `Shift-R`: reset canvas rotation
- Rotate the view left/right 90°: `Command-Option-Left/Right` on macOS;
  `Control-Alt-Left/Right` on Windows and Linux
- Flip the view horizontally/vertically: `Command-Option-H/V` on macOS;
  `Control-Alt-H/V` on Windows and Linux
- Crop or resize the canvas: `Command-Option-C` on macOS; `Control-Alt-C` on
  Windows and Linux. Drag an edge or corner handle, or drag inside the crop to
  reposition it. Press `Enter` to apply or `Escape` to cancel. Expanded areas
  are transparent and paint is not scaled.
- Save artwork immediately: `Command-S` on macOS; `Control-S` on Windows and Linux
- Export PNG: `Command-Shift-E` on macOS; `Control-Shift-E` on Windows and Linux
- Undo: `Command-Z` on macOS; `Control-Z` on Windows and Linux
- Redo: `Command-Shift-Z` on macOS; `Control-Y` on Windows;
  `Control-Shift-Z` or `Control-Y` on Linux
- On macOS and Windows, create, save, export, reference and Photoshop brush
  import, and gallery actions are available from the native **File** menu; Undo
  and Redo are available from **Edit**
- Use the minimal egui panels for brush controls and adding, selecting, or
  deleting layers
- Click a layer's eye to show or hide it; hidden selected layers cannot be painted on
- Right-click a layer to rename it, merge it down, or clip it to the transparency of the nearest non-clipped layer below; clipped layers are marked with `↳`
- Adjust the selected layer's opacity beside the add and delete controls
- Drag a paint layer's grip to reorder it; the Background remains fixed at the bottom
- Select **Background** in the Layers panel to change its color; it cannot be
  painted on or deleted
- On macOS and Windows, use the native **Settings** menu to save, reload, reset,
  or open the configuration folder
- Edit brush behavior in each preset's `brush.toml`
- Use **Reload** after editing TOML externally, or **Open config folder** to
  locate the files
