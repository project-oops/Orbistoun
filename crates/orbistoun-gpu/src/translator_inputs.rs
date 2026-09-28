//! What a translation is made from, hashed, so kept translations are refused by any build whose
//! translator could make them differently (D113).
//!
//! Compiled twice: into `build.rs`, which stamps the digest into the crate, and into this crate's
//! tests, which check the stamp against the tree and that an edit moves it. It uses only `std`, so
//! the build script needs no dependency.

use std::path::{Path, PathBuf};

/// The translator's inputs, relative to this crate's manifest: the decoder and its tables, the
/// translator, the SPIR-V builder, and the refill that drives them. A directory counts every file
/// under it.
pub(crate) const ROOTS: &[&str] = &[
    "../orbistoun-shader/src",
    "../orbistoun-shader/data",
    "../orbistoun-translate/src",
    "../orbistoun-spirv/src",
    "src/translations.rs",
];

/// A digest of every file under `roots`, resolved against `base`: each file's path relative to
/// `base` and its contents, in path order. Carriage returns are dropped, so a checkout's line
/// endings do not make another translator. A root that is missing contributes its name alone.
#[must_use]
pub(crate) fn digest(base: &Path, roots: &[&str]) -> u64 {
    let mut files = Vec::new();
    for root in roots {
        collect(&base.join(root), &mut files);
    }
    files.sort();
    let mut hash = Fnv::default();
    for root in roots {
        hash.bytes(root.as_bytes());
        hash.bytes(&[0]);
    }
    for file in &files {
        let relative = file.strip_prefix(base).unwrap_or(file);
        hash.bytes(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.bytes(&[0]);
        let contents = std::fs::read(file).unwrap_or_default();
        for &byte in contents.iter().filter(|&&b| b != b'\r') {
            hash.bytes(&[byte]);
        }
        hash.bytes(&[0]);
    }
    hash.0
}

/// Every file at or under `path`.
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if let Ok(dir) = std::fs::read_dir(path) {
        for entry in dir.flatten() {
            collect(&entry.path(), files);
        }
    }
}

/// 64-bit FNV-1a: stable across toolchains, which `DefaultHasher` does not promise.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}
