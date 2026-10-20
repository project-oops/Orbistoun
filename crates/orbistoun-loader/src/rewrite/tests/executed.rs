//! The rewrites executed: each trampoline run against the reference semantics over generated
//! inputs, and on a host that has SSE4a, the reference run against the instruction itself.

use std::arch::asm;

use orbistoun_mem::{AddressSpace, Protection};

use super::super::{Field, Form, Host, extrq, insertq, plan};
use super::Inputs;

static RANGE: orbistoun_mem::test_bases::Range =
    orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::LOADER);

/// How many inputs each comparison runs.
const ROUNDS: usize = 20_000;

/// The flags a trampoline must keep: carry, parity, adjust, zero, sign and overflow.
const ARITHMETIC_FLAGS: u64 = 0x8d5;

/// What a call into rewritten code left behind.
#[derive(Debug, PartialEq, Eq)]
struct After {
    xmm: [u128; 16],
    /// `rax`, `rcx`, `rdx` and `rbx`, each set to a sentinel before the call.
    borrowed: [u64; 4],
    /// The arithmetic flags, set before the call.
    flags: u64,
    /// Every red-zone quadword XORed with the pattern written there, ORed together.
    red_zone: u64,
}

/// Sentinels for the borrowed registers.
const SENTINELS: [u64; 4] = [
    0x1111_aaaa_1111_aaaa,
    0x2222_bbbb_2222_bbbb,
    0x3333_cccc_3333_cccc,
    0x4444_dddd_4444_dddd,
];

/// What the driver reads and writes: the `xmm` registers, the four borrowed general registers, the
/// code to call, the flags before and after, and the red-zone verdict.
#[repr(C)]
struct State {
    xmm: [u128; 16],
    borrowed: [u64; 4],
    entry: u64,
    flags: u64,
    red_zone: u64,
}

// The driver, with every register it uses fixed: `rdi` holds the `State` (System V, on every host),
// `rbp` keeps it across the call, and `r8`-`r11` are scratch the rewritten code must not disturb.
// The callee's red zone is the 128 bytes below its stack pointer, which is the driver's less the
// return address: filled with a pattern before the call and compared after it.
std::arch::global_asm!(
    ".globl orbistoun_rewrite_test_drive",
    "orbistoun_rewrite_test_drive:",
    "push rbx",
    "push rbp",
    "mov rbp, rdi",
    "movdqu xmm0, [rbp]",
    "movdqu xmm1, [rbp + 16]",
    "movdqu xmm2, [rbp + 32]",
    "movdqu xmm3, [rbp + 48]",
    "movdqu xmm4, [rbp + 64]",
    "movdqu xmm5, [rbp + 80]",
    "movdqu xmm6, [rbp + 96]",
    "movdqu xmm7, [rbp + 112]",
    "movdqu xmm8, [rbp + 128]",
    "movdqu xmm9, [rbp + 144]",
    "movdqu xmm10, [rbp + 160]",
    "movdqu xmm11, [rbp + 176]",
    "movdqu xmm12, [rbp + 192]",
    "movdqu xmm13, [rbp + 208]",
    "movdqu xmm14, [rbp + 224]",
    "movdqu xmm15, [rbp + 240]",
    "mov rax, [rbp + 256]",
    "mov rcx, [rbp + 264]",
    "mov rdx, [rbp + 272]",
    "mov rbx, [rbp + 280]",
    "mov r8, 0x5a5ac3c3a5a53c3c",
    "mov r9, -136",
    "2:",
    "mov [rsp + r9], r8",
    "add r9, 8",
    "cmp r9, -8",
    "jne 2b",
    "push qword ptr [rbp + 296]",
    "popfq",
    "call qword ptr [rbp + 288]",
    "pushfq",
    "pop qword ptr [rbp + 296]",
    "mov [rbp + 256], rax",
    "mov [rbp + 264], rcx",
    "mov [rbp + 272], rdx",
    "mov [rbp + 280], rbx",
    "xor r11, r11",
    "mov r9, -136",
    "3:",
    "mov r10, [rsp + r9]",
    "xor r10, r8",
    "or r11, r10",
    "add r9, 8",
    "cmp r9, -8",
    "jne 3b",
    "mov [rbp + 304], r11",
    "movdqu [rbp], xmm0",
    "movdqu [rbp + 16], xmm1",
    "movdqu [rbp + 32], xmm2",
    "movdqu [rbp + 48], xmm3",
    "movdqu [rbp + 64], xmm4",
    "movdqu [rbp + 80], xmm5",
    "movdqu [rbp + 96], xmm6",
    "movdqu [rbp + 112], xmm7",
    "movdqu [rbp + 128], xmm8",
    "movdqu [rbp + 144], xmm9",
    "movdqu [rbp + 160], xmm10",
    "movdqu [rbp + 176], xmm11",
    "movdqu [rbp + 192], xmm12",
    "movdqu [rbp + 208], xmm13",
    "movdqu [rbp + 224], xmm14",
    "movdqu [rbp + 240], xmm15",
    "pop rbp",
    "pop rbx",
    "ret",
);

unsafe extern "sysv64" {
    /// The driver above.
    fn orbistoun_rewrite_test_drive(state: *mut State);
}

/// Calls `entry` with the sixteen `xmm` registers loaded from `xmm`, and reads back what it left.
///
/// # Safety
///
/// `entry` must be executable code that returns with `ret`, touching no memory but its stack and
/// keeping `rbp`, `rsp` and `r8`-`r11`.
unsafe fn call(entry: u64, xmm: [u128; 16]) -> After {
    let mut state = State {
        xmm,
        borrowed: SENTINELS,
        entry,
        flags: ARITHMETIC_FLAGS | 0x2,
        red_zone: 0,
    };
    // SAFETY: the driver reads and writes only `state` and its own stack, and calls `entry`, which
    // the caller guarantees.
    unsafe { orbistoun_rewrite_test_drive(&raw mut state) };
    After {
        xmm: state.xmm,
        borrowed: state.borrowed,
        flags: state.flags & ARITHMETIC_FLAGS,
        red_zone: state.red_zone,
    }
}

/// Executable memory holding one site, rewritten for a host without SSE4a, followed by `ret`.
struct Site {
    _space: AddressSpace,
    entry: u64,
}

/// Rewrites the instruction `bytes` as a host without SSE4a needs, into executable memory.
fn rewritten(bytes: &[u8]) -> Site {
    const LEN: u64 = 0x10000;
    let base = RANGE.take();
    let room_at = base + 0x8000;
    let mut code = bytes.to_vec();
    code.push(0xc3);
    let done = plan(&[(&code, base)], Host::BARE, room_at);
    assert_eq!(
        done.patches.len(),
        1,
        "one site, rewritten: {:?}",
        done.rewrites
    );
    let mut space = AddressSpace::new();
    space
        .reserve(base, LEN, Protection::READ_WRITE)
        .expect("reserves");
    let write = |at: u64, bytes: &[u8]| {
        // SAFETY: every write lands in the reservation just made, which nothing else holds.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(at as usize),
                bytes.len(),
            );
        }
    };
    write(base, &code);
    for (at, patch) in &done.patches {
        write(*at, patch);
    }
    write(room_at, &done.room);
    space
        .protect(base, LEN, Protection::READ_EXECUTE)
        .expect("protects");
    Site {
        _space: space,
        entry: base,
    }
}

/// `extrq` or `insertq` on `dst` and `src`, with the encoding's ModRM register fields.
fn encode(form: Form) -> Vec<u8> {
    let (prefix, dst, src, field) = match form {
        Form::Extrq { dst, src, field } => (0x66, dst, src, field),
        Form::Insertq { dst, src, field } => (0xf2, dst, src, field),
        _ => unreachable!("only the SSE4a forms are encoded"),
    };
    let immediate = matches!(field, Field::Immediate { .. });
    // `extrq`'s immediate form has `/0` in the register field.
    let reg = if immediate && matches!(form, Form::Extrq { .. }) {
        0
    } else {
        dst
    };
    let rm = if immediate && matches!(form, Form::Extrq { .. }) {
        dst
    } else {
        src
    };
    let mut bytes = vec![prefix];
    // Always a REX prefix, empty where no register needs one, so even a register form of the low
    // eight is long enough for a jump and runs from its trampoline here.
    bytes.push(0x40 | (u8::from(reg >= 8) << 2) | u8::from(rm >= 8));
    bytes.extend([0x0f, if immediate { 0x78 } else { 0x79 }]);
    bytes.push(0xc0 | ((reg & 7) << 3) | (rm & 7));
    if let Field::Immediate { len, index } = field {
        bytes.extend([len, index]);
    }
    bytes
}

/// What `form` does to `xmm`, by the reference semantics.
fn reference(form: Form, mut xmm: [u128; 16]) -> [u128; 16] {
    super::super::emulate(form, &mut xmm).expect("an SSE4a form");
    xmm
}

/// Runs `form`, rewritten, over generated inputs, against the reference.
fn check(form: Form, inputs: &mut Inputs) {
    let bytes = encode(form);
    let site = rewritten(&bytes);
    for _ in 0..ROUNDS {
        let mut xmm = [0_u128; 16];
        for register in &mut xmm {
            *register = inputs.wide();
        }
        // SAFETY: the site is the rewritten instruction then `ret`, in executable memory, and its
        // trampoline touches only the stack.
        let after = unsafe { call(site.entry, xmm) };
        assert_eq!(
            after.xmm,
            reference(form, xmm),
            "{form:?} {bytes:02x?} on {xmm:x?}"
        );
        assert_eq!(
            after.borrowed, SENTINELS,
            "{form:?} kept its borrowed registers"
        );
        assert_eq!(after.flags, ARITHMETIC_FLAGS, "{form:?} kept the flags");
        assert_eq!(after.red_zone, 0, "{form:?} left the red zone alone");
    }
}

/// Every register pair and a spread of fields, in both forms of both instructions, computes what
/// the reference does and leaves everything else as it was.
#[test]
fn each_rewritten_form_matches_the_reference() {
    let mut inputs = Inputs(0x9e37_79b9_7f4a_7c15);
    let fields = [
        (0, 0),
        (1, 63),
        (8, 8),
        (16, 32),
        (24, 0),
        (40, 40),
        (63, 1),
    ];
    for dst in [0, 7, 8, 15] {
        for src in [0, 3, 9, 15] {
            check(
                Form::Extrq {
                    dst,
                    src,
                    field: Field::Register,
                },
                &mut inputs,
            );
            check(
                Form::Insertq {
                    dst,
                    src,
                    field: Field::Register,
                },
                &mut inputs,
            );
        }
        for (len, index) in fields {
            let field = Field::Immediate { len, index };
            check(
                Form::Extrq {
                    dst,
                    src: dst,
                    field,
                },
                &mut inputs,
            );
            check(
                Form::Insertq {
                    dst,
                    src: (dst + 5) & 15,
                    field,
                },
                &mut inputs,
            );
        }
    }
}

/// `extrq xmm0, xmm1` and `insertq xmm0, xmm1` on this processor.
///
/// # Safety
///
/// The processor must have SSE4a.
unsafe fn native(insert: bool, dst: u128, src: u128) -> u128 {
    let mut out = dst;
    let at = std::ptr::from_mut(&mut out);
    let from = std::ptr::from_ref(&src);
    if insert {
        // SAFETY: the caller guarantees SSE4a; the block reads and writes only the two locals.
        unsafe {
            asm!("movdqu xmm0, [{a}]", "movdqu xmm1, [{s}]", "insertq xmm0, xmm1",
                 "movdqu [{a}], xmm0", a = in(reg) at, s = in(reg) from,
                 out("xmm0") _, out("xmm1") _);
        }
    } else {
        // SAFETY: as above.
        unsafe {
            asm!("movdqu xmm0, [{a}]", "movdqu xmm1, [{s}]", "extrq xmm0, xmm1",
                 "movdqu [{a}], xmm0", a = in(reg) at, s = in(reg) from,
                 out("xmm0") _, out("xmm1") _);
        }
    }
    out
}

/// `extrq xmm0, len, index` on this processor, for the immediate form's fields.
macro_rules! native_immediate {
    ($dst:expr, $len:literal, $index:literal) => {{
        let mut out: u128 = $dst;
        let at = std::ptr::from_mut(&mut out);
        // SAFETY: called only where the processor has SSE4a; reads and writes only `out`.
        unsafe {
            asm!("movdqu xmm0, [{a}]", concat!("extrq xmm0, ", $len, ", ", $index),
                 "movdqu [{a}], xmm0", a = in(reg) at, out("xmm0") _);
        }
        (out, $len, $index)
    }};
}

/// On a processor with SSE4a, the reference agrees with the instruction over every length and
/// index, a field past bit 63 and the upper quadword included, and with the immediate form's
/// operand order.
#[test]
fn the_reference_matches_this_processor() {
    if !Host::detect().sse4a {
        eprintln!("this processor has no SSE4a: the reference is checked by the rewrites alone");
        return;
    }
    let mut inputs = Inputs(0x2545_f491_4f6c_dd1d);
    for _ in 0..ROUNDS * 10 {
        let dst = inputs.wide();
        let (len, index) = (inputs.next() & 63, inputs.next() & 63);
        let junk = inputs.next() & !0x3f3f;
        let control = u128::from(junk | len | (index << 8));
        // SAFETY: the processor has SSE4a.
        let got = unsafe { native(false, dst, (u128::from(inputs.next()) << 64) | control) };
        assert_eq!(
            got,
            extrq(dst, len as u32, index as u32),
            "extrq {dst:x} {len} {index}"
        );
        let source = (control << 64) | u128::from(inputs.next());
        // SAFETY: as above.
        let got = unsafe { native(true, dst, source) };
        assert_eq!(
            got,
            insertq(dst, source as u64, len as u32, index as u32),
            "insertq {dst:x} {source:x}"
        );
    }
    // The legacy encoding keeps the register's bits above 127, as the trampoline's legacy load does.
    if std::arch::is_x86_feature_detected!("avx2") {
        let upper: [u64; 4] = [0x5555_5555_5555_5555; 4];
        let mut kept = [0_u64; 4];
        // SAFETY: the processor has SSE4a and AVX2; the block reads `upper` and writes `kept`.
        unsafe {
            asm!("vmovdqu ymm0, [{u}]", "extrq xmm0, 16, 32", "vmovdqu [{k}], ymm0",
                 u = in(reg) upper.as_ptr(), k = in(reg) kept.as_mut_ptr(), out("ymm0") _);
        }
        assert_eq!(kept[2..], upper[2..], "bits 255:128 are kept");
    }
    let dst = inputs.wide();
    for (got, len, index) in [
        native_immediate!(dst, 16, 32),
        native_immediate!(dst, 8, 8),
        native_immediate!(dst, 0, 5),
        native_immediate!(dst, 40, 40),
    ] {
        assert_eq!(got, extrq(dst, len, index), "extrq xmm0, {len}, {index}");
    }
}
