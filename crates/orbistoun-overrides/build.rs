//! Carries each compatibility record's `[settings]` table into the binary as the repository layer.
//!
//! The records in `compat/` are the shipped layer of the override merge, but a run happens wherever
//! the binary is, not in the repository - so the part a run acts on is taken at build time. Only
//! `[settings]`: the rest of a record is a measurement written back by the tools, and embedding it
//! would rebuild everything that depends on this crate each time a run is recorded, for nothing a
//! run reads.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat");
    println!("cargo::rerun-if-changed={}", compat.display());
    let mut entries: Vec<(String, String)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(&compat) {
        for entry in dir.flatten() {
            let path = entry.path();
            println!("cargo::rerun-if-changed={}", path.display());
            if path.extension().is_none_or(|e| e != "toml") {
                continue;
            }
            let Some(title) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // A record that does not parse is the gate's to refuse, with a better message than a
            // build script can give; it contributes no settings here.
            let Ok(record) = text.parse::<toml::Table>() else {
                continue;
            };
            let Some(settings) = record.get("settings").and_then(toml::Value::as_table) else {
                continue;
            };
            if settings.is_empty() {
                continue;
            }
            let mut file = toml::Table::new();
            file.insert("settings".to_owned(), toml::Value::Table(settings.clone()));
            entries.push((title.to_owned(), file.to_string()));
        }
    }
    entries.sort();
    let mut out = String::from("/// Each shipped record's `[settings]`, by title, as TOML.\n");
    out.push_str("pub(crate) const SHIPPED: &[(&str, &str)] = &[\n");
    for (title, text) in &entries {
        let _ = writeln!(out, "    ({title:?}, {text:?}),");
    }
    out.push_str("];\n");
    let target =
        Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("shipped.rs");
    if std::fs::read_to_string(&target).ok().as_deref() != Some(out.as_str()) {
        std::fs::write(&target, out).expect("OUT_DIR is writable");
    }
}
