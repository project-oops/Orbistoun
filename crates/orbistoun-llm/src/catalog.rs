//! The model catalogue, read from data.
//!
//! Everything anybody has to know to *choose* a model lives here, and nothing about
//! how to run one does. That split is what makes [`crate::select`] testable with no
//! network, no accelerator and no model on disk: sizing is arithmetic over this table.
//!
//! # The one field that is not free-form
//!
//! [`Offline::arch`] names a loader. A value with no loader behind it is refused by
//! name at load time rather than substituted for something close, because the failure
//! it prevents is silent: a Qwen3 GGUF read through a Qwen2 loader does not error, it
//! produces a model that generates plausible-looking rubbish. Principle 3 - an
//! explicit "not handled" beats a wrong answer, and both cost the same to write.

use serde::Deserialize;

use crate::Error;

/// The catalogue shipped with this crate.
///
/// `include_str!` rather than a runtime read, matching `orbistoun-nid`'s hash suffix
/// and `orbistoun-names`' grammar: the default travels with the binary so a fresh
/// machine needs no files, and a caller who wants a different one passes it to
/// [`Catalog::parse`].
pub const DEFAULT_CATALOG: &str = include_str!("../data/models.toml");

/// Which in-process loader reads a model's weights.
///
/// Not a string, because the set of values is exactly the set of loaders that exist,
/// and that is a fact about this crate's code rather than about its data.
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
/// The protocol, deliberately not the vendor. Three of the four hosted entries in the
/// shipped catalogue speak the same OpenAI-shaped request and one does not, and the
/// distinction that matters when writing bytes onto a socket is that one, not whose
/// logo is on it.
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
    /// Environment variable holding the key. Empty means no key is needed at all,
    /// which is the local-model-server case and not an oversight.
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
        // The shipped catalogue is a test-covered constant, so a parse failure here is
        // a broken build rather than a runtime condition a caller could handle.
        Self::parse(DEFAULT_CATALOG).expect("the shipped catalogue parses")
    }
}

impl Catalog {
    /// Reads a catalogue.
    ///
    /// # Errors
    ///
    /// If the text is not valid TOML in this shape, or if it declares no models at
    /// all - an empty catalogue would present as "this machine can run nothing",
    /// which is indistinguishable from a machine that genuinely can and is the wrong
    /// thing to be quiet about.
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
    /// Falls back to the first entry if nothing is marked `auto`, which keeps a
    /// hand-written catalogue usable rather than empty.
    pub fn smallest_auto(&self) -> Option<&Offline> {
        self.offline
            .iter()
            .filter(|m| m.auto)
            .min_by_key(|m| m.min_ram_mb)
            .or_else(|| self.offline.first())
    }

    /// The entry to use when the host reports nothing measurable.
    ///
    /// Deliberately the `default` entry rather than the largest: an over-sized pick
    /// fails at load, which is minutes into a multi-gigabyte download and the worst
    /// possible place to discover it.
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

    /// The shipped catalogue parses.
    ///
    /// `Catalog::default` panics on a malformed one, so without this the first person
    /// to learn about a typo is whoever runs the binary.
    #[test]
    fn the_shipped_catalogue_parses() {
        let catalog = Catalog::parse(DEFAULT_CATALOG).expect("parses");
        assert!(!catalog.offline.is_empty());
        assert!(!catalog.online.is_empty());
    }

    /// Exactly one offline model is the unmeasured-host default.
    ///
    /// Two would make the choice depend on file order, and none would push every
    /// unmeasurable machine onto the smallest model - a silent quality floor that
    /// nothing would ever report.
    #[test]
    fn exactly_one_model_is_the_default() {
        let catalog = Catalog::default();
        let defaults: Vec<_> = catalog.offline.iter().filter(|m| m.default).collect();
        assert_eq!(defaults.len(), 1, "{defaults:?}");
    }

    /// Every id is unique, in both tables.
    ///
    /// A duplicate id is not an error anywhere - lookup takes the first - so it would
    /// present as an entry that cannot be selected however it is configured.
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

    /// Sizing is monotonic with download size.
    ///
    /// The selector picks the largest model that fits, using `min_vram_mb`. If a
    /// bigger download declared a smaller footprint it would win on a machine that
    /// cannot actually load it, and the symptom would arrive after the download.
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

    /// The hosted vendor whose API is not OpenAI-shaped is not described as though it
    /// were.
    ///
    /// This is the specific mistake two sibling projects both made - listing
    /// `api.anthropic.com/v1/chat/completions`, which is not a real endpoint - and it
    /// is cheap to pin so a future edit cannot quietly reintroduce it.
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
    ///
    /// The failure it prevents is silent: reading one model family's weights with
    /// another's loader produces output rather than an error.
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

    /// A known architecture parses, so the test above is testing the value and not
    /// the shape of the row around it.
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
