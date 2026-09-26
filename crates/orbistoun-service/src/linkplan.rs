//! A whole title linked into one plan, and the plan kept in the title library (D724).
//!
//! A run and an ahead-of-time link go through the same steps here, so the plan a run stores is the
//! plan `orbistoun-cli link` would have stored.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use orbistoun_loader::plan::{LinkPlan, ModulePlan, PlanDifference, PlanKey, SlotWrite, Standing};
use orbistoun_loader::relocate::Applied;
use orbistoun_proto::LinkSummary;

use crate::{LinkedTitle, Service, ServiceError, SymbolDbFile, TitleBases};

/// The title a module belongs to: the name of the directory holding it.
#[must_use]
pub fn title_of(module: &Path) -> String {
    module.parent().and_then(Path::file_name).map_or_else(
        || "unknown".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The executable relocated against the title's modules and stubs.
#[derive(Debug)]
pub struct ExecutableLink {
    /// What relocation applied, and every value it wrote.
    pub applied: Applied,
    /// The imports refused, when the caller asked for any to be.
    pub refused: Option<BTreeSet<usize>>,
}

/// The plan for a relocated executable followed by the title's own modules.
///
/// # Errors
///
/// When the executable cannot be parsed for its `syscall` sites.
pub fn plan_of(
    image: &orbistoun_loader::Image,
    bytes: &[u8],
    writes: Vec<SlotWrite>,
    title: &LinkedTitle,
) -> Result<LinkPlan, ServiceError> {
    let mut modules = vec![module_plan("", image, bytes, writes)?];
    modules.extend(title.plans.iter().cloned());
    Ok(LinkPlan { modules })
}

/// One module's plan: where it was placed, what relocation wrote, and its raw `syscall` sites.
///
/// # Errors
///
/// When the module cannot be parsed for its `syscall` sites.
pub fn module_plan(
    library: &str,
    image: &orbistoun_loader::Image,
    bytes: &[u8],
    writes: Vec<SlotWrite>,
) -> Result<ModulePlan, ServiceError> {
    let syscalls = orbistoun_loader::inventory::syscall_sites(bytes, image.base())?;
    Ok(ModulePlan::of(library, image, writes).with_syscalls(syscalls))
}

/// How many differing slots are named before the rest are only counted.
pub const PLAN_DIFFERENCES_NAMED: usize = 8;

/// Two plans' differences in words: each module placed differently, then the first differing slots
/// named by the import whose stub either plan wrote there.
#[must_use]
pub fn describe_plan_difference(
    difference: &PlanDifference,
    thunks: &orbistoun_thunk::ThunkTable,
    labels: &[String],
) -> Vec<String> {
    let module = |library: &str| {
        if library.is_empty() {
            "the executable".to_owned()
        } else {
            library.to_owned()
        }
    };
    let value = |v: Option<u64>| v.map_or_else(|| "nothing".to_owned(), |v| format!("{v:#x}"));
    let mut lines: Vec<String> = difference
        .placements
        .iter()
        .map(|library| format!("{} is placed differently", module(library)))
        .collect();
    lines.extend(
        difference
            .syscalls
            .iter()
            .map(|library| format!("{} lists different syscall sites", module(library))),
    );
    for slot in difference.slots.iter().take(PLAN_DIFFERENCES_NAMED) {
        let label = [slot.fresh, slot.stored]
            .into_iter()
            .flatten()
            .filter_map(|v| thunks.index_of(v))
            .find_map(|index| labels.get(index).filter(|l| !l.is_empty()))
            .cloned()
            .unwrap_or_else(|| format!("{} slot", module(&slot.library)));
        lines.push(format!(
            "{label} at {:#x}: stored {}, now {}",
            slot.at,
            value(slot.stored),
            value(slot.fresh)
        ));
    }
    let rest = difference
        .slots
        .len()
        .saturating_sub(PLAN_DIFFERENCES_NAMED);
    if rest > 0 {
        lines.push(format!("and {rest} more slots"));
    }
    lines
}

impl Service {
    /// Applies the executable's relocations against the title's code, then the stubs.
    ///
    /// The title's own modules answer first, for the imports they were bound to, and everything
    /// else falls through to the stub table: a stub answering for a function relocated in the
    /// title's own code hands the guest a placeholder it uses as an address (D483). Unanswered weak
    /// imports bind to zero (D676), and are never among those refused (D392).
    ///
    /// # Errors
    ///
    /// When the executable cannot be relocated.
    pub fn relocate_executable(
        &self,
        image: &orbistoun_loader::Image,
        bytes: &[u8],
        title: &LinkedTitle,
        symbols: &SymbolDbFile,
        mut refuse: Option<BTreeSet<usize>>,
    ) -> Result<ExecutableLink, ServiceError> {
        let weak_zero = self.unanswered_weak_imports(bytes, title, symbols);
        if let Some(refused) = refuse.as_mut() {
            refused.retain(|index| !weak_zero.contains(index));
        }
        // No offset: the executable is module 0, so its symbol index is its slot (D484).
        let stubs = orbistoun_loader::relocate::ImportResolver {
            thunks: &title.thunks,
            data: &title.data,
            refuse: refuse.as_ref(),
            weak_zero: Some(&weak_zero),
        };
        let resolver = orbistoun_loader::relocate::TitleResolver {
            bound: &title.bound,
            inner: &stubs,
        };
        let applied = self.relocate_image_recorded(image, bytes, &resolver)?;
        Ok(ExecutableLink {
            applied,
            refused: refuse,
        })
    }

    /// Weak imports that nothing answers: no module the title ships, and neither an implementation
    /// nor the symbol database.
    fn unanswered_weak_imports(
        &self,
        bytes: &[u8],
        title: &LinkedTitle,
        symbols: &SymbolDbFile,
    ) -> BTreeSet<usize> {
        let Ok(imports) = self.raw_imports_of(bytes) else {
            return BTreeSet::new();
        };
        let db = orbistoun_nid::SymbolDb::from_file(symbols).map(|(db, _)| db);
        imports
            .into_iter()
            .filter(|imp| {
                imp.binding == orbistoun_elf::dynamic::Binding::Weak
                    && !title.bound.contains_key(&imp.symbol_index)
                    && !self.is_named_with(orbistoun_nid::Nid::from_raw(imp.nid), db.as_ref())
            })
            .map(|imp| imp.symbol_index as usize)
            .collect()
    }

    /// Where the plan for the title `executable` belongs to is stored, when this service has a
    /// title library.
    #[must_use]
    pub fn link_plan_file(&self, executable: &Path) -> Option<PathBuf> {
        self.paths()
            .map(|paths| paths.title_link_plan_file(&title_of(executable)))
    }

    /// Compares a fresh plan with the one stored for the title and stores it when none is kept
    /// under its key, or always under `relink`.
    ///
    /// The differences are listed on a mismatch, and on a relink whatever the stored plan's key.
    #[must_use]
    pub fn settle_link_plan(
        &self,
        executable: (&Path, &[u8]),
        plan: &LinkPlan,
        title: &LinkedTitle,
        relink: bool,
    ) -> LinkSummary {
        let (path, bytes) = executable;
        let mut summary = LinkSummary {
            digest: plan.digest(),
            modules: plan.modules.len(),
            writes: plan.write_count(),
            syscalls: plan.syscall_count(),
            ..LinkSummary::default()
        };
        let Some(file) = self.link_plan_file(path) else {
            return summary;
        };
        let key = PlanKey::for_executable(bytes, orbistoun_env::build::line());
        let settled = if relink {
            orbistoun_loader::plan::relink(&file, &key, plan)
        } else {
            orbistoun_loader::plan::settle(&file, &key, plan)
        };
        match settled {
            Ok((standing, previous)) => {
                standing.word().clone_into(&mut summary.stored);
                if let Some(previous) = previous
                    && (relink || standing == Standing::Mismatch)
                {
                    let difference = previous.plan.differences(plan);
                    summary.differs =
                        describe_plan_difference(&difference, &title.thunks, &title.labels);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "the link plan could not be stored at {}: {e}",
                    file.display()
                );
            }
        }
        summary
    }

    /// Links a title as a run would, stores its plan, and enters nothing.
    ///
    /// The images are dropped on return, which unmaps them: only the plan outlives the call.
    ///
    /// # Errors
    ///
    /// When the executable cannot be read, placed or linked.
    pub fn link_ahead(
        &self,
        executable: &Path,
        executable_base: u64,
        bases: TitleBases,
        symbols: &SymbolDbFile,
        relink: bool,
    ) -> Result<LinkSummary, ServiceError> {
        let bytes = std::fs::read(executable).map_err(|source| ServiceError::Io {
            path: executable.display().to_string(),
            source,
        })?;
        let image = self.place_image(&bytes, executable_base)?;
        let title = self.link_title_modules(executable, bases, symbols)?;
        let linked = self.relocate_executable(&image, &bytes, &title, symbols, None)?;
        let plan = plan_of(&image, &bytes, linked.applied.writes, &title)?;
        Ok(self.settle_link_plan((executable, &bytes), &plan, &title, relink))
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_loader::plan::{PlanDifference, SlotDifference};

    /// A differing slot is named by the import whose stub either plan wrote there; a slot holding
    /// no stub is named by its module, and slots past the first few are counted.
    #[test]
    fn a_plan_difference_names_slots_by_their_import() {
        use orbistoun_mem::test_bases::{Range, crates};
        static RANGE: Range = Range::nth(crates::SERVICE);
        let thunks = orbistoun_thunk::ThunkTable::build(RANGE.take(), 2, 0x1000).expect("reserves");
        let labels = vec![
            "libkernel::sceKernelUsleep".to_owned(),
            "libc::malloc".to_owned(),
        ];
        let slot = |library: &str, at, stored, fresh| SlotDifference {
            library: library.to_owned(),
            at,
            stored,
            fresh,
        };
        let mut slots = vec![
            slot("", 0x10, thunks.address_of(0), thunks.address_of(1)),
            slot("libfoo", 0x20, None, Some(0x1234)),
        ];
        slots.extend(
            (0..super::PLAN_DIFFERENCES_NAMED as u64)
                .map(|i| slot("", 0x100 + i, Some(1), Some(2))),
        );
        let difference = PlanDifference {
            placements: vec![String::new()],
            syscalls: Vec::new(),
            slots,
        };
        let lines = super::describe_plan_difference(&difference, &thunks, &labels);
        assert_eq!(lines[0], "the executable is placed differently");
        assert!(
            lines[1].starts_with("libc::malloc at 0x10: stored 0x"),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2], "libfoo slot at 0x20: stored nothing, now 0x1234");
        assert_eq!(lines.last().map(String::as_str), Some("and 2 more slots"));
    }

    /// A module's title is the directory holding it.
    #[test]
    fn a_module_belongs_to_the_directory_holding_it() {
        assert_eq!(
            super::title_of(std::path::Path::new("titles/PPSA21564-app0/eboot.bin")),
            "PPSA21564-app0"
        );
    }
}
