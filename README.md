# Chromazen

Chromazen is a Tauri desktop painting application with a web control surface and a native Rust/wgpu
canvas. Pointer input, tablet pressure, stroke smoothing, stamp generation, history, rendering, and
presentation never cross IPC.

## Architecture

- `src/` is the reusable Rust paint engine: configuration, brush behavior, smoothing, history,
  layers, pressure integration, and wgpu rendering.
- `src-tauri/` owns the desktop lifecycle, raw Tao input adapter, native menus, capabilities, and
  the bounded control-command queue.
- `ui/` is the dependency-free web control surface. It receives revisioned snapshots and sends
  only low-frequency commands such as tool, brush, layer, and settings changes.
- The native wgpu surface renders directly into the window. The child webview is bounded to the
  300-pixel controls region; there is no web canvas, frame readback, or pixel transfer.

## Build and run

From the repository root:

```bash
npm --prefix ui run build
cargo run --release
```

The UI build uses only Node’s standard library. `cargo run` selects the Tauri workspace member by
default.

Verification:

```bash
npm --prefix ui run check
npm --prefix ui run build
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Set `CHROMAZEN_PERF=1` when launching to log native input-to-stamp, submit, and present latency
percentiles every five seconds.

## Painting controls

- Left drag: paint with the selected tool on the selected paint layer.
- `B`, `E`, `S`: select Brush, Eraser, or Smudge.
- Wheel: zoom around the cursor.
- Middle/right drag or Space + left drag: pan.
- Undo: `Command-Z` on macOS; `Control-Z` on Windows and Linux.
- Redo: `Command-Shift-Z` on macOS; `Control-Y` on Windows;
  `Control-Shift-Z` or `Control-Y` on Linux.
- The web panel controls brushes, color, smoothing, layers, canvas fitting, and settings.
- Tauri’s native Edit and Settings menus provide history and configuration actions.

## Settings and brushes

Settings are loaded from `config.toml` in the platform configuration directory:

- Linux: `~/.config/chromazen/config.toml`
- macOS: `~/Library/Application Support/chromazen/config.toml`
- Windows: the user’s roaming application-data directory

Use **Settings → Save Settings** to write it atomically. Stroke smoothing is global:

```toml
[smoothing]
strength = 0.8 # from greater than 0.0 through 1.0
```

Custom brush presets live under `brushes/<id>/` in that directory:

```text
brushes/pencil/
├── brush.toml
└── tip.png
```

The preset’s `stamp` path is resolved relative to `brush.toml`. Invalid or unavailable presets fall
back to the bundled charcoal brush.
