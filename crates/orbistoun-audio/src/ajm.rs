//! `libSceAjm` - the platform's audio codec job manager: decode and encode work submitted as jobs.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. Context set-up, instances and a batch's set-up
//! are implemented, as measured; a started batch decodes its MPEG audio jobs on the host (D889).

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
        // (ctx, &batch, priority, 0, &out, ...), as PPSA04263 calls it.
        "sceAjmBatchStart" => 6,
        // (ctx, id, timeout, &block, ...), as PPSA04263 calls it.
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
    /// How many batches `sceAjmBatchStart` has handed back an identifier for: the next one's.
    started: u32,
    /// Started batches not yet waited on, by identifier.
    unwaited: std::collections::BTreeSet<u32>,
}

/// What a second `sceAjmBatchWait` on one batch answered: the first wait ended it (REQ-gt02,
/// `20261016-000300-eboot.log` 4961).
const BATCH_GONE: u64 = 0x8093_0004;

/// A decode job a batch holds: the instance it names, and its input and output as guest
/// `(address, size)` pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Job {
    /// The instance identifier the job was given.
    instance: u32,
    /// The encoded frames to decode.
    input: (u64, u64),
    /// Where the decoded samples go.
    output: (u64, u64),
    /// Where the job's result goes - the seventh argument, from the stack - or 0 for none.
    sideband: u64,
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

/// Where an instance identifier carries its codec: bits 14 and up. Codecs 1 to 3 measured
/// `0x4003`, `0x8004` and `0xc005` (REQ-ai01, slots `03400000`, `04800000`, `05c00000` read as
/// little-endian words), and codec 1 `0x4004`, `0x4006` and `0x4008` (REQ-au01, `at9-inst-flag*`).
const CODEC_SHIFT: u32 = 14;

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
    /// in bits 14 and up. Measured on a single context; that a second context counts from 1 again
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
    fn job_decode(&mut self, buffer: u64, job: Job) -> Option<u64> {
        if !self.instances.iter().any(|&(_, id)| id == job.instance) {
            return None;
        }
        let jobs = self.batches.get_mut(&buffer)?;
        if !jobs.is_empty() {
            return None;
        }
        jobs.push(job);
        Some(0)
    }

    /// Starts the batch at `buffer`: the identifier it is waited on by - 0 for the first, as
    /// measured (REQ-gt02, line 4957), then one more each - and the jobs to run, which leave the
    /// batch. `None` for a batch never initialised or one holding no job (D889).
    fn start(&mut self, buffer: u64) -> Option<(u32, Vec<Job>)> {
        let jobs = std::mem::take(self.batches.get_mut(&buffer)?);
        if jobs.is_empty() {
            return None;
        }
        let id = self.started;
        self.started += 1;
        self.unwaited.insert(id);
        Some((id, jobs))
    }

    /// `sceAjmBatchWait`'s answer for a batch identifier `start` handed back: 0 the first time,
    /// since a batch runs to completion when it is started (D889), and [`BATCH_GONE`] after, as
    /// measured. `None` for an identifier never handed back.
    fn wait(&mut self, id: u32) -> Option<u64> {
        if self.unwaited.remove(&id) {
            Some(0)
        } else {
            (id < self.started).then_some(BATCH_GONE)
        }
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
    let job = Job {
        instance: args[1] as u32,
        input: (args[2], args[3]),
        output: (args[4], args[5]),
        sideband: seventh_argument(),
    };
    with_registry(|r, _| r.job_decode(buffer, job))
}

/// The seventh argument of the call in progress, from the guest's stack, or 0 when the dispatch
/// published no stack area (a direct call, as in a test).
fn seventh_argument() -> u64 {
    let spilled = orbistoun_thunk::stack_arguments();
    if spilled == 0 {
        return 0;
    }
    // SAFETY: the dispatch published this as the words above the return address the guest's call
    // pushed, on the calling thread's live stack.
    unsafe { orbistoun_mem::guest::read_u64(spilled) }.unwrap_or_default()
}

/// A decode job's result as the title reads it back after `sceAjmBatchWait`: the result word and
/// an internal one (0, 0), then the input bytes consumed, the output bytes written and the samples
/// decoded per channel. PPSA04263 tests the result word against `0x80000002`, adds the bytes
/// consumed to its read cursor and the samples to its stream position (`image+0x2ad25b8` on);
/// without them each stream decodes its first frames again for as long as it plays. The layout
/// is the title's reads, `assumed` for the fields it does not read.
fn sideband(consumed: u32, written: u32, samples: u64) -> [u8; 24] {
    let mut bytes = [0_u8; 24];
    bytes[8..12].copy_from_slice(&consumed.to_le_bytes());
    bytes[12..16].copy_from_slice(&written.to_le_bytes());
    bytes[16..24].copy_from_slice(&samples.to_le_bytes());
    bytes
}

/// What decoding a job's input made: the interleaved samples, how many channels they hold, and how
/// many input bytes the whole frames decoded took.
#[derive(Debug, PartialEq, Eq)]
struct Decoded {
    /// Interleaved 16-bit PCM.
    samples: Vec<i16>,
    /// Channels in each sample frame.
    channels: usize,
    /// Input bytes up to the end of the last frame read.
    consumed: usize,
}

/// Layer III bitrates in kbit/s by header index, MPEG-1 then MPEG-2 and 2.5 (ISO/IEC 11172-3
/// 2.4.2.3, 13818-3 2.4.2.3); index 0 is the free format and 15 is forbidden, so neither has a
/// length.
const LAYER3_KBITS: [[u32; 15]; 2] = [
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// MPEG-1 sample rates by header index (11172-3 2.4.2.3); MPEG-2 halves them and 2.5 quarters
/// them (13818-3 2.4.2.3).
const MPEG1_RATES: [u32; 3] = [44_100, 48_000, 32_000];

/// How long the Layer III frame a four-byte header begins is, in bytes: `None` for a header that
/// is not one, or that names the free format or a reserved field.
fn frame_length(header: [u8; 4]) -> Option<usize> {
    let word = u32::from_be_bytes(header);
    let sync = word >> 21 == 0x7ff;
    let version = (word >> 19) & 3;
    let layer = (word >> 17) & 3;
    let bitrate = ((word >> 12) & 0xf) as usize;
    let rate = ((word >> 10) & 3) as usize;
    let padding = (word >> 9) & 1;
    // Layer III is `01`; version `01` is reserved.
    if !sync || layer != 1 || version == 1 || bitrate == 0 || bitrate == 15 || rate == 3 {
        return None;
    }
    let (table, divisor, slots) = match version {
        3 => (0, 1, 144),
        2 => (1, 2, 72),
        _ => (1, 4, 72),
    };
    let kbits = LAYER3_KBITS[table][bitrate];
    let sample_rate = MPEG1_RATES[rate] / divisor;
    usize::try_from(slots * kbits * 1000 / sample_rate + padding).ok()
}

/// Each instance's decoder, by instance identifier, kept across batches; see [`new_decoder`].
static DECODERS: std::sync::Mutex<
    std::collections::BTreeMap<u32, symphonia_bundle_mp3::MpaDecoder>,
> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// A Layer III decoder, kept for each instance from one batch to the next: a frame may take its
/// main data from the frames before it (11172-3 2.4.3.4, the bit reservoir), and those can be in
/// the batch before.
fn new_decoder() -> Option<symphonia_bundle_mp3::MpaDecoder> {
    use symphonia_core::codecs::Decoder as _;
    let mut params = symphonia_core::codecs::CodecParameters::new();
    params.for_codec(symphonia_core::codecs::CODEC_TYPE_MP3);
    symphonia_bundle_mp3::MpaDecoder::try_new(
        &params,
        &symphonia_core::codecs::DecoderOptions::default(),
    )
    .ok()
}

/// Decodes the Layer III frames in `input` with `decoder`, to interleaved 16-bit PCM, every frame
/// in turn (D889); `None` if no frame decodes. A frame is found by its header and measured by
/// [`frame_length`], and the bytes from one that does not parse to the next sync are skipped.
fn decode_frames(decoder: &mut symphonia_bundle_mp3::MpaDecoder, input: &[u8]) -> Option<Decoded> {
    use symphonia_core::codecs::Decoder as _;

    let mut samples = Vec::new();
    let mut channels = 1;
    let mut consumed = 0;
    let mut at = 0;
    while let Some(header) = input.get(at..at + 4) {
        let length = frame_length([header[0], header[1], header[2], header[3]]);
        let Some(frame) = length.and_then(|length| input.get(at..at + length)) else {
            at += 1;
            continue;
        };
        at += frame.len();
        consumed = at;
        let packet = symphonia_core::formats::Packet::new_from_slice(0, 0, 0, frame);
        let Ok(pcm) = decoder.decode(&packet) else {
            continue;
        };
        let mut buffer =
            symphonia_core::audio::SampleBuffer::<i16>::new(pcm.capacity() as u64, *pcm.spec());
        channels = pcm.spec().channels.count().max(1);
        buffer.copy_interleaved_ref(pcm);
        samples.extend_from_slice(buffer.samples());
    }
    (!samples.is_empty()).then_some(Decoded {
        samples,
        channels,
        consumed,
    })
}

/// Runs one decode job: reads its input from guest memory, decodes it, and writes as much of the
/// 16-bit PCM as fits in its output. A job whose input cannot be read, or holds no frame, leaves
/// its output as it was (D889).
fn run_job(job: &Job) {
    let (Ok(input_size), Ok(output_size)) =
        (usize::try_from(job.input.1), usize::try_from(job.output.1))
    else {
        return;
    };
    let mut input = vec![0_u8; input_size];
    // SAFETY: the guest's input frames, `input_size` bytes by the job's contract; read through the
    // checked accessor.
    if !unsafe { orbistoun_mem::guest::read_into(job.input.0, &mut input) } {
        return;
    }
    let samples = {
        let mut decoders = DECODERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decoder = match decoders.entry(job.instance) {
            std::collections::btree_map::Entry::Occupied(held) => held.into_mut(),
            std::collections::btree_map::Entry::Vacant(slot) => match new_decoder() {
                Some(decoder) => slot.insert(decoder),
                None => return,
            },
        };
        decode_frames(decoder, &input)
    };
    let Some(decoded) = samples else {
        return;
    };
    let pcm: Vec<u8> = decoded
        .samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .take(output_size)
        .collect();
    // SAFETY: the guest's output, `output_size` bytes by the job's contract, and no more written;
    // through the checked accessor.
    let _ = unsafe { orbistoun_mem::guest::write_bytes(job.output.0, &pcm) };
    if job.sideband != 0 {
        let frame = 2 * decoded.channels;
        let result = sideband(
            u32::try_from(decoded.consumed).unwrap_or(u32::MAX),
            u32::try_from(pcm.len()).unwrap_or(u32::MAX),
            (pcm.len() / frame) as u64,
        );
        // SAFETY: the result block the title passed with the job; written through the checked
        // accessor.
        let _ = unsafe { orbistoun_mem::guest::write_bytes(job.sideband, &result) };
    }
}

/// `sceAjmBatchStart(ctx, &batch, priority, 0, &out, ...)`: runs the batch's decode job to
/// completion, writes the batch's identifier to the first four bytes of `out`, and answers 0, as
/// the console did in PPSA04263's shape - identifier 0 for the first batch, the rest of `out` left
/// (REQ-gt02, `20261016-000300-eboot.log` 4956-4957). Running the job at the start is D889's.
/// PPSA04263 waits on what `out` holds after it. A batch never initialised or holding no job, and
/// a null `out`, answer the placeholder.
pub(crate) fn ajm_batch_start(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let placeholder = u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    let out = args[4];
    // SAFETY: the guest's batch slot, which `sceAjmBatchInitialize` wrote; read through the
    // checked accessor.
    let Some(buffer) = (unsafe { orbistoun_mem::guest::read_u64(args[1]) }) else {
        return placeholder;
    };
    if out == 0 {
        return placeholder;
    }
    let Some((id, jobs)) = REGISTRY
        .lock()
        .ok()
        .and_then(|mut held| held.get_or_insert_with(Registry::default).start(buffer))
    else {
        return placeholder;
    };
    for job in &jobs {
        run_job(job);
    }
    // SAFETY: the guest's out block, its first four bytes the batch identifier; written through
    // the checked accessor.
    if unsafe { orbistoun_mem::guest::write_u32(out, id) } {
        0
    } else {
        placeholder
    }
}

/// `sceAjmBatchWait(ctx, id, timeout, &block, ...)`: 0 the first time for a batch
/// `sceAjmBatchStart` started, which ran to completion as it started (D889), the block left as it
/// was; `0x80930004` after, as the console answered a second wait (REQ-gt02, line 4961). An
/// identifier never handed back answers the placeholder.
pub(crate) fn ajm_batch_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, _| r.wait(args[1] as u32))
}

/// `sceAjmInstanceDestroy(ctx, id)`: 0 for an instance the context holds, `0x80930003` once it is
/// gone (REQ-ai01).
pub(crate) fn ajm_instance_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let answer = with_registry(|r, below| r.destroy(args[0] as u32, args[1] as u32, below));
    if answer == 0 {
        DECODERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(args[1] as u32));
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::{ALREADY_REGISTERED, INVALID_INSTANCE, NOT_REGISTERED, Registry};

    /// `sceAjmInstanceCreate(ctx, codec, flags, &out)` refuses a codec the context has not
    /// registered, and otherwise numbers instances from 1 with the codec in bits 14 and up; a
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
        assert_eq!(registry.create(0x15, 1, 1, 0x16), Some(Ok(0x4003)));
        assert_eq!(registry.create(0x15, 2, 1, 0x16), Some(Ok(0x8004)));
        assert_eq!(registry.create(0x15, 3, 1, 0x16), Some(Ok(0xc005)));
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

    /// A job naming `instance`, its input and output nowhere: the registry only keeps them.
    fn job(instance: u32) -> super::Job {
        super::Job {
            instance,
            input: (0x1_0000, 960),
            output: (0x2_0000, 9216),
            sideband: 0,
        }
    }

    /// Four silent frames in PPSA04263's shape - MPEG-1 Layer III, mono, 48 kHz, 80 kbit/s, no
    /// CRC, 240 bytes each, 960 in all - decode to 4 x 1152 samples of silence: 9216 bytes of
    /// 16-bit PCM, the output size the title passes (D889).
    #[test]
    fn four_silent_frames_decode_to_the_titles_output_size() {
        let mut input = Vec::new();
        for _ in 0..4 {
            let mut frame = vec![0_u8; 240];
            frame[..4].copy_from_slice(&[0xff, 0xfb, 0x64, 0xc0]);
            input.extend_from_slice(&frame);
        }
        let mut decoder = super::new_decoder().expect("a Layer III decoder");
        let frames = super::decode_frames(&mut decoder, &input).expect("the frames decode");
        assert_eq!(frames.samples.len() * 2, 9216);
        assert!(frames.samples.iter().all(|&s| s == 0));
        assert_eq!((frames.channels, frames.consumed), (1, 960));
        assert_eq!(
            super::decode_frames(&mut decoder, &[0_u8; 960]),
            None,
            "no frame in it"
        );
    }

    /// A job's result says what it took and made, at the offsets PPSA04263 reads: the result word
    /// 0, then bytes consumed at 8, bytes written at 12 and samples at 16.
    #[test]
    fn a_result_says_what_the_job_took_and_made() {
        let bytes = super::sideband(960, 9216, 4608);
        assert_eq!(bytes[..8], [0; 8]);
        assert_eq!(bytes[8..12], 960_u32.to_le_bytes());
        assert_eq!(bytes[12..16], 9216_u32.to_le_bytes());
        assert_eq!(bytes[16..24], 4608_u64.to_le_bytes());
    }

    /// A frame's length comes from its header (ISO/IEC 11172-3 2.4.3.1, 13818-3 2.4.2.3): 144
    /// x bitrate / sample rate for MPEG-1 Layer III, 72 x for MPEG-2, plus a padding byte when the
    /// header asks for one; a header that is not Layer III, or names a free or bad bitrate or rate,
    /// has none.
    #[test]
    fn a_frame_is_as_long_as_its_header_says() {
        assert_eq!(super::frame_length([0xff, 0xfb, 0x64, 0xc0]), Some(240));
        assert_eq!(
            super::frame_length([0xff, 0xfa, 0x64, 0xc0]),
            Some(240),
            "with a CRC"
        );
        assert_eq!(
            super::frame_length([0xff, 0xfb, 0x90, 0x00]),
            Some(417),
            "128k at 44.1k"
        );
        assert_eq!(
            super::frame_length([0xff, 0xfb, 0x92, 0x00]),
            Some(418),
            "padded"
        );
        assert_eq!(
            super::frame_length([0xff, 0xf3, 0x90, 0x00]),
            Some(261),
            "MPEG-2, 80k at 22.05k"
        );
        assert_eq!(
            super::frame_length([0xff, 0xfb, 0x04, 0x00]),
            None,
            "free bitrate"
        );
        assert_eq!(
            super::frame_length([0xff, 0xfd, 0x90, 0x00]),
            None,
            "Layer II"
        );
        assert_eq!(
            super::frame_length([0x00, 0x00, 0x00, 0x00]),
            None,
            "no sync"
        );
    }

    /// Starting a batch hands back an identifier and the jobs to run, once; waiting on the
    /// identifier answers 0 once it has run, and an identifier never handed back is unmeasured
    /// (D889).
    #[test]
    fn a_started_batch_is_waited_on_by_its_identifier() {
        let mut registry = Registry::default();
        assert_eq!(registry.register(0x15, 0, 0x16), Some(0));
        assert_eq!(registry.create(0x15, 0, 1, 0x16), Some(Ok(1)));
        assert_eq!(registry.batch_initialize(0x9000, 0x40), Some(0));
        assert_eq!(registry.start(0x9000), None, "nothing to run");
        assert_eq!(registry.job_decode(0x9000, job(1)), Some(0));
        let (id, jobs) = registry.start(0x9000).expect("one job to run");
        assert_eq!(id, 0, "the first batch is 0 (REQ-gt02)");
        assert_eq!(jobs, vec![job(1)]);
        assert_eq!(registry.wait(id), Some(0));
        assert_eq!(
            registry.wait(id),
            Some(0x8093_0004),
            "a batch waited on is gone (REQ-gt02)"
        );
        assert_eq!(registry.wait(id + 1), None);
        let (next, _) = {
            assert_eq!(registry.batch_initialize(0x9000, 0x40), Some(0));
            assert_eq!(registry.job_decode(0x9000, job(1)), Some(0));
            registry.start(0x9000).expect("a second batch")
        };
        assert_eq!(next, id + 1);
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
        assert_eq!(
            registry.job_decode(0x9000, job(2)),
            None,
            "no such instance"
        );
        assert_eq!(registry.job_decode(0x9000, job(1)), Some(0));
        assert_eq!(registry.job_decode(0x9000, job(1)), None, "a second job");
        assert_eq!(registry.job_decode(0xa000, job(1)), None, "no such batch");
        assert_eq!(registry.batch_initialize(0x9000, 0x40), Some(0));
        assert_eq!(
            registry.job_decode(0x9000, job(1)),
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
