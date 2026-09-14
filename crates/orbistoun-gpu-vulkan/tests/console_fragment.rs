//! The console's own pixel shader, on a device.
//!
//! `orbistoun-gpu`'s oracle test translates it and `spirv-val` accepts the module. Valid and
//! *runnable* are different claims - a module can satisfy every structural rule and still ask
//! for a feature the device was not created with, or for a descriptor nothing bound - and this
//! crate is where the difference is measured, the same way it was for the hand-written modules
//! (D549, D550).
//!
//! The shader is the GL cube's untextured pixel shader, read from the payload image captured
//! beside the command stream in oracle record A: `crates/orbistoun-gpu/tests/captures`,
//! payload offset `0x300`. Nothing here re-translates anything the other test does not; it
//! takes the same bytes to the same translator and then keeps going.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_spirv::interpolated_vertex_module;
use orbistoun_translate::wavefront::Stage;
use orbistoun_translate::{Fidelity, Strategy, Width, translate_staged};

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

/// Where the untextured pixel shader sits inside the captured payload image.
const FRAGMENT_OFFSET: usize = 0x300;

/// The payload image from oracle record A, as words.
///
/// Read out of the other crate's capture directory rather than copied here. A second copy of
/// a capture is a second thing to keep true, and the whole point of a capture is that there is
/// one of it.
fn payload() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-gpu")
        .join("tests")
        .join("captures")
        .join("agc-gl-cube-fw1240-a.payload.hex");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bytes = Vec::new();
    for line in text.lines() {
        for word in line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
        {
            let value = u32::from_str_radix(word, 16)
                .unwrap_or_else(|e| panic!("{}: {word:?}: {e}", path.display()));
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// The console's pixel shader, translated for the fragment stage.
fn console_fragment_module() -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    // `decode_program`, not `decode`: a shader carries no length and the payload image holds
    // more than this shader, so the terminator is what says where it ends - which is exactly
    // what the submission pipeline does when it reads one out of guest memory.
    let decoded = decode_program(&payload[FRAGMENT_OFFSET..], &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    translate_staged(&decoded, &encodings, strategy, Stage::Fragment)
        .expect("the console's pixel shader translates")
        .module
}

/// **A shader a console ran, translated here, draws on a device.**
///
/// # What it asserts
///
/// The shader interpolates attribute zero and exports it, so with all three corners the same
/// colour every pixel is that colour exactly - no sample position and no rounding enters into
/// it. The clear is a colour the shader never writes, so a draw that produced nothing reads as
/// a different answer rather than an absent one.
///
/// # What it cannot assert
///
/// **That these are the console's pixels.** The console drew this shader over its own vertex
/// program, which does not translate yet (D688), so the geometry here is the oracle's
/// hand-written triangle. This says the shader runs and computes with what it is given; the
/// frame hash in the oracle record is a different claim and needs the other half.
///
/// # Validation
///
/// Run it under the Khronos validation layer with `tools/validate-device.sh`. That is not a
/// detail: before the descriptor set and the device features arrived, this drew the right
/// colour *and* produced five validation errors, so the picture was the driver being tolerant
/// rather than a module that was correct (worklog 554).
#[test]
fn the_consoles_pixel_shader_draws_on_a_device() {
    if !device_or_skip("the_consoles_pixel_shader_draws_on_a_device") {
        return;
    }
    let module = console_fragment_module();
    println!("module: {} words", module.len());

    let green = [0.0, 1.0, 0.0, 1.0];
    let red = [1.0, 0.0, 0.0, 1.0];
    let vertex = interpolated_vertex_module([green; 3]);
    let pixels = draw_with(&vertex, &module, red, 8, 5).expect("the console's pixel shader drew");

    for y in 0..5 {
        for x in 0..8 {
            assert_eq!(
                pixels.at(x, y),
                Some([0, 255, 0, 255]),
                "pixel ({x}, {y}) - red here is the clear, which means the shader wrote nothing"
            );
        }
    }
}
