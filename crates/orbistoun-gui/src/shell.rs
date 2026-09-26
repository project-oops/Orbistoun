//! The shell, drawn in our own presentation rather than an imitation of the vendor's.
//!
//! It holds no state: it takes what to draw and answers what somebody did, and the session,
//! the titles and whether a run may start are decided in `orbistoun-shell` and the service.
//! The list and the shell are two presentations of one library.

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
    /// A tap: the everyday menu.
    Overlay,
    /// A hold: actions that should not be reached by accident.
    Power,
}

/// Draws a menu over whatever is behind it.
///
/// Two menus, so ending a session is never one press away from resuming it.
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
                // Resume first and largest: it is the common case.
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
                // Stated rather than offered as a button for a feature that does not exist.
                ui.small("suspending a title to disk is not built");
            }
        }
    });
    action
}

/// One tile's worth of what the shell needs to know.
///
/// Not `Row`: only the fields a tile shows, so the shell does not depend on the list
/// view's diagnostics.
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
