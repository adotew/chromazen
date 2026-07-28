use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use atomic_write_file::AtomicWriteFile;

use crate::{
    artwork::{CompositeLayer, encode_png, flatten_premultiplied_layers},
    renderer::PaintRenderer,
};

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

pub(super) struct ExportCompletion {
    pub(super) path: PathBuf,
    pub(super) result: Result<(), String>,
}

pub(super) struct ExportController {
    completion_sender: mpsc::Sender<ExportCompletion>,
    completion_receiver: mpsc::Receiver<ExportCompletion>,
    wake: WakeCallback,
    exporting: bool,
}

impl ExportController {
    pub(super) fn new(wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            completion_sender,
            completion_receiver,
            wake,
            exporting: false,
        }
    }

    pub(super) fn is_exporting(&self) -> bool {
        self.exporting
    }

    pub(super) fn start(&mut self, path: PathBuf, paint: &PaintRenderer) -> Result<(), String> {
        if self.exporting {
            return Err("an artwork export is already in progress".to_owned());
        }
        let document = paint.document_manifest();
        let readback = paint.begin_document_layer_readback()?;
        self.exporting = true;

        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let layers = readback.finish()?;
                if layers.len() != document.layers.len()
                    || layers
                        .iter()
                        .zip(&document.layers)
                        .any(|((id, _), metadata)| id.0 != metadata.id)
                {
                    return Err("exported layers do not match document metadata".to_owned());
                }
                let composite_layers: Vec<_> = layers
                    .iter()
                    .zip(&document.layers)
                    .map(|((_, image), metadata)| CompositeLayer {
                        image,
                        visible: metadata.visible,
                        opacity: metadata.opacity,
                    })
                    .collect();
                let composite =
                    flatten_premultiplied_layers(&composite_layers, document.background)?;
                write_png_atomic(&path, &composite)
            })();
            let _ = sender.send(ExportCompletion { path, result });
            wake();
        });
        Ok(())
    }

    pub(super) fn take_completion(&mut self) -> Option<ExportCompletion> {
        let completion = self.completion_receiver.try_recv().ok()?;
        self.exporting = false;
        Some(completion)
    }
}

pub(super) fn choose_export_path(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name(default_export_filename(title))
        .save_file()
        .map(ensure_png_extension)
}

fn default_export_filename(title: &str) -> String {
    let mut stem: String = title
        .trim()
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '-'
            } else {
                character
            }
        })
        .collect();
    stem = stem.trim_matches([' ', '.']).to_owned();
    if stem.is_empty() {
        stem = "Untitled".to_owned();
    }
    if is_windows_reserved_name(&stem) {
        stem.insert(0, '_');
    }
    format!("{stem}.png")
}

fn is_windows_reserved_name(stem: &str) -> bool {
    let stem = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn ensure_png_extension(mut path: PathBuf) -> PathBuf {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    {
        path.set_extension("png");
    }
    path
}

fn write_png_atomic(path: &Path, image: &image::RgbaImage) -> Result<(), String> {
    let contents = encode_png(image)?;
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|error| format!("failed to open {} for export: {error}", path.display()))?;
    file.write_all(&contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    file.flush()
        .map_err(|error| format!("failed to flush {}: {error}", path.display()))?;
    file.commit()
        .map_err(|error| format!("failed to commit {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
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
}
