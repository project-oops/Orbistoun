//! The ELF loader: places guest modules and binds their imports by dynamic linker relocation.
//!
//! Loading parses the container (`orbistoun-elf`), reserves the address space it demands
//! (`orbistoun-mem`), resolves every imported NID against the registry (`orbistoun-hle`),
//! applies relocations, and sets up thread-local storage before handing over the entry point.
//! The relocation is the binding: the guest calls whatever address its slot holds, and
//! orbistoun is the linker that writes it.
//!
//! [`survey`] parses and resolves without executing anything, answering what a title imports.

pub mod image;
pub mod plan;
pub mod process;
pub mod protect;
pub mod relocate;
pub mod tls;

pub use image::{Image, PlacedSegment};

use orbistoun_elf::{Container, ElfError};
use orbistoun_hle::Registry;
use orbistoun_nid::Nid;

/// Why a module could not be loaded or surveyed.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The container could not be parsed.
    #[error("container: {0}")]
    Elf(#[from] ElfError),
    /// The address space could not be reserved.
    #[error("address space: {0}")]
    Memory(#[from] orbistoun_mem::MemError),
    /// The container declares no loadable segments.
    #[error("container declares no loadable segments")]
    NothingToLoad,
    /// An address does not fit in a host pointer.
    #[error("address {0:#x} does not fit in a pointer")]
    AddressTooLarge(u64),
    /// A relocation would write outside the image it belongs to.
    ///
    /// A corrupt table would write over unrelated memory, so it is refused rather than
    /// clamped.
    #[error(
        "relocation targets {target:#x}, outside the image span          {span_base:#x}..{:#x}", span_base.saturating_add(*span_len)
    )]
    RelocationOutOfBounds {
        /// Address the relocation wanted to write.
        target: u64,
        /// Start of the image span.
        span_base: u64,
        /// Length of the image span.
        span_len: u64,
    },
}

/// One import a guest module asks for, and whether orbistoun can answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyedImport {
    /// Index into the dynamic symbol table - the number a call trace reports.
    pub symbol_index: u32,
    /// The hash the module imports by.
    pub nid: Nid,
    /// Library name, if the import table named one.
    pub library: Option<String>,
    /// Symbol name, if the registry or symbol database knows it.
    pub name: Option<String>,
    /// Whether orbistoun has this function declared at all.
    pub known: bool,
    /// Whether the guest wants code or data here.
    ///
    /// `known` says orbistoun has something for the slot; this says whether a thunk is the
    /// right kind of thing to put there (D307).
    pub kind: orbistoun_elf::dynamic::Kind,
}

/// What a module needs, determined without executing it.
#[derive(Debug, Clone, Default)]
pub struct Survey {
    /// Guest entry point address.
    pub entry: u64,
    /// Every import, in table order.
    pub imports: Vec<SurveyedImport>,
}

impl Survey {
    /// How many imports orbistoun cannot answer.
    ///
    /// The headline figure for a compatibility report.
    pub fn unresolved(&self) -> usize {
        self.imports.iter().filter(|i| !i.known).count()
    }
}

/// Determines what `bytes` needs, without executing any of it.
///
/// Pure inspection: parses the container, walks the import table, and checks each
/// NID against `registry`.
pub fn survey(bytes: &[u8], registry: &Registry) -> Result<Survey, LoadError> {
    let container = Container::parse(bytes)?;
    // The registry's own hasher: a name hashed with a different suffix resolves to nothing,
    // silently (D305).
    let raw = container.raw_imports(bytes, registry.hasher())?;
    // The table the ids index. `DT_NEEDED` is a different list and gives attributions that
    // fit and mean nothing.
    let libraries = container.import_libraries(bytes)?;

    let imports = raw
        .into_iter()
        .map(|import| {
            let nid = Nid::from_raw(import.nid);
            let resolved = registry.resolve(nid);
            SurveyedImport {
                symbol_index: import.symbol_index,
                kind: import.kind,
                // Prefer the declared library; fall back to the one the module named, so an
                // unresolved import is still attributed to a library.
                library: resolved.map_or_else(
                    || {
                        import
                            .library_id()
                            .and_then(|id| libraries.get(&id).cloned())
                    },
                    |r| Some(r.library.to_owned()),
                ),
                name: resolved.map(|r| r.name.to_owned()),
                nid,
                known: resolved.is_some(),
            }
        })
        .collect();

    Ok(Survey {
        entry: container.entry(),
        imports,
    })
}

#[cfg(test)]
mod tests {
    use super::{Survey, SurveyedImport};
    use orbistoun_nid::Nid;

    /// `unresolved` counts only imports the registry does not know.
    #[test]
    fn unresolved_counts_only_unknown_imports() {
        let survey = Survey {
            entry: 0x1000,
            imports: vec![
                SurveyedImport {
                    kind: orbistoun_elf::dynamic::Kind::Function,
                    symbol_index: 1,
                    nid: Nid::from_raw(1),
                    library: Some("libTest".to_owned()),
                    name: Some("testInit".to_owned()),
                    known: true,
                },
                SurveyedImport {
                    kind: orbistoun_elf::dynamic::Kind::Function,
                    symbol_index: 2,
                    nid: Nid::from_raw(2),
                    library: None,
                    name: None,
                    known: false,
                },
            ],
        };
        assert_eq!(survey.unresolved(), 1);
    }
}
