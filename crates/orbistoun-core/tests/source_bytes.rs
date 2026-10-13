//! No Rust source holds a control byte other than a tab, a newline or a carriage return.
//!
//! A raw NUL inside a byte-string literal compiles, and makes every searcher that reads the file -
//! grep, cargo-machete - treat it as binary and skip it. Machete then called a dependency unused
//! that one such file used, and CI's audit job failed on every commit until it was found (inbox
//! `REQ-20261013T1154Z-f5f2`). Write `\0` and its kin as escapes.

use std::path::Path;

/// The bytes below the printable range a source file may hold: tab, newline, carriage return (a
/// Windows checkout's line ends).
const ALLOWED: [u8; 3] = *b"\t\n\r";

fn sources(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Build output is not source.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_source_holds_a_raw_control_byte() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut found = Vec::new();
    sources(crates, &mut found);
    assert!(
        found.len() > 100,
        "the walk found the sources: {}",
        found.len()
    );

    let mut offending = Vec::new();
    for path in &found {
        let bytes = std::fs::read(path).expect("a source reads");
        for (line, text) in bytes.split(|&b| b == b'\n').enumerate() {
            if let Some(&byte) = text
                .iter()
                .find(|&&b| (b < 0x20 && !ALLOWED.contains(&b)) || b == 0x7f)
            {
                offending.push(format!(
                    "{}:{} holds byte {byte:#04x}",
                    path.display(),
                    line + 1
                ));
            }
        }
    }
    assert!(
        offending.is_empty(),
        "write these as escapes:\n{}",
        offending.join("\n")
    );
}
