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

/// A one-thread program translated as a compute module with twenty user-data words: its module, and
/// the images it samples and stores.
fn one_thread(
    program: &[u32],
) -> Result<
    (Vec<u32>, orbistoun_translate::wavefront::ImageSources),
    orbistoun_translate::TranslateError,
> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 20,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [1, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .map(|(module, _, images)| (module, images))
}

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
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                unwritten_user_data: 0,
                partial,
                integer_images,
                integer_fetched: 0,
                first_thread: 0,
                sampled_buffers: 0,
                scratch_wave_bytes: 0,
                device_addressed: false,
                float_mode: None,
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
                    selected: None,
                    more_fetched: &[],
                    fetched: Some(DispatchImage {
                        texels: &source,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                        layers: None,
                        sampling: None,
                        cube: false,
                    }),
                    stored: &[DispatchImage {
                        texels: &destination,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                        layers: None,
                        sampling: None,
                        cube: false,
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
                table_buffers: 0,
                loop_cap: 0,
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
        layers: None,
        sampling: None,
        cube: false,
    };
    let done = dispatch_guest(
        &module,
        &[0],
        (&[0; 24], [1, 1, 1]),
        &DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: Some(image(&source)),
            stored: &[image(&destination), image(&destination)],
        },
    )
    .expect("dispatched");
    assert!(!done.escaped);
    assert_eq!(done.stored, [source.clone(), source]);
}

/// A one-layer image addressed as a 2D array is its layer zero, and any other layer is out of
/// bounds - nothing read, nothing written (D895): Grand Theft Auto V's compute copy rewrites its
/// descriptors' type to `2D_ARRAY` and takes the layer from its user data. The copy at layer 0 copies
/// the source; at layer 1 the destination keeps every texel it held.
#[test]
fn a_one_layer_image_addressed_as_an_array_is_its_layer_zero() {
    if !device_or_skip("an array access to a one-layer image") {
        return;
    }
    // v_mov_b32 v2, s16 (the layer); image_load v[4:7], v[0:2], s[0:7] dmask:0xf dim:2D_ARRAY;
    // s_waitcnt; image_store v[4:7], v[0:2], s[8:15] dmask:0xf dim:2D_ARRAY; s_endpgm.
    let program = [
        0x7e04_0210_u32,
        0xf000_1f28,
        0x0000_0400,
        0xbf8c_0000,
        0xf020_1f28,
        0x0002_0400,
        0xbf81_0000,
        0xbf9f_0000,
    ];
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
            count: 17,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("an array access translates");
    assert!(
        textures.first().is_some_and(|t| t.layered),
        "the fetch is layered"
    );
    assert!(
        storage.first().is_some_and(|t| t.layered),
        "the store is layered"
    );
    let source: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0102_0304_u32 ^ i).collect();
    let destination = vec![0xdead_beef_u32; (SIDE * SIDE) as usize];
    let image = |texels| DispatchImage {
        texels,
        format: DispatchFormat::Rgba8,
        width: SIDE,
        height: SIDE,
        depth: None,
        layers: Some(1),
        sampling: None,
        cube: false,
    };
    for (layer, expected) in [(0, &source), (1, &destination)] {
        let mut push = [0; 17];
        push[16] = layer;
        let done = dispatch_guest(
            &module,
            &[0],
            (&push, [1, 1, 1]),
            &DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: Some(image(&source)),
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(done.stored.first(), Some(expected), "layer {layer}");
    }
}

/// A 2D array of several layers is fetched at the layer its address names, and a layer past its last
/// reads zero (D897): Grand Theft Auto V's compute copy reads one face of a six-layer cube through a
/// `2D_ARRAY` fetch, the layer from its user data, and stores it to a two-dimensional image.
#[test]
fn a_dispatch_fetches_the_layer_of_an_array_its_address_names() {
    if !device_or_skip("a fetch from a layer of an array") {
        return;
    }
    // v_mov_b32 v2, s16 (the layer); image_load v[4:7], v[0:2], s[0:7] dmask:0xf dim:2D_ARRAY;
    // s_waitcnt; image_store v[4:7], v[0:1], s[8:15] dmask:0xf dim:2D; s_endpgm.
    let program = [
        0x7e04_0210_u32,
        0xf000_1f28,
        0x0000_0400,
        0xbf8c_0000,
        0xf020_1f08,
        0x0002_0400,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, (textures, _)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 17,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("an array fetch translates");
    assert!(
        textures.first().is_some_and(|t| t.layered),
        "the fetch is layered"
    );
    let layers = 3_u32;
    let texels = SIDE * SIDE;
    let source: Vec<u32> = (0..texels * layers)
        .map(|i| 0x0102_0304_u32 ^ (i * 0x0101))
        .collect();
    let destination = vec![0xdead_beef_u32; texels as usize];
    for layer in 0..=layers {
        let mut push = [0; 17];
        push[16] = layer;
        let done = dispatch_guest(
            &module,
            &[0],
            (&push, [1, 1, 1]),
            &DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: Some(DispatchImage {
                    texels: &source,
                    format: DispatchFormat::Rgba8,
                    width: SIDE,
                    height: SIDE,
                    depth: None,
                    layers: Some(layers),
                    sampling: None,
                    cube: false,
                }),
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba8,
                    width: SIDE,
                    height: SIDE,
                    depth: None,
                    layers: None,
                    sampling: None,
                    cube: false,
                }],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        let expected: Vec<u32> = if layer < layers {
            let first = (layer * texels) as usize;
            source[first..first + texels as usize].to_vec()
        } else {
            vec![0; texels as usize]
        };
        assert_eq!(done.stored.first(), Some(&expected), "layer {layer}");
    }
}

/// A 2D array of several layers is stored to at the layer its address names, and a layer past its
/// last stores nothing (D897): Grand Theft Auto V's compute copy moves one face of a six-layer cube
/// into the same face of another through `2D_ARRAY` accesses, the layer from its user data.
#[test]
fn a_dispatch_stores_to_the_layer_of_an_array_its_address_names() {
    if !device_or_skip("a store to a layer of an array") {
        return;
    }
    // v_mov_b32 v2, s16 (the layer); image_load v[4:7], v[0:2], s[0:7] dmask:0xf dim:2D_ARRAY;
    // s_waitcnt; image_store v[4:7], v[0:2], s[8:15] dmask:0xf dim:2D_ARRAY; s_endpgm.
    let program = [
        0x7e04_0210_u32,
        0xf000_1f28,
        0x0000_0400,
        0xbf8c_0000,
        0xf020_1f28,
        0x0002_0400,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 17,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("an array copy translates");
    let layers = 3_u32;
    let texels = (SIDE * SIDE) as usize;
    let source: Vec<u32> = (0..SIDE * SIDE * layers)
        .map(|i| 0x0102_0304_u32 ^ (i * 0x0101))
        .collect();
    let destination = vec![0xdead_beef_u32; texels * layers as usize];
    let array = |texels| DispatchImage {
        texels,
        format: DispatchFormat::Rgba8,
        width: SIDE,
        height: SIDE,
        depth: None,
        layers: Some(layers),
        sampling: None,
        cube: false,
    };
    for layer in 0..=layers {
        let mut push = [0; 17];
        push[16] = layer;
        let done = dispatch_guest(
            &module,
            &[0],
            (&push, [1, 1, 1]),
            &DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: Some(array(&source)),
                stored: &[array(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        let mut expected = destination.clone();
        if layer < layers {
            let first = layer as usize * texels;
            expected[first..first + texels].copy_from_slice(&source[first..first + texels]);
        }
        assert_eq!(done.stored.first(), Some(&expected), "layer {layer}");
    }
}

/// A dispatch samples a texture through its guest sampler (D904): a black and white pair of texels
/// sampled at their middle reads the two averaged through a linear filter, and one of them through
/// a point filter. Grand Theft Auto V's compute program at `0x74005b75c000` samples level zero
/// (`image_sample_lz`); a sample whose level derivatives choose is refused, a dispatch having none.
#[test]
fn a_dispatch_samples_through_its_guest_sampler() {
    use orbistoun_gpu::{TextureFilter, TextureSampling};
    if !device_or_skip("a dispatch's guest sampler") {
        return;
    }
    // v_mov_b32 v2, 0; v_mov_b32 v3, 0; v_mov_b32 v0, 0.5; v_mov_b32 v1, 0.5;
    // <sample> v[4:7], v[0:1], s[0:7], s[8:11] dmask:0xf dim:2D; s_waitcnt;
    // image_store v[4:7], v[2:3], s[12:19] dmask:0xf dim:2D unorm; s_endpgm.
    let program = |sample: u32| {
        [
            0x7e04_0280_u32,
            0x7e06_0280,
            0x7e00_02f0,
            0x7e02_02f0,
            sample,
            0x0040_0400,
            0xbf8c_3f70,
            0xf020_1f08,
            0x0003_0402,
            0xbf81_0000,
            0xbf9f_0000,
        ]
    };
    // image_sample_lz, then image_sample.
    let (module, (textures, _)) =
        one_thread(&program(0xf09c_0f08)).expect("a level-zero sample translates");
    assert_eq!(
        textures.first().and_then(|t| t.sampler_user_data),
        Some(8),
        "the sampler from the user data"
    );
    assert!(
        one_thread(&program(0xf080_0f08)).is_err(),
        "a sample whose level derivatives choose"
    );
    let black_and_white = [0xff00_0000_u32, 0xffff_ffff];
    let destination = [0_u32];
    let linear = TextureSampling {
        magnify: TextureFilter::Linear,
        minify: TextureFilter::Linear,
        ..TextureSampling::CLAMPED_POINT
    };
    for (sampling, averaged) in [(linear, true), (TextureSampling::CLAMPED_POINT, false)] {
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 20], [1, 1, 1]),
            &DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: Some(DispatchImage {
                    texels: &black_and_white,
                    format: DispatchFormat::Rgba8,
                    width: 2,
                    height: 1,
                    depth: None,
                    layers: None,
                    sampling: Some(sampling),
                    cube: false,
                }),
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba8,
                    width: 1,
                    height: 1,
                    depth: None,
                    layers: None,
                    sampling: None,
                    cube: false,
                }],
            },
        )
        .expect("dispatched");
        let texel = done.stored.first().and_then(|t| t.first()).copied();
        let red = texel.map(|t| t & 0xff);
        if averaged {
            assert!(
                matches!(red, Some(0x7f | 0x80)),
                "linear: the two averaged, {texel:x?}"
            );
        } else {
            assert!(
                matches!(red, Some(0x00 | 0xff)),
                "point: one texel, {texel:x?}"
            );
        }
    }
}

/// A gather returns one channel - the one its mask's one bit names - of the four texels a sample at
/// its position would filter, in SPIR-V's `OpImageGather` order: (0, 1), (1, 1), (1, 0), (0, 0)
/// (D913). Grand Theft Auto V's fragment shader does `image_gather4_lz v[0:3], v[4:5], s[0:7],
/// s[8:11] dmask:0x1` (words 0xf11c0108 0x00400004); at the middle of a 2 x 2 image of reds
/// 0x10, 0x20 (top) and 0x30, 0x40 (bottom) it returns 0x30, 0x40, 0x20 and 0x10.
#[test]
fn a_gather_returns_one_channel_of_four_texels() {
    if !device_or_skip("a dispatch's gather") {
        return;
    }
    // v_mov_b32 v8, 0; v_mov_b32 v9, 0; v_mov_b32 v4, 0.5; v_mov_b32 v5, 0.5;
    // image_gather4_lz v[0:3], v[4:5], s[0:7], s[8:11] dmask:0x1 dim:2D; s_waitcnt;
    // image_store v[0:3], v[8:9], s[12:19] dmask:0xf dim:2D; s_endpgm.
    let words = [
        0x7e10_0280_u32,
        0x7e12_0280,
        0x7e08_02f0,
        0x7e0a_02f0,
        0xf11c_0108,
        0x0040_0004,
        0xbf8c_3f70,
        0xf020_1f08,
        0x0003_0008,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 20,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [1, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("a gather translates");
    let reds = [0xff00_0010_u32, 0xff00_0020, 0xff00_0030, 0xff00_0040];
    let destination = [0_u32];
    let done = dispatch_guest(
        &module,
        &[0],
        (&[0; 20], [1, 1, 1]),
        &DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: Some(DispatchImage {
                texels: &reds,
                format: DispatchFormat::Rgba8,
                width: 2,
                height: 2,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }),
            stored: &[DispatchImage {
                texels: &destination,
                format: DispatchFormat::Rgba8,
                width: 1,
                height: 1,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }],
        },
    )
    .expect("dispatched");
    assert_eq!(done.stored.first(), Some(&vec![0x1020_4030]));
}

/// A dispatch stores packed eleven-, eleven- and ten-bit floats as the guest's `10_11_11_FLOAT`
/// holds them, red from the low end (D908): `(1.0, 0.5, 2.0)` is `0x3c0 | 0x380 << 11 | 0x200 << 22`.
#[test]
fn a_dispatch_stores_packed_eleven_eleven_ten_floats() {
    if !device_or_skip("a dispatch's packed floats") {
        return;
    }
    // v_mov_b32 v8, 0; v_mov_b32 v9, 0; v_mov_b32 v4, 1.0; v_mov_b32 v5, 0.5; v_mov_b32 v6, 2.0;
    // v_mov_b32 v7, 1.0; image_store v[4:7], v[8:9], s[12:19] dmask:0xf dim:2D; s_endpgm.
    let words = [
        0x7e10_0280_u32,
        0x7e12_0280,
        0x7e08_02f2,
        0x7e0a_02f0,
        0x7e0c_02f4,
        0x7e0e_02f2,
        0xf020_1f08,
        0x0003_0408,
        0xbf81_0000,
        0xbf9f_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, _) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 20,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [1, 1, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("a store translates");
    let destination = [0_u32];
    let done = dispatch_guest(
        &module,
        &[0],
        (&[0; 20], [1, 1, 1]),
        &DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: None,
            stored: &[DispatchImage {
                texels: &destination,
                format: DispatchFormat::Float11_11_10,
                width: 1,
                height: 1,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }],
        },
    )
    .expect("dispatched");
    assert_eq!(done.stored.first(), Some(&vec![0x801c_03c0]));
}

/// A dispatch samples a cube through a cube view of its six faces (D907): a 1 x 1 cube of six
/// colours sampled at the middle of face `f` (`image_sample_lz dim:CUBE` at `(1.5, 1.5, f)`, the
/// face coordinates as ACO offsets them) reads face `f`'s colour. Grand Theft Auto V's
/// loading-screen program samples a 32 x 32 `16_16_16_16_FLOAT` cube so.
#[test]
fn a_dispatch_samples_a_cube_s_faces() {
    if !device_or_skip("a dispatch's cube") {
        return;
    }
    // v_mov_b32 v8, 0; v_mov_b32 v9, 0; v_mov_b32 v0, 1.5; v_mov_b32 v1, 1.5; v_mov_b32 v2, face;
    // image_sample_lz v[4:7], v[0:2], s[0:7], s[8:11] dmask:0xf dim:CUBE; s_waitcnt;
    // image_store v[4:7], v[8:9], s[12:19] dmask:0xf dim:2D unorm; s_endpgm.
    let program = |face: f32| {
        [
            0x7e10_0280_u32,
            0x7e12_0280,
            0x7e00_02ff,
            1.5f32.to_bits(),
            0x7e02_02ff,
            1.5f32.to_bits(),
            0x7e04_02ff,
            face.to_bits(),
            0xf09c_0f18,
            0x0040_0400,
            0xbf8c_3f70,
            0xf020_1f08,
            0x0003_0408,
            0xbf81_0000,
            0xbf9f_0000,
        ]
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let faces = [
        0xff00_00ff_u32,
        0xff00_ff00,
        0xffff_0000,
        0xff00_ffff,
        0xffff_00ff,
        0xffff_ff00,
    ];
    let destination = [0_u32];
    for (face, &colour) in (0_u8..).zip(&faces) {
        let bytes: Vec<u8> = program(f32::from(face))
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, (textures, _)) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 20,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false, false, false],
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [1, 1, 1],
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
        .expect("a cube sample translates");
        assert!(
            textures.first().is_some_and(|t| t.cube),
            "sampled as a cube"
        );
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 20], [1, 1, 1]),
            &DispatchImages {
                selected: None,
                more_fetched: &[],
                fetched: Some(DispatchImage {
                    texels: &faces,
                    format: DispatchFormat::Rgba8,
                    width: 1,
                    height: 1,
                    depth: None,
                    layers: Some(6),
                    sampling: None,
                    cube: true,
                }),
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba8,
                    width: 1,
                    height: 1,
                    depth: None,
                    layers: None,
                    sampling: None,
                    cube: false,
                }],
            },
        )
        .expect("dispatched");
        assert_eq!(done.stored.first(), Some(&vec![colour]), "face {face}");
    }
}

/// Registers reloaded from a table offset a texture was already loaded from are that texture again,
/// however often other loads came between (D690): Grand Theft Auto V's loading-screen program
/// reloads `s[16:23]` from offset 0x60 before each of its many samples, between loads from 0x20 and
/// 0x40, and each reload took a binding of its own until sixteen ran out.
#[test]
fn a_reload_from_a_texture_s_offset_is_that_texture() {
    // s_load_dwordx8 s[16:23], s[12:13], 0x60; s_waitcnt; image_sample_lz v2, v0, s[16:23], s[8:11]
    // dmask:0x1; the same through a load from 0x40, then from 0x60 again; s_endpgm.
    let load = |offset: u32| [0xf40c_0406_u32, 0xfa00_0000 | offset];
    let sample = [0xf09c_0108_u32, 0x0044_0200];
    let wait = 0xbf8c_c07f_u32;
    let mut words = Vec::new();
    words.extend(load(0x60));
    words.push(wait);
    words.extend(sample);
    for offset in [0x40, 0x60] {
        words.extend(load(offset));
        words.push(wait);
        words.extend(sample);
    }
    words.push(0xbf81_0000);
    let (_, (textures, _)) = one_thread(&words).expect("the samples translate");
    let offsets: Vec<Option<u32>> = textures.iter().map(|t| t.table_offset).collect();
    assert_eq!(offsets, [Some(0x60), Some(0x40)], "two textures, not three");
}

/// A store names the image the load reaching it read, as a sample does (D690): registers loaded
/// from one place in the table for a fetch and reloaded from another for a store name two images.
/// Grand Theft Auto V's compute program at `0x74005b75c000` loads `s[0:7]` from its table again
/// just before its last store.
#[test]
fn a_store_names_the_descriptor_its_reaching_load_read() {
    // s_load_dwordx8 s[0:7], s[12:13], 0x0; s_waitcnt;
    // image_load v[4:7], v[0:1], s[0:7] dmask:0xf dim:2D;
    // s_load_dwordx8 s[0:7], s[12:13], 0x40; s_waitcnt;
    // image_store v[4:7], v[0:1], s[0:7] dmask:0xf dim:2D; s_endpgm.
    let words = [
        0xf40c_0006_u32,
        0xfa00_0000,
        0xbf8c_c07f,
        0xf000_1f08,
        0x0000_0400,
        0xf40c_0006,
        0xfa00_0040,
        0xbf8c_c07f,
        0xf020_1f08,
        0x0000_0400,
        0xbf81_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (_, _, (textures, storage)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            count: 14,
            compute: Some(ComputeInputs {
                workgroup_ids: [false, false, false],
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("a store through a reloaded descriptor translates");
    assert_eq!(textures.first().and_then(|t| t.table_offset), Some(0));
    assert_eq!(storage.first().and_then(|t| t.table_offset), Some(0x40));
}

/// Two fetches through registers the program rewrote between them read two images, each bound at
/// its own texture slot (D690): the second's descriptor, which no table load or user-data word
/// names, is found per dispatch where the program reaches it (D753). Grand Theft Auto V's
/// full-screen compute pass at `0x74005b809c00` reads two textures through `s[0:7]` so. Each texel
/// fetched is stored to an image of its own: the first's, then the second's.
#[test]
fn registers_rewritten_between_two_fetches_read_two_images() {
    both_models(|| {
        // image_load v[4:7], v[0:1], s[0:7] dmask:0xf dim:2D; s_add_u32 s0, s0, s14; the same
        // into v[8:11]; s_waitcnt; image_store v[4:7], v[0:1], s[16:23]; v[8:11] through s[24:31];
        // s_endpgm.
        let words = [
            0xf000_1f08_u32,
            0x0000_0400,
            0x8000_0e00,
            0xf000_1f08,
            0x0000_0800,
            0xbf8c_c07f,
            0xf020_1f08,
            0x0004_0400,
            0xf020_1f08,
            0x0006_0800,
            0xbf81_0000,
        ];
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let (module, _, (textures, storage)) = translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 32,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false, false, false],
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [SIDE, SIDE, 1],
                    ..ComputeInputs::default()
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("two fetches through rewritten registers translate");
        assert_eq!(textures.len(), 2, "two textures");
        assert_eq!(textures[0].user_data, Some(0));
        assert_eq!(
            textures[1].computed.map(|c| (c.at, c.register)),
            Some((12, 0))
        );
        assert_eq!(storage.len(), 2);
        if !device_or_skip("two fetches through rewritten registers") {
            return;
        }
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba8,
            width: SIDE,
            height: SIDE,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        };
        let first = vec![0x1111_1111_u32; (SIDE * SIDE) as usize];
        let second: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0a0b_0c00 | i).collect();
        let destination = vec![0_u32; (SIDE * SIDE) as usize];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 32], [1, 1, 1]),
            &DispatchImages {
                selected: None,
                fetched: Some(image(&first)),
                more_fetched: &[image(&second)],
                stored: &[image(&destination), image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(done.stored, [first.clone(), second.clone()]);
    });
}

/// A dispatch binds each image it stores to, past the four every device has (D963), and a store
/// names the image the load reaching it read, whatever registers it goes through: Grand Theft Auto
/// V's full-screen compute pass at `0x74005b809c00` stores to six images, reloading `s[16:23]` from
/// one table offset after another. The copy fetches the image at table offset zero and stores it to
/// the six that follow, each loaded into `s[16:23]` in turn; each comes back equal to the source.
#[test]
fn a_dispatch_stores_to_each_image_its_reloaded_registers_name() {
    both_models(|| {
        const STORES: u32 = 6;
        // s_load_dwordx8 s[16:23], s[0:1], offset; s_waitcnt.
        let load = |offset: u32| [0xf40c_0400_u32, 0xfa00_0000 | offset, 0xbf8c_c07f];
        let mut words = load(0).to_vec();
        // image_load v[2:5], v[0:1], s[16:23] dmask:0xf dim:2D; s_waitcnt.
        words.extend([0xf000_1f08, 0x0004_0200, 0xbf8c_0000]);
        for image in 1..=STORES {
            words.extend(load(32 * image));
            // image_store v[2:5], v[0:1], s[16:23] dmask:0xf dim:2D.
            words.extend([0xf020_1f08, 0x0004_0200]);
        }
        words.push(0xbf81_0000);
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
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
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [SIDE, SIDE, 1],
                    ..ComputeInputs::default()
                }),
                per_invocation: per_invocation(),
                ..UserData::default()
            },
        )
        .expect("six stores translate");
        assert_eq!(
            textures.iter().map(|t| t.table_offset).collect::<Vec<_>>(),
            [Some(0)]
        );
        assert_eq!(
            storage.iter().map(|t| t.table_offset).collect::<Vec<_>>(),
            (1..=STORES)
                .map(|image| Some(32 * image))
                .collect::<Vec<_>>(),
            "one stored image for each load reaching a store"
        );
        if !device_or_skip("six stored images") {
            return;
        }
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba8,
            width: SIDE,
            height: SIDE,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        };
        let source: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0a0b_0c00 | i).collect();
        let destination = vec![0_u32; (SIDE * SIDE) as usize];
        let stored = vec![image(&destination); STORES as usize];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
                fetched: Some(image(&source)),
                more_fetched: &[],
                stored: &stored,
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(done.stored, vec![source.clone(); STORES as usize]);
    });
}

/// A program that rewrites only the type of image descriptors it loaded from its table keeps the
/// images the table names (D895): Grand Theft Auto V's compute copy loads two descriptors with one
/// sixteen-word load, sets each one's type to `2D_ARRAY` through `vcc_lo`, fetches from the second
/// and stores to the first. A rewrite of any other bit would name another image, and is refused.
#[test]
fn a_rewrite_of_a_table_descriptor_s_type_alone_keeps_its_image() {
    let program = |mask: u32| {
        [
            0xf410_0300_u32, // s_load_dwordx16 s[12:27], s[0:1], 0x0
            0xfa00_0000,
            0xbf8c_c07f, // s_waitcnt
            0x876a_ff17, // s_and_b32 vcc_lo, s23, mask
            mask,
            0x8817_ff6a, // s_or_b32 s23, vcc_lo, 0xd0000000
            0xd000_0000,
            0xf000_3128, // image_load v0, v[0:2], s[20:27] dmask:0x1 dim:2D_ARRAY
            0x0005_0000,
            0x876a_ff0f, // s_and_b32 vcc_lo, s15, mask
            mask,
            0x880f_ff6a, // s_or_b32 s15, vcc_lo, 0xd0000000
            0xd000_0000,
            0xbf8c_3f70, // s_waitcnt
            0xf020_3128, // image_store v0, v[4:6], s[12:19] dmask:0x1 dim:2D_ARRAY
            0x0003_0004,
            0xbf81_0000,
        ]
    };
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let translate = |words: [u32; 17]| {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &encodings, &operands);
        translate_with_user_data(
            &decoded,
            &encodings,
            Width::Wave64,
            (Stage::Compute, MeshPrimitive::default()),
            Window::default(),
            UserData {
                count: 2,
                compute: Some(ComputeInputs {
                    workgroup_ids: [false, false, false],
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [SIDE, SIDE, 1],
                    ..ComputeInputs::default()
                }),
                ..UserData::default()
            },
        )
    };
    let (_, _, (textures, storage)) =
        translate(program(0x0fff_ffff)).expect("a type rewrite keeps the images");
    assert_eq!(
        textures.first().map(|t| (t.table_offset, t.layered)),
        Some((Some(0x20), true))
    );
    assert_eq!(
        storage.first().map(|t| (t.table_offset, t.layered)),
        Some((Some(0), true))
    );
    assert!(
        translate(program(0x0fff_fff0)).is_err(),
        "a rewrite of the descriptor's low bits names another image"
    );
}

/// A type rewrite whose two instructions have another between them that touches neither register
/// keeps the image as an adjacent pair does (D895): Grand Theft Auto V's compute copy at
/// `0x74004314a600` computes a store coordinate between them, and fetches through `image_load_mip`
/// at a level its user data names.
#[test]
fn a_type_rewrite_split_by_an_unrelated_instruction_keeps_its_image() {
    let words = [
        0xf410_0300_u32, // s_load_dwordx16 s[12:27], s[0:1], 0x0
        0xfa00_0000,
        0xbf8c_c07f, // s_waitcnt
        0x876a_ff17, // s_and_b32 vcc_lo, s23, 0x0fffffff
        0x0fff_ffff,
        0x4a08_0802, // v_add_nc_u32_e32 v4, s2, v4
        0x8817_ff6a, // s_or_b32 s23, vcc_lo, 0xd0000000
        0xd000_0000,
        0xf004_3128, // image_load_mip v0, v[0:3], s[20:27] dmask:0x1 dim:2D_ARRAY
        0x0005_0000,
        0x876a_ff0f, // s_and_b32 vcc_lo, s15, 0x0fffffff
        0x0fff_ffff,
        0x880f_ff6a, // s_or_b32 s15, vcc_lo, 0xd0000000
        0xd000_0000,
        0xbf8c_3f70, // s_waitcnt
        0xf020_3128, // image_store v0, v[4:6], s[12:19] dmask:0x1 dim:2D_ARRAY
        0x0003_0004,
        0xbf81_0000,
    ];
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
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
                table_buffers: 0,
                loop_cap: 0,
                thread_id_components: 2,
                threads: [SIDE, SIDE, 1],
                ..ComputeInputs::default()
            }),
            ..UserData::default()
        },
    )
    .expect("a split type rewrite keeps the images");
    assert_eq!(
        textures.first().map(|t| (t.table_offset, t.layered)),
        Some((Some(0x20), true))
    );
    assert_eq!(
        storage.first().map(|t| (t.table_offset, t.layered)),
        Some((Some(0), true))
    );
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
                    selected: None,
                    more_fetched: &[],
                    fetched: Some(DispatchImage {
                        texels: &source,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                        layers: None,
                        sampling: None,
                        cube: false,
                    }),
                    stored: &[DispatchImage {
                        texels: &destination,
                        format: DispatchFormat::Rgba8,
                        width: SIDE,
                        height: SIDE,
                        depth: None,
                        layers: None,
                        sampling: None,
                        cube: false,
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
            layers: None,
            sampling: None,
            cube: false,
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
                selected: None,
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
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 3,
                    threads: [EDGE, EDGE, EDGE],
                    unwritten_user_data: 0,
                    partial: None,
                    integer_images: [false, true],
                    integer_fetched: 0,
                    first_thread: 0,
                    sampled_buffers: 0,
                    scratch_wave_bytes: 0,
                    device_addressed: false,
                    float_mode: None,
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
                selected: None,
                more_fetched: &[],
                fetched: None,
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba32Uint,
                    width: EDGE,
                    height: EDGE,
                    depth: Some(EDGE),
                    layers: None,
                    sampling: None,
                    cube: false,
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
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 3,
                    threads: [EDGE, EDGE, EDGE],
                    unwritten_user_data: 0,
                    partial: None,
                    integer_images: [false, false],
                    integer_fetched: 0,
                    first_thread: 0,
                    sampled_buffers: 0,
                    scratch_wave_bytes: 0,
                    device_addressed: false,
                    float_mode: None,
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
                selected: None,
                more_fetched: &[],
                fetched: None,
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgb10A2,
                    width: EDGE,
                    height: EDGE,
                    depth: Some(EDGE),
                    layers: None,
                    sampling: None,
                    cube: false,
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
            layers: None,
            sampling: None,
            cube: false,
        };
        let done = dispatch_guest(
            &translated(&COPY_R8),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
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
            layers: None,
            sampling: None,
            cube: false,
        };
        let done = dispatch_guest(
            &translated_for(&COPY_R8, [true, true]),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
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
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [SIDE, SIDE, 1],
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
                selected: None,
                more_fetched: &[],
                fetched: Some(DispatchImage {
                    texels: &source,
                    format: DispatchFormat::Rgba8,
                    width: 6,
                    height: 4,
                    depth: None,
                    layers: None,
                    sampling: None,
                    cube: false,
                }),
                stored: &[DispatchImage {
                    texels: &destination,
                    format: DispatchFormat::Rgba32Uint,
                    width: SIDE,
                    height: SIDE,
                    depth: None,
                    layers: None,
                    sampling: None,
                    cube: false,
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
                    table_buffers: 0,
                    loop_cap: 0,
                    thread_id_components: 2,
                    threads: [SIDE, SIDE, 1],
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
            layers: None,
            sampling: None,
            cube: false,
        };
        let first = vec![0x1111_1111_u32; (SIDE * SIDE) as usize];
        let second: Vec<u32> = (0..SIDE * SIDE).map(|i| 0x0a0b_0c00 | i).collect();
        let destination = vec![0_u32; (SIDE * SIDE) as usize];
        let done = dispatch_guest(
            &module,
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
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

/// A `16_16_16_16_UNORM` image, two words a texel, is fetched and stored as the four normalised
/// channels it holds (D923): a copy moves every texel exactly, as each sixteen-bit value is a float
/// that converts back to itself. PPSA21564's compute program at submission 7 reads a 256 x 64 one.
#[test]
fn a_dispatch_copies_a_sixteen_bit_normalised_image() {
    both_models(|| {
        if !device_or_skip("16_16_16_16_UNORM image copy") {
            return;
        }
        // Every channel a different value across the whole range, both extremes among them.
        let source: Vec<u32> = (0..SIDE * SIDE * 2)
            .map(|i| {
                let low = (i * 7919) & 0xFFFF;
                let high = 0xFFFF - ((i * 104_729) & 0xFFFF);
                low | high << 16
            })
            .collect();
        let destination = vec![0_u32; source.len()];
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba16Unorm,
            width: SIDE,
            height: SIDE,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        };
        let done = dispatch_guest(
            &translated(&COPY),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
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

/// An `8_8_8_8_SNORM` image, a word a texel, is fetched and stored as the four signed normalised
/// channels it holds: a copy moves every byte unchanged but -128, which reads as -1 as -127 does
/// and so is written back as -127 (Vulkan 3.9.2, "Conversion from Normalized Fixed-Point to
/// Floating-Point"). Grand Theft Auto V's in-world compute pass fetches a 256 x 256 one.
#[test]
fn a_dispatch_copies_a_signed_normalised_image() {
    both_models(|| {
        if !device_or_skip("8_8_8_8_SNORM image copy") {
            return;
        }
        // Every byte value, -128 among them, across the channels.
        let source: Vec<u32> = (0..SIDE * SIDE)
            .map(|i| u32::from_le_bytes([0_u32, 1, 2, 3].map(|c| ((i * 4 + c * 67) & 0xFF) as u8)))
            .collect();
        let expected: Vec<u32> = source
            .iter()
            .map(|&texel| {
                u32::from_le_bytes(
                    texel
                        .to_le_bytes()
                        .map(|byte| if byte == 0x80 { 0x81 } else { byte }),
                )
            })
            .collect();
        assert!(
            source
                .iter()
                .any(|texel| texel.to_le_bytes().contains(&0x80))
        );
        let destination = vec![0_u32; source.len()];
        let image = |texels| DispatchImage {
            texels,
            format: DispatchFormat::Rgba8Snorm,
            width: SIDE,
            height: SIDE,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        };
        let done = dispatch_guest(
            &translated(&COPY),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
                fetched: Some(image(&source)),
                more_fetched: &[],
                stored: &[image(&destination)],
            },
        )
        .expect("dispatched");
        assert!(!done.escaped);
        assert_eq!(
            done.stored.first().map(Vec::as_slice),
            Some(expected.as_slice())
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
            layers: None,
            sampling: None,
            cube: false,
        };
        let done = dispatch_guest(
            &translated(&COPY),
            &[0],
            (&[0; 16], [1, 1, 1]),
            &DispatchImages {
                selected: None,
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

/// A dispatch's depth-compare sample compares in the module (D976): `image_sample_c_lz` of a 1 x 1
/// depth image holding 0.5, at its middle, against references below, at and above it, answers 1.0
/// where the sampler's function holds of the reference and the depth, and 0.0 where it does not,
/// for each of the eight functions (`SQ_TEX_DEPTH_COMPARE`, never to always, in `VkCompareOp`'s
/// order). The depths are the image's texels, as a dispatch binds a depth target (D894).
///
/// ```text
/// v_mov_b32 v0, <reference> ; v_mov_b32 v1, 0.5 ; v_mov_b32 v2, 0.5 ; v_mov_b32 v8, 0 ; v9, 0
/// image_sample_c_lz v4, v[0:2], s[0:7], s[8:11] dmask:0x1 dim:2D
/// s_waitcnt
/// image_store v4, v[8:9], s[12:19] dmask:0x1 dim:2D unorm
/// s_endpgm
/// ```
#[test]
fn a_dispatch_s_compare_sample_answers_each_function() {
    use orbistoun_translate::wavefront::SHADER_COMPARE;
    use std::cmp::Ordering::{Equal, Greater, Less};
    if !device_or_skip("a dispatch's compare sample") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("encodings");
    let (_, opcode) = encodings
        .find_by_name("image_sample_c_lz")
        .expect("image_sample_c_lz");
    // `image_sample_lz`'s word with its opcode (bits 24:18) and mask (bits 11:8) replaced.
    let sample = (0xf09c_0f08_u32 & !(0x7f << 18) & !(0xf << 8)) | (opcode << 18) | (1 << 8);
    let program = |reference: f32| {
        [
            0x7e00_02ff_u32,
            reference.to_bits(),
            0x7e02_02f0,
            0x7e04_02f0,
            0x7e10_0280,
            0x7e12_0280,
            sample,
            0x0040_0400,
            0xbf8c_3f70,
            0xf020_1108,
            0x0003_0408,
            0xbf81_0000,
            0xbf9f_0000,
        ]
    };
    let operands = OperandTable::builtin().expect("operands");
    // Each function by the orderings of the reference against the depth it holds for.
    let functions: [(&str, &[std::cmp::Ordering]); 8] = [
        ("never", &[]),
        ("less", &[Less]),
        ("equal", &[Equal]),
        ("less or equal", &[Less, Equal]),
        ("greater", &[Greater]),
        ("not equal", &[Less, Greater]),
        ("greater or equal", &[Greater, Equal]),
        ("always", &[Less, Equal, Greater]),
    ];
    for (function, (name, holds)) in (0u8..).zip(functions) {
        for (reference, ordering) in [(0.25f32, Less), (0.5, Equal), (0.75, Greater)] {
            let bytes: Vec<u8> = program(reference)
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect();
            let decoded = decode(&bytes, &encodings, &operands);
            let mut compared = [0u8; orbistoun_translate::wavefront::MAX_TEXTURES];
            compared[0] = SHADER_COMPARE | function;
            let (module, _, (textures, _)) = translate_with_user_data(
                &decoded,
                &encodings,
                Width::Wave64,
                (Stage::Compute, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 20,
                    compute: Some(ComputeInputs {
                        thread_id_components: 2,
                        threads: [1, 1, 1],
                        ..ComputeInputs::default()
                    }),
                    compared,
                    ..UserData::default()
                },
            )
            .expect("a level-zero compare sample translates");
            assert!(
                textures
                    .first()
                    .is_some_and(|t| t.compare && t.shader_compare),
                "compared in the module"
            );
            let wanted = if holds.contains(&ordering) {
                1.0f32
            } else {
                0.0
            };
            assert_eq!(
                compared_against_half(&module, function),
                Some(wanted.to_bits()),
                "{name}: reference {reference} against depth 0.5"
            );
        }
    }
}

/// What `module` stores, run over a 1 x 1 depth image holding 0.5 through a point sampler whose
/// compare function is `function`, into a 1 x 1 float image.
fn compared_against_half(module: &[u32], function: u8) -> Option<u32> {
    let (depth, destination) = ([0.5f32.to_bits()], [0x5555_5555_u32]);
    let image = |texels, sampling| DispatchImage {
        texels,
        format: DispatchFormat::R32Float,
        width: 1,
        height: 1,
        depth: None,
        layers: None,
        sampling,
        cube: false,
    };
    let sampling = orbistoun_gpu::TextureSampling {
        depth_compare: function,
        ..orbistoun_gpu::TextureSampling::CLAMPED_POINT
    };
    let done = dispatch_guest(
        module,
        &[0],
        (&[0; 20], [1, 1, 1]),
        &DispatchImages {
            selected: None,
            more_fetched: &[],
            fetched: Some(image(&depth, Some(sampling))),
            stored: &[image(&destination, None)],
        },
    )
    .expect("dispatched");
    done.stored.first().and_then(|t| t.first()).copied()
}
