//! The shell, drawn.
//!
//! # What this is, and what it deliberately is not
//!
//! A console presents its library as a wall of tiles you move through with a pad, and this
//! draws one. It is **our own presentation**: principle 2 keeps vendor names out of the
//! tree, and reproducing a console's actual look would give back exactly the clean-room
//! position the rest of the project is built to hold. Tiles in a grid is how every library
//! since the CD era has been shown; a specific arrangement of them is somebody's design.
//!
//! # Why it holds no state
//!
//! Principle 13, applied inside a shim. This file takes what to draw and answers what
//! somebody did; where the session stands, what a title is and whether a run may start are
//! all decided elsewhere - in `orbistoun-shell` and the service. A view that owned a copy
//! of "which title is selected" would be a second answer to a question the window already
//! has one for.
//!
//! That is also what makes the list and the shell two presentations of one library rather
//! than two libraries.

/// What somebody did in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Start the title at this index.
    Launch(usize),
    /// Leave the shell for the list.
    ToList,
    /// Open the settings window.
    Settings,
    /// Look at the library folder again.
    Rescan,
    /// End the running title and come back here.
    Quit,
    /// Dismiss the overlay and give the title the controller back.
    Resume,
    /// Close the emulator.
    CloseEmulator,
}

/// Which menu the shell button opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Menu {
    /// A tap: what somebody wants nine times out of ten.
    Overlay,
    /// A hold: the things somebody should not reach by accident.
    Power,
}

/// Draws a menu over whatever is behind it.
///
/// # Why two menus rather than one with a section in it
///
/// The hold exists so that ending a session is not one press away from resuming it. Putting
/// both on one menu would give back exactly what the hold was for - and this is the menu
/// somebody reaches while a title is running, so a misclick has a cost.
pub(crate) fn menu(ui: &mut egui::Ui, which: Menu, running: Option<&str>) -> Option<Action> {
    let mut action = None;
    egui::Frame::popup(ui.style()).show(ui, |ui| {
        ui.set_min_width(260.0);
        match which {
            Menu::Overlay => {
                ui.heading("shell");
                if let Some(module) = running {
                    ui.weak(module);
                }
                ui.separator();
                // Resume first and largest: it is what the press was for most of the time,
                // and it is the one that must not require aiming.
                if ui.button("resume").clicked() {
                    action = Some(Action::Resume);
                }
                if ui.button("library").clicked() {
                    action = Some(Action::ToList);
                }
                if ui.button("settings").clicked() {
                    action = Some(Action::Settings);
                }
                ui.separator();
                ui.small("hold the shell button for power");
            }
            Menu::Power => {
                ui.heading("power");
                ui.separator();
                if running.is_some() && ui.button("quit the title").clicked() {
                    action = Some(Action::Quit);
                }
                if ui.button("close orbistoun").clicked() {
                    action = Some(Action::CloseEmulator);
                }
                ui.separator();
                if ui.button("back").clicked() {
                    action = Some(Action::Resume);
                }
                // Said rather than offered. A rest-mode entry that closed the emulator
                // would be a button whose label is a claim about a feature that does not
                // exist - principle 3, one level up from the emulator.
                ui.small("suspending a title to disk is not built");
            }
        }
    });
    action
}

/// One tile's worth of what the shell needs to know.
///
/// Deliberately not `Row`: this takes the two fields a tile shows, so the shell cannot
/// quietly start depending on the diagnostics the list view is built around. When a tile
/// wants a third thing, adding it here is a decision somebody makes rather than one that
/// has already happened.
pub(crate) struct Tile<'a> {
    /// The cache key for the icon, which is the directory name.
    pub(crate) key: &'a str,
    /// What to show under the tile.
    pub(crate) title: &'a str,
    /// Where the icon is, when the title ships one.
    pub(crate) icon: Option<&'a std::path::Path>,
}

/// Drawing lives next door, so this file stays the vocabulary the window acts on.
pub(crate) use crate::shell_draw::{Category, draw, shape};
