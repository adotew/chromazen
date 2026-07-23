mod app;
mod config;
mod gpu;
mod paint;
mod perf;
mod platform;
mod renderer;

fn main() {
    env_logger::init();
    app::run();
}
