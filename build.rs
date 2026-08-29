fn main() {
    // Embed the app icon as an exe resource on Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/Chromazen.ico")
            .compile()
            .expect("failed to compile Windows resources");
    }
}
