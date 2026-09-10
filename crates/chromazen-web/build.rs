use std::{env, fs, path::PathBuf};

fn main() {
    let source = "../../assets/charcoal.png";
    println!("cargo:rerun-if-changed={source}");

    let stamp = image::open(source)
        .expect("bundled charcoal stamp is valid")
        .into_rgba8();
    let alpha: Vec<_> = stamp.pixels().map(|pixel| pixel[3]).collect();
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    fs::write(output.join("charcoal.alpha"), alpha).expect("charcoal stamp can be written");
}
