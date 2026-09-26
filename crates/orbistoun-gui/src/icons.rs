//! Title icons, decoded and uploaded once and kept.
//!
//! Immediate mode redraws every frame, so decoding an icon while drawing would repeat the
//! work each frame. The cache is keyed by the title's directory name, unique within a
//! library. A failed load is cached too, so a bad icon is not retried every frame.

use std::collections::HashMap;

/// How large an icon is drawn in the library list.
pub(crate) const LIST_ICON: f32 = 36.0;
/// How large an icon is drawn in the title header.
pub(crate) const HEADER_ICON: f32 = 96.0;

/// Decoded icons, by title directory name.
#[derive(Default)]
pub(crate) struct Icons {
    /// `None` records a title whose icon could not be loaded, so it is not retried.
    loaded: HashMap<String, Option<egui::TextureHandle>>,
}

impl Icons {
    /// The icon for a title, decoding it the first time it is asked for.
    pub(crate) fn get(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        path: Option<&std::path::Path>,
    ) -> Option<&egui::TextureHandle> {
        if !self.loaded.contains_key(key) {
            let handle = path.and_then(|path| decode(ctx, key, path));
            self.loaded.insert(key.to_owned(), handle);
        }
        self.loaded.get(key)?.as_ref()
    }

    /// Forgets everything, so a rescan picks up icons that changed on disk.
    pub(crate) fn clear(&mut self) {
        self.loaded.clear();
    }
}

/// Reads and uploads one icon.
///
/// Downscaled before upload, because the source is far larger than anything drawn here.
fn decode(ctx: &egui::Context, key: &str, path: &std::path::Path) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    // Twice the largest drawn size, so a high-density display still has pixels to use.
    let target = (HEADER_ICON * 2.0) as u32;
    let scaled = decoded.thumbnail(target, target).to_rgba8();
    let size = [scaled.width() as usize, scaled.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, scaled.as_raw());
    Some(ctx.load_texture(key, image, egui::TextureOptions::LINEAR))
}
