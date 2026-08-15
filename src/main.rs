#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

#[cfg(not(target_arch = "wasm32"))]
mod app;
mod artwork;
mod config;
mod gpu;
mod paint;
#[cfg(not(target_arch = "wasm32"))]
mod platform;
mod renderer;
#[cfg(target_arch = "wasm32")]
mod web_app;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::init();
    app::run();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
