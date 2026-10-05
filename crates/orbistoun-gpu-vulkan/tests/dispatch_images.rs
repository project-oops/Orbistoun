//! A guest compute dispatch that copies one image into another, run on the device.
//!
//! The shape of radeonsi's compute image copy: each of an 8 x 8 group's threads loads the texel at
//! its own coordinate from the image whose descriptor is user data `s[0:7]`, and stores it at the
//! same coordinate in the one at `s[8:15]`. Once with whole-register coordinates and components,
//! and once with sixteen-bit ones (`a16 d16`), as ACO compiles the copy.
//!
//! ```text
//! image_load v[2:5], v[0:1], s[0:7] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
//! s_waitcnt 0
//! image_store v[2:5], v[0:1], s[8:15] dmask:0xf dim:SQ_RSRC_IMG_2D unorm
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::{DispatchFormat, DispatchImage, DispatchImages};
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, UserData, Window, translate_with_user_data,
};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// Whole-register coordinates `v[0:1]` and components `v[2:5]`.
const COPY: [u32; 7] = [
    0xf000_1f08,
    0x0000_0200,
    0xbf8c_0000,
    0xf020_1f08,
    0x0002_0200,
    0xbf81_0000,
    0xbf9f_0000,
];

/// The coordinate packed into `v6` as x then y halves (`v_lshl_or_b32 v6, v1, 16, v0`), and
/// four sixteen-bit components in `v[2:3]`.
const COPY_A16_D16: [u32; 9] = [
    0xd76f_0006,
    0x0401_2101,
    0xf000_1f08,
    0xc000_0206,
    0xbf8c_0000,
    0xf020_1f08,
    0xc002_0206,
    0xbf81_0000,
    0xbf9f_0000,
];

/// One 8 x 8 group, a thread per texel.
const SIDE: u32 = 8;

fn translated(program: &[u32]) -> Vec<u32> {
    translated_for(program, [false; 2])
}

/// The program translated for images whose numeric types are these: fetched, stored.
fn translated_for(program: &[u32], integer_images: [bool; 2]) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, (textures, storage)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 16,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                unwritten_user_data: 0,
                partial: None,
                integer_images,
            }),
            ..UserData::default()
        },
    )
    .expect("the copy translates");
    assert_eq!(textures.first().and_then(|t| t.user_data), Some(0));
    assert_eq!(storage.and_then(|t| t.user_data), Some(8));
    module
}

/// Every texel of the source reaches the same place in the destination, through each form of the
/// copy. The texels are eight-bit channels, which a sixteen-bit half carries exactly.
#[test]
fn a_dispatch_copies_one_image_into_another() {
    if !device_or_skip("image copy") {
        return;
    }
    let source: Vec<u32> = (0..SIDE * SIDE)
        .map(|i| 0x0102_0304_u32.wrapping_mul(i + 1) ^ (i << 24))
        .collect();
    for (form, program) in [("whole", &COPY[..]), ("a16 d16", &COPY_A16_D16[..])] {
        let destination = vec![0xdead_beef_u32; (SIDE * SIDE) as usize];
        let done = dispatch_guest(
            &translated(program),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                fetched: Some(DispatchImage {
                    texels: &source,
                    format: DispatchFormat::Rgba8,
                    width: SIDE,
                    height: SIDE,
                }),
                stored: Some(DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba8,
                    width: SIDE,
                    height: SIDE,
                }),
            },
        )
        .expect("dispatched");
        assert!(!done.escaped, "{form}");
        assert_eq!(done.stored.as_deref(), Some(source.as_slice()), "{form}");
    }
}

/// A copy between `32_32_32_32_UINT` images - radeonsi's view of a block-compressed image's
/// sixteen-byte blocks - moves every word exactly, a NaN's bits and all: the images are declared
/// unsigned, as their views are, so nothing passes through a float.
#[test]
fn a_dispatch_copies_an_unsigned_image_word_for_word() {
    if !device_or_skip("unsigned image copy") {
        return;
    }
    let source: Vec<u32> = (0..SIDE * SIDE * 4)
        .map(|i| match i % 4 {
            0 => 0x7fc0_1234,
            1 => 0xffff_ffff,
            _ => 0x0102_0304_u32.wrapping_mul(i + 1),
        })
        .collect();
    let destination = vec![0_u32; (SIDE * SIDE * 4) as usize];
    let image = |texels| DispatchImage {
        texels,
        format: DispatchFormat::Rgba32Uint,
        width: SIDE,
        height: SIDE,
    };
    // Both images are declared with the unsigned 32-bit element: `OpTypeImage` (25) whose sampled
    // type is the `OpTypeInt 32 0` (21). A driver may move a float image's bits unchanged too, so
    // the declaration, which Vulkan requires to match the view, is checked itself.
    let module = translated_for(&COPY, [true, true]);
    let mut unsigned = None;
    let mut images = Vec::new();
    let mut at = 5;
    while at < module.len() {
        let (count, opcode) = ((module[at] >> 16) as usize, module[at] & 0xffff);
        match opcode {
            21 if module[at + 2..at + 4] == [32, 0] => unsigned = Some(module[at + 1]),
            25 => images.push(module[at + 2]),
            _ => {}
        }
        at += count.max(1);
    }
    assert_eq!(images.len(), 2, "a fetched and a stored image");
    assert!(
        images.iter().all(|element| Some(*element) == unsigned),
        "{images:?} against {unsigned:?}"
    );
    let done = dispatch_guest(
        &module,
        &[0],
        (&[0; 16], [1, 1, 1]),
        &DispatchImages {
            fetched: Some(image(&source)),
            stored: Some(image(&destination)),
        },
    )
    .expect("dispatched");
    assert!(!done.escaped);
    assert_eq!(done.stored.as_deref(), Some(source.as_slice()));
}

/// One component a texel, `dmask:0x1`: `image_load v2, v[0:1], s[0:7]` then the same store.
const COPY_R8: [u32; 7] = [
    0xf000_1108,
    0x0000_0200,
    0xbf8c_0000,
    0xf020_1108,
    0x0002_0200,
    0xbf81_0000,
    0xbf9f_0000,
];

/// A single-channel image copies exactly: every byte value reaches the same place in the
/// destination, bound as `R8_UNORM` on both sides.
#[test]
fn a_dispatch_copies_a_single_channel_image() {
    if !device_or_skip("single-channel image copy") {
        return;
    }
    let source: Vec<u32> = (0..SIDE * SIDE).map(|i| (i * 4 + 3) & 0xff).collect();
    let destination = vec![0_u32; (SIDE * SIDE) as usize];
    let image = |texels| DispatchImage {
        texels,
        format: DispatchFormat::R8,
        width: SIDE,
        height: SIDE,
    };
    let done = dispatch_guest(
        &translated(&COPY_R8),
        &[0],
        (&[0; 16], [1, 1, 1]),
        &DispatchImages {
            fetched: Some(image(&source)),
            stored: Some(image(&destination)),
        },
    )
    .expect("dispatched");
    assert_eq!(done.stored.as_deref(), Some(source.as_slice()));
}

/// Both descriptors loaded from the table at `s[0:1]` by one `s_load_dwordx16 s[16:31], s[0:1], 0`
/// (its encoding as LLVM 18.1.8 assembles it), then `image_load` through `s[16:23]` and
/// `image_store` through `s[24:31]`.
const COPY_FROM_TABLE: [u32; 10] = [
    0xf410_0400,
    0xfa00_0000,
    0xbf8c_0000,
    0xf000_1f08,
    0x0004_0200,
    0xbf8c_0000,
    0xf020_1f08,
    0x0006_0200,
    0xbf81_0000,
    0xbf9f_0000,
];

/// Two image descriptors loaded together are two images of the table: the fetched one at its first
/// byte and the stored one thirty-two bytes on, as a compiler merges two adjacent eight-word loads.
#[test]
fn a_sixteen_word_load_names_two_images_of_its_table() {
    use orbistoun_translate::wavefront::TableWord;
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = COPY_FROM_TABLE
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (_, _, (textures, storage)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 2,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                unwritten_user_data: 0,
                partial: None,
                integer_images: [false; 2],
            }),
            ..UserData::default()
        },
    )
    .expect("the copy translates");
    let fetched = textures.first().expect("a fetched image");
    let stored = storage.expect("a stored image");
    for (source, offset) in [(fetched, 0), (&stored, 32)] {
        assert_eq!(source.table_offset, Some(offset));
        assert_eq!(source.table.low, TableWord::UserData(0));
        assert_eq!(source.table.high, TableWord::UserData(1));
    }
}
