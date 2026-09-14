//! End-to-end: a command stream's shaders reach the census corpus.
//!
//! The pieces each had tests, but the chain that matters - a submitted command stream, the
//! shader addresses hiding in its register writes, the shaders those addresses point at,
//! and the content-addressed corpus the census ranks - had none, because nothing connected
//! them (`registers` calls them "two tools that never meet"). `capture_shaders` is that
//! connection, and this drives it with a hand-built stream and a fake address space so the
//! whole path can be checked on a machine with no title and no GPU.
//!
//! The oracle is exact: a stream that names one shader address, a memory that holds a known
//! shader there, and a corpus that must end up holding exactly those bytes under their
//! content id - then the census, reading only the corpus, must see the one shader.

use orbistoun_gpu::pipeline::{GuestMemory, capture_shaders};
use orbistoun_shader::{
    CorpusCoverage, EncodingTable, OperandTable, ShaderCorpus, coverage::OpcodeKey, decode,
    shader_id,
};

/// The first word of an instruction, from its name - the same trick the translator's own
/// tests use, so a target-generation change moves the encoding rather than this test.
fn head(table: &EncodingTable, name: &str) -> u32 {
    let (family, opcode) = table
        .find_by_name(name)
        .unwrap_or_else(|| panic!("no instruction named {name}"));
    let encoding = table
        .encodings()
        .iter()
        .find(|e| e.name == family)
        .unwrap_or_else(|| panic!("no encoding family {family}"));
    encoding.value | (opcode << encoding.opcode.shift)
}

/// A little-endian dword stream, as it sits in a command buffer or a shader.
fn stream(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A type-3 command packet header: `SET_*_REG`-style, opcode plus a body length.
fn command(opcode: u8, body_dwords: u32) -> u32 {
    (3 << 30) | ((body_dwords - 1) << 16) | (u32::from(opcode) << 8)
}

/// A vocabulary naming one shader-address register pair, so a stream can point at a shader.
/// Self-contained rather than the builtin table: the mapping under test here is the
/// capture chain, not which retail register holds a fragment shader's address.
fn vocabulary() -> orbistoun_gpu::registers::Vocabulary {
    orbistoun_gpu::registers::Vocabulary::load(
        r#"
        [[opcode]]
        value = 0x76
        name = "SET_SH_REG"

        [[register_write]]
        opcode = 0x76
        name = "SET_SH_REG"
        base = 0x2C00

        [[shader_address]]
        register = 0x2C08
        stage = "fragment"
        half = "low"

        [[shader_address]]
        register = 0x2C09
        stage = "fragment"
        half = "high"
        "#,
    )
    .expect("vocabulary")
}

/// A stream that writes `address` into the fragment shader-address register pair.
fn stream_naming(address: u64) -> Vec<u8> {
    // The registers take the address in 256-byte units (see `shader_candidates`).
    let low = u32::try_from((address >> 8) & 0xFFFF_FFFF).unwrap();
    let high = u32::try_from(address >> 40).unwrap();
    // index 0x08 -> base 0x2C00 + 0x08 = 0x2C08 (low), then 0x2C09 (high): SPI_SHADER_PGM_LO/HI_PS.
    stream(&[command(0x76, 3), 0x08, low, high])
}

/// A guest address space holding one shader at one address, and nothing anywhere else.
struct OneShader {
    base: u64,
    bytes: Vec<u8>,
}

impl GuestMemory for OneShader {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        if address != self.base {
            return None;
        }
        Some(&self.bytes[..length.min(self.bytes.len())])
    }
}

/// A minimal but real shader: one vector move, then the instruction that ends a program.
/// It has an extent (it terminates) and something for the census to classify.
fn known_shader(table: &EncodingTable) -> Vec<u8> {
    // vdst is v0, whose field is zero, so only src0 = v1 (256 + 1) needs setting.
    let v_mov = head(table, "v_mov_b32_e32") | (256 + 1);
    stream(&[v_mov, head(table, "s_endpgm")])
}

#[test]
fn a_command_stream_shader_is_captured_and_the_census_can_read_it() {
    const ADDRESS: u64 = 0x2000;

    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let vocab = vocabulary();

    let shader = known_shader(&encodings);
    let memory = OneShader {
        base: ADDRESS,
        bytes: shader.clone(),
    };
    let stream = stream_naming(ADDRESS);

    let dir = tempfile::tempdir().expect("tempdir");
    let mut corpus = ShaderCorpus::open(dir.path()).expect("corpus");

    let report = capture_shaders(&stream, &memory, &vocab, &encodings, &operands, &mut corpus)
        .expect("capture");

    // One address named, one shader read, nothing missed.
    assert!(
        report.missed.is_empty(),
        "unexpected misses: {:?}",
        report.missed
    );
    assert_eq!(report.captured.len(), 1, "one shader address was named");
    let captured = &report.captured[0];
    assert_eq!(captured.stage, "fragment");
    assert!(
        captured.fresh,
        "a shader the corpus has never seen is fresh"
    );
    assert_eq!(
        captured.length,
        shader.len(),
        "the whole shader, up to its terminator"
    );
    assert_eq!(
        captured.id,
        shader_id(&shader),
        "stored under its content id"
    );

    // The bytes in the corpus are exactly the shader, byte for byte.
    assert_eq!(corpus.load(&captured.id).expect("load"), shader);

    // The census, reading only the corpus, sees the one shader and its instructions.
    let reopened = ShaderCorpus::open(dir.path()).expect("reopen");
    let mut coverage = CorpusCoverage::new();
    for id in reopened.ids() {
        let bytes = reopened.load(id).expect("load id");
        // The claim here is that the census can read what was captured, so every opcode is
        // treated as supported: completeness is a separate question this test does not make.
        coverage.observe(
            id,
            &decode(&bytes, &encodings, &operands),
            &|_: OpcodeKey| true,
        );
    }
    assert_eq!(
        coverage.shaders().len(),
        1,
        "the census sees the captured shader"
    );
}

#[test]
fn the_same_shader_captured_twice_is_stored_once() {
    const ADDRESS: u64 = 0x4000;

    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let vocab = vocabulary();

    let shader = known_shader(&encodings);
    let memory = OneShader {
        base: ADDRESS,
        bytes: shader,
    };
    let stream = stream_naming(ADDRESS);

    let dir = tempfile::tempdir().expect("tempdir");
    let mut corpus = ShaderCorpus::open(dir.path()).expect("corpus");

    let first = capture_shaders(&stream, &memory, &vocab, &encodings, &operands, &mut corpus)
        .expect("first");
    assert!(first.captured[0].fresh, "new the first time");

    let second = capture_shaders(&stream, &memory, &vocab, &encodings, &operands, &mut corpus)
        .expect("second");
    assert!(
        !second.captured[0].fresh,
        "a guest rebinds the same shader constantly; the corpus keeps one copy"
    );
    assert_eq!(corpus.len(), 1, "content addressing dedupes the rebind");
}

#[test]
fn an_address_that_names_no_shader_is_a_reported_miss_not_a_capture() {
    // The register mapping is a hypothesis. When it produces an address memory cannot
    // honour, that is evidence about the mapping and must be reported, not swallowed - and
    // it must never become a captured shader read out of whatever happened to be there.
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let vocab = vocabulary();

    let shader = known_shader(&encodings);
    let memory = OneShader {
        base: 0x8000,
        bytes: shader,
    };
    // The stream points somewhere the memory does not hold anything.
    let stream = stream_naming(0x9000);

    let dir = tempfile::tempdir().expect("tempdir");
    let mut corpus = ShaderCorpus::open(dir.path()).expect("corpus");

    let report = capture_shaders(&stream, &memory, &vocab, &encodings, &operands, &mut corpus)
        .expect("capture");

    assert!(report.captured.is_empty(), "nothing was read");
    assert_eq!(report.missed.len(), 1, "the unresolved address is reported");
    assert_eq!(report.missed[0].address, 0x9000);
    assert_eq!(corpus.len(), 0, "a miss stores nothing");
}
