//! The first hardware-produced command stream through the whole submission pipeline.
//!
//! # What this checks that the vocabulary test does not
//!
//! `tests/vocabulary.rs` reads the GL cube captures *through the register table* and stops
//! at the shader addresses. This goes on to what a real submission does next: hand those
//! addresses to guest memory, read the shaders the console actually ran, translate them,
//! and report what happened. The memory image is the shader payload as it sat at
//! `0x2008f0000` on the console, dumped in the same record as the stream
//! (oops-sdk `docs/hardware/agc-gl-cube-oracle-fw1240.md`, orbistoun worklog 539).
//!
//! # What is asserted and what is only reported
//!
//! The facts the record establishes are asserted: the walk is clean, both shader addresses
//! resolve into the payload, the two routes never disagree, and nothing names a stage the
//! draw queue cannot run.
//!
//! **And now what the translator makes of them, which began as something only printed.** When
//! this test was written nothing translated and the output was the measurement; it has since
//! moved four times, and every move was found by reading a line rather than by a test failing.
//! Record A's pixel shader translates whole - the first shader a console ran that this project
//! has turned into a module - and that is pinned here, as is the vertex program's refusal and
//! record B's. The reasons stay printed, because a reason is prose and pinning it would make
//! this test fail every time somebody improved a sentence.

mod common;

use std::path::PathBuf;

use orbistoun_gpu::RenderCommand;
use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
use orbistoun_gpu::walk;
use orbistoun_shader::{EncodingTable, Operand, OperandTable, decode_program};
use orbistoun_translate::{Fidelity, Strategy, Width};

/// Where the shader payload sat on the console; `oracle-payload-va` in the record.
const PAYLOAD_ADDRESS: u64 = 0x2_008f_0000;

/// Where the vertex buffer sat on the console; `oracle-vbo-va` in the record.
const VERTEX_BUFFER_ADDRESS: u64 = 0x2_0090_0000;

/// Where the fence sat; `oracle-fence-va` in the record.
const FENCE_ADDRESS: u64 = 0x2_0091_0000;

/// The colour target; `oracle-color-va` in the record.
const COLOUR_TARGET_ADDRESS: u64 = 0x40_0140_0000;

/// The two records: which pixel shader each ran, and the pixel hash the console produced.
struct Record {
    stem: &'static str,
    fragment: u64,
    hash: u32,
    /// Whether that pixel shader translates today, and what it is waiting for if not.
    ///
    /// Pinned rather than printed. Both translate now, as does the vertex program the two
    /// records share, and a change that stopped any of them would otherwise show up as one
    /// fewer line of output nobody was watching.
    ///
    /// **It has been `false` and the field is kept anyway.** Record B's was the last refusal
    /// in either stream, and the next shader this project meets may well be a refusal again -
    /// a field that only ever holds one value is one nobody notices going wrong.
    fragment_translates: bool,
}

const RECORDS: [Record; 2] = [
    Record {
        stem: "agc-gl-cube-fw1240-a",
        fragment: PAYLOAD_ADDRESS + 0x300,
        hash: 0x9dbf_e189,
        fragment_translates: true,
    },
    Record {
        stem: "agc-gl-cube-fw1240-b",
        fragment: PAYLOAD_ADDRESS + 0x200,
        hash: 0xc51c_ec32,
        // Its sampling instruction needed an image subsystem. The host half was built first
        // (worklog 566) and the descriptor half was settled by deciding it did not need
        // building: every sample reads the one bound texture and a module naming a second is
        // refused (D690). **This shader is the reason both of those exist.**
        fragment_translates: true,
    },
];

/// The vertex program the NGG stage ran, in both records.
const VERTEX: u64 = PAYLOAD_ADDRESS;

/// Guest memory holding only the shader payload, at its console address.
///
/// Everything else the stream names - the vertex buffer, the targets, the fence - is
/// deliberately unmapped: the pipeline reads shaders and nothing else today, and an
/// unmapped read is reported, not zero-filled.
struct Payload {
    bytes: Vec<u8>,
}

impl GuestMemory for Payload {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(PAYLOAD_ADDRESS)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("captures")
}

fn load(record: &Record) -> (Vec<u8>, Payload) {
    let dir = captures_dir();
    let stream = common::read_words(&dir.join(format!("{}.hex", record.stem)));
    let bytes = common::read_words(&dir.join(format!("{}.payload.hex", record.stem)));
    (stream, Payload { bytes })
}

/// The strategy a submission gets: the cheapest fidelity valid for each shader.
///
/// Not `Lane`: both pixel shaders the console ran save and restore the execution mask
/// around a single-lane store, and the per-lane model refuses that by design (it has no
/// mask to save). A real submission is not asked to pick a level, so neither is this.
fn pipeline() -> Pipeline {
    Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    })
    .expect("pipeline")
}

/// Prints everything one record's submission did, and writes any module it produced.
///
/// The printing is the point of this test as much as the assertions are: a reason is prose
/// and pinning prose makes a test fail whenever somebody improves a sentence, so the reasons
/// are reported and the outcomes are asserted.
fn report_of(record: &Record, submission: &orbistoun_gpu::pipeline::Submission) {
    let report = &submission.report;
    println!(
        concat!(
            "{}: hash {:#010x}: packets {}, register writes {}, shaders found {}, ",
            "resolved {}, unresolved {}, translated {}, cache hits {}, disagreed {}, ",
            "impossible {:?}"
        ),
        record.stem,
        record.hash,
        report.packets,
        report.register_writes,
        report.shaders_found,
        report.addresses_resolved,
        report.addresses_unresolved,
        report.shaders_translated,
        report.cache_hits,
        report.disagreed.len(),
        report.impossible_stages
    );
    for failure in &report.failures {
        println!(
            "  failed {} at {:#x}: {}",
            failure.stage, failure.address, failure.reason
        );
    }
    for warning in &report.warnings {
        println!("  warning: {warning}");
    }
    for (resource, module) in &submission.modules {
        println!("  module {resource:?}: {} SPIR-V words", module.len());
        emit(record.stem, module);
    }
}

/// Writes a translated module where a real validator can read it.
///
/// `tools/validate-spirv.sh` runs `spirv-val` over `target/spirv`, which is the established
/// way this project asks something other than itself whether its output is valid (the
/// emitter's own tests check structure, and a crate asserting it likes its own bytes proves
/// nothing). Until now the only modules there came from a hand-written example; this puts a
/// module translated from a shader a console ran beside them.
///
/// Failing to write is ignored on purpose: this test is about the translation, and a
/// read-only target directory is not a reason to fail it.
fn emit(stem: &str, module: &[u32]) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("spirv");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let bytes: Vec<u8> = module.iter().flat_map(|word| word.to_le_bytes()).collect();
    let _ = std::fs::write(dir.join(format!("oracle-{stem}.spv")), bytes);
}

/// Walks one record's stream and reports what the walk found.
///
/// Split from the checks below only for length; the two halves ask different questions -
/// *did the bytes parse* and *what did the pipeline make of them* - so the seam is not
/// arbitrary.
fn walked_cleanly(record: &Record, stream: &[u8]) {
    let walked = walk(stream);
    assert!(
        walked.is_trustworthy(),
        "{}: desynchronised={} overran={} trailing={}",
        record.stem,
        walked.desynchronised,
        walked.overran,
        walked.trailing_bytes
    );
    let consumed: u32 = walked.packets.iter().map(|p| p.length).sum();
    assert_eq!(
        consumed as usize,
        stream.len(),
        "{}: the walk consumes the whole stream",
        record.stem
    );
}

/// **The draws the stream asked for**, which nothing extracted until worklog 585.
///
/// Twelve of three vertices apiece, one instance each: a cube's twelve triangles, issued one at
/// a time. That count is worth pinning for a reason beyond regression - **three is what the
/// guest's own primitive shader declares it will emit**, and the two numbers were arrived at
/// from opposite ends. The stream says it through a packet body read by opcode; the shader says
/// it through `MSG_GS_ALLOC_REQ`, translated into a mesh module's output declaration. They
/// agree, and nothing made them.
fn drew_twelve_triangles(record: &Record, submission: &orbistoun_gpu::pipeline::Submission) {
    let draws: Vec<_> = submission
        .commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::Draw {
                vertices,
                instances,
                ..
            } => Some((*vertices, *instances)),
            _ => None,
        })
        .collect();
    assert_eq!(
        draws,
        vec![(3, 1); 12],
        "{}: the stream's draws are not twelve triangles",
        record.stem
    );
    assert_eq!(
        submission.report.draws,
        draws.len(),
        "{}: the report's count and the commands disagree",
        record.stem
    );
}

/// **Both hardware streams walk cleanly and find both shaders where the console found them.**
#[test]
fn the_gl_cube_streams_resolve_their_shaders_from_the_payload() {
    for record in &RECORDS {
        let (stream, memory) = load(record);
        walked_cleanly(record, &stream);

        let mut pipeline = pipeline();
        let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory);
        let report = &submission.report;

        report_of(record, &submission);

        assert_eq!(
            report.shaders_found, 2,
            "{}: a vertex program and a pixel shader",
            record.stem
        );
        assert!(
            report.disagreed.is_empty(),
            "{}: {:?}",
            record.stem,
            report.disagreed
        );
        assert!(
            report.impossible_stages.is_empty(),
            "{}: {:?}",
            record.stem,
            report.impossible_stages
        );
        assert_eq!(
            report.addresses_unresolved, 0,
            "{}: every shader address the registers named lies inside the payload image",
            record.stem
        );

        drew_twelve_triangles(record, &submission);

        // **The pixel shader translates, or it does not, and which is not left to the eye.**
        let fragment_failed = report
            .failures
            .iter()
            .any(|failure| failure.address == record.fragment);
        assert_eq!(
            !fragment_failed,
            record.fragment_translates,
            "{}: the pixel shader at {:#x} - expected it to translate: {}. Failures: {:?}",
            record.stem,
            record.fragment,
            record.fragment_translates,
            report
                .failures
                .iter()
                .map(|f| format!("{} at {:#x}", f.stage, f.address))
                .collect::<Vec<_>>()
        );
        // **The vertex program translates too**, into a mesh module - the stage a primitive
        // shader corresponds to (D688), which is now built. The last time this test was
        // updated it asserted the opposite and named the condition under which it would want
        // changing; this is that change (worklog 558).
        let vertex_failed = report
            .failures
            .iter()
            .any(|failure| failure.address == VERTEX);
        assert!(
            !vertex_failed,
            "{}: the vertex program did not translate: {:?}",
            record.stem,
            report
                .failures
                .iter()
                .map(|failure| failure.reason.clone())
                .collect::<Vec<_>>()
        );

        let expected_modules = usize::from(record.fragment_translates) + 1;
        assert_eq!(
            submission.modules.len(),
            expected_modules,
            "{}: a module for each shader that translated",
            record.stem
        );
        assert!(
            submission.modules.values().all(|module| module.len() > 100),
            "{}: a module that short is an empty one",
            record.stem
        );

        // Whatever the translator made of them, the only shaders it was ever asked about
        // are the two the console ran.
        for failure in &report.failures {
            assert!(
                [VERTEX, record.fragment].contains(&failure.address),
                "{}: a failure at {:#x} names a shader the stream never bound",
                record.stem,
                failure.address
            );
        }
    }
}

/// Every scalar register pair the program loads as two literals, with the 64-bit value they make
/// and the offset of the low half's load.
///
/// A pair is a `s_mov_b32` of a literal into `sN` and another into `sN+1`, which is how a shader
/// forms a 64-bit address in two scalar registers when it has nowhere else to get one from.
fn literal_address_pairs(program: &[u8]) -> Vec<(u64, u16, u32)> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(program, &encodings, &operands);
    let loads: Vec<(u16, u32, u32)> = decoded
        .instructions
        .iter()
        .filter(|instruction| {
            instruction.encoding.is_some_and(|index| {
                let family = &encodings.encodings()[usize::from(index)].name;
                encodings.mnemonic_for(family, instruction.opcode) == Some("s_mov_b32")
            })
        })
        .filter_map(|instruction| match instruction.operands.as_slice() {
            [Operand::Scalar(register), Operand::Literal(value)] => {
                Some((*register, *value, instruction.offset))
            }
            _ => None,
        })
        .collect();
    loads
        .iter()
        .filter_map(|&(register, low, at)| {
            loads
                .iter()
                .find(|&&(high_register, _, _)| high_register == register + 1)
                .map(|&(_, high, _)| ((u64::from(high) << 32) | u64::from(low), register, at))
        })
        .collect()
}

/// **No register write carries the vertex buffer's address. The vertex program loads it itself.**
///
/// `REQ-20260914T2348Z-4e71` asked the console which register writes in a submitted stream carry
/// a draw's vertex-buffer address, so the submission pipeline could anchor a shader's memory window
/// there instead of at zero. The request came back without an answer this project can use - the
/// values it quoted are not in the sweep log it cites - but the question never needed hardware:
/// these two captures are a draw the console really made from a real vertex buffer, at an address
/// the record states, and the stream and the payload are both here.
///
/// Searching them says the question was aimed at the wrong place. The vertex program forms the
/// address itself, `s_mov_b32 s2, 0x00900000` then `s_mov_b32 s3, 0x2` at +0x4c, and uses the pair
/// as a base. The stream names the buffer nowhere, in either encoding it uses for addresses.
///
/// # Why the absence is worth trusting
///
/// A search that finds nothing proves nothing unless it can find something, so the same search is
/// required to find the three addresses the stream *does* carry: the shader payload and the colour
/// target in 256-byte units (register `SPI_SHADER_PGM_LO_*` and `CB_COLOR0_BASE`), and the fence as
/// a whole 64-bit address. Remove the positive checks and this test can pass on an empty file.
///
/// # What this does not say
///
/// That a *retail* title does the same. oops-gl bakes its addresses into its own shaders; a title
/// built with the vendor toolchain presumably reaches its buffers through register or table state,
/// and nothing here has measured that. So this settles where to look for these two captures and is
/// no licence to derive a window from shader literals in general.
#[test]
fn the_gl_cube_vertex_buffer_address_is_a_shader_literal_and_no_register_carries_it() {
    for record in &RECORDS {
        let (stream, memory) = load(record);

        // The vertex program is the payload's first 0x200 bytes (the record's own layout).
        let pairs = literal_address_pairs(&memory.bytes[..0x200]);
        assert!(
            pairs.contains(&(VERTEX_BUFFER_ADDRESS, 2, 0x4c)),
            concat!(
                "{}: the vertex program does not load {:#x} into s2:s3 at +0x4c; ",
                "literal pairs found: {:#x?}"
            ),
            record.stem,
            VERTEX_BUFFER_ADDRESS,
            pairs
        );

        let words: Vec<u32> = stream
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four bytes")))
            .collect();
        let in_units = |address: u64| u32::try_from(address >> 8).expect("fits a register");
        let low = |address: u64| u32::try_from(address & 0xffff_ffff).expect("low half");
        let high = |address: u64| u32::try_from(address >> 32).expect("high half");
        let as_pair = |address: u64| {
            words
                .windows(2)
                .any(|pair| pair == [low(address), high(address)])
        };

        // The positive controls: the search finds what the stream does carry.
        assert!(
            words.contains(&in_units(PAYLOAD_ADDRESS)),
            "{}: the shader payload is not named in 256-byte units - the search is broken",
            record.stem
        );
        assert!(
            words.contains(&in_units(COLOUR_TARGET_ADDRESS)),
            "{}: the colour target is not named in 256-byte units - the search is broken",
            record.stem
        );
        assert!(
            as_pair(FENCE_ADDRESS),
            "{}: the fence is not named as a whole address - the search is broken",
            record.stem
        );

        // And the finding.
        assert!(
            !words.contains(&in_units(VERTEX_BUFFER_ADDRESS)),
            "{}: the stream names the vertex buffer in 256-byte units after all",
            record.stem
        );
        assert!(
            !words.contains(&low(VERTEX_BUFFER_ADDRESS)),
            "{}: the stream carries the vertex buffer's low half after all",
            record.stem
        );
    }
}
