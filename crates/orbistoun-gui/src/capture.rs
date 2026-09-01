//! Turning a frame into a file on disk.
//!
//! # What this captures, and what it does not
//!
//! **The emulator window.** Not a guest frame - there is not one yet. No title in the
//! corpus reaches its own main loop, nothing has been submitted to a command buffer, and
//! `orbistoun-video` has no output surface (`docs/PROJECT_STATUS.md`).
//!
//! That is worth saying plainly rather than shipping a button labelled *screenshot* and
//! letting somebody assume it means what it means in every other emulator. What it does
//! capture is genuinely worth having: this window is a dense diagnostic surface - a call
//! tail, a register dump, a ranked finding list - and "paste the panel that says this"
//! currently means reaching for an operating-system screen grab.
//!
//! When phase 6 lands and there *is* a guest frame, this is the seam it arrives at: the
//! composition changes, the encoding and the naming do not.
//!
//! # Why the request and the reply are separate
//!
//! A window's pixels are not available to the code drawing it. Asking egui for them is a
//! viewport command, and the answer comes back as an input event on a **later frame** -
//! so a capture is two halves that cannot be collapsed into one function call, however
//! much a button handler would like it to be.

use std::io::Write as _;

use image::ImageEncoder as _;

/// Where a capture went, or why it did not.
///
/// Reported rather than logged. A file written somewhere the user cannot see is the same
/// to them as no file at all, and a failure that only reaches a log is worse - the button
/// looked like it worked.
pub(crate) type Outcome = Result<std::path::PathBuf, String>;

/// Writes one frame out as a PNG, and says where it went.
///
/// The name carries the title it was taken against, so a directory of captures can be read
/// without opening any of them, and a millisecond timestamp so they sort in the order they
/// were taken. Same shape as a run id (`orbistoun-report`), for the same reason.
pub(crate) fn save(
    dir: &std::path::Path,
    label: Option<&str>,
    image: &egui::ColorImage,
    unix_ms: u64,
) -> Outcome {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        // A zero-sized frame writes a file no viewer will open. Refusing says so at the
        // moment it happened rather than when somebody double-clicks it (principle 3).
        return Err("the window reported a frame with no pixels".to_owned());
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}-{unix_ms:013}.png", stem(label)));

    // `Color32` is premultiplied RGBA in memory and that is exactly what a PNG wants, so
    // this is a copy rather than a conversion.
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }

    let file =
        std::fs::File::create(&path).map_err(|e| format!("creating {}: {e}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    // The encoder is named rather than inferred from the extension: format guessing needs
    // the whole codec zoo compiled in, and this crate deliberately carries PNG only.
    image::codecs::png::PngEncoder::new(&mut writer)
        .write_image(
            &rgba,
            u32::try_from(width).map_err(|_| "the frame is wider than a PNG can be")?,
            u32::try_from(height).map_err(|_| "the frame is taller than a PNG can be")?,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("encoding {}: {e}", path.display()))?;
    // Flushed explicitly. A `BufWriter` dropped on the way out of a function reports
    // nothing when the final write fails, and a truncated PNG is the one outcome that
    // looks like success from here.
    writer
        .flush()
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// A filename stem that is safe on every platform this runs on.
///
/// A title's name comes out of its own metadata, so it can hold anything - a colon, a
/// slash, a character Windows refuses outright. Anything that is not plainly a filename
/// character becomes `-`, and a name that survives nothing falls back rather than
/// producing a file called `.png`.
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
        // Bounded, because a metadata title can be a sentence and a path has a limit.
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
        // Enough to prove a real PNG rather than an empty file: the eight-byte signature.
        let bytes = std::fs::read(&path).expect("reading it back");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    }

    #[test]
    fn a_frame_with_no_pixels_is_refused_rather_than_written() {
        // A zero-byte file that no viewer opens is the failure mode that looks like
        // success from the toolbar, which is the one worth refusing (principle 3).
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
        // Titles come out of a guest's own metadata and are not filenames. This one is
        // three separate ways to fail on Windows in a single string.
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
