//! Instructions the host lacks, made to run on it (D725, D924).
//!
//! A title built for the console's processor may use instructions only AMD processors execute.
//! On a host without one, each site the plan's sweep finds is rewritten at link, while the image
//! is still writable:
//!
//! - an `extrq` or `insertq` long enough to hold a jump becomes a jump to a trampoline placed past
//!   the module's last segment, which computes the same bits with instructions every x86-64 host
//!   has and jumps back;
//! - one too short for a jump is left as it is, and the illegal-instruction fault it raises is
//!   answered by [`emulate`] from the faulting thread's registers;
//! - `monitorx` becomes a no-op and `mwaitx` a `pause`: a monitored wait may end early for any
//!   reason, so a wait that ends at once is one the architecture allows;
//! - `clzero`, `rdpru` and `mcommit` are refused by name when they fault. No title in the library
//!   uses them, and `rdpru` reads console counters nothing has measured.
//!
//! The semantics are the AMD64 Architecture Programmer's Manual's, Volume 4: `EXTRQ` and
//! `INSERTQ` take a field length and a bit index of six bits each, a length of zero meaning
//! sixty-four, and act on the low quadword of the destination. What the manual leaves undefined
//! (the upper quadword, a field running past bit 63) is what the host this was written on
//! measured; see [`extrq`].

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};

/// The instruction set extensions a rewrite depends on, as one host has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Host {
    /// `extrq` and `insertq`.
    pub sse4a: bool,
    /// `monitorx` and `mwaitx`.
    pub monitorx: bool,
    /// `clzero`.
    pub clzero: bool,
    /// `rdpru`.
    pub rdpru: bool,
    /// `mcommit`.
    pub mcommit: bool,
    /// Whether what this host lacks was taken away from a processor that has it, by
    /// `ORBISTOUN_HOST_LACKS`: a site left to fault then gets `ud2`, so the fault is raised.
    pub rehearsal: bool,
}

impl Host {
    /// A host with every one, as the console is: nothing is rewritten.
    pub const FULL: Self = Self {
        sse4a: true,
        monitorx: true,
        clzero: true,
        rdpru: true,
        mcommit: true,
        rehearsal: false,
    };

    /// A host with none, as an Intel processor is.
    pub const BARE: Self = Self {
        sse4a: false,
        monitorx: false,
        clzero: false,
        rdpru: false,
        mcommit: false,
        rehearsal: false,
    };

    /// What this processor reports through `cpuid`: extended leaf `0x8000_0001` `ecx` bit 6
    /// (SSE4a) and bit 29 (MONITORX), and leaf `0x8000_0008` `ebx` bit 0 (CLZERO), bit 4 (RDPRU)
    /// and bit 8 (MCOMMIT), as AMD's CPUID specification assigns them.
    #[must_use]
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let top = core::arch::x86_64::__cpuid(0x8000_0000).eax;
            let ecx = if top >= 0x8000_0001 {
                core::arch::x86_64::__cpuid(0x8000_0001).ecx
            } else {
                0
            };
            let ebx = if top >= 0x8000_0008 {
                core::arch::x86_64::__cpuid(0x8000_0008).ebx
            } else {
                0
            };
            Self {
                sse4a: ecx & (1 << 6) != 0,
                monitorx: ecx & (1 << 29) != 0,
                clzero: ebx & 1 != 0,
                rdpru: ebx & (1 << 4) != 0,
                mcommit: ebx & (1 << 8) != 0,
                rehearsal: false,
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Self::BARE
        }
    }

    /// This host, less whatever `ORBISTOUN_HOST_LACKS` names: a host that has an instruction can
    /// run a title as one without it would. Read once.
    #[must_use]
    pub fn running() -> Self {
        static RUNNING: OnceLock<Host> = OnceLock::new();
        *RUNNING.get_or_init(|| {
            Self::detect().without(orbistoun_env::HOST_LACKS.get().as_deref().unwrap_or(""))
        })
    }

    /// The same host with each extension in the comma-separated `names` taken away, a rehearsal
    /// when any of them was there to take.
    #[must_use]
    pub fn without(mut self, names: &str) -> Self {
        for name in names.split(',').map(str::trim) {
            let field = match name {
                "sse4a" => &mut self.sse4a,
                "monitorx" => &mut self.monitorx,
                "clzero" => &mut self.clzero,
                "rdpru" => &mut self.rdpru,
                "mcommit" => &mut self.mcommit,
                _ => continue,
            };
            self.rehearsal |= *field;
            *field = false;
        }
        self
    }

    /// Whether this host has every one, so nothing is rewritten.
    #[must_use]
    pub const fn has_all(self) -> bool {
        self.sse4a && self.monitorx && self.clzero && self.rdpru && self.mcommit
    }

    /// Whether this host executes `form` itself.
    #[must_use]
    pub const fn has(self, form: Form) -> bool {
        match form {
            Form::Extrq { .. } | Form::Insertq { .. } => self.sse4a,
            Form::Monitorx | Form::Mwaitx => self.monitorx,
            Form::Clzero => self.clzero,
            Form::Rdpru => self.rdpru,
            Form::Mcommit => self.mcommit,
        }
    }

    /// Whether a site of `form` left to fault would not fault here, because a rehearsal took away
    /// what the processor has.
    fn rehearses(self, form: Form) -> bool {
        self.rehearsal && !self.has(form) && Self::detect().has(form)
    }
}

/// Where an `extrq` or `insertq` takes its field length and bit index from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The two immediates: length, then index.
    Immediate {
        /// The field length, six bits, zero meaning sixty-four.
        len: u8,
        /// The bit index, six bits.
        index: u8,
    },
    /// A register: bits 5:0 the length and bits 13:8 the index, of its low quadword for `extrq`
    /// and its high quadword for `insertq`.
    Register,
}

/// One AMD-only instruction, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `extrq xmm, ...`: the field of the destination's low quadword, moved down to bit 0.
    Extrq {
        /// The register extracted from and written, 0-15.
        dst: u8,
        /// The register holding the field, when it is not in immediates.
        src: u8,
        /// Where the field is.
        field: Field,
    },
    /// `insertq xmm, xmm, ...`: the source's low bits written into a field of the destination.
    Insertq {
        /// The register written, 0-15.
        dst: u8,
        /// The register whose low bits are inserted, 0-15.
        src: u8,
        /// Where the field is.
        field: Field,
    },
    /// `monitorx`.
    Monitorx,
    /// `mwaitx`.
    Mwaitx,
    /// `clzero`.
    Clzero,
    /// `rdpru`.
    Rdpru,
    /// `mcommit`.
    Mcommit,
}

impl Form {
    /// The instruction's mnemonic, as the plan lists it.
    #[must_use]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::Extrq { .. } => "extrq",
            Self::Insertq { .. } => "insertq",
            Self::Monitorx => "monitorx",
            Self::Mwaitx => "mwaitx",
            Self::Clzero => "clzero",
            Self::Rdpru => "rdpru",
            Self::Mcommit => "mcommit",
        }
    }

    /// The form of a decoded instruction, or `None` for one every host has.
    #[must_use]
    pub fn of(instruction: &Instruction) -> Option<Self> {
        // The xmm registers are numbered consecutively in the decoder's register list.
        let xmm = |register: Register| {
            ((register as u32).wrapping_sub(Register::XMM0 as u32) & 0xf) as u8
        };
        let immediates = |first: u32| {
            (instruction.op_kind(first) == OpKind::Immediate8
                && instruction.op_kind(first + 1) == OpKind::Immediate8_2nd)
                .then(|| Field::Immediate {
                    len: instruction.immediate8() & 0x3f,
                    index: instruction.immediate8_2nd() & 0x3f,
                })
        };
        Some(match instruction.mnemonic() {
            Mnemonic::Extrq => match instruction.op_count() {
                3 => Self::Extrq {
                    dst: xmm(instruction.op0_register()),
                    src: xmm(instruction.op0_register()),
                    field: immediates(1)?,
                },
                2 => Self::Extrq {
                    dst: xmm(instruction.op0_register()),
                    src: xmm(instruction.op1_register()),
                    field: Field::Register,
                },
                _ => return None,
            },
            Mnemonic::Insertq => Self::Insertq {
                dst: xmm(instruction.op0_register()),
                src: xmm(instruction.op1_register()),
                field: match instruction.op_count() {
                    4 => immediates(2)?,
                    2 => Field::Register,
                    _ => return None,
                },
            },
            Mnemonic::Monitorx => Self::Monitorx,
            Mnemonic::Mwaitx => Self::Mwaitx,
            Mnemonic::Clzero => Self::Clzero,
            Mnemonic::Rdpru => Self::Rdpru,
            Mnemonic::Mcommit => Self::Mcommit,
            _ => return None,
        })
    }

    /// The instruction at the start of `code`, which sits at `ip`, with its length, when it is
    /// one of these.
    #[must_use]
    pub fn decode(code: &[u8], ip: u64) -> Option<(Self, usize)> {
        let instruction = Decoder::with_ip(64, code, ip, DecoderOptions::NONE).decode();
        if instruction.is_invalid() {
            return None;
        }
        Self::of(&instruction).map(|form| (form, instruction.len()))
    }
}

/// A field `len` bits wide at bit zero, a length of zero meaning sixty-four.
const fn field_mask(len: u32) -> u64 {
    u64::MAX >> ((64 - (len & 0x3f)) & 0x3f)
}

/// `EXTRQ`: bits `index .. index + len` of `dst`'s low quadword, moved down to bit 0 and the rest
/// of the quadword cleared. The upper quadword is cleared, and a field that runs past bit 63 takes
/// only the bits below it - both undefined in the manual, and both measured on an AMD processor
/// that has the instruction (D924).
#[must_use]
pub const fn extrq(dst: u128, len: u32, index: u32) -> u128 {
    (((dst as u64) >> (index & 0x3f)) & field_mask(len)) as u128
}

/// `INSERTQ`: the low `len` bits of `src`, written into `dst`'s low quadword at bit `index`, every
/// other bit of the quadword kept. The upper quadword is cleared, and the part of a field that runs
/// past bit 63 is dropped - both undefined in the manual, and measured as for [`extrq`].
#[must_use]
pub const fn insertq(dst: u128, src: u64, len: u32, index: u32) -> u128 {
    let index = index & 0x3f;
    let mask = field_mask(len) << index;
    (((dst as u64) & !mask) | ((src & field_mask(len)) << index)) as u128
}

/// The length and index a register-form field names, from the quadword holding them.
const fn from_control(control: u64) -> (u32, u32) {
    ((control & 0x3f) as u32, ((control >> 8) & 0x3f) as u32)
}

/// Why a faulting AMD-only instruction was not carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal(pub Form);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = match self.0 {
            Form::Rdpru => "it reads the console's processor counters, which nothing has measured",
            Form::Clzero | Form::Mcommit => "no title orbistoun has seen uses it",
            _ => "it is rewritten at link, and this site was not",
        };
        write!(
            f,
            "`{}` is an AMD-only instruction this host lacks, and orbistoun does not carry it out: {why}",
            self.0.mnemonic()
        )
    }
}

/// Carries out `form` against a faulting thread's sixteen `xmm` registers, as the processor that
/// has it would. A monitored wait does nothing, as one that ends at once.
///
/// # Errors
///
/// For `clzero`, `rdpru` and `mcommit`, named in the refusal.
pub fn emulate(form: Form, xmm: &mut [u128; 16]) -> Result<(), Refusal> {
    match form {
        Form::Extrq { dst, src, field } => {
            let (len, index) = match field {
                Field::Immediate { len, index } => (u32::from(len), u32::from(index)),
                Field::Register => from_control(xmm[usize::from(src)] as u64),
            };
            xmm[usize::from(dst)] = extrq(xmm[usize::from(dst)], len, index);
            Ok(())
        }
        Form::Insertq { dst, src, field } => {
            let source = xmm[usize::from(src)];
            let (len, index) = match field {
                Field::Immediate { len, index } => (u32::from(len), u32::from(index)),
                Field::Register => from_control((source >> 64) as u64),
            };
            xmm[usize::from(dst)] = insertq(xmm[usize::from(dst)], source as u64, len, index);
            Ok(())
        }
        Form::Monitorx | Form::Mwaitx => Ok(()),
        Form::Clzero | Form::Rdpru | Form::Mcommit => Err(Refusal(form)),
    }
}

/// The length of `jmp rel32`.
const JUMP: usize = 5;

/// `jmp rel32` from `from` to `to`, or `None` when they are more than two gibibytes apart.
fn jump(from: u64, to: u64) -> Option<[u8; JUMP]> {
    let rel = i32::try_from(to.wrapping_sub(from.wrapping_add(JUMP as u64)) as i64).ok()?;
    let [a, b, c, d] = rel.to_le_bytes();
    Some([0xe9, a, b, c, d])
}

/// Machine code, appended to.
#[derive(Debug, Default)]
struct Code(Vec<u8>);

/// Where the trampoline's frame keeps the destination, then the source.
const DST: u8 = 0;
/// See [`DST`].
const SRC: u8 = 16;

impl Code {
    fn put(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// `movdqu` between `xmm` and `[rsp + disp]`: opcode `0x7f` stores, `0x6f` loads. Legacy SSE,
    /// so the register's bits above 127 are kept.
    fn xmm_frame(&mut self, opcode: u8, xmm: u8, disp: u8) {
        self.put(&[0xf3]);
        if xmm >= 8 {
            self.put(&[0x44]);
        }
        self.put(&[0x0f, opcode, 0x44 | ((xmm & 7) << 3), 0x24, disp]);
    }

    /// `mov r64, [rsp + disp]`, `reg` numbered as the encoding numbers them (rax 0 .. rbx 3).
    fn load(&mut self, reg: u8, disp: u8) {
        self.put(&[0x48, 0x8b, 0x44 | (reg << 3), 0x24, disp]);
    }

    /// The field length into `ecx`.
    fn length(&mut self, field: Field, control: u8) {
        match field {
            Field::Immediate { len, .. } => self.put(&[0xb9, len, 0, 0, 0]),
            Field::Register => {
                self.load(1, control);
                self.put(&[0x83, 0xe1, 0x3f]); // and ecx, 63
            }
        }
    }

    /// The bit index into `ecx`.
    fn index(&mut self, field: Field, control: u8) {
        match field {
            Field::Immediate { index, .. } => self.put(&[0xb9, index, 0, 0, 0]),
            Field::Register => {
                self.load(1, control);
                self.put(&[0xc1, 0xe9, 0x08, 0x83, 0xe1, 0x3f]); // shr ecx, 8; and ecx, 63
            }
        }
    }

    /// `rdx` = a field `ecx` bits wide at bit zero, zero meaning sixty-four.
    fn mask(&mut self) {
        self.put(&[
            0x48, 0xc7, 0xc2, 0xff, 0xff, 0xff, 0xff, // mov rdx, -1
            0xf7, 0xd9, // neg ecx
            0x83, 0xe1, 0x3f, // and ecx, 63
            0x48, 0xd3, 0xea, // shr rdx, cl
        ]);
    }
}

/// The frame below the red zone a trampoline keeps the registers it borrows in.
const RED_ZONE: [u8; 5] = [0x48, 0x8d, 0x64, 0x24, 0x80]; // lea rsp, [rsp - 128]

/// A trampoline at `at` that carries out `form` and jumps to `back`, or `None` for a form that
/// has none or a `back` out of reach.
///
/// It steps below the System V red zone first, so the leaf function it interrupts keeps what it
/// stored there, and borrows `rax`, `rcx`, `rdx`, `rbx` and the flags, saved and restored with
/// `pushfq` and `popfq`. The `xmm` operands go through the frame with `movdqu`, the quadword
/// arithmetic is done in general registers, and the destination is loaded back as the result's low
/// quadword over a cleared upper one. The load is legacy SSE, so the register's bits above 127 are
/// kept, as the instruction keeps them.
#[must_use]
pub fn trampoline(form: Form, at: u64, back: u64) -> Option<Vec<u8>> {
    let (Form::Extrq { dst, src, field } | Form::Insertq { dst, src, field }) = form else {
        return None;
    };
    let mut code = Code::default();
    code.put(&RED_ZONE);
    code.put(&[0x9c, 0x50, 0x51, 0x52, 0x53]); // pushfq; push rax, rcx, rdx, rbx
    code.put(&[0x48, 0x8d, 0x64, 0x24, 0xe0]); // lea rsp, [rsp - 32]
    code.xmm_frame(0x7f, dst, DST);
    code.xmm_frame(0x7f, src, SRC);
    code.load(0, DST);
    if let Form::Extrq { .. } = form {
        // The field is in the source's low quadword; for the immediate form the source is the
        // destination, and the immediates are used instead.
        code.index(field, SRC);
        code.put(&[0x48, 0xd3, 0xe8]); // shr rax, cl
        code.length(field, SRC);
        code.mask();
        code.put(&[0x48, 0x21, 0xd0]); // and rax, rdx
    } else {
        // The field is in the source's high quadword.
        code.load(3, SRC);
        code.length(field, SRC + 8);
        code.mask();
        code.put(&[0x48, 0x21, 0xd3]); // and rbx, rdx
        code.index(field, SRC + 8);
        code.put(&[
            0x48, 0xd3, 0xe3, // shl rbx, cl
            0x48, 0xd3, 0xe2, // shl rdx, cl
            0x48, 0xf7, 0xd2, // not rdx
            0x48, 0x21, 0xd0, // and rax, rdx
            0x48, 0x09, 0xd8, // or rax, rbx
        ]);
    }
    code.put(&[0x48, 0x89, 0x44, 0x24, DST]); // mov [rsp], rax
    code.put(&[0x48, 0xc7, 0x44, 0x24, DST + 8, 0, 0, 0, 0]); // mov qword [rsp + 8], 0
    code.xmm_frame(0x6f, dst, DST);
    code.put(&[0x48, 0x8d, 0x64, 0x24, 0x20]); // lea rsp, [rsp + 32]
    code.put(&[0x5b, 0x5a, 0x59, 0x58, 0x9d]); // pop rbx, rdx, rcx, rax; popfq
    code.put(&[0x48, 0x8d, 0xa4, 0x24, 0x80, 0, 0, 0]); // lea rsp, [rsp + 128]
    let from = at.wrapping_add(code.0.len() as u64);
    code.put(&jump(from, back)?);
    Some(code.0)
}

/// How a site was made to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    /// Replaced by a jump to a trampoline.
    Trampoline,
    /// Replaced, in its own bytes, by an instruction every host has.
    InPlace,
    /// Left to fault, and carried out by [`emulate`] when it does.
    AtFault,
    /// Left to fault, and refused by name when it does.
    Refused,
}

impl How {
    /// The word the plan lists it under.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Trampoline => "trampoline",
            Self::InPlace => "in-place",
            Self::AtFault => "at-fault",
            Self::Refused => "refused",
        }
    }
}

/// One site as rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// Its guest address.
    pub at: u64,
    /// What it is.
    pub form: Form,
    /// How it runs.
    pub how: How,
}

/// What rewriting one module writes: bytes over sites, and the trampolines.
#[derive(Debug, Default)]
pub struct ModuleRewrite {
    /// Every site, by address.
    pub rewrites: Vec<Rewrite>,
    /// The bytes written over each rewritten site, by address.
    pub patches: Vec<(u64, Vec<u8>)>,
    /// Where the trampolines go.
    pub room_at: u64,
    /// The trampolines, one after another from [`Self::room_at`].
    pub room: Vec<u8>,
    /// Sites a rehearsal turned into `ud2`, so their fault is raised on a host that has them, each
    /// with the instruction it stands for and that instruction's length.
    pub planted: Vec<(u64, Form, usize)>,
}

/// `ud2`, planted where a rehearsal needs a fault the host would not raise.
const UD2: [u8; 2] = [0x0f, 0x0b];

/// What `host` needs done to the executable segments `code` - each its bytes and guest address -
/// with trampolines from `room_at` on.
#[must_use]
pub fn plan(code: &[(&[u8], u64)], host: Host, room_at: u64) -> ModuleRewrite {
    let mut out = ModuleRewrite {
        room_at,
        ..ModuleRewrite::default()
    };
    if host.has_all() {
        return out;
    }
    for &(segment, start) in code {
        let decoder = Decoder::with_ip(64, segment, start, DecoderOptions::NONE);
        for instruction in decoder {
            let Some(form) = Form::of(&instruction) else {
                continue;
            };
            if host.has(form) {
                continue;
            }
            let (at, len) = (instruction.ip(), instruction.len());
            let how = rewrite_one(form, (at, len), host, &mut out);
            out.rewrites.push(Rewrite { at, form, how });
        }
    }
    out
}

/// Decides one site and writes what it needs into `out`.
fn rewrite_one(form: Form, (at, len): (u64, usize), host: Host, out: &mut ModuleRewrite) -> How {
    let filled = |head: &[u8], fill: u8| {
        let mut bytes = head.to_vec();
        bytes.resize(len, fill);
        bytes
    };
    match form {
        Form::Monitorx => {
            // nop dword [rax], then one-byte nops for any prefixes.
            out.patches.push((at, filled(&[0x0f, 0x1f, 0x00], 0x90)));
            How::InPlace
        }
        Form::Mwaitx => {
            out.patches.push((at, filled(&[0xf3, 0x90], 0x90))); // pause
            How::InPlace
        }
        Form::Extrq { .. } | Form::Insertq { .. } => {
            let place = out.room_at.wrapping_add(out.room.len() as u64);
            let back = at.wrapping_add(len as u64);
            let built = (len >= JUMP)
                .then(|| Some((trampoline(form, place, back)?, jump(at, place)?)))
                .flatten();
            if let Some((body, entry)) = built {
                out.room.extend_from_slice(&body);
                // Padded with `int3`: nothing branches into an instruction's middle, and if
                // something did it would stop at once.
                out.patches.push((at, filled(&entry, 0xcc)));
                How::Trampoline
            } else {
                if host.rehearses(form) {
                    out.patches.push((at, filled(&UD2, 0xcc)));
                    out.planted.push((at, form, len));
                }
                How::AtFault
            }
        }
        Form::Clzero | Form::Rdpru | Form::Mcommit => How::Refused,
    }
}

/// Sites a rehearsal planted `ud2` at, by address, with the instruction each stands for.
static PLANTED: RwLock<BTreeMap<u64, (Form, usize)>> = RwLock::new(BTreeMap::new());

/// Records the sites a rehearsal planted, so their faults are answered as the instruction's.
pub fn register_planted(sites: &[(u64, Form, usize)]) {
    if sites.is_empty() {
        return;
    }
    if let Ok(mut planted) = PLANTED.write() {
        planted.extend(sites.iter().map(|&(at, form, len)| (at, (form, len))));
    }
}

/// What faulted at `rip`, whose bytes begin `code`: the instruction there, or the one a rehearsal
/// planted `ud2` over. `None` for anything else.
#[must_use]
pub fn faulted(code: &[u8], rip: u64) -> Option<(Form, usize)> {
    if code.starts_with(&UD2) {
        return PLANTED.read().ok()?.get(&rip).copied();
    }
    Form::decode(code, rip)
}

#[cfg(test)]
mod tests;
