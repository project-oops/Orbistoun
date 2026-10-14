//! A title's mods (D836): user content that changes a title without orbistoun naming it.
//!
//! A mod is a directory of its own under the title's `mods` directory:
//!
//! ```text
//! <title directory>/mods/<name>/
//!     mod.toml      optional: `enabled = false` turns it off, `description` says what it is
//!     app0/         optional: files laid over the title's own, as its /app0 sees them
//! ```
//!
//! Every mod not turned off applies, in the order of the directories' names, so where two replace
//! the same file the later name's wins. Orbistoun ships none and changes nothing by a title's id:
//! a mod is the user's, for the titles they put it beside.

use std::path::{Path, PathBuf};

/// A mod's own description of itself, from its `mod.toml`. Every field is optional.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ModFile {
    /// Whether it applies; on unless it says otherwise.
    pub enabled: bool,
    /// What it does, for a person choosing which to turn on.
    pub description: Option<String>,
}

impl Default for ModFile {
    fn default() -> Self {
        Self {
            enabled: true,
            description: None,
        }
    }
}

/// One mod as found under a title's `mods` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mod {
    /// Its directory's name.
    pub name: String,
    /// Its directory.
    pub dir: PathBuf,
    /// What its `mod.toml` says, or the defaults where it has none.
    pub file: ModFile,
    /// Why its `mod.toml` was not read, where it has one that does not parse: such a mod is left
    /// off rather than applied on a guess.
    pub unreadable: Option<String>,
}

impl Mod {
    /// The tree it lays over the title's `/app0`, where it has one.
    #[must_use]
    pub fn app0(&self) -> Option<PathBuf> {
        let app0 = self.dir.join(APP0_DIR);
        app0.is_dir().then_some(app0)
    }

    /// Whether it applies: turned on, and its `mod.toml` read.
    #[must_use]
    pub fn applies(&self) -> bool {
        self.file.enabled && self.unreadable.is_none()
    }
}

/// The directory in a mod that is laid over the title's `/app0`.
pub const APP0_DIR: &str = "app0";

/// The file a mod describes itself in.
pub const MOD_FILE: &str = "mod.toml";

/// Every mod under `mods_dir`, in the order they apply - by name - whether or not each does. None
/// where the directory does not exist.
#[must_use]
pub fn found(mods_dir: &Path) -> Vec<Mod> {
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut mods: Vec<Mod> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let dir = entry.path();
            let (file, unreadable) = match std::fs::read_to_string(dir.join(MOD_FILE)) {
                Err(_) => (ModFile::default(), None),
                Ok(text) => match toml::from_str::<ModFile>(&text) {
                    Ok(file) => (file, None),
                    Err(e) => (ModFile::default(), Some(e.to_string())),
                },
            };
            Mod {
                name: entry.file_name().to_string_lossy().into_owned(),
                dir,
                file,
                unreadable,
            }
        })
        .collect();
    mods.sort_by(|a, b| a.name.cmp(&b.name));
    mods
}

#[cfg(test)]
mod tests {
    use super::found;

    /// Mods are found in name order, a `mod.toml` can turn one off, and one whose file does not
    /// parse is left off with the reason.
    #[test]
    fn mods_apply_in_name_order_unless_turned_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let make = |name: &str, toml: Option<&str>, app0: bool| {
            let at = dir.path().join(name);
            std::fs::create_dir_all(&at).expect("mod dir");
            if let Some(toml) = toml {
                std::fs::write(at.join(super::MOD_FILE), toml).expect("mod.toml");
            }
            if app0 {
                std::fs::create_dir_all(at.join(super::APP0_DIR)).expect("app0");
            }
        };
        make("b-textures", None, true);
        make("a-fixes", Some("description = \"fixes\"\n"), true);
        make("c-off", Some("enabled = false\n"), true);
        make("d-broken", Some("enabled = maybe\n"), false);
        std::fs::write(dir.path().join("readme.txt"), b"not a mod").expect("file");

        let mods = found(dir.path());
        let names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a-fixes", "b-textures", "c-off", "d-broken"]);
        let applying: Vec<&str> = mods
            .iter()
            .filter(|m| m.applies())
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(applying, ["a-fixes", "b-textures"]);
        assert_eq!(mods[0].file.description.as_deref(), Some("fixes"));
        assert!(mods[3].unreadable.is_some());
        assert!(mods[3].app0().is_none());
        assert!(found(&dir.path().join("none")).is_empty());
    }
}
