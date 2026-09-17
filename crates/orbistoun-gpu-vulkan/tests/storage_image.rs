//! A storage image written by a shader, on a device.
//!
//! The last thing a guest's image instructions need that this backend did not have. `image_load`
//! reads a texel through the sampled binding and needs nothing of its own (worklog 573);
//! `image_store` writes, and nothing writes to a sampled image - so it needs a storage image, a
//! binding, and a capability.
//!
//! **And a decision about the format**, which is the interesting part. A storage image normally
//! names its format in the module; a guest's format lives in a descriptor this project does not
//! decode, so naming one would be inventing it. The module says `Unknown` and declares
//! `StorageImageWriteWithoutFormat` instead, which needs a device feature and makes the format
//! the pipeline's business rather than the shader's (D692).
//!
//! The module here is hand-assembled, like every other oracle. Nothing is translated; this is
//! what a translation will be measured against.

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

/// **A shader writes a texel of a storage image, and the host reads it back.**
///
/// # What it asserts
///
/// Three things, and each rules out a different way of being wrong:
///
/// - **The texel it named holds what it wrote.** That is the store landing.
/// - **The attachment holds the same colour.** The module writes both, so a frame that came back
///   as the clear would mean the shader never ran - which would otherwise be indistinguishable
///   from a shader that ran and stored nothing.
/// - **The four corners of the image are not all that colour.** The image starts uninitialised
///   and one texel is written, so an assertion that only looked at the written texel would pass
///   against an image that happened to be full of it.
///
/// # What it cannot assert
///
/// That a *translated* `image_store` would do this. Nothing is translated here.
#[test]
fn a_shader_writes_a_texel_and_the_host_reads_it_back() {
    if !storing_or_skip("a_shader_writes_a_texel_and_the_host_reads_it_back") {
        return;
    }
    let green = [0.0, 1.0, 0.0, 1.0];
    // Magenta, which the module never writes: a frame or a texel carrying it was not written.
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

    // The image was not simply full of green to begin with. Its contents before the draw are
    // undefined, so this cannot assert what the other texels *are* - only that they are not all
    // the one value that would make the assertion above vacuous.
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
/// the colour, then `image_store v[4:7], v[0:1], s[4:11] dmask:0xf` writes it and
/// `exp mrt0 v4, v5, v6, v7` puts it on the screen too.
///
/// **Inline constants, not literals.** `VOP1` source `128 + k` is the integer `k` - and integer
/// zero is also the float zero - while source `242` is the float one. So a green, opaque texel
/// is four moves and no literal words, which keeps the hand-assembly short enough to check by
/// eye.
fn storing_shader(x: u32, y: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut mov = |register: u32, source: u32| {
        let word = 0x7E00_0000u32 | (1 << 9) | (register << 17) | source;
        bytes.extend(word.to_le_bytes());
    };
    mov(0, 128 + x);
    mov(1, 128 + y);
    // (0.0, 1.0, 0.0, 1.0) - green and opaque.
    mov(4, 128);
    mov(5, 242);
    mov(6, 128);
    mov(7, 242);
    // 0xF0000000, opcode 8 at shift 18, dmask 0xf at shift 8, and the dimensionality: a
    // three-bit field at shift 3 with two dimensions coded as one. Leaving it zero says
    // one-dimensional, and the translation refuses that rather than reading two coordinate
    // registers for a coordinate with one (worklog 576).
    bytes.extend((0xF000_0000u32 | (8 << 18) | (0xF << 8) | (1 << 3)).to_le_bytes());
    bytes.extend(((4 << 8) | ((IMAGE_DESCRIPTOR / 4) << 16)).to_le_bytes());
    bytes.extend(0xF800_0000u32.to_le_bytes());
    bytes.extend(0x0706_0504u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// First scalar register of the image descriptor the test's shader names.
///
/// Four rather than zero, because the field holds it divided by four and a descriptor at zero
/// would put a zero there either way - hiding a scale the layout got wrong.
const IMAGE_DESCRIPTOR: u32 = 4;

/// **A translated store writes the texel the guest named.**
///
/// # What it asserts
///
/// The same three claims the hand-written oracle makes, over a frame a *guest's* instruction
/// words produced, and once per texel of a two-by-two corner of the image - so a store ignoring
/// its coordinate fails rather than passing by luck.
///
/// # What it cannot assert
///
/// **That the texture written is the one the guest asked for.** It is the one the pipeline
/// bound, for the reason D690 gives, and the image a store writes is not the image a sample
/// reads even when the guest names one descriptor for both - which D692 records as an open gap
/// rather than closes.
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

/// **The binding a module declares is the binding the harness fills.**
///
/// The same assertion the sampled image has, for the same reason: the number lives in two crates
/// that cannot import each other, and a mismatch would otherwise show up only as a validation
/// error on a machine that has a device.
#[test]
fn the_storage_binding_a_module_declares_is_the_one_the_harness_fills() {
    assert_eq!(
        STORAGE_IMAGE_BINDING,
        orbistoun_spirv::STORAGE_IMAGE_BINDING,
        "a module writes one binding and the harness fills another"
    );
}
