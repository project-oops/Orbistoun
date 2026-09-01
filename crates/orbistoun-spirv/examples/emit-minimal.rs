//! Writes the minimal module to a file, so a real validator can judge it.
//!
//! An example rather than a test: the validator lives outside this toolchain, so the
//! Rust side produces the artefact and `tools/validate-spirv.sh` runs `spirv-val` over
//! it. A crate cannot validate its own output by asserting that it likes it.

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "minimal.spv".to_owned());
    let write = |name: &str, words: Vec<u32>| {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let at = std::path::Path::new(&path).with_file_name(name);
        std::fs::write(&at, bytes).expect("write module");
        println!("wrote {}", at.display());
    };
    write(
        "minimal.spv",
        orbistoun_spirv::minimal_compute_module([64, 1, 1]),
    );
    write(
        "storage-write.spv",
        orbistoun_spirv::storage_buffer_write_module(0xABCD_1234, 4),
    );
}
