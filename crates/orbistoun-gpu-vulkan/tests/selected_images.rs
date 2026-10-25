//! A dispatch that selects each lane's image descriptor from a table at run time, run on the device
//! with every image the table names bound in an array and found by its descriptor (D983).
//!
//! The program is a waterfall over four records, as Grand Theft Auto V's ray-traversal programs at
//! `0x74005b854a00` and `0x74005b858800` run theirs: each lane's record is its id's low two bits,
//! loaded with `s_buffer_load_dwordx8` at the record's offset, and the lane fetches - or samples -
//! the texel its id's upper bits index from the image that record names.
//!
//! ```text
//! v_and_b32_e32 v1, 3, v0
//! v_lshlrev_b32_e32 v3, 2, v0
//! v_lshrrev_b32_e32 v4, 2, v0
//! v_mov_b32_e32 v5, 0
//! (the sample's coordinates and sampler: v6 = (x + 0.5) / 16, v7 = 0.5, s[8:11] = 0)
//! s_mov_b64 s[20:21], exec
//! loop:
//! v_readfirstlane_b32 s12, v1
//! v_cmp_eq_u32_e32 vcc, s12, v1
//! s_and_saveexec_b64 s[22:23], vcc
//! s_lshl_b32 s12, s12, 5
//! s_buffer_load_dwordx8 s[24:31], s[0:3], s12
//! s_waitcnt lgkmcnt(0)
//! image_load v2, v[4:5], s[24:31] dmask:0x1 dim:2D   (or image_sample_lz v2, v[6:7], s[24:31], s[8:11])
//! s_andn2_b64 exec, s[22:23], vcc
//! s_cbranch_execnz loop
//! s_mov_b64 exec, s[20:21]
//! s_waitcnt vmcnt(0)
//! buffer_store_dword v2, v3, s[4:7], 0 offen
//! s_endpgm
//! ```

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::{
    DispatchFormat, DispatchImage, DispatchImages, SelectedImageSet,
};
use orbistoun_gpu_vulkan::{BufferIn, dispatch_guest_with_buffers};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::Width;
use orbistoun_translate::draw_buffers::DescriptorWord;
use orbistoun_translate::wavefront::{
    ComputeInputs, MeshPrimitive, Stage, TextureSource, USER_DATA_BLOCK_WORDS, UserData, Window,
    selected_directory, translate_with_user_data,
};

/// `image_load v2, v[4:5], s[24:31] dmask:0x1 dim:SQ_RSRC_IMG_2D`.
const FETCH: [u32; 2] = [0xf000_0108, 0x0006_0204];
/// `image_sample_lz v2, v[6:7], s[24:31], s[8:11] dmask:0x1 dim:SQ_RSRC_IMG_2D`.
const SAMPLE: [u32; 2] = [0xf09c_0108, 0x0046_0206];

/// The sample's coordinates and its sampler, ahead of the waterfall: `x` at the texel's centre
/// across sixteen, `y` at the one row's, and a sampler of zeros (repeat, point filtering).
const SAMPLE_SETUP: [u32; 9] = [
    0x7e0c_0d04, // v_cvt_f32_u32_e32 v6, v4
    0x060c_0cf0, // v_add_f32_e32 v6, 0.5, v6
    0x100c_0cff, // v_mul_f32_e32 v6, 0x3d800000, v6
    0x3d80_0000,
    0x7e0e_02f0, // v_mov_b32_e32 v7, 0.5
    0xbe88_0380, // s_mov_b32 s8, 0
    0xbe89_0380, // s_mov_b32 s9, 0
    0xbe8a_0380, // s_mov_b32 s10, 0
    0xbe8b_0380, // s_mov_b32 s11, 0
];

/// `image_sample_c_lz v2, v[5:7], s[24:31], s[8:11] dmask:0x1 dim:SQ_RSRC_IMG_2D`: the reference
/// in `v5`, then the sample's coordinates.
const COMPARE: [u32; 2] = [0xf0bc_0108, 0x0046_0205];

/// The reference every lane compares against: 2005.0, which image 2's texels pass below, at and
/// above, and images 0, 1 and 3's all one way (`v_mov_b32_e32 v5, 0x44faa000`).
const REFERENCE: [u32; 2] = [0x7e0a_02ff, 0x44fa_a000];

/// The waterfall, reading with `read`'s two words.
fn program(read: [u32; 2]) -> Vec<u32> {
    let mut words = vec![
        0x3602_0083, // v_and_b32_e32 v1, 3, v0
        0x3406_0082, // v_lshlrev_b32_e32 v3, 2, v0
        0x2c08_0082, // v_lshrrev_b32_e32 v4, 2, v0
        0x7e0a_0280, // v_mov_b32_e32 v5, 0
    ];
    if read == SAMPLE || read == COMPARE {
        words.extend_from_slice(&SAMPLE_SETUP);
    }
    if read == COMPARE {
        words.extend_from_slice(&REFERENCE);
    }
    words.extend_from_slice(&[
        0xbe94_047e, // s_mov_b64 s[20:21], exec
        0x7e18_0501, // loop: v_readfirstlane_b32 s12, v1
        0x7d84_020c, // v_cmp_eq_u32_e32 vcc, s12, v1
        0xbe96_246a, // s_and_saveexec_b64 s[22:23], vcc
        0x8f0c_850c, // s_lshl_b32 s12, s12, 5
        0xf42c_0600, // s_buffer_load_dwordx8 s[24:31], s[0:3], s12
        0x1800_0000,
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
    ]);
    words.extend_from_slice(&read);
    words.extend_from_slice(&[
        0x8afe_6a16, // s_andn2_b64 exec, s[22:23], vcc
        0xbf89_fff5, // s_cbranch_execnz loop
        0xbefe_0414, // s_mov_b64 exec, s[20:21]
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xe070_1000, // buffer_store_dword v2, v3, s[4:7], 0 offen
        0x8001_0203,
        0xbf81_0000, // s_endpgm
        0xbf9f_0000,
    ]);
    words
}

/// `SQ_BUF_RSRC_WORD3` as radeonsi's raw descriptors carry it: `OOB_SELECT` 3.
const RAW: u32 = 0x3101_6fac;

/// A raw buffer descriptor of `bytes` at `base`.
fn descriptor(base: u64, bytes: u32) -> [u32; 4] {
    [base as u32, (base >> 32) as u32 & 0xffff, bytes, RAW]
}

/// Record `n`'s image descriptor: eight words no other record shares. The module compares them
/// whole; what they describe is the host's business, and here the test's.
fn record(n: u32) -> [u32; 8] {
    [
        0x0040_0000 + n * 0x100,
        0x0590_0000,
        0x0000_000f,
        0x9000_0fac,
        n,
        0,
        0,
        0,
    ]
}

/// The texel image `n` holds at `x`, as a float's bits.
fn texel(n: u32, x: u32) -> u32 {
    let (n, x) = (
        u16::try_from(n).expect("small"),
        u16::try_from(x).expect("small"),
    );
    (f32::from(n) * 1000.0 + f32::from(x)).to_bits()
}

/// Whether a device is here to run on, saying so either way.
fn device_present() -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            false
        }
    }
}

/// The waterfall reading with `read`, translated as a dispatch's is: its module and texture slots.
/// The whole wave runs in one invocation: a waterfall reads another lane's value.
fn translated(read: [u32; 2]) -> (Vec<u32>, Vec<TextureSource>) {
    translated_comparing(read, 0)
}

/// [`translated`], slot zero's compares made in the module as `compared` says (D976): the
/// function its sampler names, and whether it filters.
fn translated_comparing(read: [u32; 2], compared: u8) -> (Vec<u32>, Vec<TextureSource>) {
    translated_words(&program(read), compared)
}

/// The program `words`, translated as [`translated_comparing`] translates the waterfall.
fn translated_words(words: &[u32], compared: u8) -> (Vec<u32>, Vec<TextureSource>) {
    let mut slots = [0_u8; orbistoun_translate::wavefront::MAX_TEXTURES];
    slots[0] = compared;
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _, (textures, _)) = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            first_register: 0,
            count: 8,
            block_offset: 0,
            compute: Some(ComputeInputs {
                workgroup_ids: [false; 3],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            draw_buffers: true,
            compared: slots,
            ..UserData::default()
        },
    )
    .expect("the waterfall translates through its table");
    (module, textures)
}

/// Runs the module over the four records' images, the directory naming those `named` lists:
/// what the store's buffer holds after, and whether any lane escaped.
fn run(module: &[u32], named: &[u32]) -> (Vec<u32>, bool) {
    const TABLE: u64 = 0x4_0a00_0000;
    const DESTINATION: u64 = 0x4_0b00_0000;
    let mut block = vec![0_u32; USER_DATA_BLOCK_WORDS as usize];
    block[..4].copy_from_slice(&descriptor(TABLE, 4 * 32));
    block[4..8].copy_from_slice(&descriptor(DESTINATION, 64 * 4));
    let table: Vec<u32> = (0..4).flat_map(record).collect();
    let destination = vec![0_u32; 64];
    let texels: Vec<Vec<u32>> = named
        .iter()
        .map(|&n| (0..16).map(|x| texel(n, x)).collect())
        .collect();
    let images: Vec<DispatchImage<'_>> = texels
        .iter()
        .map(|texels| DispatchImage {
            texels,
            format: DispatchFormat::R32Float,
            width: 16,
            height: 1,
            depth: None,
            layers: None,
            sampling: None,
            cube: false,
        })
        .collect();
    let directory = selected_directory(
        &named
            .iter()
            .map(|&n| (record(n), false))
            .collect::<Vec<_>>(),
    )
    .expect("a directory");
    // Slot zero's own binding holds its sampler alone.
    let placeholder = [0_u32];
    let done = dispatch_guest_with_buffers(
        module,
        &[0],
        (&block, [1, 1, 1]),
        &DispatchImages {
            fetched: Some(DispatchImage {
                texels: &placeholder,
                format: DispatchFormat::Rgba8,
                width: 1,
                height: 1,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }),
            more_fetched: &[],
            stored: &[],
            selected: Some(SelectedImageSet {
                slots: 1,
                images: &images,
                directory: &directory,
            }),
        },
        &[BufferIn::Words(&table), BufferIn::Words(&destination)],
    )
    .expect("dispatched");
    (done.buffers[1].words.clone(), done.escaped)
}

/// What each lane reads: its record's image's texel at its index.
fn expected() -> Vec<u32> {
    (0..64).map(|lane| texel(lane & 3, lane >> 2)).collect()
}

/// The access's image is a record selected from the table at run time, its slot reading from the
/// arrays, with the table traced to the user data and the sampler to its constants (D983).
#[test]
fn the_access_selects_from_its_table() {
    let (_, textures) = translated(SAMPLE);
    let [texture] = textures.as_slice() else {
        panic!("one texture slot: {textures:?}");
    };
    let selected = texture
        .computed
        .and_then(|computed| computed.selected)
        .expect("selected at run time");
    assert_eq!(
        selected.table.words,
        [0, 1, 2, 3].map(DescriptorWord::UserData)
    );
    assert_eq!(selected.table.element, 0);
    assert_eq!(selected.sampler, Some([DescriptorWord::Constant(0); 4]));
    assert!(selected.sampled);
}

/// Each lane fetches from the image the record it selected names, texel by texel, and a lane whose
/// record the directory does not hold escapes: the dispatch's result is then refused.
#[test]
fn each_lane_fetches_from_the_image_it_selected() {
    if !device_present() {
        return;
    }
    let (module, _) = translated(FETCH);
    let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
    assert!(!escaped, "every record is in the directory");
    assert_eq!(stored, expected(), "each lane read its own image");
    let (stored, escaped) = run(&module, &[3, 0, 1]);
    assert!(escaped, "record 2 is not in the directory");
    for (lane, (&got, &want)) in (0_u32..).zip(stored.iter().zip(&expected())) {
        if lane & 3 != 2 {
            assert_eq!(got, want, "lane {lane}, the directory's places reordered");
        }
    }
}

/// A sample through the slot's sampler reads the image the lane selected the same way.
#[test]
fn each_lane_samples_the_image_it_selected() {
    if !device_present() {
        return;
    }
    let (module, _) = translated(SAMPLE);
    let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
    assert!(!escaped, "every record is in the directory");
    assert_eq!(stored, expected(), "each lane sampled its own image");
}

/// Whether `reference` passes against `texel` by the sampler's `SQ_TEX_DEPTH_COMPARE` function, as
/// a compare sampler compares a depth (`reference OP texel`).
fn passes(function: u8, reference: f32, texel: f32) -> bool {
    let order = reference.total_cmp(&texel);
    match function {
        0 => false,
        1 => order.is_lt(),
        2 => order.is_eq(),
        3 => order.is_le(),
        4 => order.is_gt(),
        5 => order.is_ne(),
        6 => order.is_ge(),
        _ => true,
    }
}

/// A depth-compare sample of a selected image is made in the module (D976) by each of the eight
/// functions a sampler names: every lane's answer, 1.0 where its reference passes against the
/// texel of the image it selected and 0.0 where it does not, below, at and above the texel.
#[test]
fn each_lane_compares_the_image_it_selected_by_every_function() {
    if !device_present() {
        return;
    }
    let reference = f32::from_bits(REFERENCE[1]);
    for function in 0..8_u8 {
        let compared = orbistoun_translate::wavefront::SHADER_COMPARE | function;
        let (module, _) = translated_comparing(COMPARE, compared);
        let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
        assert!(!escaped, "every record is in the directory");
        let want: Vec<u32> = (0..64)
            .map(|lane| {
                let texel = f32::from_bits(texel(lane & 3, lane >> 2));
                if passes(function, reference, texel) {
                    1.0_f32.to_bits()
                } else {
                    0
                }
            })
            .collect();
        assert_eq!(stored, want, "function {function}");
    }
}

/// A filtered compare of a selected image, sampled at texel centres, blends the four answers of
/// the texels a bilinear filter reads with all the weight on the centre's: every function answers
/// as the point compare does.
#[test]
fn a_filtered_compare_of_a_selected_image_answers_at_texel_centres() {
    if !device_present() {
        return;
    }
    let reference = f32::from_bits(REFERENCE[1]);
    for function in [1_u8, 3, 4, 6] {
        let compared = orbistoun_translate::wavefront::SHADER_COMPARE
            | orbistoun_translate::wavefront::SHADER_COMPARE_LINEAR
            | function;
        let (module, _) = translated_comparing(COMPARE, compared);
        let (stored, escaped) = run(&module, &[0, 1, 2, 3]);
        assert!(!escaped, "every record is in the directory");
        let want: Vec<u32> = (0..64)
            .map(|lane| {
                let texel = f32::from_bits(texel(lane & 3, lane >> 2));
                if passes(function, reference, texel) {
                    1.0_f32.to_bits()
                } else {
                    0
                }
            })
            .collect();
        assert_eq!(stored, want, "function {function}");
    }
}

/// The waterfall comparing, its sampler built in place by `setup` in place of the four moves of
/// zero, ahead of the loop.
fn comparing_with_sampler(setup: &[u32]) -> Vec<u32> {
    let mut words = program(COMPARE);
    let at = words
        .iter()
        .position(|&word| word == SAMPLE_SETUP[5])
        .expect("the sampler's moves");
    words.splice(at..at + 4, setup.iter().copied());
    words
}

/// Each bit of `word` twice, the low first: what `s_bitreplicate_b64_b32` makes.
fn replicated(word: u32) -> u64 {
    (0..32)
        .filter(|bit| word >> bit & 1 != 0)
        .fold(0_u64, |pair, bit| pair | 0b11 << (2 * bit))
}

/// A sampler a program builds from constants, as Grand Theft Auto V's shadow compares at
/// `0x74005b858800` build theirs, is traced to its words through the translator's own semantics:
/// a bitfield mask, a bit reversal, a replication and 64-bit moves of literals (D983).
#[test]
fn a_sampler_built_from_constants_is_traced() {
    use DescriptorWord::Constant;
    let (_, textures) = translated_words(
        &comparing_with_sampler(&[
            0xb008_31b6, // s_movk_i32 s8, 0x31b6
            0x9209_8c8c, // s_bfm_b32 s9, 12, 12
            0xbe8a_03ff, // s_mov_b32 s10, 0x6500000
            0x0650_0000,
            0xbe8b_0b81, // s_brev_b32 s11, 1
        ]),
        0,
    );
    let sampler = textures[0]
        .computed
        .and_then(|computed| computed.selected)
        .and_then(|selected| selected.sampler);
    assert_eq!(
        sampler,
        Some([0x31b6, 0x00ff_f000, 0x0650_0000, 0x8000_0000].map(Constant)),
        "the first form"
    );
    let (_, textures) = translated_words(
        &comparing_with_sampler(&[
            0xbe88_3bff, // s_bitreplicate_b64_b32 s[8:9], 0xfc00040
            0x0fc0_0040,
            0xbe8a_04ff, // s_mov_b64 s[10:11], 0x6500000
            0x0650_0000,
        ]),
        0,
    );
    let pair = replicated(0x0fc0_0040);
    let sampler = textures[0]
        .computed
        .and_then(|computed| computed.selected)
        .and_then(|selected| selected.sampler);
    assert_eq!(
        sampler,
        Some([pair as u32, (pair >> 32) as u32, 0x0650_0000, 0].map(Constant)),
        "the second form"
    );
}
