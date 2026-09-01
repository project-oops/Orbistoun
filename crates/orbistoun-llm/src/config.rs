//! The registry: what is configured, in what order, and which entry wins here.
//!
//! # An ordered list, not a mode switch
//!
//! A machine is rarely "offline" or "online". It is a machine with an accelerator that
//! is sometimes busy, a model server that is sometimes running, and a key that is
//! sometimes in the environment. So the configuration is a **priority-ordered list**,
//! and the effective engine is the first entry that is both *compatible with this
//! machine* and *filled in enough to run*. Everything else stays visible, in order, so
//! that "why is it using that one" has an answer.
//!
//! Compatible and configured are separate questions on purpose. An entry with no key
//! is not broken - it is waiting, and it starts working the moment the variable is
//! set, with no edit to anything.
//!
//! # `auto` and what it protects
//!
//! A registry written by [`Config::seeded_for`] is marked `auto`, meaning "nobody has
//! opinions about this yet". Anything auto may be re-tuned as the machine changes. The
//! moment a person saves their own, it stops being auto and is never machine-written
//! again. Without that flag, tuning would silently overwrite a deliberate choice, and
//! the only symptom would be a setting that keeps coming back.
//!
//! # No migrations
//!
//! An entry naming a model the catalogue no longer holds is *invalid*, not *old*.
//! [`Config::normalise`] re-resolves it to the current recommendation rather than
//! mapping it to a successor, so a catalogue rewrite needs no table of what used to be
//! called what. Principle 10, and it is why swapping a whole model family is a data
//! edit rather than a release note.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::catalog::Catalog;
use crate::host::Host;
use crate::select::{self, Device};

/// Whether an entry runs here or elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A runtime this process downloads, starts and supervises.
    ///
    /// The accelerator path. It needs no build feature and no vendor toolkit, so it is
    /// the only local option that reaches a GPU on a machine that was handed a binary.
    Managed,
    /// Downloaded once, then run in this process.
    Offline,
    /// An HTTP endpoint - on this machine, on the network, or hosted.
    Online,
    /// A command on this machine that already holds someone's session.
    ///
    /// **The only capable model that costs nothing to set up.** No key, no download and
    /// no accelerator - an installed coding assistant is already signed in, and this
    /// borrows that. It runs a subprocess rather than making a request, which is why it
    /// is its own kind rather than an [`Kind::Online`] entry with an odd endpoint.
    ///
    /// It answers over the network on somebody else's account, so it ranks with the
    /// hosted providers rather than with the local ones (D333).
    Cli,
}

/// Environment variable consulted for any entry whose provider variable is unset.
///
/// Useful for a custom endpoint that needs a key but is not one of the named
/// providers. Checked last, so a provider-specific variable always wins.
///
/// Named by `orbistoun-env` rather than here, so the one list of what this project reads
/// stays one list - the same reason `orbistoun-paths` names its two that way (D288).
pub const ENV_FALLBACK_KEY: &str = orbistoun_env::LLM_API_KEY.name;

/// One configured backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integration {
    /// Unique within the registry, and what a report names.
    pub id: String,
    /// What to call it in a list.
    pub name: String,
    /// Where it runs.
    pub kind: Kind,
    /// The catalogue entry this rests on - an `[[offline]]` id or an `[[online]]` id.
    pub source: String,
    /// Online only: the model string, overriding the provider's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Online only: the endpoint, overriding the provider's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Online only: a key written here rather than read from the environment.
    ///
    /// Supported because some setups have nowhere else to put one, and left empty by
    /// everything this crate writes. A key in a config file is a key in a backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Offline only: which pool the weights should live in.
    #[serde(default = "cpu")]
    pub device: Device,
}

/// Whether an endpoint is on this machine.
///
/// Decides ordering, not correctness, so a hostname that resolves to loopback without
/// looking like it is simply ranked lower - which costs a place in a list, not a
/// failure. Resolving names here would mean a DNS lookup to render a settings page.
fn is_loopback(endpoint: &str) -> bool {
    let authority = endpoint
        .split("//")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or_default();
    // A bracketed literal is split on `]`, not on `:` - an IPv6 address is mostly
    // colons, so the port separator cannot be found by looking for one.
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn cpu() -> Device {
    Device::Cpu
}

impl Integration {
    /// Can this machine run it at all?
    ///
    /// A GPU entry on a machine with no accelerator is *incompatible*, not
    /// misconfigured - there is nothing to fix, and it becomes usable if hardware
    /// appears. An online entry is always compatible, because whether the endpoint
    /// answers is a runtime question and pretending to know it here would only move
    /// the failure somewhere less informative.
    ///
    /// **Two conditions, not one.** An accelerator entry needs hardware *and* a build
    /// that can address it. Checking only the hardware is not a near miss - it silently
    /// sizes a model against video memory the process will never touch, then runs it on
    /// the CPU. That happened: a sixteen-gigabyte card selected a four-billion-parameter
    /// model, which then ran at CPU speed with the warning going to a subscriber nobody
    /// had attached.
    #[must_use]
    pub fn compatible(&self, host: &Host) -> bool {
        match self.kind {
            // Only whether a prebuilt runtime exists for this platform. Not whether
            // there is a GPU: the runtime carries processor backends too and picks a
            // device if it finds one, so it is the better local engine either way.
            Kind::Managed => crate::runtime::available(),
            Kind::Offline => {
                self.device != Device::Gpu
                    || (host.accelerator.is_some() && crate::embedded::accelerator_supported())
            }
            Kind::Online => true,
            // Installed or not, which is knowable without running anything. An entry for
            // a command nobody has would otherwise sit in the ladder answering nothing.
            Kind::Cli => crate::cli::CliEngine::available(&self.source),
        }
    }

    /// Is it filled in enough to try?
    ///
    /// Not "will it work" - that needs the network, or a multi-gigabyte download, and
    /// answering it here would mean doing both to render a list.
    #[must_use]
    pub fn configured(&self, catalog: &Catalog) -> bool {
        match self.kind {
            Kind::Managed | Kind::Offline => catalog.offline(&self.source).is_some(),
            Kind::Online => {
                let Some(provider) = catalog.online(&self.source) else {
                    return self.endpoint.is_some();
                };
                provider.key_env.is_empty() || self.key(catalog).is_some()
            }
            // Nothing to fill in. That is the entire appeal of it: there is no key to
            // paste and no model to fetch, so configured and installed are the same
            // question, and `compatible` already asked it.
            Kind::Cli => true,
        }
    }

    /// The key to use, if there is one.
    ///
    /// Explicit value first, then the provider's own variable, then the generic one.
    /// An empty string counts as absent throughout: an exported-but-empty variable is
    /// how a shell says "unset" more often than it is a deliberate empty key.
    pub fn key(&self, catalog: &Catalog) -> Option<String> {
        let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
        if let Some(key) = self.api_key.clone().and_then(non_empty) {
            return Some(key);
        }
        if let Some(provider) = catalog.online(&self.source) {
            if !provider.key_env.is_empty() {
                if let Some(key) = std::env::var(&provider.key_env).ok().and_then(non_empty) {
                    return Some(key);
                }
            }
        }
        std::env::var(ENV_FALLBACK_KEY).ok().and_then(non_empty)
    }
}

/// A machine's ordered registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Priority order. First compatible and configured entry wins.
    #[serde(default)]
    pub integrations: Vec<Integration>,
    /// True while nobody has expressed an opinion; see the module documentation.
    #[serde(default)]
    pub auto: bool,
}

impl Config {
    /// The registry a fresh machine gets, sized to it.
    ///
    /// **A starting guess, not a ranking.** `Llm::benchmark` measures every entry and
    /// reorders by what came back, so this only has to be sane before anyone has run it:
    /// an accelerator here, then an installed command, then a model server somebody is
    /// already running, then this machine's CPU, then the hosted providers.
    ///
    /// It used to be ordered by an argument - local before hosted, because a trace, a
    /// fault address and a guest's own strings are this project's material and the default
    /// should not be to post them elsewhere. That is **dropped** (D334). Two things
    /// undermined it: the only consumer sends no guest material - the prompt is library
    /// names, confirmed vendor names and English words - and measuring the engines put the
    /// argument's preferred order the wrong way round, twelve usable words against two.
    #[must_use]
    pub fn seeded_for(catalog: &Catalog, host: &Host) -> Self {
        let mut integrations = Vec::new();

        if let Some(choice) = select::recommend(catalog, host) {
            // First, and on every platform that has one. It downloads its own
            // accelerator backend, so unlike everything below it this reaches a GPU on a
            // machine that was handed a binary and told to run it.
            if crate::runtime::available() {
                integrations.push(Integration {
                    id: "managed".to_owned(),
                    name: "Managed runtime (GPU)".to_owned(),
                    kind: Kind::Managed,
                    source: choice.model.id.clone(),
                    model: None,
                    endpoint: None,
                    api_key: None,
                    device: Device::Gpu,
                });
            }
            // **No in-process accelerator entry.** There used to be one, behind a build
            // feature, and it is gone with the feature: it could never be compatible, so
            // seeding it would put an entry that can never answer above one that can -
            // and every reader of the list would assume it was the one answering.
            //
            // The CPU entry is sized independently of the accelerator pick above, which
            // may be far too large to live in system memory. This is what runs when the
            // runtime cannot be fetched at all.
            let on_cpu = select::recommend(
                catalog,
                &Host {
                    accelerator: None,
                    ..host.clone()
                },
            );
            integrations.push(Integration {
                id: "local-cpu".to_owned(),
                name: "On-device CPU".to_owned(),
                kind: Kind::Offline,
                source: on_cpu.map_or_else(|| choice.model.id.clone(), |c| c.model.id.clone()),
                model: None,
                endpoint: None,
                api_key: None,
                device: Device::Cpu,
            });
        }

        // A model server on this machine goes **above** the in-process CPU engine.
        //
        // "Local first" was the ordering principle and it is not fine-grained enough:
        // both of these are local, and one of them is an order of magnitude faster
        // because a server like Ollama ships its own accelerator runtime and does not
        // need this build to have been compiled with one. Ranking an in-process CPU
        // model above it means a machine with a working GPU path never takes it.
        //
        // Costs a refused connection when nothing is listening, which on localhost is
        // immediate, and `Reply::fell_back` records that it happened.
        let (local_servers, hosted): (Vec<_>, Vec<_>) = catalog
            .online
            .iter()
            .partition(|provider| is_loopback(&provider.endpoint));
        let entry = |provider: &crate::catalog::Online| Integration {
            id: provider.id.clone(),
            name: provider.label.clone(),
            kind: Kind::Online,
            source: provider.id.clone(),
            model: None,
            endpoint: None,
            api_key: None,
            device: Device::Cpu,
        };
        let cpu_entry = integrations.pop();

        // High, because on the one machine this has been measured on it returned twelve
        // usable words to a local model's two, and it needs neither a key nor a download.
        // Only a starting guess - the benchmark settles it (D334).
        if crate::cli::CliEngine::available(crate::cli::CLAUDE_CODE) {
            integrations.push(Integration {
                id: crate::cli::CLAUDE_CODE.to_owned(),
                name: "Claude Code (installed, no key)".to_owned(),
                kind: Kind::Cli,
                source: crate::cli::CLAUDE_CODE.to_owned(),
                model: None,
                endpoint: None,
                api_key: None,
                device: Device::Cpu,
            });
        }
        integrations.extend(local_servers.into_iter().map(entry));
        integrations.extend(cpu_entry);
        integrations.extend(hosted.into_iter().map(entry));

        Self {
            integrations,
            auto: true,
        }
    }

    /// Re-resolves entries naming a model the catalogue no longer holds.
    ///
    /// Returns true if anything changed, so a caller can persist the repair. Not an
    /// alias table: unknown means invalid, and invalid resolves to whatever this
    /// machine should be running now.
    pub fn normalise(&mut self, catalog: &Catalog, host: &Host) -> bool {
        let mut changed = false;
        for integration in &mut self.integrations {
            if !matches!(integration.kind, Kind::Managed | Kind::Offline)
                || catalog.offline(&integration.source).is_some()
            {
                continue;
            }
            let sized_for = Host {
                accelerator: if integration.device == Device::Gpu {
                    host.accelerator.clone()
                } else {
                    None
                },
                ..host.clone()
            };
            if let Some(choice) = select::recommend(catalog, &sized_for) {
                integration.source.clone_from(&choice.model.id);
                changed = true;
            }
        }
        changed
    }

    /// The entries that could answer here, best first.
    pub fn candidates<'a>(
        &'a self,
        catalog: &'a Catalog,
        host: &'a Host,
    ) -> impl Iterator<Item = &'a Integration> {
        self.integrations
            .iter()
            .filter(move |i| i.compatible(host) && i.configured(catalog))
    }

    /// Moves one entry to the front, so it answers if it can.
    ///
    /// Returns whether the id was found, so a caller can tell "asked for something that is
    /// not there" from "asked for something that is". Silently doing nothing would let a
    /// person believe they had chosen an engine while a different one answered - which for
    /// this registry is a question about where their prompt went.
    ///
    /// **Preference, not pinning.** The entry only leads; everything else keeps its order
    /// behind it, so a chosen engine that cannot answer still falls through to the ladder
    /// rather than failing the run. And it is not persisted: choosing once should not
    /// quietly rewrite what every later run does.
    pub fn prefer(&mut self, id: &str) -> bool {
        let Some(at) = self.integrations.iter().position(|i| i.id == id) else {
            return false;
        };
        let chosen = self.integrations.remove(at);
        self.integrations.insert(0, chosen);
        true
    }

    /// The entry that would answer here, for a status line.
    pub fn active<'a>(&'a self, catalog: &'a Catalog, host: &'a Host) -> Option<&'a Integration> {
        self.candidates(catalog, host).next()
    }

    /// Reads a registry.
    ///
    /// # Errors
    ///
    /// If the file cannot be read or is not valid TOML in this shape. A malformed
    /// registry is an error rather than a silent reseed: somebody wrote that file, and
    /// overwriting it with defaults would destroy the thing they were trying to say.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("reading {}: {e}", path.display())))?;
        toml::from_str(&text).map_err(|e| Error::Config(format!("parsing {}: {e}", path.display())))
    }

    /// Writes a registry, creating the directory if it is missing.
    ///
    /// # Errors
    ///
    /// If the directory cannot be created or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Config(format!("creating {}: {e}", parent.display())))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialising the registry: {e}")))?;
        std::fs::write(path, text)
            .map_err(|e| Error::Config(format!("writing {}: {e}", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, Integration, Kind, is_loopback};
    use crate::catalog::Catalog;
    use crate::host::{Accelerator, Host};
    use crate::select::Device;

    fn gpu_host() -> Host {
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

    /// A machine with no accelerator gets no *in-process* accelerator entry.
    ///
    /// Listing one would put an entry that can never run at the top of the ladder,
    /// where every reader would assume it is what answers.
    ///
    /// Scoped to `Offline` deliberately. The managed runtime is an accelerator entry
    /// too and is **not** excluded here, because it does not depend on this machine
    /// having been probed successfully - it downloads its own backend and then asks the
    /// runtime which devices exist, which sees vendors `nvidia-smi` never will.
    #[test]
    fn a_machine_with_no_accelerator_gets_no_in_process_gpu_entry() {
        let catalog = Catalog::default();
        let config = Config::seeded_for(&catalog, &Host::unmeasured());
        assert!(
            !config
                .integrations
                .iter()
                .any(|i| i.kind == Kind::Offline && i.device == Device::Gpu)
        );
    }

    /// The CPU entry is sized for the CPU, not inherited from the accelerator pick.
    ///
    /// This is the bug the separate sizing call exists to prevent: a machine with 12 GB
    /// of VRAM and 4 GB of free system memory would otherwise list a CPU fallback that
    /// cannot load, and it would only find out after the download.
    #[test]
    fn the_cpu_entry_is_sized_independently_of_the_accelerator() {
        let catalog = Catalog::default();
        let host = Host {
            ram_mb: Some(4_000),
            ..gpu_host()
        };
        let config = Config::seeded_for(&catalog, &host);
        let cpu = config
            .integrations
            .iter()
            .find(|i| i.id == "local-cpu")
            .expect("a cpu entry");
        let model = catalog.offline(&cpu.source).expect("in catalogue");
        assert!(model.min_ram_mb <= 4_000, "{} does not fit", model.id);
    }

    /// Nothing hosted outranks anything on this machine.
    ///
    /// Not a performance claim. A trace, a fault address and a guest's own strings are
    /// this project's material, and the default must not be to post them elsewhere.
    #[test]
    fn nothing_hosted_outranks_anything_local() {
        let catalog = Catalog::default();
        let config = Config::seeded_for(&catalog, &gpu_host());
        let hosted_from = config
            .integrations
            .iter()
            .position(|i| i.kind == Kind::Online && !is_loopback(hosted_endpoint(&catalog, i)))
            .expect("some hosted entry");
        let last_local = config
            .integrations
            .iter()
            .rposition(|i| i.kind == Kind::Offline || is_loopback(hosted_endpoint(&catalog, i)))
            .expect("some local entry");
        assert!(last_local < hosted_from, "{:?}", config.integrations);
    }

    /// **A preferred entry leads, and the rest keep their order behind it.**
    ///
    /// Order is the whole of the ladder's meaning, so moving one entry must not shuffle
    /// the others - a chosen engine that cannot answer should fall through to exactly the
    /// sequence that would have run without the choice.
    #[test]
    fn a_preferred_entry_leads_and_the_rest_keep_their_order() {
        let mut config = Config {
            integrations: ["a", "b", "c", "d"].map(named).to_vec(),
            auto: true,
        };
        assert!(config.prefer("c"));
        let order: Vec<&str> = config.integrations.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b", "d"]);
    }

    /// **Asking for an entry that is not there says so.**
    ///
    /// Returning quietly would let somebody believe they had chosen where their prompt
    /// goes while something else answered, which is the one thing this registry is for.
    #[test]
    fn preferring_something_absent_reports_it() {
        let mut config = Config {
            integrations: ["a"].map(named).to_vec(),
            auto: true,
        };
        assert!(!config.prefer("nothing-like-this"));
        assert_eq!(
            config.integrations.len(),
            1,
            "the registry was disturbed anyway"
        );
    }

    /// One entry, by id, for the ordering tests.
    fn named(id: &str) -> Integration {
        Integration {
            id: id.to_owned(),
            name: id.to_owned(),
            kind: Kind::Online,
            source: id.to_owned(),
            model: None,
            endpoint: None,
            api_key: None,
            device: Device::Cpu,
        }
    }

    /// The managed runtime leads the ladder, on any platform that has one.
    ///
    /// It is the only local entry that reaches an accelerator without a build feature
    /// or a vendor toolkit, so anything ahead of it would take a machine with a GPU and
    /// run it on the processor - which is what this whole arrangement exists to stop.
    #[test]
    fn the_managed_runtime_leads_the_ladder() {
        if !crate::runtime::available() {
            return;
        }
        let catalog = Catalog::default();
        let host = gpu_host();
        let config = Config::seeded_for(&catalog, &host);
        assert_eq!(
            config.integrations.first().map(|i| i.kind),
            Some(Kind::Managed),
            "{:?}",
            config.integrations
        );
        assert_eq!(
            config.active(&catalog, &host).map(|i| i.kind),
            Some(Kind::Managed)
        );
    }

    /// A model server on this machine outranks the in-process CPU engine.
    ///
    /// The finer-grained rule that replaced "local before hosted", which was too coarse
    /// to be useful: both are local, and one of them ships its own accelerator runtime.
    /// A machine whose only working GPU path is a local server would otherwise never
    /// take it, because an in-process CPU model always answers first.
    #[test]
    fn a_local_server_outranks_the_in_process_cpu_engine() {
        let catalog = Catalog::default();
        let config = Config::seeded_for(&catalog, &gpu_host());
        let server = config
            .integrations
            .iter()
            .position(|i| i.id == "ollama")
            .expect("the local server entry");
        let cpu = config
            .integrations
            .iter()
            .position(|i| i.id == "local-cpu")
            .expect("the in-process entry");
        assert!(server < cpu, "{:?}", config.integrations);
    }

    /// An endpoint on this machine is recognised as one; anything else is not.
    #[test]
    fn loopback_is_recognised() {
        for local in [
            "http://localhost:11434/v1/chat/completions",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:1234/v1",
        ] {
            assert!(is_loopback(local), "{local}");
        }
        for remote in [
            "https://api.anthropic.com/v1/messages",
            "http://192.168.1.20:11434/v1",
            "https://localhost.example.com/v1",
        ] {
            assert!(!is_loopback(remote), "{remote}");
        }
    }

    /// The endpoint an entry would actually use.
    fn hosted_endpoint<'a>(catalog: &'a Catalog, entry: &'a Integration) -> &'a str {
        entry
            .endpoint
            .as_deref()
            .or_else(|| catalog.online(&entry.source).map(|p| p.endpoint.as_str()))
            .unwrap_or_default()
    }

    /// An accelerator entry needs a build that can address one, not just the hardware.
    ///
    /// The failure this prevents is not theoretical - it is what the first real run did.
    /// A sixteen-gigabyte card made the GPU entry look compatible, the selector sized a
    /// four-billion-parameter model against **video memory**, and the process then ran it
    /// on the CPU because the build has no accelerator backend compiled in. Nothing said
    /// so: the warning goes to a tracing subscriber, and nothing had attached one.
    #[test]
    fn a_gpu_entry_needs_a_build_that_can_use_a_gpu() {
        let entry = Integration {
            id: "local-gpu".to_owned(),
            name: "x".to_owned(),
            kind: Kind::Offline,
            source: "qwen3-0.6b".to_owned(),
            model: None,
            endpoint: None,
            api_key: None,
            device: Device::Gpu,
        };
        assert_eq!(
            entry.compatible(&gpu_host()),
            crate::embedded::accelerator_supported(),
            "compatibility must track the build, not only the hardware"
        );
        // And on the plain build - the one everybody gets - the ladder must fall past
        // the in-process accelerator entry. It may well land on the managed runtime,
        // which reaches a GPU without any of this, so the assertion is about which
        // mechanism answers rather than about whether a GPU is reached at all.
        if !crate::embedded::accelerator_supported() {
            let catalog = Catalog::default();
            let host = gpu_host();
            let config = Config::seeded_for(&catalog, &host);
            let active = config.active(&catalog, &host).expect("something usable");
            assert!(
                active.kind != Kind::Offline || active.device != Device::Gpu,
                "{}",
                active.id
            );
        }
    }

    /// A hosted entry with no key is unconfigured, not incompatible.
    ///
    /// The distinction is what lets it sit in the list and start working when the
    /// variable is set, rather than being hidden as unusable on this machine.
    #[test]
    fn a_keyless_hosted_entry_is_unconfigured_but_compatible() {
        let catalog = Catalog::default();
        let entry = Integration {
            id: "anthropic".to_owned(),
            name: "x".to_owned(),
            kind: Kind::Online,
            source: "anthropic".to_owned(),
            model: None,
            endpoint: None,
            api_key: None,
            device: Device::Cpu,
        };
        assert!(entry.compatible(&Host::unmeasured()));
        // Only assert the unconfigured half when the ambient environment has no key,
        // so this passes on a developer machine that happens to have one exported.
        let has_key = entry.key(&catalog).is_some();
        assert_eq!(entry.configured(&catalog), has_key);
    }

    /// A local model server needs no key and is configured immediately.
    #[test]
    fn a_keyless_provider_is_configured_without_one() {
        let catalog = Catalog::default();
        let entry = Integration {
            id: "ollama".to_owned(),
            name: "x".to_owned(),
            kind: Kind::Online,
            source: "ollama".to_owned(),
            model: None,
            endpoint: None,
            api_key: None,
            device: Device::Cpu,
        };
        assert!(entry.configured(&catalog));
    }

    /// An entry naming a model that no longer exists is repaired, not aliased.
    ///
    /// The property that makes a catalogue rewrite free: no table of former names has
    /// to be kept, because "unknown" and "invalid" are the same thing.
    #[test]
    fn an_unknown_model_is_re_resolved() {
        let catalog = Catalog::default();
        let host = gpu_host();
        let mut config = Config::seeded_for(&catalog, &host);
        config.integrations[0].source = "qwen1.5-42b".to_owned();
        assert!(config.normalise(&catalog, &host));
        assert!(catalog.offline(&config.integrations[0].source).is_some());
        // And a second pass changes nothing, or the repair would rewrite the file on
        // every load.
        assert!(!config.normalise(&catalog, &host));
    }

    /// A valid choice is left alone by the repair.
    ///
    /// Somebody hand-picked the 8B model; normalising must not size it back down just
    /// because the selector would have chosen differently.
    #[test]
    fn a_hand_picked_model_survives_normalising() {
        let catalog = Catalog::default();
        let host = gpu_host();
        let mut config = Config::seeded_for(&catalog, &host);
        config.integrations[0].source = "qwen3-8b".to_owned();
        assert!(!config.normalise(&catalog, &host));
        assert_eq!(config.integrations[0].source, "qwen3-8b");
    }

    /// A seeded registry is marked auto; a round trip through disk keeps the flag.
    #[test]
    fn a_seeded_registry_is_auto_and_survives_a_round_trip() {
        let catalog = Catalog::default();
        let config = Config::seeded_for(&catalog, &gpu_host());
        assert!(config.auto);

        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("llm.toml");
        config.save(&path).expect("save");
        let read = Config::load(&path).expect("load");
        assert!(read.auto);
        assert_eq!(read.integrations.len(), config.integrations.len());
    }

    /// A malformed registry is an error rather than a silent reseed.
    ///
    /// Somebody wrote that file. Replacing it with defaults would destroy what they
    /// were trying to say and report success.
    #[test]
    fn a_malformed_registry_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("llm.toml");
        std::fs::write(&path, "integrations = \"not a list\"").expect("write");
        assert!(Config::load(&path).is_err());
    }

    /// A saved registry never contains a key this crate put there.
    #[test]
    fn a_seeded_registry_writes_no_keys() {
        let catalog = Catalog::default();
        let config = Config::seeded_for(&catalog, &gpu_host());
        let text = toml::to_string_pretty(&config).expect("serialise");
        assert!(!text.contains("api_key"), "{text}");
    }
}
