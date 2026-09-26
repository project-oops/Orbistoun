//! Placing the modules a title ships, and indexing what they export (D640).
//!
//! A module the title ships has real code, so the answer for an import it provides is an address
//! inside the placed image, not a stub. [`crate::titlemodules`] finds the files; this places them
//! and works out which imports they answer. An encoded import's library id is half its identity, so
//! matching is by library and hash; a plain name is matched across every module and reported as
//! ambiguous when more than one answers. Placement copies bytes only: the caller relocates and
//! protects the images before anything binds, because unrelocated code fails somewhere unrelated.

use std::collections::BTreeMap;

use orbistoun_elf::dynamic::{Kind, NameForm, RawExport, RawImport};
use orbistoun_loader::Image;

use crate::titlemodules::TitleModule;

/// One symbol a title's own module makes available, at its placed address.
#[derive(Debug, Clone)]
pub struct TitleExport {
    /// Which of the title's modules exports it.
    pub library: String,
    /// The hash an importer names it by.
    pub nid: u64,
    /// Whether it is code or data.
    pub kind: Kind,
    /// The name exactly as the module spells it, encoding included.
    pub name: String,
    /// Where it now lives, module base included.
    pub address: u64,
}

/// One module exporting one hash twice.
///
/// A NID hashes the symbol name alone, so a module declaring one name in two places collides with
/// itself. The first wins and the choice is reported. Two different modules exporting one hash is
/// ordinary: the importer names the library.
#[derive(Debug, Clone)]
pub struct ExportCollision {
    /// The contested hash.
    pub nid: u64,
    /// The module that declared it twice.
    pub library: String,
    /// The name, as the export that got there first spells it.
    pub kept: String,
    /// The name of the export that was passed over.
    pub dropped: String,
}

/// An import whose hash a module answers, with a kind that disagrees.
///
/// Not bound: binding a data export as a function hands the guest a thunk where it expects a value,
/// and the reverse hands it a value where it expects code (D307). The import stays on its stub.
#[derive(Debug, Clone)]
pub struct KindMismatch {
    /// The name the executable imports by.
    pub name: String,
    /// What the executable's table says it wants.
    pub wanted: Kind,
    /// What the module's table says it has.
    pub found: Kind,
    /// Which of the title's modules holds it.
    pub library: String,
}

/// An unattributed import that more than one of the title's modules answers.
///
/// A plain name carries no library id, so the hash is all there is to match. Left unbound, since a
/// guess that lands in the wrong module is a call into unrelated code.
#[derive(Debug, Clone)]
pub struct Ambiguous {
    /// The name, as the executable spells it.
    pub name: String,
    /// Every module that answers it.
    pub answered_by: Vec<String>,
}

/// The title's own modules, placed, with an index of everything they export.
///
/// Holds the images, because dropping one unmaps it: an address in the index is live exactly as
/// long as this value.
#[derive(Debug)]
pub struct PlacedTitleModules {
    images: Vec<(String, Image)>,
    exports: BTreeMap<String, BTreeMap<u64, TitleExport>>,
    collisions: Vec<ExportCollision>,
}

impl PlacedTitleModules {
    /// Every module placed, in the order they were placed, with the library name each answers.
    #[must_use]
    pub fn images(&self) -> &[(String, Image)] {
        &self.images
    }

    /// The same images, mutably, so they can be re-protected once relocation has finished.
    ///
    /// Placement leaves every page writable and none executable, because relocation writes into
    /// text, so a module is not runnable until its protection is applied (D489).
    pub fn images_mut(&mut self) -> &mut [(String, Image)] {
        &mut self.images
    }

    /// Everything the placed modules export, by library and then by hash.
    #[must_use]
    pub const fn exports(&self) -> &BTreeMap<String, BTreeMap<u64, TitleExport>> {
        &self.exports
    }

    /// How many symbols were indexed in total.
    #[must_use]
    pub fn export_count(&self) -> usize {
        self.exports.values().map(BTreeMap::len).sum()
    }

    /// Hashes a single module claimed twice.
    #[must_use]
    pub fn collisions(&self) -> &[ExportCollision] {
        &self.collisions
    }

    /// Which of `imports` these modules answer, by dynamic symbol index.
    ///
    /// `libraries` is the executable's own library table, which an encoded import's `library_id`
    /// indexes. The address map is keyed by symbol index, as a relocation is, so a resolver needs
    /// no lookup logic of its own. A kind mismatch (D307) and an unattributed name two modules
    /// answer are left out and reported; an import with no match is absent, the ordinary case for
    /// platform imports.
    #[must_use]
    pub fn resolve(&self, imports: &[RawImport], libraries: &BTreeMap<u16, String>) -> Resolution {
        let mut out = Resolution::default();
        for import in imports {
            let found = match import.form {
                NameForm::Encoded { library_id, .. } => {
                    let Some(library) = libraries.get(&library_id) else {
                        continue;
                    };
                    self.exports
                        .get(library)
                        .and_then(|by_nid| by_nid.get(&import.nid).map(|export| vec![export]))
                }
                // No attribution to narrow it, so every module is a candidate (D483).
                NameForm::Plain => {
                    let all: Vec<&TitleExport> = self
                        .exports
                        .values()
                        .filter_map(|by_nid| by_nid.get(&import.nid))
                        .collect();
                    (!all.is_empty()).then_some(all)
                }
            };
            let Some(candidates) = found else { continue };
            if candidates.len() > 1 {
                out.ambiguous.push(Ambiguous {
                    name: import.name.clone(),
                    answered_by: candidates.iter().map(|e| e.library.clone()).collect(),
                });
                continue;
            }
            let export = candidates[0];
            if kinds_disagree(import.kind, export.kind) {
                out.mismatches.push(KindMismatch {
                    name: import.name.clone(),
                    wanted: import.kind,
                    found: export.kind,
                    library: export.library.clone(),
                });
                continue;
            }
            out.addresses.insert(import.symbol_index, export.address);
            out.bound.push(BoundImport {
                name: import.name.clone(),
                nid: import.nid,
                library: export.library.clone(),
                address: export.address,
            });
        }
        out
    }
}

/// One import answered by one of the title's own modules.
///
/// Carried beside the address map: a resolver wants a bare index-to-address lookup, and a report
/// wants to name which module answered.
#[derive(Debug, Clone)]
pub struct BoundImport {
    /// The name the executable imports by.
    pub name: String,
    /// The hash, so a binding can be cross-referenced against any other report.
    pub nid: u64,
    /// Which of the title's own modules answered it.
    pub library: String,
    /// Where it landed.
    pub address: u64,
}

/// What the title's own modules answer, and what they nearly answered.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// Dynamic symbol index to the address inside a placed module.
    pub addresses: BTreeMap<u32, u64>,
    /// The same bindings, with the module that supplied each.
    pub bound: Vec<BoundImport>,
    /// Hashes that matched but whose kind did not.
    pub mismatches: Vec<KindMismatch>,
    /// Unattributed names more than one module answered.
    pub ambiguous: Vec<Ambiguous>,
}

impl Resolution {
    /// How many imports each of the title's own modules answered.
    #[must_use]
    pub fn by_library(&self) -> BTreeMap<&str, usize> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for bound in &self.bound {
            *counts.entry(bound.library.as_str()).or_default() += 1;
        }
        counts
    }
}

/// Whether an import's kind and an export's kind are known to be different things.
///
/// [`Kind::Unspecified`] is not a disagreement: the table did not say, which is not a claim that
/// the symbol is the other thing.
const fn kinds_disagree(wanted: Kind, found: Kind) -> bool {
    matches!(
        (wanted, found),
        (Kind::Function, Kind::Object) | (Kind::Object, Kind::Function)
    )
}

/// Reads each module's exports and folds them into one index, per library.
///
/// Built from every module before anything binds (D482), so a result does not depend on the order
/// the filesystem offered the modules.
fn index_exports(
    placed: &[(String, Image)],
    read: &[(String, Vec<RawExport>)],
) -> (
    BTreeMap<String, BTreeMap<u64, TitleExport>>,
    Vec<ExportCollision>,
) {
    let mut exports: BTreeMap<String, BTreeMap<u64, TitleExport>> = BTreeMap::new();
    let mut collisions = Vec::new();
    for ((library, image), (_, found)) in placed.iter().zip(read) {
        let by_nid = exports.entry(library.clone()).or_default();
        for export in found {
            let address = image.base().saturating_add(export.offset);
            if let Some(existing) = by_nid.get(&export.nid) {
                collisions.push(ExportCollision {
                    nid: export.nid,
                    library: library.clone(),
                    kept: existing.name.clone(),
                    dropped: export.name.clone(),
                });
                continue;
            }
            by_nid.insert(
                export.nid,
                TitleExport {
                    library: library.clone(),
                    nid: export.nid,
                    kind: export.kind,
                    name: export.name.clone(),
                    address,
                },
            );
        }
    }
    (exports, collisions)
}

/// Where the next module goes, given where the last one ended.
///
/// A whole granule of guard between modules, so a stray offset off the end of one lands in unmapped
/// space and faults rather than reaching into the next.
fn next_base(image: &Image, granularity: u64) -> u64 {
    let (start, len) = image.span();
    let end = start.saturating_add(len);
    let aligned = end.next_multiple_of(granularity.max(1));
    aligned.saturating_add(granularity)
}

/// Places each module in turn and returns them with everything they export.
///
/// The caller supplies `base`, as for the main executable: where a run puts things is the run's
/// layout decision.
pub(crate) fn place_all(
    modules: &[TitleModule],
    base: u64,
    granularity: u64,
    place: impl Fn(&[u8], u64) -> Result<Image, crate::ServiceError>,
    exports_of: impl Fn(&[u8]) -> Result<Vec<RawExport>, crate::ServiceError>,
) -> Result<PlacedTitleModules, crate::ServiceError> {
    let mut images = Vec::new();
    let mut read = Vec::new();
    let mut at = base;
    for module in modules {
        let bytes = std::fs::read(&module.path).map_err(|source| crate::ServiceError::Io {
            path: module.path.display().to_string(),
            source,
        })?;
        let image = place(&bytes, at)?;
        at = next_base(&image, granularity);
        read.push((module.library.clone(), exports_of(&bytes)?));
        images.push((module.library.clone(), image));
    }
    let (exports, collisions) = index_exports(&images, &read);
    Ok(PlacedTitleModules {
        images,
        exports,
        collisions,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use orbistoun_elf::dynamic::{Binding, Kind, NameForm, RawImport};

    use super::{PlacedTitleModules, TitleExport, kinds_disagree};

    /// A kind the table did not state is not a claim about the other kind.
    #[test]
    fn an_unstated_kind_is_not_a_disagreement() {
        assert!(!kinds_disagree(Kind::Unspecified, Kind::Function));
        assert!(!kinds_disagree(Kind::Function, Kind::Unspecified));
        assert!(!kinds_disagree(Kind::Unspecified, Kind::Unspecified));
    }

    /// Code and data are a disagreement in both directions (D307).
    #[test]
    fn code_and_data_disagree_whichever_way_round() {
        assert!(kinds_disagree(Kind::Function, Kind::Object));
        assert!(kinds_disagree(Kind::Object, Kind::Function));
    }

    /// A kind that agrees with itself binds.
    #[test]
    fn matching_kinds_agree() {
        assert!(!kinds_disagree(Kind::Function, Kind::Function));
        assert!(!kinds_disagree(Kind::Object, Kind::Object));
    }

    /// Builds an index without placing anything, so resolution is testable on its own.
    fn indexed(entries: &[(&str, u64, Kind, u64)]) -> PlacedTitleModules {
        let mut exports: BTreeMap<String, BTreeMap<u64, TitleExport>> = BTreeMap::new();
        for (library, nid, kind, address) in entries {
            exports.entry((*library).to_owned()).or_default().insert(
                *nid,
                TitleExport {
                    library: (*library).to_owned(),
                    nid: *nid,
                    kind: *kind,
                    name: format!("symbol{nid:#x}"),
                    address: *address,
                },
            );
        }
        PlacedTitleModules {
            images: Vec::new(),
            exports,
            collisions: Vec::new(),
        }
    }

    fn encoded(symbol_index: u32, nid: u64, library_id: u16) -> RawImport {
        RawImport {
            symbol_index,
            nid,
            form: NameForm::Encoded {
                library_id,
                module_id: 0,
            },
            kind: Kind::Function,
            binding: Binding::Global,
            name: format!("import{nid:#x}"),
        }
    }

    fn plain(symbol_index: u32, nid: u64) -> RawImport {
        RawImport {
            symbol_index,
            nid,
            form: NameForm::Plain,
            kind: Kind::Function,
            binding: Binding::Global,
            name: format!("import{nid:#x}"),
        }
    }

    /// An import binds into the library it names, and not into another that shares a hash (D483).
    #[test]
    fn an_attributed_import_binds_only_into_the_library_it_names() {
        let modules = indexed(&[
            ("Gameplay", 0xAAAA, Kind::Function, 0x1000),
            ("Helper", 0xAAAA, Kind::Function, 0x9000),
        ]);
        let libraries = BTreeMap::from([(0, "Gameplay".to_owned()), (1, "Helper".to_owned())]);
        let resolved = modules.resolve(&[encoded(7, 0xAAAA, 1)], &libraries);
        assert_eq!(resolved.addresses.get(&7), Some(&0x9000));
        assert_eq!(resolved.bound[0].library, "Helper");
    }

    /// A library the executable's table does not list cannot be matched against; a hash-only
    /// fallback is what the test above forbids.
    #[test]
    fn an_import_naming_an_unknown_library_binds_to_nothing() {
        let modules = indexed(&[("Gameplay", 0xAAAA, Kind::Function, 0x1000)]);
        let libraries = BTreeMap::from([(0, "Gameplay".to_owned())]);
        assert!(
            modules
                .resolve(&[encoded(7, 0xAAAA, 42)], &libraries)
                .addresses
                .is_empty()
        );
    }

    /// An unattributed name two modules answer is reported, not guessed at.
    #[test]
    fn an_unattributed_name_two_modules_answer_is_left_unbound() {
        let modules = indexed(&[
            ("Gameplay", 0xBBBB, Kind::Function, 0x1000),
            ("Helper", 0xBBBB, Kind::Function, 0x9000),
        ]);
        let resolved = modules.resolve(&[plain(3, 0xBBBB)], &BTreeMap::new());
        assert!(resolved.addresses.is_empty(), "a guess was made");
        assert_eq!(resolved.ambiguous.len(), 1);
        assert_eq!(resolved.ambiguous[0].answered_by, ["Gameplay", "Helper"]);
    }

    /// An unattributed name only one module answers binds to it.
    #[test]
    fn an_unattributed_name_one_module_answers_binds() {
        let modules = indexed(&[("Gameplay", 0xCCCC, Kind::Function, 0x1000)]);
        let resolved = modules.resolve(&[plain(3, 0xCCCC)], &BTreeMap::new());
        assert_eq!(resolved.addresses.get(&3), Some(&0x1000));
    }

    /// A hash that matches with a kind that does not is reported and left on its stub (D307).
    #[test]
    fn a_kind_mismatch_is_reported_rather_than_bound() {
        let modules = indexed(&[("Gameplay", 0xDDDD, Kind::Object, 0x1000)]);
        let libraries = BTreeMap::from([(0, "Gameplay".to_owned())]);
        let resolved = modules.resolve(&[encoded(5, 0xDDDD, 0)], &libraries);
        assert!(resolved.addresses.is_empty());
        assert_eq!(resolved.mismatches.len(), 1);
        assert_eq!(resolved.mismatches[0].found, Kind::Object);
    }
}

/// Why one import did not bind to a module the title ships (D640).
///
/// A count of failures is not a reason for any of them, so each unbound import carries the step
/// that declined it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unbound {
    /// The import names a library id the module's own table does not list.
    NoLibraryName,
    /// The library is named, and no module the title ships exports under that name.
    NoSuchLibrary,
    /// The library is placed, and it does not export this hash.
    NotExported,
    /// More than one module answered a name that carries no attribution.
    Ambiguous,
    /// The hash matched and the kind did not: a function wanted where an object is exported, or the
    /// reverse.
    KindMismatch,
    /// It resolved, and orbistoun implements it, so this project answers instead.
    KeptByOrbistoun,
}

impl Unbound {
    /// One line a reader can act on.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::NoLibraryName => {
                "the import carries a library id the module's own library table does not list"
            }
            Self::NoSuchLibrary => "no module this title ships exports under that library name",
            Self::NotExported => "that module is placed and does not export this hash",
            Self::Ambiguous => "more than one module answered a name that carries no attribution",
            Self::KindMismatch => {
                "that module exports the hash with a different kind - one wants a function and the other has an object, or the reverse"
            }
            Self::KeptByOrbistoun => "orbistoun implements it, so this project answers instead",
        }
    }
}

/// Every import that did not bind to a title module, and why not.
#[derive(Debug, Clone)]
pub struct BindingAccount {
    /// How many imports the executable declares.
    pub imports: usize,
    /// How many were bound to a module the title ships.
    pub bound: usize,
    /// The rest, by reason, with an example for each.
    pub unbound: Vec<(Unbound, String, usize)>,
    /// Which of the title's modules answered, and how many each. A total cannot say whether the
    /// module that matters answered.
    pub by_library: Vec<(String, usize)>,
}

impl BindingAccount {
    /// The lines a run prints about it, printed even when everything bound.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "orbistoun: {} of {} import(s) bound to a module this title ships",
            self.bound, self.imports
        )];
        if self.by_library.is_empty() {
            out.push("orbistoun:   none of them, so every call goes to a stub".to_owned());
        } else {
            let each: Vec<String> = self
                .by_library
                .iter()
                .map(|(library, count)| format!("{library} {count}"))
                .collect();
            out.push(format!("orbistoun:   from {}", each.join(", ")));
        }
        for (why, library, count) in &self.unbound {
            out.push(format!(
                "orbistoun:   {count} unbound from {library} - {}",
                why.why()
            ));
        }
        out
    }
}
