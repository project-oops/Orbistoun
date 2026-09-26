//! Writes a capture of the emulator window to a PNG file.
//!
//! This captures the window, a diagnostic surface, not a guest frame. A window's pixels are
//! requested with a viewport command and arrive as an input event on a later frame, so a
//! capture is a request and a separate reply.

use std::io::Write as _;

use image::ImageEncoder as _;

/// Where a capture went, or why it did not.
///
/// Reported in the window rather than logged, so the user sees where it went or why not.
pub(crate) type Outcome = Result<std::path::PathBuf, String>;

/// Writes one frame out as a PNG, and says where it went.
///
/// The name carries the title and a millisecond timestamp, so captures are identifiable
/// and sort in order, like a run id (`orbistoun-report`).
pub(crate) fn save(
    dir: &std::path::Path,
    label: Option<&str>,
    image: &egui::ColorImage,
    unix_ms: u64,
) -> Outcome {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        // A zero-sized frame would write a file no viewer opens.
        return Err("the window reported a frame with no pixels".to_owned());
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}-{unix_ms:013}.png", stem(label)));

    // `Color32` is already RGBA in memory, so this is a copy rather than a conversion.
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }

    let file =
        std::fs::File::create(&path).map_err(|e| format!("creating {}: {e}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    // The encoder is named rather than inferred from the extension, because this crate
    // carries only the PNG codec.
    image::codecs::png::PngEncoder::new(&mut writer)
        .write_image(
            &rgba,
            u32::try_from(width).map_err(|_| "the frame is wider than a PNG can be")?,
            u32::try_from(height).map_err(|_| "the frame is taller than a PNG can be")?,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("encoding {}: {e}", path.display()))?;
    // Flushed explicitly: a dropped `BufWriter` does not report a failed final write.
    writer
        .flush()
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// A filename stem that is safe on every platform this runs on.
///
/// A title's name comes from its metadata and can hold any character. Anything that is not
/// plainly a filename character becomes `-`, and an empty result falls back to a fixed stem.
fn stem(label: Option<&str>) -> String {
    let Some(label) = label else {
        return "orbistoun".to_owned();
    };
    let cleaned: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "orbistoun".to_owned()
    } else {
        // Bounded, because a path has a length limit.
        trimmed.chars().take(64).collect()
    }
}

/// Milliseconds since the epoch, for naming a capture.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::{save, stem};

    fn frame(width: usize, height: usize) -> egui::ColorImage {
        egui::ColorImage {
            size: [width, height],
            pixels: vec![egui::Color32::from_rgb(1, 2, 3); width * height],
        }
    }

    #[test]
    fn a_capture_lands_where_it_says_it_did() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = save(
            dir.path(),
            Some("PPSA02664-app0"),
            &frame(4, 3),
            1_700_000_000_123,
        )
        .expect("a frame with pixels should write");

        assert!(path.exists(), "the reported path must be the file");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("PPSA02664-app0-1700000000123.png"),
            "the name carries the title and sorts by when it was taken"
        );
        // The eight-byte signature proves a real PNG rather than an empty file.
        let bytes = std::fs::read(&path).expect("reading it back");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    }

    #[test]
    fn a_frame_with_no_pixels_is_refused_rather_than_written() {
        // A zero-sized frame is refused rather than written as an unreadable file.
        let dir = tempfile::tempdir().expect("a temp dir");
        let err = save(dir.path(), None, &frame(0, 0), 1).expect_err("must refuse");
        assert!(err.contains("no pixels"), "{err}");
        assert_eq!(
            std::fs::read_dir(dir.path())
                .expect("reading the dir")
                .count(),
            0,
            "and must not leave a file behind"
        );
    }

    #[test]
    fn a_title_that_is_not_a_filename_still_produces_one() {
        // A colon, a slash and a question mark, each invalid in a Windows filename.
        assert_eq!(stem(Some("Game: The/Sequel?")), "Game--The-Sequel");
        assert_eq!(
            stem(Some("///")),
            "orbistoun",
            "a name that survives nothing"
        );
        assert_eq!(stem(None), "orbistoun");
        assert_eq!(stem(Some(&"x".repeat(200))).len(), 64, "bounded for a path");
    }
}
