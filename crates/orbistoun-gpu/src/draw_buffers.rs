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
        // Structured: the index below the record count, the offset unchecked.
        1 if records == 0 => 0,
        1 => last_record + reach.ok_or(UNBOUNDED)?,
        // Raw: the offset and the access's width within the record count in bytes, the index
        // unchecked.
        3 if reads.indexed && stride != 0 => return Err(UNBOUNDED),
        3 => reach.map_or(records, |reach| reach.min(records)),
        // Only a record count of zero is out of bounds.
        _ if records == 0 => 0,
        _ if reads.indexed && stride != 0 => return Err(UNBOUNDED),
        _ => reach.ok_or(UNBOUNDED)?,
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
const MOST_BYTES: u64 = 256 << 20;

/// How many bytes [`BufferCache`] keeps, the oldest ranges going first past it: a world's vertex
/// buffers, which a frame draws from again, and a bound for a guest streaming new ones every frame.
const CACHE_BYTES: usize = 512 << 20;

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
pub(crate) type Evaluation = Result<Option<([u32; 4], u32)>, Refusal>;

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
        125 => 0,
        code @ 128..=192 => u32::from(code - 128),
        code @ 193..=208 => 0u32.wrapping_sub(u32::from(code - 192)),
        code => scalars
            .get(usize::from(code))
            .copied()
            .flatten()
            .ok_or_else(|| unknown(usize::from(code)))?,
    };
    Ok(Some((words, soffset)))
}

/// [`decode_prefix`], kept by the prefix's words: a draw resolves its descriptors from the same
/// prefix every time, and PPSA25872's draws each decoded it again, 0.1 ms of every draw.
fn decoded_prefix(
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
/// a bound on the work a dispatch can ask of the host, refused by name beyond it.
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

/// The range a dispatch's global loads read (D883): the program run on the host over every wave of
/// every group the dispatch runs, each from its entry state - the user data, the group's enabled id
/// components, and each lane's thread id, x fastest, where the lane has a thread - and the span of
/// every address an active lane forms. A partial dispatch's last group runs only its threads (D855).
///
/// # Errors
///
/// A dispatch of more waves than [`MOST_DISPATCH_WAVES`], or a load whose lanes' addresses or mask
/// the host cannot know.
pub(crate) fn dispatch_global_range(
    global: &GlobalLoads,
    (user_data, inputs, groups, width): (
        &[u32],
        orbistoun_translate::wavefront::ComputeInputs,
        [u32; 3],
        u32,
    ),
    memory: &impl GuestMemory,
) -> Result<(u64, u64), Refusal> {
    use orbistoun_translate::evaluate::{
        LOOPS_PAST_THE_BOUND, WaveEntry, long_wave_lane_addresses, wave_lane_addresses,
    };
    let waves = inputs.waves(width);
    let runs = groups.iter().map(|&n| u64::from(n)).product::<u64>() * u64::from(waves);
    if runs > MOST_DISPATCH_WAVES {
        return Err(concat!(
            "a dispatch of more waves than one preparation runs on the host to find where its ",
            "global loads read (D883)"
        )
        .into());
    }
    let (encodings, _) = tables()?;
    let prepared = prepared_program(global, crate::content_hash(&global.prefix))?;
    let mut word = |address: u64| {
        let bytes = memory.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let [x_threads, y_threads, z_threads] = inputs.threads.map(|n| n.max(1));
    // The span every wave reads, folded wave by wave: a long loop's addresses are many.
    let mut span: Option<(u64, u64)> = None;
    for z in 0..groups[2] {
        for y in 0..groups[1] {
            for x in 0..groups[0] {
                let group = [x, y, z];
                let group_ids: Vec<u32> = (0..3)
                    .filter(|&component| inputs.workgroup_ids[component])
                    .map(|component| group[component])
                    .collect();
                for wave in 0..waves {
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
                            // The last group of a partial dispatch runs only the threads it has.
                            let runs = inputs.partial.is_none_or(|partial| {
                                (0..3).all(|c| {
                                    group[c] != partial.last[c] || id[c] < partial.threads[c]
                                })
                            });
                            // The x id counts from the dispatch's first thread (D861).
                            runs.then_some([id[0] + inputs.first_thread, id[1], id[2]])
                        })
                        .collect();
                    let entry = WaveEntry {
                        group_ids: group_ids.clone(),
                        threads,
                        components: inputs.thread_id_components,
                    };
                    let addresses = match wave_lane_addresses(
                        &prepared.program,
                        encodings,
                        user_data,
                        (width, &entry),
                        &mut word,
                    ) {
                        Ok(addresses) => addresses,
                        // A loop longer than the model follows is followed natively (D850).
                        Err(why) if why == LOOPS_PAST_THE_BOUND => long_wave_lane_addresses(
                            &prepared.program,
                            user_data,
                            (width, &entry),
                            &mut word,
                        )
                        .map_err(|at| -> Refusal {
                            let step = prepared.program.instruction_named(at);
                            if prepared.program.runs_quickly() {
                                format!("{why}; the native run stops at {step}").into()
                            } else {
                                format!("{why}; the native run does not carry out {step}").into()
                            }
                        })?,
                        Err(why) => {
                            return Err(
                                format!("{why}; {}", wave_entry(user_data.len(), inputs)).into()
                            );
                        }
                    };
                    let (first, bytes) = span_of(global, addresses);
                    if bytes > 0 {
                        let end = first + bytes;
                        span =
                            Some(span.map_or((first, end), |(low, high)| {
                                (low.min(first), high.max(end))
                            }));
                    }
                }
            }
        }
    }
    Ok(span.map_or((0, 0), |(low, high)| (low, high - low)))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            match computed.through {
                ComputedAccess::Descriptor { reads, .. } => (
                    crate::pipeline::guest_address_of(decode_buffer_descriptor(values).base),
                    u64::from(soffset) + descriptor_extent_to(values, reads, last_index(reads))?,
                ),
                // A scalar load reads its words from the base plus the scalar offset, which the
                // translated load does not add: the range starts there.
                ComputedAccess::Pointer { end, .. } => {
                    let base = u64::from(values[0]) | u64::from(values[1] & 0xffff) << 32;
                    (base.wrapping_add(u64::from(soffset)), u64::from(end))
                }
            }
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
    // The module reads whole words counted from the range's first byte, as the hardware does from
    // a word-aligned base.
    if address % 4 != 0 {
        return Err("a draw buffer whose base is not word-aligned (D733)".into());
    }
    if length > MOST_BYTES {
        return Err("a draw buffer larger than one draw binds (D733)".into());
    }
    Ok((address, length))
}

/// The vertex ids a chunk of a non-indexed draw reads in place of indices (D741): `count`
/// thirty-two-bit ids counting up from its first; empty for anything else.
fn counting_indices(indices: Option<Result<(IndexBuffer, u32), &'static str>>) -> DrawBuffer {
    let ids: Vec<u32> = match indices {
        Some(Ok((IndexBuffer::Counting { first }, count))) => {
            (0..count).map(|id| first.wrapping_add(id)).collect()
        }
        _ => Vec::new(),
    };
    let bytes: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
    let mut hasher = crate::ContentHasher::new(ids.len());
    hasher.bytes(&bytes);
    DrawBuffer {
        hash: hasher.finish(),
        bytes: bytes.into(),
        base: 0,
    }
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
                            resolve_draw_range(source, (&words[at], vertices, &system), ids, memory)
                                .map(Some)
                        })
                        .chain(indices.map(|found| {
                            found
                                .map_err(Into::into)
                                .map(|(buffer, count)| match buffer {
                                    IndexBuffer::Guest { address, bytes } => {
                                        Some((address, u64::from(count) * u64::from(bytes)))
                                    }
                                    IndexBuffer::Counting { .. } => None,
                                })
                        }));
                    let resolved: Result<Vec<DrawBuffer>, Refusal> = ranges
                        .map(|range| {
                            let Some(range) = range? else {
                                return Ok(counting_indices(indices));
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
        // An offset in a register is bounded by the stride only where the mode checks it.
        assert_eq!(
            descriptor_extent(words(STRUCTURED_WITH_OFFSET), vector(None, true)),
            Ok(9 * 24 + 23 + 16)
        );
        assert!(descriptor_extent(words(STRUCTURED), vector(None, true)).is_err());
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
    /// preparation runs is refused by name.
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
        assert_eq!(range, Ok((0x7_0000_1000, 0x1a0)));
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
            super::dispatch_global_range(&global, (&[0x1000, 7], later, [2, 1, 1], 64), &memory),
            Ok((0x7_0000_1000 + 4000, 0x1a0))
        );
        assert!(
            super::dispatch_global_range(
                &global,
                (&[0x1000, 7], inputs, [5000, 1, 1], 64),
                &memory
            )
            .is_err()
        );
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
        assert_eq!(range, Ok((0x7_0000, 400)));
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
