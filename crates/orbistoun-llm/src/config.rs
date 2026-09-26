//! The registry: what is configured, in what order, and which entry wins here.
//!
//! A priority-ordered list rather than an offline/online switch: the effective engine is the
//! first entry both compatible with this machine and configured enough to run, and the rest stay
//! visible in order. An entry with no key is waiting, not broken. A registry written by
//! [`Config::seeded_for`] is marked `auto` and may be re-tuned as the machine changes; once a
//! person saves their own it is never machine-written again. An entry naming a model the
//! catalogue no longer holds is invalid, and [`Config::normalise`] re-resolves it to the current
//! recommendation, so no table of former names is kept.

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
    /// The accelerator path: no build feature or vendor toolkit, so it reaches a GPU from a binary
    /// somebody was handed.
    Managed,
    /// Downloaded once, then run in this process.
    Offline,
    /// An HTTP endpoint - on this machine, on the network, or hosted.
    Online,
    /// A command on this machine that already holds someone's session.
    ///
    /// Needs no key, download or accelerator. It runs a subprocess rather than making a request,
    /// hence its own kind, and it answers over the network on somebody else's account, so it ranks
    /// with the hosted providers (D333).
    Cli,
}

/// Environment variable consulted for any entry whose provider variable is unset.
///
/// For a custom endpoint that needs a key. Checked last, so a provider-specific variable wins.
/// Named by `orbistoun-env`, which holds the one list of variables this project reads (D288).
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
    /// For setups with nowhere else to put one; this crate never writes it, since a key in a config
    /// file is a key in a backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Offline only: which pool the weights should live in.
    #[serde(default = "cpu")]
    pub device: Device,
}

/// Whether an endpoint is on this machine.
///
/// Decides ordering, not correctness, so no DNS lookup is made: a hostname that resolves to
/// loopback is merely ranked lower.
fn is_loopback(endpoint: &str) -> bool {
    let authority = endpoint
        .split("//")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or_default();
    // A bracketed IPv6 literal is split on `]`, since the address itself is mostly colons.
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
    /// A GPU entry with no accelerator is incompatible, not misconfigured; an online entry is always
    /// compatible, since whether it answers is a runtime question. An accelerator entry needs the
    /// hardware and a build that can address it, or a model is sized against video memory and run on
    /// the CPU.
    #[must_use]
    pub fn compatible(&self, host: &Host) -> bool {
        match self.kind {
            // Only whether a prebuilt runtime exists here: it carries processor backends too, so it is the
            // better local engine with or without a GPU.
            Kind::Managed => crate::runtime::available(),
            Kind::Offline => {
                self.device != Device::Gpu
                    || (host.accelerator.is_some() && crate::embedded::accelerator_supported())
            }
            Kind::Online => true,
            // Installed or not, knowable without running anything.
            Kind::Cli => crate::cli::CliEngine::available(&self.source),
        }
    }

    /// Is it filled in enough to try? Not whether it will work, which needs the network or a large
    /// download.
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
            // Nothing to fill in: no key and no model, so `compatible` already answered.
            Kind::Cli => true,
        }
    }

    /// The key to use, if there is one.
    ///
    /// Explicit value first, then the provider's own variable, then the generic one. An empty string
    /// counts as absent, since an exported-but-empty variable usually means unset.
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
    /// A starting guess that `Llm::benchmark` reorders by measurement (D334): the managed runtime,
    /// then an installed command, then a model server already running here, then this machine's CPU,
    /// then the hosted providers.
    #[must_use]
    pub fn seeded_for(catalog: &Catalog, host: &Host) -> Self {
        let mut integrations = Vec::new();

        if let Some(choice) = select::recommend(catalog, host) {
            // First on every platform that has one: it downloads its own accelerator backend.
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
            // No in-process accelerator entry: it could never be compatible, and would sit above entries
            // that can answer. The CPU entry is sized independently of the accelerator pick, which may not
            // fit in system memory; it runs when the runtime cannot be fetched.
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

        // A model server on this machine goes above the in-process CPU engine: both are local, but a
        // server ships its own accelerator runtime. A refused connection on localhost is immediate, and
        // `Reply::fell_back` records it.
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

        // High because it needs neither a key nor a download; the benchmark settles its place.
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
    /// Returns true if anything changed, so a caller can persist the repair. Unknown means invalid,
    /// and invalid resolves to what this machine should run now.
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
    /// Returns whether the id was found, so a person never believes they chose an engine while
    /// another answered. The entry only leads: the rest keep their order behind it, so a chosen
    /// engine that cannot answer falls through. Not persisted.
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
    /// If the file cannot be read or is not valid TOML in this shape. A malformed registry is an
    /// error rather than a silent reseed that would overwrite what somebody wrote.
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

    /// A machine with no accelerator gets no in-process accelerator entry.
    ///
    /// Scoped to `Offline`: the managed runtime is not excluded, since it asks its own runtime
    /// which devices exist.
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

    /// The CPU entry is sized for system memory, not inherited from the accelerator pick.
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

    /// In the seeded order, nothing hosted outranks an entry on this machine.
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

    /// A preferred entry leads, and the rest keep their order behind it.
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

    /// Asking for an entry that is not there says so.
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

    /// The managed runtime leads the ladder on any platform that has one.
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
        // On the plain build the ladder falls past the in-process accelerator entry; it may land on
        // the managed runtime, so this asserts which mechanism answers, not whether a GPU is reached.
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

    /// A hosted entry with no key is unconfigured but compatible, so it starts working when the
    /// variable is set.
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
        // The unconfigured half is asserted only when the environment has no key exported.
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

    /// An entry naming a model that no longer exists is re-resolved, not aliased.
    #[test]
    fn an_unknown_model_is_re_resolved() {
        let catalog = Catalog::default();
        let host = gpu_host();
        let mut config = Config::seeded_for(&catalog, &host);
        config.integrations[0].source = "qwen1.5-42b".to_owned();
        assert!(config.normalise(&catalog, &host));
        assert!(catalog.offline(&config.integrations[0].source).is_some());
        // A second pass changes nothing, or the repair would rewrite the file on every load.
        assert!(!config.normalise(&catalog, &host));
    }

    /// A valid hand-picked model survives normalising.
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
