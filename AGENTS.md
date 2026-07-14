# Repository Guidelines

## Project Structure & Module Organization

`src/main.rs` starts the native application. Application state, commands, menus, input, and UI live under `src/app/`; configuration parsing and brush preset models are in `src/config/`; painting behavior and stroke smoothing are in `src/paint/`. Platform-specific input, including macOS pressure handling, belongs in `src/platform/`. GPU rendering is organized under `src/renderer/`, with WGSL programs in `src/renderer/shaders/`. Keep bundled runtime images in `assets/`. Tests currently live beside their implementation in `#[cfg(test)]` modules rather than in a separate `tests/` directory.

## Build, Test, and Development Commands

- `cargo run --release` builds and launches the performance-oriented application described in the README.
- `cargo build` performs a fast debug build while developing.
- `cargo test` runs all unit tests, including configuration, smoothing, menu, and renderer-stamp tests.
- `cargo fmt --all -- --check` verifies standard Rust formatting; run `cargo fmt --all` to apply it.
- `cargo clippy --all-targets --all-features -- -D warnings` treats lint findings as failures.

Use the native Settings menu on macOS or Windows when manually testing configuration save, reload, and reset behavior.

## Coding Style & Naming Conventions

Follow `rustfmt` defaults (four-space indentation). Use `snake_case` for modules, functions, and variables; `UpperCamelCase` for structs, enums, and traits; and `SCREAMING_SNAKE_CASE` for constants. Prefer small modules with explicit responsibilities, and keep platform code behind appropriate `cfg` gates. Document invariants around GPU resources, coordinate transforms, pressure input, and interpolation where behavior is not obvious.

## Testing Guidelines

Add focused `#[test]` cases in a local `mod tests`. Name tests after observable behavior, such as `invalid_strength_uses_default`. Cover edge cases for parsing, fallback behavior, stroke geometry, and batching. No coverage threshold is configured, but changes to pure logic should include regression tests. Run `cargo test` and Clippy before submitting.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects such as `Connect native settings menu` and `Avoid decoding brushes during discovery`. Keep each commit scoped to one coherent change. Pull requests should explain the user-visible effect, list verification commands and tested platforms, and link relevant issues. Include screenshots or a brief capture for UI or rendering changes; call out configuration-format, shader, or platform-specific impacts explicitly.
