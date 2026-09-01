//! Title icons, decoded once and kept.
//!
//! # Why a cache rather than an image widget
//!
//! Immediate mode redraws whenever the pointer moves, so anything done while drawing is
//! done sixty times a second. A title icon is a third of a megabyte of PNG; decoding one
//! per frame per row would make scrolling the library the most expensive thing this
//! program does, which would be an absurd thing to be true of a list of eight items.
//!
//! So each icon is decoded once, uploaded once, and held. The cache is keyed by the
//! title's directory name, which is unique within a library by construction.
//!
//! # Failures are cached too
//!
//! A missing or corrupt icon caches its failure, deliberately. Without that, an icon that
//! cannot be decoded is *retried every frame* - the most expensive possible response to a
//! file that will never work.

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
/// Downscaled before upload: the source is far larger than anything drawn here, and
/// holding full-size textures for a whole library is memory spent on pixels nobody sees.
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
