//! Harvesting FreeBSD symbol maps, without going through the command-line tool.
//!
//! Does exactly what `orbistoun-cli harvest` does. It exists because the tool links the
//! whole workspace, so a crate anybody is mid-edit on can stop the harvest working - and
//! the harvest has no business depending on the shader translator.
//!
//! ```text
//! cargo run -p orbistoun-names --example harvest -- <freebsd-src> <revision>
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use orbistoun_names::harvest::{
    FREEBSD_LIBRARY_PATHS, is_version_script, parse_symbol_map, render,
};

/// Collects every version script beneath a directory.
fn find_maps(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // An unreadable directory costs its own symbols, not the run.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_maps(&path, found);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_version_script)
        {
            found.push(path);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = args
        .next()
        .expect("usage: harvest <freebsd-src> <revision> [out]");
    let revision = args
        .next()
        .expect("a revision is required - a citation without one is not a citation");
    let out = args
        .next()
        .unwrap_or_else(|| "crates/orbistoun-names/data/standard.txt".to_owned());

    let root = Path::new(&source);
    let mut maps = Vec::new();
    for library in FREEBSD_LIBRARY_PATHS {
        let path = root.join(library);
        if path.is_dir() {
            find_maps(&path, &mut maps);
        } else {
            // Named rather than silently skipped: a sparse checkout missing one library
            // yields a smaller list, and the reader should know which.
            eprintln!("note: {} is not present, skipping", path.display());
        }
    }
    assert!(
        !maps.is_empty(),
        "no Symbol.map files under {source} - is this a FreeBSD source tree?"
    );
    maps.sort();

    let mut names = BTreeSet::new();
    for map in &maps {
        let text = std::fs::read_to_string(map).unwrap_or_default();
        for symbol in parse_symbol_map(&text) {
            names.insert(symbol.name);
        }
    }

    let names: Vec<String> = names.into_iter().collect();
    let text = render(&names, &revision, &orbistoun_nid::today());
    std::fs::write(&out, text).expect("write the word list");
    println!(
        "harvested {} names from {} symbol maps into {out}",
        names.len(),
        maps.len()
    );
}
