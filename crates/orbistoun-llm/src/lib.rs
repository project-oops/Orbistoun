//! Language-model access as a generic question-and-answer service: [`Request`] in, [`Reply`] out.
//!
//! Nothing here knows what orbistoun is, and the crate depends on no other workspace crate, so
//! callers with different jobs share it (D212). An unconfigured machine probes itself
//! (`Host::probe`), sizes a model (`select::recommend`), writes an ordered registry
//! (`Config::seeded_for`) and answers through the first compatible, configured entry, every
//! step overridable in one file. Requests are deterministic by default, and a [`Reply`] names
//! the entry and model that answered and what was tried first. Every failure is an
//! [`Error`]; nothing returns an empty or invented reply. Whether a proposal may be recorded as
//! knowledge, given a model can recall rather than reason, is the caller's question.

#![forbid(unsafe_code)]

pub mod bench;
pub mod catalog;
pub mod cli;
pub mod config;
pub mod embedded;
pub mod engine;
pub mod host;
pub mod online;
pub mod runtime;
pub mod select;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use catalog::Catalog;
pub use config::{Config, Integration, Kind};
pub use engine::{Ask, Attempt, Engine, Reply, Request};
pub use host::Host;
pub use select::Device;

/// Anything that can go wrong, named by which part of the world failed.
///
/// Split finely because the responses differ: a missing key is a person's job, an unreachable
/// endpoint means trying the next entry, a refusal is neither, and a protocol surprise means
/// this crate is wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The catalogue could not be read.
    #[error("the model catalogue is unusable: {0}")]
    Catalog(String),
    /// The registry could not be read, written, or made sense of.
    #[error("the AI configuration is unusable: {0}")]
    Config(String),
    /// A model's files could not be fetched.
    #[error("could not fetch a model: {0}")]
    Download(String),
    /// A model could not be loaded or run.
    #[error("could not run a model: {0}")]
    Model(String),
    /// An endpoint could not be reached.
    #[error("could not reach the endpoint: {0}")]
    Transport(String),
    /// An endpoint answered, and said no.
    #[error("the endpoint rejected the request with HTTP {status}: {body}")]
    Rejected {
        /// The HTTP status.
        status: u16,
        /// As much of the body as is worth carrying.
        body: String,
    },
    /// The model declined to answer.
    #[error("the model declined to answer (category: {0})")]
    Refused(String),
    /// A reply arrived in a shape this crate does not understand.
    #[error("the reply could not be read: {0}")]
    Protocol(String),
    /// Nothing was available to ask.
    ///
    /// Carries every entry considered and what was wrong with it.
    #[error("no usable AI backend: {0}")]
    Unavailable(String),
}

/// The file a registry is stored in, beneath whatever root the caller supplies.
pub const CONFIG_FILE: &str = "llm.toml";

/// The directory models are stored in, beneath the same root.
pub const MODELS_DIR: &str = "models";

/// The service.
///
/// Holds a catalogue, a registry, a measured host, and a root to write beneath. Cheap to
/// construct: nothing is downloaded or loaded until something is asked.
#[derive(Debug)]
pub struct Llm {
    catalog: Catalog,
    config: Config,
    host: Host,
    root: PathBuf,
    /// Engines already built, by entry id, so an ask does not reload weights or restart a server.
    engines: Mutex<HashMap<String, Arc<dyn Engine>>>,
}

impl Llm {
    /// Opens the service beneath `root`, configuring this machine if it never has been.
    ///
    /// `root` is supplied because this crate has no path policy: the caller holds the guarantee
    /// that orbistoun writes nowhere outside its resolved root. A registry that cannot be written is
    /// logged and not fatal, so a read-only installation re-decides each time.
    ///
    /// # Errors
    ///
    /// If an existing registry is present but unreadable; it is never reseeded over.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::open_with(root, Catalog::default(), Host::probe())
    }

    /// [`Llm::open`], with the catalogue and host supplied, so every decision this crate makes is
    /// testable on one machine.
    ///
    /// # Errors
    ///
    /// As [`Llm::open`].
    pub fn open_with(
        root: impl Into<PathBuf>,
        catalog: Catalog,
        host: Host,
    ) -> Result<Self, Error> {
        let root = root.into();
        let path = root.join(CONFIG_FILE);

        let mut config = if path.exists() {
            let mut existing = Config::load(&path)?;
            if existing.normalise(&catalog, &host) {
                tracing::info!(
                    path = %path.display(),
                    "an entry named a model the catalogue no longer holds; re-resolved"
                );
                let _ = existing.save(&path);
            }
            existing
        } else {
            tracing::info!(host = %host.summary(), "no AI configuration; sizing this machine");
            let seeded = Config::seeded_for(&catalog, &host);
            if let Err(e) = seeded.save(&path) {
                // Not fatal: a read-only installation decides afresh each time, and reaches the same decision.
                tracing::warn!(error = %e, "could not persist the AI configuration");
            }
            seeded
        };
        config.integrations.retain(|i| !i.id.is_empty());

        Ok(Self {
            catalog,
            config,
            host,
            root,
            engines: Mutex::new(HashMap::new()),
        })
    }

    /// The measured machine.
    #[must_use]
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// The catalogue in force.
    #[must_use]
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The registry in force.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Measures every configured entry and reorders the ladder by what came back.
    ///
    /// The order is a measurement, ranked by usable words with latency as a tiebreak (see
    /// [`bench`](mod@bench)), on the caller's representative request, since an easy question does
    /// not discriminate (D334). Every entry is asked, and the result is written to the registry.
    ///
    /// # Errors
    ///
    /// If the reordered registry cannot be written. An entry failing to answer scores nothing and
    /// sorts last.
    pub fn benchmark(
        &mut self,
        request: &Request,
        score: &dyn Fn(&str) -> usize,
    ) -> Result<Vec<bench::Measurement>, Error> {
        let ids: Vec<String> = self
            .config
            .candidates(&self.catalog, &self.host)
            .map(|i| i.id.clone())
            .collect();
        let mut measured = Vec::with_capacity(ids.len());
        for id in &ids {
            let Some(integration) = self.config.integrations.iter().find(|i| &i.id == id) else {
                continue;
            };
            match self.engine(integration) {
                Ok(engine) => measured.push(bench::measure(id, engine.as_ref(), request, score)),
                Err(e) => measured.push(bench::Measurement {
                    id: id.clone(),
                    usable: 0,
                    took: std::time::Duration::ZERO,
                    failure: Some(e.to_string()),
                }),
            }
        }
        bench::rank(&mut measured);

        // Applied back to front, so each `prefer` puts its entry ahead of those already moved. Entries
        // not measured keep their places behind.
        for measurement in measured.iter().rev() {
            self.config.prefer(&measurement.id);
        }
        self.config.save(&self.config_path())?;
        Ok(measured)
    }

    /// Puts one entry at the front of the ladder for this process only.
    ///
    /// Returns whether the id exists. Not written to the registry: see
    /// [`Config::prefer`].
    pub fn prefer(&mut self, id: &str) -> bool {
        self.config.prefer(id)
    }

    /// Where the registry is stored.
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    /// Where models are stored.
    #[must_use]
    pub fn models_dir(&self) -> PathBuf {
        self.root.join(MODELS_DIR)
    }

    /// Replaces the registry and persists it, marking it as a person's decision, which is never
    /// machine-rewritten.
    ///
    /// # Errors
    ///
    /// If the registry cannot be written.
    pub fn set_config(&mut self, mut config: Config) -> Result<(), Error> {
        config.auto = false;
        config.save(&self.config_path())?;
        self.config = config;
        Ok(())
    }

    /// Re-sizes this machine and replaces the registry, but only if nobody has expressed an
    /// opinion. Returns whether anything was written.
    ///
    /// # Errors
    ///
    /// If the registry cannot be written.
    pub fn retune(&mut self) -> Result<bool, Error> {
        if !self.config.auto {
            return Ok(false);
        }
        self.host = Host::probe();
        let seeded = Config::seeded_for(&self.catalog, &self.host);
        seeded.save(&self.config_path())?;
        self.config = seeded;
        Ok(true)
    }

    /// Whether anything at all could answer here.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.config
            .candidates(&self.catalog, &self.host)
            .next()
            .is_some()
    }

    /// Builds the engine for one entry, for a caller that warms, reports on or drives a specific
    /// entry rather than the ladder.
    ///
    /// # Errors
    ///
    /// If the entry cannot be turned into something callable.
    pub fn engine(&self, integration: &Integration) -> Result<Arc<dyn Engine>, Error> {
        if let Ok(built) = self.engines.lock() {
            if let Some(engine) = built.get(&integration.id) {
                return Ok(Arc::clone(engine));
            }
        }
        let engine = self.build_engine(integration)?;
        if let Ok(mut built) = self.engines.lock() {
            built.insert(integration.id.clone(), Arc::clone(&engine));
        }
        Ok(engine)
    }

    /// Builds an engine, ignoring what has been built before.
    fn build_engine(&self, integration: &Integration) -> Result<Arc<dyn Engine>, Error> {
        match integration.kind {
            Kind::Managed => {
                let model = self.offline_model(integration)?;
                Ok(Arc::new(runtime::ManagedEngine::start(
                    &model,
                    &self.root,
                    &self.models_dir(),
                    &self.catalog,
                )?))
            }
            Kind::Offline => {
                let model = self.offline_model(integration)?;
                Ok(Arc::new(embedded::EmbeddedEngine::new(
                    model,
                    integration.device,
                    self.models_dir(),
                )))
            }
            Kind::Online => Ok(Arc::new(online::OnlineEngine::new(
                integration,
                &self.catalog,
            )?)),
            Kind::Cli => Ok(Arc::new(cli::CliEngine::new(&integration.source)?)),
        }
    }

    /// The catalogue model an entry names.
    fn offline_model(&self, integration: &Integration) -> Result<catalog::Offline, Error> {
        self.catalog
            .offline(&integration.source)
            .ok_or_else(|| {
                Error::Config(format!(
                    "`{}` names the model `{}`, which the catalogue does not hold",
                    integration.id, integration.source
                ))
            })
            .cloned()
    }

    /// Asks, walking the ladder until something answers.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when nothing is configured for this machine or every configured entry
    /// failed, carrying what each one said.
    pub fn ask(&self, request: &Request) -> Result<Reply, Error> {
        let mut attempts: Vec<Attempt> = Vec::new();

        for integration in self.config.candidates(&self.catalog, &self.host) {
            let engine = match self.engine(integration) {
                Ok(engine) => engine,
                Err(e) => {
                    attempts.push(Attempt {
                        id: integration.id.clone(),
                        failure: Some(e.to_string()),
                    });
                    continue;
                }
            };
            tracing::debug!(engine = %engine.describe(), "asking");
            match engine.complete(request) {
                Ok(text) => {
                    attempts.push(Attempt {
                        id: integration.id.clone(),
                        failure: None,
                    });
                    return Ok(Reply {
                        text,
                        backend: integration.id.clone(),
                        model: engine.model(),
                        attempts,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        engine = %engine.describe(),
                        error = %e,
                        "backend failed; trying the next"
                    );
                    attempts.push(Attempt {
                        id: integration.id.clone(),
                        failure: Some(e.to_string()),
                    });
                }
            }
        }

        Err(Error::Unavailable(self.explain(&attempts)))
    }

    /// Why nothing answered, in one line a person can act on.
    fn explain(&self, attempts: &[Attempt]) -> String {
        if attempts.is_empty() {
            let configured = self.config.integrations.len();
            if configured == 0 {
                return format!(
                    "nothing is configured. Delete {} to have it written again",
                    self.config_path().display()
                );
            }
            // Positional: `format_args!` cannot capture from the surrounding scope through a `concat!`
            // expansion.
            return format!(
                concat!(
                    "none of the {} configured entries is usable on this machine ({}). ",
                    "A hosted entry needs a key in its environment variable; a local ",
                    "one needs an entry the catalogue still holds"
                ),
                configured,
                self.host.summary()
            );
        }
        let failures: Vec<String> = attempts
            .iter()
            .filter_map(|a| a.failure.as_ref().map(|f| format!("{}: {f}", a.id)))
            .collect();
        failures.join("; ")
    }
}

impl Ask for Llm {
    fn ask(&self, request: &Request) -> Result<Reply, Error> {
        Self::ask(self, request)
    }
}

/// Opens the service beneath `root`, or logs why not and answers `None`.
pub fn open_or_warn(root: &Path) -> Option<Llm> {
    match Llm::open(root) {
        Ok(llm) => Some(llm),
        Err(e) => {
            tracing::warn!(error = %e, "AI is unavailable");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CONFIG_FILE, Catalog, Config, Host, Llm, Request};
    use crate::host::Accelerator;

    fn host() -> Host {
        Host {
            ram_mb: Some(32_000),
            cpu_cores: Some(16),
            accelerator: Some(Accelerator {
                name: "test".to_owned(),
                vram_mb: 12_000,
            }),
            ..Host::unmeasured()
        }
    }

    /// A machine that has never been configured configures itself, and writes it down.
    #[test]
    fn a_fresh_machine_configures_itself() {
        let dir = tempfile::tempdir().expect("temp dir");
        let llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        assert!(llm.config().auto);
        assert!(!llm.config().integrations.is_empty());
        assert!(dir.path().join(CONFIG_FILE).exists());
    }

    /// Opening writes a registry and nothing else: no model, no download.
    #[test]
    fn opening_downloads_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        assert!(!llm.models_dir().exists());
        let written: Vec<_> = std::fs::read_dir(dir.path())
            .expect("readable")
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(written.len(), 1, "{written:?}");
    }

    /// A person's registry is never machine-rewritten afterwards.
    #[test]
    fn a_saved_registry_is_never_retuned() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        assert!(llm.retune().expect("retunes"), "an auto registry re-tunes");

        let mut mine = llm.config().clone();
        mine.integrations.truncate(1);
        llm.set_config(mine).expect("saves");
        assert!(!llm.config().auto);
        assert!(!llm.retune().expect("no-op"), "a saved registry re-tuned");
        assert_eq!(llm.config().integrations.len(), 1);
    }

    /// A saved registry survives being reopened.
    #[test]
    fn a_saved_registry_is_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        {
            let mut llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
            let mut mine = llm.config().clone();
            mine.integrations.truncate(1);
            llm.set_config(mine).expect("saves");
        }
        let reopened = Llm::open_with(dir.path(), Catalog::default(), host()).expect("reopens");
        assert!(!reopened.config().auto);
        assert_eq!(reopened.config().integrations.len(), 1);
    }

    /// An unreadable registry is an error, not a silent reseed.
    #[test]
    fn a_malformed_registry_is_not_quietly_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(CONFIG_FILE), "integrations = 3").expect("write");
        assert!(Llm::open_with(dir.path(), Catalog::default(), host()).is_err());
    }

    /// With nothing configured, asking names the file to fix.
    #[test]
    fn an_empty_registry_explains_itself() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        llm.set_config(Config::default()).expect("saves");
        assert!(!llm.is_available());

        let err = llm.ask(&Request::new("hello")).expect_err("nothing to ask");
        let rendered = err.to_string();
        assert!(rendered.contains(CONFIG_FILE), "{rendered}");
    }

    /// Models are stored beneath the supplied root and nowhere else.
    #[test]
    fn everything_is_written_beneath_the_supplied_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        assert!(llm.models_dir().starts_with(dir.path()));
        assert!(llm.config_path().starts_with(dir.path()));
    }
}
