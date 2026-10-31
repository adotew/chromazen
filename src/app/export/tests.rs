use super::*;

#[test]
fn export_filename_is_derived_from_the_title() {
    assert_eq!(
        default_export_filename("  Evening Study  "),
        "Evening Study.png"
    );
    assert_eq!(default_export_filename("a/b:c"), "a-b-c.png");
    assert_eq!(default_export_filename("..."), "Untitled.png");
    assert_eq!(default_export_filename("CON"), "_CON.png");
}

#[test]
fn png_extension_is_added_or_corrected() {
    assert_eq!(
        ensure_png_extension(PathBuf::from("study")),
        PathBuf::from("study.png")
    );
    assert_eq!(
        ensure_png_extension(PathBuf::from("study.jpg")),
        PathBuf::from("study.png")
    );
    assert_eq!(
        ensure_png_extension(PathBuf::from("study.PNG")),
        PathBuf::from("study.PNG")
    );
}

#[test]
fn atomic_export_writes_a_decodable_png() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("export.png");
    let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
    write_png_atomic(&path, &image).unwrap();
    assert_eq!(image::open(path).unwrap().to_rgba8(), image);
}
