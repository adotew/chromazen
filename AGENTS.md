# Repository Guidelines

## Project Structure & Module Organization

Chromazen is a single native Rust binary (`edition = "2024"`). `src/main.rs` initializes logging and starts the winit application in `src/app.rs`. The `src/app/` modules own event handling, commands, menus, editor/gallery UI, autosave, settings, export, brush import, and reference-image loading. Keep UI-only behavior there rather than in the renderer.

Artwork persistence lives under `src/artwork/`: `format.rs` defines and validates versioned manifests, `store.rs` manages revision directories and atomic commits, and `raster.rs` handles CPU compositing and PNG encoding. Configuration and brush preset discovery/import are under `src/config/`; stroke models and smoothing are under `src/paint/`.

Platform input belongs in `src/platform/`, behind the appropriate `cfg` gates: AppKit pressure on macOS, Windows Ink on Windows, and Wayland tablet-v2 on Linux. `src/gpu.rs` owns wgpu device/surface setup. `src/renderer.rs` and `src/renderer/` own canvas state, layers, history, persistence readback, stamps, sampling, resources, and view transforms; WGSL programs live in `src/renderer/shaders/`.

Keep bundled brushes, icons, fonts, and app icons in `assets/`. macOS bundle metadata and assembly scripts live in `packaging/macos/`. Treat `target/` and `dist/` as generated output. Unit tests stay beside their implementation in `#[cfg(test)]` modules.

## Build, Test, and Development Commands

Use a current stable Rust toolchain with edition 2024 support.

- `cargo run --release` builds and launches the performance-oriented application.
- `cargo build` performs a faster debug build.
- `cargo test` runs the colocated unit tests.
- `cargo fmt --all -- --check` verifies formatting; `cargo fmt --all` applies it.
- `cargo clippy --all-targets --all-features -- -D warnings` treats every lint as an error.
- `./packaging/macos/build-app.sh` builds a release binary, creates and ad-hoc signs `dist/Chromazen.app`, then replaces `/Applications/Chromazen.app`; run it only on macOS when that side effect is intended.

Run formatting, tests, and Clippy before submitting changes.

## Coding Style & Architecture Constraints

Follow `rustfmt` defaults. Use `snake_case` for modules, functions, and variables; `UpperCamelCase` for structs and enums; and `SCREAMING_SNAKE_CASE` for constants. Prefer small, direct helpers over new abstractions or dependencies.

Preserve these boundaries and invariants:

- Route editor and native-menu actions through the existing command/controller flow instead of duplicating state transitions.
- Keep filesystem work and image encoding off the interactive rendering path; reuse the existing completion-channel controllers.
- Preserve atomic configuration, artwork revision, autosave, and export writes. Schema changes require version handling, validation, migration where applicable, and regression tests.
- Be explicit about document, workspace, window, physical-pixel, and texture coordinates. Maintain premultiplied-alpha assumptions in CPU and GPU compositing.
- Keep Rust buffer layouts synchronized with WGSL structs, and document non-obvious GPU copy/alignment or resource-lifetime requirements.
- Isolate platform APIs in `src/platform/` and provide safe fallback behavior on unsupported targets.

## Testing Guidelines

Name tests after observable behavior, such as `malformed_config_is_reported_and_preserved`. Add focused regression tests for changes to parsing, migration, persistence, smoothing, coordinate transforms, layer ordering, history metadata, shortcut mapping, stamp batching, and platform-independent input logic. Use temporary directories for filesystem tests.

GPU command correctness and native tablet/menu integration may be checked manually when no reusable headless harness exists. For user-visible changes, exercise the gallery-to-editor flow, autosave/reopen, PNG export, undo/redo, layer operations, references, and affected shortcuts. Test native menus and pressure input on each supported platform touched by the change.

## Commit & Pull Request Guidelines

Use short imperative commit subjects consistent with recent history, such as `Add bristle brush`, `Move toolbar to top center`, and `Fix stroke smoothing at full strength`. Keep each commit to one coherent change.

Pull requests should describe the user-visible effect, list verification commands and manually tested platforms, and link relevant issues. Include screenshots or a short capture for UI/rendering changes. Explicitly call out artwork/config schema changes, shader or GPU-layout changes, persistence effects, new assets, and platform-specific behavior.
