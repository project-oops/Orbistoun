//! A translated texture sample, checked against the framebuffer (D690).
//!
//! The guest interpolates a texture coordinate, samples a texture with it and exports the texel.
//! `image_sample_lz` (level zero) and the plain `image_sample` (level from derivatives) are
//! different SPIR-V instructions, so each is checked against a hand-written oracle emitting its own
//! form. A guest's resource and sampler operands describe a surface at a guest address no host
//! image stands behind, so every sample reads the one texture the pipeline bound, and a module
//! naming a second is refused.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{COARSE_TEXEL, draw_with_texture};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::{Lod, interpolated_vertex_module, sampling_fragment_module};
use orbistoun_translate::wavefront::{Stage, Window, translate_for};
use orbistoun_translate::{TranslateError, Width};

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

/// A two-by-two texture whose four texels are all different, first row then second.
///
/// `R8G8B8A8_UNORM` packs a word red first, so a word reads as `0xAABBGGRR`.
const TEXELS: [u32; 4] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xff60_4020];

/// The same four, as the bytes a pixel comes back as.
const EXPECTED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [32, 64, 96, 255],
];

/// A shader that interpolates a coordinate, samples a texture with it, and exports the texel.
///
/// Written as guest instruction words:
///
/// - `v_interp_p1_f32 v0, v0, attr0.x` and the same into `v1` for `.y`: the coordinate.
/// - `image_sample<opcode> v[4:7], v[0:1], s[4:11], s[12:15] dmask:0xf`: the sample, in either
///   form.
/// - `exp mrt0 v4, v5, v6, v7`: the texel, to the attachment.
///
/// The destination does not overlap the coordinate it reads. `MIMG` is `0xF0000000` with the opcode
/// at shift 18 and the mask at shift 8 of the first word; the second word holds the coordinate
/// register at shift 0, the destination at shift 8, and the two descriptors at shifts 16 and 21
/// divided by four, as `opcode-operands.toml` records. A test below decodes it first.
fn sampling_shader(opcode: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    // v0 <- attr0.x, v1 <- attr0.y
    for channel in 0u32..2 {
        let word = 0xC800_0000u32 | (channel << 18) | (channel << 8);
        bytes.extend(word.to_le_bytes());
    }
    for word in image_sample_words(opcode) {
        bytes.extend(word.to_le_bytes());
    }
    // exp mrt0 v4, v5, v6, v7
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// The two sampling opcodes this file builds shaders from, and what each means. They differ only in
/// where the level of detail comes from, and translate to different SPIR-V instructions.
const SAMPLE_OPCODES: [(u32, Lod); 2] = [(39, Lod::Zero), (32, Lod::Implicit)];

/// First scalar register of the image descriptor the test's shader names. Four rather than zero,
/// because the field holds it divided by four and zero would hide a wrong scale.
const IMAGE_DESCRIPTOR: u32 = 4;

/// First scalar register of the sampler descriptor, past the eight the image occupies.
const SAMPLER_DESCRIPTOR: u32 = 12;

/// The second `MIMG` word, from the register numbers as a disassembly writes them. Dividing here
/// lets a caller write `s[4:11]` and `s[12:15]` as `4` and `12`, checkable against the instruction
/// by eye.
const fn mimg_operands(vaddr: u32, vdata: u32, resource: u32, sampler: u32) -> u32 {
    vaddr | (vdata << 8) | ((resource / 4) << 16) | ((sampler / 4) << 21)
}

/// `image_sample<opcode> v[4:7], v[0:1], s[4:11], s[12:15] dmask:0xf`, as its two words. One
/// function, so the decode assertion and the shaders that run use the same instruction.
const fn image_sample_words(opcode: u32) -> [u32; 2] {
    [
        // 0xF0000000, the opcode at shift 18, dmask 0xf at shift 8, and the dimensionality.
        0xF000_0000 | (opcode << 18) | (0xF << 8) | TWO_DIMENSIONAL,
        mimg_operands(0, 4, IMAGE_DESCRIPTOR, SAMPLER_DESCRIPTOR),
    ]
}

/// The dimensionality field, saying this instruction's coordinate has two components.
///
/// A three-bit field at shift 3 of the first word, with two dimensions coded as one. Zero means
/// one-dimensional, which the translation refuses for a two-register coordinate. Measured by
/// assembling one instruction at each of the eight dimensionalities and differencing.
const TWO_DIMENSIONAL: u32 = 1 << 3;

fn translated(opcode: u32) -> Result<Vec<u32>, TranslateError> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&sampling_shader(opcode), &encodings, &operands);
    translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        Stage::Fragment,
        Window::default(),
    )
    .map(|(module, _)| module)
}

/// The hand-assembled instruction is the instruction it claims to be, asserted before anything is
/// translated and with no device.
#[test]
fn the_sampling_instructions_decode_to_what_they_were_written_as() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    for (opcode, lod) in SAMPLE_OPCODES {
        let bytes: Vec<u8> = image_sample_words(opcode)
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let decoded = decode(&bytes, &encodings, &operands);
        let instruction = decoded.instructions.first().expect("one instruction");

        // The family comes back as an index into the encoding table and the name is looked up from
        // that pair, the route the translator's dispatch takes.
        let family = instruction
            .encoding
            .and_then(|i| encodings.encodings().get(usize::from(i)))
            .map(|e| e.name.as_str())
            .expect("the word matched an encoding");
        let expected = match lod {
            Lod::Zero => "image_sample_lz",
            Lod::Implicit => "image_sample",
        };
        assert_eq!(
            encodings.mnemonic_for(family, instruction.opcode),
            Some(expected),
            "opcode {opcode} is not {expected}: {family} opcode {}",
            instruction.opcode
        );

        let printed: Vec<String> = instruction
            .operands
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            printed,
            vec!["v4", "v0", "s4", "s12", "0xf"],
            concat!(
                "{}: the destination, coordinate, image descriptor, sampler and mask are not ",
                "where this test thinks"
            ),
            expected
        );
    }
}

/// A translated texture sample reads the texel its coordinate names.
///
/// The oracle's four-quadrant claim (bound and read, coordinate reaches the sample, rows the right
/// way up, nothing filtered) over a frame a guest's instruction words produced, compared byte for
/// byte with the hand-written level-zero oracle over the same geometry and texture. The texture is
/// the one the pipeline bound (D690).
#[test]
fn a_translated_sample_reads_the_texel_its_coordinate_names() {
    if !device_or_skip("a_translated_sample_reads_the_texel_its_coordinate_names") {
        return;
    }
    let size = (8, 8);
    // Zero, two and two, so the coordinate sweeps the whole texture once: the triangle's corners
    // are at (-1, -1), (3, -1) and (-1, 3). The oracle's corners.
    let corners = [
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    // Magenta, which is none of the four texels.
    let clear = [1.0, 0.0, 1.0, 1.0];
    let vertex = interpolated_vertex_module(corners);

    for (opcode, lod) in SAMPLE_OPCODES {
        let module = translated(opcode).expect("the texture sample translated");
        let by_translation = draw_with_texture(&vertex, &module, clear, size, (&TEXELS, 2))
            .expect("the translated draw ran");

        let quadrants = [((1, 1), 0), ((6, 1), 1), ((1, 6), 2), ((6, 6), 3)];
        for ((x, y), texel) in quadrants {
            assert_eq!(
                by_translation.at(x, y),
                Some(EXPECTED[texel]),
                concat!(
                    "{:?}: pixel ({}, {}) should be texel {}; magenta here means the translated ",
                    "shader sampled nothing, and another texel means the coordinate reached the ",
                    "wrong one"
                ),
                lod,
                x,
                y,
                texel
            );
        }

        let by_hand = draw_with_texture(
            &vertex,
            &sampling_fragment_module(lod),
            clear,
            size,
            (&TEXELS, 2),
        )
        .expect("the hand-written draw ran");
        assert_eq!(
            by_translation.bytes, by_hand.bytes,
            concat!(
                "{:?}: the translated sample and the hand-written oracle disagree about a frame ",
                "they differ in only by which shader produced it"
            ),
            lod
        );
    }
}

/// A shader that fetches one texel by its index and exports it.
///
/// `v_mov_b32 v0, x` and `v_mov_b32 v1, y` put the texel's index in the coordinate registers, then
/// `image_load v[4:7], v[0:1], s[4:11] dmask:0xf` reads it. Constant rather than interpolated, so
/// the claim is "this texel, exactly". `VOP1` is `0x7E000000` with the opcode at shift 9, the
/// destination at shift 17 and the source at shift 0; source `128 + k` is the inline constant `k`.
/// `image_load` has no sampler operand, so its mask sits where a sample's sampler does.
fn fetching_shader(x: u32, y: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (register, value) in [(0u32, x), (1, y)] {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | (128 + value);
        bytes.extend(word.to_le_bytes());
    }
    // 0xF0000000, dmask 0xf at shift 8, the dimensionality, and no opcode term: `image_load` is
    // opcode zero.
    bytes.extend((0xF000_0000u32 | (0xF << 8) | TWO_DIMENSIONAL).to_le_bytes());
    // The coordinate at shift 0, the destination at shift 8, and the image descriptor at shift 16
    // divided by four. No sampler field.
    bytes.extend(((4 << 8) | ((IMAGE_DESCRIPTOR / 4) << 16)).to_le_bytes());
    // exp mrt0 v4, v5, v6, v7
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// A translated fetch reads the texel its index names.
///
/// `image_load` reads a texel by integer index with no sampler, so on the host it is a fetch from
/// the image unwrapped from the bound pair. Four draws, one per texel, each asserting every pixel
/// is that texel, so a fetch ignoring its coordinate fails and the two rows are told apart.
#[test]
fn a_translated_fetch_reads_the_texel_its_index_names() {
    if !device_or_skip("a_translated_fetch_reads_the_texel_its_index_names") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let size = (8, 8);
    // Magenta, which is none of the four texels.
    let clear = [1.0, 0.0, 1.0, 1.0];
    // The fetch ignores the varying; the vertex module covers the frame.
    let vertex = interpolated_vertex_module([[0.0, 0.0, 0.0, 1.0]; 3]);

    for y in 0..2u32 {
        for x in 0..2u32 {
            let decoded = decode(&fetching_shader(x, y), &encodings, &operands);
            let module = translate_for(
                &decoded,
                &encodings,
                Width::Wave64,
                Stage::Fragment,
                Window::default(),
            )
            .map(|(module, _)| module)
            .expect("the texel fetch translated");

            let pixels = draw_with_texture(&vertex, &module, clear, size, (&TEXELS, 2))
                .expect("the translated draw ran");
            let expected = EXPECTED[(y * 2 + x) as usize];
            for pixel_y in 0..8 {
                for pixel_x in 0..8 {
                    assert_eq!(
                        pixels.at(pixel_x, pixel_y),
                        Some(expected),
                        concat!(
                            "texel ({}, {}): pixel ({}, {}) is not it. Magenta means the fetch ",
                            "read nothing; another texel means the index reached the wrong one"
                        ),
                        x,
                        y,
                        pixel_x,
                        pixel_y
                    );
                }
            }
        }
    }
}

/// An image with a different number of dimensions is refused, not read as two.
///
/// Every image translation here reads two coordinate registers, which is wrong for any other
/// dimensionality: reading two of a three-dimensional coordinate samples a place the guest never
/// named. The dimensionality is a symbolic modifier the operand solver skips, so the field (three
/// bits from bit three) is read directly. This is the two-dimensional instruction above with each
/// other dimensionality named.
#[test]
fn an_image_that_is_not_two_dimensional_is_refused() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    // Every code but two dimensions: 1D, 2D, 3D, cube, 1D array, 2D array, 2D multi-sampled, 2D
    // multi-sampled array, in the order a disassembler prints them and the measured first bytes
    // 0x00 to 0x38 put them.
    for code in (0..8u32).filter(|code| *code != 1) {
        let mut words = image_sample_words(39);
        words[0] = (words[0] & !(0b111 << 3)) | (code << 3);
        let mut bytes = Vec::new();
        for channel in 0u32..2 {
            bytes.extend((0xC800_0000u32 | (channel << 18) | (channel << 8)).to_le_bytes());
        }
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(0xBF81_0000u32.to_le_bytes());

        let decoded = decode(&bytes, &encodings, &operands);
        let error = translate_for(
            &decoded,
            &encodings,
            Width::Wave64,
            Stage::Fragment,
            Window::default(),
        )
        .expect_err("an image that is not two-dimensional must be refused");

        let TranslateError::Unsupported { detail, .. } = error else {
            panic!("dimensionality {code} refused for the wrong reason: {error:?}");
        };
        assert!(
            detail.contains("two-dimensional"),
            "dimensionality {code} refused, but not for the number of dimensions: {detail}"
        );
    }
}

/// A guest shader that samples at a level it names, and exports what came back.
///
/// Three moves fill the address registers, the coordinate and then the level last, and
/// `image_sample_l v[4:7], v[0:2], s[4:11], s[12:15]` reads with them. `VOP1` source `240` is the
/// inline float one half and `242` is one; `128` is the integer zero, whose bits are the float
/// zero.
fn levelled_shader(level: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut mov = |register: u32, source: u32| {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | source;
        bytes.extend(word.to_le_bytes());
    };
    // 0.5, 0.5: the far quadrant of a two-by-two texture, and the middle of a one-by-one.
    mov(0, 240);
    mov(1, 240);
    mov(2, level);
    // Opcode 36 is the levelled sample.
    bytes.extend((0xF000_0000u32 | (36 << 18) | (0xF << 8) | TWO_DIMENSIONAL).to_le_bytes());
    bytes.extend(mimg_operands(0, 4, IMAGE_DESCRIPTOR, SAMPLER_DESCRIPTOR).to_le_bytes());
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// A translated levelled sample reads the level it names.
///
/// The harness builds two levels, the caller's texels and one coarse texel that is none of them, so
/// the same coordinate at two levels gives two different answers. With a two-register coordinate
/// and a one-register level only one arrangement fits, so this does not show the level is the last
/// address element; that placement comes from compiler output.
#[test]
fn a_translated_levelled_sample_reads_the_level_it_names() {
    if !device_or_skip("a_translated_levelled_sample_reads_the_level_it_names") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let clear = [1.0, 0.0, 1.0, 1.0];
    let vertex = interpolated_vertex_module([[0.0, 0.0, 0.0, 1.0]; 3]);

    // The coarse texel as the bytes a pixel comes back as: a word is packed red first.
    let coarse = COARSE_TEXEL.to_le_bytes();
    // Inline source 128 is the integer zero, whose bits are the float zero; 242 is the float one.
    let expected = [(128u32, EXPECTED[3]), (242, coarse)];

    for (level, want) in expected {
        let decoded = decode(&levelled_shader(level), &encodings, &operands);
        let module = translate_for(
            &decoded,
            &encodings,
            Width::Wave64,
            Stage::Fragment,
            Window::default(),
        )
        .map(|(module, _)| module)
        .expect("the levelled sample translated");

        let pixels = draw_with_texture(&vertex, &module, clear, (8, 8), (&TEXELS, 2))
            .expect("the translated draw ran");
        assert_eq!(
            pixels.at(4, 4),
            Some(want),
            concat!(
                "level from inline source {}: the pixel is not the texel that level holds. The ",
                "two levels answer differently by construction, so the same answer for both ",
                "means the level never reached the instruction"
            ),
            level
        );
    }
}

/// A module naming a second texture is refused, not guessed at.
///
/// One texture is bound, and mapping every sample onto it would draw a plausible frame for a shader
/// using two (D690). Two samples naming different image descriptors: the second must not translate.
#[test]
fn a_second_texture_is_refused_rather_than_guessed() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    let mut bytes = Vec::new();
    for channel in 0u32..2 {
        let word = 0xC800_0000u32 | (channel << 18) | (channel << 8);
        bytes.extend(word.to_le_bytes());
    }
    let words = image_sample_words(39);
    bytes.extend(words[0].to_le_bytes());
    bytes.extend(words[1].to_le_bytes());
    // The same sample from s[20:27]: a second texture, by register number. The field is replaced,
    // not merged into.
    let elsewhere = (words[1] & !(0x1F << 16)) | ((20 / 4) << 16);
    bytes.extend(words[0].to_le_bytes());
    bytes.extend(elsewhere.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());

    let decoded = decode(&bytes, &encodings, &operands);
    let error = translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        Stage::Fragment,
        Window::default(),
    )
    .expect_err("a module sampling two textures must be refused");

    let TranslateError::Unsupported { detail, .. } = error else {
        panic!("refused for the wrong reason: {error:?}");
    };
    // Two textures bind only where each descriptor is found in the descriptor table; neither is
    // here, so which is which cannot be told.
    assert!(
        detail.contains("reads two textures"),
        "refused, but not for naming two textures: {detail}"
    );
}

/// Two textures whose descriptors come from the descriptor table translate, each at its own binding
/// and offset.
///
/// The open-toolchain GL context's second texture unit loads its image descriptor from `+0x40` of
/// the table `s[0:1]` points at, and the first from `+0x00` (oops-sdk `tex-prolog2.s`). The second
/// unit is sampled first, as that prolog does, so slot 0 is the `+0x40` texture.
#[test]
fn two_textures_from_the_descriptor_table_translate_with_their_offsets() {
    use orbistoun_translate::wavefront::{MeshPrimitive, TextureSource, UserData};
    use orbistoun_translate::{Fidelity, Strategy};
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let s_load_x8 = |dst: u32, offset: u32| {
        let (family, opcode) = encodings
            .find_by_name("s_load_dwordx8")
            .expect("the target has s_load_dwordx8");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|e| e.name == family)
            .expect("its family");
        // Destination at bit 6, the base pair s[0:1] halved at bit 0, the byte offset after.
        [
            encoding.value | (opcode << encoding.opcode.shift) | (dst << 6),
            offset,
        ]
    };

    let mut bytes = Vec::new();
    for channel in 0u32..2 {
        let word = 0xC800_0000u32 | (channel << 18) | (channel << 8);
        bytes.extend(word.to_le_bytes());
    }
    let second_unit = 20;
    for word in s_load_x8(second_unit, 0x40)
        .into_iter()
        .chain(s_load_x8(IMAGE_DESCRIPTOR, 0x00))
    {
        bytes.extend(word.to_le_bytes());
    }
    let words = image_sample_words(39);
    let from_second = (words[1] & !(0x1F << 16)) | ((second_unit / 4) << 16);
    for word in [words[0], from_second, words[0], words[1], 0xBF81_0000] {
        bytes.extend(word.to_le_bytes());
    }

    let decoded = decode(&bytes, &encodings, &operands);
    let translated = orbistoun_translate::translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Wavefront,
            width: Width::Wave64,
        },
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData::default(),
    )
    .expect("two textures from the table translate");
    assert_eq!(
        translated.textures,
        [
            TextureSource {
                slot: 0,
                table_offset: Some(0x40)
            },
            TextureSource {
                slot: 1,
                table_offset: Some(0x00)
            },
        ]
    );
}

/// A descriptor rewritten between two samples is refused too.
///
/// A shader can load a second image descriptor into the same eight registers and sample again, so
/// both samples name `s[0:7]`. A write anywhere inside either group means the descriptor is no
/// longer the one the last sample used (D690). `s_mov_b32 s6, 0` lands mid-group, which a check on
/// the group's first register would miss.
#[test]
fn a_descriptor_rewritten_between_samples_is_refused() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    let mut bytes = Vec::new();
    for channel in 0u32..2 {
        let word = 0xC800_0000u32 | (channel << 18) | (channel << 8);
        bytes.extend(word.to_le_bytes());
    }
    let words = image_sample_words(39);
    bytes.extend(words[0].to_le_bytes());
    bytes.extend(words[1].to_le_bytes());
    // `s_mov_b32 s6, 0`: SOP1 is 0xBE800000 with the opcode at shift 8, the destination at shift 16
    // and the source at shift 0. Source 128 is the inline constant zero.
    bytes.extend((0xBE80_0000u32 | (6 << 16) | (3 << 8) | 128).to_le_bytes());
    // The same texture by register number, and no longer the same texture in fact.
    bytes.extend(words[0].to_le_bytes());
    bytes.extend(words[1].to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());

    let decoded = decode(&bytes, &encodings, &operands);
    let error = translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        Stage::Fragment,
        Window::default(),
    )
    .expect_err("a descriptor rewritten between samples must be refused");

    let TranslateError::Unsupported { detail, .. } = error else {
        panic!("refused for the wrong reason: {error:?}");
    };
    assert!(
        detail.contains("written"),
        "refused, but not for the descriptor being rewritten: {detail}"
    );
}
