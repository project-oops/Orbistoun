//! The system keyboard a title opened, drawn over its picture (D826).
//!
//! A plain grid of our own: the entry above, the keys below, the highlighted one filled. No
//! artwork, arrangement or proportions of anyone else's are copied. The rules - which key is where
//! and what a press does - live in `orbistoun_shell::keyboard`, where they are tested; this only
//! draws a [`View`].

use orbistoun_shell::keyboard::{COLUMNS, Key, View};

/// The panel's share of the picture's height, along its bottom.
const HEIGHT_SHARE: f32 = 0.45;

/// A small count as a length: rows, columns and spans are all far inside `u16`.
fn count(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::from(u16::MAX), f32::from)
}

/// What a key shows.
fn label(key: Key, shifted: bool) -> String {
    match key {
        Key::Char(c) if shifted => c.to_ascii_uppercase().to_string(),
        Key::Char(c) => c.to_string(),
        Key::Shift => if shifted { "abc" } else { "ABC" }.to_owned(),
        Key::Space => "space".to_owned(),
        Key::Delete => "delete".to_owned(),
        Key::Done => "done".to_owned(),
    }
}

/// Draws `view` across the bottom of `picture`.
pub(crate) fn draw(ui: &egui::Ui, picture: egui::Rect, view: &View) {
    let painter = ui.painter_at(picture);
    let panel = egui::Rect::from_min_max(
        egui::pos2(
            picture.left(),
            picture.bottom() - picture.height() * HEIGHT_SHARE,
        ),
        picture.right_bottom(),
    );
    painter.rect_filled(panel, 0.0, egui::Color32::from_black_alpha(220));

    let gap = (panel.height() * 0.02).max(2.0);
    let rows = view.rows.len().max(1);
    // The entry takes a row's height above the keys.
    let row_height = (panel.height() - gap * (count(rows) + 2.0)) / (count(rows) + 1.0);
    let column_width = (panel.width() - gap * (count(COLUMNS) + 1.0)) / count(COLUMNS);
    let text_size = (row_height * 0.45).max(8.0);

    let entry = egui::Rect::from_min_size(
        panel.min + egui::vec2(gap, gap),
        egui::vec2(panel.width() - gap * 2.0, row_height),
    );
    painter.rect_filled(entry, 4.0, egui::Color32::from_gray(235));
    let shown = format!(
        "{}_   {}/{}",
        view.text,
        view.text.chars().count(),
        view.most
    );
    painter.text(
        entry.left_center() + egui::vec2(gap * 2.0, 0.0),
        egui::Align2::LEFT_CENTER,
        shown,
        egui::FontId::monospace(text_size),
        egui::Color32::BLACK,
    );

    for (row_index, row) in view.rows.iter().enumerate() {
        let top = entry.bottom() + gap + count(row_index) * (row_height + gap);
        let mut column = 0;
        for (key_index, &(key, span)) in row.iter().enumerate() {
            let left = panel.left() + gap + count(column) * (column_width + gap);
            let width = count(span) * column_width + count(span.saturating_sub(1)) * gap;
            let rect =
                egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, row_height));
            let highlighted = view.highlight == (row_index, key_index);
            let (fill, ink) = if highlighted {
                (egui::Color32::from_rgb(70, 140, 230), egui::Color32::WHITE)
            } else {
                (egui::Color32::from_gray(60), egui::Color32::from_gray(230))
            };
            painter.rect_filled(rect, 4.0, fill);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label(key, view.shifted),
                egui::FontId::proportional(text_size),
                ink,
            );
            column += span;
        }
    }
}
