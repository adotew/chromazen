// Hide the console window in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod artwork;
mod config;
mod gpu;
mod paint;
mod platform;
mod renderer;

fn main() {
    env_logger::init();
    app::run();
}
