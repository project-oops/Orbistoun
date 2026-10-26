//! Compiled pipelines are kept where the host names (D994): a guest dispatch's pipeline is written
//! there as a checked entry, under a directory for the device and driver that made it, and D744's
//! single cache file beside it is dropped unread. Its own test binary, because the place is the
//! process's and must be named before the device opens.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_gpu_vulkan::pipeline_cache::keep_pipelines_at;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

/// A window the program reaches nothing of.
const WINDOW: u64 = 0x4_0187_0000;

#[test]
fn a_dispatch_s_pipeline_is_kept_where_the_host_names() {
    let base =
        std::env::temp_dir().join(format!("orbistoun-kept-pipelines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let monolithic = base.with_extension("bin");
    std::fs::write(&monolithic, [0xaa; 64]).expect("written");
    keep_pipelines_at(base.clone());
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            let _ = std::fs::remove_file(&monolithic);
            return;
        }
    }
    // `s_endpgm`, translated for one wave of thirty-two.
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode(&0xbf81_0000_u32.to_le_bytes(), &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave32,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, 64).expect("a window"),
        UserData {
            count: 4,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [32, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("translated");
    dispatch_guest(
        &module,
        &[0; 64],
        (&[0; 32], [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("the dispatch ran");

    assert!(!monolithic.exists(), "D744's file is dropped unread");
    let drivers: Vec<_> = std::fs::read_dir(&base)
        .expect("the place was made")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(
        drivers.len(),
        1,
        "one directory, this driver's: {drivers:?}"
    );
    let entries: Vec<Vec<u8>> = std::fs::read_dir(&drivers[0])
        .expect("listed")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "bin"))
        .map(|entry| std::fs::read(entry.path()).expect("read"))
        .collect();
    let _ = std::fs::remove_dir_all(&base);
    assert!(!entries.is_empty(), "the pipeline was kept");
    assert!(
        entries.iter().all(|entry| entry.starts_with(b"ORBPIPE\0")),
        "each in its envelope"
    );
}
