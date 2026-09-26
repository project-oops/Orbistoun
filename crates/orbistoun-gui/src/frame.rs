//! The shim's end of the frame crossing (D695): reads the region an [`Event::Frame`] names
//! and turns it into an image ready to upload as a texture.
//!
//! The read lives with the writer, in `orbistoun_worker::frame_region`, so the two sides
//! share one layout; this module only interprets the bytes as an `egui` image.

use orbistoun_proto::{Event, FrameFormat};
use std::path::Path;

/// Reads the frame `event` names, from `dir`, into an image the caller uploads with
/// `egui::Context::load_texture`.
///
/// # Errors
///
/// If the descriptor is not a frame, if the region cannot be read (a missing file, or a
/// `region` that is not a bare name, see `orbistoun_worker::frame_region::read_frame`), or if
/// its size does not match the dimensions and format; a mismatched region is corrupt.
pub(crate) fn frame_image(dir: &Path, event: &Event) -> Result<egui::ColorImage, String> {
    let &Event::Frame {
        width,
        height,
        format,
        ..
    } = event
    else {
        return Err("not a frame event".to_owned());
    };
    let bytes = orbistoun_worker::frame_region::read_frame(dir, event)
        .map_err(|e| format!("frame region unreadable: {e}"))?;
    let (w, h) = (width as usize, height as usize);
    let bytes_per_pixel = match format {
        FrameFormat::Rgba8 => 4,
    };
    let expected = w.saturating_mul(h).saturating_mul(bytes_per_pixel);
    if bytes.len() != expected {
        return Err(format!(
            "frame region is {} bytes, not the {expected} a {w}x{h} {format:?} frame needs",
            bytes.len()
        ));
    }
    Ok(match format {
        FrameFormat::Rgba8 => egui::ColorImage::from_rgba_unmultiplied([w, h], &bytes),
    })
}

#[cfg(test)]
mod tests {
    use orbistoun_proto::FrameFormat;
    use orbistoun_worker::frame_region::write_frame;

    /// A frame the worker wrote reads into an image of the right size, pixels in order.
    #[test]
    fn a_frame_the_worker_wrote_reads_into_an_image_of_the_right_size_and_pixels() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A 2x1 image: red then green, RGBA.
        let bytes = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let event = write_frame(dir.path(), 3, 2, 1, FrameFormat::Rgba8, &bytes).expect("write");

        let image = super::frame_image(dir.path(), &event).expect("image");
        assert_eq!(image.size, [2, 1]);
        assert_eq!(
            image.pixels[0],
            egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)
        );
        assert_eq!(
            image.pixels[1],
            egui::Color32::from_rgba_unmultiplied(0, 255, 0, 255)
        );
    }

    /// A region of the wrong size is refused before `from_rgba_unmultiplied` can panic on it.
    #[test]
    fn a_region_of_the_wrong_size_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let event =
            write_frame(dir.path(), 0, 1, 1, FrameFormat::Rgba8, &[1, 2, 3]).expect("write");
        assert!(super::frame_image(dir.path(), &event).is_err());
    }
}
