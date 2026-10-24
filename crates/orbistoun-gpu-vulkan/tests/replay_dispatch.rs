//! A captured dispatch, read back from its directory and run again alone: every run answers, none
//! loses the device, and the same inputs leave the same output each time.

use std::borrow::Cow;

use orbistoun_gpu::bound_ranges::BoundRanges;
use orbistoun_gpu_vulkan::capture::{Capture, CapturedBuffer, Reason};
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::replay_dispatch;

/// A dispatch replayed from its capture runs each time it is asked, and leaves the same output.
#[test]
fn a_captured_dispatch_replays_alone_with_the_same_output() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let dir =
        std::env::temp_dir().join(format!("orbistoun-replay-dispatch-{}", std::process::id()));
    let capture = Capture {
        program: Some(0x3dc_3400),
        reason: Reason::Asked,
        module: Cow::Owned(orbistoun_spirv::storage_buffer_write_module(0xABCD_1234, 4)),
        memory: Cow::Owned((0..256).collect()),
        push: Cow::Owned(Vec::new()),
        groups: [1, 1, 1],
        images: Vec::new(),
        buffers: vec![
            CapturedBuffer::Words(Cow::Owned(vec![7; 64])),
            CapturedBuffer::Unread(64),
        ],
        ranges: BoundRanges::default(),
        ordinal: None,
    };
    capture.write_to(&dir).expect("the capture was written");
    let out = dir.join("out");

    let replay = replay_dispatch(&dir, 3, Some(&out));
    let read = |name: &str| std::fs::read(out.join(name)).map(|bytes| bytes.len());
    let (memory, read_buffer, unread_buffer) = (
        read("memory.bin"),
        read("buffer-0.bin"),
        read("buffer-1.bin"),
    );
    let _ = std::fs::remove_dir_all(&dir);
    let replay = replay.expect("the capture was read");
    println!("{replay}");
    // Each output is whole: the window and both buffers at the lengths they were handed over.
    assert_eq!(memory.ok(), Some(256 * 4));
    assert_eq!(read_buffer.ok(), Some(64 * 4));
    assert_eq!(unread_buffer.ok(), Some(64 * 4));

    assert!(!replay.lost, "{replay}");
    assert_eq!(replay.runs.len(), 3, "{replay}");
    let hashes: Vec<u64> = replay
        .runs
        .iter()
        .map(|run| run.outcome.as_ref().expect("the run answered").hash)
        .collect();
    assert!(hashes.iter().all(|hash| *hash == hashes[0]), "{replay}");
}
