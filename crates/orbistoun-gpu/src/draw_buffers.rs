//! A draw's buffers resolved from guest memory (D733).
//!
//! The translator names each buffer a module reads through by where its descriptor or base comes
//! from ([`BufferSource`]); per draw, this finds the guest range each names - from the draw's user
//! data, constants and the descriptor-table words it reads - and the bytes the descriptor's bounds
//! rules let the module reach, and hands them to the backend as a
//! [`RenderCommand::BindDrawBuffers`]. A range that cannot be bound exactly is refused by name.

use std::collections::BTreeMap;

use crate::perf::{Span, span};

use orbistoun_translate::draw_buffers::{
    BufferSource, Computed, ComputedAccess, DescriptorReads, DescriptorWord, GlobalLoads,
    RecordIndex,
};
use orbistoun_translate::wavefront::{TableBase, TableWord};

use crate::backend::{
    DrawBuffer, IndexBuffer, RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS,
};
use crate::pipeline::GuestMemory;
use crate::registers::decode_buffer_descriptor;

/// Why a draw buffer could not be bound: a fixed reason, or one naming what the program did.
type Refusal = std::borrow::Cow<'static, str>;

mod resolved;
pub(crate) mod tables;

/// The bytes from a descriptor's base every read the program makes through it can reach, as the
/// descriptor's bounds rules admit them, or why nothing bounds them.
///
/// # Errors
///
/// A refusal naming what leaves the reads unbounded.
pub fn descriptor_extent(words: [u32; 4], reads: DescriptorReads) -> Result<u64, &'static str> {
    descriptor_extent_to(words, reads, None)
}

/// [`descriptor_extent`] for a draw whose indexed accesses reach no record past `last_index`
/// (D879): the bytes through that record, or through the descriptor's last where it has fewer.
///
/// # Errors
///
/// A refusal naming what leaves the reads unbounded.
pub fn descriptor_extent_to(
    words: [u32; 4],
    reads: DescriptorReads,
    last_index: Option<u64>,
) -> Result<u64, &'static str> {
    const UNBOUNDED: &str = concat!(
        "a buffer read at a register offset or record index its descriptor's out-of-bounds mode ",
        "does not limit, so no range holds every read (D733)"
    );
    let descriptor = decode_buffer_descriptor(words);
    if descriptor.unsupported {
        return Err("a buffer descriptor asking for swizzled or thread-indexed addressing (D733)");
    }
    // Which descriptor leaves its reads unbounded, said once for each, so its mode is read rather
    // than guessed.
    let unbounded = || {
        static SAID: std::sync::Mutex<Vec<[u32; 4]>> = std::sync::Mutex::new(Vec::new());
        if let Ok(mut said) = SAID.lock()
            && said.len() < 16
            && !said.contains(&words)
        {
            said.push(words);
            tracing::info!(
                "a buffer descriptor leaves its reads unbounded: words {words:#010x?}, out-of-bounds mode {}, {reads:?}",
                (words[3] >> 28) & 3
            );
        }
        UNBOUNDED
    };
    let (stride, records) = (u64::from(descriptor.stride), u64::from(descriptor.records));
    // A scalar buffer load reads each word within the record count, which is in bytes with no
    // stride and in strides with one (the translator's `scalar_buffer_load`; radeonsi
    // `si_state.c:3506-3527`).
    let scalar = if stride == 0 {
        records
    } else {
        records * stride
    }
    .min(u64::from(reads.scalar_reach));
    if !reads.vector {
        return Ok(scalar);
    }
    let reach = reads.vector_reach.map(u64::from);
    // Where the last record an access can index starts: the last the draw's ids reach where they
    // bound it (D879), else every record's when it indexes, and the first's when not.
    let last_record = if reads.indexed {
        let last = records.saturating_sub(1);
        last_index.map_or(last, |index| index.min(last)) * stride
    } else {
        0
    };
    // The four out-of-bounds modes (`OOB_SELECT`, bits 29:28 of the fourth word, Mesa
    // `gfx10-rsrc.json`), as the translator's `buffer_out_of_bounds` applies them. An access reads
    // at most sixteen bytes from its offset.
    let vector = match (words[3] >> 28) & 3 {
        // Structured with offset: the index below the record count and the offset below the stride.
        0 if records == 0 || stride == 0 => 0,
        0 => last_record + reach.unwrap_or(u64::MAX).min(stride - 1 + 16),
        // Structured: the index below the record count, the offset unchecked. With no stride nothing
        // is bounded on hardware (D917, measured), and the host binds the record count in bytes: the
        // table the descriptor names, which the program is taken to read within (assumed).
        1 if records == 0 => 0,
        1 if stride == 0 => reach.map_or(records, |reach| reach.min(records)),
        // With a stride and no reach known for the offset, the program is taken to read within its
        // record, as a vertex fetch does: through the last record's stride (D946, assumed).
        1 => last_record + reach.unwrap_or(stride),
        // Raw: the offset and the access's width within the record count in bytes, the index
        // unchecked.
        3 if reads.indexed && stride != 0 => return Err(unbounded()),
        3 => reach.map_or(records, |reach| reach.min(records)),
        // Only a record count of zero is out of bounds.
        _ if records == 0 => 0,
        _ if reads.indexed && stride != 0 => return Err(unbounded()),
        _ => reach.ok_or_else(unbounded)?,
    };
    Ok(scalar.max(vector))
}

/// The bytes of the `length` at `address` that guest memory holds: all of them, or the longest
/// mapped run of whole words from `address` (D795). `None` when not even a word is mapped there.
fn mapped_prefix(memory: &impl GuestMemory, address: u64, length: usize) -> Option<&[u8]> {
    if let Some(bytes) = memory.read(address, length) {
        return Some(bytes);
    }
    // A mapping holds a contiguous run, so a readable prefix is found by halving: `held` is
    // readable (or zero), `past` is not.
    let (mut held, mut past) = (0_usize, length);
    while past - held > 4 {
        let middle = (held + (past - held) / 2) & !3;
        if middle <= held {
            break;
        }
        if memory.read(address, middle).is_some() {
            held = middle;
        } else {
            past = middle;
        }
    }
    if held == 0 {
        return None;
    }
    memory.read(address, held)
}

/// The most bytes one draw buffer binds; a descriptor naming more is refused rather than read.
pub(crate) const MOST_BYTES: u64 = 256 << 20;

/// How many bytes [`BufferCache`] keeps, the oldest ranges going first past it: a world's vertex
/// buffers, which a frame draws from again, and a bound for a guest streaming new ones every frame.
const CACHE_BYTES: usize = 256 << 20;

/// How many stale places [`BufferCache`]'s order holds beyond twice its entries before they are
/// cleared out.
const STALE_PLACES: usize = 1024;

/// The most bytes a draw buffer has for [`BufferCache`] to compare it with what it kept rather
/// than ask the host whether it was written: reading and comparing this many costs less than the
/// asking.
const COMPARED_BYTES: usize = 16 << 10;

/// Why a draw buffer with nothing mapped at its base binds nothing.
const UNMAPPED: &str = concat!(
    "a draw buffer's range has no guest memory mapped at its base, so the bytes its ",
    "reads reach are not there to bind (D733)"
);

/// `bytes` as a draw buffer at `address`: whole words, the tail past the bytes held zero - the
/// module never reads a word the range does not start - and hashed. Only to the word the held bytes
/// end in, so a range cut at its mapping (D795) is not filled out with zeros the guest never wrote.
fn padded_buffer(address: u64, bytes: &[u8]) -> DrawBuffer {
    // Laid down in the shared slice itself, allocated once whole: made as a vector and then
    // shared, a range was two fresh allocations and a copy between them, and an allocation of
    // megabytes comes from the system a page at a time (MCUB00001, its ring re-read every frame).
    let mut padded: std::sync::Arc<[u8]> =
        std::iter::repeat_n(0_u8, bytes.len().div_ceil(4) * 4).collect();
    if let Some(into) = std::sync::Arc::get_mut(&mut padded) {
        into[..bytes.len()].copy_from_slice(bytes);
    }
    let mut hasher = crate::ContentHasher::new(padded.len() / 4);
    hasher.bytes(&padded);
    DrawBuffer {
        hash: hasher.finish(),
        bytes: padded,
        base: address,
        stored: false,
    }
}

/// Guest bytes read for draws, by where they lie and how many, kept across submissions while the
/// host's write tracking reports them unwritten, as texels are - or, for a small range, while they
/// compare equal.
#[derive(Debug, Default)]
pub struct BufferCache {
    /// Each range's bytes, when it was marked, and the generation it was kept at.
    entries: std::collections::HashMap<(u64, usize), (Option<u64>, DrawBuffer, u64)>,
    /// The ranges in the order they were kept, each with its generation; one whose entry was kept
    /// again since is a stale place and is skipped.
    order: std::collections::VecDeque<((u64, usize), u64)>,
    /// The bytes the entries hold.
    bytes: usize,
    /// The next generation to keep a range at.
    generation: u64,
}

impl BufferCache {
    /// The bytes of guest buffers kept.
    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    /// The `length` bytes at `address`, from the cache while nothing has written them.
    fn read(
        &mut self,
        address: u64,
        length: usize,
        memory: &impl GuestMemory,
    ) -> Result<DrawBuffer, &'static str> {
        // Nothing to read: a buffer no lane reaches (D758).
        if length == 0 {
            return Ok(DrawBuffer {
                hash: crate::ContentHasher::new(0).finish(),
                bytes: Vec::new().into(),
                base: address,
                stored: false,
            });
        }
        let span_of = length as u64;
        // A small range is compared rather than watched: asking the host whether it was written is
        // a system call or two, some microseconds, where reading and comparing a constant buffer's
        // few hundred bytes takes a fraction of one - and a guest rewrites those every frame.
        if length <= COMPARED_BYTES {
            return span(Span::BufferCompare, || {
                self.read_compared(address, length, memory)
            });
        }
        if let Some((since, buffer, _)) = self.entries.get(&(address, length))
            && span(Span::BufferWatch, || {
                since.and_then(|since| orbistoun_mem::watch::written_since(address, span_of, since))
            }) == Some(false)
        {
            return Ok(buffer.clone());
        }
        span(Span::BufferRead, || {
            // Marked before the bytes are hashed, so a write racing the hash is seen next time.
            let since = orbistoun_mem::watch::mark(address, span_of);
            let bytes = mapped_prefix(memory, address, length).ok_or(UNMAPPED)?;
            let buffer = padded_buffer(address, bytes);
            self.keep((address, length), since, buffer.clone());
            Ok(buffer)
        })
    }

    /// A small range's bytes, read now and kept as the same buffer, hash and all, while they are
    /// the bytes kept before.
    fn read_compared(
        &mut self,
        address: u64,
        length: usize,
        memory: &impl GuestMemory,
    ) -> Result<DrawBuffer, &'static str> {
        let bytes = mapped_prefix(memory, address, length).ok_or(UNMAPPED)?;
        if let Some((_, buffer, _)) = self.entries.get(&(address, length))
            && buffer.bytes.get(..bytes.len()) == Some(bytes)
            && buffer.bytes.len() == bytes.len().div_ceil(4) * 4
        {
            return Ok(buffer.clone());
        }
        let buffer = padded_buffer(address, bytes);
        self.keep((address, length), None, buffer.clone());
        Ok(buffer)
    }

    /// Keeps `buffer` for its range, letting the oldest ranges go while the cache holds more than
    /// [`CACHE_BYTES`].
    fn keep(&mut self, range: (u64, usize), since: Option<u64>, buffer: DrawBuffer) {
        let size = buffer.bytes.len();
        if let Some((_, old, _)) = self.entries.remove(&range) {
            self.bytes -= old.bytes.len();
        }
        self.generation += 1;
        self.entries.insert(range, (since, buffer, self.generation));
        self.order.push_back((range, self.generation));
        self.bytes += size;
        // A range kept again leaves a stale place behind, and a set rewritten every frame within
        // the bytes never reaches the eviction below: the stale places go once they outnumber the
        // entries.
        if self.order.len() > 2 * self.entries.len() + STALE_PLACES {
            let entries = &self.entries;
            self.order.retain(|(range, generation)| {
                entries.get(range).is_some_and(|e| e.2 == *generation)
            });
        }
        while self.bytes > CACHE_BYTES {
            let Some((oldest, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&oldest)
                .is_some_and(|entry| entry.2 == generation)
                && let Some((_, gone, _)) = self.entries.remove(&oldest)
            {
                self.bytes -= gone.bytes.len();
            }
        }
    }
}

/// The value one descriptor word or address half has for a draw with these user-data words.
pub(crate) fn resolve_word(
    word: DescriptorWord,
    user_data: &[u32; USER_DATA_WORDS],
    memory: &impl GuestMemory,
) -> Result<u32, &'static str> {
    match word {
        DescriptorWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| user_data.get(index).copied())
            .ok_or("a buffer descriptor word from a user-data word the stage does not have"),
        DescriptorWord::Constant(value) => Ok(value),
        DescriptorWord::Loaded { table, offset } => {
            let at = table_address(table, user_data).wrapping_add(u64::from(offset));
            let bytes = memory
                .read(at, 4)
                .ok_or("a buffer descriptor read from a table that is not mapped (D733)")?;
            Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
    }
}

/// What a computed access reads through, and its scalar offset, for a draw with these user-data
/// words (D753): the program before the access, run over them, the system registers `system` seeds
/// and guest memory. A descriptor's four words, or a scalar load's base pair in the first two;
/// `None` where the program branches past the access, which then reads through nothing (D800).
///
/// # Errors
///
/// A refusal naming why the prefix leaves what it reads through or the offset unknown.
pub(crate) fn evaluate_computed(
    computed: &Computed,
    (user_data, system): (&[u32; USER_DATA_WORDS], &[(u32, u32)]),
    memory: &impl GuestMemory,
) -> Evaluation {
    let count = usize::try_from(computed.user_data)
        .unwrap_or(USER_DATA_WORDS)
        .min(USER_DATA_WORDS);
    let user_data = &user_data[..count];
    let read = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let fingerprint = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (computed, user_data, system).hash(&mut hasher);
        hasher.finish()
    };
    let kept = evaluated().lock().ok().and_then(|evaluated| {
        evaluated
            .answers
            .get(&fingerprint)?
            .iter()
            .find(|kept| {
                *kept.computed == *computed
                    && kept.user_data == user_data
                    && kept.system == system
                    && kept
                        .reads
                        .iter()
                        .all(|(at, held)| memory.read(*at, held.len()) == Some(held.as_slice()))
            })
            .map(|kept| kept.answer.clone())
    });
    if let Some(answer) = kept {
        return answer;
    }
    let mut reads = Vec::new();
    // The replay reads a table a word at a time, eight or sixteen together for one scalar load,
    // and each read asks where guest memory lies and what is deferred over it: a line of words is
    // read once and the rest served from it, guest memory standing still while the replay runs
    // (PPSA04263, 27 reads an evaluation, 0.5 us each).
    let mut line: Option<(u64, [u8; READ_LINE])> = None;
    let answer = span(Span::BufferEvaluate, || {
        evaluate_computed_now(computed, (user_data, system), &mut |address| {
            let held = word_through_line(&mut line, address, memory).or_else(|| read(address))?;
            reads.push((address, held));
            Some(held)
        })
    });
    let program = interned(computed);
    if let Ok(mut evaluated) = evaluated().lock() {
        evaluated.keep(
            fingerprint,
            Evaluated {
                computed: program,
                user_data: user_data.to_vec(),
                system: system.to_vec(),
                reads: runs_of(&reads),
                answer: answer.clone(),
            },
        );
    }
    answer
}

/// The bytes of guest memory a replay reads at once, from an address a multiple of them.
const READ_LINE: usize = 64;

/// The word at `address` from `line`, the line holding it read into it first when it is another;
/// `None` where the aligned line cannot be read whole, and the word is read on its own.
fn word_through_line(
    line: &mut Option<(u64, [u8; READ_LINE])>,
    address: u64,
    memory: &impl GuestMemory,
) -> Option<u32> {
    let base = address & !(READ_LINE as u64 - 1);
    let offset = usize::try_from(address - base).ok()?;
    if offset + 4 > READ_LINE {
        return None;
    }
    if line.as_ref().is_none_or(|(at, _)| *at != base) {
        let bytes: [u8; READ_LINE] = memory.read(base, READ_LINE)?.try_into().ok()?;
        *line = Some((base, bytes));
    }
    let (_, bytes) = line.as_ref()?;
    Some(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

/// What [`evaluate_computed`] answers.
pub(crate) type Evaluation = Result<Option<([u32; 4], Option<u32>)>, Refusal>;

/// An answer [`evaluate_computed`] gave, with what it was asked and each guest word its replay read
/// and what that held: asked the same again, it holds while every one of those words does. A
/// title's draws compute their descriptors from tables that stay put from frame to frame, and
/// replaying the program for each was 30 us a draw (PPSA02664, a sixth of a run), half of them
/// asked before.
struct Evaluated {
    /// The program and access, shared with every answer kept for them ([`interned`]).
    computed: std::sync::Arc<Computed>,
    user_data: Vec<u32>,
    system: Vec<(u32, u32)>,
    /// The words the replay read, as runs of consecutive bytes, each checked again with one read.
    reads: Vec<(u64, Vec<u8>)>,
    answer: Evaluation,
}

/// The words a replay read, as runs of consecutive bytes from each run's first address: a
/// descriptor's four words are one run, checked again with one read rather than four - each read
/// asks where guest memory lies and whether anything is deferred over it (PPSA25872, a tenth of
/// its command processor's time). A word read again inside a run is not added twice.
fn runs_of(reads: &[(u64, u32)]) -> Vec<(u64, Vec<u8>)> {
    let mut runs: Vec<(u64, Vec<u8>)> = Vec::new();
    for &(address, word) in reads {
        if runs.iter().any(|(at, bytes)| {
            address >= *at && address.saturating_add(4) <= at.saturating_add(bytes.len() as u64)
        }) {
            continue;
        }
        match runs.last_mut() {
            Some((at, bytes)) if at.saturating_add(bytes.len() as u64) == address => {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            _ => runs.push((address, word.to_le_bytes().to_vec())),
        }
    }
    runs
}

/// The answers [`evaluate_computed`] keeps, by a hash of what each was asked, and the order they
/// were kept in.
#[derive(Default)]
struct EvaluatedAnswers {
    answers: std::collections::HashMap<u64, Vec<Evaluated>>,
    order: std::collections::VecDeque<u64>,
}

impl EvaluatedAnswers {
    /// Keeps `answer` under `fingerprint`, letting the oldest answers go past [`EVALUATED_KEPT`]
    /// rather than all of them: a title whose draws ask a few thousand questions more than the
    /// limit lost every answer each time it filled, five times a second (PPSA04263, whose questions
    /// repeat 92% of the time but missed 70%).
    fn keep(&mut self, fingerprint: u64, answer: Evaluated) {
        self.answers.entry(fingerprint).or_default().push(answer);
        self.order.push_back(fingerprint);
        while self.order.len() > EVALUATED_KEPT {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(kept) = self.answers.get_mut(&oldest) {
                if !kept.is_empty() {
                    kept.remove(0);
                }
                if kept.is_empty() {
                    self.answers.remove(&oldest);
                }
            }
        }
    }
}

fn evaluated() -> &'static std::sync::Mutex<EvaluatedAnswers> {
    static EVALUATED: std::sync::OnceLock<std::sync::Mutex<EvaluatedAnswers>> =
        std::sync::OnceLock::new();
    EVALUATED.get_or_init(Default::default)
}

/// The most answers [`evaluated`] keeps; past it, the oldest go first. Each holds its words and
/// the runs it read, its program shared ([`interned`]).
const EVALUATED_KEPT: usize = 16_384;

/// `computed`, shared: every answer kept for one program and access holds the same one, rather than
/// a copy of a prefix of hundreds of bytes each.
fn interned(computed: &Computed) -> std::sync::Arc<Computed> {
    type Interned = std::collections::HashMap<u64, Vec<std::sync::Arc<Computed>>>;
    static INTERNED: std::sync::OnceLock<std::sync::Mutex<Interned>> = std::sync::OnceLock::new();
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        computed.hash(&mut hasher);
        hasher.finish()
    };
    let Ok(mut interned) = INTERNED.get_or_init(Default::default).lock() else {
        return std::sync::Arc::new(computed.clone());
    };
    if interned.len() >= PREFIXES_KEPT && !interned.contains_key(&key) {
        span(Span::BuffersEmptied, || interned.clear());
    }
    let kept = interned.entry(key).or_default();
    if let Some(found) = kept.iter().find(|kept| ***kept == *computed) {
        return std::sync::Arc::clone(found);
    }
    let shared = std::sync::Arc::new(computed.clone());
    kept.push(std::sync::Arc::clone(&shared));
    shared
}

/// [`evaluate_computed`], replayed: guest memory's words come through `read`.
fn evaluate_computed_now(
    computed: &Computed,
    (user_data, system): (&[u32], &[(u32, u32)]),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Evaluation {
    const UNKNOWN: &str = concat!(
        "a buffer descriptor the program computes from something the host cannot know for the ",
        "draw (D753)"
    );
    let (encodings, decode) = decoded_prefix(&computed.prefix)?;
    let explained = orbistoun_translate::evaluate::scalar_prefix_explained(
        &decode,
        encodings,
        (computed.at, computed.program_address),
        (computed.first_register, user_data),
        system,
        read,
    )?;
    let Some((scalars, unknowns)) = explained else {
        return Ok(None);
    };
    // The register the draw cannot know, and the instruction that left it so - and, one step back,
    // the unknown scalar it read and what left that so.
    let spelled = |register: usize| match register {
        0..=105 => format!("s{register}"),
        106 => "vcc_lo".to_owned(),
        107 => "vcc_hi".to_owned(),
        code => format!("scalar code {code}"),
    };
    let cause = |register: usize| match unknowns.get(register).copied().flatten() {
        Some(orbistoun_translate::evaluate::Unknown::LeftBy(at)) => format!(
            "after {} at {at:#x}",
            crate::pipeline::instruction_text(&decode, encodings, at)
                .unwrap_or_else(|| "an instruction".to_owned())
        ),
        Some(orbistoun_translate::evaluate::Unknown::PathsDisagree) => {
            "where the paths to the access disagree".to_owned()
        }
        None => "and nothing before the access set it".to_owned(),
    };
    let unknown = |register: usize| -> Refusal {
        let mut why = format!(
            "{UNKNOWN}, the access at {:#x}: {} is unknown {}",
            computed.at,
            spelled(register),
            cause(register)
        );
        if let Some(orbistoun_translate::evaluate::Unknown::LeftBy(at)) =
            unknowns.get(register).copied().flatten()
            && let Some(step) = decode.instructions.iter().find(|i| i.offset == at)
            && let Some(read) = step
                .operands
                .iter()
                .skip(1)
                .find_map(|operand| match operand {
                    orbistoun_shader::Operand::Scalar(source)
                        if scalars
                            .get(usize::from(*source))
                            .copied()
                            .flatten()
                            .is_none() =>
                    {
                        Some(usize::from(*source))
                    }
                    _ => None,
                })
        {
            let _ = std::fmt::Write::write_fmt(
                &mut why,
                format_args!(", which read {} unknown {}", spelled(read), cause(read)),
            );
        }
        why.into()
    };
    let (first, count) = match computed.through {
        ComputedAccess::Descriptor { first, .. } => (usize::from(first), 4),
        ComputedAccess::Pointer { base, .. } => (usize::from(base), 2),
    };
    let mut words = [0u32; 4];
    for (step, word) in words.iter_mut().enumerate().take(count) {
        *word = scalars
            .get(first + step)
            .copied()
            .flatten()
            .ok_or_else(|| unknown(first + step))?;
    }
    // The offset field's codes: a scalar register, `null` (125), or an inline integer, 128 to 192
    // counting up from zero and 193 to 208 down from -1.
    let soffset = match computed.soffset {
        125 => Some(0),
        code @ 128..=192 => Some(u32::from(code - 128)),
        code @ 193..=208 => Some(0u32.wrapping_sub(u32::from(code - 192))),
        code => match scalars.get(usize::from(code)).copied().flatten() {
            Some(soffset) => Some(soffset),
            // A descriptor's offset the host cannot know: its buffer is bound whole (D901).
            None if count == 4 => None,
            None => return Err(unknown(usize::from(code))),
        },
    };
    Ok(Some((words, soffset)))
}

/// [`decode_prefix`], kept by the prefix's words: a draw resolves its descriptors from the same
/// prefix every time, and PPSA25872's draws each decoded it again, 0.1 ms of every draw.
pub(crate) fn decoded_prefix(
    prefix: &[u32],
) -> Result<
    (
        &'static orbistoun_shader::EncodingTable,
        std::sync::Arc<orbistoun_shader::Decode>,
    ),
    &'static str,
> {
    /// Prefixes decoded, by their words.
    type Prefixes = std::collections::HashMap<Vec<u32>, std::sync::Arc<orbistoun_shader::Decode>>;
    static PREFIXES: std::sync::OnceLock<std::sync::Mutex<Prefixes>> = std::sync::OnceLock::new();
    let prefixes = PREFIXES.get_or_init(Default::default);
    let (encodings, _) = tables()?;
    if let Some(known) = prefixes
        .lock()
        .ok()
        .and_then(|prefixes| prefixes.get(prefix).cloned())
    {
        return Ok((encodings, known));
    }
    let (_, decode) = decode_prefix(prefix)?;
    let decode = std::sync::Arc::new(decode);
    if let Ok(mut prefixes) = prefixes.lock() {
        if prefixes.len() >= PREFIXES_KEPT {
            span(Span::BuffersEmptied, || prefixes.clear());
        }
        prefixes.insert(prefix.to_vec(), std::sync::Arc::clone(&decode));
    }
    Ok((encodings, decode))
}

/// The most decoded prefixes [`decoded_prefix`] keeps.
const PREFIXES_KEPT: usize = 1024;

/// The shader tables, and a prefix of a program decoded with them.
fn decode_prefix(
    prefix: &[u32],
) -> Result<
    (
        &'static orbistoun_shader::EncodingTable,
        orbistoun_shader::Decode,
    ),
    &'static str,
> {
    let (encodings, operands) = tables()?;
    let bytes: Vec<u8> = prefix.iter().flat_map(|word| word.to_le_bytes()).collect();
    Ok((
        encodings,
        orbistoun_shader::decode_program(&bytes, encodings, operands),
    ))
}

/// The shader tables, loaded once.
fn tables() -> Result<
    (
        &'static orbistoun_shader::EncodingTable,
        &'static orbistoun_shader::OperandTable,
    ),
    &'static str,
> {
    static TABLES: std::sync::OnceLock<
        Option<(
            orbistoun_shader::EncodingTable,
            orbistoun_shader::OperandTable,
        )>,
    > = std::sync::OnceLock::new();
    let (encodings, operands) = TABLES
        .get_or_init(|| {
            Some((
                orbistoun_shader::EncodingTable::builtin().ok()?,
                orbistoun_shader::OperandTable::builtin().ok()?,
            ))
        })
        .as_ref()
        .ok_or("the shader encoding tables did not load")?;
    Ok((encodings, operands))
}

/// The guest range a primitive shader's global loads read for a draw of `vertices` vertices with
/// these user-data words (D758): from the lowest address an active lane forms to the end of the
/// widest read from the highest. Empty, at zero, when no lane reaches a load.
///
/// # Errors
///
/// A refusal naming why the lanes' addresses cannot be found.
fn global_range(
    global: &GlobalLoads,
    user_data: &[u32; USER_DATA_WORDS],
    vertices: u32,
    memory: &impl GuestMemory,
) -> Result<(u64, u64), Refusal> {
    /// A run's answer, and every word it read with the value it read: the answer holds while they
    /// all still read the same.
    type Run = (Vec<(u64, Option<u32>)>, Result<(u64, u64), Refusal>);
    /// Runs kept, by program, user data and vertices; a bound, since a guest streaming new tables
    /// every frame adds a run a draw.
    const RUNS_KEPT: usize = 4096;
    static RUNS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<Vec<u32>, Run>>> =
        std::sync::OnceLock::new();
    if vertices > global.width {
        return Err(concat!(
            "a draw of more vertices than one wave holds, whose global loads' lanes are not all ",
            "run by one (D758)"
        )
        .into());
    }
    let count = usize::try_from(global.user_data)
        .unwrap_or(USER_DATA_WORDS)
        .min(USER_DATA_WORDS);
    let mut word = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let hash = crate::content_hash(&global.prefix);
    let prepared = prepared_program(global, hash)?;
    let draw = (
        (global.first_register, &user_data[..count]),
        (global.width, vertices),
    );
    // A program whose native run is trusted runs in less than keeping its answer costs: a title
    // drawing a triangle at a time never draws the same user data twice (D850).
    if let Some(found) = prepared.trusted(draw, &mut word) {
        return Ok(span_of(global, found));
    }
    // The same program over the same user data and vertices reads the same words; while they hold
    // what they held, the lanes form the same addresses.
    let mut key = vec![
        hash as u32,
        (hash >> 32) as u32,
        global.first_register,
        global.width,
        vertices,
    ];
    key.extend_from_slice(&user_data[..count]);
    let runs = RUNS.get_or_init(Default::default);
    if let Ok(runs) = runs.lock()
        && let Some((reads, answer)) = runs.get(&key)
        && reads.iter().all(|&(address, value)| word(address) == value)
    {
        return answer.clone();
    }
    let mut reads = Vec::new();
    let answer = prepared
        .lanes(draw, &mut |address| {
            let value = word(address);
            reads.push((address, value));
            value
        })
        .map(|found| span_of(global, found));
    if let Ok(mut runs) = runs.lock() {
        if runs.len() >= RUNS_KEPT {
            span(Span::BuffersEmptied, || runs.clear());
        }
        runs.insert(key, (reads, answer.clone()));
    }
    answer
}

/// How many runs of a program check its native answer against the model's before the native one
/// is taken alone (D850).
const QUICK_CHECKS: u32 = 64;

/// A global-load program prepared for runs, and how far its native run is trusted (D850).
struct Prepared {
    program: orbistoun_translate::evaluate::LaneProgram,
    /// Whether its native run is still taken: until an answer differs from the model's.
    quick: std::sync::atomic::AtomicBool,
    /// How many native answers have matched the model's.
    checks: std::sync::atomic::AtomicU32,
}

/// A draw as a lane run sees it: the first user-data register and the words, then the wave's width
/// and the draw's vertices.
type Draw<'a> = ((u32, &'a [u32]), (u32, u32));

impl Prepared {
    /// Each load's lanes' addresses natively, once its native run has matched the model's
    /// [`QUICK_CHECKS`] times; `None` before that, or where the native run declines.
    fn trusted(
        &self,
        (user_data, lanes): Draw<'_>,
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) -> Option<orbistoun_translate::evaluate::LaneAddresses> {
        use std::sync::atomic::Ordering::Relaxed;
        if !self.quick.load(Relaxed) || self.checks.load(Relaxed) < QUICK_CHECKS {
            return None;
        }
        orbistoun_translate::evaluate::quick_lane_addresses(&self.program, user_data, lanes, read)
    }

    /// Each load's lanes' addresses: through the model, and natively beside it while the native
    /// run is being checked. A native answer that differs from the model's stops the native run
    /// for the program, and says so.
    fn lanes(
        &self,
        (user_data, lanes): Draw<'_>,
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) -> Result<orbistoun_translate::evaluate::LaneAddresses, Refusal> {
        use orbistoun_translate::evaluate::{lane_addresses, quick_lane_addresses};
        use std::sync::atomic::Ordering::Relaxed;
        let (encodings, _) = tables()?;
        let quick = if self.quick.load(Relaxed) {
            quick_lane_addresses(&self.program, user_data, lanes, read)
        } else {
            None
        };
        let model = lane_addresses(&self.program, encodings, user_data, lanes, read);
        if let Some(found) = quick {
            if model.as_ref() == Ok(&found) {
                self.checks.fetch_add(1, Relaxed);
            } else if self.quick.swap(false, Relaxed) {
                tracing::warn!(
                    concat!(
                        "a global load's native lane run answered {:x?} where the model answers ",
                        "{:x?}; this program runs through the model from now (D850)"
                    ),
                    found,
                    model
                );
            }
        }
        model
    }
}

/// The program `global` runs, prepared once: decoded, sliced and compiled for native runs. Kept by
/// its words' content hash and length, since hashing a few hundred words with a map's own hasher
/// every draw cost more than the native run.
fn prepared_program(
    global: &GlobalLoads,
    hash: u64,
) -> Result<std::sync::Arc<Prepared>, &'static str> {
    type Programs = std::collections::HashMap<(u64, usize), std::sync::Arc<Prepared>>;
    static PROGRAMS: std::sync::OnceLock<std::sync::Mutex<Programs>> = std::sync::OnceLock::new();
    let programs = PROGRAMS.get_or_init(Default::default);
    let key = (hash, global.prefix.len());
    if let Some(known) = programs
        .lock()
        .ok()
        .and_then(|programs| programs.get(&key).cloned())
    {
        return Ok(known);
    }
    let (encodings, decode) = decode_prefix(&global.prefix)?;
    let offsets: Vec<u32> = global.loads.iter().map(|&(at, _)| at).collect();
    let program = orbistoun_translate::evaluate::LaneProgram::new(decode, encodings, &offsets);
    let prepared = std::sync::Arc::new(Prepared {
        quick: std::sync::atomic::AtomicBool::new(program.runs_quickly()),
        checks: std::sync::atomic::AtomicU32::new(0),
        program,
    });
    if let Ok(mut programs) = programs.lock() {
        programs.insert(key, prepared.clone());
    }
    Ok(prepared)
}

/// The range the loads' lanes read: from the word the lowest address is in to the end of the
/// widest read from the highest; empty, at zero, when no lane reached a load.
fn span_of(
    global: &GlobalLoads,
    found: orbistoun_translate::evaluate::LaneAddresses,
) -> (u64, u64) {
    let mut span: Option<(u64, u64)> = None;
    for (at, addresses) in found {
        let reads = global
            .loads
            .iter()
            .find(|&&(load, _)| load == at)
            .map_or(16, |&(_, bytes)| u64::from(bytes));
        for address in addresses {
            let end = address.saturating_add(reads);
            span = Some(span.map_or((address, end), |(low, high)| {
                (low.min(address), high.max(end))
            }));
        }
    }
    // Whole words, from the word the lowest address is in to the end of the word the last byte is
    // in, as the module reads them: a sub-word load's last byte can sit mid-word (D883).
    span.map_or((0, 0), |(low, high)| {
        let first = low & !3;
        (first, (high.saturating_add(3) & !3) - first)
    })
}

/// The most waves a dispatch's global loads are run through on the host in one preparation (D883):
/// a bound on the work a dispatch can ask of the host, beyond which a sample of its groups is run
/// (D950).
const MOST_DISPATCH_WAVES: u64 = 4096;

/// What a dispatch's lane run seeds each wave with (D883), as a refusal says it: the user data
/// from `s0`, the group's enabled id components after it, and the thread id components from `v0`.
fn wave_entry(user_data: usize, inputs: orbistoun_translate::wavefront::ComputeInputs) -> String {
    let mut parts = Vec::new();
    if user_data > 0 {
        parts.push(format!("its user data in s0 to s{}", user_data - 1));
    }
    let mut next = user_data;
    for (component, axis) in ["x", "y", "z"].iter().enumerate() {
        if inputs.workgroup_ids[component] {
            parts.push(format!("its group's {axis} id in s{next}"));
            next += 1;
        }
    }
    let components = inputs.thread_id_components.clamp(1, 3);
    parts.push(format!(
        "its thread ids in v0{} ({} component{})",
        if components > 1 {
            format!(" to v{}", components - 1)
        } else {
            String::new()
        },
        ["one", "two", "three"][components as usize - 1],
        if components > 1 { "s" } else { "" }
    ));
    let last = parts.pop().unwrap_or_default();
    if parts.is_empty() {
        format!("a wave of this dispatch starts with {last}")
    } else {
        format!(
            "a wave of this dispatch starts with {}, and {last}",
            parts.join(", ")
        )
    }
}

/// The range a dispatch binds for its global loads (D883, D950), or for a computed access a sample
/// of its groups reached (D951).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GlobalRange {
    /// The guest range bound, `(address, bytes)`: what the waves run read, and where they were a
    /// sample, a margin about it.
    pub(crate) range: (u64, u64),
    /// What the waves run read, `(address, bytes)`.
    pub(crate) read: (u64, u64),
    /// How many groups were run, where they were a sample of the dispatch's rather than all of
    /// them: a load outside the range is then possible, and the module reports it as an escape.
    pub(crate) sampled: Option<u64>,
    /// Every page the waves run read a byte of, ascending, by guest address: what a dispatch's
    /// global loads' buffer holds where every wave ran ([`crate::global_pages`]). Empty for a
    /// computed access's range.
    pub(crate) pages: Vec<u64>,
}

/// The most margin a sampled dispatch's global range has on each side beyond what its waves read
/// (D950): room for groups between the sampled ones that read a little further, the bytes bound
/// bounded however large the span.
const MOST_SAMPLED_MARGIN: u64 = 1 << 20;

/// The groups a dispatch too large to run whole is sampled at (D950): along each dimension its
/// first and last group and as many evenly spaced between as the waves allow, every combination of
/// them. The corners of the grid are always among them, so a load whose address is an affine
/// function of the group and thread ids reads its lowest and highest bytes in a sampled group.
/// `waves` bounds the sampled groups' waves, beyond the corners, which are always taken (D951).
fn sampled_groups(groups: [u32; 3], waves_per_group: u64, waves: u64) -> Vec<[u32; 3]> {
    let budget = (waves / waves_per_group.max(1)).max(1);
    let taken = |along: u32| -> u64 {
        groups
            .iter()
            .map(|&n| u64::from(n.min(along)))
            .product::<u64>()
    };
    // The most points along a dimension whose grid fits the budget; two at least, for the corners.
    let mut along = 2u32;
    while along < groups.iter().copied().max().unwrap_or(1) && taken(along + 1) <= budget {
        along += 1;
    }
    let points = |n: u32| -> Vec<u32> {
        if n <= along {
            return (0..n).collect();
        }
        let last = u64::from(n - 1);
        let steps = u64::from(along - 1);
        let mut points: Vec<u32> = (0..=steps)
            .map(|step| u32::try_from(step * last / steps).unwrap_or(n - 1))
            .collect();
        points.dedup();
        points
    };
    let [xs, ys, zs] = groups.map(points);
    let mut sampled = Vec::with_capacity(xs.len() * ys.len() * zs.len());
    for &z in &zs {
        for &y in &ys {
            for &x in &xs {
                sampled.push([x, y, z]);
            }
        }
    }
    sampled
}

/// `read` widened by a margin on each side (D950) - an eighth of its bytes, at most
/// [`MOST_SAMPLED_MARGIN`], in whole pages - where guest memory is mapped across the widened range;
/// a side whose margin is not mapped keeps none.
fn with_margin(read: (u64, u64), memory: &impl GuestMemory) -> (u64, u64) {
    const PAGE: u64 = 0x1000;
    let margin = (read.1 / 8).min(MOST_SAMPLED_MARGIN).next_multiple_of(PAGE);
    if read.1 == 0 || margin == 0 {
        return read;
    }
    let mapped = |(address, bytes): (u64, u64)| {
        usize::try_from(bytes).is_ok_and(|bytes| memory.mapped(address, bytes))
    };
    let end = read.0 + read.1;
    let low = read.0.saturating_sub(margin);
    let high = end.saturating_add(margin);
    [(low, high), (read.0, high), (low, end)]
        .into_iter()
        .map(|(low, high)| (low, high - low))
        .find(|&range| mapped(range))
        .unwrap_or(read)
}

/// The range a dispatch's global loads read (D883): the program run on the host over every wave of
/// every group the dispatch runs, each from its entry state - the user data, the group's enabled id
/// components, and each lane's thread id, x fastest, where the lane has a thread - and the span of
/// every address an active lane forms. A partial dispatch's last group runs only its threads (D855).
///
/// A dispatch of more waves than [`MOST_DISPATCH_WAVES`] runs every wave of a sample of its groups
/// instead ([`sampled_groups`]), and binds what they read with a margin ([`with_margin`]); the
/// module reports a load outside it as an escape, and the dispatch's result is refused (D950).
///
/// # Errors
///
/// A load whose lanes' addresses or mask the host cannot know.
pub(crate) fn dispatch_global_range(
    global: &GlobalLoads,
    dispatch: (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
        u32,
    ),
    memory: &impl GuestMemory,
) -> Result<GlobalRange, Refusal> {
    global_range_within(
        global,
        dispatch,
        (MOST_PREPARATION_STEPS, MOST_NATIVE_PREPARATION_STEPS),
        memory,
    )
}

/// [`dispatch_global_range`] with `(steps, native)` for every wave it runs to share: the model's
/// steps and the native run's (D951).
fn global_range_within(
    global: &GlobalLoads,
    (user_data, inputs, groups, width): (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
        u32,
    ),
    (mut steps, mut native): (usize, usize),
    memory: &impl GuestMemory,
) -> Result<GlobalRange, Refusal> {
    use orbistoun_translate::evaluate::{
        LOOPS_PAST_THE_BOUND, PAST_THE_PREPARATION_STEPS, long_wave_lane_addresses_within,
        wave_lane_addresses_within,
    };
    let waves_per_group = u64::from(inputs.waves(width));
    let runs = groups.iter().map(|&n| u64::from(n)).product::<u64>() * waves_per_group;
    // A sample of a dispatch too large to run whole is device-addressed instead, where the backend
    // offers it (D981).
    if runs > MOST_DISPATCH_WAVES && !crate::device_addressed::host_runs_allowed() {
        return Err(format!(
            "its global loads' range: {}",
            crate::device_addressed::HOST_RUN_PASSED
        )
        .into());
    }
    let (entries, sampled) = if runs > MOST_DISPATCH_WAVES {
        let chosen = sampled_groups(groups, waves_per_group, MOST_DISPATCH_WAVES);
        let count = chosen.len() as u64;
        let entries = chosen
            .into_iter()
            .flat_map(|group| group_waves(inputs, group, width))
            .collect();
        (entries, Some(count))
    } else {
        (dispatch_waves(inputs, groups, width), None)
    };
    let (encodings, _) = tables()?;
    let prepared = prepared_program(global, crate::content_hash(&global.prefix))?;
    let mut word = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    // The span every wave reads, folded wave by wave: a long loop's addresses are many.
    let mut span: Option<(u64, u64)> = None;
    // And every page they read a byte of.
    let mut pages = std::collections::BTreeSet::new();
    // The steps every wave shares, the model's and the native run's each (D951).
    let (allowed, allowed_native) = (steps, native);
    let waves = entries.len();
    for (ran, entry) in entries.into_iter().enumerate() {
        let addresses = match wave_lane_addresses_within(
            &prepared.program,
            encodings,
            user_data,
            (width, &entry),
            (&mut word, &mut steps),
        ) {
            Ok(addresses) => addresses,
            // A loop longer than the model follows is followed natively (D850).
            Err(why) if why == LOOPS_PAST_THE_BOUND => long_wave_lane_addresses_within(
                &prepared.program,
                user_data,
                (width, &entry),
                (&mut word, &mut native),
            )
            .map_err(|at| -> Refusal {
                if at == u32::MAX && native == 0 {
                    return format!(
                        "the native replay ran past {allowed_native} steps, the preparation's bound, before every wave's global loads: {ran} of {waves} waves ran (D951)"
                    )
                    .into();
                }
                let step = prepared.program.instruction_named(at);
                if prepared.program.runs_quickly() {
                    format!("{why}; the native run stops at {step}").into()
                } else {
                    format!("{why}; the native run does not carry out {step}").into()
                }
            })?,
            Err(why) if why == PAST_THE_PREPARATION_STEPS => {
                return Err(format!(
                    "the host replay ran past {allowed} steps, the preparation's bound, before every wave's global loads: {ran} of {waves} waves ran (D951)"
                )
                .into());
            }
            Err(why) => {
                return Err(format!("{why}; {}", wave_entry(user_data.len(), inputs)).into());
            }
        };
        note_pages(global, &addresses, &mut pages);
        let (first, bytes) = span_of(global, addresses);
        if bytes > 0 {
            let end = first + bytes;
            span = Some(span.map_or((first, end), |(low, high)| (low.min(first), high.max(end))));
        }
    }
    let read = span.map_or((0, 0), |(low, high)| (low, high - low));
    Ok(GlobalRange {
        range: if sampled.is_some() {
            with_margin(read, memory)
        } else {
            read
        },
        read,
        sampled,
        pages: pages.into_iter().collect(),
    })
}

/// Adds to `pages` every page a load's lanes read a byte of: from the word each address is in to
/// the end of the word its last byte is in, as [`span_of`] counts them.
fn note_pages(
    global: &GlobalLoads,
    found: &orbistoun_translate::evaluate::LaneAddresses,
    pages: &mut std::collections::BTreeSet<u64>,
) {
    for (at, addresses) in found {
        let reads = global
            .loads
            .iter()
            .find(|&&(load, _)| load == *at)
            .map_or(16, |&(_, bytes)| u64::from(bytes));
        let words = |address: u64| {
            let first = address & !3;
            (
                first,
                (address.saturating_add(reads).saturating_add(3) & !3) - first,
            )
        };
        if let Some(held) =
            crate::global_pages::GlobalPages::of(0, addresses.iter().map(|&a| words(a)))
        {
            pages.extend(held.pages);
        }
    }
}

/// Every wave of a dispatch of `groups` workgroups as the hardware starts it (D883), group by group
/// as [`group_waves`] starts each.
fn dispatch_waves(
    inputs: orbistoun_translate::wavefront::ComputeInputs,
    groups: [u32; 3],
    width: u32,
) -> Vec<orbistoun_translate::evaluate::WaveEntry> {
    let mut entries = Vec::new();
    for z in 0..groups[2] {
        for y in 0..groups[1] {
            for x in 0..groups[0] {
                entries.extend(group_waves(inputs, [x, y, z], width));
            }
        }
    }
    entries
}

/// The first wave of each of the first `count` groups of a dispatch of `groups`, in the order
/// [`dispatch_waves`] starts them: what a sampled replay probes. Built group by group, so a sample
/// of a huge grid costs its sample - every wave of the grid made first and then thinned grew to
/// 7 GiB on GTA V's full-screen passes and exhausted the host.
fn first_waves_of_first_groups(
    inputs: orbistoun_translate::wavefront::ComputeInputs,
    groups: [u32; 3],
    width: u32,
    count: usize,
) -> Vec<orbistoun_translate::evaluate::WaveEntry> {
    (0..groups[2])
        .flat_map(|z| (0..groups[1]).flat_map(move |y| (0..groups[0]).map(move |x| [x, y, z])))
        .take(count)
        .filter_map(|group| group_waves(inputs, group, width).into_iter().next())
        .collect()
}

/// Every wave of the group at `group` as the hardware starts it (D883): its group's enabled id
/// components, and each lane's thread ids - `None` past the group's threads, or outside a partial
/// last group (D855) - the x id counting from the dispatch's first thread (D861).
fn group_waves(
    inputs: orbistoun_translate::wavefront::ComputeInputs,
    group: [u32; 3],
    width: u32,
) -> Vec<orbistoun_translate::evaluate::WaveEntry> {
    let waves = inputs.waves(width);
    let [x_threads, y_threads, z_threads] = inputs.threads.map(|n| n.max(1));
    let group_ids: Vec<u32> = (0..3)
        .filter(|&component| inputs.workgroup_ids[component])
        .map(|component| group[component])
        .collect();
    (0..waves)
        .map(|wave| {
            let threads = (0..width)
                .map(|lane| {
                    let thread = wave * width + lane;
                    if thread >= x_threads * y_threads * z_threads {
                        return None;
                    }
                    let id = [
                        thread % x_threads,
                        thread / x_threads % y_threads,
                        thread / (x_threads * y_threads),
                    ];
                    let runs = inputs.partial.is_none_or(|partial| {
                        (0..3).all(|c| group[c] != partial.last[c] || id[c] < partial.threads[c])
                    });
                    runs.then_some([id[0] + inputs.first_thread, id[1], id[2]])
                })
                .collect();
            orbistoun_translate::evaluate::WaveEntry {
                group_ids: group_ids.clone(),
                threads,
                components: inputs.thread_id_components,
            }
        })
        .collect()
}

/// How many bytes past a computed access's prefix a probed run reads of its program: the loop the
/// access lies in closes after it (D900).
const PROBE_TAIL_BYTES: usize = 4096;

/// The descriptors, and scalar offsets, a dispatch's waves reach a computed access with (D900):
/// each wave run on the host through its program - read from guest memory, past the access to the
/// end of the loop it lies in - the access's registers recorded each time it runs. A waterfall loop
/// runs it once a distinct value across its wave's lanes.
///
/// # Errors
///
/// A refusal naming what the host cannot follow: no program address, or what a wave's run could
/// not know. `address` is the program's, whose prefix the access's is. With `sample`, only the
/// first wave of each of that many groups is run, however many the dispatch has: what a refusal
/// reports of a dispatch whose waves cannot be run.
///
/// A dispatch of more waves than [`MOST_COMPUTED_WAVES`] runs every wave of a lattice of its
/// groups instead ([`sampled_groups`]), and says how many groups it ran (D951): what they reach is
/// then bound by [`sampled_reached_range`], and an access outside it is an escape. A lattice that
/// reaches the access with nothing that binds a byte places nothing: every wave is run instead,
/// as without a sample, where the dispatch has no more than [`MOST_DISPATCH_WAVES`], and the
/// access is refused where it has more.
pub(crate) fn dispatch_computed_descriptors(
    (computed, address, sample): (&Computed, u64, Option<usize>),
    dispatch: (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
        u32,
    ),
    memory: &impl GuestMemory,
) -> Result<ReachedDescriptors, Refusal> {
    computed_descriptors_within(
        (computed, address, sample),
        dispatch,
        MOST_PREPARATION_STEPS,
        memory,
    )
}

/// [`dispatch_computed_descriptors`] with `steps` for every wave it runs to share (D951).
fn computed_descriptors_within(
    (computed, address, sample): (&Computed, u64, Option<usize>),
    (user_data, inputs, groups, width): (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
        u32,
    ),
    mut steps: usize,
    memory: &impl GuestMemory,
) -> Result<ReachedDescriptors, Refusal> {
    use orbistoun_translate::evaluate::{LaneProgram, wave_probes_within};
    let waves_per_group = u64::from(inputs.waves(width));
    let runs = groups.iter().map(|&n| u64::from(n)).product::<u64>() * waves_per_group;
    let (encodings, operands) = tables()?;
    let length = computed.prefix.len() * 4 + PROBE_TAIL_BYTES;
    let bytes = memory
        .read(address, length)
        .or_else(|| memory.read(address, computed.prefix.len() * 4))
        .ok_or("a program that is not readable guest memory (D900)")?;
    let decode = orbistoun_shader::decode_program(bytes, encodings, operands);
    let (first, count) = match computed.through {
        ComputedAccess::Descriptor { first, .. } => (usize::from(first), 4),
        ComputedAccess::Pointer { base, .. } => (usize::from(base), 2),
    };
    let mut registers: Vec<usize> = (first..first + count).collect();
    let soffset = usize::from(computed.soffset);
    if soffset < 128 {
        registers.push(soffset);
    }
    let program = LaneProgram::with_probes(decode, encodings, &[(computed.at, registers.clone())]);
    let mut word = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    // `steps` are shared by every wave this preparation runs: no preparation holds the host for
    // longer than they take (D951).
    let allowed = steps;
    let mut run = |entries: Vec<orbistoun_translate::evaluate::WaveEntry>| {
        let mut reached = ReachedDescriptors::default();
        for (ran, entry) in entries.iter().enumerate() {
            let (probed, last) = wave_probes_within(
                &program,
                encodings,
                user_data,
                (width, entry),
                (&mut word, &mut steps),
            )
            .map_err(|why| replay_bound(why, (computed.at, allowed), (ran, entries.len())))?;
            *reached.ends.entry(last).or_default() += 1;
            for (_, values) in probed {
                let unknown: Vec<String> = registers
                    .iter()
                    .zip(&values)
                    .filter(|(_, value)| value.is_none())
                    .map(|(register, _)| format!("s{register}"))
                    .collect();
                let values: Option<Vec<u32>> = values.into_iter().collect();
                let values = values.ok_or_else(|| {
                    format!(
                        "a wave reaches the access with {} unknown, ending at {last:#x}",
                        unknown.join(", ")
                    )
                })?;
                let mut words = [0; 4];
                words[..count].copy_from_slice(&values[..count]);
                reached
                    .descriptors
                    .insert((words, values.get(count).copied().unwrap_or(0)));
            }
        }
        Ok::<_, Refusal>(reached)
    };
    match sample {
        Some(count) => run(first_waves_of_first_groups(inputs, groups, width, count)),
        // Every wave of a lattice of groups, the corners in it: a waterfall's descriptor follows
        // its lanes, so a group's other waves can reach another (D951).
        None if runs > MOST_COMPUTED_WAVES => {
            let chosen = sampled_groups(groups, waves_per_group, MOST_COMPUTED_WAVES);
            let groups_run = chosen.len() as u64;
            let mut reached = run(chosen
                .into_iter()
                .flat_map(|group| group_waves(inputs, group, width))
                .collect())?;
            reached.sampled = Some(groups_run);
            if !reaches_nothing(computed, &reached) {
                return Ok(reached);
            }
            // A lattice that reaches nothing places nothing: an access only some threads take,
            // as a waterfall over a tree's leaves is, can lie wholly between the sampled groups.
            // Every wave is run where a preparation can; past that the access is refused, never
            // bound empty.
            if runs > MOST_DISPATCH_WAVES {
                let ends: Vec<String> = reached
                    .ends
                    .iter()
                    .map(|(at, waves)| format!("{waves} at {at:#x}"))
                    .collect();
                return Err(format!(
                    "a sample of {groups_run} groups reaches the access in no wave, their runs ending {}, and the dispatch's {runs} waves are more than are run whole (D951)",
                    ends.join(", ")
                )
                .into());
            }
            run(dispatch_waves(inputs, groups, width))
        }
        None => run(dispatch_waves(inputs, groups, width)),
    }
}

/// The most steps one preparation's host replay of a computed access runs, across every wave it
/// runs (D951). A step of the host interpreter, its wave's setup shared out, cost about half a
/// microsecond in a release build (measured: 256 waves of a 22-step program in 3 ms), so this
/// holds a preparation to about two seconds - room for the full runs Grand Theft Auto V's tree
/// walks took before D951, 0.5 to 1.2 s; one that would run longer is refused, never left to
/// hold the run.
pub(crate) const MOST_PREPARATION_STEPS: usize = 1 << 22;

/// The most steps one preparation's native runs of a dispatch's global loads take, across every
/// wave (D951): four waves' worth of [`MOST_NATIVE_STEPS`], the native run being compiled and far
/// quicker a step than the model.
///
/// [`MOST_NATIVE_STEPS`]: orbistoun_translate::evaluate::MOST_NATIVE_STEPS
const MOST_NATIVE_PREPARATION_STEPS: usize = 4 * orbistoun_translate::evaluate::MOST_NATIVE_STEPS;

/// A wave's refusal, worded where it ran out of steps (D951): its own bound, or the preparation's
/// `allowed`, before reaching the access at byte `at`; after `ran` of the `waves` to run.
fn replay_bound(
    why: orbistoun_translate::evaluate::LaneRefusal,
    (at, allowed): (u32, usize),
    (ran, waves): (usize, usize),
) -> Refusal {
    use orbistoun_translate::evaluate::{
        LOOPS_PAST_THE_BOUND, MOST_WAVE_STEPS, PAST_THE_PREPARATION_STEPS,
    };
    if why == LOOPS_PAST_THE_BOUND {
        format!(
            "the host replay ran past a wave's step bound (at most {MOST_WAVE_STEPS} steps) before reaching the access at {at:#x}, in wave {} of {waves} (D951)",
            ran + 1
        )
        .into()
    } else if why == PAST_THE_PREPARATION_STEPS {
        format!(
            "the host replay ran past {allowed} steps, the preparation's bound, before reaching the access at {at:#x} in every wave: {ran} of {waves} waves ran (D951)"
        )
        .into()
    } else {
        why
    }
}

/// Whether what a lattice of a dispatch's groups reached a computed access with binds nothing
/// (D951): no descriptor, or only ones whose range is empty - a null descriptor - which cannot say
/// where the buffer the other groups reach lies.
fn reaches_nothing(computed: &Computed, reached: &ReachedDescriptors) -> bool {
    reached.descriptors.iter().all(|&(values, soffset)| {
        computed_range(computed.through, (values, Some(soffset)), None)
            .is_ok_and(|(_, bytes)| bytes == 0)
    })
}

/// The most groups whose scalar prefix [`dispatch_group_descriptors`] replays.
const MOST_DISPATCH_GROUPS: u64 = 1 << 16;

/// The descriptors, and scalar offsets, a dispatch's groups reach a computed access with, where its
/// scalar prefix depends on nothing a wave's lanes hold (D914): replayed once a group, the group's
/// ids in the registers after its `user_data`, one for each dimension `inputs` hands the program,
/// as the hardware loads them. A group that branches past the access reaches nothing.
///
/// # Errors
///
/// A dispatch that hands its program no group id, one of more groups than are replayed, or why a
/// group's replay could not know the descriptor.
pub(crate) fn dispatch_group_descriptors(
    computed: &Computed,
    (user_data, inputs, groups): (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
    ),
    memory: &impl GuestMemory,
) -> Result<ReachedDescriptors, Refusal> {
    let first_id = computed.first_register + u32::try_from(user_data.len()).unwrap_or(u32::MAX);
    let dimensions: Vec<usize> = (0..3).filter(|&d| inputs.workgroup_ids[d]).collect();
    if dimensions.is_empty() {
        return Err("a dispatch whose program is handed no group id (D914)".into());
    }
    if groups.iter().map(|&n| u64::from(n)).product::<u64>() > MOST_DISPATCH_GROUPS {
        return Err("a dispatch of more groups than are replayed on the host (D914)".into());
    }
    let mut read = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let mut reached = ReachedDescriptors::default();
    for z in 0..groups[2] {
        for y in 0..groups[1] {
            for x in 0..groups[0] {
                let id = [x, y, z];
                let system: Vec<(u32, u32)> = (first_id..)
                    .zip(&dimensions)
                    .map(|(register, &d)| (register, id[d]))
                    .collect();
                let seeded = || {
                    let named: Vec<String> = system
                        .iter()
                        .map(|(register, _)| format!("s{register}"))
                        .collect();
                    named.join(", ")
                };
                let found = evaluate_computed_now(computed, (user_data, &system), &mut read)
                    .map_err(|why| format!("{why} (the group's ids in {})", seeded()))?;
                let Some((words, soffset)) = found else {
                    continue;
                };
                let soffset = soffset
                    .ok_or("a group reaches the access at an offset the host cannot know (D914)")?;
                reached.descriptors.insert((words, soffset));
            }
        }
    }
    Ok(reached)
}

/// What a dispatch's waves reach a computed access with, run on the host (D900): each distinct
/// descriptor with its scalar offset, and how many waves' runs ended at each byte.
#[derive(Debug, Default)]
pub(crate) struct ReachedDescriptors {
    /// The distinct descriptors and scalar offsets.
    pub descriptors: std::collections::BTreeSet<([u32; 4], u32)>,
    /// Waves by the byte of the last instruction their run ran.
    pub ends: BTreeMap<u32, u32>,
    /// How many groups were run, where they were a lattice of the dispatch's rather than all of
    /// them (D951): an unsampled group can then reach the access with another descriptor.
    pub sampled: Option<u64>,
}

/// The most waves a dispatch's computed access is run through on the host in one preparation
/// before a lattice of its groups is run instead (D951). A wave of a waterfall program runs its
/// loop once a distinct value across its lanes, up to 64 passes of the whole loop on the host
/// interpreter: some tenths of a millisecond a wave, so 256 keeps a preparation's run within about
/// 100 ms. At the 4096 the global loads' short prefixes run (D950), Grand Theft Auto V's in-world
/// frame ground on one such program for minutes.
pub(crate) const MOST_COMPUTED_WAVES: u64 = 256;

/// The range a computed access binds from what a lattice of a dispatch's groups reach it with
/// (D951): every reached descriptor's range, from the lowest base to the highest end, widened by
/// a margin as a sampled dispatch's global loads are (D950). The bound buffer starts at a base of
/// its own, the access reads it relative to that from the descriptor it computes, and one the
/// buffer does not hold is an escape.
///
/// # Errors
///
/// A scalar load's base pair, which is not read relative to a base of its own; a range larger
/// than a draw buffer binds; or one that crosses a four-gigabyte span, whose low halves the
/// module's addresses cannot order.
pub(crate) fn sampled_reached_range(
    computed: &Computed,
    reached: &ReachedDescriptors,
    memory: &impl GuestMemory,
) -> Result<GlobalRange, Refusal> {
    if matches!(computed.through, ComputedAccess::Pointer { .. }) {
        return Err(
            "a scalar load's base pair, which a sample of the dispatch's groups does not place (D951)"
                .into(),
        );
    }
    let mut span: Option<(u64, u64)> = None;
    for &(values, soffset) in &reached.descriptors {
        let (base, bytes) = computed_range(computed.through, (values, Some(soffset)), None)?;
        let end = base.saturating_add(bytes);
        span = Some(span.map_or((base, end), |(low, high)| (low.min(base), high.max(end))));
    }
    let read = span.map_or((0, 0), |(low, high)| (low, high - low));
    // Nothing reached places nothing, and an empty range would make every access an escape.
    if read.1 == 0 {
        return Err(
            "a sample of its groups reaches it with no descriptor that binds a byte (D951)".into(),
        );
    }
    let read = checked_range(read).map_err(|why| -> Refusal {
        format!(
            "what a sample of its groups reaches, {:#x}+{:#x}: {why}",
            read.0, read.1
        )
        .into()
    })?;
    let within = |(address, bytes): (u64, u64)| {
        bytes <= MOST_BYTES && (bytes == 0 || address >> 32 == (address + bytes - 1) >> 32)
    };
    if !within(read) {
        return Err(format!(
            "what a sample of its groups reaches, {:#x}+{:#x}, crosses a four-gigabyte span (D951)",
            read.0, read.1
        )
        .into());
    }
    let widened = with_margin(read, memory);
    Ok(GlobalRange {
        range: if within(widened) { widened } else { read },
        read,
        sampled: reached.sampled,
        pages: Vec::new(),
    })
}

/// A buffer descriptor's fourth word for a draw with these user-data words - its format and
/// selects (D738) - where the source has one and it can be found. A computed descriptor the program
/// branches past, from the system registers `system` seeds, is null: zero (D800).
pub(crate) fn resolve_fourth_word(
    source: &BufferSource,
    draw: (&[u32; USER_DATA_WORDS], &[(u32, u32)]),
    memory: &impl GuestMemory,
) -> Option<u32> {
    fourth_word_or_why(source, draw, memory).ok()
}

/// [`resolve_fourth_word`], or why the word cannot be found: what a refused format load reports
/// for the slot it reads through.
///
/// # Errors
///
/// What kept the source's fourth word from being read at this draw.
pub(crate) fn fourth_word_or_why(
    source: &BufferSource,
    (user_data, system): (&[u32; USER_DATA_WORDS], &[(u32, u32)]),
    memory: &impl GuestMemory,
) -> Result<u32, Refusal> {
    match source {
        BufferSource::Descriptor { words, .. } => resolve_word(words[3], user_data, memory)
            .map_err(|why| format!("its descriptor's fourth word {:?}: {why}", words[3]).into()),
        BufferSource::Computed(computed)
            if matches!(computed.through, ComputedAccess::Descriptor { .. }) =>
        {
            evaluate_computed(computed, (user_data, system), memory)
                .map(|found| found.map_or(0, |(words, _)| words[3]))
                .map_err(|why| format!("its computed descriptor: {why}").into())
        }
        // A scalar load's base names no format, and a global load has no descriptor.
        BufferSource::Computed(_) => {
            Err("it is a scalar load's base, which names no format".into())
        }
        BufferSource::Pointer { .. } | BufferSource::Global(_) => {
            Err("it is a pointer or a global access, which has no descriptor".into())
        }
    }
}

/// A table or scalar-load base's 64-bit address for a draw with these user-data words.
fn table_address(table: TableBase, user_data: &[u32; USER_DATA_WORDS]) -> u64 {
    let half = |word: TableWord| match word {
        TableWord::UserData(index) => usize::try_from(index)
            .ok()
            .and_then(|index| user_data.get(index).copied())
            .unwrap_or(0),
        TableWord::Constant(value) => value,
    };
    u64::from(half(table.low)) | u64::from(half(table.high)) << 32
}

/// The guest range a source names for a draw of `vertices` vertices with these user-data words and
/// system registers: its address and length. A computed access the program branches past binds
/// nothing (D800).
///
/// # Errors
///
/// A refusal naming why the range cannot be bound exactly.
pub(crate) fn resolve_range(
    source: &BufferSource,
    draw: (&[u32; USER_DATA_WORDS], u32, &[(u32, u32)]),
    memory: &impl GuestMemory,
) -> Result<(u64, u64), Refusal> {
    resolve_draw_range(source, draw, None, memory)
}

/// [`resolve_range`] for a draw whose vertex lanes hold the ids `ids` names (D879): an indexed
/// access by them reaches only the records they index.
fn resolve_draw_range(
    source: &BufferSource,
    draw: (&[u32; USER_DATA_WORDS], u32, &[(u32, u32)]),
    ids: Option<DrawIds>,
    memory: &impl GuestMemory,
) -> Result<(u64, u64), Refusal> {
    span(Span::BufferResolve, || {
        resolve_range_now(source, draw, ids, memory)
    })
}

/// The last vertex id and instance id a non-indexed draw's vertex lanes hold (D879).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DrawIds {
    vertex: u32,
    instance: u32,
}

impl DrawIds {
    /// A draw's: its vertex ids count up from its first vertex (D761), a split draw's through its
    /// last chunk (D741, D787), and its instances from zero, one at least (D863). [`None`] for an
    /// indexed draw, whose vertex ids are its indices, for a draw of no vertices, and where the ids
    /// would wrap.
    fn of(command: &RenderCommand) -> Option<Self> {
        let (last, instances) = match *command {
            RenderCommand::Draw {
                vertices,
                instances,
                first_vertex,
            } => (
                u64::from(first_vertex) + u64::from(vertices.checked_sub(1)?),
                instances,
            ),
            RenderCommand::DrawChunks {
                chunks,
                vertices,
                last,
                instances,
                first_vertex,
                stride,
            } => {
                let first_chunk = u64::from(first_vertex) + u64::from(vertices.checked_sub(1)?);
                let last_chunk = u64::from(first_vertex)
                    + u64::from(chunks.checked_sub(1)?) * u64::from(stride)
                    + u64::from(last.checked_sub(1)?);
                (first_chunk.max(last_chunk), instances)
            }
            _ => return None,
        };
        Some(Self {
            vertex: u32::try_from(last).ok()?,
            instance: instances.max(1) - 1,
        })
    }

    /// The last record an access indexing by `index` reaches for a draw with these user-data
    /// words: the id plus the word added to it. [`None`] where the word cannot be read or the sum
    /// would wrap, as the hardware's 32-bit index does.
    fn last_index(
        self,
        index: RecordIndex,
        user_data: &[u32; USER_DATA_WORDS],
        memory: &impl GuestMemory,
    ) -> Option<u64> {
        let (id, plus) = match index {
            RecordIndex::Vertex(plus) => (self.vertex, plus),
            RecordIndex::Instance(plus) => (self.instance, plus),
        };
        let plus = match plus {
            Some(word) => resolve_word(word, user_data, memory).ok()?,
            None => 0,
        };
        let last = u64::from(id) + u64::from(plus);
        u32::try_from(last).is_ok().then_some(last)
    }
}

/// [`resolve_range`], timed there.
fn resolve_range_now(
    source: &BufferSource,
    (user_data, vertices, system): (&[u32; USER_DATA_WORDS], u32, &[(u32, u32)]),
    ids: Option<DrawIds>,
    memory: &impl GuestMemory,
) -> Result<(u64, u64), Refusal> {
    let last_index = |reads: DescriptorReads| {
        ids.zip(reads.index)
            .and_then(|(ids, index)| ids.last_index(index, user_data, memory))
    };
    let (address, length) = match source {
        BufferSource::Global(global) => global_range(global, user_data, vertices, memory)?,
        BufferSource::Pointer { base, extent } => {
            (table_address(*base, user_data), u64::from(*extent))
        }
        // From the descriptor's base, through the scalar offset the shader adds to every address
        // and the extent the descriptor admits past it.
        BufferSource::Computed(computed) => {
            let Some((values, soffset)) = evaluate_computed(computed, (user_data, system), memory)?
            else {
                return Ok((0, 0));
            };
            let last = match computed.through {
                ComputedAccess::Descriptor { reads, .. } => last_index(reads),
                ComputedAccess::Pointer { .. } => None,
            };
            computed_range(computed.through, (values, soffset), last)?
        }
        BufferSource::Descriptor { words, reads } => {
            let (words, reads) = (*words, *reads);
            let mut values = [0u32; 4];
            for (value, word) in values.iter_mut().zip(words) {
                *value = resolve_word(word, user_data, memory)?;
            }
            let base = decode_buffer_descriptor(values).base;
            (
                crate::pipeline::guest_address_of(base),
                descriptor_extent_to(values, reads, last_index(reads))?,
            )
        }
    };
    checked_range((address, length))
}

/// `(address, length)` where one binding can hold it: the module reads whole words counted from the
/// range's first byte, as the hardware does from a word-aligned base, and no more than
/// [`MOST_BYTES`] (D733).
fn checked_range((address, length): (u64, u64)) -> Result<(u64, u64), Refusal> {
    if address % 4 != 0 {
        return Err("a draw buffer whose base is not word-aligned (D733)".into());
    }
    if length > MOST_BYTES {
        return Err("a draw buffer larger than one draw binds (D733)".into());
    }
    Ok((address, length))
}

/// The range a computed access binds through the descriptor or base `values` and scalar offset
/// `soffset` the program reaches it with: from the descriptor's base, through the offset the shader
/// adds to every address and the extent the descriptor admits past it - the records through
/// `last_index` where the draw's ids bound them (D879). At an offset the host cannot know, the
/// whole buffer, wherever the access lands in it (D901). A scalar load reads its words from the base
/// plus the offset, which the translated load does not add: its range starts there.
fn computed_range(
    through: ComputedAccess,
    (values, soffset): ([u32; 4], Option<u32>),
    last_index: Option<u64>,
) -> Result<(u64, u64), Refusal> {
    let base = crate::pipeline::guest_address_of(decode_buffer_descriptor(values).base);
    Ok(match (through, soffset) {
        (ComputedAccess::Descriptor { reads, .. }, Some(soffset)) => (
            base,
            u64::from(soffset) + descriptor_extent_to(values, reads, last_index)?,
        ),
        (ComputedAccess::Descriptor { .. }, None) => {
            (base, decode_buffer_descriptor(values).byte_len())
        }
        (ComputedAccess::Pointer { end, .. }, Some(soffset)) => {
            let base = u64::from(values[0]) | u64::from(values[1] & 0xffff) << 32;
            (base.wrapping_add(u64::from(soffset)), u64::from(end))
        }
        (ComputedAccess::Pointer { .. }, None) => {
            return Err("a scalar load's base at an offset the host cannot know".into());
        }
    })
}

/// The range a computed access binds from what a dispatch's waves reach it with, run on the host
/// (D900): nothing where no wave reaches it - its program branches past it in every one - the one
/// descriptor's range where every wave reaches it with the same, refused where they reach it with
/// several, one binding holding one buffer.
///
/// # Errors
///
/// A refusal naming how many descriptors the waves reach it with, or why the one cannot be bound.
pub(crate) fn reached_range(
    computed: &Computed,
    reached: &ReachedDescriptors,
) -> Result<(u64, u64), Refusal> {
    let mut found = reached.descriptors.iter();
    match (found.next(), found.next()) {
        (None, _) => Ok((0, 0)),
        (Some(&(values, soffset)), None) => {
            checked_range(computed_range(computed.through, (values, Some(soffset)), None)?)
        }
        (Some(_), Some(_)) => Err(format!(
            "its waves reach it with {} distinct descriptors, which one binding does not hold (D900)",
            reached.descriptors.len()
        )
        .into()),
    }
}

/// The ids a fan's chunk reads as a list (D898): `primitives` triangles from the fan's `start`-th,
/// each the fan's first index and the two after the triangle's own, rotated as `Assembly::Fan`
/// rotates them; its `k`-th index through `index`. `None` where one cannot be read.
fn fan_ids(
    index: impl Fn(u32) -> Option<u32>,
    start: u32,
    primitives: u32,
    provoking_last: bool,
) -> Option<Vec<u32>> {
    let pivot = index(0)?;
    let mut ids = Vec::with_capacity(primitives as usize * 3);
    for triangle in start..start + primitives {
        let (next, after) = (index(triangle + 1)?, index(triangle + 2)?);
        ids.extend(if provoking_last {
            [pivot, next, after]
        } else {
            [next, after, pivot]
        });
    }
    Some(ids)
}

/// The vertex ids a chunk reads in place of the draw's indices: a non-indexed draw's `count`
/// thirty-two-bit ids counting up from its first (D741), or a fan's triangles written out (D898);
/// empty for anything else.
fn chunk_indices(
    indices: Option<Result<(IndexBuffer, u32), &'static str>>,
    memory: &impl GuestMemory,
) -> Result<DrawBuffer, Refusal> {
    let ids: Vec<u32> = match indices {
        Some(Ok((IndexBuffer::Counting { first }, count))) => {
            (0..count).map(|id| first.wrapping_add(id)).collect()
        }
        Some(Ok((
            IndexBuffer::Fan {
                address,
                bytes,
                first,
                start,
                provoking_last,
            },
            count,
        ))) => {
            let index = |at: u32| match address {
                Some(address) => {
                    let place = address + u64::from(at) * u64::from(bytes);
                    let read = memory.read(place, bytes as usize)?;
                    Some(match *read {
                        [low, high] => u32::from(u16::from_le_bytes([low, high])),
                        [a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                        _ => return None,
                    })
                }
                None => Some(first.wrapping_add(at)),
            };
            fan_ids(index, start, count / 3, provoking_last)
                .ok_or("a fan whose indices are not readable (D898)")?
        }
        _ => Vec::new(),
    };
    let bytes: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
    let mut hasher = crate::ContentHasher::new(ids.len());
    hasher.bytes(&bytes);
    Ok(DrawBuffer {
        hash: hasher.finish(),
        bytes: bytes.into(),
        base: 0,
        stored: false,
    })
}

/// The translated primitive shaders that read their vertex ids from the draw's index buffer (D740),
/// and the assembly each one translated for a draw's geometry takes its primitives by (D800).
pub type PrimitiveShaders<'a> = (
    &'a std::collections::BTreeSet<ResourceId>,
    &'a BTreeMap<ResourceId, orbistoun_translate::wavefront::Assembly>,
);

/// The system registers a module translated for `assembly` seeds for a draw of `vertices`
/// vertices, for the host replay of its program (D800); none for a module with no geometry.
fn system_scalars(
    assembly: Option<&orbistoun_translate::wavefront::Assembly>,
    vertices: u32,
) -> Vec<(u32, u32)> {
    assembly.map_or_else(Vec::new, |assembly| {
        orbistoun_translate::wavefront::geometry_system_scalars(
            vertices,
            assembly.primitives_from(vertices),
        )
        .to_vec()
    })
}

/// The lanes a primitive shader's draw runs as vertices (D758): a split draw's chunks run as many
/// as its full ones (D787).
fn draw_lanes(command: &RenderCommand) -> u32 {
    match command {
        RenderCommand::Draw { vertices, .. } | RenderCommand::DrawChunks { vertices, .. } => {
            *vertices
        }
        RenderCommand::DrawIndexed { indices, .. } => *indices,
        _ => 0,
    }
}

/// Inserts a [`RenderCommand::BindDrawBuffers`] before each draw whose shaders read through
/// buffers, for each stage whose buffers differ from the ones last bound, so every draw reads the
/// ranges its own user data names. A draw whose buffers cannot all be bound is counted in
/// `refused`, with the first reason, and binds none for that stage. A primitive shader's program
/// is replayed from the system registers its module seeds for the draw, by the assembly in
/// `assemblies` it was translated for (D800).
pub fn bind_draw_buffers(
    commands: &mut Vec<RenderCommand>,
    (sources, (index_readers, assemblies), cache): (
        &BTreeMap<ResourceId, Vec<BufferSource>>,
        PrimitiveShaders<'_>,
        &mut BufferCache,
    ),
    memory: &impl GuestMemory,
    refused: &mut (usize, Option<Refusal>),
) {
    const STAGES: [ShaderStage; 2] = [ShaderStage::Vertex, ShaderStage::Fragment];
    let index = |stage: ShaderStage| STAGES.iter().position(|s| *s == stage);
    let mut words = [[0u32; USER_DATA_WORDS]; 2];
    let mut modules: [Option<ResourceId>; 2] = [None; 2];
    let mut bound: [Option<Vec<(u64, usize, u64)>>; 2] = [None, None];
    // Ranges read this submission: guest memory does not change while it is prepared.
    let mut read: BTreeMap<(u64, u64), Result<DrawBuffer, Refusal>> = BTreeMap::new();
    // And for the same reason, each descriptor's range is resolved once for the words it is formed
    // from, not once a draw.
    let mut resolved = resolved::Resolved::default();
    let mut out = Vec::with_capacity(commands.len());
    for command in commands.drain(..) {
        match &command {
            RenderCommand::SetUserData { stage, words: set } => {
                if let Some(at) = index(*stage) {
                    words[at] = *set;
                }
            }
            RenderCommand::BindShader { stage, shader } => {
                if let Some(at) = index(*stage) {
                    modules[at] = Some(*shader);
                }
            }
            RenderCommand::Draw { .. }
            | RenderCommand::DrawIndexed { .. }
            | RenderCommand::DrawChunks { .. } => {
                let (vertices, ids) = (draw_lanes(&command), DrawIds::of(&command));
                for (at, stage) in STAGES.into_iter().enumerate() {
                    let empty = Vec::new();
                    let module = modules[at];
                    let list = module.and_then(|m| sources.get(&m)).unwrap_or(&empty);
                    // An indexed primitive shader reads its index buffer after its traced buffers
                    // (D740).
                    let indices = match &command {
                        RenderCommand::DrawIndexed {
                            indices,
                            index_buffer,
                            ..
                        } if module.is_some_and(|m| index_readers.contains(&m)) => Some(
                            index_buffer
                                .map(|buffer| (buffer, *indices))
                                .ok_or("an indexed draw whose index size is not read (D740)"),
                        ),
                        _ => None,
                    };
                    if list.is_empty() && indices.is_none() {
                        continue;
                    }
                    let system = system_scalars(module.and_then(|m| assemblies.get(&m)), vertices);
                    let ranges = list
                        .iter()
                        .map(|source| {
                            resolved
                                .range(source, (&words[at], vertices, &system), ids, memory)
                                .map(Some)
                        })
                        .chain(indices.map(|found| {
                            found
                                .map_err(Into::into)
                                .map(|(buffer, count)| match buffer {
                                    IndexBuffer::Guest { address, bytes } => {
                                        Some((address, u64::from(count) * u64::from(bytes)))
                                    }
                                    IndexBuffer::Counting { .. } | IndexBuffer::Fan { .. } => None,
                                })
                        }));
                    let resolved: Result<Vec<DrawBuffer>, Refusal> = ranges
                        .map(|range| {
                            let Some(range) = range? else {
                                return chunk_indices(indices, memory);
                            };
                            read.entry(range)
                                .or_insert_with(|| {
                                    let length = usize::try_from(range.1)
                                        .map_err(|_| "a draw buffer larger than this host holds")?;
                                    Ok(cache.read(range.0, length, memory)?)
                                })
                                .clone()
                        })
                        .collect();
                    match resolved {
                        Ok(buffers) => {
                            let key: Vec<(u64, usize, u64)> = buffers
                                .iter()
                                .map(|b| (b.hash, b.bytes.len(), b.base))
                                .collect();
                            if bound[at].as_ref() != Some(&key) {
                                out.push(RenderCommand::BindDrawBuffers { stage, buffers });
                                bound[at] = Some(key);
                            }
                        }
                        Err(why) => {
                            refused.0 += 1;
                            refused
                                .1
                                .get_or_insert_with(|| named_refusal(stage, module, &why));
                        }
                    }
                }
            }
            _ => {}
        }
        out.push(command);
    }
    *commands = out;
}

/// A draw's buffer refusal with the stage and module it was refused for, as the refused
/// submission's listing names the module beside the program it binds.
fn named_refusal(stage: ShaderStage, module: Option<ResourceId>, why: &Refusal) -> Refusal {
    format!(
        "the {} module {}: {why}",
        format!("{stage:?}").to_lowercase(),
        module.map_or_else(|| "none".to_owned(), |m| format!("{m:?}"))
    )
    .into()
}

#[cfg(test)]
mod cache_tests {
    use super::{BufferCache, CACHE_BYTES};
    use crate::backend::DrawBuffer;

    fn buffer(bytes: usize) -> DrawBuffer {
        DrawBuffer {
            hash: bytes as u64,
            bytes: vec![0u8; bytes].into(),
            base: 0,
            stored: false,
        }
    }

    /// A world's many vertex buffers all stay while they fit the budget; past it the oldest go
    /// first, and a range kept again is not let go for its first place.
    #[test]
    fn the_cache_keeps_every_range_it_has_room_for() {
        let mut cache = BufferCache::default();
        for range in 0..300u64 {
            cache.keep((range * 0x1000, 64), None, buffer(64));
        }
        assert_eq!(
            cache.entries.len(),
            300,
            "three hundred small ranges all kept"
        );
        cache.keep((0, 64), None, buffer(64));
        let big = CACHE_BYTES - 64;
        cache.keep((0x100_0000, big), None, buffer(big));
        assert!(
            cache.entries.contains_key(&(0, 64)),
            "kept again, so newest but one"
        );
        assert!(cache.entries.contains_key(&(0x100_0000, big)));
        assert!(
            !cache.entries.contains_key(&(0x1000, 64)),
            "the oldest went first"
        );
        assert!(cache.bytes <= CACHE_BYTES);
    }

    /// Constant buffers rewritten every frame, far within the bytes, leave the order no longer
    /// than a bound over the ranges kept, however many frames go by; and the oldest still goes
    /// first once the bytes are passed.
    #[test]
    fn ranges_kept_again_every_frame_leave_a_bounded_order() {
        let mut cache = BufferCache::default();
        for _frame in 0..200 {
            for range in 0..50u64 {
                cache.keep((range * 0x1000, 256), None, buffer(256));
            }
        }
        assert_eq!(cache.entries.len(), 50);
        assert!(
            cache.order.len() <= 2 * 50 + super::STALE_PLACES + 1,
            "{} places for 50 ranges",
            cache.order.len()
        );
        let big = CACHE_BYTES - 49 * 256;
        cache.keep((0x100_0000, big), None, buffer(big));
        assert!(!cache.entries.contains_key(&(0, 256)), "the oldest went");
        assert!(cache.entries.contains_key(&(49 * 0x1000, 256)));
        assert!(cache.bytes <= CACHE_BYTES);
    }

    /// A small range is compared, not watched: read again unchanged it is the very buffer kept,
    /// and changed it is a new one, with its own hash.
    #[test]
    fn a_small_range_is_kept_while_its_bytes_compare_equal() {
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let mut memory = Memory((0..0x40_u32).map(|byte| byte as u8).collect());
        let mut cache = BufferCache::default();
        let first = cache.read(0x1000, 0x3e, &memory).expect("read");
        let again = cache.read(0x1000, 0x3e, &memory).expect("read");
        assert!(
            std::sync::Arc::ptr_eq(&first.bytes, &again.bytes),
            "the same buffer"
        );
        assert_eq!(first.bytes.len(), 0x40, "padded to whole words");
        memory.0[0x10] ^= 0xff;
        let changed = cache.read(0x1000, 0x3e, &memory).expect("read");
        assert_ne!(changed.hash, first.hash, "a new buffer for new bytes");
        assert_eq!(changed.bytes[0x10], 0x10 ^ 0xff);
    }
}

#[cfg(test)]
mod tests {
    use super::descriptor_extent;
    use orbistoun_translate::draw_buffers::DescriptorReads;

    /// A sampled replay probes the first wave of each of the first groups, the same entries the
    /// whole grid thinned out gives, without making the grid's waves.
    #[test]
    fn a_sample_is_the_first_wave_of_the_first_groups() {
        let inputs = orbistoun_translate::wavefront::ComputeInputs {
            workgroup_ids: [true, true, false],
            thread_id_components: 2,
            threads: [104, 1, 1],
            ..orbistoun_translate::wavefront::ComputeInputs::default()
        };
        let groups = [5, 3, 1];
        let waves = usize::try_from(inputs.waves(64)).expect("a few waves");
        let thinned: Vec<_> = super::dispatch_waves(inputs, groups, 64)
            .into_iter()
            .step_by(waves)
            .take(7)
            .collect();
        assert_eq!(
            super::first_waves_of_first_groups(inputs, groups, 64, 7),
            thinned
        );
        assert_eq!(
            super::first_waves_of_first_groups(inputs, groups, 64, 100).len(),
            15,
            "no more than the grid holds"
        );
    }

    /// A chunk of a fan reads its triangles as a list (D898): from its first triangle on, each the
    /// fan's first index and the two after the triangle's own, rotated for the provoking
    /// convention as `Assembly::Fan` orders them.
    #[test]
    fn a_fan_chunk_s_ids_are_its_triangles_written_out() {
        let indices = [10_u32, 11, 12, 13, 14];
        let index = |at: u32| indices.get(at as usize).copied();
        assert_eq!(
            super::fan_ids(index, 1, 2, true),
            Some(vec![10, 12, 13, 10, 13, 14])
        );
        assert_eq!(
            super::fan_ids(index, 1, 2, false),
            Some(vec![12, 13, 10, 13, 14, 10])
        );
        assert_eq!(super::fan_ids(index, 2, 2, true), None, "past the indices");
    }

    /// A range running past the guest memory that holds it binds the part held (D795): mesa-cube's
    /// vertex buffer starts eight bytes into its mapping and its records run eight bytes past it.
    /// Nothing mapped at the base is still refused.
    #[test]
    fn a_range_past_its_mapping_binds_the_part_mapped() {
        struct Memory(Vec<u8>);
        impl super::GuestMemory for Memory {
            fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
                let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
                self.0.get(start..start.checked_add(length)?)
            }
        }
        let memory = Memory((0..0x100_u32).map(|byte| byte as u8).collect());
        let mut cache = super::BufferCache::default();
        let bound = cache
            .read(0x1008, 0x100, &memory)
            .expect("the mapped part binds");
        assert_eq!(bound.bytes.len(), 0xf8, "up to the mapping's end");
        assert_eq!(bound.bytes[0], 8, "from the range's base");
        assert!(
            cache.read(0x2000, 0x10, &memory).is_err(),
            "nothing mapped there"
        );
    }

    /// radeonsi's raw out-of-bounds mode, in a descriptor's fourth word: `OOB_SELECT` 3 in bits
    /// 29:28 (Mesa `gfx10-rsrc.json`, `SQ_BUF_RSRC_WORD3`), as its fast-path constant buffer has.
    const RAW: u32 = 0x3101_6fac;
    /// The same flags with `OOB_SELECT` 0, structured with offset.
    const STRUCTURED_WITH_OFFSET: u32 = RAW & !(3 << 28);
    /// With `OOB_SELECT` 1, structured.
    const STRUCTURED: u32 = STRUCTURED_WITH_OFFSET | 1 << 28;

    fn scalar(reach: u32) -> DescriptorReads {
        DescriptorReads {
            scalar_reach: reach,
            ..DescriptorReads::default()
        }
    }

    fn vector(reach: Option<u32>, indexed: bool) -> DescriptorReads {
        DescriptorReads {
            scalar_reach: 0,
            vector: true,
            vector_reach: reach,
            indexed,
            index: None,
        }
    }

    /// The clear's constant buffer: sixteen bytes, all of which its one load reads.
    #[test]
    fn a_scalar_load_reaches_the_record_count_or_its_own_end() {
        assert_eq!(descriptor_extent([0, 4, 16, RAW], scalar(16)), Ok(16));
        assert_eq!(descriptor_extent([0, 4, 64, RAW], scalar(16)), Ok(16));
        assert_eq!(descriptor_extent([0, 4, 8, RAW], scalar(16)), Ok(8));
    }

    /// Through a descriptor with a stride the record count is in units of the stride (radeonsi
    /// `si_state.c:3506-3527`, GFX6-7 and 10, and GFX9's SMEM), so a scalar load reaches its own end
    /// or the records' bytes: the AGC formatted copy's constants, one sixteen-byte record read for
    /// eight bytes (PPSA03416), reach eight.
    #[test]
    fn a_scalar_load_through_a_stride_counts_records_in_strides() {
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 16, RAW], scalar(16)),
            Ok(16)
        );
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 1, RAW], scalar(8)),
            Ok(8)
        );
        assert_eq!(
            descriptor_extent([0, 4 | 16 << 16, 1, RAW], scalar(32)),
            Ok(16)
        );
    }

    /// With no stride, `OOB_SELECT` 1 bounds nothing on hardware, and the host binds the record count
    /// in bytes as the table the program reads (D917): Grand Theft Auto V's 32-byte table of its own
    /// constant data.
    #[test]
    fn a_strideless_structured_read_reaches_its_records() {
        assert_eq!(
            descriptor_extent([0, 4, 32, STRUCTURED], vector(None, false)),
            Ok(32)
        );
        assert_eq!(
            descriptor_extent([0, 4, 32, STRUCTURED], vector(Some(8), false)),
            Ok(8)
        );
    }

    /// An indexed fetch reaches the last record's start plus its own reach into a record.
    #[test]
    fn an_indexed_fetch_reaches_into_the_last_record() {
        let stride = 24;
        let words = |flags| [0, 4 | stride << 16, 10, flags];
        assert_eq!(
            descriptor_extent(words(STRUCTURED), vector(Some(12), true)),
            Ok(9 * 24 + 12)
        );
        assert_eq!(
            descriptor_extent(words(STRUCTURED_WITH_OFFSET), vector(Some(12), true)),
            Ok(9 * 24 + 12)
        );
        // An offset in a register is bounded by the stride where the mode checks it, and taken to
        // lie within its record where it does not (D946).
        assert_eq!(
            descriptor_extent(words(STRUCTURED_WITH_OFFSET), vector(None, true)),
            Ok(9 * 24 + 23 + 16)
        );
        assert_eq!(
            descriptor_extent(words(STRUCTURED), vector(None, true)),
            Ok(10 * 24)
        );
    }

    /// An indexed fetch the draw's ids bound reaches through their last record, and no further
    /// than the descriptor's (D879).
    #[test]
    fn a_fetch_the_draw_s_ids_bound_reaches_their_last_record() {
        use super::descriptor_extent_to;
        let words = [0, 4 | 24 << 16, 10, STRUCTURED];
        let reads = vector(Some(12), true);
        assert_eq!(descriptor_extent_to(words, reads, Some(2)), Ok(2 * 24 + 12));
        assert_eq!(descriptor_extent_to(words, reads, Some(0)), Ok(12));
        assert_eq!(
            descriptor_extent_to(words, reads, Some(500)),
            Ok(9 * 24 + 12),
            "capped at the last record"
        );
        assert_eq!(
            descriptor_extent_to(words, vector(Some(12), false), Some(500)),
            Ok(12),
            "an access that does not index reads the first record"
        );
    }

    /// MCUB00001's fetch (D879): a vertex buffer descriptor running from the draw's base to the end
    /// of Mesa's upload ring binds only the vertices the draw names - its vertex ids plus the base
    /// vertex in user data. An indexed draw, or one whose ids would wrap, binds the whole.
    #[test]
    fn a_draw_binds_the_vertices_its_ids_index() {
        use super::{DrawIds, RenderCommand};
        use orbistoun_translate::draw_buffers::{BufferSource, DescriptorWord, RecordIndex};
        let source = BufferSource::Descriptor {
            words: [0, 1, 2, 3].map(DescriptorWord::UserData),
            reads: DescriptorReads {
                index: Some(RecordIndex::Vertex(Some(DescriptorWord::UserData(5)))),
                ..vector(Some(16), true)
            },
        };
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        // Sixteen-byte records, a hundred thousand of them, and the base vertex 4.
        user_data[..4].copy_from_slice(&[0x10_0000, 16 << 16, 100_000, STRUCTURED]);
        user_data[5] = 4;
        let memory = At(0, Vec::new());
        let draw = |command: &RenderCommand| {
            super::resolve_draw_range(&source, (&user_data, 0, &[]), DrawIds::of(command), &memory)
                .map(|(_, length)| length)
        };
        let triangle = RenderCommand::Draw {
            vertices: 3,
            instances: 1,
            first_vertex: 0,
        };
        assert_eq!(draw(&triangle), Ok((4 + 2) * 16 + 16));
        let chunks = RenderCommand::DrawChunks {
            chunks: 3,
            vertices: 96,
            last: 12,
            instances: 1,
            first_vertex: 0,
            stride: 96,
        };
        assert_eq!(draw(&chunks), Ok((4 + 2 * 96 + 11) * 16 + 16));
        let indexed = RenderCommand::DrawIndexed {
            indices: 3,
            instances: 1,
            first_index: 0,
            index_buffer: None,
        };
        assert_eq!(draw(&indexed), Ok(99_999 * 16 + 16));
        user_data[5] = u32::MAX;
        let wrapping = super::resolve_draw_range(
            &source,
            (&user_data, 0, &[]),
            DrawIds::of(&triangle),
            &memory,
        );
        assert_eq!(wrapping.map(|(_, length)| length), Ok(99_999 * 16 + 16));
    }

    /// A raw-checked buffer indexed by record has no bound on the index, and is refused.
    #[test]
    fn a_raw_buffer_indexed_by_record_is_refused() {
        assert!(descriptor_extent([0, 4 | 24 << 16, 10, RAW], vector(Some(4), true)).is_err());
        assert_eq!(
            descriptor_extent([0, 4, 100, RAW], vector(None, false)),
            Ok(100)
        );
    }

    /// Swizzled records are refused rather than bound as a plain range.
    #[test]
    fn swizzled_addressing_is_refused() {
        assert!(descriptor_extent([0, 4 | 1 << 31, 16, RAW], scalar(16)).is_err());
    }

    /// A dispatch's global loads bind the span every wave of every group forms (D883): each lane's
    /// address is `s[0:1]` plus four times its thread id plus its group's id (`s2`), and the byte
    /// loads' span ends at the word their last byte is in. A dispatch of more waves than one
    /// preparation runs binds what a sample of its groups reads (D950).
    #[test]
    fn a_dispatchs_global_loads_bind_what_every_wave_reads() {
        use orbistoun_shader::EncodingTable;
        use orbistoun_translate::draw_buffers::GlobalLoads;
        use orbistoun_translate::wavefront::ComputeInputs;
        let encodings = EncodingTable::builtin().expect("encodings");
        let short_form = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
            let (family, opcode) = encodings.find_by_name(name).expect(name);
            let encoding = encodings
                .encodings()
                .iter()
                .find(|encoding| encoding.name == family)
                .expect("its family");
            encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
        };
        let global = GlobalLoads {
            prefix: vec![
                short_form("v_lshlrev_b32_e32", 1, 130, 0),
                short_form("v_add_nc_u32_e32", 1, 0, 1),
                short_form("v_add_nc_u32_e32", 1, 2, 1),
                short_form("v_mov_b32_e32", 2, 1, 0),
                // global_load_ubyte v3, v[1:2], off.
                0xdc20_0000,
                0x037d_0001,
            ],
            loads: vec![(16, 1)],
            first_register: 0,
            user_data: 2,
            width: 64,
        };
        // Groups of 104 threads, two waves each, the group's x id after two user-data words.
        let inputs = ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: 1,
            threads: [104, 1, 1],
            ..ComputeInputs::default()
        };
        let memory = At(0, Vec::new());
        let range =
            super::dispatch_global_range(&global, (&[0x1000, 7], inputs, [2, 1, 1], 64), &memory);
        // Group 1's thread 103 reads the last byte, 0x19d past the base.
        assert_eq!(
            range,
            Ok(super::GlobalRange {
                range: (0x7_0000_1000, 0x1a0),
                read: (0x7_0000_1000, 0x1a0),
                sampled: None,
                pages: vec![0x7_0000_1000],
            })
        );
        // A refusal says what the run seeded each wave with, so a register past it reads as one the
        // dispatch's entry does not fill: here an address from `s5`, past the user data and the id.
        let past = GlobalLoads {
            prefix: vec![
                short_form("v_mov_b32_e32", 1, 5, 0),
                short_form("v_mov_b32_e32", 2, 5, 0),
                0xdc20_0000,
                0x037d_0001,
            ],
            loads: vec![(8, 1)],
            ..global.clone()
        };
        let refused =
            super::dispatch_global_range(&past, (&[0x1000, 7], inputs, [1, 1, 1], 64), &memory)
                .expect_err("s5 is unknown");
        assert!(
            refused.contains(concat!(
                "a wave of this dispatch starts with its user data in s0 to s1, its group's x id ",
                "in s2, and its thread ids in v0 (one component)"
            )),
            "{refused}"
        );
        // A wave of a larger streamout draw numbers its threads from its first (D861).
        let later = ComputeInputs {
            first_thread: 1000,
            ..inputs
        };
        assert_eq!(
            super::dispatch_global_range(&global, (&[0x1000, 7], later, [2, 1, 1], 64), &memory)
                .map(|found| found.range),
            Ok((0x7_0000_1000 + 4000, 0x1a0))
        );
        // More waves than a preparation runs: a sample of the groups, the last among them (D950).
        let sampled = super::dispatch_global_range(
            &global,
            (&[0x1000, 7], inputs, [5000, 1, 1], 64),
            &memory,
        )
        .expect("sampled");
        // Group 4999's thread 103 reads the last byte, 0x1523 past the base.
        assert_eq!(sampled.read, (0x7_0000_1000, 0x1524));
        assert!(sampled.sampled.is_some_and(|groups| groups < 5000));
    }

    /// A global load through a pointer each lane loaded from a buffer binds what every lane's
    /// pointer reaches, as Grand Theft Auto V's compute program at 0x3cf9d00 loads each object's
    /// pointer at its thread's index and reads the object: the index each lane loads at is in the
    /// address's slice, so the run computes it rather than leaving it zero, which read lane zero's
    /// pointer in every lane and bound one object.
    #[test]
    fn a_global_load_through_loaded_pointers_binds_every_lanes_object() {
        use orbistoun_shader::EncodingTable;
        use orbistoun_translate::draw_buffers::GlobalLoads;
        use orbistoun_translate::wavefront::ComputeInputs;
        const TABLE: u64 = 0x1000;
        const OBJECTS: u64 = 0x2000;
        let encodings = EncodingTable::builtin().expect("encodings");
        let (family, opcode) = encodings
            .find_by_name("v_lshlrev_b32_e32")
            .expect("v_lshlrev_b32_e32");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        let global = GlobalLoads {
            prefix: vec![
                // v_lshlrev_b32 v5, 3, v0: each lane's byte in the table.
                encoding.value | (opcode << encoding.opcode.shift) | (5 << 17) | 0x83,
                // buffer_load_dwordx2 v[2:3], v5, s[4:7], 0 offen.
                0xe034_1000,
                0x8001_0205,
                // s_waitcnt vmcnt(0).
                0xbf8c_3f70,
                // global_load_dword v4, v[2:3], off.
                0xdc30_8000,
                0x047d_0002,
            ],
            loads: vec![(16, 4)],
            first_register: 0,
            user_data: 8,
            width: 64,
        };
        let inputs = ComputeInputs {
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        // Lane i's pointer, at byte 8i of the table, is to the object 0x40 bytes after lane i - 1's.
        let mut bytes = vec![0u8; 0x3000];
        for lane in 0..64 {
            let pointer = OBJECTS + lane * 0x40;
            let at = usize::try_from(lane * 8).expect("small");
            bytes[at..at + 8].copy_from_slice(&pointer.to_le_bytes());
        }
        let memory = At(TABLE, bytes);
        let user_data = [0, 0, 0, 0, TABLE as u32, 0, 64 * 8, RAW];
        let found =
            super::dispatch_global_range(&global, (&user_data, inputs, [1, 1, 1], 64), &memory)
                .expect("every lane's pointer is known");
        // Lane 63's object, 0xfc0 past lane 0's, holds the last word read.
        assert_eq!(found.range, (OBJECTS, 0xfc4));
        assert_eq!(found.pages, [OBJECTS]);
    }

    /// A dispatch of far more waves than a preparation runs, whose global loads index a table by
    /// group (D950): 240 x 135 groups of 64 threads, a 1920x1080 pass, each lane reading the word
    /// at `s[0:1]` plus sixteen times its group's x id plus 3840 times its y id plus four times
    /// its thread id. A sample of the groups runs, the corners among them, and what it reads is every
    /// group's reads exactly: from group (0, 0)'s thread 0 to group (239, 134)'s thread 63. Mapped
    /// memory about it gives the range its margin; unmapped memory gives none.
    #[test]
    fn a_dispatch_too_large_to_replay_binds_every_groups_reads_from_a_sample() {
        use orbistoun_shader::EncodingTable;
        use orbistoun_translate::draw_buffers::GlobalLoads;
        use orbistoun_translate::wavefront::ComputeInputs;
        let encodings = EncodingTable::builtin().expect("encodings");
        let short_form = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
            let (family, opcode) = encodings.find_by_name(name).expect(name);
            let encoding = encodings
                .encodings()
                .iter()
                .find(|encoding| encoding.name == family)
                .expect("its family");
            encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
        };
        // v1 = s0 + 4 * tid + 16 * s2 + 3840 * s3, v2 = s1; global_load_dword v3, v[1:2], off.
        // 3840 is a row of 240 groups' sixteen bytes; the literal follows the multiply.
        let literal = 255;
        let global = GlobalLoads {
            prefix: vec![
                short_form("v_lshlrev_b32_e32", 1, 130, 0),
                short_form("v_add_nc_u32_e32", 1, 0, 1),
                short_form("v_mov_b32_e32", 4, 2, 0),
                short_form("v_lshlrev_b32_e32", 4, 132, 4),
                short_form("v_add_nc_u32_e32", 1, 260, 1),
                short_form("v_mov_b32_e32", 5, 3, 0),
                short_form("v_mul_u32_u24_e32", 5, literal, 5),
                3840,
                short_form("v_add_nc_u32_e32", 1, 261, 1),
                short_form("v_mov_b32_e32", 2, 1, 0),
                // global_load_dword v3, v[1:2], off.
                0xdc30_0000,
                0x037d_0001,
            ],
            loads: vec![(40, 4)],
            first_register: 0,
            user_data: 2,
            width: 64,
        };
        let inputs = ComputeInputs {
            workgroup_ids: [true, true, false],
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        let base = 0x7_0010_0000_u64;
        let user_data = [base as u32, (base >> 32) as u32];
        let last = 4 * 63 + 16 * 239 + 3840 * 134;
        let read = (base, last + 4);
        let unmapped = super::dispatch_global_range(
            &global,
            (&user_data, inputs, [240, 135, 1], 64),
            &At(0, Vec::new()),
        )
        .expect("sampled");
        assert_eq!(unmapped.read, read, "every group's reads");
        assert_eq!(unmapped.range, read, "no margin where memory is not mapped");
        // A wave a group: no more groups than a preparation runs waves.
        assert!(
            unmapped
                .sampled
                .is_some_and(|groups| groups <= super::MOST_DISPATCH_WAVES),
            "{:?}",
            unmapped.sampled
        );
        // Mapped a megabyte about it, the range takes an eighth of its bytes each side, in pages.
        let around = 1 << 20;
        let mapped = At(base - around, vec![0; (read.1 + 2 * around) as usize]);
        let found =
            super::dispatch_global_range(&global, (&user_data, inputs, [240, 135, 1], 64), &mapped)
                .expect("sampled");
        let margin = (read.1 / 8).next_multiple_of(0x1000);
        assert_eq!(found.read, read);
        assert_eq!(found.range, (base - margin, read.1 + 2 * margin));
    }

    /// A dispatch whose global load loops longer than the model follows binds what every pass
    /// reads, found natively (D850, D883): a wave narrows its mask a pass at a time and leaves
    /// through `s_cbranch_execz`, as Grand Theft Auto V's copy loop does - here a hundred passes of
    /// four bytes from `s0`.
    #[test]
    fn a_dispatchs_long_loop_binds_every_pass() {
        use orbistoun_shader::EncodingTable;
        use orbistoun_translate::draw_buffers::GlobalLoads;
        use orbistoun_translate::wavefront::ComputeInputs;
        let encodings = EncodingTable::builtin().expect("encodings");
        let short_form = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
            let (family, opcode) = encodings.find_by_name(name).expect(name);
            let encoding = encodings
                .encodings()
                .iter()
                .find(|encoding| encoding.name == family)
                .expect("its family");
            encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
        };
        let (family, opcode) = encodings.find_by_name("s_andn2_b64").expect("andn2");
        let sop2 = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .map(|encoding| encoding.value | (opcode << encoding.opcode.shift))
            .expect("its family");
        let prefix = vec![
            // v_mov_b32 v6, 0.
            short_form("v_mov_b32_e32", 6, 128, 0),
            // loop: v_cmp_le_u32 vcc, s1, v6; s_andn2_b64 exec, exec, vcc; s_cbranch_execz end.
            short_form("v_cmp_le_u32_e32", 0, 1, 6),
            sop2 | 0x7e << 16 | 0x6a << 8 | 0x7e,
            0xbf88_0007,
            // v_add_nc_u32 v1, s0, v6; v_mov_b32 v2, 0; global_load_dword v9, v[1:2], off.
            short_form("v_add_nc_u32_e32", 1, 0, 6),
            short_form("v_mov_b32_e32", 2, 128, 0),
            0xdc30_8000,
            0x097d_0001,
            // v_add_nc_u32 v6, 4, v6; s_branch loop.
            short_form("v_add_nc_u32_e32", 6, 128 + 4, 6),
            0xbf82_fff7,
        ];
        let global = GlobalLoads {
            prefix,
            loads: vec![(24, 4)],
            first_register: 0,
            user_data: 2,
            width: 64,
        };
        let inputs = ComputeInputs {
            thread_id_components: 1,
            threads: [1, 1, 1],
            ..ComputeInputs::default()
        };
        let range = super::dispatch_global_range(
            &global,
            (&[0x7_0000, 400], inputs, [1, 1, 1], 64),
            &At(0, Vec::new()),
        );
        assert_eq!(range.map(|found| found.range), Ok((0x7_0000, 400)));
        // The preparation's steps bound the waves together, the model's and the native run's
        // each (D951): too few of either, and the loads are refused by name, not followed on.
        let within = |budgets: (usize, usize), groups: u32| {
            super::global_range_within(
                &global,
                (&[0x7_0000, 400], inputs, [groups, 1, 1], 64),
                budgets,
                &At(0, Vec::new()),
            )
        };
        let native = within((super::MOST_PREPARATION_STEPS, 2000), 8).expect_err("native");
        assert!(
            native.contains("the native replay ran past 2000 steps")
                && native.contains("of 8 waves"),
            "{native}"
        );
        let model = within((20, super::MOST_NATIVE_PREPARATION_STEPS), 8).expect_err("model");
        assert!(
            model.contains("the host replay ran past 20 steps") && model.contains("0 of 8 waves"),
            "{model}"
        );
    }

    /// Guest memory at one address.
    struct At(u64, Vec<u8>);
    impl crate::pipeline::GuestMemory for At {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            let start = usize::try_from(address.checked_sub(self.0)?).ok()?;
            self.1.get(start..start.checked_add(length)?)
        }
    }

    /// A computed descriptor the draw cannot know is refused naming the register and the instruction
    /// that left it unknown: `v_readlane_b32 s4, v0, 0` at byte 0 overwrites the first word of the
    /// descriptor in `s[4:7]` with a vector result, and the refusal says so rather than listing what
    /// might have.
    #[test]
    fn an_unknown_descriptor_names_the_instruction_that_left_it_so() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed};
        let encodings = orbistoun_shader::EncodingTable::builtin().expect("encodings");
        let (family, opcode) = encodings
            .find_by_name("v_readlane_b32")
            .expect("v_readlane_b32");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|encoding| encoding.name == family)
            .expect("its family");
        let source = BufferSource::Computed(Box::new(Computed {
            prefix: vec![
                encoding.value | (opcode << encoding.opcode.shift) | 4,
                256 | (128 << 9),
            ],
            at: 8,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 4,
                reads: scalar(16),
            },
            soffset: 125,
            first_register: 4,
            user_data: 4,
            program_address: None,
        }));
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[..4].copy_from_slice(&[0x100, 0, 16, RAW]);
        let refused = super::resolve_range(&source, (&user_data, 3, &[]), &At(0, Vec::new()))
            .expect_err("refused");
        assert!(
            refused.contains("s4 is unknown after `v_readlane_b32") && refused.contains("at 0x0"),
            "{refused}"
        );
    }

    /// A computed descriptor the program branches past (D800) is null: its range binds nothing and
    /// its fourth word is zero, which a format load reads as zeros. `s_mov_b32 exec_lo, s3;
    /// s_mov_b32 exec_hi, 0; s_mov_b64 vcc, exec; s_cbranch_vccnz 1` jumps over the access at 16
    /// whenever the draw's merged_wave_info is seeded, as a draw's is.
    #[test]
    fn a_computed_descriptor_no_lane_reaches_is_null() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed};
        let source = BufferSource::Computed(Box::new(Computed {
            prefix: vec![0xbefe_0303, 0xbeff_0380, 0xbeea_047e, 0xbf87_0001],
            at: 16,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 8,
                reads: vector(Some(16), true),
            },
            soffset: 125,
            first_register: 4,
            user_data: 0,
            program_address: None,
        }));
        let user_data = [0u32; super::USER_DATA_WORDS];
        let memory = At(0, Vec::new());
        let seeds = orbistoun_translate::wavefront::geometry_system_scalars(3, 1);
        assert_eq!(
            super::resolve_range(&source, (&user_data, 3, &seeds), &memory),
            Ok((0, 0))
        );
        assert_eq!(
            super::resolve_fourth_word(&source, (&user_data, &seeds), &memory),
            Some(0)
        );
        // Unseeded, the mask is unknown and so is the descriptor.
        assert!(super::resolve_range(&source, (&user_data, 3, &[]), &memory).is_err());
    }

    /// A computed descriptor (D753) is found by running the program before the access over the
    /// draw's user data and memory: here a load of the descriptor from the table the user data
    /// names. Its range runs from the descriptor's base through the scalar offset the access adds
    /// and the extent past it, and its fourth word is the format a format load reads.
    #[test]
    fn a_computed_descriptor_binds_from_its_base_past_the_offset() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed};
        let mut table = Vec::new();
        for word in [0x2000u32, 16 << 16, 4, STRUCTURED] {
            table.extend(word.to_le_bytes());
        }
        let memory = At(0x1000, table);
        let source = BufferSource::Computed(Box::new(Computed {
            // s_load_dwordx4 s[0:3], s[4:5], 0x0; s_waitcnt lgkmcnt(0)
            prefix: vec![0xf408_0002, 0xfa00_0000, 0xbf8c_c07f],
            at: 12,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 0,
                reads: vector(Some(16), true),
            },
            // The inline constant 16.
            soffset: 128 + 16,
            first_register: 4,
            user_data: 2,
            program_address: None,
        }));
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[0] = 0x1000;
        assert_eq!(
            super::resolve_range(&source, (&user_data, 0, &[]), &memory),
            Ok((0x2000, 16 + 4 * 16)),
            "four sixteen-byte records past a sixteen-byte offset"
        );
        assert_eq!(
            super::resolve_fourth_word(&source, (&user_data, &[]), &memory),
            Some(STRUCTURED)
        );
        // A scalar load's base pair, computed the same way, binds from the base past the offset,
        // as far as the load reaches.
        let pointer = BufferSource::Computed(Box::new(Computed {
            prefix: Vec::new(),
            at: 0,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Pointer {
                base: 4,
                end: 16,
            },
            soffset: 128 + 16,
            first_register: 4,
            user_data: 2,
            program_address: None,
        }));
        assert_eq!(
            super::resolve_range(&pointer, (&user_data, 0, &[]), &memory),
            Ok((0x1010, 16))
        );
        assert_eq!(
            super::resolve_fourth_word(&pointer, (&user_data, &[]), &memory),
            None
        );
        // A table the user data does not name leaves the descriptor unknown.
        user_data[0] = 0x9000;
        assert!(super::resolve_range(&source, (&user_data, 0, &[]), &memory).is_err());
    }

    /// A dispatch's computed descriptor that depends only on its group's ids is found by replaying
    /// the scalar prefix once a group, the ids in the registers after the user data (D914): Grand
    /// Theft Auto V's 0x78 x 0x44-group dispatch indexes a table by `s16` so. Groups reaching one
    /// descriptor are one; groups reaching two are both listed.
    #[test]
    fn a_group_s_descriptor_is_found_from_its_ids() {
        use orbistoun_translate::draw_buffers::Computed;
        use orbistoun_translate::wavefront::ComputeInputs;
        let computed = Computed {
            // s_lshl_b32 s6, s6, 4; s_add_u32 s4, s4, s6; s_load_dwordx4 s[0:3], s[4:5], 0x0;
            // s_waitcnt lgkmcnt(0)
            prefix: vec![
                0x8f06_8406,
                0x8004_0604,
                0xf408_0002,
                0xfa00_0000,
                0xbf8c_c07f,
            ],
            at: 20,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 0,
                reads: vector(Some(16), true),
            },
            soffset: 128,
            first_register: 4,
            user_data: 2,
            program_address: None,
        };
        let table = |second: u32| {
            let mut bytes = Vec::new();
            for word in [
                0x2000_u32,
                16 << 16,
                4,
                STRUCTURED,
                second,
                16 << 16,
                4,
                STRUCTURED,
            ] {
                bytes.extend(word.to_le_bytes());
            }
            At(0x1000, bytes)
        };
        let inputs = ComputeInputs {
            workgroup_ids: [true, false, false],
            ..ComputeInputs::default()
        };
        let same = super::dispatch_group_descriptors(
            &computed,
            (&[0x1000, 0], inputs, [2, 1, 1]),
            &table(0x2000),
        )
        .expect("found");
        assert_eq!(
            same.descriptors.into_iter().collect::<Vec<_>>(),
            [([0x2000, 16 << 16, 4, STRUCTURED], 0)]
        );
        let two = super::dispatch_group_descriptors(
            &computed,
            (&[0x1000, 0], inputs, [2, 1, 1]),
            &table(0x3000),
        )
        .expect("found");
        assert_eq!(two.descriptors.len(), 2);
        // With no group id in a register, nothing tells the groups apart.
        assert!(
            super::dispatch_group_descriptors(
                &computed,
                (&[0x1000, 0], ComputeInputs::default(), [2, 1, 1]),
                &table(0x2000),
            )
            .is_err()
        );
    }

    /// A computed descriptor the host finds, read at a scalar offset it cannot know - one the
    /// program forms from its lanes, as Grand Theft Auto V's compute programs read a descriptor
    /// table at a lane's index - binds the descriptor's whole buffer (D901): the access adds the
    /// offset itself, wherever in the buffer it lands. A scalar load's base at such an offset is
    /// still refused: its range starts at the offset.
    #[test]
    fn a_computed_descriptor_at_an_offset_the_host_cannot_know_binds_whole() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed, ComputedAccess};
        let mut table = Vec::new();
        for word in [0x2000u32, 16 << 16, 4, STRUCTURED] {
            table.extend(word.to_le_bytes());
        }
        let memory = At(0x1000, table);
        let computed = |through| {
            BufferSource::Computed(Box::new(Computed {
                // s_load_dwordx4 s[0:3], s[4:5], 0x0; s_waitcnt lgkmcnt(0)
                prefix: vec![0xf408_0002, 0xfa00_0000, 0xbf8c_c07f],
                at: 12,
                through,
                // s8, which nothing before the access sets.
                soffset: 8,
                first_register: 4,
                user_data: 2,
                program_address: None,
            }))
        };
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[0] = 0x1000;
        let descriptor = computed(ComputedAccess::Descriptor {
            first: 0,
            reads: vector(Some(16), true),
        });
        assert_eq!(
            super::resolve_range(&descriptor, (&user_data, 0, &[]), &memory),
            Ok((0x2000, 4 * 16)),
            "the descriptor's four sixteen-byte records, whole"
        );
        let pointer = computed(ComputedAccess::Pointer { base: 0, end: 16 });
        assert!(super::resolve_range(&pointer, (&user_data, 0, &[]), &memory).is_err());
    }

    /// A computed access a dispatch's waves were run to (D900): one no wave reaches binds nothing;
    /// one every wave reaches with the same descriptor binds that descriptor's range, from its base
    /// through the offset and the extent past it; one they reach with several is refused, one
    /// binding holding one buffer. Grand Theft Auto V's waterfall dispatch skips its access in
    /// every wave, all 79 leaving at the branch before it.
    #[test]
    fn a_reached_descriptor_binds_its_range() {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        let computed = Computed {
            prefix: Vec::new(),
            at: 0,
            through: ComputedAccess::Descriptor {
                first: 0,
                reads: vector(Some(16), true),
            },
            soffset: 8,
            first_register: 4,
            user_data: 2,
            program_address: None,
        };
        let mut reached = super::ReachedDescriptors::default();
        assert_eq!(super::reached_range(&computed, &reached), Ok((0, 0)));
        reached
            .descriptors
            .insert(([0x2000, 16 << 16, 4, STRUCTURED], 16));
        assert_eq!(
            super::reached_range(&computed, &reached),
            Ok((0x2000, 16 + 4 * 16))
        );
        reached
            .descriptors
            .insert(([0x3000, 16 << 16, 4, STRUCTURED], 0));
        assert!(super::reached_range(&computed, &reached).is_err());
    }

    /// A computed descriptor over a grid of far more waves than are run whole is found from a
    /// lattice of its groups, every wave of each (D951): in bounded time, and with the corners in
    /// the lattice, a descriptor whose base is an affine function of the group's ids spans every
    /// group's buffer. Here 64 x 64 groups each read the descriptor at `s0` plus sixteen times
    /// `x + 64 y`, each naming a 256-byte buffer of its own.
    #[test]
    fn a_computed_descriptor_too_large_to_run_whole_binds_every_group_from_a_sample() {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        use orbistoun_translate::wavefront::ComputeInputs;
        // The user data in s[0:1], the group's x and y ids in s2 and s3 (D883):
        // s_lshl_b32 s3, s3, 6; s_add_u32 s2, s2, s3; s_lshl_b32 s2, s2, 4; s_add_u32 s0, s0, s2;
        // s_load_dwordx4 s[4:7], s[0:1], 0x0; s_waitcnt lgkmcnt(0);
        // buffer_load_dword v1, v0, s[4:7], 0 offen; s_waitcnt vmcnt(0); s_endpgm.
        let program = [
            0x8f03_8603_u32,
            0x8002_0302,
            0x8f02_8402,
            0x8000_0200,
            0xf408_0100,
            0xfa00_0000,
            0xbf8c_c07f,
            0xe030_1000,
            0x8001_0100,
            0xbf8c_3f70,
            0xbf81_0000,
        ];
        let computed = Computed {
            prefix: program[..7].to_vec(),
            at: 28,
            through: ComputedAccess::Descriptor {
                first: 4,
                reads: vector(None, false),
            },
            soffset: 128,
            first_register: 0,
            user_data: 2,
            program_address: None,
        };
        let (code, table, buffers) = (0x100_usize, 0x1_0000_usize, 0x10_0000_u32);
        let mut bytes = vec![0_u8; 0x30_0000];
        for (at, word) in program.iter().enumerate() {
            bytes[code + 4 * at..code + 4 * at + 4].copy_from_slice(&word.to_le_bytes());
        }
        for group in 0..64 * 64_u32 {
            let words = [buffers + group * 0x100, 0, 0x100, RAW];
            for (at, word) in words.iter().enumerate() {
                let place = table + 16 * group as usize + 4 * at;
                bytes[place..place + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        let memory = At(0, bytes);
        let inputs = ComputeInputs {
            workgroup_ids: [true, true, false],
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        let started = std::time::Instant::now();
        let reached = super::dispatch_computed_descriptors(
            (&computed, code as u64, None),
            (&[table as u32, 0], inputs, [64, 64, 1], 64),
            &memory,
        )
        .expect("run over a sample");
        let took = started.elapsed();
        let groups = reached.sampled.expect("a sample, not every wave");
        assert!(
            groups <= super::MOST_COMPUTED_WAVES,
            "{groups} groups, a wave each"
        );
        assert!(
            took < std::time::Duration::from_secs(10),
            "{took:?} for the sample"
        );
        let found = super::sampled_reached_range(&computed, &reached, &memory).expect("bound");
        let read = (u64::from(buffers), 64 * 64 * 0x100);
        assert_eq!(
            found.read, read,
            "from group (0, 0)'s buffer to group (63, 63)'s end"
        );
        let margin = read.1 / 8;
        assert_eq!(found.range, (read.0 - margin, read.1 + 2 * margin));
        assert_eq!(found.sampled, Some(groups));
        // A scalar load's base pair is not placed so.
        let pointer = Computed {
            through: ComputedAccess::Pointer { base: 0, end: 16 },
            ..computed
        };
        assert!(super::sampled_reached_range(&pointer, &reached, &memory).is_err());
    }

    /// A dispatch's descriptor whose base is built in `vcc_lo` and `vcc_hi` as scalars - loaded,
    /// shifted, folded with `s_lshl4_add_u32` and `s_addk_i32`, added to a user-data word - is
    /// found by the group replay (D914), with no wave run at all: the shape of Grand Theft Auto V's
    /// dispatch at `0x400003e1f500`, which lost `vcc_hi` at its `s_addk_i32` and fell back to
    /// running every wave.
    #[test]
    fn a_group_s_descriptor_built_in_vcc_halves_is_found() {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        use orbistoun_translate::wavefront::ComputeInputs;
        // s_load_dword vcc_lo, s[4:5], 0x9c; s_waitcnt; s_lshl_b32 vcc_hi, vcc_lo, 5;
        // s_load_dword vcc_lo, s[4:5], 0x80; s_waitcnt; s_lshl4_add_u32 vcc_hi, vcc_lo, vcc_hi;
        // s_addk_i32 vcc_hi, 0x80; s_add_u32 s8, s3, vcc_hi; s_addc_u32 s9, s0, 0;
        // s_mov_b32 s10, 0x100; s_mov_b32 s11, RAW - the descriptor in s[8:11], read at 60.
        let computed = Computed {
            prefix: vec![
                0xf400_1a82,
                0xfa00_009c,
                0xbf8c_c07f,
                0x8f6b_856a,
                0xf400_1a82,
                0xfa00_0080,
                0xbf8c_c07f,
                0x98eb_6b6a,
                0xb7eb_0080,
                0x8008_6b03,
                0x8209_8000,
                0xbe8a_03ff,
                0x100,
                0xbe8b_03ff,
                RAW,
            ],
            at: 60,
            through: ComputedAccess::Descriptor {
                first: 8,
                reads: vector(None, false),
            },
            soffset: 128,
            first_register: 0,
            user_data: 7,
            program_address: None,
        };
        let mut table = vec![0_u8; 0x100];
        table[0x80..0x84].copy_from_slice(&0x10_u32.to_le_bytes());
        table[0x9c..0xa0].copy_from_slice(&2_u32.to_le_bytes());
        let inputs = ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        let reached = super::dispatch_group_descriptors(
            &computed,
            (&[0, 0, 1, 0x20, 0x8000, 0, 0], inputs, [300, 1, 1]),
            &At(0x8000, table),
        )
        .expect("found group by group");
        // The base: 0x20 + (0x10 << 4) + (2 << 5) + 0x80.
        assert_eq!(
            reached.descriptors.into_iter().collect::<Vec<_>>(),
            [([0x1e0, 0, 0x100, RAW], 0)]
        );
    }

    /// A preparation's host replay is bounded in steps across all its waves, not only in each
    /// (D951): waves that each loop a hundred times before the access finish within their own
    /// bound, and together run past a preparation's, which refuses the access by name rather than
    /// hold the run. A wave that loops without end is refused at its own bound.
    #[test]
    fn a_preparation_s_replay_stops_at_its_step_bound() {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        use orbistoun_translate::wavefront::ComputeInputs;
        // s_movk_i32 s9, 100; loop: s_sub_u32 s9, s9, 1; s_cmp_lg_u32 s9, 0;
        // s_cbranch_scc1 loop; buffer_load_dword v3, v0, s[4:7], 0 offen - the access, at 16;
        // s_waitcnt vmcnt(0); s_endpgm.
        let counted = [
            0xb009_0064_u32,
            0x8089_8109,
            0xbf07_8009,
            0xbf85_fffd,
            0xe030_1000,
            0x8001_0300,
            0xbf8c_3f70,
            0xbf81_0000,
        ];
        // loop: s_branch loop; then the same access, at 4.
        let endless = [0xbf82_ffff_u32, 0xe030_1000, 0x8001_0300, 0xbf81_0000];
        let computed = |program: &[u32], at: u32| Computed {
            prefix: program[..usize::try_from(at / 4).expect("small")].to_vec(),
            at,
            through: ComputedAccess::Descriptor {
                first: 4,
                reads: vector(None, false),
            },
            soffset: 128,
            first_register: 0,
            user_data: 8,
            program_address: None,
        };
        let memory = |program: &[u32]| {
            let mut bytes = vec![0_u8; 0x2_0000];
            for (at, word) in program.iter().enumerate() {
                bytes[0x100 + 4 * at..0x104 + 4 * at].copy_from_slice(&word.to_le_bytes());
            }
            At(0, bytes)
        };
        let inputs = ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        let user_data = [0, 0, 0, 0, 0x1_0000, 0, 0x100, RAW];
        let run = |program: &[u32], at: u32, steps: usize| {
            super::computed_descriptors_within(
                (&computed(program, at), 0x100, None),
                (&user_data, inputs, [200, 1, 1], 64),
                steps,
                &memory(program),
            )
        };
        // Some 300 steps a wave: 200 waves fit a preparation's steps, and not 10,000.
        let reached = run(&counted, 16, super::MOST_PREPARATION_STEPS).expect("within the bound");
        assert_eq!(reached.descriptors.len(), 1);
        let refused = run(&counted, 16, 10_000).expect_err("past the preparation's steps");
        assert!(
            refused.contains("ran past 10000 steps, the preparation's bound")
                && refused.contains("access at 0x10"),
            "{refused}"
        );
        let refused = run(&endless, 4, super::MOST_PREPARATION_STEPS).expect_err("endless");
        assert!(
            refused.contains("a wave's step bound") && refused.contains("in wave 1 of"),
            "{refused}"
        );
    }

    /// A program shaped as Grand Theft Auto V's tree walks are (D951), and its computed access:
    /// each thread loads its entry's kind, and only kind 7 loads a descriptor from a table and
    /// reads through it. The user data holds the kinds' descriptor in `s[0:3]` and the table's in
    /// `s[4:7]`, the group's id follows in `s8`. `v1 = 4 * (64 * s8 + tid)`;
    /// `buffer_load_dword v2, v1, s[0:3], 0 offen`; `v_cmpx_eq_u32 7, v2`; `s_cbranch_execz` to the
    /// end; `s_buffer_load_dwordx4 s[12:15], s[4:7], 0x0`; `buffer_load_dword v3, v0, s[12:15], 0
    /// offen` - the access, at 48.
    fn kind_walk() -> (Vec<u32>, orbistoun_translate::draw_buffers::Computed) {
        use orbistoun_translate::draw_buffers::{Computed, ComputedAccess};
        let encodings = orbistoun_shader::EncodingTable::builtin().expect("encodings");
        let short_form = |name: &str, dst: u32, src0: u32, vsrc1: u32| {
            let (family, opcode) = encodings.find_by_name(name).expect(name);
            let encoding = encodings
                .encodings()
                .iter()
                .find(|encoding| encoding.name == family)
                .expect("its family");
            encoding.value | (opcode << encoding.opcode.shift) | (dst << 17) | (vsrc1 << 9) | src0
        };
        let program = vec![
            short_form("v_mov_b32_e32", 1, 8, 0),
            short_form("v_lshlrev_b32_e32", 1, 134, 1),
            short_form("v_add_nc_u32_e32", 1, 256, 1),
            short_form("v_lshlrev_b32_e32", 1, 130, 1),
            0xe030_1000,
            0x8000_0201,
            0xbf8c_3f70,
            short_form("v_cmpx_eq_u32_e32", 0, 135, 2),
            0xbf88_0006,
            0xf428_0302,
            0xfa00_0000,
            0xbf8c_c07f,
            0xe030_1000,
            0x8003_0300,
            0xbf8c_3f70,
            0xbf81_0000,
        ];
        let computed = Computed {
            prefix: program[..12].to_vec(),
            at: 48,
            through: ComputedAccess::Descriptor {
                first: 12,
                reads: vector(None, false),
            },
            soffset: 128,
            first_register: 0,
            user_data: 8,
            program_address: None,
        };
        (program, computed)
    }

    /// An access only some threads take - Grand Theft Auto V's tree walks at `0x400003ce6000` and
    /// `0x74005b87e400` enter their waterfall only for an entry of one kind - can lie wholly
    /// between the groups a lattice samples (D951). A lattice that reaches nothing never binds an
    /// empty range: every wave is run, as without a sample, where the dispatch has no more than
    /// a preparation runs whole, and the access is refused past that. Here one thread of one group
    /// off the lattice holds the kind that reaches the access ([`kind_walk`]).
    #[test]
    fn a_sample_that_reaches_nothing_runs_every_wave_or_refuses() {
        use orbistoun_translate::wavefront::ComputeInputs;
        let (program, computed) = kind_walk();
        let inputs = ComputeInputs {
            workgroup_ids: [true, false, false],
            thread_id_components: 1,
            threads: [64, 1, 1],
            ..ComputeInputs::default()
        };
        let (code, table, kinds, target) = (0x100_usize, 0x8000_usize, 0x1_0000_usize, 0x40_0000);
        let prepared = |groups: u32, kind_at: Option<u32>| {
            let mut bytes = vec![0_u8; 0x50_0000];
            for (at, word) in program.iter().enumerate() {
                bytes[code + 4 * at..code + 4 * at + 4].copy_from_slice(&word.to_le_bytes());
            }
            for (at, word) in [target, 0, 0x100, RAW].iter().enumerate() {
                bytes[table + 4 * at..table + 4 * at + 4].copy_from_slice(&word.to_le_bytes());
            }
            if let Some(group) = kind_at {
                // Thread 5 of that group.
                let place = kinds + 4 * (64 * group as usize + 5);
                bytes[place..place + 4].copy_from_slice(&7_u32.to_le_bytes());
            }
            let user_data = [kinds as u32, 0, groups * 256, RAW, table as u32, 0, 16, RAW];
            (At(0, bytes), user_data)
        };
        let off_lattice = |groups: u32| {
            let sampled = super::sampled_groups([groups, 1, 1], 1, super::MOST_COMPUTED_WAVES);
            (0..groups)
                .find(|&group| !sampled.contains(&[group, 0, 0]))
                .expect("a group off the lattice")
        };
        // 452 groups of a wave each, as `0x400003ce6000`'s dispatch: the lattice misses the one
        // group, and every wave is run instead.
        let hidden = off_lattice(452);
        let (memory, user_data) = prepared(452, Some(hidden));
        let reached = super::dispatch_computed_descriptors(
            (&computed, code as u64, None),
            (&user_data, inputs, [452, 1, 1], 64),
            &memory,
        )
        .expect("every wave run");
        assert_eq!(reached.sampled, None, "the whole dispatch, not the lattice");
        assert_eq!(
            reached.descriptors.into_iter().collect::<Vec<_>>(),
            [([target, 0, 0x100, RAW], 0)],
            "the hidden group's descriptor"
        );
        // No thread of any group takes it: every wave run says so exactly, and nothing is read.
        let (memory, user_data) = prepared(452, None);
        let none = super::dispatch_computed_descriptors(
            (&computed, code as u64, None),
            (&user_data, inputs, [452, 1, 1], 64),
            &memory,
        )
        .expect("every wave run");
        assert_eq!(none.sampled, None);
        assert_eq!(super::reached_range(&computed, &none), Ok((0, 0)));
        // Past the waves a preparation runs whole, a lattice that reaches nothing is refused.
        let groups = u32::try_from(super::MOST_DISPATCH_WAVES).expect("small") + 500;
        let (memory, user_data) = prepared(groups, Some(off_lattice(groups)));
        let refused = super::dispatch_computed_descriptors(
            (&computed, code as u64, None),
            (&user_data, inputs, [groups, 1, 1], 64),
            &memory,
        )
        .expect_err("refused, not bound empty");
        assert!(
            refused.contains("reaches the access in no wave"),
            "{refused}"
        );
        // And an empty reach is never bound from a sample.
        let empty = super::ReachedDescriptors {
            sampled: Some(256),
            ..super::ReachedDescriptors::default()
        };
        assert!(super::sampled_reached_range(&computed, &empty, &memory).is_err());
    }

    /// Past the most answers kept, the oldest goes and every other stays, rather than all of
    /// them going at once.
    #[test]
    fn past_the_most_answers_kept_the_oldest_goes() {
        let computed = std::sync::Arc::new(orbistoun_translate::draw_buffers::Computed {
            prefix: Vec::new(),
            at: 0,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Pointer {
                base: 4,
                end: 16,
            },
            soffset: 125,
            first_register: 4,
            user_data: 2,
            program_address: None,
        });
        let answer = |word: u32| super::Evaluated {
            computed: std::sync::Arc::clone(&computed),
            user_data: vec![word],
            system: Vec::new(),
            reads: Vec::new(),
            answer: Ok(None),
        };
        let mut kept = super::EvaluatedAnswers::default();
        let most = u32::try_from(super::EVALUATED_KEPT).expect("fits");
        for word in 0..=most {
            kept.keep(u64::from(word % 7), answer(word));
        }
        let held: Vec<u32> = kept
            .answers
            .values()
            .flatten()
            .map(|answer| answer.user_data[0])
            .collect();
        assert_eq!(held.len(), super::EVALUATED_KEPT);
        assert!(!held.contains(&0), "the oldest went");
        assert!(held.contains(&1) && held.contains(&most), "the rest stayed");
    }

    /// The words a replay read are kept as runs: a descriptor's four are one, a word elsewhere
    /// another, and a word read again adds nothing.
    #[test]
    fn the_words_a_replay_read_are_kept_as_runs() {
        let runs = super::runs_of(&[
            (0x10, 1),
            (0x14, 2),
            (0x18, 3),
            (0x1c, 4),
            (0x40, 5),
            (0x14, 2),
        ]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].0, 0x10);
        assert_eq!(
            runs[0].1,
            [1u32, 2, 3, 4]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>()
        );
        assert_eq!(runs[1], (0x40, 5u32.to_le_bytes().to_vec()));
    }

    /// A computed descriptor asked for again is answered as kept only while the guest words its
    /// program read still hold what they did: the same program and user data over a table whose
    /// descriptor moved reads the new one.
    #[test]
    fn a_kept_computed_descriptor_holds_only_while_what_it_read_does() {
        use orbistoun_translate::draw_buffers::{BufferSource, Computed};
        let table_at = |base: u32| {
            let mut table = Vec::new();
            for word in [base, 16 << 16, 4, STRUCTURED] {
                table.extend(word.to_le_bytes());
            }
            At(0x5_1000, table)
        };
        let source = BufferSource::Computed(Box::new(Computed {
            // s_load_dwordx4 s[0:3], s[4:5], 0x0; s_waitcnt lgkmcnt(0)
            prefix: vec![0xf408_0002, 0xfa00_0000, 0xbf8c_c07f],
            at: 12,
            through: orbistoun_translate::draw_buffers::ComputedAccess::Descriptor {
                first: 0,
                reads: vector(Some(16), true),
            },
            soffset: 125,
            first_register: 4,
            user_data: 2,
            program_address: None,
        }));
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[0] = 0x5_1000;
        let range = |memory: &At| super::resolve_range(&source, (&user_data, 0, &[]), memory);
        assert_eq!(range(&table_at(0x6000)), Ok((0x6000, 4 * 16)));
        assert_eq!(
            range(&table_at(0x6000)),
            Ok((0x6000, 4 * 16)),
            "asked again"
        );
        assert_eq!(
            range(&table_at(0x7000)),
            Ok((0x7000, 4 * 16)),
            "the descriptor moved, so the kept answer does not hold"
        );
    }

    /// The open-toolchain GL context's vertex prologue and one four-component attribute fetch
    /// (oops-sdk `glsl_vs.c`, `vs_emit_ngg_preamble` and `vs_load_attributes`): the vertex index
    /// `s12` plus the lane, read from `base + stride * index` with the base and stride from the
    /// table at `s[10:11]`, as three components or four by the table's fourth word.
    const GL_FETCH: [u32; 35] = [
        0xbfa0_0001, // s_inst_prefetch 0x1
        0xbe8d_037e, // s_mov_b32 s13, exec_lo
        0xbefc_03ff,
        0x0000_1003, // s_mov_b32 m0, 0x1003
        0xbf80_0000, // s_nop 0
        0xbf90_0009, // s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefe_0381, // s_mov_b32 exec_lo, 1
        0x7e02_02ff,
        0x2028_0600, // v_mov_b32 v1, 0x20280600
        0xf800_0941,
        0x0000_0001, // exp prim v1, off, off, off done
        0xbf8c_ff0f, // s_waitcnt expcnt(0)
        0xbefe_0387, // s_mov_b32 exec_lo, 7
        0xd765_000e,
        0x0001_00c1, // v_mbcnt_lo_u32_b32 v14, -1, 0
        0x4a1c_1c0c, // v_add_nc_u32 v14, s12, v14
        0x7e02_0280, // v_mov_b32 v1, 0
        0xf408_0105,
        0xfa00_0000, // s_load_dwordx4 s[4:7], s[10:11], 0x0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xd569_0004,
        0x0002_1c06, // v_mul_lo_u32 v4, s6, v14
        0xd70f_6a02,
        0x0002_0804, // v_add_co_u32 v2, vcc_lo, s4, v4
        0x5006_0205, // v_add_co_ci_u32_e32 v3, vcc_lo, s5, v1, vcc_lo
        0xbf06_8407, // s_cmp_eq_u32 s7, 4
        0xbf85_0004, // s_cbranch_scc1 4
        0x7e2e_02f2, // v_mov_b32 v23, 1.0
        0xdc3c_8000,
        0x147d_0002, // global_load_dwordx3 v[20:22], v[2:3], off
        0xbf82_0002, // s_branch 2
        0xdc38_8000,
        0x147d_0002, // global_load_dwordx4 v[20:23], v[2:3], off
        0xbf8c_3f70, // s_waitcnt vmcnt(0)
        0xbf81_0000, // s_endpgm
    ];

    /// A primitive shader's global loads (D758) bind, for each draw, the range its vertex lanes'
    /// addresses span: here three vertices from the draw's base vertex, `stride` apart, each read
    /// sixteen bytes wide. A draw of more vertices than the wave holds is refused.
    #[test]
    fn a_global_load_binds_the_range_its_vertex_lanes_read() {
        let encodings = orbistoun_shader::EncodingTable::builtin().expect("encodings");
        let operands = orbistoun_shader::OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = GL_FETCH.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = orbistoun_shader::decode_program(&bytes, &encodings, &operands);
        let traced =
            orbistoun_translate::draw_buffers::trace(&decode, &encodings, (8, 5), (false, true));
        // The attribute table, then the global loads.
        let [
            _,
            orbistoun_translate::draw_buffers::BufferSource::Global(global),
        ] = traced.sources.as_slice()
        else {
            panic!("the table and the global loads: {:?}", traced.sources);
        };
        let mut global = global.clone();
        global.width = 32;
        let source = orbistoun_translate::draw_buffers::BufferSource::Global(global);

        let (table, vertices, stride) = (0x2000_0000u64, 0x74_3000_0000u64, 28u64);
        let mut words = Vec::new();
        for word in [vertices as u32, (vertices >> 32) as u32, stride as u32, 4] {
            words.extend(word.to_le_bytes());
        }
        let memory = At(table, words);
        let mut user_data = [0u32; super::USER_DATA_WORDS];
        user_data[2] = table as u32;
        user_data[4] = 3;
        assert_eq!(
            super::resolve_range(&source, (&user_data, 3, &[]), &memory),
            Ok((vertices + 3 * stride, 2 * stride + 16))
        );
        assert!(super::resolve_range(&source, (&user_data, 33, &[]), &memory).is_err());
    }

    /// An indexed draw of a primitive shader that reads its vertex ids from its index buffer binds
    /// that buffer's indices after the module's traced buffers - here it has none - and a module
    /// that does not read them binds nothing (D740).
    #[test]
    fn an_index_reading_draw_binds_its_index_buffer() {
        use crate::backend::{IndexBuffer, RenderCommand, ResourceId, ShaderStage};
        let (reader, other) = (ResourceId(3), ResourceId(4));
        let indices: Vec<u8> = [5u16, 9, 7].iter().flat_map(|i| i.to_le_bytes()).collect();
        let memory = At(0x2000, indices.clone());
        let bind = |module| {
            let mut commands = vec![
                RenderCommand::BindShader {
                    stage: ShaderStage::Vertex,
                    shader: module,
                },
                RenderCommand::DrawIndexed {
                    indices: 3,
                    instances: 1,
                    first_index: 0,
                    index_buffer: Some(IndexBuffer::Guest {
                        address: 0x2000,
                        bytes: 2,
                    }),
                },
            ];
            super::bind_draw_buffers(
                &mut commands,
                (
                    &std::collections::BTreeMap::new(),
                    (
                        &std::collections::BTreeSet::from([reader]),
                        &std::collections::BTreeMap::new(),
                    ),
                    &mut super::BufferCache::default(),
                ),
                &memory,
                &mut (0, None),
            );
            commands
                .into_iter()
                .filter_map(|command| match command {
                    RenderCommand::BindDrawBuffers { stage, buffers } => Some((stage, buffers)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let bound = bind(reader);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].0, ShaderStage::Vertex);
        // Six bytes, padded to whole words.
        assert_eq!(bound[0].1[0].bytes[..6], indices[..]);
        assert!(
            bind(other).is_empty(),
            "a module that does not read indices binds none"
        );
    }

    /// A chunk of a non-indexed draw binds its own run of vertex ids, counting up from its first,
    /// as thirty-two-bit indices (D741).
    #[test]
    fn a_counting_chunk_binds_its_run_of_vertex_ids() {
        use crate::backend::{IndexBuffer, RenderCommand, ResourceId, ShaderStage};
        let reader = ResourceId(3);
        let mut commands = vec![
            RenderCommand::BindShader {
                stage: ShaderStage::Vertex,
                shader: reader,
            },
            RenderCommand::DrawIndexed {
                indices: 3,
                instances: 1,
                first_index: 0,
                index_buffer: Some(IndexBuffer::Counting { first: 63 }),
            },
        ];
        super::bind_draw_buffers(
            &mut commands,
            (
                &std::collections::BTreeMap::new(),
                (
                    &std::collections::BTreeSet::from([reader]),
                    &std::collections::BTreeMap::new(),
                ),
                &mut super::BufferCache::default(),
            ),
            &At(0, Vec::new()),
            &mut (0, None),
        );
        let RenderCommand::BindDrawBuffers { buffers, .. } = &commands[1] else {
            panic!("no buffers bound: {commands:?}");
        };
        let expected: Vec<u8> = [63u32, 64, 65]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        assert_eq!(buffers[0].bytes[..], expected[..]);
    }

    /// Each clear draw binds the sixteen bytes its user-data word 2 names, once while they are the
    /// same bytes and again when a draw names others; a draw naming unmapped memory binds nothing
    /// and is counted.
    #[test]
    fn each_draw_binds_the_range_its_user_data_names() {
        use crate::backend::{RenderCommand, ResourceId, ShaderStage, USER_DATA_WORDS};
        use orbistoun_translate::draw_buffers::{BufferSource, DescriptorWord};

        const HIGH: u64 = 4 << 32;
        let module = ResourceId(7);
        let sources = std::collections::BTreeMap::from([(
            module,
            vec![BufferSource::Descriptor {
                words: [
                    DescriptorWord::UserData(2),
                    DescriptorWord::Constant(4),
                    DescriptorWord::Constant(16),
                    DescriptorWord::Constant(RAW),
                ],
                reads: scalar(16),
            }],
        )]);
        let colours: Vec<u8> = (0u8..32).collect();
        let memory = At(HIGH | 0x1000, colours.clone());
        let draw_at = |low: u32| {
            let mut words = [0u32; USER_DATA_WORDS];
            words[2] = low;
            [
                RenderCommand::SetUserData {
                    stage: ShaderStage::Fragment,
                    words,
                },
                RenderCommand::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                },
            ]
        };
        let mut commands = vec![RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: module,
        }];
        for low in [0x1000, 0x1000, 0x1010, 0x9000] {
            commands.extend(draw_at(low));
        }
        let mut refused = (0, None);
        super::bind_draw_buffers(
            &mut commands,
            (
                &sources,
                (
                    &std::collections::BTreeSet::new(),
                    &std::collections::BTreeMap::new(),
                ),
                &mut super::BufferCache::default(),
            ),
            &memory,
            &mut refused,
        );
        let bound: Vec<&[u8]> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::BindDrawBuffers { stage, buffers } => {
                    assert_eq!(*stage, ShaderStage::Fragment);
                    Some(&*buffers[0].bytes)
                }
                _ => None,
            })
            .collect();
        assert_eq!(bound, [&colours[..16], &colours[16..]]);
        assert_eq!(refused.0, 1, "the draw naming unmapped memory");
        assert!(refused.1.is_some());
    }
}
