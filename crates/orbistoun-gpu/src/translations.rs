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
    /// What the translation wanted the caller told.
    pub warnings: Vec<String>,
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
}

impl TranslationStore {
    /// The store at `path`, or an empty one when none is there or it cannot be read.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        // Written beside and renamed over, so a run stopped mid-write leaves the old store or none.
        let partial = path.with_extension("partial");
        std::fs::write(&partial, bytes)?;
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
    pub fn insert(&mut self, key: u64, kept: Kept) {
        self.entries.insert(key, kept);
        self.changed = true;
    }

    /// How many translations are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
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
            if let Some(made) = translate(&kept.bytes, kept.context, encodings, operands) {
                *kept = made;
                refill.translated += 1;
                true
            } else {
                refill.dropped += 1;
                false
            }
        });
        translator.clone_into(&mut self.translator);
        self.changed = true;
        refill
    }
}

/// Translates a recorded shader against its context, or `None` when it no longer decodes cleanly or
/// translates.
fn translate(
    bytes: &[u8],
    context: Context,
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
        context,
        module: translated.module,
        textures: translated.textures,
        warnings: translated
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Context, TranslationStore, translate};
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
        let kept = translate(&TRIVIAL, context(), &encodings, &operands).expect("translates");
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
        let mut kept = translate(&TRIVIAL, context(), &encodings, &operands).expect("translates");
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
}
