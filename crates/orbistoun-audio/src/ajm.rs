//! `libSceAjm` - the platform's audio codec job manager: decode and encode work submitted as jobs.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. Context set-up and instances are implemented,
//! as measured; the codec jobs themselves are not.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;
use std::sync::atomic::{AtomicU32, Ordering};

guest_module! {
    "libSceAjm" {
        "sceAjmBatchCancel" => 6,
        "sceAjmBatchErrorDump" => 6,
        // (buffer, size, &batch), as measured.
        "sceAjmBatchInitialize" => 3,
        // (&batch, instance, input, input size, output, output size), as measured.
        "sceAjmBatchJobDecode" => 6,
        "sceAjmBatchJobInitialize" => 6,
        "sceAjmBatchJobSetGaplessDecode" => 6,
        "sceAjmBatchStart" => 6,
        "sceAjmBatchWait" => 6,
        // (ctx).
        "sceAjmFinalize" => 1,
        // (0, &ctx), as measured.
        "sceAjmInitialize" => 2,
        // (ctx, codec, flags, &out), as measured.
        "sceAjmInstanceCreate" => 4,
        // (ctx, id).
        "sceAjmInstanceDestroy" => 2,
        // (ctx, codec, 0).
        "sceAjmModuleRegister" => 3,
        "sceAjmModuleUnregister" => 6,
    }
}

/// `SCE_AJM_ERROR_INVALID_PARAMETER`-family answer the census measured for a non-zero first
/// argument and for a null slot (`0x80930005`).
const INVALID: u64 = 0x8093_0005;

/// The next context identifier: `0x15` first, as REQ-cn10 arm 1 measured a fresh process's first
/// context, then one more each time.
static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(FIRST_CONTEXT);

/// `sceAjmInitialize(0, &ctx)`: writes a 32-bit context identifier and answers 0, the four bytes
/// after it left as they were (`call1-ctx` `15000000ffffffff`, `call2-ctx` `16000000...`,
/// `20261009-151440-eboot.obs.log`). A non-zero first argument or a null slot answers
/// `0x80930005`, as the census's pattern and zero arguments were answered.
pub(crate) fn ajm_initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (reserved, out) = (args[0], args[1]);
    if reserved != 0 || out == 0 {
        return INVALID;
    }
    let context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's out-parameter, a four-byte context identifier by the call's contract.
    if unsafe { orbistoun_mem::guest::write_u32(out, context) } {
        0
    } else {
        INVALID
    }
}

/// What a repeat `sceAjmModuleRegister` of a codec answered (REQ-aj01).
const ALREADY_REGISTERED: u64 = 0x8093_0009;

/// The codecs measured registering: 0 to 3 (REQ-aj01).
const MEASURED_CODECS: u32 = 4;

/// The first context identifier `sceAjmInitialize` issues.
const FIRST_CONTEXT: u32 = 0x15;

/// Which codecs each context has registered, and which contexts are finalised.
#[derive(Default)]
struct Registry {
    /// `(context, codec)` pairs registered.
    registered: std::collections::BTreeSet<(u32, u32)>,
    /// Contexts finalised.
    finalised: std::collections::BTreeSet<u32>,
    /// The last instance number each context issued.
    issued: std::collections::BTreeMap<u32, u32>,
    /// `(context, instance id)` pairs created and not destroyed.
    instances: std::collections::BTreeSet<(u32, u32)>,
    /// Batches by the buffer `sceAjmBatchInitialize` was given, with the jobs each holds.
    batches: std::collections::BTreeMap<u64, Vec<Job>>,
}

/// A decode job a batch holds, by the instance it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Job {
    /// The instance identifier the job was given.
    instance: u32,
}

/// The smallest batch buffer measured: PPSA04263's 0x40 bytes, which holds one job (REQ-ai02).
const SMALLEST_BATCH: u64 = 0x40;

/// The largest batch buffer measured, 0x1000 bytes (REQ-ai02).
const LARGEST_BATCH: u64 = 0x1000;

/// The bytes `sceAjmBatchInitialize` wrote at the start of the buffer: zero but for the size at
/// +8, and nothing past them (`buffer-80b`, REQ-ai02).
const BATCH_HEADER_BYTES: usize = 0x20;

/// What `sceAjmInstanceCreate` answered for a codec the context had not registered, the slot left
/// as it was (REQ-ai01).
const NOT_REGISTERED: u64 = 0x8093_000a;

/// What a second `sceAjmInstanceDestroy` of one instance answered (REQ-ai01).
const INVALID_INSTANCE: u64 = 0x8093_0003;

/// The flags `sceAjmInstanceCreate` was measured accepting: 0, 1 and 2 (REQ-ai01).
const MEASURED_FLAGS: u64 = 3;

/// Where an instance identifier carries its codec: bits 22 and up, as codecs 1 to 3 measured
/// (`0x00400003`, `0x00800004`, `0x00c00005`, REQ-ai01).
const CODEC_SHIFT: u32 = 22;

impl Registry {
    /// Whether `ctx` was issued below `issued_below` and is not finalised.
    fn live(&self, ctx: u32, issued_below: u32) -> bool {
        (FIRST_CONTEXT..issued_below).contains(&ctx) && !self.finalised.contains(&ctx)
    }

    /// `sceAjmModuleRegister`'s answer, or `None` where nothing measured it: a context that is not
    /// live, or a codec past those measured.
    fn register(&mut self, ctx: u32, codec: u32, issued_below: u32) -> Option<u64> {
        if !self.live(ctx, issued_below) || codec >= MEASURED_CODECS {
            return None;
        }
        Some(if self.registered.insert((ctx, codec)) {
            0
        } else {
            ALREADY_REGISTERED
        })
    }

    /// `sceAjmFinalize`'s answer for a live context, which it finalises; `None` for any other.
    fn finalize(&mut self, ctx: u32, issued_below: u32) -> Option<u64> {
        if !self.live(ctx, issued_below) {
            return None;
        }
        self.registered.retain(|&(c, _)| c != ctx);
        self.instances.retain(|&(c, _)| c != ctx);
        self.finalised.insert(ctx);
        Some(0)
    }

    /// `sceAjmInstanceCreate`'s answer: the identifier to write, or the refusal for a codec `ctx`
    /// has not registered; `None` where nothing measured it - a context that is not live, a codec
    /// past those measured, or flags other than 0 to 2.
    ///
    /// Numbered from 1 per context, one more for each instance whatever its codec, with the codec
    /// in bits 22 and up. Measured on a single context; that a second context counts from 1 again
    /// is assumed.
    fn create(
        &mut self,
        ctx: u32,
        codec: u32,
        flags: u64,
        issued_below: u32,
    ) -> Option<Result<u32, u64>> {
        if !self.live(ctx, issued_below) || codec >= MEASURED_CODECS || flags >= MEASURED_FLAGS {
            return None;
        }
        if !self.registered.contains(&(ctx, codec)) {
            return Some(Err(NOT_REGISTERED));
        }
        let number = self.issued.entry(ctx).or_insert(0);
        *number += 1;
        let id = *number | codec << CODEC_SHIFT;
        self.instances.insert((ctx, id));
        Some(Ok(id))
    }

    /// `sceAjmBatchInitialize`'s answer for a buffer of the sizes measured, which starts the batch
    /// at that buffer afresh; `None` for any other size.
    fn batch_initialize(&mut self, buffer: u64, size: u64) -> Option<u64> {
        if !(SMALLEST_BATCH..=LARGEST_BATCH).contains(&size) {
            return None;
        }
        self.batches.insert(buffer, Vec::new());
        Some(0)
    }

    /// `sceAjmBatchJobDecode`'s answer: 0 for the first job on a batch, naming an instance some
    /// context holds; `None` for a batch never initialised, an instance not held, or a second
    /// job, none of which is measured.
    fn job_decode(&mut self, buffer: u64, instance: u32) -> Option<u64> {
        if !self.instances.iter().any(|&(_, id)| id == instance) {
            return None;
        }
        let jobs = self.batches.get_mut(&buffer)?;
        if !jobs.is_empty() {
            return None;
        }
        jobs.push(Job { instance });
        Some(0)
    }

    /// `sceAjmInstanceDestroy`'s answer on a live context: 0 for an instance it holds, which it
    /// ends, and the refusal for one it does not, such as one already destroyed.
    fn destroy(&mut self, ctx: u32, id: u32, issued_below: u32) -> Option<u64> {
        if !self.live(ctx, issued_below) {
            return None;
        }
        Some(if self.instances.remove(&(ctx, id)) {
            0
        } else {
            INVALID_INSTANCE
        })
    }
}

/// The process's AJM registry.
static REGISTRY: std::sync::Mutex<Option<Registry>> = std::sync::Mutex::new(None);

/// Runs `f` against the registry with the next context identifier, answering the placeholder
/// where it answers `None`.
fn with_registry(f: impl FnOnce(&mut Registry, u32) -> Option<u64>) -> u64 {
    let issued_below = NEXT_CONTEXT.load(Ordering::Relaxed);
    REGISTRY
        .lock()
        .ok()
        .and_then(|mut held| f(held.get_or_insert_with(Registry::default), issued_below))
        .unwrap_or_else(|| u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()))
}

/// `sceAjmModuleRegister(ctx, codec, 0)`: 0 the first time a live context registers one of codecs
/// 0 to 3, `0x80930009` after (REQ-aj01, `20261009-215800-eboot.log` 4282-4290).
pub(crate) fn ajm_module_register(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, below| r.register(args[0] as u32, args[1] as u32, below))
}

/// `sceAjmFinalize(ctx)`: 0 for a live context, which it ends (REQ-aj01, lines 4291-4297).
pub(crate) fn ajm_finalize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, below| r.finalize(args[0] as u32, below))
}

/// `sceAjmInstanceCreate(ctx, codec, flags, &out)`: writes the instance identifier as four bytes
/// and answers 0, the four after it left as they were; a codec the context has not registered
/// answers `0x8093000a` with the slot untouched (REQ-ai01,
/// `report-20261015-batch1.log`, `070-user/oneshot-setup-calls`). A null slot is unmeasured and
/// answers the placeholder.
pub(crate) fn ajm_instance_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[3];
    if out == 0 {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    with_registry(|r, below| {
        Some(
            match r.create(args[0] as u32, args[1] as u32, args[2], below)? {
                // SAFETY: the guest's out-parameter, by the call's contract; four bytes written, as
                // measured, through the checked accessor.
                Ok(id) if unsafe { orbistoun_mem::guest::write_u32(out, id) } => 0,
                Ok(_) => return None,
                Err(refusal) => refusal,
            },
        )
    })
}

/// `sceAjmBatchInitialize(buffer, size, &batch)`: writes the 32-byte header - zero but for `size`
/// at +8 - over the start of `buffer`, nothing past it, and the batch's identity to `batch`, and
/// answers 0, for the 0x40- and 0x1000-byte buffers measured (REQ-ai02,
/// `20261015-164000-eboot.log` 11333-11342).
///
/// The identity written is the buffer's own address. Assumed: the console's value is an address
/// that sat, in both arms, exactly where its buffer did - past the 960-byte input on the stack,
/// and at the end of the 9216-byte output on the heap - but no row names the buffer's address.
/// A null pointer and any other size are unmeasured and answer the placeholder.
pub(crate) fn ajm_batch_initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (buffer, size, batch) = (args[0], args[1], args[2]);
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    if buffer == 0 || batch == 0 || !(SMALLEST_BATCH..=LARGEST_BATCH).contains(&size) {
        return placeholder;
    }
    let mut header = [0_u8; BATCH_HEADER_BYTES];
    header[8..16].copy_from_slice(&size.to_le_bytes());
    // SAFETY: the guest's batch buffer, at least `size` bytes by the call's contract and so at
    // least the header; written through the checked accessor.
    if !unsafe { orbistoun_mem::guest::write_bytes(buffer, &header) } {
        return placeholder;
    }
    // SAFETY: the guest's batch slot, eight bytes by the call's contract; written through the
    // checked accessor.
    if !unsafe { orbistoun_mem::guest::write_u64(batch, buffer) } {
        return placeholder;
    }
    with_registry(|r, _| r.batch_initialize(buffer, size))
}

/// `sceAjmBatchJobDecode(&batch, instance, input, input_size, output, output_size)`: answers 0 for
/// the first job on a batch `sceAjmBatchInitialize` made, naming an instance that exists, as it did
/// for PPSA04263's shape - an MPEG audio frame of 960 bytes into 9216 (REQ-ai02, lines 11337-11344),
/// the output left as it was.
///
/// The console also writes a 56-byte job into the batch buffer; its fields are not all named by
/// the rows, so it is not written here, and nothing a title was seen to do reads it back. A batch
/// never made, an instance not held and a second job on one batch are unmeasured - the console
/// faulted on the second in a 0x40-byte batch - and answer the placeholder.
pub(crate) fn ajm_batch_job_decode(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's batch slot, which `sceAjmBatchInitialize` wrote; read through the
    // checked accessor.
    let Some(buffer) = (unsafe { orbistoun_mem::guest::read_u64(args[0]) }) else {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    };
    with_registry(|r, _| r.job_decode(buffer, args[1] as u32))
}

/// `sceAjmInstanceDestroy(ctx, id)`: 0 for an instance the context holds, `0x80930003` once it is
/// gone (REQ-ai01).
pub(crate) fn ajm_instance_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, below| r.destroy(args[0] as u32, args[1] as u32, below))
}

#[cfg(test)]
mod tests {
    use super::{ALREADY_REGISTERED, INVALID_INSTANCE, NOT_REGISTERED, Registry};

    /// `sceAjmInstanceCreate(ctx, codec, flags, &out)` refuses a codec the context has not
    /// registered, and otherwise numbers instances from 1 with the codec in bits 22 and up; a
    /// second destroy of one is refused (REQ-ai01, `report-20261015-batch1.log`,
    /// `070-user/oneshot-setup-calls`).
    #[test]
    fn instances_are_numbered_as_measured() {
        let mut registry = Registry::default();
        assert_eq!(registry.create(0x15, 0, 1, 0x16), Some(Err(NOT_REGISTERED)));
        assert_eq!(registry.register(0x15, 0, 0x16), Some(0));
        assert_eq!(registry.create(0x15, 0, 1, 0x16), Some(Ok(1)));
        assert_eq!(registry.create(0x15, 0, 1, 0x16), Some(Ok(2)));
        for codec in 1..4 {
            assert_eq!(registry.register(0x15, codec, 0x16), Some(0));
        }
        assert_eq!(registry.create(0x15, 1, 1, 0x16), Some(Ok(0x0040_0003)));
        assert_eq!(registry.create(0x15, 2, 1, 0x16), Some(Ok(0x0080_0004)));
        assert_eq!(registry.create(0x15, 3, 1, 0x16), Some(Ok(0x00c0_0005)));
        assert_eq!(registry.create(0x15, 0, 0, 0x16), Some(Ok(6)));
        assert_eq!(registry.create(0x15, 0, 2, 0x16), Some(Ok(7)));
        assert_eq!(registry.create(0x15, 0, 4, 0x16), None, "unmeasured flags");
        assert_eq!(
            registry.create(0x99, 0, 1, 0x16),
            None,
            "a context never issued"
        );
        assert_eq!(registry.destroy(0x15, 1, 0x16), Some(0));
        assert_eq!(registry.destroy(0x15, 1, 0x16), Some(INVALID_INSTANCE));
    }

    /// A batch takes one decode job for an instance that exists and answers 0; a job for an
    /// instance never created, on a batch never initialised, or a second job on a batch, is
    /// unmeasured - the console faulted on the second - and answers nothing here (REQ-ai02,
    /// `20261015-164000-eboot.log` 11333-11355).
    #[test]
    fn a_batch_takes_one_decode_job_as_measured() {
        let mut registry = Registry::default();
        assert_eq!(registry.register(0x15, 0, 0x16), Some(0));
        assert_eq!(registry.create(0x15, 0, 1, 0x16), Some(Ok(1)));
        assert_eq!(registry.batch_initialize(0x9000, 0x40), Some(0));
        assert_eq!(registry.job_decode(0x9000, 2), None, "no such instance");
        assert_eq!(registry.job_decode(0x9000, 1), Some(0));
        assert_eq!(registry.job_decode(0x9000, 1), None, "a second job");
        assert_eq!(registry.job_decode(0xa000, 1), None, "no such batch");
        assert_eq!(registry.batch_initialize(0x9000, 0x40), Some(0));
        assert_eq!(
            registry.job_decode(0x9000, 1),
            Some(0),
            "initialised afresh"
        );
        assert_eq!(
            registry.batch_initialize(0x9000, 0x20),
            None,
            "smaller than measured"
        );
    }

    /// `sceAjmModuleRegister(ctx, codec, 0)` answers 0 for codecs 0 to 3 and `0x80930009` when the
    /// codec is already registered; `sceAjmFinalize(ctx)` answers 0 (REQ-aj01,
    /// `20261009-215800-eboot.log` 4282-4297).
    #[test]
    fn modules_register_once_per_context_as_measured() {
        let mut registry = Registry::default();
        for codec in 0..4 {
            assert_eq!(registry.register(0x15, codec, 0x17), Some(0));
            assert_eq!(
                registry.register(0x15, codec, 0x17),
                Some(ALREADY_REGISTERED)
            );
        }
        assert_eq!(
            registry.register(0x99, 0, 0x17),
            None,
            "a context never issued"
        );
        assert_eq!(
            registry.register(0x15, 9, 0x17),
            None,
            "an unmeasured codec"
        );
        assert_eq!(registry.finalize(0x15, 0x17), Some(0));
        assert_eq!(registry.finalize(0x16, 0x17), Some(0));
        assert_eq!(
            registry.register(0x15, 0, 0x17),
            None,
            "a finalised context"
        );
    }
}
