# Chromazen controls

This directory contains the dependency-free web control surface. Run `npm run build` to copy the
source into `dist/` for Tauri.

The native Rust runtime remains authoritative for pointer input, pressure, stroke smoothing, stamp
generation, history, wgpu rendering, and presentation. The webview sends only low-frequency typed
commands; range inputs are coalesced to one command per animation frame.
