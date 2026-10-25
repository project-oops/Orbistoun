//! Runs one wave of a captured guest dispatch on the host interpreter, and compares what it stores
//! with what the device stores running the same program over the same inputs: `trace-dispatch`.
//!
//! The host interpreter and the translator are two implementations of the instruction set. Run
//! from the same registers and memory, a word they store differently names the store; cutting the
//! program short at successive points on both (`--stop-at`) finds the first point where the device
//! has done something the host has not, and the trace's registers there name the instruction.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use orbistoun_gpu::translations::{Context, TranslationStore};
use orbistoun_gpu_vulkan::capture::ImageRole;
use orbistoun_gpu_vulkan::capture::{Capture, CapturedBuffer};
use orbistoun_gpu_vulkan::compute_images::DispatchImages;
use orbistoun_shader::{Decode, EncodingTable, OperandTable, decode_program};
use orbistoun_translate::draw_buffers::{BufferSource, Computed};
use orbistoun_translate::evaluate::{
    LaneProgram, TraceImages, TraceStop, WaveEntry, WaveTrace, wave_trace_with_images,
};

/// The registers a trace records: every vector register a wave32 program addresses below 128, and
/// every scalar code below `vcc_lo`'s.
const TRACED_VECTORS: u32 = 128;
const TRACED_SCALARS: u32 = 106;

/// `s_endpgm`, and `s_nop 0`, as words.
const S_ENDPGM: u32 = 0xbf81_0000;
const S_NOP: u32 = 0xbf80_0000;

/// What `trace-dispatch` is asked.
pub(crate) struct TraceRequest<'a> {
    pub dir: &'a Path,
    pub program: Option<&'a Path>,
    pub kept: Option<&'a str>,
    pub user_words: Option<usize>,
    /// The groups whose first wave runs on the host, each `[x, y]`; the first is the one `--points`
    /// shows.
    pub groups: Vec<[u32; 2]>,
    pub against: Option<&'a Path>,
    /// Where `--device` writes what the device left, named as `replay-dispatch --out` names it.
    pub out: Option<&'a Path>,
    pub points: bool,
    pub lane: u32,
    pub threads: Option<u32>,
    pub device: bool,
    pub stop_at: Option<u32>,
    /// Instructions to replace by a branch, each `(from, to)` by byte: to run an existing store as
    /// a probe of the registers it reads.
    pub jumps: Vec<(u32, u32)>,
    /// Words to write over the program, each at a byte: an instruction replaced by others of its
    /// length, its control flow left as it was.
    pub patches: Vec<(u32, Vec<u32>)>,
}

impl Shape {
    /// The workgroup ids the hardware loads after the user data for group `[x, y]`: one for each
    /// component the context enables, the first by default.
    fn group_ids(&self, group: [u32; 2]) -> Vec<u32> {
        let enabled = self
            .context
            .and_then(|context| context.user_data.compute)
            .map_or([true, false, false], |inputs| inputs.workgroup_ids);
        enabled
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .map(|(axis, _)| group.get(axis).copied().unwrap_or(0))
            .collect()
    }
}

/// The program a trace runs and how its dispatch is shaped.
struct Shape {
    /// Its bytes, end-of-program included, with any `--jump` and `--stop-at` patches applied.
    bytes: Vec<u8>,
    /// Its bytes as they were.
    original: Vec<u8>,
    /// The context its dispatch was translated under, where the title's store kept it.
    context: Option<Context>,
    /// How many user-data words land in `s0` upward.
    user_words: usize,
    /// The group's threads.
    threads: [u32; 3],
    /// The wave's width.
    width: u32,
}

/// Traces the first wave of group `group` of the dispatch captured in `dir` on the host, prints
/// where the run stopped and what it stored, and compares its stores with the device's: those of a
/// `replay-dispatch --out` directory (`against`), or of a run made now (`device`), which needs the
/// translation context the title's store kept for the captured module (`kept`).
pub(crate) fn cmd_trace_dispatch(request: &TraceRequest<'_>) -> Result<()> {
    let capture = Capture::read_from(request.dir)
        .with_context(|| format!("reading the capture in {}", request.dir.display()))?;
    let encodings = EncodingTable::builtin()?;
    let operands = OperandTable::builtin()?;
    let shape = shape(request, &capture, (&encodings, &operands))?;
    let decode = decode_program(&shape.bytes, &encodings, &operands);
    let [x, y, z] = shape.threads;
    let threads = x * y * z;
    let width = shape.width.max(1);
    let user_data = capture
        .push
        .get(..shape.user_words)
        .context("fewer push words than the user data")?;
    let regions = memory_regions(&capture);
    let mut read = |address: u64| read_word(&regions, address);
    let mut images = CaptureImages(&capture);
    let program = LaneProgram::new(decode.clone(), &encodings, &[]).clamping(
        shape
            .context
            .and_then(|context| context.user_data.dx10_clamp),
    );
    // Each group's run, its stores gathered into the first's for the comparison: groups store
    // apart, so a later group's store never meets an earlier one's.
    let mut run: Option<WaveTrace> = None;
    for &group in &request.groups {
        // The group's waves, each its threads' ids.
        let waves: Vec<WaveEntry> = (0..threads.div_ceil(width))
            .map(|wave| WaveEntry {
                group_ids: shape.group_ids(group),
                threads: (wave * width..((wave + 1) * width).min(threads))
                    .map(|t| Some([t % x, t / x % y, t / (x * y)]))
                    .collect(),
                components: if y * z == 1 { 1 } else { 3 },
            })
            .collect();
        let traced = wave_trace_with_images(
            &program,
            &encodings,
            user_data,
            (width, &waves),
            (TRACED_VECTORS, TRACED_SCALARS, request.lane),
            (&mut read, &mut images),
        );
        print!(
            "group {group:?}: {}",
            summary(&traced, threads, waves.len())
        );
        match &mut run {
            None => {
                if request.points {
                    print!("{}", points(&traced, &listing(&decode, &encodings)));
                }
                run = Some(traced);
            }
            Some(first) => {
                first.written.extend(traced.written);
                first.texels.extend(traced.texels);
            }
        }
    }
    let run = run.context("name a group to trace")?;
    if request.device {
        let Some(context) = shape.context else {
            bail!("--device needs the translation context: name the title with --kept");
        };
        let original = decode_program(&shape.original, &encodings, &operands);
        let files = run_on_device(&capture, (&original, &decode), &encodings, &context)?;
        if let Some(out) = request.out {
            std::fs::create_dir_all(out)?;
            for (name, words) in &files {
                let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
                std::fs::write(out.join(name), bytes)?;
            }
        }
        let device = |name: &str| files.get(name).cloned();
        print!("{}", compare(&capture, &run, &device));
        print!("{}", compare_texels(&capture, &run, &device));
    } else if let Some(against) = request.against {
        let read_file = |name: &str| -> Option<Vec<u32>> {
            let bytes = std::fs::read(against.join(name)).ok()?;
            Some(
                bytes
                    .chunks_exact(4)
                    .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                    .collect(),
            )
        };
        print!("{}", compare(&capture, &run, &read_file));
        print!("{}", compare_texels(&capture, &run, &read_file));
    }
    Ok(())
}

/// The program, its context and its dispatch's shape: from the title's store where `--kept` names
/// the title, the kept translation whose module is the captured one; else from `--program`, the
/// module's `LocalSize` and `--user-words`.
fn shape(
    request: &TraceRequest<'_>,
    capture: &Capture<'_>,
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> Result<Shape> {
    let (bytes, context) = match (request.kept, request.program) {
        (Some(title), _) => {
            let file = orbistoun_paths::Paths::resolve().title_translations_file(title);
            let store = TranslationStore::load(&file);
            let kept = store
                .entries()
                .find(|kept| kept.module[..] == capture.module[..])
                .with_context(|| {
                    format!(
                        "no translation kept in {} is the captured module",
                        file.display()
                    )
                })?;
            (kept.bytes.clone(), Some(kept.context))
        }
        (None, Some(path)) => (
            std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
            None,
        ),
        // A capture that noted its source holds the program and its context itself.
        (None, None) => match &capture.source {
            Some(source) => (source.program.clone(), Some(source.context)),
            None => bail!(
                "the capture notes no source: name the program, --kept <title> or --program <file>"
            ),
        },
    };
    let compute = context.and_then(|context| context.user_data.compute);
    let threads = match (request.threads, compute) {
        (Some(threads), _) => [threads, 1, 1],
        (None, Some(inputs)) => inputs.threads,
        (None, None) => local_size(&capture.module).context("the module declares no LocalSize")?,
    };
    let width = match context.map(|context| context.strategy) {
        Some(orbistoun_translate::Strategy::Predicated { width, .. }) => width.lanes(),
        _ => 32,
    };
    let user_words = match (request.user_words, context) {
        (Some(words), _) => words,
        (None, Some(context)) => context.user_data.count as usize,
        (None, None) => bail!("say how many push words are user data: --user-words"),
    };
    let original = bytes.clone();
    let mut bytes = bytes;
    for (at, words) in &request.patches {
        let start = *at as usize;
        let end = start + 4 * words.len();
        let Some(place) = bytes.get_mut(start..end) else {
            bail!("a patch at {at:#x} runs past the program");
        };
        for (slot, word) in place.chunks_exact_mut(4).zip(words) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
    }
    for &(from, to) in &request.jumps {
        bytes = branched(&bytes, (from, to), (encodings, operands))?;
    }
    if let Some(at) = request.stop_at {
        let end = decode_program(&bytes, encodings, operands)
            .instructions
            .iter()
            .rev()
            .find(|instruction| instruction.word == S_ENDPGM)
            .map(|instruction| instruction.offset)
            .context("the program has no s_endpgm")?;
        bytes = branched(&bytes, (at, end), (encodings, operands))?;
    }
    Ok(Shape {
        bytes,
        original,
        context,
        user_words,
        threads,
        width,
    })
}

/// `bytes` with the instruction at byte `at` replaced by a branch to byte `to`, padded with `s_nop`
/// to the instruction's length: every other instruction is still there, so the translation traces
/// the same buffers and binds them in the same slots. A branch to the last `s_endpgm` stops the
/// program there.
fn branched(
    bytes: &[u8],
    (at, to): (u32, u32),
    (encodings, operands): (&EncodingTable, &OperandTable),
) -> Result<Vec<u8>> {
    let decode = decode_program(bytes, encodings, operands);
    let position = decode
        .instructions
        .iter()
        .position(|instruction| instruction.offset == at)
        .with_context(|| format!("no instruction starts at {at:#x}"))?;
    if !decode
        .instructions
        .iter()
        .any(|instruction| instruction.offset == to)
    {
        bail!("no instruction starts at {to:#x}");
    }
    let length = decode
        .instructions
        .get(position + 1)
        .map_or(4, |next| next.offset - at);
    // `s_branch` counts words from the instruction after it.
    let words = (i64::from(to) - i64::from(at) - 4) / 4;
    let branch = 0xbf82_0000 | (u32::from(i16::try_from(words)? as u16));
    let mut patched = bytes.to_vec();
    let start = at as usize;
    patched[start..start + 4].copy_from_slice(&branch.to_le_bytes());
    for pad in (start + 4..start + length as usize).step_by(4) {
        patched[pad..pad + 4].copy_from_slice(&S_NOP.to_le_bytes());
    }
    Ok(patched)
}

/// Refused where the patched `decode` would bind its buffers otherwise than the `original`
/// program the capture holds: other sources, or an access served from another slot.
fn same_bindings(
    (original, decode): (&Decode, &Decode),
    encodings: &EncodingTable,
    context: &Context,
) -> Result<()> {
    // Each source as it binds: a computed one is found by running the program's words before its
    // access, which a patch that leaves the scalar registers alone resolves to the same buffer, so
    // those words are set aside.
    let traced = |decode: &Decode| {
        orbistoun_translate::wavefront::draw_buffers_for(
            decode,
            encodings,
            context.stage,
            context.user_data,
        )
        .map(|buffers| {
            let sources: Vec<String> = buffers
                .sources
                .iter()
                .map(|source| match source {
                    BufferSource::Computed(computed) => format!(
                        "{:?}",
                        Computed {
                            prefix: Vec::new(),
                            ..(**computed).clone()
                        }
                    ),
                    other => format!("{other:?}"),
                })
                .collect();
            (sources.join("; "), buffers.served)
        })
        .map_err(|e| anyhow::anyhow!("tracing the buffers: {e}"))
    };
    let (sources, served) = traced(original)?;
    let (patched_sources, patched_served) = traced(decode)?;
    if sources != patched_sources {
        bail!(
            "the patched program traces other buffers than the captured one:\n  captured {sources}\n  patched  {patched_sources}"
        );
    }
    let moved: Vec<_> = served
        .iter()
        .filter(|(at, slot)| patched_served.get(at).is_some_and(|other| other != *slot))
        .map(|(at, _)| format!("{at:#x}"))
        .collect();
    if !moved.is_empty() {
        bail!(
            "the patched program serves the accesses at {} from other slots",
            moved.join(", ")
        );
    }
    Ok(())
}

/// Translates `decode` under `context` and runs it on the device over the capture's inputs, its
/// buffers bound as the capture binds them; what it leaves, named as `replay-dispatch --out` names
/// it. Refused where the binding would not be the capture's: the `original` program translated
/// again must be the captured module, and a patched `decode` must trace its buffers into the same
/// slots.
fn run_on_device(
    capture: &Capture<'_>,
    (original, decode): (&Decode, &Decode),
    encodings: &EncodingTable,
    context: &Context,
) -> Result<BTreeMap<String, Vec<u32>>> {
    let again = orbistoun_translate::translate_with_user_data(
        original,
        encodings,
        context.strategy,
        (context.stage, context.primitive),
        context.window,
        context.user_data,
    )
    .map_err(|e| anyhow::anyhow!("the program does not translate under its context: {e}"))?;
    // A build that translates the program otherwise - a fix under test - still binds the buffers
    // the tracing finds, which the capture's slots hold.
    if again.module[..] != capture.module[..] {
        println!("device: this build translates the program otherwise than the captured module");
    }
    same_bindings((original, decode), encodings, context)?;
    let translated = orbistoun_translate::translate_with_user_data(
        decode,
        encodings,
        context.strategy,
        (context.stage, context.primitive),
        context.window,
        context.user_data,
    )
    .map_err(|e| anyhow::anyhow!("the program does not translate under its context: {e}"))?;
    if !std::ptr::eq(original, decode) && translated.module[..] == capture.module[..] {
        println!("device: the patched program translates to the captured module, so runs as it");
    }
    // A device-addressed module reaches guest memory where it lay: the capture's guest pages are
    // placed there again, and what it left in them is compared as `guest-<n>.bin` (D980).
    if orbistoun_gpu_vulkan::direct::addresses_directly(&translated.module) {
        let (done, guest_after) =
            orbistoun_gpu_vulkan::replay::run_once(capture, &translated.module)?;
        if done.escaped || done.loop_cut.is_some() {
            println!(
                "device: escaped {}, loop cut {:?}",
                done.escaped, done.loop_cut
            );
        }
        let mut files: BTreeMap<String, Vec<u32>> =
            orbistoun_gpu_vulkan::replay::outputs(capture, &done)
                .into_iter()
                .collect();
        files.extend(
            guest_after
                .into_iter()
                .enumerate()
                .map(|(index, words)| (format!("guest-{index}.bin"), words)),
        );
        return Ok(files);
    }
    let (fetched, more_fetched, stored) = capture.dispatch_images();
    // A capture holds no images selected at run time (D983).
    let images = DispatchImages {
        fetched,
        more_fetched: &more_fetched,
        stored: &stored,
        selected: None,
    };
    let done = orbistoun_gpu_vulkan::dispatch_guest_with_buffers(
        &translated.module,
        &capture.memory,
        (&capture.push, capture.groups),
        &images,
        &capture.buffers_in(),
    )?;
    if done.escaped || done.loop_cut.is_some() {
        println!(
            "device: escaped {}, loop cut {:?}",
            done.escaped, done.loop_cut
        );
    }
    Ok(orbistoun_gpu_vulkan::replay::outputs(capture, &done)
        .into_iter()
        .collect())
}

/// Each instruction's text by its byte, as the trace lists it.
fn listing(decode: &Decode, encodings: &EncodingTable) -> BTreeMap<u32, String> {
    decode
        .instructions
        .iter()
        .map(|instruction| {
            let name = instruction
                .encoding
                .and_then(|index| encodings.encodings().get(usize::from(index)))
                .and_then(|family| encodings.mnemonic_for(&family.name, instruction.opcode))
                .unwrap_or("?");
            let operands: Vec<String> = instruction
                .operands
                .iter()
                .map(ToString::to_string)
                .collect();
            (
                instruction.offset,
                format!("{name} {}", operands.join(", ")),
            )
        })
        .collect()
}

/// The `LocalSize` a SPIR-V module's `OpExecutionMode` declares.
fn local_size(module: &[u32]) -> Option<[u32; 3]> {
    const OP_EXECUTION_MODE: u32 = 16;
    const LOCAL_SIZE: u32 = 17;
    let mut at = 5;
    while let Some(&head) = module.get(at) {
        let count = (head >> 16) as usize;
        if count == 0 {
            return None;
        }
        if head & 0xffff == OP_EXECUTION_MODE && module.get(at + 2) == Some(&LOCAL_SIZE) {
            let size = module.get(at + 3..at + 6)?;
            return Some([size[0], size[1], size[2]]);
        }
        at += count;
    }
    None
}

/// A capture's images as a host trace reads them: the image a descriptor names, by its texels'
/// first level.
struct CaptureImages<'a>(&'a Capture<'a>);

impl TraceImages for CaptureImages<'_> {
    fn load(&mut self, descriptor: [u32; 8], texel: [u32; 3], level: u32) -> Option<[u32; 4]> {
        (level == 0).then(|| self.0.texel_read(descriptor, texel))?
    }
    fn sample(
        &mut self,
        descriptor: [u32; 8],
        sampler: [u32; 4],
        coordinates: [f32; 3],
        level: f32,
    ) -> Option<[u32; 4]> {
        self.0.texel_sample(descriptor, sampler, coordinates, level)
    }
}

/// Every texel the host's image stores wrote, against what the device left in the stored image of
/// the same place (`stored-<n>.bin`): how many agree, differ by one step of the format, differ
/// more, or are not known on the host, with the first that differ more listed.
fn compare_texels(
    capture: &Capture<'_>,
    run: &WaveTrace,
    device: &dyn Fn(&str) -> Option<Vec<u32>>,
) -> String {
    let mut text = String::new();
    let (mut agreed, mut near, mut unknown, mut unplaced, mut differ) = (0, 0, 0, 0, 0);
    for ((descriptor, [x, y, layer], level), (components, at)) in &run.texels {
        let Some(index) = capture.image_named(*descriptor) else {
            unplaced += 1;
            continue;
        };
        let image = &capture.images[index];
        // The stored images, in order, are the device's `stored-<n>.bin`.
        let stored = capture.images[..index]
            .iter()
            .filter(|image| image.role == ImageRole::Stored)
            .count();
        let Some(after) = (image.role == ImageRole::Stored && *level == 0)
            .then(|| device(&format!("stored-{stored}.bin")))
            .flatten()
        else {
            unplaced += 1;
            continue;
        };
        let place = ((*layer as usize * image.height as usize + *y as usize)
            * image.width as usize)
            + *x as usize;
        let (Some(&before), Some(&left)) = (image.texels.get(place), after.get(place)) else {
            unplaced += 1;
            continue;
        };
        let Some(host) = image.stored_word(components, before) else {
            unknown += 1;
            continue;
        };
        if host == left {
            agreed += 1;
            continue;
        }
        let step = host
            .to_le_bytes()
            .iter()
            .zip(left.to_le_bytes())
            .all(|(a, b)| a.abs_diff(b) <= 1);
        if step {
            near += 1;
            continue;
        }
        differ += 1;
        if differ <= 64 {
            let _ = writeln!(
                text,
                "image {index} texel ({x}, {y}, {layer}): host {host:#010x} device {left:#010x}  host store at {at:#x}"
            );
        }
    }
    let _ = writeln!(
        text,
        concat!(
            "texels the host stored: {agreed} alike, {near} a step apart, {differ} differ; ",
            "{unknown} with a value the host does not know, {unplaced} in no stored image found"
        ),
        agreed = agreed,
        near = near,
        differ = differ,
        unknown = unknown,
        unplaced = unplaced,
    );
    text
}

/// The guest memory a capture holds, by start address: the guest ranges a device-addressed dispatch
/// reached (D980), its window and each buffer with words of its own. A buffer the program only
/// stores to holds nothing the run can read.
fn memory_regions<'a>(capture: &'a Capture<'_>) -> Vec<(u64, &'a [u32])> {
    let mut regions: Vec<(u64, &[u32])> = capture
        .guest
        .iter()
        .map(|range| (range.at, &range.words[..]))
        .collect();
    if let Some((start, _)) = capture.ranges.window {
        regions.push((start, &capture.memory[..]));
    }
    for (buffer, (start, _)) in capture.buffers.iter().zip(&capture.ranges.buffers) {
        if let CapturedBuffer::Words(words) = buffer {
            regions.push((*start, &words[..]));
        }
    }
    regions
}

/// The word at byte `address` in `regions`, where one holds it.
fn read_word(regions: &[(u64, &[u32])], address: u64) -> Option<u32> {
    regions.iter().find_map(|(start, words)| {
        let offset = address.checked_sub(*start)?;
        (offset % 4 == 0)
            .then(|| words.get(usize::try_from(offset / 4).ok()?).copied())
            .flatten()
    })
}

/// Where the run stopped, how far it modelled the program, and how much it stored.
fn summary(run: &WaveTrace, threads: u32, waves: usize) -> String {
    let mut text = String::new();
    let stop = match run.stop {
        TraceStop::Ended => "the program ended".to_owned(),
        TraceStop::ForkedAt(at) => format!("a branch on a mask the host cannot know at {at:#x}"),
        TraceStop::StepBound => "the step bound".to_owned(),
        TraceStop::Unnamed(at) => format!("an instruction with no name at {at:#x}"),
    };
    let _ = writeln!(
        text,
        concat!(
            "group of {threads} threads in {waves} waves; {} instructions run by the traced ",
            "lane's wave; stopped at {stop}",
        ),
        run.points.len(),
        threads = threads,
        waves = waves,
        stop = stop,
    );
    match run.points.iter().find(|point| point.past_unmodelled) {
        Some(point) => {
            let _ = writeln!(text, "first instruction not modelled: {:#x}", point.offset);
        }
        None => {
            let _ = writeln!(text, "every instruction run was modelled");
        }
    }
    let unknown = run
        .written
        .values()
        .filter(|(value, _)| value.is_none())
        .count();
    let _ = writeln!(
        text,
        "{} words stored ({unknown} with a value the host does not know)",
        run.written.len()
    );
    text
}

/// Each instruction run, as `listing` writes it, with the registers it changed: the scalars, and
/// the traced lane's vector registers.
fn points(run: &WaveTrace, listing: &BTreeMap<u32, String>) -> String {
    let mut text = String::new();
    let mut vectors: Vec<Option<u32>> = vec![None; TRACED_VECTORS as usize];
    let mut scalars: Vec<Option<u32>> = vec![None; TRACED_SCALARS as usize];
    let show = |value: Option<u32>| value.map_or("?".to_owned(), |value| format!("{value:#x}"));
    for point in &run.points {
        let instruction = listing.get(&point.offset).map_or("?", String::as_str);
        let _ = write!(text, "{:#06x} {instruction:<44} |", point.offset);
        for (register, (old, new)) in vectors.iter_mut().zip(&point.vectors).enumerate() {
            if old != new {
                let _ = write!(text, " v{register}={}", show(*new));
                *old = *new;
            }
        }
        for (register, (old, new)) in scalars.iter_mut().zip(&point.scalars).enumerate() {
            if old != new {
                let _ = write!(text, " s{register}={}", show(*new));
                *old = *new;
            }
        }
        text.push('\n');
    }
    text
}

/// Memory the device left that a comparison reads: a buffer, or a guest range a device-addressed
/// dispatch reached.
struct Place<'a> {
    /// How the comparison names it.
    name: String,
    /// The guest address of its first word.
    start: u64,
    /// Its words as the device left them.
    after: Vec<u32>,
    /// Its words before, where the capture holds them.
    before: Option<&'a [u32]>,
    /// Whether the device stored the word at an index.
    stored_at: Box<dyn Fn(usize) -> bool + 'a>,
}

/// Each buffer with words of its own the device answers, and each guest range: a buffer's stores are
/// its write marks; a guest range keeps none, so a word the device stored there is one it changed.
fn device_places<'a>(
    capture: &'a Capture<'_>,
    device: &dyn Fn(&str) -> Option<Vec<u32>>,
) -> Vec<Place<'a>> {
    let mut places = Vec::new();
    for (slot, (buffer, (start, _))) in capture
        .buffers
        .iter()
        .zip(&capture.ranges.buffers)
        .enumerate()
    {
        if matches!(buffer, CapturedBuffer::Alias(..)) {
            continue;
        }
        let Some(after) = device(&format!("buffer-{slot}.bin")) else {
            continue;
        };
        let marks = device(&format!("buffer-{slot}.written.bin"));
        places.push(Place {
            name: format!("buffer {slot}"),
            start: *start,
            after,
            before: match buffer {
                CapturedBuffer::Words(words) => Some(&words[..]),
                _ => None,
            },
            stored_at: Box::new(move |index| {
                marks
                    .as_ref()
                    .is_some_and(|marks| marks.get(index) != Some(&0))
            }),
        });
    }
    for (index, range) in capture.guest.iter().enumerate() {
        let Some(after) = device(&format!("guest-{index}.bin")) else {
            continue;
        };
        let before = &range.words[..];
        let changed: Vec<bool> = after
            .iter()
            .enumerate()
            .map(|(at, word)| before.get(at) != Some(word))
            .collect();
        places.push(Place {
            name: format!("guest {:#x}", range.at),
            start: range.at,
            after,
            before: Some(before),
            stored_at: Box::new(move |at| changed.get(at).copied().unwrap_or(false)),
        });
    }
    places
}

/// Every word the host and the device stored differently, buffer by buffer, and how many of each
/// difference each of the host's stores accounts for. `device` answers a file of the device's
/// output by its `replay-dispatch --out` name.
fn compare(
    capture: &Capture<'_>,
    run: &WaveTrace,
    device: &dyn Fn(&str) -> Option<Vec<u32>>,
) -> String {
    let mut text = String::new();
    // By the host store that wrote it (or none), each kind of difference.
    let mut by_store: BTreeMap<Option<u32>, [usize; 3]> = BTreeMap::new();
    let mut listed = 0;
    let mut agreed = 0;
    // Of the words only the device stored, how many it stored as they already were.
    let mut device_only_unchanged = 0;
    for place in device_places(capture, device) {
        let Place {
            name,
            start,
            after,
            before,
            stored_at,
        } = place;
        for (index, stored) in after.iter().enumerate() {
            let address = start + 4 * index as u64;
            let device_stored = stored_at(index);
            let host = run.written.get(&address);
            let kind = match (host, device_stored) {
                // Stored by neither, or with a value the host does not know: not a difference it
                // can name.
                (None, false) | (Some((None, _)), _) => continue,
                (Some((Some(value), _)), true) if value == stored => {
                    agreed += 1;
                    continue;
                }
                (Some(_), true) => 0,
                (None, true) => 1,
                (Some(_), false) => 2,
            };
            let writer = host.map(|(_, at)| *at);
            by_store.entry(writer).or_default()[kind] += 1;
            if kind == 1 && before.and_then(|words| words.get(index)) == Some(stored) {
                device_only_unchanged += 1;
            }
            // Every word both stored differently, and the first of the rest.
            if kind == 0 || listed < 64 {
                listed += 1;
                let host = host
                    .and_then(|(value, _)| *value)
                    .map_or("-".to_owned(), |value| format!("{value:#010x}"));
                let device = if device_stored {
                    format!("{stored:#010x}")
                } else {
                    "-".to_owned()
                };
                let by = writer.map_or(String::new(), |at| format!("  host store at {at:#x}"));
                let _ = writeln!(
                    text,
                    "{name} word {index:#x} ({address:#x}): host {host} device {device}{by}"
                );
            }
        }
    }
    let differences: usize = by_store.values().flatten().sum();
    let _ = writeln!(
        text,
        concat!(
            "{agreed} words both stored alike; {differences} differ (listed {listed}); ",
            "{device_only_unchanged} of those only the device stored hold what they held before",
        ),
        agreed = agreed,
        differences = differences,
        listed = listed,
        device_only_unchanged = device_only_unchanged,
    );
    for (writer, [both, device_only, host_only]) in &by_store {
        let writer = writer.map_or("no host store".to_owned(), |at| {
            format!("host store {at:#x}")
        });
        let _ = writeln!(
            text,
            "  {writer}: {both} stored differently, {device_only} by the device only, {host_only} by the host only"
        );
    }
    text
}
