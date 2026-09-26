//! Shared by the tests that read captures. Not a test target: cargo treats a directory with a
//! `mod.rs` under `tests/` as a module for the tests that declare it.

use std::path::Path;

/// Reads a capture's words: little-endian dwords written as eight hex digits, separated by
/// whitespace, with `#` to the end of a line a comment.
///
/// Text rather than bytes: the provenance guard refuses a `.bin`, and a capture should read in a
/// diff. A word that is not eight hex digits stops the test naming the line rather than being
/// skipped.
///
/// # Panics
///
/// When the file cannot be read, or a word in it is not eight hex digits.
pub(crate) fn read_words(path: &Path) -> Vec<u8> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bytes = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let content = line.split('#').next().unwrap_or_default();
        for word in content.split_whitespace() {
            let value = if word.len() == 8 {
                u32::from_str_radix(word, 16).ok()
            } else {
                None
            };
            let value = value.unwrap_or_else(|| {
                panic!(
                    "{}:{}: {word:?} is not an eight-digit hex word",
                    path.display(),
                    index + 1
                )
            });
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}
