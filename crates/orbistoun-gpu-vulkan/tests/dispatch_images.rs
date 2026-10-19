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
    ComputeInputs, MeshPrimitive, PartialGroups, Stage, UserData, Window, translate_with_user_data,
};

thread_local! {
    /// Whether the test running on this thread translates one invocation per lane (D880).
    static PER_LANE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `test` twice: the whole wave in one invocation, then one invocation per lane (D880). Each
/// run is held to the same expectations, so the two models are checked against each other.
fn both_models(test: impl Fn()) {
    for per_lane in [false, true] {
        println!("[{}]", if per_lane { "per lane" } else { "whole wave" });
        PER_LANE.with(|cell| cell.set(per_lane));
        test();
    }
}

/// What a module translated now asks of `UserData::per_invocation`.
fn per_invocation() -> Option<u32> {
    PER_LANE.with(std::cell::Cell::get).then_some(64)
}

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
    translated_with(program, integer_images, None)
}

/// The program translated for these image types, with a last group that runs only some threads.
fn translated_with(
    program: &[u32],
    integer_images: [bool; 2],
    partial: Option<PartialGroups>,
) -> Vec<u32> {
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
                partial,
                integer_images,
                first_thread: 0,
            }),
            per_invocation: per_invocation(),
            ..UserData::default()
        },
    )
    .expect("the copy translates");
    assert_eq!(textures.first().and_then(|t| t.user_data), Some(0));
    assert_eq!(storage.first().and_then(|t| t.user_data), Some(8));
    module
}

/// Every texel of the source reaches the same place in the destination, through each form of the
/// copy. The texels are eight-bit channels, which a sixteen-bit half carries exactly.
#[test]
fn a_dispatch_copies_one_image_into_another() {
    both_models(|| {
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
                    more_fetched: &[],
                    fetched: Some(DispatchImage {
                        texels: &source,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                    }),
                    stored: &[DispatchImage {
                        texels: &destination,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                    }],
                },
            )
            .expect("dispatched");
            assert!(!done.escaped, "{form}");
            assert_eq!(
                done.stored.first().map(Vec::as_slice),
                Some(source.as_slice()),
                "{form}"
            );
        }
    });
}

/// A dispatch may store to more than one image, each its own binding, read back in the order its
/// program first stores to them (D893): Grand Theft Auto V's scene compute program writes back the
/// image it read and a second one. The copy stores what it loads to `s[8:15]` and `s[16:23]`.
#[test]
fn a_dispatch_stores_to_two_images() {
    if !device_or_skip("two stored images") {
        return;
    }
    // The copy, then a second store through `s[16:23]` (the descriptor's register / 4 at 16).
    let program = [
        COPY[0],
        COPY[1],
        COPY[2],
        COPY[3],
        COPY[4],
        COPY[3],
        0x0004_0200,
        COPY[5],
        COPY[6],
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, (_, storage)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 24,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("a copy into two images translates");
    assert_eq!(
        storage.iter().map(|t| t.user_data).collect::<Vec<_>>(),
        [Some(8), Some(16)]
    );
    let source: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0102_0304_u32 ^ i).collect();
    let destination = vec![0xdead_beef_u32; (SIDE * SIDE) as usize];
    let image = |texels| DispatchImage {
        texels,
        format: DispatchFormat::Rgba8,
        width: SIDE,
        height: SIDE,
        depth: None,
    };
    let done = dispatch_guest(
        &module,
        &[0],
        (&[0; 24], [1, 1, 1]),
        &DispatchImages {
            more_fetched: &[],
            fetched: Some(image(&source)),
            stored: &[image(&destination), image(&destination)],
        },
    )
    .expect("dispatched");
    assert!(!done.escaped);
    assert_eq!(done.stored, [source.clone(), source]);
}

/// Threads a partial last group runs along x.
const RUN: u32 = 3;

/// A last group that runs some of its threads stores only theirs: every other texel keeps what it
/// held. radeonsi reads one pixel back with one thread of 64 (`glReadPixels`), and the 63 that do
/// not run never compute their coordinates - under `a16` they all point at the origin, where a
/// store from each would put zero over the one texel the copy is for.
#[test]
fn an_inactive_thread_stores_nothing() {
    both_models(|| {
        if !device_or_skip("partial-group image copy") {
            return;
        }
        let source: Vec<u32> = (0..SIDE * SIDE).map(|i| 0xff00_0000 | (i + 1)).collect();
        let kept = 0xdead_beef_u32;
        let partial = PartialGroups {
            last: [0, 0, 0],
            threads: [RUN, SIDE, 1],
        };
        // The user data, then the partial shape where the module reads it (D855).
        let mut push = vec![0_u32; orbistoun_translate::wavefront::PARTIAL_PUSH_WORD as usize];
        push.extend(partial.push_words());
        for (form, program) in [("whole", &COPY[..]), ("a16 d16", &COPY_A16_D16[..])] {
            let destination = vec![kept; (SIDE * SIDE) as usize];
            let module = translated_with(program, [false; 2], Some(partial));
            let done = dispatch_guest(
                &module,
                &[0],
                (&push, [1, 1, 1]),
                &DispatchImages {
                    more_fetched: &[],
                    fetched: Some(DispatchImage {
                        texels: &source,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                    }),
                    stored: &[DispatchImage {
                        texels: &destination,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                    }],
                },
            )
            .expect("dispatched");
            let expected: Vec<u32> = (0..SIDE * SIDE)
                .map(|i| {
                    if i % SIDE < RUN {
                        source[i as usize]
                    } else {
                        kept
                    }
                })
                .collect();
            assert_eq!(
                done.stored.first().map(Vec::as_slice),
                Some(expected.as_slice()),
                "{form}"
            );
        }
    });
}

/// A copy between `32_32_32_32_UINT` images - radeonsi's view of a block-compressed image's
/// sixteen-byte blocks - moves every word exactly, a NaN's bits and all: the images are declared
/// unsigned, as their views are, so nothing passes through a float.
#[test]
fn a_dispatch_copies_an_unsigned_image_word_for_word() {
    both_models(|| {
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
            depth: None,
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
                more_fetched: &[],
                fetched: Some(image(&source)),
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(source.as_slice())
        );
    });
}

/// Each thread of a 4 x 4 x 4 group stores its own id and a seven, `[x, y, z, 7]`, at its own
/// place in a volume: `v_mov_b32 v3, 7`, then `image_store v[0:3], v[0:2], s[0:7] dmask:0xf
/// dim:SQ_RSRC_IMG_3D unorm`.
const STORE_IDS_3D: [u32; 5] = [
    0x7e06_0287,
    0xf020_1f10,
    0x0000_0000,
    0xbf81_0000,
    0xbf9f_0000,
];

/// A dispatch stores to a three-dimensional image (D823): every texel of a 4 x 4 x 4 unsigned
/// volume holds the id of the thread whose coordinate it is, so the slices are bound, written and
/// read back in order. Grand Theft Auto V's first sixteen-wave dispatch stores to a volume.
#[test]
fn a_dispatch_stores_to_a_volume() {
    both_models(|| {
        const EDGE: u32 = 4;
        if !device_or_skip("volume store") {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = STORE_IDS_3D.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, (_, storage)) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 8,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false, false, false],
                    thread_id_components: 3,
                    threads: [EDGE, EDGE, EDGE],
                    unwritten_user_data: 0,
                    partial: None,
                    integer_images: [false, true],
                    first_thread: 0,
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the store translates");
        assert!(storage.first().is_some_and(|source| source.volume));
        let texels = (EDGE * EDGE * EDGE) as usize;
        let destination = vec![0xdead_beef_u32; texels * 4];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 8], [1, 1, 1]),
            &DispatchImages {
                more_fetched: &[],
                fetched: None,
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba32Uint,
                    width: EDGE,
                    height: EDGE,
                    depth: Some(EDGE),
                }],
            },
        )
        .expect("dispatched");
        let expected: Vec<u32> = (0..EDGE * EDGE * EDGE)
            .flat_map(|i| [i % EDGE, (i / EDGE) % EDGE, i / (EDGE * EDGE), 7])
            .collect();
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(expected.as_slice())
        );
    });
}

/// Each thread stores opaque red at its own place: `v_mov_b32 v4, 1.0`, `v5, 0`, `v6, 0`, `v7,
/// 1.0`, then `image_store v[4:7], v[0:2], s[0:7] dmask:0xf dim:SQ_RSRC_IMG_3D unorm`.
const STORE_RED_3D: [u32; 8] = [
    0x7e08_02f2,
    0x7e0a_0280,
    0x7e0c_0280,
    0x7e0e_02f2,
    0xf020_1f10,
    0x0000_0400,
    0xbf81_0000,
    0xbf9f_0000,
];

/// A ten-bit volume, the guest's `2_10_10_10_UNORM` as Grand Theft Auto V's first sixteen-wave
/// dispatch stores to it, takes a normalised store as the guest packs it: opaque red is ten bits
/// of red at the low end and two of alpha at the top, `0xc00003ff`, in every texel.
#[test]
fn a_dispatch_stores_to_a_ten_bit_volume() {
    both_models(|| {
        const EDGE: u32 = 4;
        if !device_or_skip("ten-bit volume store") {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = STORE_RED_3D.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, _) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 8,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false, false, false],
                    thread_id_components: 3,
                    threads: [EDGE, EDGE, EDGE],
                    unwritten_user_data: 0,
                    partial: None,
                    integer_images: [false, false],
                    first_thread: 0,
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the store translates");
        let destination = vec![0_u32; (EDGE * EDGE * EDGE) as usize];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 8], [1, 1, 1]),
            &DispatchImages {
                more_fetched: &[],
                fetched: None,
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgb10A2,
                    width: EDGE,
                    height: EDGE,
                    depth: Some(EDGE),
                }],
            },
        )
        .expect("dispatched");
        let expected = vec![0xc000_03ff_u32; destination.len()];
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(expected.as_slice())
        );
    });
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
    both_models(|| {
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
            depth: None,
        };
        let done = dispatch_guest(
            &translated(&COPY_R8),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                more_fetched: &[],
                fetched: Some(image(&source)),
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(source.as_slice())
        );
    });
}

/// A single-channel eight-bit unsigned image copies exactly through a module that declares it
/// integer: every byte value reaches the same place, bound as `R8_UINT` on both sides. Grand Theft
/// Auto V's compute program reads a linear `8_UINT` image.
#[test]
fn a_dispatch_copies_an_eight_bit_integer_image() {
    both_models(|| {
        if !device_or_skip("eight-bit integer image copy") {
            return;
        }
        let source: Vec<u32> = (0..SIDE * SIDE).map(|i| (i * 7 + 1) & 0xff).collect();
        let destination = vec![0_u32; (SIDE * SIDE) as usize];
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::R8Uint,
            width: SIDE,
            height: SIDE,
            depth: None,
        };
        let done = dispatch_guest(
            &translated_for(&COPY_R8, [true, true]),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                more_fetched: &[],
                fetched: Some(image(&source)),
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(source.as_slice())
        );
    });
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
    both_models(|| {
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
                    first_thread: 0,
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the copy translates");
        let fetched = textures.first().expect("a fetched image");
        let stored = *storage.first().expect("a stored image");
        for (source, offset) in [(fetched, 0), (&stored, 32)] {
            assert_eq!(source.table_offset, Some(offset));
            assert_eq!(source.table.low, TableWord::UserData(0));
            assert_eq!(source.table.high, TableWord::UserData(1));
        }
    });
}

/// Each thread asks the fetched image's level-zero size and stores it with a seven and a nine at
/// its own place: `v_mov_b32 v4, 0`, `image_get_resinfo v[2:3], v4, s[0:7] dmask:0x3
/// dim:SQ_RSRC_IMG_2D`, `v_mov_b32 v4, 7`, `v5, 9`, then the store of [`COPY`].
const SIZE_QUERY: [u32; 10] = [
    0x7e08_0280,
    0xf038_0308,
    0x0000_0204,
    0x7e08_0287,
    0x7e0a_0289,
    0xbf8c_0000,
    0xf020_1f08,
    0x0002_0200,
    0xbf81_0000,
    0xbf9f_0000,
];

/// `image_get_resinfo` answers a level's width and height in its first two channels, as ACO reads
/// a mask of `0x3` (`aco_select_nir.cpp` 334-343): a 6 x 4 image's level zero is 6 and 4 in every
/// thread. PPSA21564's compute program at `0x50053c700` asks one before it reads.
#[test]
fn a_dispatch_asks_an_image_s_size() {
    both_models(|| {
        if !device_or_skip("image size query") {
            return;
        }
        let source = vec![0_u32; 6 * 4];
        let destination = vec![0_u32; (SIDE * SIDE * 4) as usize];
        let done = dispatch_guest(
            &translated_for(&SIZE_QUERY, [false, true]),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                more_fetched: &[],
                fetched: Some(DispatchImage {
                    texels: &source,
                    format: DispatchFormat::Rgba8,
                    width: 6,
                    height: 4,
                    depth: None,
                }),
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba32Uint,
                    width: SIDE,
                    height: SIDE,
                    depth: None,
                }],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        let stored = done.stored.first().expect("stored");
        assert!(
            stored.chunks(4).all(|texel| texel == [6, 4, 7, 9]),
            "{:?}",
            &stored[..8]
        );
    });
}

/// Three descriptors from the table at `s[0:1]` - `s_load_dwordx16 s[16:31], s[0:1], null` and
/// `s_load_dwordx8 s[32:39], s[0:1], 0x40` - then `image_load` from the images at `s[16:23]` and
/// `s[32:39]`, and the second's texels stored through `s[24:31]`.
const SECOND_IMAGE_COPY: [u32; 14] = [
    0xf410_0400,
    0xfa00_0000,
    0xf40c_0800,
    0xfa00_0040,
    0xbf8c_0000,
    0xf000_1f08,
    0x0004_0200,
    0xf000_1f08,
    0x0008_0600,
    0xbf8c_0000,
    0xf020_1f08,
    0x0006_0600,
    0xbf81_0000,
    0xbf9f_0000,
];

/// A dispatch fetching from two images binds each at its texture slot's binding, as a draw's are
/// (D690): the second image's texels, stored, are its own, not the first's. PPSA21564's compute
/// program at `0x50053c700` reads two images and asks the size of a third.
#[test]
fn a_dispatch_fetches_from_two_images() {
    both_models(|| {
        if !device_or_skip("two fetched images") {
            return;
        }
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = SECOND_IMAGE_COPY
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, (textures, storage)) = translate_with_user_data(
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
                    first_thread: 0,
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("the copy translates");
        assert_eq!(
            textures.iter().map(|t| t.table_offset).collect::<Vec<_>>(),
            [Some(0), Some(0x40)],
            "two fetched images, in slot order"
        );
        assert_eq!(storage.first().and_then(|t| t.table_offset), Some(32));
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba8,
            width: SIDE,
            height: SIDE,
            depth: None,
        };
        let first = vec![0x1111_1111_u32; (SIDE * SIDE) as usize];
        let second: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0a0b_0c00 | i).collect();
        let destination = vec![0_u32; (SIDE * SIDE) as usize];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                fetched: Some(image(&first)),
                more_fetched: &[image(&second)],
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(second.as_slice())
        );
    });
}

/// A `16_16_16_16_FLOAT` image, two words a texel, is fetched and stored as the four halves it
/// holds: a copy between two moves every texel exactly, each channel a value a half holds.
/// PPSA21564's compute program at `0x50053c700` reads a 1024 x 1024 one.
#[test]
fn a_dispatch_copies_a_half_float_image() {
    both_models(|| {
        if !device_or_skip("half-float image copy") {
            return;
        }
        // Halves 1.0, 2.0, -0.5 and 0.25 at each texel, the first moved by its index in steps of
        // 1/1024, exact in a half.
        let source: Vec<u32> = (0..SIDE * SIDE)
            .flat_map(|i| [(0x3c00 + i) | (0x4000 << 16), 0xb800 | (0x3400 << 16)])
            .collect();
        let destination = vec![0_u32; source.len()];
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba16Float,
            width: SIDE,
            height: SIDE,
            depth: None,
        };
        let done = dispatch_guest(
            &translated(&COPY),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                fetched: Some(image(&source)),
                more_fetched: &[],
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(source.as_slice())
        );
    });
}
