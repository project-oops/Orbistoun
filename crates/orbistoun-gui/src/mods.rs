//! A title's mods, listed with a switch each (D836).
//!
//! The directory is the list: a mod is installed by putting its directory under the title's
//! `mods`, and this window only reads what is there and turns each on or off in its `mod.toml`.

use orbistoun_overrides::mods::{self, Mod};

/// One mod, with what it holds read once rather than every frame.
struct Row {
    found: Mod,
    /// What it changes - files, patches, cheats - or why a file of it does not read.
    holds: String,
}

/// The mods window for one title, while it is open.
pub(crate) struct ModsWindow {
    /// Which title, by its directory's name, as a run finds its mods.
    title: String,
    dir: std::path::PathBuf,
    rows: Vec<Row>,
    /// What happened to the last switch.
    status: Option<Result<String, String>>,
}

/// What a mod changes, in a few words.
fn holds(found: &Mod) -> String {
    let mut parts = Vec::new();
    if found.app0().is_some() {
        parts.push("files".to_owned());
    }
    match found.patches() {
        Ok(patches) if !patches.is_empty() => parts.push(format!("{} patch(es)", patches.len())),
        Ok(_) => {}
        Err(why) => parts.push(format!("{} unreadable: {why}", mods::PATCH_FILE)),
    }
    match found.cheats() {
        Ok(cheats) if !cheats.is_empty() => parts.push(format!("{} cheat(s)", cheats.len())),
        Ok(_) => {}
        Err(why) => parts.push(format!("{} unreadable: {why}", mods::CHEAT_FILE)),
    }
    if parts.is_empty() {
        "nothing yet".to_owned()
    } else {
        parts.join(", ")
    }
}

impl ModsWindow {
    /// Reads `title`'s mods from `dir`.
    pub(crate) fn open(title: String, dir: std::path::PathBuf) -> Self {
        let mut window = Self {
            title,
            dir,
            rows: Vec::new(),
            status: None,
        };
        window.rescan();
        window
    }

    fn rescan(&mut self) {
        self.rows = mods::found(&self.dir)
            .into_iter()
            .map(|found| Row {
                holds: holds(&found),
                found,
            })
            .collect();
    }

    /// Draws it; false once closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let mut rescan = false;
        egui::Window::new(format!("mods - {}", self.title))
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.small(concat!(
                    "Each directory under the title's mods folder is one mod, applied at the next ",
                    "start in name order: app0/ laid over the title's files, patches.toml and ",
                    "cheats.toml written into its code and memory."
                ));
                ui.separator();
                if self.rows.is_empty() {
                    ui.weak("no mods - put one in the mods folder and rescan");
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for row in &mut self.rows {
                            ui.horizontal(|ui| {
                                let mut on = row.found.applies();
                                let switch = ui.add_enabled(
                                    row.found.unreadable.is_none(),
                                    egui::Checkbox::new(&mut on, &row.found.name),
                                );
                                if switch.changed() {
                                    self.status = Some(
                                        row.found
                                            .set_enabled(on)
                                            .map(|()| {
                                                format!(
                                                    "{} {} from the next start",
                                                    row.found.name,
                                                    if on { "applies" } else { "is off" }
                                                )
                                            })
                                            .map_err(|why| format!("{}: {why}", row.found.name)),
                                    );
                                }
                                ui.weak(&row.holds);
                            });
                            if let Some(why) = &row.found.unreadable {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    format!(
                                        "left off: its {} does not parse: {why}",
                                        mods::MOD_FILE
                                    ),
                                );
                            } else if let Some(what) = &row.found.file.description {
                                ui.small(what);
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("open mods folder").clicked() {
                        let _ = std::fs::create_dir_all(&self.dir);
                        crate::payloads::open_in_file_manager(&self.dir);
                    }
                    if ui.button("rescan").clicked() {
                        rescan = true;
                    }
                });
                if let Some(status) = &self.status {
                    match status {
                        Ok(message) => ui.small(message),
                        Err(error) => ui.colored_label(egui::Color32::LIGHT_RED, error),
                    };
                }
            });
        if rescan {
            self.rescan();
        }
        open
    }
}
