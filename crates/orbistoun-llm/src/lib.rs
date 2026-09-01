//! Local-first language-model access, as a generic question-and-answer service.
//!
//! # What this crate is for
//!
//! Somewhere above this crate there is a loop: run a title, read what the run reports,
//! decide what to change, change it, run again. Two of its steps are a person
//! ([THE_LOOP.md](../../../docs/THE_LOOP.md) marks them 17 and 18), and this crate is
//! the machinery that lets something else attempt them.
//!
//! It does not attempt them. **Nothing here knows what orbistoun is.** This crate has
//! no dependency on any other crate in the workspace, deliberately: the callers arrive
//! later and there will be several of them, with different jobs, and a service shaped
//! around whichever one came first is a service the rest fight. What it offers is
//! [`Request`] in and [`Reply`] out.
//!
//! # What it does on its own
//!
//! ```text
//!   Host::probe()          what is this machine
//!        |
//!   select::recommend()    the largest catalogue model that fits it
//!        |
//!   Config::seeded_for()   an ordered registry, written once and then owned by a person
//!        |
//!   Llm::ask()             first compatible + configured entry that answers
//!        |
//!   EmbeddedEngine         downloads on first use, then runs in this process
//! ```
//!
//! No setup, no server, no key. A machine that has never been configured probes
//! itself, writes a registry, fetches a model sized for it, and answers - and every
//! step of that is overridable by editing one file, because none of it is a decision
//! anybody should have to accept.
//!
//! # Three properties worth stating
//!
//! **Local outranks hosted.** A trace, a fault address and a guest's own strings are
//! this project's material. The default ladder puts this machine first and reaches a
//! hosted provider only when configured to.
//!
//! **Deterministic by default.** Temperature zero, fixed seed. The loop above measures
//! progress by changing one thing and re-running; a proposer that answers differently
//! each time makes that measurement meaningless.
//!
//! **Attributable.** A [`Reply`] carries which entry answered, which model, and what
//! was tried before it. D046 makes a run report embed its own inputs so a difference
//! between runs can be blamed on the change rather than on drift - a model is such an
//! input, and one that silently fell back from a 4B to a 0.6B has drifted.
//!
//! # What it will not do
//!
//! Fabricate. Every failure is an [`Error`] naming what went wrong; nothing here
//! returns an empty or invented reply to avoid one. A hosted refusal is reported as a
//! refusal rather than as an empty answer, because "proposed nothing" and "was not
//! allowed to propose" are different facts and only one of them is worth retrying.
//! Principle 3.
//!
//! # Provenance
//!
//! CLAUDE.md principle 1 names a model in the loop as a route to contaminated
//! provenance: a thing that has read the public internet can *recall* an answer and
//! present it as reasoning. This crate does not attempt to solve that, and should not
//! be read as having solved it. It moves bytes. Whether a proposal that came back
//! through it may be recorded as knowledge, and under what account, is a question for
//! the caller and for the knowledge vocabulary - which is where the mechanism for it
//! already lives.

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
/// Split this finely because the caller's response differs: a missing key is a person's
/// job, an unreachable endpoint is worth trying the next entry for, a refusal is
/// neither, and a protocol surprise means this crate is wrong about something.
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
    /// Carries every entry that was considered and what was wrong with it, because
    /// "no AI available" with no further detail is the least actionable message a tool
    /// can produce.
    #[error("no usable AI backend: {0}")]
    Unavailable(String),
}

/// The file a registry is stored in, beneath whatever root the caller supplies.
pub const CONFIG_FILE: &str = "llm.toml";

/// The directory models are stored in, beneath the same root.
pub const MODELS_DIR: &str = "models";

/// The service.
///
/// Holds a catalogue, a registry, a measured host, and a root to write beneath. Cheap
/// to construct - nothing is downloaded or loaded until something is asked.
#[derive(Debug)]
pub struct Llm {
    catalog: Catalog,
    config: Config,
    host: Host,
    root: PathBuf,
    /// Engines already built, by entry id.
    ///
    /// **Not an optimisation.** Without it every ask rebuilt its engine, which for the
    /// in-process one meant re-reading gigabytes of weights per question, and for the
    /// managed one would mean starting and killing a server per question. The cost was
    /// real and measured: it was most of a round in the first live experiment.
    engines: Mutex<HashMap<String, Arc<dyn Engine>>>,
}

impl Llm {
    /// Opens the service beneath `root`, configuring this machine if it never has been.
    ///
    /// `root` is supplied rather than resolved because this crate has no path policy
    /// of its own and must not acquire one: orbistoun guarantees it never writes
    /// outside its own resolved root, and a several-gigabyte download landing in an
    /// ambient user cache would break that guarantee without any error. The caller
    /// that holds the guarantee passes the directory.
    ///
    /// On a machine with no registry this probes, sizes, writes one, and says so. A
    /// registry that cannot be *written* is logged and not fatal - a read-only
    /// installation still works, it simply re-decides every time.
    ///
    /// # Errors
    ///
    /// If an existing registry is present but unreadable. That is not reseeded:
    /// somebody wrote that file, and replacing it with defaults would destroy what
    /// they meant and report success.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::open_with(root, Catalog::default(), Host::probe())
    }

    /// [`Llm::open`], with the catalogue and host supplied.
    ///
    /// Exists so the whole resolution path is testable on one machine: every decision
    /// this crate makes is a function of these two arguments.
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
                // Not fatal. A read-only installation is fully usable; it just decides
                // afresh each time, which is the same decision.
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
    /// **The order is a measurement, not a policy.** It used to be an argument - local
    /// engines first, on the reasoning that this project's material should not be posted
    /// elsewhere by default - and that is now settled by running them instead (D334).
    ///
    /// **The request is the caller's, and it matters that it is representative.** A short
    /// easy question does not discriminate - measured, and it ranked two engines equal
    /// that differ by six times in the real loop (D334).
    ///
    /// Ranked by usable words returned, with latency as a tiebreak, for the reason
    /// [`bench`](mod@bench) gives at length: ranking by speed picks the engine that answers quickly
    /// and says almost nothing, and the model's time is a rounding error beside the sweep
    /// that follows it anyway.
    ///
    /// Written to the registry, because the point of measuring is that the next run does
    /// not have to. Every entry is asked, including ones below the one that answers today,
    /// since an engine that never runs is one nobody can find out about.
    ///
    /// # Errors
    ///
    /// If the reordered registry cannot be written. A single entry failing to answer is a
    /// result rather than an error - it scores nothing and sorts last.
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

        // Applied back to front, so each `prefer` puts its entry ahead of the ones already
        // moved. Entries nobody measured - not compatible here, not configured - keep
        // their places behind everything that was.
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

    /// Replaces the registry and persists it, marking it as a person's decision.
    ///
    /// After this the registry is never machine-rewritten. That is the whole purpose
    /// of the flag: without it, re-tuning would silently revert a deliberate choice
    /// and the only symptom would be a setting that keeps coming back.
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

    /// Re-sizes this machine and replaces the registry, but only if nobody has
    /// expressed an opinion.
    ///
    /// Returns whether anything was written.
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

    /// Builds the engine for one entry.
    ///
    /// Public because a caller may want to warm a model, report on one, or drive a
    /// specific entry rather than the ladder.
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
    /// [`Error::Unavailable`] when nothing is configured for this machine, or when
    /// every configured entry failed - carrying what each one said, because a bare
    /// "no AI available" tells nobody which of a missing key, a stopped server and a
    /// failed download to go and fix.
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
            // Positional rather than named: a format string built by `concat!` is a
            // macro expansion, and `format_args!` refuses to capture from the
            // surrounding scope through one.
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

/// Opens the service beneath `root`, or reports why not.
///
/// A convenience for the common case, where a caller wants an [`Option`] and a log
/// line rather than an error to handle.
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

    /// Opening writes a registry and nothing else. No model, no download.
    ///
    /// The property behind "download on first use of a model": opening the service is
    /// something a status line does, and it must not cost gigabytes.
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
    ///
    /// Without the flag, re-tuning would silently revert a deliberate choice and the
    /// only symptom would be a setting that keeps coming back.
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

    /// With nothing configured, asking says what to do about it.
    ///
    /// A bare "no AI available" tells nobody which of a missing key, a stopped server
    /// and a failed download to go and fix, so the message names the file.
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
    ///
    /// This is the containment guarantee this crate has to honour without being able
    /// to state it: `orbistoun-paths` promises orbistoun never writes outside its own
    /// root, and several gigabytes landing in an ambient cache would break that with
    /// no error anywhere.
    #[test]
    fn everything_is_written_beneath_the_supplied_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let llm = Llm::open_with(dir.path(), Catalog::default(), host()).expect("opens");
        assert!(llm.models_dir().starts_with(dir.path()));
        assert!(llm.config_path().starts_with(dir.path()));
    }
}
