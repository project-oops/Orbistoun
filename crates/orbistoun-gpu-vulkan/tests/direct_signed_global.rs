//! A device-addressed dispatch's global access below its address (D964): the `OFFSET` field of a
//! global or scratch access is a signed twelve-bit number, so `offset:-8` reaches the word eight
//! bytes below the address its registers hold, not 4088 bytes above it. A missing device, or one
//! without direct addressing, skips with a line `bin/orbistoun check` surfaces.

use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::direct;
use orbistoun_gpu_vulkan::{Availability, dispatch_guest, probe};
use orbistoun_mem::Protection;
use orbistoun_mem::test_bases::{Range, crates};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, USER_DATA_BLOCK_WORDS, UserData, Window,
    translate_with_user_data,
};

static BASES: Range = Range::nth(crates::GPU_VULKAN);

/// The bytes the test reserves.
const RESERVED: u64 = 1 << 20;

/// A window no access of the program reaches: a device-addressed dispatch reads guest memory by
/// device address, not through it.
const WINDOW: u64 = 0x4_0187_0000;

/// `SADDR` naming no scalar base, `off`, and the global segment, bits 15:14.
const OFF: u32 = 0x7d;
const GLOBAL: u32 = 2;

/// Whether the device addresses guest memory directly; prints a skip otherwise.
fn direct_or_skip(test: &str) -> bool {
    let reason = match probe() {
        Availability::Available { .. } if direct::alignment().is_some() => return true,
        Availability::Available { .. } => {
            "the device lacks VK_EXT_external_memory_host or bufferDeviceAddress".to_owned()
        }
        Availability::Unavailable { reason } => reason,
    };
    println!();
    println!("!! SKIPPED: {test}");
    println!("!! {reason}");
    println!("!! this test did NOT run");
    println!();
    false
}

fn read(at: u64) -> u32 {
    // SAFETY: every address read lies in the reservation the test holds, committed read-write.
    unsafe { orbistoun_mem::guest::read_u32(at) }.expect("a guest address")
}

fn write(at: u64, value: u32) {
    // SAFETY: as for `read`.
    assert!(unsafe { orbistoun_mem::guest::write_u32(at, value) });
}

/// The instruction head of `name`: its family's fixed bits and its opcode.
fn head(name: &str) -> u32 {
    let table = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = table
        .find_by_name(name)
        .unwrap_or_else(|| panic!("this target has no instruction named {name}"));
    let encoding = table
        .encodings()
        .iter()
        .find(|encoding| encoding.name == family)
        .unwrap_or_else(|| panic!("no encoding family named {family}"));
    encoding.value | (opcode << encoding.opcode.shift)
}

/// A global access of one word between `data` and the pair at `address`, its offset field
/// `field` (twelve bits as encoded).
fn global(name: &str, address: u32, data: u32, field: u32) -> [u32; 2] {
    let head = head(name) & !(3 << 14) | GLOBAL << 14 | field;
    if name.contains("store") {
        [head, address | (data << 8) | (OFF << 16)]
    } else {
        [head, address | (OFF << 16) | (data << 24)]
    }
}

/// ```text
/// v_mov_b32 v0, s0 ; v_mov_b32 v1, s1          the guest address A
/// global_load_dword v2, v[0:1], off offset:-4  the word below A
/// global_store_dword v[0:1], v2, off offset:-8 into the word below that
/// s_endpgm
/// ```
fn program() -> Vec<u32> {
    let mut words = vec![head("v_mov_b32_e32"), head("v_mov_b32_e32") | (1 << 17) | 1];
    words.extend(global("global_load_dword", 0, 2, 0xffc));
    words.extend(global("global_store_dword", 0, 2, 0xff8));
    words.push(head("s_endpgm"));
    words
}

/// `program` translated as a device-addressed dispatch of one group of sixty-four threads, with two
/// user-data words.
fn device_addressed(program: &[u32]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the program decodes cleanly");
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, 64).expect("a window"),
        UserData {
            count: 2,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                device_addressed: true,
                ..ComputeInputs::default()
            }),
            local_words: Some(64),
            draw_buffers: true,
            ..UserData::default()
        },
    )
    .map(|(module, _, _)| module)
    .expect("the program translates device-addressed")
}

/// A global load at `offset:-4` reads the word four bytes below the address, and a global store at
/// `offset:-8` writes the word eight below it; the word 4088 bytes above, where the field read
/// unsigned would lead, is left as it was.
#[test]
fn a_global_access_below_its_address_reaches_below_it() {
    if !direct_or_skip("a_global_access_below_its_address_reaches_below_it") {
        return;
    }
    let base = BASES.take();
    let mut space = orbistoun_mem::AddressSpace::new();
    space
        .reserve(base, RESERVED, Protection::READ_WRITE)
        .expect("reserve");
    let address = base + 0x2000;
    write(address - 4, 0x0bad_f00d);
    for at in [address - 8, address + 0xff8, address + 0xffc] {
        write(at, 0x5555_5555);
    }
    let module = device_addressed(&program());
    let mut push = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    #[expect(clippy::cast_possible_truncation, reason = "the halves of an address")]
    push[..2].copy_from_slice(&[address as u32, (address >> 32) as u32]);
    dispatch_guest(
        &module,
        &[0; 64],
        (&push, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched");
    assert_eq!(
        read(address - 8),
        0x0bad_f00d,
        "the word below, stored in the one below it"
    );
    assert_eq!(
        read(address + 0xff8),
        0x5555_5555,
        "nothing lands 4088 bytes above"
    );
    assert!(space.release(base, RESERVED), "released");
}
