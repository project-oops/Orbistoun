//! A **translated** texture sample, checked against the framebuffer.
//!
//! `translated_interpolation.rs` put a translated interpolated attribute on the screen. This
//! puts a translated *texel* there: the guest interpolates a texture coordinate, samples a
//! texture with it, and exports what came back.
//!
//! **Both sampling forms.** A guest's `image_sample_lz` names level zero and the plain
//! `image_sample` lets the implementation choose from the derivatives of the coordinate. Those
//! are two different SPIR-V instructions with different operands, so each is checked against the
//! hand-written oracle emitting *its own* form - checking one against the other's oracle would
//! pass on a texture with one level and prove nothing.
//!
//! The host half went up first and on purpose (worklog 566): an image, a view, a sampler and a
//! hand-assembled module that reads a known texel through them. This is the translation
//! measured against it, which is the order D549 argues for.
//!
//! What a guest's instruction names and what this binds are not the same thing, and D690 is
//! why. Its resource operand names eight scalar registers holding an image descriptor and its
//! sampler operand four more; those describe a surface at a guest address that no host image
//! stands behind. Every sample in a module therefore reads the one texture the pipeline bound,
//! and a module that names a second is refused rather than drawn wrong.

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
/// Written as guest instruction words, so what is translated is the encoding rather than a
/// convenience. Four instructions:
///
/// - `v_interp_p1_f32 v0, v0, attr0.x` and the same into `v1` for `.y` - the coordinate, which
///   a real textured shader gets exactly this way.
/// - `image_sample<opcode> v[4:7], v[0:1], s[4:11], s[12:15] dmask:0xf` - the sample, in
///   whichever of the two forms the caller asked for.
/// - `exp mrt0 v4, v5, v6, v7` - the texel, to the attachment.
///
/// The destination is `v[4:7]` rather than `v[0:3]` so it does not overlap the coordinate it
/// reads, which would make the second half of the assertion depend on evaluation order.
///
/// # Where the bits come from
///
/// `MIMG` is `0xF0000000` with the opcode at shift 18, and the mask at shift 8 of the first
/// word - the encoding table's own row. The second word holds the coordinate register at shift
/// 0, the destination at shift 8, and the two descriptors at shifts 16 and 21 **divided by
/// four**, which is the layout `opcode-operands.toml` solved from assembled probes. The test
/// below decodes it and asserts every field before anything is translated, so a hand-assembly
/// that is wrong fails as itself rather than as a wrong picture.
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

/// The two sampling opcodes this file builds shaders from, and what each one means.
///
/// **They differ only in where the level of detail comes from**, which is the whole reason both
/// are here: the guest's `_lz` names level zero and the plain form lets the implementation
/// choose from the derivatives of the coordinate. Those are two different SPIR-V instructions
/// with different operands, so a translation checked on one says nothing about the other.
const SAMPLE_OPCODES: [(u32, Lod); 2] = [(39, Lod::Zero), (32, Lod::Implicit)];

/// First scalar register of the image descriptor the test's shader names.
///
/// Four rather than zero on purpose. The encoding holds this field **divided by four**, and a
/// descriptor at register zero would put a zero in the field either way - so a scale the layout
/// got wrong would be invisible here, which is the one thing a hand-assembly must not hide.
const IMAGE_DESCRIPTOR: u32 = 4;

/// First scalar register of the sampler descriptor, past the eight the image occupies.
const SAMPLER_DESCRIPTOR: u32 = 12;

/// The second `MIMG` word, from the register numbers as a disassembly writes them.
///
/// The two descriptors are stored divided by four, so doing that division here rather than at
/// the call site is what lets the caller name `s[4:11]` and `s[12:15]` as `4` and `12` - the
/// numbers in the instruction this file claims to assemble, checkable against it by eye.
/// Pre-divided literals would read as `1` and `3`, which match nothing written anywhere.
const fn mimg_operands(vaddr: u32, vdata: u32, resource: u32, sampler: u32) -> u32 {
    vaddr | (vdata << 8) | ((resource / 4) << 16) | ((sampler / 4) << 21)
}

/// `image_sample<opcode> v[4:7], v[0:1], s[4:11], s[12:15] dmask:0xf`, as its two words.
///
/// One function rather than a constant per opcode, so the decode assertion below and the shaders
/// that run are unambiguously the same instruction - two hand-assemblies that could drift apart
/// is exactly the thing a test like this must not have.
const fn image_sample_words(opcode: u32) -> [u32; 2] {
    [
        // 0xF0000000, the opcode at shift 18, dmask 0xf at shift 8, and the dimensionality.
        0xF000_0000 | (opcode << 18) | (0xF << 8) | TWO_DIMENSIONAL,
        mimg_operands(0, 4, IMAGE_DESCRIPTOR, SAMPLER_DESCRIPTOR),
    ]
}

/// The dimensionality field, saying this instruction's coordinate has two components.
///
/// A three-bit field at shift 3 of the first word, with two dimensions coded as one. **It has to
/// be set**: leaving it zero says one-dimensional, and the translation refuses that rather than
/// reading two coordinate registers for a coordinate that has one. Measured by assembling one
/// instruction at each of the eight dimensionalities and differencing (worklog 576).
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

/// **The hand-assembled instruction is the instruction it claims to be.**
///
/// Asserted before anything is translated, and it needs no device. A wrong hand-assembly would
/// otherwise reach the framebuffer and fail there, where a wrong picture has a dozen possible
/// causes and this has one.
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

        // The family comes back as an index into the encoding table, and the name is looked up
        // from that pair - the same route the translator's own dispatch takes, so a name that
        // resolves here is a name that will dispatch there.
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

/// **A translated texture sample reads the texel its coordinate names.**
///
/// # What this asserts
///
/// The same four-quadrant claim the hand-written oracle makes, over a frame a *guest's*
/// instruction words produced: the image is bound and read, the coordinate reaches the sample,
/// the rows are the right way up, and nothing is filtered.
///
/// And then the assertion that means the most: the translated frame is compared **byte for
/// byte** against the hand-written level-zero oracle over the same geometry and the same
/// texture. The two differ only in what produced the fragment shader, so a difference is the
/// translation and nothing else.
///
/// # What it cannot assert
///
/// **That the texture is the one the guest asked for.** It is the one the pipeline bound, and
/// D690 says why: the eight registers naming a descriptor describe a surface this backend has
/// never uploaded. What the translation does guarantee is that a module needing more than one
/// texture is refused rather than drawn - `a_second_texture_is_refused_rather_than_guessed`
/// below is that half.
#[test]
fn a_translated_sample_reads_the_texel_its_coordinate_names() {
    if !device_or_skip("a_translated_sample_reads_the_texel_its_coordinate_names") {
        return;
    }
    let size = (8, 8);
    // Zero, two and two, so the coordinate sweeps the whole texture once across the visible
    // frame - the triangle's corners are at (-1, -1), (3, -1) and (-1, 3), two of them outside
    // it. The same corners the oracle uses, for the same reason.
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
/// `v_mov_b32 v0, x` and `v_mov_b32 v1, y` put the texel's index in the coordinate registers,
/// then `image_load v[4:7], v[0:1], s[4:11] dmask:0xf` reads it. **Constant rather than
/// interpolated on purpose**: a fetch takes a texel's index, and the assertion this test wants
/// to make is "this texel, exactly" - an interpolated coordinate would make it "some texel,
/// depending on where the pixel is", which is a weaker claim about a harder thing.
///
/// `VOP1` is `0x7E000000` with the opcode at shift 9, the destination at shift 17 and the
/// source at shift 0; source `128 + k` is the inline constant `k`. `image_load` has **four**
/// operands rather than five - no sampler - so its mask sits where a sample's sampler does.
fn fetching_shader(x: u32, y: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (register, value) in [(0u32, x), (1, y)] {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | (128 + value);
        bytes.extend(word.to_le_bytes());
    }
    // 0xF0000000, dmask 0xf at shift 8, the dimensionality, and **no opcode term because
    // `image_load` is opcode zero** - the shift is written out in the sampling builder above,
    // where it is not.
    bytes.extend((0xF000_0000u32 | (0xF << 8) | TWO_DIMENSIONAL).to_le_bytes());
    // The coordinate at shift 0, the destination at shift 8, and the image descriptor at shift
    // 16 divided by four. No sampler field, which is the whole difference from a sample.
    bytes.extend(((4 << 8) | ((IMAGE_DESCRIPTOR / 4) << 16)).to_le_bytes());
    // exp mrt0 v4, v5, v6, v7
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// **A translated fetch reads the texel its index names.**
///
/// # Why this is not the sampling test again
///
/// A guest's `image_load` names an image descriptor and **no sampler**, because it reads a texel
/// by its integer index and there is nothing to filter, wrap or choose a level for. On the host
/// that is a fetch, and a fetch takes an image rather than an image and its sampler - so the
/// bound pair is unwrapped first. It needs no binding of its own, no device feature and no claim
/// about the texture's format, which is why it was worth doing before the store that does.
///
/// # What it asserts
///
/// Four draws, one per texel of the two-by-two texture, each asserting **every pixel** of the
/// frame is that texel. Four different indices give four different answers, so a fetch ignoring
/// its coordinate fails rather than passing by luck, and the two rows are told apart - which is
/// the claim about upload order a fetch makes as directly as a sample does.
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
    // The fetch ignores the varying; the vertex module is here to cover the frame.
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

/// **An image with a different number of dimensions is refused, not read as two.**
///
/// # Why this is the guard that was missing
///
/// Every image translation here reads **two** coordinate registers. That is right for a
/// two-dimensional image and wrong for every other kind: a three-dimensional coordinate is three
/// registers, and reading two of it would sample a place the guest never named - and draw a
/// frame that looks entirely plausible.
///
/// Nothing checked, for a reason worth keeping: the dimensionality is printed as a symbolic name
/// and the operand solver skips symbolic modifiers by design, so it never reached the operand
/// table and the translation could not have looked at it. It is in the instruction all the same,
/// and assembling one instruction at each of the eight dimensionalities put the field at three
/// bits from bit three, with every other byte identical (worklog 576).
///
/// This is that guard, watched failing. The instruction is the one the test above translates
/// with two dimensions, with three named instead.
#[test]
fn an_image_that_is_not_two_dimensional_is_refused() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    // Every code but two dimensions. They run 1D, 2D, 3D, cube, 1D array, 2D array,
    // 2D multi-sampled, 2D multi-sampled array - the order a disassembler prints them, and the
    // order the measured first bytes 0x00, 0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38 put them in.
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
/// Three moves fill the address registers - the coordinate and then **the level, last** - and
/// `image_sample_l v[4:7], v[0:2], s[4:11], s[12:15]` reads with them.
///
/// `VOP1` source `240` is the inline float one half and `242` is one; `128` is the integer zero,
/// whose bit pattern is also the float zero. So a coordinate at the middle of the texture and a
/// level of nought or one are four moves and no literal words.
fn levelled_shader(level: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut mov = |register: u32, source: u32| {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | source;
        bytes.extend(word.to_le_bytes());
    };
    // 0.5, 0.5 - the far quadrant of a two-by-two texture, and the middle of a one-by-one.
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

/// **A translated levelled sample reads the level it names.**
///
/// # Why this needed the harness to change
///
/// A texture with one level answers the same texel whatever level is asked for, so a test on
/// one would pass for a translation that dropped the level operand entirely - which is exactly
/// the claim this instruction rests on. The harness now builds two levels: the caller's texels,
/// and one coarse texel that is deliberately none of them.
///
/// # What it asserts
///
/// The same coordinate at two levels gives two different answers, each the right one. That is
/// the level operand reaching the instruction, which nothing else here could show.
///
/// # What it cannot assert
///
/// That the level is the **last** address element rather than some other one. With a
/// two-register coordinate and a one-register level there is only one arrangement that puts a
/// number in the level's place, so this passes for any translation that reads the third
/// register as the level. Where it sits was measured from a compiler instead (worklog 577).
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

/// **A module naming a second texture is refused, not guessed at.**
///
/// # Why this is the other half
///
/// One texture is bound. A translation that mapped every sample onto it regardless would draw a
/// plausible frame for a shader using two, which is the failure D104 refused for export targets
/// in exactly these words. D690 chose the refusal, and a refusal nobody has watched happen is a
/// refusal nobody knows anything about.
///
/// Two samples naming different image descriptors, and the second must not translate.
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
    // The same sample again, from s[20:27] instead - a second texture, by register number.
    // The field is replaced rather than merged into, so the register it names is exactly this.
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
    // Two textures bind (worklog 840), but only where each one's descriptor is found in the
    // descriptor table; neither is here, so which is which cannot be told.
    assert!(
        detail.contains("reads two textures"),
        "refused, but not for naming two textures: {detail}"
    );
}

/// **Two textures whose descriptors come from the descriptor table translate, each at its own
/// binding and offset** (worklog 840): the open-toolchain GL context's second texture unit loads
/// its image descriptor from `+0x40` of the table `s[0:1]` points at, where the first loads from
/// `+0x00` (oops-sdk `tex-prolog2.s`). Sampled second-unit first, as that prolog does, so slot 0
/// is the `+0x40` texture.
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

/// **A descriptor rewritten between two samples is refused too.**
///
/// # Why comparing register numbers is not enough on its own
///
/// A shader can load a second image descriptor into the *same* eight registers and sample
/// again. Both samples then name `s[0:7]`, so the check above sees one texture where there are
/// two - and the second sample would read the first one's texture and draw a plausible frame.
/// D690's third rule closes that: a write anywhere inside either group means the descriptor is
/// no longer the one the last sample used, and the next sample is refused.
///
/// This is the negative test for it. `s_mov_b32 s6, 0` lands in the middle of `s[4:11]`, which
/// is the case a check watching only the first register of the group would miss.
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
    // `s_mov_b32 s6, 0`: SOP1 is 0xBE800000 with the opcode at shift 8, the destination at
    // shift 16 and the source at shift 0. Source 128 is the inline constant zero.
    bytes.extend((0xBE80_0000u32 | (6 << 16) | (3 << 8) | 128).to_le_bytes());
    // The same texture, by register number, and no longer the same texture in fact.
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
