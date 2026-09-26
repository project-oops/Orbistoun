//! Writes the example modules to files, so a real validator can judge them.
//!
//! An example rather than a test: `tools/validate-spirv.sh` runs `spirv-val`, which lives
//! outside this toolchain, over what this writes.

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
    // The mesh oracle: its stage, execution modes and output-count instruction were read
    // from a compiled reference.
    write(
        "mesh.spv",
        orbistoun_spirv::triangle_mesh_module([[0.0, 1.0, 0.0, 1.0]; 3]),
    );
    // The sampling oracle, in both forms: a guest asks for level zero by name, and a fragment
    // stage may let the implementation choose.
    write(
        "sampling.spv",
        orbistoun_spirv::sampling_fragment_module(orbistoun_spirv::Lod::Implicit),
    );
    write(
        "sampling-lod0.spv",
        orbistoun_spirv::sampling_fragment_module(orbistoun_spirv::Lod::Zero),
    );
    // The storing oracle: a module may not write a format-less image without declaring the
    // capability.
    write(
        "storing.spv",
        orbistoun_spirv::storing_fragment_module([1, 0], [0.0, 1.0, 0.0, 1.0]),
    );
}
