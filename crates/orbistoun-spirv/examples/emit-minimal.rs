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
    // The mesh oracle, which is the one module here a validator has something new to say
    // about: its stage, its execution modes and the instruction that declares its output
    // counts were all read out of a compiled reference rather than known.
    write(
        "mesh.spv",
        orbistoun_spirv::triangle_mesh_module([[0.0, 1.0, 0.0, 1.0]; 3]),
    );
    // The sampling oracle, whose image type, sampled-image type and sampling instructions are
    // all new here and none of which this crate can judge for itself. Both forms, because they
    // are different instructions: a guest asks for level zero by name and a fragment stage may
    // let the implementation choose.
    write(
        "sampling.spv",
        orbistoun_spirv::sampling_fragment_module(orbistoun_spirv::Lod::Implicit),
    );
    write(
        "sampling-lod0.spv",
        orbistoun_spirv::sampling_fragment_module(orbistoun_spirv::Lod::Zero),
    );
    // The storing oracle, whose storage image type, write instruction and format-less
    // capability are all new here - and whose capability is the one a validator has most to say
    // about, because a module may not write a format-less image without declaring it.
    write(
        "storing.spv",
        orbistoun_spirv::storing_fragment_module([1, 0], [0.0, 1.0, 0.0, 1.0]),
    );
}
