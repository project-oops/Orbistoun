//! A program's `FLOAT_MODE` decides its 32-bit subnormals, on the device.
//!
//! The field is bits 19:12 of `COMPUTE_PGM_RSRC1` (Mesa `src/amd/registers/gfx10.json:11880`), the
//! MODE register's low byte: 32-bit subnormals in bits 5:4, 16- and 64-bit in 7:6, each 0 to flush
//! them on input and output, 1 to keep them on input only, 2 on output only, 3 to keep them
//! (`src/amd/compiler/aco_ir.h:137-157`). A module declares the matching float control, so the
//! driver no longer chooses.
//!
//! The program subtracts the subnormal `0x40` from the subnormal `0x20` - the inline integers 64
//! and 32 read as floats - and leaves the difference in `v0`.

use orbistoun_gpu_vulkan::{Availability, dispatch, probe};
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::predicated::MEMORY_WORDS;
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Warning, Width, translate_with_user_data};

/// `v_mov_b32 v1, 64; v_mov_b32 v2, 32; v_sub_f32 v0, v2, v1; s_endpgm`.
const SUBTRACTION: [u32; 4] = [0x7e02_02c0, 0x7e04_02a0, 0x0800_0302, 0xbf81_0000];

/// Keeps 32-bit subnormals on input and output: `FP_32_DENORMS` (gfx10.json:300).
const KEEP: u8 = 0x30;
/// Flushes them on input and output.
const FLUSH: u8 = 0x00;
/// Keeps them on input only, which SPIR-V's float controls cannot say.
const KEEP_INPUT: u8 = 0x10;

/// `v_mov_b32 v0, 64; v_rcp_f32 v0, v0; s_endpgm`: the reciprocal of the subnormal `0x40`.
const RECIPROCAL: [u32; 3] = [0x7e00_02c0, 0x7e00_5500, 0xbf81_0000];

/// The program translated under `float_mode` by `fidelity`'s model: its `v0`, and its warnings.
fn subtract(float_mode: u8, fidelity: Fidelity) -> (u32, Vec<Warning>) {
    run(&SUBTRACTION, float_mode, fidelity)
}

/// `program` translated under `float_mode` by `fidelity`'s model: its `v0`, and its warnings.
fn run(program: &[u32], float_mode: u8, fidelity: Fidelity) -> (u32, Vec<Warning>) {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = program.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    let translated = translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity,
            width: Width::default(),
        },
        (Stage::Compute, MeshPrimitive::default()),
        Window::default(),
        UserData {
            float_mode: Some(float_mode),
            ..UserData::default()
        },
    )
    .expect("translates");
    let out = dispatch(&translated.module, 16, MEMORY_WORDS as usize, [1, 1, 1]).expect("runs");
    (out.observed[0], translated.warnings)
}

/// A program that keeps 32-bit subnormals subtracts them exactly; one that flushes them answers
/// zero, of either sign. Both models agree, which is what lets them be compared at all. One that
/// keeps them on input only is translated as flushing, its results being what it flushes, and
/// says so.
#[test]
fn a_subnormal_subtraction_follows_the_float_mode() {
    let Availability::Available { properties } = probe() else {
        println!("SKIPPED - no device");
        return;
    };
    if !properties.subnormals_preserved {
        println!(
            "SKIPPED keeping - {} cannot keep 32-bit subnormals on request",
            properties.device
        );
    }
    if !properties.subnormals_flushed {
        println!(
            "SKIPPED flushing - {} cannot flush 32-bit subnormals on request",
            properties.device
        );
    }
    for fidelity in [Fidelity::Lane, Fidelity::Wavefront] {
        if properties.subnormals_preserved {
            let (kept, warnings) = subtract(KEEP, fidelity);
            assert_eq!(kept, 0x8000_0020, "{fidelity:?}: 0x20 - 0x40, kept");
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        if properties.subnormals_flushed {
            let (flushed, _) = subtract(FLUSH, fidelity);
            assert_eq!(
                flushed & 0x7fff_ffff,
                0,
                "{fidelity:?}: flushed, {flushed:#x}"
            );
            let (closest, warnings) = subtract(KEEP_INPUT, fidelity);
            assert_eq!(
                closest & 0x7fff_ffff,
                0,
                "{fidelity:?}: flushed as the closest"
            );
            assert!(
                warnings.iter().any(|w| matches!(
                    w,
                    Warning::FloatMode {
                        float_mode: KEEP_INPUT
                    }
                )),
                "{warnings:?}"
            );
        }
    }
}

/// The reciprocal of a subnormal overflows to infinity whether the float mode keeps it (its true
/// reciprocal is beyond the largest float) or flushes it (the reciprocal of zero) - though SPIR-V's
/// division promises nothing for a divisor below 2^-126, which one driver answered with zero.
#[test]
fn a_subnormal_s_reciprocal_is_infinite_in_every_mode() {
    if !matches!(probe(), Availability::Available { .. }) {
        println!("SKIPPED - no device");
        return;
    }
    for fidelity in [Fidelity::Lane, Fidelity::Wavefront] {
        for mode in [KEEP, FLUSH] {
            let (reciprocal, _) = run(&RECIPROCAL, mode, fidelity);
            assert_eq!(reciprocal, 0x7f80_0000, "{fidelity:?} under {mode:#x}");
        }
    }
}
