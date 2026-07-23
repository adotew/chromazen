# Repository Guidelines

## Project Structure & Module Organization

`src-tauri/src/main.rs` starts the desktop application. Tauri lifecycle, native menus, raw Tao input, and the typed control queue live under `src-tauri/src/`. The dependency-free web controls are authored in `ui/src/` and built into `ui/dist/`. The reusable native engine lives in `src/`: configuration and brush presets are in `src/config/`, painting behavior and stroke smoothing are in `src/paint/`, and platform-specific pressure input is in `src/platform/`. GPU rendering is organized under `src/renderer/`; stroke history and its persistent canvas mirror live in `src/renderer/history.rs`, and WGSL programs live in `src/renderer/shaders/`. Keep bundled runtime images in `assets/`. Tests live beside their implementation in `#[cfg(test)]` modules.

## Build, Test, and Development Commands

- `npm --prefix ui run build` creates the bundled control assets without external packages.
- `cargo run --release` builds and launches the Tauri application selected as the default workspace member.
- `cargo build --workspace` performs a debug build of the engine and desktop shell.
- `cargo test --workspace` runs all unit tests, including configuration, smoothing, menu, protocol, and renderer-stamp tests.
- `cargo fmt --all -- --check` verifies standard Rust formatting; run `cargo fmt --all` to apply it.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` treats lint findings as failures.

Use the native Settings menu when manually testing configuration save, reload, reset, and folder opening. Also verify undo/redo shortcuts, native Edit menu enablement, controls-webview resizing, and painting at the canvas/webview boundary.

## Coding Style & Naming Conventions

Follow `rustfmt` defaults (four-space indentation). Use `snake_case` for modules, functions, and variables; `UpperCamelCase` for structs, enums, and traits; and `SCREAMING_SNAKE_CASE` for constants. Prefer small modules with explicit responsibilities, and keep platform code behind appropriate `cfg` gates. Document invariants around GPU resources, coordinate transforms, pressure input, and interpolation where behavior is not obvious. Keep pointer movement, pressure sampling, smoothing, stamp generation, and wgpu presentation on the native event-loop path; never route stroke samples or rendered pixels through Tauri IPC.

## Testing Guidelines

Add focused `#[test]` cases in a local `mod tests`. Name tests after observable behavior, such as `invalid_strength_uses_default`. Cover edge cases for parsing, fallback behavior, stroke geometry, batching, history metadata, protocol serialization, and shortcut/menu mapping. Run `npm --prefix ui run check` for web changes. GPU copy correctness may be verified manually unless a reusable headless-wgpu test harness is added. No coverage threshold is configured, but changes to pure logic should include regression tests. Run workspace tests and Clippy before submitting.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects such as `Connect native settings menu` and `Avoid decoding brushes during discovery`. Keep each commit scoped to one coherent change. Pull requests should explain the user-visible effect, list verification commands and tested platforms, and link relevant issues. Include screenshots or a brief capture for UI or rendering changes; call out configuration-format, shader, or platform-specific impacts explicitly.
