//! One frame's passes, each with what it left in the images it wrote (`ORBISTOUN_PASS_DUMP`,
//! D986).
//!
//! While a flip the run keeps is being drawn, every compute dispatch and every run of draws into one
//! set of colour targets is a pass. After each, the images it wrote are written out as their
//! texels: a dispatch's stored images as the device answered them, the draws' colour targets as
//! read back. Two listings go beside them: `passes.tsv`, a line a pass naming its kind, program,
//! work and the images it read, and `outputs.tsv`, a line an image it wrote with its address,
//! format and extent. A pass the host refused is listed too, with what it would have written where
//! that is known, and writes nothing.
//!
//! The judging half is pure: [`texel`] reads one texel of a listed format as floats, and
//! [`judge`] measures a written image for what a correct pass seldom leaves - non-finite values,
//! nothing but zero, or neighbouring texels as unlike each other as any two are (noise).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{ResourceId, TargetEncoding};

/// The colour targets a run of draws drew into, read back: each one's encoding, extent and bytes as
/// the device holds them, `None` for one that could not be read. Installed by the worker, which
/// owns the device.
pub type TargetReader = fn(&[ResourceId]) -> Vec<Option<TargetTexels>>;

/// One colour target as read back after a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTexels {
    /// How its texels hold colour.
    pub encoding: TargetEncoding,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Its bytes, tightly packed, as the encoding holds them.
    pub bytes: Vec<u8>,
}

/// Whether a frame's passes are being dumped now: checked before any work is done for a pass.
static ARMED: AtomicBool = AtomicBool::new(false);

/// The dump in progress.
static STATE: Mutex<State> = Mutex::new(State::new());

/// A run of draws whose targets are not yet read back: draws into the same targets after it extend
/// it, and anything else reads it back first.
#[derive(Debug)]
struct OpenDraws {
    /// Its pass number.
    pass: u32,
    /// The colour targets, each its id and guest address.
    targets: Vec<(ResourceId, u64)>,
    /// Draws so far.
    draws: usize,
    /// The guest pixel programs bound, by name.
    programs: BTreeSet<String>,
    /// The images its draws sample, by guest address.
    inputs: BTreeSet<u64>,
}

#[derive(Debug)]
struct State {
    /// Where the dumps of the flip being drawn go.
    dir: Option<PathBuf>,
    /// The next pass's number.
    next: u32,
    /// The draws not yet read back.
    open: Option<OpenDraws>,
    /// The hash of what each image address last held when written out, and the file it went to:
    /// an image a pass left as it was is listed by the earlier file.
    written: Option<HashMap<u64, (u64, String)>>,
    /// The reader of draws' targets.
    reader: Option<TargetReader>,
}

impl State {
    const fn new() -> Self {
        Self {
            dir: None,
            next: 0,
            open: None,
            written: None,
            reader: None,
        }
    }
}

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Installs what reads a run of draws' colour targets back. First install wins.
pub fn install_target_reader(reader: TargetReader) {
    let mut state = state();
    if state.reader.is_none() {
        state.reader = Some(reader);
    }
}

/// Whether passes are being dumped now.
#[must_use]
pub fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

/// Starts dumping the passes of the frame drawn before guest flip `flip` into `dir`'s
/// `passes-<flip>/`, or with `None` ends the dump in progress, reading back the draws still open.
pub fn arm(at: Option<(u64, &Path)>) {
    flush_open();
    let mut state = state();
    if let Some(dir) = state.dir.take() {
        tracing::info!(
            "{} passes dumped into {}; orbistoun-cli passes reads them",
            state.next,
            dir.display()
        );
    }
    state.written = None;
    ARMED.store(false, Ordering::Relaxed);
    let Some((flip, dir)) = at else {
        return;
    };
    let dir = dir.join(format!("passes-{flip:06}"));
    let headers = [
        (PASSES, "pass\tkind\tprogram\twork\tinputs\tnote\n"),
        (
            OUTPUTS,
            "pass\tslot\taddress\tformat\twidth\theight\tlayers\tfile\n",
        ),
    ];
    let made = std::fs::create_dir_all(&dir).and_then(|()| {
        headers
            .iter()
            .try_for_each(|(name, header)| std::fs::write(dir.join(name), header))
    });
    if let Err(e) = made {
        tracing::warn!("the passes of flip {flip} cannot be dumped: {e}");
        return;
    }
    tracing::info!(
        "dumping every pass of the frame before flip {flip} into {} (D986)",
        dir.display()
    );
    state.dir = Some(dir);
    state.next = 0;
    state.written = Some(HashMap::new());
    ARMED.store(true, Ordering::Relaxed);
}

/// The listing of passes.
pub const PASSES: &str = "passes.tsv";
/// The listing of what each pass wrote.
pub const OUTPUTS: &str = "outputs.tsv";

/// The kind of a pass, as its listing names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A compute dispatch that ran.
    Dispatch,
    /// A run of draws into one set of colour targets that ran.
    Draws,
    /// A compute dispatch the host refused: it wrote nothing.
    RefusedDispatch,
    /// Draws the host refused: they wrote nothing.
    RefusedDraws,
}

impl Kind {
    /// Its name in the listing.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Draws => "draws",
            Self::RefusedDispatch => "refused-dispatch",
            Self::RefusedDraws => "refused-draws",
        }
    }

    /// Whether the host refused the pass.
    #[must_use]
    pub const fn refused(self) -> bool {
        matches!(self, Self::RefusedDispatch | Self::RefusedDraws)
    }
}

/// One image a pass wrote: where, what a texel is, its extent, and its texels when it has them.
#[derive(Debug, Clone)]
pub struct Written<'a> {
    /// Its guest address; zero for one the host holds alone.
    pub address: u64,
    /// Its format, as [`texel`] reads it.
    pub format: String,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Slices or layers, one for a 2D image.
    pub layers: u32,
    /// Its texels, `None` for a pass that wrote nothing.
    pub bytes: Option<std::borrow::Cow<'a, [u8]>>,
}

/// One pass as its listing says it.
#[derive(Debug, Clone)]
pub struct Pass<'a> {
    /// What it is.
    pub kind: Kind,
    /// The guest program that made it, by name.
    pub program: String,
    /// What it did: a dispatch's grid, the draws' count.
    pub work: String,
    /// The images it read, by guest address.
    pub inputs: Vec<u64>,
    /// Why it was refused, or anything else worth saying.
    pub note: String,
    /// What it wrote.
    pub outputs: Vec<Written<'a>>,
}

/// Lists a compute dispatch that ran, with the images it stored: `stored` the texels the device
/// answered for each of `images` in turn.
pub fn dispatch_ran(
    (program, groups): (u64, [u32; 3]),
    images: &crate::pipeline::DispatchImages,
    stored: &[Vec<u32>],
) {
    if !armed() {
        return;
    }
    let outputs = images
        .stored
        .iter()
        .enumerate()
        .map(|(slot, image)| {
            let bytes = stored
                .get(slot)
                .filter(|words| !words.is_empty())
                .map(|words| std::borrow::Cow::Borrowed(zerocopy::IntoBytes::as_bytes(&words[..])));
            stored_written(image, bytes)
        })
        .collect();
    record(&Pass {
        kind: Kind::Dispatch,
        program: format!("{program:#x}"),
        work: format!("{groups:?} groups"),
        inputs: dispatch_inputs(images),
        note: String::new(),
        outputs,
    });
}

/// The listing's line for a stored image, with `bytes` its texels after.
fn stored_written<'a>(
    image: &crate::pipeline::StoredImage,
    bytes: Option<std::borrow::Cow<'a, [u8]>>,
) -> Written<'a> {
    let texels = &image.texels;
    Written {
        address: image.span().map_or(0, |(base, _)| base),
        format: format!("image:{:?}", texels.format),
        width: texels.width,
        height: texels.height,
        layers: texels.depth.or(texels.layers).unwrap_or(1),
        bytes,
    }
}

/// The images a dispatch reads, by guest address.
fn dispatch_inputs(images: &crate::pipeline::DispatchImages) -> Vec<u64> {
    images
        .fetched_span
        .into_iter()
        .chain(images.more_fetched.iter().map(|(_, span)| *span))
        .chain(
            images
                .selected
                .iter()
                .flat_map(|selected| selected.images.iter().map(|(_, span)| *span)),
        )
        .map(|(base, _)| base)
        .collect()
}

thread_local! {
    /// The images the dispatch being carried out on this thread would store to, once prepared: what
    /// a refusal after its preparation names as left unwritten.
    static PREPARED: std::cell::RefCell<Option<(Vec<u64>, Vec<Written<'static>>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Notes what the dispatch being carried out on this thread would read and store, once prepared.
pub fn dispatch_prepared(images: &crate::pipeline::DispatchImages) {
    if !armed() {
        return;
    }
    let outputs = images
        .stored
        .iter()
        .map(|image| stored_written(image, None))
        .collect();
    PREPARED.with(|held| *held.borrow_mut() = Some((dispatch_inputs(images), outputs)));
}

/// Lists a compute dispatch the host refused, `why`, with what it would have read and written where
/// it was prepared before it was refused.
pub fn dispatch_refused(program: Option<u64>, groups: [u32; 3], why: &str) {
    let prepared = PREPARED.with(|held| held.borrow_mut().take());
    if !armed() {
        return;
    }
    let (inputs, outputs) = prepared.unwrap_or_default();
    record(&Pass {
        kind: Kind::RefusedDispatch,
        program: program.map_or_else(|| "unknown".to_owned(), |at| format!("{at:#x}")),
        work: format!("{groups:?} groups"),
        inputs,
        note: why.to_owned(),
        outputs,
    });
}

/// Forgets what the dispatch carried out on this thread was prepared to write: it ran.
pub fn dispatch_done() {
    PREPARED.with(|held| held.borrow_mut().take());
}

/// Notes a run of draws that was carried out, `submission` as prepared for it: draws into the
/// targets the open run draws into extend it, and draws into others read the open run back first.
pub fn draws_ran(submission: &crate::pipeline::Submission) {
    if !armed() {
        return;
    }
    let targets = draw_targets(submission);
    let mut programs = BTreeSet::new();
    let mut draws = 0;
    for command in &submission.commands {
        match command {
            crate::RenderCommand::BindShader {
                stage: crate::ShaderStage::Fragment,
                shader,
            } => {
                programs.insert(
                    crate::pipeline::shader_name(*shader).unwrap_or_else(|| format!("{shader:?}")),
                );
            }
            crate::RenderCommand::Draw { .. } | crate::RenderCommand::DrawIndexed { .. } => {
                draws += 1;
            }
            crate::RenderCommand::DrawChunks { chunks, .. } => {
                draws += usize::try_from(*chunks).unwrap_or(1);
            }
            _ => {}
        }
    }
    let inputs: BTreeSet<u64> = submission
        .report
        .bound_slots
        .iter()
        .map(|&(_, base)| base)
        .collect();
    {
        let mut state = state();
        if let Some(open) = state.open.as_mut()
            && open.targets == targets
        {
            open.draws += draws;
            open.programs.extend(programs);
            open.inputs.extend(inputs);
            return;
        }
    }
    flush_open();
    let mut state = state();
    let pass = state.next;
    state.next += 1;
    state.open = Some(OpenDraws {
        pass,
        targets,
        draws,
        programs,
        inputs,
    });
}

/// The colour targets a submission's draws draw into, each its id and guest address: those its
/// last `SetRenderTargets` names.
fn draw_targets(submission: &crate::pipeline::Submission) -> Vec<(ResourceId, u64)> {
    let ids = submission
        .commands
        .iter()
        .rev()
        .find_map(|command| match command {
            crate::RenderCommand::SetRenderTargets { colour, .. } => Some(colour.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let addresses: Vec<u64> = submission
        .colour_target
        .iter()
        .map(|target| target.base)
        .chain(
            submission
                .more_colour_targets
                .iter()
                .map(|more| more.target.map_or(0, |target| target.base)),
        )
        .collect();
    ids.iter()
        .enumerate()
        .map(|(at, &id)| (id, addresses.get(at).copied().unwrap_or(0)))
        .collect()
}

/// Lists draws the host refused, `why`: they wrote nothing. Draws refused after the command
/// processor went on (D948) are listed where the refusal is learned.
pub fn draws_refused(why: &str) {
    if !armed() {
        return;
    }
    record(&Pass {
        kind: Kind::RefusedDraws,
        program: String::new(),
        work: String::new(),
        inputs: Vec::new(),
        note: why.to_owned(),
        outputs: Vec::new(),
    });
}

/// Reads back the open run of draws and lists it.
fn flush_open() {
    let (open, reader) = {
        let mut state = state();
        (state.open.take(), state.reader)
    };
    let Some(open) = open else {
        return;
    };
    let ids: Vec<ResourceId> = open.targets.iter().map(|&(id, _)| id).collect();
    let read = reader.map(|read| read(&ids)).unwrap_or_default();
    let outputs = open
        .targets
        .iter()
        .enumerate()
        .map(
            |(at, &(_, address))| match read.get(at).cloned().flatten() {
                Some(target) => Written {
                    address,
                    format: format!("target:{:?}", target.encoding),
                    width: target.width,
                    height: target.height,
                    layers: 1,
                    bytes: Some(std::borrow::Cow::Owned(target.bytes)),
                },
                None => Written {
                    address,
                    format: "unread".to_owned(),
                    width: 0,
                    height: 0,
                    layers: 1,
                    bytes: None,
                },
            },
        )
        .collect();
    write_pass(
        open.pass,
        &Pass {
            kind: Kind::Draws,
            program: open.programs.into_iter().collect::<Vec<_>>().join(","),
            work: format!("{} draws", open.draws),
            inputs: open.inputs.into_iter().collect(),
            note: String::new(),
            outputs,
        },
    );
}

/// Lists a pass that is not a run of draws: the open run is read back and listed first.
fn record(pass: &Pass<'_>) {
    flush_open();
    let number = {
        let mut state = state();
        let number = state.next;
        state.next += 1;
        number
    };
    write_pass(number, pass);
}

/// Writes pass `number`'s lines and the texels it wrote.
fn write_pass(number: u32, pass: &Pass<'_>) {
    let mut state = state();
    let Some(dir) = state.dir.clone() else {
        return;
    };
    let mut outputs = String::new();
    for (slot, written) in pass.outputs.iter().enumerate() {
        let file = match &written.bytes {
            None => "-".to_owned(),
            Some(bytes) => {
                let hash = bytes_hash(bytes);
                let kept = state.written.as_mut();
                match kept.as_ref().and_then(|kept| kept.get(&written.address)) {
                    Some((before, file)) if *before == hash && written.address != 0 => {
                        format!("={file}")
                    }
                    _ => {
                        let file = format!("p{number:04}-o{slot}-{:x}.bin", written.address);
                        if let Err(e) = std::fs::write(dir.join(&file), bytes) {
                            tracing::warn!("pass {number}'s output could not be written: {e}");
                        }
                        if let Some(kept) = kept {
                            kept.insert(written.address, (hash, file.clone()));
                        }
                        file
                    }
                }
            }
        };
        let _ = writeln!(
            outputs,
            "{number}\t{slot}\t{:#x}\t{}\t{}\t{}\t{}\t{file}",
            written.address, written.format, written.width, written.height, written.layers
        );
    }
    let inputs: Vec<String> = pass.inputs.iter().map(|at| format!("{at:#x}")).collect();
    let line = format!(
        "{number}\t{}\t{}\t{}\t{}\t{}\n",
        pass.kind.name(),
        pass.program,
        pass.work,
        inputs.join(","),
        pass.note.replace(['\t', '\n'], " ")
    );
    drop(state);
    let appended =
        append(&dir.join(PASSES), &line).and_then(|()| append(&dir.join(OUTPUTS), &outputs));
    if let Err(e) = appended {
        tracing::warn!("pass {number} could not be listed: {e}");
    }
}

/// A hash of an image's bytes, to tell one a pass left as it was.
fn bytes_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?
        .write_all(text.as_bytes())
}

/// What a texel's channels are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numeric {
    /// Normalised to `[0, 1]` (or `[-1, 1]`).
    Normalised,
    /// Floats, which may be anything.
    Float,
    /// Integers.
    Integer,
}

/// How a listed format's texels are read: bytes a texel, channels, and what they are. `None` for a
/// format this does not read.
#[must_use]
pub fn layout(format: &str) -> Option<(usize, usize, Numeric)> {
    use Numeric::{Float, Integer, Normalised};
    // A dispatch image's (`image:`) texels are a word or more each, the texel in the low bytes; a
    // target's (`target:`) are as its encoding holds them. The two name no format alike but
    // `Float11_11_10`, which both hold one way.
    let name = format.split_once(':')?.1;
    Some(match name {
        "Rgba8" | "Rgba8Srgb" | "Rgb10A2" | "Rgba8Snorm" | "Unorm8" | "Srgb8" | "Unorm10x3A2"
        | "Snorm8" => (4, 4, Normalised),
        "R8" | "R16Unorm" => (4, 1, Normalised),
        "Rg8Unorm" | "Unorm8x2" | "Unorm16x2" => (4, 2, Normalised),
        "Rgba8Uint" => (4, 4, Integer),
        "R8Uint" | "R16Uint" | "R32Uint" | "Uint16" => (4, 1, Integer),
        "R16Float" | "R32Float" | "Float32" | "Float16" => (4, 1, Float),
        "Rg16Float" | "Float16x2" => (4, 2, Float),
        "Float11_11_10" => (4, 3, Float),
        "Rgba16Float" | "Float16x4" => (8, 4, Float),
        "Rgba16Unorm" => (8, 4, Normalised),
        "Rg32Uint" | "Uint32x2" => (8, 2, Integer),
        "Float32x2" => (8, 2, Float),
        "Rgba32Uint" => (16, 4, Integer),
        "Float32x4" => (16, 4, Float),
        _ => return None,
    })
}

/// One texel of `format` from its bytes, as up to four floats, the channels it lacks zero.
// An integer channel past 2^24 is shown to the nearest float: the picture and the judging need no
// more.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn texel(format: &str, bytes: &[u8]) -> Option<[f32; 4]> {
    let name = format.split_once(':')?.1;
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(at * 4..at * 4 + 4)?.try_into().ok()?,
        ))
    };
    let unorm = |value: u32, bits: u32| value as f32 / ((1_u64 << bits) - 1) as f32;
    let snorm8 = |byte: u32| (f32::from(byte as u8 as i8) / 127.0).max(-1.0);
    let half = |bits: u32| crate::formats::half_to_f32(bits as u16);
    let w0 = word(0)?;
    let bytes4 = |w: u32, f: &dyn Fn(u32) -> f32| {
        [f(w & 0xFF), f(w >> 8 & 0xFF), f(w >> 16 & 0xFF), f(w >> 24)]
    };
    Some(match name {
        "Rgba8" | "Rgba8Srgb" | "Unorm8" | "Srgb8" => bytes4(w0, &|b| unorm(b, 8)),
        "Rgba8Snorm" | "Snorm8" => bytes4(w0, &snorm8),
        "Rgba8Uint" => bytes4(w0, &|b| b as f32),
        "R8" => [unorm(w0 & 0xFF, 8), 0.0, 0.0, 0.0],
        "R8Uint" => [(w0 & 0xFF) as f32, 0.0, 0.0, 0.0],
        "R16Unorm" => [unorm(w0 & 0xFFFF, 16), 0.0, 0.0, 0.0],
        "R16Uint" | "Uint16" => [(w0 & 0xFFFF) as f32, 0.0, 0.0, 0.0],
        "R32Uint" => [w0 as f32, 0.0, 0.0, 0.0],
        "R16Float" | "Float16" => [half(w0 & 0xFFFF), 0.0, 0.0, 0.0],
        "R32Float" | "Float32" => [f32::from_bits(w0), 0.0, 0.0, 0.0],
        "Rg16Float" | "Float16x2" => [half(w0 & 0xFFFF), half(w0 >> 16), 0.0, 0.0],
        "Rg8Unorm" | "Unorm8x2" => [unorm(w0 & 0xFF, 8), unorm(w0 >> 8 & 0xFF, 8), 0.0, 0.0],
        "Unorm16x2" => [unorm(w0 & 0xFFFF, 16), unorm(w0 >> 16, 16), 0.0, 0.0],
        "Float11_11_10" => [
            crate::formats::small_float(w0 & 0x7FF, 6),
            crate::formats::small_float(w0 >> 11 & 0x7FF, 6),
            crate::formats::small_float(w0 >> 22, 5),
            0.0,
        ],
        "Rgb10A2" | "Unorm10x3A2" => [
            unorm(w0 & 0x3FF, 10),
            unorm(w0 >> 10 & 0x3FF, 10),
            unorm(w0 >> 20 & 0x3FF, 10),
            unorm(w0 >> 30, 2),
        ],
        "Rgba16Float" | "Float16x4" => {
            let w1 = word(1)?;
            [
                half(w0 & 0xFFFF),
                half(w0 >> 16),
                half(w1 & 0xFFFF),
                half(w1 >> 16),
            ]
        }
        "Rgba16Unorm" => {
            let w1 = word(1)?;
            [
                unorm(w0 & 0xFFFF, 16),
                unorm(w0 >> 16, 16),
                unorm(w1 & 0xFFFF, 16),
                unorm(w1 >> 16, 16),
            ]
        }
        "Float32x2" => [f32::from_bits(w0), f32::from_bits(word(1)?), 0.0, 0.0],
        "Rg32Uint" | "Uint32x2" => [w0 as f32, word(1)? as f32, 0.0, 0.0],
        "Float32x4" => [
            f32::from_bits(w0),
            f32::from_bits(word(1)?),
            f32::from_bits(word(2)?),
            f32::from_bits(word(3)?),
        ],
        "Rgba32Uint" => [w0 as f32, word(1)? as f32, word(2)? as f32, word(3)? as f32],
        _ => return None,
    })
}

/// What [`judge`] measured of an image.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Judged {
    /// Texels measured.
    pub texels: u64,
    /// Channel values that are NaN.
    pub nan: u64,
    /// Channel values that are infinite.
    pub inf: u64,
    /// Texels whose every channel is zero.
    pub zero: u64,
    /// The mean difference between a texel and its right and lower neighbours over the mean
    /// difference between texels half the image apart, over the colour channels: near zero for a
    /// picture, near one where neighbours are as unlike as any two texels (noise). Zero for an image
    /// whose texels are all alike.
    pub noise: f32,
    /// The largest finite channel value, for scaling a float image to view.
    pub peak: f32,
}

impl Judged {
    /// The flags a pass's listing gives this image: `nan`, `inf`, `zero` (nothing but zero) and
    /// `noise` (above [`NOISE_FLAG`]).
    #[must_use]
    pub fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if self.nan > 0 {
            flags.push("nan");
        }
        if self.inf > 0 {
            flags.push("inf");
        }
        if self.texels > 0 && self.zero == self.texels {
            flags.push("zero");
        }
        if self.noise > NOISE_FLAG {
            flags.push("noise");
        }
        flags
    }
}

/// The neighbour-difference ratio past which an image is flagged as noise: a picture's neighbours
/// are far more alike than texels across it, and coloured static's are not.
pub const NOISE_FLAG: f32 = 0.6;

/// Measures `width` x `height` (x `layers`) texels of `format` in `bytes`: non-finite values, zero
/// texels, and how alike neighbours are against texels far apart. `None` for a format [`texel`]
/// does not read.
// The sums' counts are texels, far below 2^52: the ratio is exact enough to judge by.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn judge(
    format: &str,
    (width, height, layers): (u32, u32, u32),
    bytes: &[u8],
) -> Option<Judged> {
    let (stride, channels, _) = layout(format)?;
    let (width, height) = (width as usize, height as usize);
    let plane = width * height;
    let total = (plane * layers.max(1) as usize).min(bytes.len() / stride);
    let mut judged = Judged::default();
    let read = |index: usize| texel(format, &bytes[index * stride..index * stride + stride]);
    // The colour channels the noise is measured over: alpha aside where there are four.
    let colour = channels.min(3);
    let finite_sum = |a: [f32; 4], b: [f32; 4]| -> Option<f64> {
        let mut sum = 0.0;
        for c in 0..colour {
            let d = f64::from(a[c]) - f64::from(b[c]);
            if !d.is_finite() {
                return None;
            }
            sum += d.abs();
        }
        Some(sum)
    };
    let (mut near, mut near_n, mut far, mut far_n) = (0.0_f64, 0_u64, 0.0_f64, 0_u64);
    for index in 0..total {
        let Some(value) = read(index) else { continue };
        judged.texels += 1;
        let mut all_zero = true;
        for &channel in value.iter().take(channels) {
            if channel.is_nan() {
                judged.nan += 1;
            } else if channel.is_infinite() {
                judged.inf += 1;
            } else {
                judged.peak = judged.peak.max(channel);
            }
            if channel != 0.0 {
                all_zero = false;
            }
        }
        if all_zero {
            judged.zero += 1;
        }
        let within = index % plane;
        let (x, y) = (within % width, within / width);
        let layer = index - within;
        // Every texel against its right and lower neighbours, and against the texel half the image
        // away across and down.
        for neighbour in [
            (x + 1 < width).then(|| index + 1),
            (y + 1 < height).then(|| index + width),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(other) = read(neighbour)
                && let Some(d) = finite_sum(value, other)
            {
                near += d;
                near_n += 1;
            }
        }
        let away = layer + ((y + height / 2) % height) * width + (x + width / 2) % width;
        if away < total
            && let Some(other) = read(away)
            && let Some(d) = finite_sum(value, other)
        {
            far += d;
            far_n += 1;
        }
    }
    if near_n > 0 && far_n > 0 && far > 0.0 {
        judged.noise = ((near / near_n as f64) / (far / far_n as f64)) as f32;
    }
    Some(judged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba8(texels: &[[u8; 4]]) -> Vec<u8> {
        texels.iter().flatten().copied().collect()
    }

    /// A deterministic sequence of bytes as unlike their neighbours as any.
    fn noise_bytes(count: usize) -> Vec<u8> {
        let mut state = 0x1234_5678_u32;
        (0..count)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn a_gradient_is_not_noise_and_static_is() {
        let (w, h) = (64, 64);
        let gradient: Vec<[u8; 4]> = (0..w * h)
            .map(|i| [(i % w * 4) as u8, (i / w * 4) as u8, 128, 255])
            .collect();
        let judged = judge("image:Rgba8", (w as u32, h as u32, 1), &rgba8(&gradient)).unwrap();
        assert!(judged.noise < 0.2, "{judged:?}");
        assert!(judged.flags().is_empty(), "{judged:?}");
        let static_ = noise_bytes(w * h * 4);
        let judged = judge("target:Unorm8", (w as u32, h as u32, 1), &static_).unwrap();
        assert!(judged.noise > NOISE_FLAG, "{judged:?}");
        assert_eq!(judged.flags(), ["noise"]);
    }

    #[test]
    fn non_finite_and_zero_texels_are_counted() {
        // Four half-float texels: zero, NaN in red, infinity in green, one.
        let halves: [[u16; 4]; 4] = [
            [0, 0, 0, 0],
            [0x7E00, 0, 0, 0],
            [0, 0x7C00, 0, 0],
            [0x3C00, 0x3C00, 0x3C00, 0x3C00],
        ];
        let bytes: Vec<u8> = halves
            .iter()
            .flatten()
            .flat_map(|h| h.to_le_bytes())
            .collect();
        let judged = judge("image:Rgba16Float", (2, 2, 1), &bytes).unwrap();
        assert_eq!(
            (judged.texels, judged.nan, judged.inf, judged.zero),
            (4, 1, 1, 1)
        );
        assert!((judged.peak - 1.0).abs() < f32::EPSILON, "{judged:?}");
        assert_eq!(judged.flags(), ["nan", "inf"]);
        let zeros = vec![0; 16];
        assert_eq!(
            judge("target:Float32", (2, 2, 1), &zeros).unwrap().flags(),
            ["zero"]
        );
    }

    #[test]
    fn texels_read_as_their_formats_hold_them() {
        assert_eq!(
            texel("image:Rgb10A2", &(1023_u32 | 3 << 30).to_le_bytes()),
            Some([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            texel("target:Float16x2", &0x3C00_4000_u32.to_le_bytes()),
            Some([2.0, 1.0, 0.0, 0.0])
        );
        assert_eq!(
            texel("image:R8", &[255, 9, 9, 9]),
            Some([1.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(texel("target:Nothing", &[0; 4]), None);
        assert_eq!(layout("image:Rgba16Float"), Some((8, 4, Numeric::Float)));
        assert_eq!(layout("target:Float16x4"), Some((8, 4, Numeric::Float)));
    }
}
