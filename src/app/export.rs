use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use atomic_write_file::AtomicWriteFile;
use chromazen_canvas::Canvas;

use crate::artwork::{CompositeLayer, CompositeRows};

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

    pub(super) fn start(&mut self, path: PathBuf, paint: &Canvas) -> Result<(), String> {
        if self.exporting {
            return Err("an artwork export is already in progress".to_owned());
        }
        let document = paint.document_snapshot();
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
                        .any(|((id, _), metadata)| id != &metadata.id)
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
                        clipped: metadata.clipped,
                    })
                    .collect();
                write_png_atomic(&path, &composite_layers, document.background)
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

fn write_png_atomic(
    path: &Path,
    layers: &[CompositeLayer<'_>],
    background: [u8; 3],
) -> Result<(), String> {
    let rows = CompositeRows::new(layers, background)?;
    let [width, height] = rows.size();
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|error| format!("failed to open {} for export: {error}", path.display()))?;
    {
        let mut encoder = png::Encoder::new(&mut file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("failed to begin PNG export: {error}"))?;
        {
            let mut stream = writer
                .stream_writer_with_size(64 * 1024)
                .map_err(|error| format!("failed to begin PNG rows: {error}"))?;
            let mut row = vec![0; width as usize * 4];
            for y in 0..height {
                rows.row(y, &mut row);
                stream
                    .write_all(&row)
                    .map_err(|error| format!("failed to write PNG row: {error}"))?;
            }
            stream
                .finish()
                .map_err(|error| format!("failed to finish PNG rows: {error}"))?;
        }
        writer
            .finish()
            .map_err(|error| format!("failed to finish PNG export: {error}"))?;
    }
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
    fn streamed_export_matches_legacy_flattening_across_many_rows_and_clips() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("export.png");
        let base = image::RgbaImage::from_fn(517, 261, |x, y| {
            let alpha = ((x + y) % 256) as u8;
            image::Rgba([alpha / 2, alpha / 3, alpha / 4, alpha])
        });
        let clipped = image::RgbaImage::from_fn(517, 261, |x, y| {
            let alpha = ((x * 7 + y * 3) % 256) as u8;
            image::Rgba([0, alpha / 2, 0, alpha])
        });
        let invisible = image::RgbaImage::from_pixel(517, 261, image::Rgba([255; 4]));
        let layers = [
            CompositeLayer {
                image: &base,
                visible: true,
                opacity: 67,
                clipped: false,
            },
            CompositeLayer {
                image: &clipped,
                visible: true,
                opacity: 89,
                clipped: true,
            },
            CompositeLayer {
                image: &invisible,
                visible: false,
                opacity: 100,
                clipped: false,
            },
        ];
        let expected = crate::artwork::flatten_premultiplied_layers(&layers, [17, 45, 72]).unwrap();
        write_png_atomic(&path, &layers, [17, 45, 72]).unwrap();
        assert_eq!(image::open(&path).unwrap().to_rgba8(), expected);
        let small = image::RgbaImage::new(1, 1);
        let invalid = [
            CompositeLayer {
                image: &base,
                visible: true,
                opacity: 100,
                clipped: false,
            },
            CompositeLayer {
                image: &small,
                visible: true,
                opacity: 100,
                clipped: false,
            },
        ];
        assert!(write_png_atomic(&path, &invalid, [0; 3]).is_err());
        assert_eq!(image::open(&path).unwrap().to_rgba8(), expected);
    }

    #[test]
    fn atomic_export_writes_a_decodable_png() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("export.png");
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
        write_png_atomic(
            &path,
            &[CompositeLayer {
                image: &image,
                visible: true,
                opacity: 100,
                clipped: false,
            }],
            [0; 3],
        )
        .unwrap();
        assert_eq!(image::open(path).unwrap().to_rgba8(), image);
    }
}
