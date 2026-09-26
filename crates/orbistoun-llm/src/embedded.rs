//! A model downloaded once and then run inside this process.
//!
//! No server or daemon. The first request that needs the weights downloads them - never at
//! startup, construction or `check` - so a configured but unused engine costs nothing. The files
//! go into a directory the caller supplies, since the caller holds `orbistoun-paths`' guarantee
//! that nothing is written outside the resolved root. Downloads are written to `.part` and
//! renamed on completion, so a file that exists is whole and a truncated GGUF never reaches a
//! loader.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use candle_core::quantized::gguf_file;
use candle_core::{Device as CandleDevice, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::{quantized_qwen2, quantized_qwen3};
use tokenizers::Tokenizer;

use crate::Error;
use crate::catalog::{Arch, Offline};
use crate::engine::{Engine, Request};
use crate::select::Device;

/// Where a model's files are fetched from; the path shape is that host's published convention.
const HOST: &str = "https://huggingface.co";

/// The tokenizer file, which lives in the base repository rather than beside the
/// quantised weights.
const TOKENIZER: &str = "tokenizer.json";

/// How long to wait for a connection before giving up on the host.
///
/// There is no overall deadline: a slow link fetching gigabytes is working, and a dead host
/// shows up at connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// A model that runs here.
///
/// The weights load on first use and stay loaded. The mutex exists because generation walks a
/// key-value cache for one sequence at a time.
#[derive(Debug)]
pub struct EmbeddedEngine {
    model: Offline,
    device: Device,
    cache_root: PathBuf,
    loaded: Mutex<Option<Loaded>>,
}

/// Weights, tokeniser, and the device they were placed on.
struct Loaded {
    weights: Weights,
    tokenizer: Tokenizer,
    device: CandleDevice,
    end_of_turn: u32,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Written rather than derived: neither the weights nor the tokeniser belong in a debug line.
        f.debug_struct("Loaded")
            .field("device", &self.device)
            .field("end_of_turn", &self.end_of_turn)
            .finish_non_exhaustive()
    }
}

/// One loader per architecture, dispatched from catalogue data.
///
/// An enum: the loaders are named in a data file and an unknown name is refused, because another
/// family's loader produces fluent wrong output rather than failing.
enum Weights {
    Qwen3(quantized_qwen3::ModelWeights),
    Qwen2(quantized_qwen2::ModelWeights),
}

impl Weights {
    fn forward(&mut self, input: &Tensor, position: usize) -> candle_core::Result<Tensor> {
        match self {
            Self::Qwen3(model) => model.forward(input, position),
            Self::Qwen2(model) => model.forward(input, position),
        }
    }
}

impl EmbeddedEngine {
    /// Prepares an engine. Downloads nothing and loads nothing.
    #[must_use]
    pub fn new(model: Offline, device: Device, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            model,
            device,
            cache_root: cache_root.into(),
            loaded: Mutex::new(None),
        }
    }

    /// Where this model's files live.
    #[must_use]
    pub fn model_dir(&self) -> PathBuf {
        self.cache_root.join(&self.model.id)
    }

    /// True when the weights are already on disk, so a caller can warn about a
    /// download before starting one.
    #[must_use]
    pub fn is_downloaded(&self) -> bool {
        let dir = self.model_dir();
        whole(&dir.join(&self.model.file)) && whole(&dir.join(TOKENIZER))
    }

    /// Fetches whatever is missing.
    ///
    /// # Errors
    ///
    /// If the directory cannot be created, the host cannot be reached, or a file
    /// arrives incomplete.
    pub fn ensure_downloaded(&self) -> Result<(), Error> {
        ensure_model(&self.model, &self.cache_root).map(|_| ())
    }

    /// Loads the weights, downloading them first if they are absent.
    fn load(&self) -> Result<Loaded, Error> {
        self.ensure_downloaded()?;
        let dir = self.model_dir();

        let device = self.candle_device();
        tracing::info!(
            model = %self.model.id,
            device = ?device,
            "loading model"
        );

        let path = dir.join(&self.model.file);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| Error::Model(format!("opening {}: {e}", path.display())))?;
        let content = gguf_file::Content::read(&mut file)
            .map_err(|e| Error::Model(format!("reading {}: {e}", path.display())))?;

        let weights = match self.model.arch {
            Arch::Qwen3 => quantized_qwen3::ModelWeights::from_gguf(content, &mut file, &device)
                .map(Weights::Qwen3),
            Arch::Qwen2 => quantized_qwen2::ModelWeights::from_gguf(content, &mut file, &device)
                .map(Weights::Qwen2),
        }
        .map_err(|e| Error::Model(format!("loading {} weights: {e}", self.model.id)))?;

        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER))
            .map_err(|e| Error::Model(format!("loading the tokeniser: {e}")))?;
        let end_of_turn = tokenizer.token_to_id(END_OF_TURN).ok_or_else(|| {
            // Without the end-of-turn token, generation stops only at the token cap and every reply
            // carries the start of an invented next turn.
            Error::Model(format!(
                concat!(
                    "the tokeniser for {} has no `{}` token, so nothing would end ",
                    "a reply"
                ),
                self.model.id, END_OF_TURN
            ))
        })?;

        tracing::info!(model = %self.model.id, "model ready");
        Ok(Loaded {
            weights,
            tokenizer,
            device,
            end_of_turn,
        })
    }

    /// The device to place weights on: the processor, always.
    ///
    /// This engine is the CPU fallback; [`crate::runtime`] reaches a GPU with no build-time vendor
    /// toolkit (D219). An entry asking for an accelerator here cannot be honoured, and says so.
    fn candle_device(&self) -> CandleDevice {
        if self.device == Device::Gpu {
            tracing::warn!(
                model = %self.model.id,
                concat!(
                    "the in-process engine has no accelerator support; running on the ",
                    "processor. The managed runtime is the accelerated path"
                )
            );
        }
        CandleDevice::Cpu
    }
}

/// Fetches a model's files if they are missing, and returns the weights.
///
/// Free rather than a method because this engine and [`crate::runtime`] use the same GGUF, so a
/// machine that used one has the other's download.
///
/// # Errors
///
/// If the directory cannot be created, the host cannot be reached, or a file arrives
/// incomplete.
pub fn ensure_model(model: &Offline, cache_root: &Path) -> Result<PathBuf, Error> {
    let dir = cache_root.join(&model.id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::Download(format!("creating {}: {e}", dir.display())))?;

    let weights = dir.join(&model.file);
    fetch(
        &format!("{HOST}/{}/resolve/main/{}", model.repo, model.file),
        &weights,
        model.download_mb,
        &model.id,
    )?;
    fetch(
        &format!("{HOST}/{}/resolve/main/{TOKENIZER}", model.tokenizer_repo),
        &dir.join(TOKENIZER),
        0,
        &model.id,
    )?;
    Ok(weights)
}

/// Whether the in-process engine can put weights on an accelerator. It cannot.
///
/// A function so its callers keep this engine from being offered for an accelerator entry,
/// which would size a model against VRAM and run it on the CPU. [`crate::runtime`] is the
/// accelerated path (D219).
#[must_use]
pub const fn accelerator_supported() -> bool {
    false
}

/// The token that ends a turn in this chat template.
const END_OF_TURN: &str = "<|im_end|>";

/// Wraps a request in the chat template these models were trained on.
///
/// `/no_think` is appended to the system message for Qwen3, which otherwise spends much of the
/// token budget reasoning before it answers; the reasoning is not recorded anywhere.
fn chat_prompt(arch: Arch, request: &Request) -> String {
    let mut system = request.system.clone().unwrap_or_default();
    if arch == Arch::Qwen3 {
        if !system.is_empty() {
            system.push('\n');
        }
        system.push_str("/no_think");
    }
    let mut prompt = String::new();
    if !system.is_empty() {
        prompt.push_str("<|im_start|>system\n");
        prompt.push_str(&system);
        prompt.push_str("<|im_end|>\n");
    }
    prompt.push_str("<|im_start|>user\n");
    prompt.push_str(&request.prompt);
    prompt.push_str("<|im_end|>\n<|im_start|>assistant\n");
    prompt
}

impl Engine for EmbeddedEngine {
    fn describe(&self) -> String {
        format!(
            "{} on the {}",
            self.model.id,
            match self.device {
                Device::Cpu => "CPU",
                Device::Gpu => "accelerator",
            }
        )
    }

    fn model(&self) -> String {
        self.model.id.clone()
    }

    fn complete(&self, request: &Request) -> Result<String, Error> {
        let mut guard = self
            .loaded
            .lock()
            .map_err(|_| Error::Model("the model lock was poisoned by an earlier panic".into()))?;
        if guard.is_none() {
            *guard = Some(self.load()?);
        }
        let loaded = guard.as_mut().expect("just loaded");
        let text = generate(loaded, &chat_prompt(self.model.arch, request), request)?;
        // `/no_think` suppresses the reasoning content but not the tags, so a reply can still arrive
        // wrapped in an empty pair.
        Ok(crate::engine::without_reasoning(&text).to_owned())
    }
}

/// Runs the model until it stops, hits a stop string, or reaches the cap.
fn generate(loaded: &mut Loaded, prompt: &str, request: &Request) -> Result<String, Error> {
    let fail = |e: candle_core::Error| Error::Model(e.to_string());

    let encoded = loaded
        .tokenizer
        .encode(prompt, true)
        .map_err(|e| Error::Model(format!("tokenising the prompt: {e}")))?;
    let prompt_tokens = encoded.get_ids();
    if prompt_tokens.is_empty() {
        return Err(Error::Model("the prompt tokenised to nothing".into()));
    }

    // Zero temperature is argmax, which candle expresses as no sampling rather than dividing
    // logits by zero.
    let temperature = (request.temperature > 0.0).then(|| f64::from(request.temperature));
    let mut sampler = LogitsProcessor::new(request.seed, temperature, None);

    let input = Tensor::new(prompt_tokens, &loaded.device)
        .map_err(fail)?
        .unsqueeze(0)
        .map_err(fail)?;
    let logits = loaded
        .weights
        .forward(&input, 0)
        .map_err(fail)?
        .squeeze(0)
        .map_err(fail)?;
    let mut next = sampler.sample(&logits).map_err(fail)?;

    let mut produced: Vec<u32> = Vec::new();
    for step in 0..request.max_tokens as usize {
        if next == loaded.end_of_turn {
            break;
        }
        produced.push(next);

        if !request.stop.is_empty() {
            let so_far = decode(loaded, &produced)?;
            if let Some(at) = request.stop.iter().find_map(|s| so_far.find(s.as_str())) {
                return Ok(so_far[..at].to_owned());
            }
        }

        let input = Tensor::new(&[next], &loaded.device)
            .map_err(fail)?
            .unsqueeze(0)
            .map_err(fail)?;
        let logits = loaded
            .weights
            .forward(&input, prompt_tokens.len() + step)
            .map_err(fail)?
            .squeeze(0)
            .map_err(fail)?;
        next = sampler.sample(&logits).map_err(fail)?;
    }

    decode(loaded, &produced)
}

fn decode(loaded: &Loaded, tokens: &[u32]) -> Result<String, Error> {
    loaded
        .tokenizer
        .decode(tokens, true)
        .map_err(|e| Error::Model(format!("decoding the reply: {e}")))
}

/// True when a file exists and holds something.
///
/// A zero-length file, as a download killed before its first write leaves, is treated as
/// absent.
fn whole(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
}

/// Downloads one file, unless it is already there.
///
/// Writes to `.part` and renames, so a file that exists is a file that is complete.
pub(crate) fn fetch(url: &str, destination: &Path, size_mb: u32, model: &str) -> Result<(), Error> {
    if whole(destination) {
        return Ok(());
    }
    if size_mb > 0 {
        tracing::info!(
            %model,
            size_mb,
            destination = %destination.display(),
            "downloading model weights - this happens once"
        );
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(None)
        .build()
        .map_err(|e| Error::Download(e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| Error::Download(format!("fetching {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Download(format!("fetching {url}: {e}")))?;
    let expected = response.content_length();

    let part = destination.with_extension("part");
    let mut file = std::fs::File::create(&part)
        .map_err(|e| Error::Download(format!("creating {}: {e}", part.display())))?;

    let mut buffer = vec![0_u8; 1 << 20];
    let mut written: u64 = 0;
    let mut last_report = 0_u64;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|e| Error::Download(format!("reading {url}: {e}")))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|e| Error::Download(format!("writing {}: {e}", part.display())))?;
        written += read as u64;
        if let Some(total) = expected {
            let percent = written * 100 / total.max(1);
            if percent >= last_report + 10 {
                last_report = percent;
                tracing::info!(%model, percent, "downloading");
            }
        }
    }
    file.flush()
        .map_err(|e| Error::Download(format!("flushing {}: {e}", part.display())))?;
    drop(file);

    // A truncated body is not a socket error, so the length is checked here before the rename
    // publishes the file.
    if let Some(total) = expected {
        if written != total {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Download(format!(
                "{url} ended after {written} of {total} bytes"
            )));
        }
    }
    std::fs::rename(&part, destination).map_err(|e| {
        Error::Download(format!(
            "publishing {} as {}: {e}",
            part.display(),
            destination.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{END_OF_TURN, EmbeddedEngine, chat_prompt, whole};
    use crate::catalog::{Arch, Catalog};
    use crate::engine::{Engine, Request};
    use crate::select::Device;

    fn engine(dir: &std::path::Path) -> EmbeddedEngine {
        let catalog = Catalog::default();
        let model = catalog.offline("qwen3-0.6b").expect("present").clone();
        EmbeddedEngine::new(model, Device::Cpu, dir)
    }

    /// Constructing an engine downloads nothing and writes nothing.
    #[test]
    fn constructing_an_engine_touches_no_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let built = engine(dir.path());
        assert!(!built.is_downloaded());
        assert!(
            std::fs::read_dir(dir.path())
                .expect("readable")
                .next()
                .is_none(),
            "construction created something"
        );
    }

    /// A zero-length file counts as absent.
    #[test]
    fn an_empty_file_is_not_a_download() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("weights.gguf");
        std::fs::write(&path, b"").expect("write");
        assert!(!whole(&path));
        std::fs::write(&path, b"x").expect("write");
        assert!(whole(&path));
    }

    /// The chat template closes the user turn and opens the assistant's; otherwise the model
    /// continues the user's message.
    #[test]
    fn the_chat_template_hands_the_turn_over() {
        let prompt = chat_prompt(Arch::Qwen2, &Request::new("why"));
        assert!(prompt.ends_with("<|im_start|>assistant\n"), "{prompt}");
        assert!(prompt.contains(END_OF_TURN));
    }

    /// Qwen3 is asked not to think out loud; Qwen2 has no such mode and is not.
    #[test]
    fn only_qwen3_is_told_not_to_think_aloud() {
        let three = chat_prompt(Arch::Qwen3, &Request::new("why").with_system("rules"));
        assert!(three.contains("/no_think"), "{three}");
        assert!(three.contains("rules"), "{three}");

        let two = chat_prompt(Arch::Qwen2, &Request::new("why").with_system("rules"));
        assert!(!two.contains("/no_think"), "{two}");
    }

    /// A model with no system message still gets the suppression.
    #[test]
    fn suppression_survives_an_absent_system_message() {
        let prompt = chat_prompt(Arch::Qwen3, &Request::new("why"));
        assert!(prompt.contains("/no_think"), "{prompt}");
    }

    /// The engine describes which pool it was placed in.
    #[test]
    fn an_engine_says_where_it_runs() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(engine(dir.path()).describe().contains("CPU"));
    }

    /// The offline path end to end: download, GGUF parse, architecture loader, chat template and
    /// stop token. Opt-in, since it downloads a model and runs inference.
    ///
    /// ```text
    /// cargo test -p orbistoun-llm --release -- --ignored embedded_model
    /// ```
    #[test]
    #[ignore = "downloads a model and runs inference; opt-in via --ignored"]
    fn embedded_model_loads_and_answers() {
        let dir = tempfile::tempdir().expect("temp dir");
        let built = engine(dir.path());
        let reply = built
            .complete(&Request::new("Reply with the single word: ready.").with_max_tokens(16))
            .expect("the model should load and answer");
        assert!(!reply.trim().is_empty(), "empty reply");
        assert!(built.is_downloaded());
    }
}
