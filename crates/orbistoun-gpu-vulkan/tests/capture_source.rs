//! A capture keeps what its module was translated from - the guest program and the translation
//! context - and translating that again reproduces the module it keeps, byte for byte. Needs no
//! device: the translator runs on the host.

use std::borrow::Cow;

use orbistoun_gpu::bound_ranges::BoundRanges;
use orbistoun_gpu::translation_source::Source;
use orbistoun_gpu::translations::Context;
use orbistoun_gpu_vulkan::capture::{Capture, MANIFEST, Reason};
use orbistoun_translate::wavefront::{ComputeInputs, MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Width};

/// radeonsi's compute buffer clear, as `guest_dispatch.rs` runs it: each thread stores the clear
/// value from user data words 4-7 through the descriptor in words 8-11.
const CLEAR: [u32; 11] = [
    0xd746_0000,
    0x0401_0c0c,
    0xd700_0004,
    0x0000_0880,
    0xd700_0006,
    0x0000_0c80,
    0x3400_0084,
    0xe078_1000,
    0x8002_0400,
    0xbf81_0000,
    0xbf9f_0000,
];

/// The clear as a dispatch of groups of 64 would translate it, over a window of 4096 words.
fn source() -> Source {
    let context = Context {
        strategy: Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::Wave64,
        },
        stage: Stage::Compute,
        primitive: MeshPrimitive::default(),
        window: Window::spanning_address(0x4_0187_0000, 4096).expect("a window"),
        user_data: UserData {
            count: 12,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 1,
                threads: [64, 1, 1],
                unwritten_user_data: 0,
                partial: None,
                integer_images: [false; 2],
                integer_fetched: 0,
                first_thread: 0,
                sampled_buffers: 0,
                scratch_wave_bytes: 0,
                device_addressed: false,
                float_mode: None,
            }),
            ..UserData::default()
        },
    };
    Source {
        address: 0x4000_03dc_9a00,
        program: CLEAR.iter().flat_map(|word| word.to_le_bytes()).collect(),
        context,
    }
}

/// The program and context a capture keeps read back as they were, and translating them again
/// gives the module the capture keeps, byte for byte; cut short, the program translates to
/// something else.
#[test]
fn re_translating_a_capture_reproduces_its_module() {
    let source = source();
    let module = source.translate(None).expect("the clear translates");
    let capture = Capture {
        program: Some(source.address),
        module: Cow::Owned(module),
        source: Some(source.clone()),
        ranges: BoundRanges::default(),
        ordinal: Some(1),
        ..Capture::of(
            (None, Reason::Asked),
            &[],
            &[],
            (&[], [1, 1, 1]),
            &orbistoun_gpu_vulkan::compute_images::DispatchImages::default(),
            &[],
        )
    };
    let dir = std::env::temp_dir().join(format!("orbistoun-capture-source-{}", std::process::id()));
    capture.write_to(&dir).expect("written");
    let read = Capture::read_from(&dir);
    let kept = std::fs::read(dir.join("module.spv"));
    let manifest = std::fs::read_to_string(dir.join(MANIFEST));
    let _ = std::fs::remove_dir_all(&dir);
    let read = read.expect("read back");
    assert_eq!(read.source.as_ref(), Some(&source), "program and context");
    assert!(
        manifest
            .expect("a manifest")
            .contains("source 0x400003dc9a00 program.bin translation.json")
    );

    let again = read
        .source
        .expect("a source")
        .translate(None)
        .expect("translates again");
    let again: Vec<u8> = again.iter().flat_map(|word| word.to_le_bytes()).collect();
    assert_eq!(again, kept.expect("module.spv"), "byte for byte");

    let cut = source.translate(Some(3)).ok();
    assert_ne!(
        cut.as_deref(),
        Some(capture.module.as_ref()),
        "the first three instructions alone"
    );
}
