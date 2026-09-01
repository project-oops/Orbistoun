//! Categories along a row, their children down a column.
//!
//! # The arrangement, and why it is ours
//!
//! A row of categories with the selected one's children underneath is how console shells
//! have shown a library for twenty years, and it is a *shape* rather than a design: it
//! comes from having a pad with a directional control and more things than fit on a screen.
//! What is somebody's design is the artwork, the motion, the sounds and the exact
//! proportions, and none of that is copied here (principle 2, D313).
//!
//! It earns its place for a reason beyond looking right: **the whole thing is reachable
//! with four directions and one button**, which is what makes a controller a way to use
//! this rather than a thing the emulator merely reads.
//!
//! # Where the navigation lives
//!
//! Not here. `orbistoun_shell::Cross` holds where the highlight is and every rule about
//! moving it, because those rules are all edges - the ends of a row, a column shorter than
//! the one beside it, a category with nothing in it - and a draw function is where edges go
//! to be discovered by somebody holding a direction until something looks wrong.

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
    /// Titles is not first, deliberately: the row reads left to right and the user belongs
    /// at the start of it, but the *highlight* starts on titles because that is what
    /// somebody opened the shell to look at.
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
/// Handed to `Cross` so it can clamp. Computed from the same things that get drawn, so the
/// highlight cannot be somewhere the drawing does not put an item.
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

    // The row. Clicking a heading moves the highlight to it, so the same screen works with
    // a pointer and with four directions - neither is a second implementation of the other.
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
                // Reported where the titles would be. A folder that cannot be read is the
                // answer to "why is this empty", and it belongs where the reader is looking.
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
                // Said rather than offered, because a button whose label claims a feature
                // that does not exist is principle 3 one level up from the emulator.
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
/// Selection is drawn rather than left to hover, because the highlight is moved by a pad as
/// often as by a pointer and a hover style would leave a controller user unable to see where
/// they are.
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
                        // A title with no artwork still occupies the same width, so the
                        // labels stay in one column rather than stepping in and out.
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
    // The whole row, not just the label - a pointer aiming at a sixteen-point word inside a
    // fifty-six point row is aiming at the wrong thing.
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

    /// **The shape has one entry per heading.**
    ///
    /// `Cross` clamps against this, so a shape shorter than the row would let the highlight
    /// sit on a category that is drawn but cannot be navigated to.
    #[test]
    fn the_shape_describes_every_category() {
        assert_eq!(shape(7, false).len(), Category::ROW.len());
    }

    /// **Power gains an item only while something is running.**
    ///
    /// The highlight is clamped against this, so an off-by-one here would leave "quit the
    /// title" selectable with no title - or unreachable with one.
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
