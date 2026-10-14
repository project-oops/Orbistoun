//! A title's mods (D836): user content that changes a title without orbistoun naming it.
//!
//! A mod is a directory of its own under the title's `mods` directory:
//!
//! ```text
//! <title directory>/mods/<name>/
//!     mod.toml      optional: `enabled = false` turns it off, `description` says what it is
//!     app0/         optional: files laid over the title's own, as its /app0 sees them
//!     patches.toml  optional: bytes replaced in the title's executable once it is linked
//!     cheats.toml   optional: values written into the running title, once or at every flip
//! ```
//!
//! A patch names where it applies by its offset from the module's base, the bytes it expects
//! there and the bytes it writes, of the same length:
//!
//! ```toml
//! [[patch]]
//! offset = 0x1a2b3c          # from the executable's base, as a disassembler shows it
//! original = "74 0e"         # what must be there: a build the patch was not made for refuses
//! bytes = "eb 0e"            # what is written
//! description = "skip the intro"
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

/// One patch from a mod's `patches.toml`, as written there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PatchEntry {
    /// The module it patches, by file name; the title's executable when left out.
    #[serde(default)]
    module: Option<String>,
    /// Where, from the module's base.
    offset: u64,
    /// The bytes that must be there, as hexadecimal pairs.
    original: String,
    /// The bytes written in their place, as many.
    bytes: String,
    /// What it does.
    #[serde(default)]
    description: Option<String>,
}

/// A `patches.toml`: its patches, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
struct PatchFile {
    #[serde(default, rename = "patch")]
    patches: Vec<PatchEntry>,
}

/// One patch, read: where, what must be there and what is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The module it patches, by file name; `None` for the title's executable.
    pub module: Option<String>,
    /// Where, from the module's base.
    pub offset: u64,
    /// The bytes that must be there.
    pub original: Vec<u8>,
    /// The bytes written in their place.
    pub bytes: Vec<u8>,
    /// What it does.
    pub description: Option<String>,
}

/// The file a mod's patches are in.
pub const PATCH_FILE: &str = "patches.toml";

/// Bytes written as hexadecimal pairs, spaced or not: `"74 0e"`, `"740e"`.
///
/// # Errors
///
/// An odd number of digits, or anything that is not one.
pub fn hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if digits.len() % 2 != 0 {
        return Err(format!("\"{text}\" is not whole bytes"));
    }
    digits
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| format!("\"{text}\" is not hexadecimal bytes"))
        })
        .collect()
}

impl Mod {
    /// Its patches, from its `patches.toml`; none where it has none.
    ///
    /// # Errors
    ///
    /// A `patches.toml` that does not parse, or a patch whose bytes are not hexadecimal or are not
    /// as many as it expects to find.
    pub fn patches(&self) -> Result<Vec<Patch>, String> {
        let Ok(text) = std::fs::read_to_string(self.dir.join(PATCH_FILE)) else {
            return Ok(Vec::new());
        };
        let file: PatchFile = toml::from_str(&text).map_err(|e| e.to_string())?;
        file.patches
            .into_iter()
            .enumerate()
            .map(|(at, entry)| {
                let original = hex_bytes(&entry.original)?;
                let bytes = hex_bytes(&entry.bytes)?;
                if original.len() != bytes.len() || bytes.is_empty() {
                    return Err(format!(
                        "patch {} at {:#x} writes {} byte(s) over {}: a patch replaces bytes in place",
                        at + 1,
                        entry.offset,
                        bytes.len(),
                        original.len()
                    ));
                }
                Ok(Patch {
                    module: entry.module,
                    offset: entry.offset,
                    original,
                    bytes,
                    description: entry.description,
                })
            })
            .collect()
    }
}

/// Where each of a mod's patches writes in a module placed at `base` whose segments span
/// `segments` (each its first address and length), checked against what is there now: every
/// patch inside one segment and finding its original bytes, or none written. `read` answers the
/// bytes at an address. All or nothing, so a mod made for another build of the title changes none
/// of it.
///
/// # Errors
///
/// The first patch that lies outside every segment, cannot be read, or finds other bytes than it
/// expects - with what it found.
pub fn plan_patches(
    patches: &[Patch],
    (base, segments): (u64, &[(u64, u64)]),
    read: impl Fn(u64, usize) -> Option<Vec<u8>>,
) -> Result<Vec<(u64, Vec<u8>)>, String> {
    patches
        .iter()
        .enumerate()
        .map(|(at, patch)| {
            let start = base
                .checked_add(patch.offset)
                .ok_or("an offset past the address space")?;
            let end = start
                .checked_add(patch.bytes.len() as u64)
                .ok_or("an offset past the address space")?;
            let inside = segments
                .iter()
                .any(|&(first, length)| first <= start && end <= first.saturating_add(length));
            if !inside {
                return Err(format!(
                    "patch {} at {:#x} lies outside the module's segments",
                    at + 1,
                    patch.offset
                ));
            }
            let found = read(start, patch.original.len())
                .ok_or_else(|| format!("patch {} at {:#x} cannot be read", at + 1, patch.offset))?;
            if found != patch.original {
                return Err(format!(
                    "patch {} at {:#x} expects {} and finds {}: made for another build",
                    at + 1,
                    patch.offset,
                    hex(&patch.original),
                    hex(&found)
                ));
            }
            Ok((start, patch.bytes.clone()))
        })
        .collect()
}

/// When a cheat writes its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheatTiming {
    /// At every guest flip, so the value stays whatever the title does: a frozen value.
    #[default]
    Flip,
    /// Once, at the first flip.
    Once,
}

/// One cheat from a mod's `cheats.toml`, as written there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CheatEntry {
    /// Where the chain starts, from the executable's base.
    offset: u64,
    /// Each pointer followed from there: the eight bytes at the address so far are read as an
    /// address, and this added to it.
    #[serde(default)]
    pointers: Vec<i64>,
    /// The bytes written, as hexadecimal pairs.
    value: String,
    /// When.
    #[serde(default)]
    every: CheatTiming,
    /// What it does.
    #[serde(default)]
    description: Option<String>,
}

/// A `cheats.toml`: its cheats, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
struct CheatFile {
    #[serde(default, rename = "cheat")]
    cheats: Vec<CheatEntry>,
}

/// One cheat, read: where its chain starts, the pointers it follows, what it writes and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cheat {
    /// Where the chain starts, from the executable's base.
    pub offset: u64,
    /// Each pointer followed from there, as an offset added to the address read.
    pub pointers: Vec<i64>,
    /// The bytes written.
    pub value: Vec<u8>,
    /// When.
    pub every: CheatTiming,
    /// What it does.
    pub description: Option<String>,
}

/// The file a mod's cheats are in.
pub const CHEAT_FILE: &str = "cheats.toml";

impl Mod {
    /// Its cheats, from its `cheats.toml`; none where it has none.
    ///
    /// # Errors
    ///
    /// A `cheats.toml` that does not parse, or a cheat whose value is not hexadecimal bytes.
    pub fn cheats(&self) -> Result<Vec<Cheat>, String> {
        let Ok(text) = std::fs::read_to_string(self.dir.join(CHEAT_FILE)) else {
            return Ok(Vec::new());
        };
        let file: CheatFile = toml::from_str(&text).map_err(|e| e.to_string())?;
        file.cheats
            .into_iter()
            .map(|entry| {
                let value = hex_bytes(&entry.value)?;
                if value.is_empty() {
                    return Err(format!("the cheat at {:#x} writes nothing", entry.offset));
                }
                Ok(Cheat {
                    offset: entry.offset,
                    pointers: entry.pointers,
                    value,
                    every: entry.every,
                    description: entry.description,
                })
            })
            .collect()
    }
}

impl Cheat {
    /// The address it writes at in an executable placed at `base`: its offset from there, then each
    /// pointer followed - the eight bytes at the address so far read as an address, its offset
    /// added. `None` where a pointer cannot be read or is null, as a structure not yet made is.
    pub fn address(&self, base: u64, read_u64: impl Fn(u64) -> Option<u64>) -> Option<u64> {
        let mut at = base.checked_add(self.offset)?;
        for &step in &self.pointers {
            let pointed = read_u64(at).filter(|&pointer| pointer != 0)?;
            at = pointed.checked_add_signed(step)?;
        }
        Some(at)
    }
}

/// Bytes as spaced hexadecimal pairs.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
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
    use super::{Cheat, CheatTiming, Patch, found, hex_bytes, plan_patches};

    /// A cheat's address is its offset from the base, then each pointer read and offset; a null
    /// or unreadable pointer leaves it nowhere.
    #[test]
    fn a_cheat_follows_its_pointer_chain() {
        let cheat = |pointers: Vec<i64>| Cheat {
            offset: 0x100,
            pointers,
            value: vec![1],
            every: CheatTiming::Flip,
            description: None,
        };
        let memory = |at: u64| match at {
            0x1100 => Some(0x9000),
            0x9010 => Some(0x7000),
            0x2100 => Some(0),
            _ => None,
        };
        assert_eq!(cheat(vec![]).address(0x1000, memory), Some(0x1100));
        assert_eq!(cheat(vec![0x10]).address(0x1000, memory), Some(0x9010));
        assert_eq!(cheat(vec![0x10, -8]).address(0x1000, memory), Some(0x6ff8));
        assert_eq!(cheat(vec![0x10]).address(0x2000, memory), None, "null");
        assert_eq!(
            cheat(vec![0x10]).address(0x3000, memory),
            None,
            "unreadable"
        );
    }

    /// A mod's `cheats.toml` reads, frozen at every flip unless it says once.
    #[test]
    fn a_cheat_file_reads() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(super::CHEAT_FILE),
            concat!(
                "[[cheat]]\noffset = 0x10\npointers = [0x8]\nvalue = \"e8 03 00 00\"\n",
                "[[cheat]]\noffset = 0x20\nvalue = \"01\"\nevery = \"once\"\n",
            ),
        )
        .unwrap();
        let module = super::Mod {
            name: "m".to_owned(),
            dir: dir.path().to_path_buf(),
            file: super::ModFile::default(),
            unreadable: None,
        };
        let cheats = module.cheats().unwrap();
        assert_eq!(cheats.len(), 2);
        assert_eq!(cheats[0].value, [0xe8, 0x03, 0, 0]);
        assert_eq!(cheats[0].pointers, [8]);
        assert_eq!(cheats[0].every, CheatTiming::Flip);
        assert_eq!(cheats[1].every, CheatTiming::Once);
    }

    fn patch(offset: u64, original: &[u8], bytes: &[u8]) -> Patch {
        Patch {
            module: None,
            offset,
            original: original.to_vec(),
            bytes: bytes.to_vec(),
            description: None,
        }
    }

    /// Hexadecimal pairs read spaced or not; odd digits and non-digits are refused.
    #[test]
    fn hex_bytes_read_spaced_or_not() {
        assert_eq!(hex_bytes("74 0e").unwrap(), [0x74, 0x0e]);
        assert_eq!(hex_bytes("740E").unwrap(), [0x74, 0x0e]);
        assert!(hex_bytes("740").is_err());
        assert!(hex_bytes("zz").is_err());
    }

    /// Every patch inside a segment and finding its original is planned; one that is not stops all.
    #[test]
    fn patches_apply_all_or_none() {
        let memory = |at: u64, len: usize| -> Option<Vec<u8>> {
            Some(
                (0..len)
                    .map(|i| u8::try_from((at as usize + i) & 0xff).unwrap())
                    .collect(),
            )
        };
        let segments = [(0x1000, 0x100)];
        let ok = [
            patch(0x10, &[0x10, 0x11], &[0x90, 0x90]),
            patch(0x20, &[0x20], &[0xeb]),
        ];
        assert_eq!(
            plan_patches(&ok, (0x1000, &segments), memory).unwrap(),
            [(0x1010, vec![0x90, 0x90]), (0x1020, vec![0xeb])]
        );
        let wrong = [ok[0].clone(), patch(0x20, &[0x99], &[0xeb])];
        let why = plan_patches(&wrong, (0x1000, &segments), memory).unwrap_err();
        assert!(why.contains("expects 99 and finds 20"), "{why}");
        let outside = [patch(0xff, &[0xff, 0x00], &[0, 0])];
        assert!(
            plan_patches(&outside, (0x1000, &segments), memory)
                .unwrap_err()
                .contains("outside")
        );
    }

    /// A mod's `patches.toml` reads into patches, and one replacing a different length is refused.
    #[test]
    fn a_patch_file_reads_and_refuses_a_length_change() {
        let dir = tempfile::tempdir().expect("tempdir");
        let write = |text: &str| std::fs::write(dir.path().join(super::PATCH_FILE), text).unwrap();
        let module = super::Mod {
            name: "m".to_owned(),
            dir: dir.path().to_path_buf(),
            file: super::ModFile::default(),
            unreadable: None,
        };
        assert!(module.patches().unwrap().is_empty(), "none without a file");
        write("[[patch]]\noffset = 0x10\noriginal = \"74 0e\"\nbytes = \"eb 0e\"\n");
        let patches = module.patches().unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(
            (patches[0].offset, patches[0].bytes.clone()),
            (0x10, vec![0xeb, 0x0e])
        );
        write("[[patch]]\noffset = 0x10\noriginal = \"74\"\nbytes = \"eb 0e\"\n");
        assert!(module.patches().unwrap_err().contains("in place"));
    }

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
