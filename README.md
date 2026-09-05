<p align="center">
  <img src="assets/app-icon-source.png" alt="Chromazen icon" width="128">
</p>

<h1 align="center">Chromazen</h1>

<p align="center">
  A fast, native painting app built for a focused drawing experience.
  <br>
  <a href="#about">About</a> · <a href="#getting-started">Getting started</a> · <a href="#controls">Controls</a>
</p>

## About

Chromazen is a lightweight painting app for macOS, Windows, and Linux. It keeps the interface out of the way while providing the essentials for sketching and painting:

- Pressure-sensitive brushes, erasing, and smudging
- Paint layers, clipping, opacity, and undo/redo
- Canvas zoom, pan, rotation, flipping, cropping, and transforms
- Reference images beside or over the canvas
- Automatic saving and full-resolution PNG export
- Bundled brush presets and Photoshop `.abr` import on macOS and Windows

Tablet pressure works through AppKit on macOS, Windows Ink on Windows, and tablet-v2 on Linux Wayland. Mouse input also works everywhere.

## Getting started

To run from source, install the [Rust toolchain](https://rustup.rs/), clone this repository, and run:

```sh
cargo run --release
```

The app opens to the artwork gallery. Create an artwork or select an existing one to begin. Changes save automatically; use **File → Export PNG…** to export the finished image.

## Controls

Press `?` or choose **Help → Keyboard Shortcuts** for the complete shortcut list.

| Action | Control |
| --- | --- |
| Brush / Eraser / Smudge / Transform | `B` / `E` / `S` / `T` |
| Show or hide sidebar | `Tab` |
| Resize brush | `Shift` + left-drag vertically |
| Eyedropper | Hold `Option` (macOS) or `Alt` |
| Zoom | Mouse wheel |
| Pan | Middle/right-drag or `Space` + left-drag |
| Rotate view | `R` + left-drag |
| Save now | `Command-S` (macOS) or `Control-S` |
| Export PNG | `Command-Shift-E` (macOS) or `Control-Shift-E` |
| Undo | `Command-Z` (macOS) or `Control-Z` |

Drop PNG or JPEG files into the editor to add reference images. On macOS and Windows, references and Photoshop brushes can also be imported from the **File** menu.
