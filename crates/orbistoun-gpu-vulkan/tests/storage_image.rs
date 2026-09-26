//! A storage image written by a shader, on a device.
//!
//! `image_load` reads a texel through the sampled binding; `image_store` writes, which needs a
//! storage image, a binding and a capability. A guest's format lives in a descriptor this project
//! does not decode, so the module declares `Unknown` and `StorageImageWriteWithoutFormat`, which
//! needs a device feature and makes the format the pipeline's business (D692). The oracle module is
//! hand-assembled.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{STORAGE_IMAGE_BINDING, draw_storing};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::{fullscreen_triangle_vertex_module, storing_fragment_module};
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{Stage, Window, translate_for};

/// Whether the device can write a format-less storage image, said aloud when it cannot.
fn storing_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } if properties.storage_image_write => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Available { properties } => {
            println!(
                concat!(
                    "[{}] SKIPPED - {} cannot write a storage image without a declared format, ",
                    "so a guest's image store cannot run here"
                ),
                what, properties.device
            );
            false
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// A shader writes a texel of a storage image, and the host reads it back.
///
/// - The texel it named holds what it wrote: the store landed.
/// - The attachment holds the same colour, so the shader ran; a frame showing the clear would
///   otherwise look like a shader that ran and stored nothing.
/// - The four corners of the image are not all that colour, so an image that happened to be full of
///   it does not pass.
#[test]
fn a_shader_writes_a_texel_and_the_host_reads_it_back() {
    if !storing_or_skip("a_shader_writes_a_texel_and_the_host_reads_it_back") {
        return;
    }
    let green = [0.0, 1.0, 0.0, 1.0];
    // Magenta, which the module never writes.
    let clear = [1.0, 0.0, 1.0, 1.0];
    let at = [1u32, 0];

    let drawn = draw_storing(
        &fullscreen_triangle_vertex_module(),
        &storing_fragment_module(at, green),
        clear,
        (8, 8),
    )
    .expect("the storing draw ran");

    assert_eq!(
        drawn.stored.at(at[0], at[1]),
        Some([0, 255, 0, 255]),
        "the texel the shader named does not hold what it wrote"
    );
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                drawn.pixels.at(x, y),
                Some([0, 255, 0, 255]),
                "pixel ({x}, {y}) - magenta is the clear, so the shader never ran"
            );
        }
    }

    // The image's contents before the draw are undefined, so this asserts only that the other
    // texels are not all the value that would make the assertion above vacuous.
    let corners = [(0, 0), (7, 0), (0, 7), (7, 7)];
    let untouched: Vec<_> = corners
        .into_iter()
        .filter_map(|(x, y)| drawn.stored.at(x, y))
        .collect();
    assert!(
        untouched.iter().any(|texel| *texel != [0, 255, 0, 255]),
        concat!(
            "every corner of the storage image is the colour the shader wrote to one texel, so ",
            "the texel assertion above proves nothing: {:?}"
        ),
        untouched
    );
}

/// A guest shader that stores one texel and exports the same colour.
///
/// `v_mov_b32` fills the coordinate registers with the texel's index and the data registers with
/// the colour, then `image_store v[4:7], v[0:1], s[4:11] dmask:0xf` writes it and `exp mrt0 v4, v5,
/// v6, v7` puts it on the screen. Inline constants, not literals: `VOP1` source `128 + k` is the
/// integer `k` (and integer zero is float zero), while source `242` is the float one.
fn storing_shader(x: u32, y: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut mov = |register: u32, source: u32| {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | source;
        bytes.extend(word.to_le_bytes());
    };
    mov(0, 128 + x);
    mov(1, 128 + y);
    // (0.0, 1.0, 0.0, 1.0): green and opaque.
    mov(4, 128);
    mov(5, 242);
    mov(6, 128);
    mov(7, 242);
    // 0xF0000000, opcode 8 at shift 18, dmask 0xf at shift 8, and the dimensionality: a three-bit
    // field at shift 3, with two dimensions coded as one. Zero would mean one-dimensional, which
    // the translation refuses for a store with two coordinate registers.
    bytes.extend((0xF000_0000u32 | (8 << 18) | (0xF << 8) | (1 << 3)).to_le_bytes());
    bytes.extend(((4 << 8) | ((IMAGE_DESCRIPTOR / 4) << 16)).to_le_bytes());
    // `exp mrt0` with all four channels enabled (`EN`, bits 0-3): an export that enables nothing
    // writes nothing.
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// First scalar register of the image descriptor the test's shader names. Four rather than zero,
/// because the field holds it divided by four and zero would hide a wrong scale.
const IMAGE_DESCRIPTOR: u32 = 4;

/// A translated store writes the texel the guest named.
///
/// The oracle's three claims, over a frame a guest's instruction words produced, once per texel of
/// a two-by-two corner, so a store ignoring its coordinate fails. The image written is the one the
/// pipeline bound, and a store's image is not a sample's image even when the guest names one
/// descriptor for both (D692).
#[test]
fn a_translated_store_writes_the_texel_the_guest_named() {
    if !storing_or_skip("a_translated_store_writes_the_texel_the_guest_named") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let clear = [1.0, 0.0, 1.0, 1.0];
    let vertex = fullscreen_triangle_vertex_module();

    for y in 0..2u32 {
        for x in 0..2u32 {
            let decoded = decode(&storing_shader(x, y), &encodings, &operands);
            let module = translate_for(
                &decoded,
                &encodings,
                Width::Wave64,
                Stage::Fragment,
                Window::default(),
            )
            .map(|(module, _)| module)
            .expect("the image store translated");

            let drawn =
                draw_storing(&vertex, &module, clear, (8, 8)).expect("the translated draw ran");
            assert_eq!(
                drawn.stored.at(x, y),
                Some([0, 255, 0, 255]),
                "texel ({x}, {y}) does not hold what the translated store wrote"
            );
            assert_eq!(
                drawn.pixels.at(0, 0),
                Some([0, 255, 0, 255]),
                "the attachment is the clear, so the translated shader never ran"
            );
        }
    }
}

/// The binding a module declares is the binding the harness fills: the number lives in two crates
/// that cannot import each other.
#[test]
fn the_storage_binding_a_module_declares_is_the_one_the_harness_fills() {
    assert_eq!(
        STORAGE_IMAGE_BINDING,
        orbistoun_spirv::STORAGE_IMAGE_BINDING,
        "a module writes one binding and the harness fills another"
    );
}

/// A mesh module reads its draw data where, and in the shape, the harness writes it (D718): the
/// binding, the words per draw and the draws per dispatch live in both crates, and a mismatch would
/// hand a workgroup another draw's words.
#[test]
fn the_draw_data_a_module_reads_is_laid_out_as_the_harness_writes_it() {
    use orbistoun_gpu_vulkan::framebuffer::{
        DRAW_DATA_BINDING, DRAW_DATA_MOST_DRAWS, DRAW_DATA_STRIDE_WORDS,
    };
    assert_eq!(DRAW_DATA_BINDING, orbistoun_spirv::DRAW_DATA_BINDING);
    assert_eq!(
        DRAW_DATA_STRIDE_WORDS,
        orbistoun_spirv::DRAW_DATA_STRIDE_WORDS
    );
    assert_eq!(DRAW_DATA_MOST_DRAWS, orbistoun_spirv::DRAW_DATA_MOST_DRAWS);
}
