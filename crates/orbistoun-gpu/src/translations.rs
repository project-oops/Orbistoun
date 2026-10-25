//! Translations kept across runs, keyed by content (D113).
//!
//! Each entry holds a shader's bytes and everything its translation was made against beside the
//! module it produced, so a later run serves the module without translating, and a run on another
//! translator makes every translation again from the recorded shaders before the guest starts.

use std::collections::BTreeMap;
use std::path::Path;

use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, TextureSource, UserData, Window};
use orbistoun_translate::{Strategy, translate_with_user_data};

/// A digest of the translator's sources and tables as this crate was built (`build.rs`).
pub const TRANSLATOR_INPUTS: &str = env!("ORBISTOUN_TRANSLATOR_INPUTS");

/// The translator in `build`: the build line for whoever reads the file, and the digest of what
/// translates, so a build from an edited tree is another translator though it names the same
/// commit. A run that lays every program out as the dispatch loop (`ORBISTOUN_CONTROL_FLOW`, D972)
/// is another translator too: its modules differ from the structured ones a store keeps under the
/// same keys, so the store is made again rather than served.
#[must_use]
pub fn translator(build: &str) -> String {
    let made = identity(build, TRANSLATOR_INPUTS);
    if orbistoun_translate::structured::enabled() {
        made
    } else {
        format!("{made} - dispatch loop")
    }
}

fn identity(build: &str, inputs: &str) -> String {
    format!("{build} - translator {inputs}")
}

/// What a translation was made against: the same shader under another of these is another module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Context {
    /// How control flow and the wavefront were modelled.
    pub strategy: Strategy,
    /// The host stage it was translated for.
    pub stage: Stage,
    /// The primitive a mesh module assembles.
    pub primitive: MeshPrimitive,
    /// The guest memory window its accesses are checked against.
    pub window: Window,
    /// Its stage's user-data layout.
    pub user_data: UserData,
}

/// One kept translation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    /// The shader, end-of-program included.
    pub bytes: Vec<u8>,
    /// What it was translated against.
    pub context: Context,
    /// The SPIR-V module, as words.
    pub module: Vec<u32>,
    /// Which texture the module samples at which binding.
    pub textures: Vec<TextureSource>,
    /// Where each of a guest dispatch's stored images' descriptors came from (D893).
    #[serde(default, deserialize_with = "one_or_many")]
    pub storage: Vec<TextureSource>,
    /// What the translation wanted the caller told.
    pub warnings: Vec<String>,
}

impl Kept {
    /// Lets go of its lists' spare capacity.
    fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
        self.module.shrink_to_fit();
        self.textures.shrink_to_fit();
        self.storage.shrink_to_fit();
        self.warnings.shrink_to_fit();
    }
}

/// A kept translation's stored images: a list, or the one image - or none - a store kept before a
/// dispatch could store to more than one holds (D893), so a store written then still loads.
fn one_or_many<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TextureSource>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Many(Vec<TextureSource>),
        One(Option<TextureSource>),
    }
    Ok(match serde::Deserialize::deserialize(deserializer)? {
        Stored::Many(sources) => sources,
        Stored::One(source) => source.into_iter().collect(),
    })
}

/// What a refill did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Refill {
    /// Entries already made by this translator.
    pub kept: usize,
    /// Entries translated again from their recorded shader.
    pub translated: usize,
    /// Entries this translator refuses, dropped.
    pub dropped: usize,
}

/// A title's kept translations.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TranslationStore {
    /// The build whose translator made every module here.
    translator: String,
    /// By cache key.
    entries: BTreeMap<u64, Kept>,
    /// Whether anything changed since it was read or written.
    #[serde(skip)]
    changed: bool,
    /// What [`Self::held_bytes`] answers, counted as entries come and go.
    #[serde(skip)]
    held: u64,
}

/// The bytes a kept translation holds: its shader and its module, as allocated.
fn held_by(kept: &Kept) -> u64 {
    (kept.bytes.capacity() + kept.module.capacity() * 4) as u64
}

impl TranslationStore {
    /// The store at `path`, or an empty one when none is there or it cannot be read.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        // Streamed, as it is written.
        let mut store: Self = std::fs::File::open(path)
            .ok()
            .and_then(|file| serde_json::from_reader(std::io::BufReader::new(file)).ok())
            .unwrap_or_default();
        // A list read from JSON grows by doubling, as nothing says its length first: up to half
        // of each would be spare.
        store.entries.values_mut().for_each(Kept::shrink_to_fit);
        store.count_held();
        store
    }

    /// Counts what its entries hold afresh.
    fn count_held(&mut self) {
        self.held = self.entries.values().map(held_by).sum();
    }

    /// Writes the store to `path` when it changed.
    ///
    /// # Errors
    ///
    /// When the directory cannot be created or the file written.
    pub fn save_if_changed(&mut self, path: &Path) -> std::io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Written beside and renamed over, so a run stopped mid-write leaves the old store or none;
        // and streamed, since the whole store as one string is hundreds of megabytes more held.
        let partial = path.with_extension("partial");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&partial)?);
        serde_json::to_writer(&mut file, self).map_err(std::io::Error::other)?;
        std::io::Write::flush(&mut file)?;
        drop(file);
        std::fs::rename(&partial, path)?;
        self.changed = false;
        Ok(())
    }

    /// The kept translation under `key`, when its shader is these bytes.
    #[must_use]
    pub fn get(&self, key: u64, bytes: &[u8]) -> Option<&Kept> {
        self.entries.get(&key).filter(|kept| kept.bytes == bytes)
    }

    /// Keeps a translation just made.
    pub fn insert(&mut self, key: u64, mut kept: Kept) {
        kept.shrink_to_fit();
        self.held += held_by(&kept);
        if let Some(old) = self.entries.insert(key, kept) {
            self.held = self.held.saturating_sub(held_by(&old));
        }
        self.changed = true;
    }

    /// Every kept translation, by cache key.
    pub fn entries(&self) -> impl Iterator<Item = &Kept> {
        self.entries.values()
    }

    /// How many translations are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The bytes its translations hold: each shader and its module, as allocated.
    #[must_use]
    pub const fn held_bytes(&self) -> u64 {
        self.held
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Makes every entry this `translator`'s: entries another build made are translated again from
    /// their recorded shader and context, and one it refuses is dropped.
    pub fn refill(
        &mut self,
        translator: &str,
        (encodings, operands): (&EncodingTable, &OperandTable),
    ) -> Refill {
        if self.translator == translator {
            return Refill {
                kept: self.entries.len(),
                ..Refill::default()
            };
        }
        let mut refill = Refill::default();
        self.entries.retain(|_, kept| {
            if let Some(made) = translate(&kept.bytes, &kept.context, encodings, operands) {
                *kept = made;
                kept.shrink_to_fit();
                refill.translated += 1;
                true
            } else {
                refill.dropped += 1;
                false
            }
        });
        translator.clone_into(&mut self.translator);
        self.count_held();
        self.changed = true;
        refill
    }
}

/// Translates a recorded shader against its context, or `None` when it no longer decodes cleanly or
/// translates.
fn translate(
    bytes: &[u8],
    context: &Context,
    encodings: &EncodingTable,
    operands: &OperandTable,
) -> Option<Kept> {
    let decoded = decode_program(bytes, encodings, operands);
    if !decoded.terminated || !decoded.is_trustworthy() || decoded.consumed != bytes.len() {
        return None;
    }
    let translated = translate_with_user_data(
        &decoded,
        encodings,
        context.strategy,
        (context.stage, context.primitive),
        context.window,
        context.user_data,
    )
    .ok()?;
    Some(Kept {
        bytes: bytes.to_vec(),
        context: *context,
        module: translated.module,
        textures: translated.textures,
        storage: translated.storage,
        warnings: translated
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Context, TranslationStore, identity, translate};
    use crate::translator_inputs::digest;
    use orbistoun_shader::{EncodingTable, OperandTable};
    use orbistoun_translate::Strategy;
    use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};

    /// `s_endpgm` alone: the smallest shader that decodes and translates.
    const TRIVIAL: [u8; 4] = 0xBF81_0000_u32.to_le_bytes();

    fn tables() -> (EncodingTable, OperandTable) {
        (
            EncodingTable::builtin().expect("encodings"),
            OperandTable::builtin().expect("operands"),
        )
    }

    fn context() -> Context {
        Context {
            strategy: Strategy::default(),
            stage: Stage::Compute,
            primitive: MeshPrimitive::default(),
            window: Window::default(),
            user_data: UserData::default(),
        }
    }

    /// A kept translation is served only for the bytes it was made from, and survives a round trip
    /// through its file.
    #[test]
    fn a_kept_translation_is_served_for_its_own_bytes() {
        let (encodings, operands) = tables();
        let kept = translate(&TRIVIAL, &context(), &encodings, &operands).expect("translates");
        let mut store = TranslationStore::default();
        store.insert(7, kept.clone());
        assert_eq!(store.get(7, &TRIVIAL), Some(&kept));
        assert_eq!(store.get(7, &[0, 0, 0, 0]), None);
        assert_eq!(store.get(8, &TRIVIAL), None);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("title").join("shader-translations.json");
        store.save_if_changed(&path).expect("written");
        assert_eq!(TranslationStore::load(&path).get(7, &TRIVIAL), Some(&kept));
    }

    /// A translation kept when a dispatch stored to one image at most, its stored image a single
    /// entry or `null`, still loads, as a list of that one or of none (D893).
    #[test]
    fn a_store_kept_with_one_stored_image_still_loads() {
        let (encodings, operands) = tables();
        let kept = translate(&TRIVIAL, &context(), &encodings, &operands).expect("translates");
        let source = orbistoun_translate::wavefront::TextureSource {
            user_data: Some(8),
            ..orbistoun_translate::wavefront::TextureSource::default()
        };
        let mut value = serde_json::to_value(&kept).expect("serialises");
        for (old, expected) in [
            (serde_json::Value::Null, Vec::new()),
            (
                serde_json::to_value(source).expect("serialises"),
                vec![source],
            ),
        ] {
            value["storage"] = old;
            let loaded: super::Kept = serde_json::from_value(value.clone()).expect("loads");
            assert_eq!(loaded.storage, expected);
        }
    }

    /// Nothing is written until something changes.
    #[test]
    fn an_unchanged_store_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("shader-translations.json");
        TranslationStore::default()
            .save_if_changed(&path)
            .expect("nothing to do");
        assert!(!path.exists());
    }

    /// Another build's translations are made again from their recorded shaders, and one this build
    /// refuses is dropped; this build's own are kept as they are.
    #[test]
    fn a_refill_translates_another_builds_entries_again() {
        let (encodings, operands) = tables();
        let mut kept = translate(&TRIVIAL, &context(), &encodings, &operands).expect("translates");
        let module = kept.module.clone();
        kept.module = vec![0xdead_beef];
        let mut refused = kept.clone();
        refused.bytes = vec![0xff; 4];
        let mut store = TranslationStore::default();
        store.insert(1, kept);
        store.insert(2, refused);

        let refill = store.refill("this build", (&encodings, &operands));
        assert_eq!((refill.translated, refill.dropped), (1, 1));
        assert_eq!(store.get(1, &TRIVIAL).map(|k| &k.module), Some(&module));
        assert_eq!(store.len(), 1);

        let again = store.refill("this build", (&encodings, &operands));
        assert_eq!((again.kept, again.translated), (1, 0));
    }

    /// Two uncommitted builds name the same commit; an edit to the translator between them makes
    /// them two translators, and the second translates the first's entries again. On 2026-09-28 an
    /// edit to the export translation was served the previous build's modules.
    #[test]
    fn an_uncommitted_translator_edit_is_another_translator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("orbistoun-gpu");
        let export = dir.path().join("orbistoun-translate/src/export.rs");
        std::fs::create_dir_all(export.parent().expect("parent")).expect("mkdir");
        std::fs::create_dir_all(&base).expect("mkdir");
        let roots = ["../orbistoun-translate/src"];
        std::fs::write(&export, "fn export() {}\n").expect("written");
        let before = format!("{:016x}", digest(&base, &roots));
        std::fs::write(&export, "fn export() { discard_invalid(); }\n").expect("written");
        let after = format!("{:016x}", digest(&base, &roots));

        let build = "v0.1.0 - 41eab6e-dirty";
        let (first, second) = (identity(build, &before), identity(build, &after));
        assert_ne!(first, second);

        let (encodings, operands) = tables();
        let mut kept = translate(&TRIVIAL, &context(), &encodings, &operands).expect("translates");
        let module = kept.module.clone();
        let mut store = TranslationStore::default();
        store.refill(&first, (&encodings, &operands));
        kept.module = vec![0xdead_beef];
        store.insert(1, kept);
        let refill = store.refill(&second, (&encodings, &operands));
        assert_eq!(refill.translated, 1);
        assert_eq!(store.get(1, &TRIVIAL).map(|k| &k.module), Some(&module));
    }

    /// Line endings alone are not an edit.
    #[test]
    fn line_endings_do_not_move_the_digest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("src/a.rs");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, "a\nb\n").expect("written");
        let lf = digest(dir.path(), &["src"]);
        std::fs::write(&file, "a\r\nb\r\n").expect("written");
        assert_eq!(digest(dir.path(), &["src"]), lf);
    }

    /// The stamped digest is the tree's: `build.rs` hashed the roots the tests hash.
    #[test]
    fn the_stamped_digest_is_the_trees() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            format!("{:016x}", digest(base, crate::translator_inputs::ROOTS)),
            super::TRANSLATOR_INPUTS
        );
        assert!(super::translator("v").ends_with(super::TRANSLATOR_INPUTS));
    }

    /// A run that lays programs out as the dispatch loop is another translator (D972): a store kept
    /// by a structured run is made again for it, rather than serving modules of the other layout
    /// under the same keys, and the next structured run makes it again in turn.
    #[test]
    fn the_layout_of_control_flow_is_part_of_the_translator() {
        use orbistoun_translate::structured::with_shape;
        let structured = with_shape(true, || super::translator("v"));
        let dispatched = with_shape(false, || super::translator("v"));
        assert_ne!(structured, dispatched);
        assert!(structured.ends_with(super::TRANSLATOR_INPUTS));
    }
}
