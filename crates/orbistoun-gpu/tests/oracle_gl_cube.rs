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

use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
use orbistoun_gpu::walk;
use orbistoun_translate::{Fidelity, Strategy, Width};

/// Where the shader payload sat on the console; `oracle-payload-va` in the record.
const PAYLOAD_ADDRESS: u64 = 0x2_008f_0000;

/// The two records: which pixel shader each ran, and the pixel hash the console produced.
struct Record {
    stem: &'static str,
    fragment: u64,
    hash: u32,
    /// Whether that pixel shader translates today, and what it is waiting for if not.
    ///
    /// Pinned rather than printed. Record A's translates whole, as does the vertex program
    /// both records share, and a change that stopped either would otherwise show up as one
    /// fewer line of output nobody was watching.
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
        // Its sampling instruction needs an image subsystem: a descriptor model and a
        // SPIR-V image type, neither of which exists (worklog 549).
        fragment_translates: false,
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
