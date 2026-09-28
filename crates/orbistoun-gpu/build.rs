//! Stamps a digest of the shader translator's sources and tables into the crate, so kept
//! translations follow the translator rather than the commit: every uncommitted build names the
//! same commit, and an edit to the translator must still retranslate what a title kept (D113).

#[path = "src/translator_inputs.rs"]
mod translator_inputs;

use std::path::Path;

fn main() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    for root in translator_inputs::ROOTS {
        println!("cargo::rerun-if-changed={}", base.join(root).display());
    }
    println!(
        "cargo::rustc-env=ORBISTOUN_TRANSLATOR_INPUTS={:016x}",
        translator_inputs::digest(base, translator_inputs::ROOTS)
    );
}
