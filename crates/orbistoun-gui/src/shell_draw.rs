//! Categories along a row, the selected one's items down a column.
//!
//! The arrangement is a generic shape for a controller-driven library; no artwork, motion,
//! sound or proportions are copied. Everything is reachable with four directions and one
//! button. The navigation rules live in `orbistoun_shell::Cross`, where they are tested.

use orbistoun_shell::{Cross, Settings};

use crate::shell::{Action, Tile};

/// One heading on the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    /// Who is signed in.
    User,
    /// The library.
    Titles,
    /// Everything configurable.
    Settings,
    /// Ending things.
    Power,
}

impl Category {
    /// The row, in order.
    ///
    /// The user heads the row, but the highlight starts on titles (see [`Self::START`]).
    pub(crate) const ROW: [Self; 4] = [Self::User, Self::Titles, Self::Settings, Self::Power];

    /// Where the highlight starts.
    pub(crate) const START: usize = 1;

    /// The heading.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Titles => "titles",
            Self::Settings => "settings",
            Self::Power => "power",
        }
    }
}

/// What sits under `settings`.
const SETTINGS_ITEMS: [(&str, Action); 3] = [
    ("console and controllers", Action::Settings),
    ("rescan the library", Action::Rescan),
    ("developer list view", Action::ToList),
];

/// How many items each category holds, in row order.
///
/// Handed to `Cross` so it can clamp. Computed from what is drawn, so the highlight always
/// lands on a drawn item.
pub(crate) fn shape(titles: usize, running: bool) -> Vec<usize> {
    vec![
        1,
        titles,
        SETTINGS_ITEMS.len(),
        // "quit the title" only exists while there is one.
        1 + usize::from(running),
    ]
}

/// Draws the shell.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    settings: &Settings,
    library: Result<&[Tile<'_>], &str>,
    icons: &mut crate::icons::Icons,
    build: &str,
    running: Option<&str>,
    at: &mut Cross,
) -> Option<Action> {
    let mut action = None;
    let tiles: &[Tile<'_>] = library.unwrap_or(&[]);

    // The row. Clicking a heading moves the highlight to it, so a pointer and a controller
    // drive the same state.
    ui.horizontal(|ui| {
        for (index, category) in Category::ROW.iter().enumerate() {
            let selected = index == at.category;
            let text =
                egui::RichText::new(category.label()).size(if selected { 22.0 } else { 17.0 });
            if ui.selectable_label(selected, text).clicked() {
                at.category = index;
                at.clamp(&shape(tiles.len(), running.is_some()));
            }
            ui.add_space(18.0);
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        match Category::ROW[at.category] {
            Category::User => {
                ui.add_space(8.0);
                let signed_in = settings
                    .current()
                    .map_or("nobody is signed in", |user| user.name.as_str());
                if row(ui, at.item == 0, signed_in, None, icons).clicked() {
                    action = Some(Action::Settings);
                }
                ui.small("change this under settings");
            }
            Category::Titles => match library {
                // Reported where the titles would be, since it explains the empty column.
                Err(why) => {
                    ui.add_space(8.0);
                    ui.label("the library could not be read");
                    ui.weak(why);
                    if ui.button("look again").clicked() {
                        action = Some(Action::Rescan);
                    }
                }
                Ok([]) => {
                    ui.add_space(8.0);
                    ui.label("no titles here yet");
                    ui.weak("drop a title's folder into the library and look again");
                    if ui.button("look again").clicked() {
                        action = Some(Action::Rescan);
                    }
                }
                Ok(tiles) => {
                    for (index, tile) in tiles.iter().enumerate() {
                        if row(ui, index == at.item, tile.title, Some(tile), icons).clicked() {
                            action = Some(Action::Launch(index));
                        }
                    }
                }
            },
            Category::Settings => {
                for (index, (label, what)) in SETTINGS_ITEMS.iter().enumerate() {
                    if row(ui, index == at.item, label, None, icons).clicked() {
                        action = Some(*what);
                    }
                }
            }
            Category::Power => {
                let mut index = 0;
                if let Some(module) = running {
                    if row(ui, index == at.item, "quit the title", None, icons).clicked() {
                        action = Some(Action::Quit);
                    }
                    ui.small(module);
                    index += 1;
                }
                if row(ui, index == at.item, "close orbistoun", None, icons).clicked() {
                    action = Some(Action::CloseEmulator);
                }
                // Stated rather than offered as a button for a feature that does not exist.
                ui.small("suspending a title to disk is not built");
            }
        }
    });

    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.weak(build);
    });

    action
}

/// One item in the column, with artwork when it has any.
///
/// Selection is drawn rather than left to hover, because a controller moves the highlight
/// as often as a pointer does.
fn row(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    tile: Option<&Tile<'_>>,
    icons: &mut crate::icons::Icons,
) -> egui::Response {
    /// Height of one row, and the side of its artwork.
    const ROW: f32 = 56.0;

    let response = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                if let Some(tile) = tile {
                    match icons
                        .get(ui.ctx(), tile.key, tile.icon)
                        .map(egui::TextureHandle::id)
                    {
                        Some(texture) => {
                            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                                texture,
                                egui::vec2(ROW, ROW),
                            )));
                        }
                        // A title with no artwork occupies the same width, so labels align.
                        None => {
                            ui.add_sized(
                                egui::vec2(ROW, ROW),
                                egui::Label::new(first_glyph(label)),
                            );
                        }
                    }
                }
                let text = egui::RichText::new(label).size(if selected { 19.0 } else { 16.0 });
                ui.add(egui::Label::new(if selected {
                    text.strong()
                } else {
                    text
                }));
            });
        })
        .response;
    // The whole row is the click target, not just the label.
    let response = response.interact(egui::Sense::click());
    if selected {
        ui.painter()
            .rect_stroke(response.rect, 4.0, ui.visuals().selection.stroke);
    }
    response
}

/// A stand-in for a title that ships no artwork.
fn first_glyph(title: &str) -> String {
    title
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().to_string())
}

#[cfg(test)]
mod tests {
    use super::{Category, first_glyph, shape};

    /// A title with artwork and one without both get something to look at.
    #[test]
    fn a_title_with_no_artwork_still_gets_a_distinguishable_row() {
        assert_eq!(first_glyph("Some Title"), "S");
        assert_eq!(first_glyph("  spaced"), "S");
        assert_eq!(first_glyph("---"), "?");
        assert_eq!(first_glyph(""), "?");
    }

    /// The shape has one entry per heading, so every drawn category is navigable.
    #[test]
    fn the_shape_describes_every_category() {
        assert_eq!(shape(7, false).len(), Category::ROW.len());
    }

    /// Power gains its "quit the title" item only while a title is running.
    #[test]
    fn quitting_is_only_offered_when_there_is_something_to_quit() {
        assert_eq!(shape(0, false)[3], 1);
        assert_eq!(shape(0, true)[3], 2);
    }

    /// An empty library is still a category somebody can be looking at.
    #[test]
    fn an_empty_library_is_a_reachable_screen() {
        assert_eq!(shape(0, false)[1], 0);
    }

    /// The highlight starts on the library rather than on the first heading.
    #[test]
    fn the_highlight_starts_where_somebody_was_looking_for() {
        assert_eq!(Category::ROW[Category::START], Category::Titles);
    }
}
