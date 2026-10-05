//! A guest compute dispatch through buffers of its own (D746), run on the device.
//!
//! The program is radeonsi's buffer copy at its least: each thread loads the word at its own index
//! through the descriptor in user-data words 8-11 and stores it through the one in words 12-15.
//!
//! ```text
//! v_lshlrev_b32_e32 v0, 2, v0
//! buffer_load_dword v1, v0, s[8:11], 0 offen
//! s_waitcnt vmcnt(0)
//! buffer_store_dword v1, v0, s[12:15], 0 offen
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest_with_buffers;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

const COPY: [u32; 8] = [
    0x3400_0082,
    0xe030_1000,
    0x8002_0100,
    0xbf8c_3f70,
    0xe070_1000,
    0x8003_0100,
    0xbf81_0000,
    0xbf9f_0000,
];

/// `SQ_BUF_RSRC_WORD3` as radeonsi's descriptors carry it: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
/// The two buffers' bases, 128 MiB apart as STKT00001's are: no window spans both.
const SOURCE: u64 = 0x4_0a60_0000;
const DESTINATION: u64 = 0x4_1260_0000;

fn user_data() -> Vec<u32> {
    let mut block = vec![0_u32; 32];
    for (at, base) in [(8, SOURCE), (12, DESTINATION)] {
        block[at] = base as u32;
        block[at + 1] = (base >> 32) as u32 & 0xffff;
        block[at + 2] = 64 * 4;
        block[at + 3] = WORD3;
    }
    block
}

/// Each of 64 threads copies its word from one bound buffer to the other, and the module needs no
/// window: both accesses are traced to a slot, the store included.
#[test]
fn a_dispatch_copies_between_its_own_buffers() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = COPY.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            first_register: 0,
            count: 16,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [false; 3],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .expect("the copy translates through its buffers");
    let source: Vec<u32> = (0..64).map(|i| 0x5000_0000 | i).collect();
    let destination = vec![0xdead_beef_u32; 64];
    let done = dispatch_guest_with_buffers(
        &module,
        &[0],
        (&user_data(), [1, 1, 1]),
        &DispatchImages::default(),
        &[source.clone(), destination],
    )
    .expect("dispatched");
    assert!(!done.escaped, "nothing left the bound buffers");
    assert_eq!(done.buffers.len(), 2);
    assert_eq!(done.buffers[0], source, "the source is unchanged");
    assert_eq!(done.buffers[1], source, "the destination holds the copy");
}
