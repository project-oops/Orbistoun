//! The SDWA and DPP operand modifiers on any instruction that has them (D979), held to the
//! semantics each field is published with (AMD's published RDNA 2 instruction set; LLVM's
//! `AMDGPUModifierSyntax`), written out again here on the host.
//!
//! Each program runs one wave of sixty-four lanes, each its lane id in `v0`, from data every lane
//! holds differently. Its results are checked three ways: on the host evaluator, which runs the
//! translator's own semantics over numbers (D753), and on the device both ways a dispatch runs -
//! the whole wave in one invocation, and one invocation per lane, another lane's value then the
//! host subgroup's shuffle (D880, D973).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_gpu_vulkan::dispatch_guest;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_translate::evaluate::{LaneProgram, WaveEntry, wave_trace};
use orbistoun_translate::wavefront::{ComputeInputs, MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_with_user_data};

const WINDOW: u64 = 0x4_0187_0000;
const WINDOW_WORDS: u32 = 4096;
/// What memory held before, so an unwritten word is told from a written one.
const BEFORE: u32 = 0xdead_beef;
/// The output descriptor's fourth word: `OOB_SELECT` 3, raw.
const WORD3: u32 = 0x3101_6fac;
const LITERAL: u32 = 255;
/// The modifier markers in a short form's first source field.
const SDWA: u32 = 0xf9;
const DPP16: u32 = 0xfa;
const DPP8: u32 = 0xe9;
const DPP8_FI: u32 = 0xea;
/// The SDWA selects.
const BYTE_0: u32 = 0;
const BYTE_1: u32 = 1;
const BYTE_2: u32 = 2;
const BYTE_3: u32 = 3;
const WORD_0: u32 = 4;
const WORD_1: u32 = 5;
const DWORD: u32 = 6;
/// `dst_unused`.
const PAD: u32 = 0;
const SEXT: u32 = 1;
const PRESERVE: u32 = 2;
/// Scalar codes: `vcc` and `exec`'s halves.
const VCC_LO: u32 = 106;
const VCC_HI: u32 = 107;
const EXEC_LO: u32 = 126;
const EXEC_HI: u32 = 127;
/// The inline float `-4.0`.
const MINUS_FOUR: u32 = 247;

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

/// The two ways a dispatch runs: the whole wave in one invocation, and one invocation per lane.
const MODELS: [(&str, bool); 2] = [("whole wave", false), ("per lane", true)];

/// The source code of an inline integer.
const fn inline(value: u32) -> u32 {
    128 + value
}

/// A vector register as a source.
const fn vgpr(register: u32) -> u32 {
    256 + register
}

/// A word of the encoding table's family for `name`, its opcode placed, with `fields` ored in.
fn encoded(name: &str, fields: u32) -> u32 {
    let encodings = EncodingTable::builtin().expect("encodings");
    let (family, opcode) = encodings.find_by_name(name).expect("this target has it");
    let encoding = encodings
        .encodings()
        .iter()
        .find(|e| e.name == family)
        .expect("the family it was found in");
    encoding.value | (opcode << encoding.opcode.shift) | fields
}

/// `name vDst, <source code>`, a one-source short form.
fn vop1(name: &str, dst: u32, source: u32) -> u32 {
    encoded(name, source | (dst << 17))
}

/// `name vDst, <source code>, vSrc1`, a two-source short form.
fn vop2(name: &str, dst: u32, source: u32, vsrc1: u32) -> u32 {
    encoded(name, source | (vsrc1 << 9) | (dst << 17))
}

/// `s_mov_b32 sDst, <source code>`.
fn s_mov(dst: u32, source: u32) -> u32 {
    encoded("s_mov_b32", (dst << 16) | source)
}

/// An SDWA modifier word's fields.
#[derive(Clone, Copy, Default)]
struct Sdwa {
    /// The first source's code, and whether it is in the scalar space.
    src0: (u32, bool),
    /// Each source's select and sign extension; `(0, false)` stands for the default `DWORD`.
    sel: [(u32, bool); 2],
    /// The destination select and fill; `(0, 0)` stands for `DWORD`.
    dst: (u32, u32),
    /// Whether the second source's field names a scalar register.
    src1_scalar: bool,
    clamp: bool,
    omod: u32,
    /// Each source's negate and absolute value.
    neg: [bool; 2],
    abs: [bool; 2],
}

impl Sdwa {
    fn on(src0: u32) -> Self {
        Self {
            src0: (src0, false),
            sel: [(DWORD, false); 2],
            dst: (DWORD, PAD),
            ..Self::default()
        }
    }

    fn word(self) -> u32 {
        self.src0.0
            | self.dst.0 << 8
            | self.dst.1 << 11
            | u32::from(self.clamp) << 13
            | self.omod << 14
            | self.sel[0].0 << 16
            | u32::from(self.sel[0].1) << 19
            | u32::from(self.neg[0]) << 20
            | u32::from(self.abs[0]) << 21
            | u32::from(self.src0.1) << 23
            | self.sel[1].0 << 24
            | u32::from(self.sel[1].1) << 27
            | u32::from(self.neg[1]) << 28
            | u32::from(self.abs[1]) << 29
            | u32::from(self.src1_scalar) << 31
    }
}

/// The part of `value` an SDWA select names, zero- or sign-extended.
fn select(value: u32, (sel, sext): (u32, bool)) -> u32 {
    let (bits, shift) = match sel {
        0..=3 => (8, 8 * sel),
        4 | 5 => (16, 16 * (sel - 4)),
        _ => return value,
    };
    let field = (value >> shift) & ((1 << bits) - 1);
    if sext {
        sign_extended(field, bits)
    } else {
        field
    }
}

/// A word read as a signed integer.
const fn signed(value: u32) -> i32 {
    i32::from_ne_bytes(value.to_ne_bytes())
}

/// A `bits`-wide field sign-extended to 32 bits.
const fn sign_extended(field: u32, bits: u32) -> u32 {
    let top = 1 << (bits - 1);
    (field ^ top).wrapping_sub(top)
}

/// `result` placed into the part of `old` an SDWA destination select names, the rest zero, the
/// part's sign above it with zero below, or `old`'s bits.
fn place(result: u32, old: u32, (sel, unused): (u32, u32)) -> u32 {
    let (bits, shift) = match sel {
        0..=3 => (8, 8 * sel),
        4 | 5 => (16, 16 * (sel - 4)),
        _ => return result,
    };
    let mask = (1_u32 << bits) - 1;
    let field = result & mask;
    match unused {
        PAD => field << shift,
        SEXT => sign_extended(field, bits) << shift,
        _ => (old & !(mask << shift)) | (field << shift),
    }
}

/// The bits of a half holding `value`, which every value here is exactly: zero, or a normal.
fn half(value: f32) -> u32 {
    if value == 0.0 {
        return u32::from(value.is_sign_negative()) << 15;
    }
    let bits = value.to_bits();
    // A float's biased exponent of 113 is a half's 1, its smallest normal one, and 142 its 30.
    let exponent = (bits >> 23) & 0xff;
    assert!((113..143).contains(&exponent), "{value} is a normal half");
    assert_eq!(bits & 0x1fff, 0, "{value} is exact in a half");
    ((bits >> 16) & 0x8000) | ((exponent - 112) << 10) | ((bits >> 13) & 0x3ff)
}

/// The value of half bits `bits`, a normal or zero.
fn from_half(bits: u32) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::try_from((bits >> 10) & 0x1f).expect("five bits");
    let mantissa = f32::from(u16::try_from(bits & 0x3ff).expect("ten bits"));
    if exponent == 0 {
        return sign * mantissa * 2f32.powi(-24);
    }
    sign * (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
}

/// A lane's number as a float.
fn lane_float(lane: u32) -> f32 {
    f32::from(u16::try_from(lane).expect("a lane"))
}

/// The data every program starts from, by lane: `v1` and `v2` scattered bit patterns, `v3` the old
/// destination, `v20` the lane as a float, `v21` a quarter of it less four, and `v22` the halves of
/// `v20` (low) and `v21` (high).
fn data(lane: u32) -> [u32; 6] {
    let v20 = lane_float(lane);
    let v21 = v20 * 0.25 - 4.0;
    [
        lane.wrapping_mul(0x009e_3779) ^ 0x8bad_f00d,
        lane.wrapping_mul(0x007f_4a7c) ^ 0x1357_9bdf,
        lane ^ 0xa5a5_5a5a,
        v20.to_bits(),
        v21.to_bits(),
        half(v21) << 16 | half(v20),
    ]
}

/// The program that computes [`data`].
fn setup() -> Vec<u32> {
    vec![
        vop2("v_mul_u32_u24_e32", 1, LITERAL, 0),
        0x009e_3779,
        vop2("v_xor_b32_e32", 1, LITERAL, 1),
        0x8bad_f00d,
        vop2("v_mul_u32_u24_e32", 2, LITERAL, 0),
        0x007f_4a7c,
        vop2("v_xor_b32_e32", 2, LITERAL, 2),
        0x1357_9bdf,
        vop2("v_xor_b32_e32", 3, LITERAL, 0),
        0xa5a5_5a5a,
        vop1("v_cvt_f32_u32_e32", 20, vgpr(0)),
        vop2("v_mul_f32_e32", 21, LITERAL, 20),
        0.25_f32.to_bits(),
        vop2("v_add_f32_e32", 21, MINUS_FOUR, 21),
        vop1("v_mov_b32_e32", 22, inline(0)),
        vop1("v_mov_b32_e32", 23, inline(0)),
        vop1("v_cvt_f16_f32_e32", 22, vgpr(20)),
        vop1("v_cvt_f16_f32_e32", 23, vgpr(21)),
        vop2("v_lshlrev_b32_e32", 23, inline(16), 23),
        vop2("v_or_b32_e32", 22, vgpr(23), 22),
    ]
}

/// `v_mov_b32 vD, v3`: a destination holding the old bits every lane's [`data`] gives it.
fn old(dst: u32) -> u32 {
    vop1("v_mov_b32_e32", dst, vgpr(3))
}

/// `s_mov_b64 exec, -1`.
fn all_lanes() -> Vec<u32> {
    vec![s_mov(EXEC_LO, 193), s_mov(EXEC_HI, 193)]
}

/// The window word lane `lane`'s result `index` is stored at.
const fn slot(index: usize, lane: u32) -> usize {
    index * 64 + lane as usize
}

/// Stores each of `results` into the window, 64 words a result, a word a lane; then the end.
fn stores(results: &[u32]) -> Vec<u32> {
    let mut program = all_lanes();
    program.push(vop2("v_lshlrev_b32_e32", 8, inline(2), 0));
    for (index, &register) in results.iter().enumerate() {
        let offset = u32::try_from(index * 256).expect("small");
        program.extend([vop2("v_add_nc_u32_e32", 9, LITERAL, 8), offset]);
        // buffer_store_dword vR, v9, s[8:11], 0 offen
        program.extend([
            encoded("buffer_store_dword", 1 << 12),
            9 | (register << 8) | (2 << 16) | (inline(0) << 24),
        ]);
    }
    program.push(encoded("s_endpgm", 0));
    program
}

fn decoded(words: &[u32]) -> orbistoun_shader::Decode {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let decoded = decode(&bytes, &encodings, &operands);
    assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
    decoded
}

/// `body`'s results on the host evaluator: each of `results` in each lane.
fn evaluated(body: &[u32], results: &[u32]) -> Vec<Vec<Option<u32>>> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let mut words = body.to_vec();
    words.push(encoded("s_endpgm", 0));
    let program = LaneProgram::new(decoded(&words), &encodings, &[]);
    let wave = WaveEntry {
        group_ids: Vec::new(),
        threads: (0..64).map(|lane| Some([lane, 0, 0])).collect(),
        components: 1,
    };
    let vectors = results.iter().max().map_or(1, |r| r + 1);
    let run = wave_trace(
        &program,
        &encodings,
        &[],
        (64, &[wave]),
        (vectors, 1, 0),
        &mut |_| None,
    );
    assert!(
        matches!(run.stop, orbistoun_translate::evaluate::TraceStop::Ended),
        "the evaluator ran to the end: {:?}",
        run.stop
    );
    (0..64)
        .map(|lane| {
            results
                .iter()
                .map(|&register| run.lanes[lane][register as usize])
                .collect()
        })
        .collect()
}

/// `body` and the stores of `results`, translated for one group of sixty-four, one invocation per
/// lane where `per_lane`, and run over a window that held [`BEFORE`].
fn dispatched(body: &[u32], results: &[u32], per_lane: bool, dx10_clamp: bool) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let mut words = body.to_vec();
    words.extend(stores(results));
    let subgroup = match probe() {
        Availability::Available { properties } => properties.subgroup_size,
        Availability::Unavailable { .. } => 0,
    };
    // As the pipeline translates a dispatch: left to choose, which must run it one invocation per
    // lane on the host's subgroup, or asked for the whole wave by name.
    let asked_for = if per_lane {
        Fidelity::Auto
    } else {
        Fidelity::Wavefront
    };
    let translated = translate_with_user_data(
        &decoded(&words),
        &encodings,
        Strategy::Predicated {
            fidelity: asked_for,
            width: Width::Wave64,
        },
        (Stage::Compute, MeshPrimitive::default()),
        Window::spanning_address(WINDOW, WINDOW_WORDS).expect("a window"),
        UserData {
            count: 12,
            compute: Some(ComputeInputs {
                workgroup_ids: [true, false, false],
                thread_id_components: 1,
                threads: [64, 1, 1],
                ..ComputeInputs::default()
            }),
            per_invocation: Some(subgroup),
            dx10_clamp: Some(dx10_clamp),
            ..UserData::default()
        },
    )
    .expect("the program translates");
    let expected = if per_lane {
        Fidelity::Lane
    } else {
        Fidelity::Wavefront
    };
    assert_eq!(
        translated.fidelity, expected,
        "the model asked for: {:?}",
        translated.warnings
    );
    let module = translated.module;
    let mut block = vec![0_u32; 32];
    block[8] = WINDOW as u32;
    block[9] = (WINDOW >> 32) as u32 & 0xffff;
    block[10] = WINDOW_WORDS * 4;
    block[11] = WORD3;
    dispatch_guest(
        &module,
        &vec![BEFORE; WINDOW_WORDS as usize],
        (&block, [1, 1, 1]),
        &DispatchImages::default(),
    )
    .expect("dispatched, not the device lost")
    .memory
}

/// Holds `body`'s `results` to `expected(lane, index)` on the evaluator (unless `device_only`) and
/// on the device both ways a dispatch runs.
fn check(
    what: &str,
    (body, results): (&[u32], &[u32]),
    device_only: bool,
    expected: impl Fn(u32, usize) -> u32,
) {
    if !device_only {
        let lanes = evaluated(body, results);
        for (lane, values) in (0..64_u32).zip(&lanes) {
            for (index, value) in values.iter().enumerate() {
                assert_eq!(
                    *value,
                    Some(expected(lane, index)),
                    "{what}, evaluator: result {index} (v{}), lane {lane}",
                    results[index]
                );
            }
        }
    }
    if !device_or_skip(what) {
        return;
    }
    for (model, per_lane) in MODELS {
        let memory = dispatched(body, results, per_lane, true);
        for lane in 0..64_u32 {
            for index in 0..results.len() {
                assert_eq!(
                    memory[slot(index, lane)],
                    expected(lane, index),
                    "{what}, {model}: result {index} (v{}), lane {lane}",
                    results[index]
                );
            }
        }
    }
}

/// The first register a test's results go in.
const FIRST_RESULT: u32 = 24;

/// Each source select, zero- and sign-extended, of each source of an integer add; then of the
/// instruction Grand Theft Auto V's ray-tracing programs refused, `v_min_u32`, and a sign-extended
/// select of an arithmetic shift's amount and value.
#[test]
fn sdwa_source_selects_extend_as_published() {
    let selects: Vec<(u32, bool)> = (0..=6)
        .flat_map(|sel| [(sel, false), (sel, true)])
        .collect();
    let mut cases: Vec<(&str, Sdwa)> = Vec::new();
    for &sel in &selects {
        cases.push((
            "v_add_nc_u32_e32",
            Sdwa {
                sel: [sel, (DWORD, false)],
                ..Sdwa::on(1)
            },
        ));
        cases.push((
            "v_min_u32_e32",
            Sdwa {
                sel: [(WORD_1, false), sel],
                ..Sdwa::on(1)
            },
        ));
        cases.push((
            "v_min_i32_e32",
            Sdwa {
                sel: [sel, (BYTE_2, true)],
                ..Sdwa::on(1)
            },
        ));
    }
    let mut body = setup();
    let mut results = Vec::new();
    for (index, (name, sdwa)) in cases.iter().enumerate() {
        let dst = FIRST_RESULT + index as u32;
        body.extend([old(dst), vop2(name, dst, SDWA, 2), sdwa.word()]);
        results.push(dst);
    }
    check(
        "SDWA source selects",
        (&body, &results),
        false,
        |lane, index| {
            let [v1, v2, ..] = data(lane);
            let (name, sdwa) = cases[index];
            let (a, b) = (select(v1, sdwa.sel[0]), select(v2, sdwa.sel[1]));
            match name {
                "v_add_nc_u32_e32" => a.wrapping_add(b),
                "v_min_u32_e32" => a.min(b),
                _ => u32::from_ne_bytes(signed(a).min(signed(b)).to_ne_bytes()),
            }
        },
    );
}

/// Each destination select with each fill of the bits it leaves: zero, the part's sign above it,
/// and the destination's old bits; over a two-source integer instruction, a one-source one and a
/// carry-in add, whose carry the condition mask gives.
#[test]
fn sdwa_destination_selects_place_as_published() {
    let mut cases: Vec<(&str, (u32, u32))> = Vec::new();
    for dst_sel in [BYTE_0, BYTE_1, BYTE_2, BYTE_3, WORD_0, WORD_1, DWORD] {
        for unused in [PAD, SEXT, PRESERVE] {
            cases.push(("v_sub_nc_u32_e32", (dst_sel, unused)));
            cases.push(("v_not_b32_e32", (dst_sel, unused)));
        }
    }
    cases.push(("v_add_co_ci_u32_e32", (BYTE_1, SEXT)));
    cases.push(("v_add_co_ci_u32_e32", (WORD_1, PRESERVE)));
    let mut body = setup();
    let carried = |lane: u32| (0x4924_9249_9249_2492_u64 >> lane) & 1 == 1;
    let mut results = Vec::new();
    for (index, (name, dst)) in cases.iter().enumerate() {
        let at = FIRST_RESULT + index as u32;
        let sdwa = Sdwa {
            dst: *dst,
            ..Sdwa::on(1)
        }
        .word();
        body.push(old(at));
        if name.contains("_co_") {
            // vcc: every third lane carries in, set again before each carry writes it.
            body.extend([
                s_mov(VCC_LO, LITERAL),
                0x9249_2492,
                s_mov(VCC_HI, LITERAL),
                0x4924_9249,
            ]);
        }
        if *name == "v_not_b32_e32" {
            body.extend([vop1(name, at, SDWA), sdwa]);
        } else {
            body.extend([vop2(name, at, SDWA, 2), sdwa]);
        }
        results.push(at);
    }
    check(
        "SDWA destination selects",
        (&body, &results),
        false,
        |lane, index| {
            let [v1, v2, v3, ..] = data(lane);
            let (name, dst) = cases[index];
            let result = match name {
                "v_sub_nc_u32_e32" => v1.wrapping_sub(v2),
                "v_not_b32_e32" => !v1,
                _ => v1.wrapping_add(v2).wrapping_add(u32::from(carried(lane))),
            };
            place(result, v3, dst)
        },
    );
}

/// Grand Theft Auto V's ray-tracing programs as they were refused in run 344, their words as the
/// guest holds them: `v_min_u32_sdwa v8, v43, v43 src0_sel:WORD_0 src1_sel:WORD_1` at
/// `0x74005b854a00`, `v_min_u32_sdwa v6, v39, v39` the same at `0x74005b857200`, and
/// `v_mul_u32_u24_sdwa v5, 3, v5 src1_sel:WORD_0` at `0x74005b858800`.
#[test]
fn the_ray_tracing_programs_sdwa_words_compute_as_published() {
    let mut body = setup();
    body.extend([
        vop1("v_mov_b32_e32", 43, vgpr(1)),
        vop1("v_mov_b32_e32", 39, vgpr(2)),
        vop1("v_mov_b32_e32", 5, vgpr(3)),
        0x2610_56f9,
        0x0504_062b,
        0x260c_4ef9,
        0x0504_0627,
        0x160a_0af9,
        0x0486_0683,
        // Out of the registers the stores address through.
        vop1("v_mov_b32_e32", 24, vgpr(8)),
        vop1("v_mov_b32_e32", 25, vgpr(6)),
        vop1("v_mov_b32_e32", 26, vgpr(5)),
    ]);
    check(
        "the ray-tracing programs' SDWA words",
        (&body, &[24, 25, 26]),
        false,
        |lane, index| {
            let [v1, v2, v3, ..] = data(lane);
            match index {
                0 => (v1 & 0xffff).min(v1 >> 16),
                1 => (v2 & 0xffff).min(v2 >> 16),
                _ => 3 * (v3 & 0xffff),
            }
        },
    );
}

/// `v20 * v21` of [`data`], each under its negate and absolute value.
fn product([.., v20, v21, _]: [u32; 6], neg: [bool; 2], abs: [bool; 2]) -> f32 {
    let (mut a, mut b) = (f32::from_bits(v20), f32::from_bits(v21));
    if abs[0] {
        a = a.abs();
    }
    if neg[0] {
        a = -a;
    }
    if abs[1] {
        b = b.abs();
    }
    if neg[1] {
        b = -b;
    }
    a * b
}

/// An instruction, its SDWA word, and what it computes from a lane's [`data`].
type FloatCase = (&'static str, Sdwa, fn([u32; 6]) -> u32);

/// The cases of [`sdwa_float_modifiers_compute_as_published`].
fn float_cases() -> Vec<FloatCase> {
    vec![
        (
            "v_mul_f32_e32",
            Sdwa {
                neg: [true, false],
                abs: [false, true],
                ..Sdwa::on(20)
            },
            |d| product(d, [true, false], [false, true]).to_bits(),
        ),
        (
            "v_mul_f32_e32",
            Sdwa {
                neg: [false, true],
                abs: [true, false],
                omod: 1,
                ..Sdwa::on(20)
            },
            |d| (product(d, [false, true], [true, false]) * 2.0).to_bits(),
        ),
        (
            "v_mul_f32_e32",
            Sdwa {
                omod: 2,
                ..Sdwa::on(20)
            },
            |d| (product(d, [false; 2], [false; 2]) * 4.0).to_bits(),
        ),
        (
            "v_mul_f32_e32",
            Sdwa {
                omod: 3,
                ..Sdwa::on(20)
            },
            |d| (product(d, [false; 2], [false; 2]) * 0.5).to_bits(),
        ),
        // v_add_f16_sdwa vD, v22, v22 src0_sel:WORD_1 src1_sel:WORD_0, into each half.
        (
            "v_add_f16_e32",
            Sdwa {
                sel: [(WORD_1, false), (WORD_0, false)],
                dst: (WORD_1, PRESERVE),
                ..Sdwa::on(22)
            },
            |[_, _, v3, _, _, v22]| {
                let sum = from_half(v22 >> 16) + from_half(v22 & 0xffff);
                (v3 & 0xffff) | half(sum) << 16
            },
        ),
        (
            "v_add_f16_e32",
            Sdwa {
                sel: [(WORD_1, false), (WORD_0, false)],
                dst: (WORD_0, PAD),
                neg: [true, false],
                ..Sdwa::on(22)
            },
            |[.., v22]| half(-from_half(v22 >> 16) + from_half(v22 & 0xffff)),
        ),
        (
            "v_add_f16_e32",
            Sdwa {
                sel: [(WORD_1, false), (WORD_1, false)],
                abs: [true, false],
                ..Sdwa::on(22)
            },
            |[.., v22]| half(from_half(v22 >> 16).abs() + from_half(v22 >> 16)),
        ),
        (
            "v_cvt_f32_ubyte0_e32",
            Sdwa {
                sel: [(BYTE_2, false), (DWORD, false)],
                ..Sdwa::on(1)
            },
            |[v1, ..]| f32::from(u8::try_from((v1 >> 16) & 0xff).expect("a byte")).to_bits(),
        ),
        (
            "v_cvt_f16_f32_e32",
            Sdwa {
                dst: (WORD_1, PRESERVE),
                ..Sdwa::on(21)
            },
            |[_, _, v3, _, v21, _]| (v3 & 0xffff) | half(f32::from_bits(v21)) << 16,
        ),
        (
            "v_mov_b32_e32",
            Sdwa {
                sel: [(WORD_0, false), (DWORD, false)],
                neg: [true, false],
                ..Sdwa::on(1)
            },
            |[v1, ..]| (v1 & 0xffff) ^ 0x8000_0000,
        ),
    ]
}

/// The float modifiers: each source's negate and absolute value, and the output multiplier, on a
/// single-precision product; the halves a sixteen-bit add reads and writes, a half's negate on its
/// sign bit, a conversion of a selected byte, a narrowing into a preserved high half, and a negated
/// bit pattern's move.
#[test]
fn sdwa_float_modifiers_compute_as_published() {
    let cases = float_cases();
    let mut body = setup();
    let mut results = Vec::new();
    for (index, (name, sdwa, _)) in cases.iter().enumerate() {
        let at = FIRST_RESULT + index as u32;
        body.push(old(at));
        if name.starts_with("v_cvt") || name.starts_with("v_mov") {
            body.extend([vop1(name, at, SDWA), sdwa.word()]);
        } else {
            let second = if *name == "v_mul_f32_e32" { 21 } else { 22 };
            body.extend([vop2(name, at, SDWA, second), sdwa.word()]);
        }
        results.push(at);
    }
    check(
        "SDWA float modifiers",
        (&body, &results),
        false,
        |lane, index| (cases[index].2)(data(lane)),
    );
}

/// The output clamp, on a single-precision product and on a sixteen-bit sum, in a stage whose
/// `DX10_CLAMP` is set: run on the device only, the evaluator having no stage mode to follow.
#[test]
fn an_sdwa_clamp_bounds_a_float_result() {
    let mut body = setup();
    body.extend([
        old(24),
        vop2("v_mul_f32_e32", 24, SDWA, 21),
        Sdwa {
            clamp: true,
            ..Sdwa::on(20)
        }
        .word(),
        old(25),
        vop2("v_add_f16_e32", 25, SDWA, 22),
        Sdwa {
            clamp: true,
            sel: [(WORD_1, false), (WORD_1, false)],
            dst: (WORD_0, PRESERVE),
            ..Sdwa::on(22)
        }
        .word(),
    ]);
    check("the SDWA clamp", (&body, &[24, 25]), true, |lane, index| {
        let [_, _, v3, v20, v21, v22] = data(lane);
        if index == 0 {
            // A negative zero clamps to zero as every value below it does.
            let product = f32::from_bits(v20) * f32::from_bits(v21);
            if product <= 0.0 {
                0
            } else {
                product.min(1.0).to_bits()
            }
        } else {
            let high = from_half(v22 >> 16);
            (v3 & 0xffff_0000) | half((high + high).clamp(0.0, 1.0))
        }
    });
}

/// Compares in their SDWA form over a select of a float source's bits, and over a sign-extended
/// byte, each answer read back a lane at a time by a select.
#[test]
fn sdwa_compares_select_any_source() {
    let mut body = setup();
    body.extend([
        // v_cmp_lt_f32_sdwa s[20:21], v22, v20 src0_sel:WORD_1: a half's bits as a tiny float,
        // below every lane's float but lane zero's.
        encoded("v_cmp_lt_f32_e32", SDWA | (20 << 9)),
        Sdwa {
            sel: [(WORD_1, false), (DWORD, false)],
            dst: (20, 0),
            ..Sdwa::on(22)
        }
        .word()
            | 1 << 15,
        encoded("v_cndmask_b32_e64", 24),
        inline(0) | inline(1) << 9 | 20 << 18,
        // v_cmp_lt_i32_sdwa vcc, sext(v1), v2 src0_sel:BYTE_1.
        encoded("v_cmp_lt_i32_e32", SDWA | (2 << 9)),
        Sdwa {
            sel: [(BYTE_1, true), (DWORD, false)],
            dst: (0, 0),
            ..Sdwa::on(1)
        }
        .word(),
        encoded("v_cndmask_b32_e64", 25),
        inline(0) | inline(1) << 9 | VCC_LO << 18,
    ]);
    check("SDWA compares", (&body, &[24, 25]), false, |lane, index| {
        let [v1, v2, ..] = data(lane);
        u32::from(if index == 0 {
            lane != 0
        } else {
            signed(select(v1, (BYTE_1, true))) < signed(v2)
        })
    });
}

/// The execution mask the DPP tests run under: some lanes of every row and bank off.
const EXEC: u64 = 0xf0f0_ff3c_5a5a_fff7;

/// `s_mov_b64 exec, EXEC`.
fn partial_exec() -> Vec<u32> {
    vec![
        s_mov(EXEC_LO, LITERAL),
        EXEC as u32,
        s_mov(EXEC_HI, LITERAL),
        (EXEC >> 32) as u32,
    ]
}

fn runs(lane: u32) -> bool {
    (EXEC >> lane) & 1 == 1
}

/// The lane a DPP16 row control reads for `lane`, `None` past its row's edge.
fn dpp16_source(lane: u32, control: u32) -> Option<u32> {
    let (place, row, by) = (lane & 15, lane & !15, control & 15);
    let read = match control {
        0..=0xff => Some((place & !3) | ((control >> (2 * (place & 3))) & 3)),
        0x101..=0x10f => (place + by < 16).then_some(place + by),
        0x111..=0x11f => place.checked_sub(by),
        0x121..=0x12f => Some((place + 16 - by) & 15),
        0x140 => Some(15 - place),
        0x141 => Some((place & 8) | (7 - (place & 7))),
        0x150..=0x15f => Some(by),
        0x160..=0x16f => Some(place ^ by),
        _ => panic!("no such control {control:#x}"),
    };
    read.map(|read| row | read)
}

/// A DPP16 word.
#[derive(Clone, Copy)]
struct Dpp16 {
    src: u32,
    control: u32,
    fi: bool,
    bound: bool,
    rows: u32,
    banks: u32,
    neg: [bool; 2],
    abs: [bool; 2],
}

impl Dpp16 {
    fn word(self) -> u32 {
        self.src
            | self.control << 8
            | u32::from(self.fi) << 18
            | u32::from(self.bound) << 19
            | u32::from(self.neg[0]) << 20
            | u32::from(self.abs[0]) << 21
            | u32::from(self.neg[1]) << 22
            | u32::from(self.abs[1]) << 23
            | self.banks << 24
            | self.rows << 28
    }

    /// What lane `lane` reads of `source` (every lane's), or `None` where the lane is not written:
    /// it does not run, its row or bank is off, or its source lies past the row or does not run,
    /// unfetched, without `bound_ctrl` to read zero instead.
    fn read(self, lane: u32, source: &[u32]) -> Option<u32> {
        let enabled = (self.rows >> (lane >> 4)) & (self.banks >> ((lane >> 2) & 3)) & 1 == 1;
        if !runs(lane) || !enabled {
            return None;
        }
        match dpp16_source(lane, self.control) {
            Some(from) if runs(from) || self.fi => Some(source[from as usize]),
            _ if self.bound => Some(0),
            _ => None,
        }
    }
}

/// Every DPP16 row control, under a partial execution mask, with and without `bound_ctrl` and
/// `fi`, and under row and bank masks: moves of a scattered pattern, each into a destination
/// holding its old bits.
#[test]
fn dpp16_row_controls_fetch_and_bound_as_published() {
    let controls = [
        0x1b, 0xe4, 0x101, 0x10f, 0x111, 0x113, 0x121, 0x12f, 0x140, 0x141, 0x150, 0x15d, 0x160,
        0x165,
    ];
    let mut cases = Vec::new();
    for control in controls {
        for (fi, bound) in [(false, false), (false, true), (true, false), (true, true)] {
            cases.push(Dpp16 {
                src: 1,
                control,
                fi,
                bound,
                rows: 0xf,
                banks: 0xf,
                neg: [false; 2],
                abs: [false; 2],
            });
        }
    }
    for (rows, banks) in [(0xa, 0xf), (0xf, 0x5), (0x6, 0x9)] {
        cases.push(Dpp16 {
            src: 1,
            control: 0x111,
            fi: false,
            bound: true,
            rows,
            banks,
            neg: [false; 2],
            abs: [false; 2],
        });
    }
    let mut body = setup();
    for index in 0..cases.len() {
        body.push(old(FIRST_RESULT + index as u32));
    }
    body.extend(partial_exec());
    let mut results = Vec::new();
    for (index, dpp) in cases.iter().enumerate() {
        let at = FIRST_RESULT + index as u32;
        body.extend([vop1("v_mov_b32_e32", at, DPP16), dpp.word()]);
        results.push(at);
    }
    let v1: Vec<u32> = (0..64).map(|lane| data(lane)[0]).collect();
    check(
        "DPP16 row controls",
        (&body, &results),
        false,
        |lane, index| cases[index].read(lane, &v1).unwrap_or(data(lane)[2]),
    );
}

/// DPP16 on two-source instructions, the permuted source first: an integer difference, a float
/// sum under each source's negate and absolute value, a select by the condition mask, and a fused
/// multiply-add into its destination.
#[test]
fn dpp16_reads_across_lanes_for_any_instruction() {
    let base = Dpp16 {
        src: 1,
        control: 0x111,
        fi: false,
        bound: false,
        rows: 0xf,
        banks: 0xf,
        neg: [false; 2],
        abs: [false; 2],
    };
    let float = Dpp16 {
        src: 20,
        control: 0x4e,
        bound: true,
        neg: [true, false],
        abs: [false, true],
        ..base
    };
    let mask: u64 = 0x0123_4567_89ab_cdef;
    let mut body = setup();
    body.extend([
        old(24),
        old(25),
        old(26),
        vop1("v_mov_b32_e32", 27, vgpr(21)),
        s_mov(VCC_LO, LITERAL),
        mask as u32,
        s_mov(VCC_HI, LITERAL),
        (mask >> 32) as u32,
    ]);
    body.extend(partial_exec());
    body.extend([
        vop2("v_sub_nc_u32_e32", 24, DPP16, 2),
        base.word(),
        vop2("v_add_f32_e32", 25, DPP16, 21),
        float.word(),
        vop2("v_cndmask_b32_e32", 26, DPP16, 2),
        Dpp16 {
            control: 0x165,
            ..base
        }
        .word(),
        vop2("v_fmac_f32_e32", 27, DPP16, 20),
        Dpp16 {
            src: 20,
            control: 0x1b,
            bound: true,
            ..base
        }
        .word(),
    ]);
    let all: Vec<[u32; 6]> = (0..64).map(data).collect();
    let column = |at: usize| -> Vec<u32> { all.iter().map(|d| d[at]).collect() };
    let (v1, v20) = (column(0), column(3));
    check(
        "DPP16 on two-source instructions",
        (&body, &[24, 25, 26, 27]),
        false,
        |lane, index| {
            let [_, v2, v3, own20, v21, _] = data(lane);
            match index {
                0 => base.read(lane, &v1).map_or(v3, |a| a.wrapping_sub(v2)),
                1 => float.read(lane, &v20).map_or(v3, |a| {
                    (-f32::from_bits(a) + f32::from_bits(v21).abs()).to_bits()
                }),
                2 => Dpp16 {
                    control: 0x165,
                    ..base
                }
                .read(lane, &v1)
                .map_or(v3, |a| if (mask >> lane) & 1 == 1 { v2 } else { a }),
                _ => {
                    let dpp = Dpp16 {
                        src: 20,
                        control: 0x1b,
                        bound: true,
                        ..base
                    };
                    dpp.read(lane, &v20).map_or(v21, |a| {
                        f32::from_bits(a)
                            .mul_add(f32::from_bits(own20), f32::from_bits(v21))
                            .to_bits()
                    })
                }
            }
        },
    );
}

/// DPP8: each lane reads the lane of its group of eight its select names, a lane that does not run
/// read as zero unless `fi:1` fetches it, on a move and on a two-source integer sum.
#[test]
fn dpp8_reads_within_groups_of_eight() {
    let reversed = [7, 6, 5, 4, 3, 2, 1, 0];
    let scattered = [1, 1, 3, 3, 6, 0, 7, 2];
    let pack = |selects: [u32; 8], src: u32| {
        selects
            .iter()
            .enumerate()
            .fold(src, |word, (lane, &select)| word | select << (8 + 3 * lane))
    };
    let cases = [
        (DPP8, reversed, "v_mov_b32_e32"),
        (DPP8_FI, reversed, "v_mov_b32_e32"),
        (DPP8, scattered, "v_add_nc_u32_e32"),
        (DPP8_FI, scattered, "v_add_nc_u32_e32"),
    ];
    let mut body = setup();
    for index in 0..cases.len() {
        body.push(old(FIRST_RESULT + index as u32));
    }
    body.extend(partial_exec());
    let mut results = Vec::new();
    for (index, (marker, selects, name)) in cases.iter().enumerate() {
        let at = FIRST_RESULT + index as u32;
        if *name == "v_mov_b32_e32" {
            body.push(vop1(name, at, *marker));
        } else {
            body.push(vop2(name, at, *marker, 2));
        }
        body.push(pack(*selects, 1));
        results.push(at);
    }
    check("DPP8", (&body, &results), false, |lane, index| {
        let [_, v2, v3, ..] = data(lane);
        if !runs(lane) {
            return v3;
        }
        let (marker, selects, name) = cases[index];
        let from = (lane & !7) | selects[(lane & 7) as usize];
        let read = if runs(from) || marker == DPP8_FI {
            data(from)[0]
        } else {
            0
        };
        if name == "v_mov_b32_e32" {
            read
        } else {
            read.wrapping_add(v2)
        }
    });
}
