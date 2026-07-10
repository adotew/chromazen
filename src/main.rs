mod app;
mod gpu;
mod paint;
mod platform;
mod renderer;

fn main() {
    env_logger::init();
    app::run();
}
