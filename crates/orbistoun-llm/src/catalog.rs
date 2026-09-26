//! The model catalogue, read from data.
//!
//! Everything needed to choose a model lives here and nothing about running one, so
//! [`crate::select`] is testable with no network, accelerator or model on disk. The one
//! constrained field is [`Offline::arch`], which names a loader: a value with no loader is
//! refused by name at load time, because weights read through the wrong family's loader
//! produce plausible output rather than an error.

use serde::Deserialize;

use crate::Error;

/// The catalogue shipped with this crate.
///
/// Embedded with `include_str!` so a fresh machine needs no files; a caller wanting another
/// passes it to [`Catalog::parse`].
pub const DEFAULT_CATALOG: &str = include_str!("../data/models.toml");

/// Which in-process loader reads a model's weights.
///
/// An enum, because the valid values are exactly the loaders this crate's code has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    /// `candle_transformers::models::quantized_qwen3`.
    Qwen3,
    /// `candle_transformers::models::quantized_qwen2`.
    Qwen2,
}

/// How an endpoint expects to be spoken to.
///
/// Named by protocol, since the protocol is what decides the bytes written to the socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wire {
    /// `POST /chat/completions`, bearer token, `choices[0].message.content`.
    OpenAi,
    /// The Messages API: `POST /v1/messages`, `x-api-key`, `content[]` blocks.
    Anthropic,
}

/// A model this crate can download and run in-process.
#[derive(Debug, Clone, Deserialize)]
pub struct Offline {
    /// Stable identifier, and what a config file stores.
    pub id: String,
    /// Short human name.
    pub label: String,
    /// One line on what it is for. May be empty.
    #[serde(default)]
    pub note: String,
    /// Which loader reads its weights.
    pub arch: Arch,
    /// The repository holding the quantised weights.
    pub repo: String,
    /// The weights file within that repository.
    pub file: String,
    /// The repository holding `tokenizer.json`, which is usually not the same one.
    pub tokenizer_repo: String,
    /// Download size, for telling somebody what is about to happen.
    pub download_mb: u32,
    /// System memory needed to run it on CPU.
    pub min_ram_mb: u32,
    /// Accelerator memory needed to run it on a GPU.
    pub min_vram_mb: u32,
    /// May the selector choose this without being asked.
    #[serde(default)]
    pub auto: bool,
    /// The single entry chosen when the host cannot be measured.
    #[serde(default)]
    pub default: bool,
}

/// An endpoint somebody else runs.
#[derive(Debug, Clone, Deserialize)]
pub struct Online {
    /// Stable identifier, and what a config file stores.
    pub id: String,
    /// Short human name.
    pub label: String,
    /// One line of context. May be empty.
    #[serde(default)]
    pub note: String,
    /// How to speak to it.
    pub wire: Wire,
    /// Where to send the request.
    pub endpoint: String,
    /// The model string used when a config entry names none.
    pub default_model: String,
    /// Environment variable holding the key. Empty means no key is needed, as for a local model
    /// server.
    #[serde(default)]
    pub key_env: String,
    /// Where a person gets a key. Shown, never fetched.
    #[serde(default)]
    pub key_url: String,
}

/// Everything that can be chosen between.
#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    /// Models run in this process.
    #[serde(default)]
    pub offline: Vec<Offline>,
    /// Endpoints run elsewhere.
    #[serde(default)]
    pub online: Vec<Online>,
}

impl Default for Catalog {
    fn default() -> Self {
        // The shipped catalogue is a test-covered constant, so a parse failure is a broken build.
        Self::parse(DEFAULT_CATALOG).expect("the shipped catalogue parses")
    }
}

impl Catalog {
    /// Reads a catalogue.
    ///
    /// # Errors
    ///
    /// If the text is not valid TOML in this shape, or declares no models at all: an empty
    /// catalogue would read as "this machine can run nothing".
    pub fn parse(text: &str) -> Result<Self, Error> {
        let catalog: Self = toml::from_str(text).map_err(|e| Error::Catalog(e.to_string()))?;
        if catalog.offline.is_empty() && catalog.online.is_empty() {
            return Err(Error::Catalog(
                "the catalogue declares no models and no endpoints".to_owned(),
            ));
        }
        Ok(catalog)
    }

    /// The offline model with this id.
    pub fn offline(&self, id: &str) -> Option<&Offline> {
        self.offline.iter().find(|m| m.id == id)
    }

    /// The hosted endpoint with this id.
    pub fn online(&self, id: &str) -> Option<&Online> {
        self.online.iter().find(|p| p.id == id)
    }

    /// The smallest model the selector is allowed to choose: what runs anywhere.
    ///
    /// Falls back to the first entry if nothing is marked `auto`, so a hand-written catalogue
    /// stays usable.
    pub fn smallest_auto(&self) -> Option<&Offline> {
        self.offline
            .iter()
            .filter(|m| m.auto)
            .min_by_key(|m| m.min_ram_mb)
            .or_else(|| self.offline.first())
    }

    /// The entry to use when the host reports nothing measurable.
    ///
    /// The `default` entry rather than the largest: an over-sized pick fails at load, after a
    /// multi-gigabyte download.
    pub fn balanced_default(&self) -> Option<&Offline> {
        self.offline
            .iter()
            .find(|m| m.default)
            .or_else(|| self.smallest_auto())
    }
}

#[cfg(test)]
mod tests {
    use super::{Arch, Catalog, DEFAULT_CATALOG, Wire};

    /// The shipped catalogue parses, so `Catalog::default` cannot panic.
    #[test]
    fn the_shipped_catalogue_parses() {
        let catalog = Catalog::parse(DEFAULT_CATALOG).expect("parses");
        assert!(!catalog.offline.is_empty());
        assert!(!catalog.online.is_empty());
    }

    /// Exactly one offline model is the unmeasured-host default.
    ///
    /// Two would make the choice depend on file order; none would put every unmeasurable machine
    /// on the smallest model.
    #[test]
    fn exactly_one_model_is_the_default() {
        let catalog = Catalog::default();
        let defaults: Vec<_> = catalog.offline.iter().filter(|m| m.default).collect();
        assert_eq!(defaults.len(), 1, "{defaults:?}");
    }

    /// Every id is unique in both tables; lookup takes the first, so a duplicate could never be
    /// selected.
    #[test]
    fn ids_are_unique() {
        let catalog = Catalog::default();
        for ids in [
            catalog.offline.iter().map(|m| &m.id).collect::<Vec<_>>(),
            catalog.online.iter().map(|p| &p.id).collect::<Vec<_>>(),
        ] {
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "duplicate id in {ids:?}");
        }
    }

    /// A bigger download never declares a smaller footprint, or it would win on a machine that
    /// cannot load it.
    #[test]
    fn a_bigger_model_never_claims_a_smaller_footprint() {
        let catalog = Catalog::default();
        let mut by_size = catalog.offline.clone();
        by_size.sort_by_key(|m| m.download_mb);
        for pair in by_size.windows(2) {
            assert!(
                pair[0].min_vram_mb <= pair[1].min_vram_mb,
                "{} claims less VRAM than the smaller {}",
                pair[1].id,
                pair[0].id
            );
            assert!(
                pair[0].min_ram_mb <= pair[1].min_ram_mb,
                "{} claims less RAM than the smaller {}",
                pair[1].id,
                pair[0].id
            );
        }
    }

    /// The Messages API is not described with an OpenAI-shaped endpoint such as
    /// `api.anthropic.com/v1/chat/completions`, which does not exist.
    #[test]
    fn the_messages_api_is_not_described_as_openai_shaped() {
        let catalog = Catalog::default();
        let entry = catalog.online("anthropic").expect("present");
        assert_eq!(entry.wire, Wire::Anthropic);
        assert!(
            entry.endpoint.ends_with("/v1/messages"),
            "{}",
            entry.endpoint
        );
    }

    /// An empty catalogue is refused rather than accepted as "nothing available".
    #[test]
    fn an_empty_catalogue_is_refused() {
        assert!(Catalog::parse("").is_err());
    }

    /// An architecture with no loader behind it fails at parse.
    #[test]
    fn an_unknown_architecture_is_refused() {
        let text = r#"
[[offline]]
id = "x"
label = "x"
arch = "llama"
repo = "r"
file = "f"
tokenizer_repo = "t"
download_mb = 1
min_ram_mb = 1
min_vram_mb = 1
"#;
        assert!(Catalog::parse(text).is_err());
    }

    /// A known architecture parses, so the test above tests the value and not the row's shape.
    #[test]
    fn a_known_architecture_parses() {
        let text = r#"
[[offline]]
id = "x"
label = "x"
arch = "qwen2"
repo = "r"
file = "f"
tokenizer_repo = "t"
download_mb = 1
min_ram_mb = 1
min_vram_mb = 1
"#;
        let catalog = Catalog::parse(text).expect("parses");
        assert_eq!(catalog.offline[0].arch, Arch::Qwen2);
    }
}
