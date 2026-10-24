//! The device memory this process holds, by what it is held for.
//!
//! The driver reports every allocation it makes and frees (`VK_EXT_device_memory_report`), its
//! own for pipelines and pools included. An allocation this crate makes is named by the outermost
//! `purpose` in force on the allocating thread. A Windows driver charges device memory, local
//! memory included, to the process's private bytes, so this is most of what the process holds
//! beyond its heap there. A device without the report is measured whole through
//! `VK_EXT_memory_budget`.
//!
//! The report arrives inside the driver's own calls, on whatever thread made them and with
//! whatever that thread holds, the driver's locks among them; and the census reads it on whatever
//! thread closes a perf window. So both sides touch only atomics: the bytes by name, and the live
//! allocations in a fixed table of them, never a lock.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use ash::vk;

thread_local! {
    /// What the allocations made on this thread now are for, as its slot in [`NAMES`].
    static PURPOSE: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The name an allocation made while no purpose is in force is held under.
const UNNAMED: &str = "device memory not named";

/// The names the driver's own allocations are held under, by the kind of object they serve, and
/// [`UNNAMED`]: the first slots of [`NAMES`].
const FIXED: [&str; 5] = [
    UNNAMED,
    "driver's own for pipelines",
    "driver's own for command buffers",
    "driver's own for descriptors",
    "driver's own, other",
];

/// How many names allocations are held under, at most: the fixed ones and every purpose named.
const NAMES: usize = 64;

/// The purposes named, each in the slot it was first given; the fixed names' slots stay empty.
static NAMED: [OnceLock<&'static str>; NAMES] = [const { OnceLock::new() }; NAMES];

/// The next slot a purpose not named before takes.
static NEXT_NAME: AtomicUsize = AtomicUsize::new(FIXED.len());

/// The bytes held under each slot's name.
static BYTES: [AtomicU64; NAMES] = [const { AtomicU64::new(0) }; NAMES];

/// The name in `slot`, once it has one.
fn name_of(slot: usize) -> Option<&'static str> {
    FIXED
        .get(slot)
        .copied()
        .or_else(|| NAMED.get(slot)?.get().copied())
}

/// The slot `name` is held under: the one it was given first, else the next free one, else
/// [`UNNAMED`]'s once every slot is taken. Two threads naming one purpose at once may give it two
/// slots, which [`held`] adds together.
fn slot_of(name: &'static str) -> usize {
    let named = NEXT_NAME.load(Ordering::Acquire).min(NAMES);
    if let Some(slot) = (0..named).find(|&slot| name_of(slot) == Some(name)) {
        return slot;
    }
    let slot = NEXT_NAME.fetch_add(1, Ordering::AcqRel);
    match NAMED.get(slot) {
        Some(cell) => {
            let _ = cell.set(name);
            slot
        }
        None => 0,
    }
}

/// What `purpose` names until the guard drops. The outermost purpose wins, so a caller that knows
/// what a generic helper allocates for names it over the helper's own default.
#[must_use]
pub(crate) fn purpose(name: &'static str) -> Purpose {
    let outer = PURPOSE.with(Cell::get);
    if outer.is_none() {
        let slot = slot_of(name);
        PURPOSE.with(|purpose| purpose.set(Some(slot)));
    }
    Purpose { outer }
}

/// Keeps a [`purpose`] in force while it lives.
pub(crate) struct Purpose {
    outer: Option<usize>,
}

impl Drop for Purpose {
    fn drop(&mut self) {
        if self.outer.is_none() {
            PURPOSE.with(|purpose| purpose.set(None));
        }
    }
}

/// The slot the driver's own allocation for an object of `kind` is held under.
fn driver_own(kind: vk::ObjectType) -> usize {
    match kind {
        vk::ObjectType::PIPELINE
        | vk::ObjectType::SHADER_MODULE
        | vk::ObjectType::PIPELINE_CACHE => 1,
        vk::ObjectType::COMMAND_POOL | vk::ObjectType::COMMAND_BUFFER => 2,
        vk::ObjectType::DESCRIPTOR_POOL | vk::ObjectType::DESCRIPTOR_SET => 3,
        _ => 4,
    }
}

/// The live allocations, by the driver's id for them, each with its slot and bytes: an
/// open-addressed table of atomics, so a free finds what its allocation was counted under without
/// a lock.
mod live {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// How many allocations can be live at once and found again; one past that is counted and
    /// never let go of.
    const OBJECTS: usize = 1 << 17;
    /// How far a lookup goes from where its key belongs.
    const PROBES: usize = 4096;
    /// A slot never used, which ends a lookup.
    const EMPTY: u64 = 0;
    /// A slot let go of, which a lookup passes over and a new allocation takes again.
    const GONE: u64 = 1;

    static KEYS: [AtomicU64; OBJECTS] = [const { AtomicU64::new(EMPTY) }; OBJECTS];
    static HELD: [AtomicU64; OBJECTS] = [const { AtomicU64::new(0) }; OBJECTS];

    /// The table's key for the driver's `id`: never [`EMPTY`] or [`GONE`], which two ids share
    /// with others and are then held under a neighbour's key.
    fn key(id: u64) -> u64 {
        match id ^ 0x5bd1_e995_9e37_79b9 {
            EMPTY | GONE => 2,
            key => key,
        }
    }

    fn home(key: u64) -> usize {
        (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 32) as usize % OBJECTS
    }

    /// Records `held` for `id`, answering what it replaced; `None` too when the table has no room
    /// within reach, and the allocation is then never let go of.
    pub(super) fn insert(id: u64, held: u64) -> Option<u64> {
        let key = key(id);
        let start = home(key);
        let mut grave = None;
        let mut probe = 0;
        while probe < PROBES {
            let at = (start + probe) % OBJECTS;
            match KEYS[at].load(Ordering::Acquire) {
                found if found == key => return Some(HELD[at].swap(held, Ordering::AcqRel)),
                GONE => {
                    grave.get_or_insert(at);
                }
                EMPTY => {
                    let into = grave.unwrap_or(at);
                    let was = if into == at { EMPTY } else { GONE };
                    if KEYS[into]
                        .compare_exchange(was, key, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        HELD[into].store(held, Ordering::Release);
                        return None;
                    }
                    // Another allocation took it meanwhile: this slot is looked at again, so no
                    // empty slot is passed over - a lookup stops at the first.
                    grave = None;
                    continue;
                }
                _ => {}
            }
            probe += 1;
        }
        // No empty slot within reach: the first one let go of, where there is one.
        let into = grave?;
        KEYS[into]
            .compare_exchange(GONE, key, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        HELD[into].store(held, Ordering::Release);
        None
    }

    /// Forgets `id`, answering what it held.
    pub(super) fn remove(id: u64) -> Option<u64> {
        let key = key(id);
        let start = home(key);
        for probe in 0..PROBES {
            let at = (start + probe) % OBJECTS;
            match KEYS[at].load(Ordering::Acquire) {
                found if found == key => {
                    let held = HELD[at].swap(0, Ordering::AcqRel);
                    KEYS[at].store(GONE, Ordering::Release);
                    return Some(held);
                }
                EMPTY => return None,
                _ => {}
            }
        }
        None
    }
}

/// How many low bits of a live allocation's record hold its bytes; its slot is above them.
const BYTES_BITS: u32 = 56;

fn record(slot: usize, bytes: u64) -> u64 {
    ((slot as u64) << BYTES_BITS) | (bytes & ((1 << BYTES_BITS) - 1))
}

/// Takes what a live allocation's `record` counted back out of its slot's bytes.
fn uncount(record: u64) {
    let slot = (record >> BYTES_BITS) as usize;
    let bytes = record & ((1 << BYTES_BITS) - 1);
    if let Some(held) = BYTES.get(slot) {
        let _ = held.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |held| {
            Some(held.saturating_sub(bytes))
        });
    }
}

/// Records one reported event. Called from inside the driver: touches only atomics and the
/// calling thread's purpose.
fn note(event: vk::DeviceMemoryReportEventTypeEXT, id: u64, bytes: u64, kind: vk::ObjectType) {
    match event {
        vk::DeviceMemoryReportEventTypeEXT::ALLOCATE
        | vk::DeviceMemoryReportEventTypeEXT::IMPORT => {
            let slot = if kind == vk::ObjectType::DEVICE_MEMORY {
                PURPOSE.with(Cell::get).unwrap_or(0)
            } else {
                driver_own(kind)
            };
            if let Some(was) = live::insert(id, record(slot, bytes)) {
                uncount(was);
            }
            BYTES[slot].fetch_add(bytes, Ordering::Relaxed);
        }
        vk::DeviceMemoryReportEventTypeEXT::FREE | vk::DeviceMemoryReportEventTypeEXT::UNIMPORT => {
            if let Some(was) = live::remove(id) {
                uncount(was);
            }
        }
        _ => {}
    }
}

/// The driver's report of an allocation or a free.
unsafe extern "system" fn reported(
    data: *const vk::DeviceMemoryReportCallbackDataEXT<'_>,
    _user: *mut core::ffi::c_void,
) {
    // SAFETY: the driver passes a valid callback structure for the duration of the call.
    let Some(data) = (unsafe { data.as_ref() }) else {
        return;
    };
    note(data.ty, data.memory_object_id, data.size, data.object_type);
}

/// Whether the device offers the report, and the budget.
pub(crate) fn offered(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    extensions: &[vk::ExtensionProperties],
) -> (bool, bool) {
    (
        report_offered(instance, physical, extensions),
        budget_offered(extensions),
    )
}

/// Whether `extensions` offer the report, and the device's features include it.
fn report_offered(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    extensions: &[vk::ExtensionProperties],
) -> bool {
    let named = extensions.iter().any(|extension| {
        extension
            .extension_name_as_c_str()
            .is_ok_and(|name| name == ash::ext::device_memory_report::NAME)
    });
    if !named {
        return false;
    }
    let mut report = vk::PhysicalDeviceDeviceMemoryReportFeaturesEXT::default();
    let mut chained = vk::PhysicalDeviceFeatures2::default().push_next(&mut report);
    // SAFETY: the handle came from enumeration on this live instance, and the chained structure
    // outlives the call.
    unsafe { instance.get_physical_device_features2(physical, &mut chained) };
    report.device_memory_report == vk::TRUE
}

/// Whether `extensions` offer the budget.
fn budget_offered(extensions: &[vk::ExtensionProperties]) -> bool {
    extensions.iter().any(|extension| {
        extension
            .extension_name_as_c_str()
            .is_ok_and(|name| name == ash::ext::memory_budget::NAME)
    })
}

/// The report's create info, to chain onto the device's when [`report_offered`]: every event then
/// reaches [`reported`].
pub(crate) fn report_info() -> vk::DeviceDeviceMemoryReportCreateInfoEXT<'static> {
    vk::DeviceDeviceMemoryReportCreateInfoEXT::default().pfn_user_callback(Some(reported))
}

/// Whether the device was created with the report, so [`held`] can trust [`BYTES`].
static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The instance and physical device the budget is asked of, where the device was created with
/// it, which [`held`] measures by where there is no report. The session they belong to is never
/// destroyed, and asking the budget synchronises on nothing of ours.
static BUDGET: OnceLock<(ash::Instance, vk::PhysicalDevice)> = OnceLock::new();

/// Records whether the device was created with the report and with the budget, and what the
/// budget is asked of.
pub(crate) fn note_enabled(
    report: bool,
    budget: bool,
    (instance, physical): (&ash::Instance, vk::PhysicalDevice),
) {
    REPORTED.store(report, Ordering::Relaxed);
    if budget {
        let _ = BUDGET.set((instance.clone(), physical));
    }
}

/// The device memory the process holds, by what it is held for, largest first; empty before the
/// device opens or where the driver says nothing. Takes no lock.
#[must_use]
pub fn held() -> Vec<(&'static str, u64)> {
    if REPORTED.load(Ordering::Relaxed) {
        return by_name();
    }
    budgeted()
        .map(|bytes| vec![("device memory", bytes)])
        .unwrap_or_default()
}

/// The bytes held under each name, largest first.
fn by_name() -> Vec<(&'static str, u64)> {
    let mut held: Vec<(&'static str, u64)> = (0..NAMES)
        .filter_map(|slot| Some((name_of(slot)?, BYTES[slot].load(Ordering::Relaxed))))
        .collect();
    // A name given two slots is one holder.
    held.sort_by(|a, b| a.0.cmp(b.0));
    held.dedup_by(|later, kept| {
        let same = later.0 == kept.0;
        if same {
            kept.1 += later.1;
        }
        same
    });
    held.retain(|&(_, bytes)| bytes > 0);
    held.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    held
}

/// The device memory the driver counts this process as using on every heap, where it offers the
/// budget.
fn budgeted() -> Option<u64> {
    let (instance, physical) = BUDGET.get()?;
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
    // SAFETY: the physical device is the never-destroyed session's, valid for the process, and
    // the chained structure outlives the call.
    unsafe {
        instance.get_physical_device_memory_properties2(*physical, &mut properties);
    }
    let heaps = properties.memory_properties.memory_heap_count as usize;
    Some(budget.heap_usage.iter().take(heaps).sum())
}

#[cfg(test)]
mod tests {
    use ash::vk;

    fn named(name: &str) -> u64 {
        super::by_name()
            .into_iter()
            .find(|&(held, _)| held == name)
            .map_or(0, |(_, bytes)| bytes)
    }

    /// An allocation is held under the purpose in force when it was made, the outermost one when
    /// they nest, and is let go of when freed; the driver's own are held by the object they serve.
    #[test]
    fn an_allocation_is_held_under_its_purpose_until_freed() {
        let alloc = vk::DeviceMemoryReportEventTypeEXT::ALLOCATE;
        let free = vk::DeviceMemoryReportEventTypeEXT::FREE;
        let memory = vk::ObjectType::DEVICE_MEMORY;
        {
            let _outer = super::purpose("test targets");
            let _inner = super::purpose("test helper");
            super::note(alloc, u64::MAX - 1, 300, memory);
        }
        super::note(alloc, u64::MAX - 2, 7, vk::ObjectType::PIPELINE);
        assert_eq!(named("test targets"), 300);
        assert_eq!(named("test helper"), 0, "the outer purpose names it");
        assert!(named("driver's own for pipelines") >= 7);
        super::note(free, u64::MAX - 1, 0, memory);
        super::note(free, u64::MAX - 2, 0, vk::ObjectType::PIPELINE);
        assert_eq!(named("test targets"), 0);
        {
            let _helper = super::purpose("test helper");
            super::note(alloc, u64::MAX - 3, 5, memory);
        }
        assert_eq!(named("test helper"), 5, "a purpose ends with its guard");
        super::note(free, u64::MAX - 3, 0, memory);
    }

    /// Allocations made and freed from many threads at once, as a driver reports them, are each
    /// found again when freed, however many come and go.
    #[test]
    fn allocations_from_many_threads_are_all_let_go() {
        const THREADS: u64 = 8;
        const EACH: u64 = 20_000;
        let alloc = vk::DeviceMemoryReportEventTypeEXT::ALLOCATE;
        let free = vk::DeviceMemoryReportEventTypeEXT::FREE;
        let threads: Vec<_> = (0..THREADS)
            .map(|thread| {
                std::thread::spawn(move || {
                    let _for = super::purpose("test churn");
                    for round in 0..EACH {
                        let id = (1 << 40) + thread * EACH + round;
                        super::note(alloc, id, 3, vk::ObjectType::DEVICE_MEMORY);
                        if round % 2 == 1 {
                            super::note(free, id - 1, 0, vk::ObjectType::DEVICE_MEMORY);
                            super::note(free, id, 0, vk::ObjectType::DEVICE_MEMORY);
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("a reporting thread");
        }
        assert_eq!(named("test churn"), 0);
    }

    /// A render target made on a device that reports its allocations is held under render
    /// targets: its image at least, its pixels passing through staging lent for each copy. Skipped,
    /// and says so, where no device is present or it does not report.
    #[test]
    fn a_device_reports_a_render_target_under_its_purpose() {
        if crate::compute::session().is_err()
            || !super::REPORTED.load(std::sync::atomic::Ordering::Relaxed)
        {
            eprintln!("no Vulkan device that reports its allocations: not exercised here");
            return;
        }
        let targets = || {
            super::held()
                .into_iter()
                .find(|&(name, _)| name == "render targets")
                .map_or(0, |(_, bytes)| bytes)
        };
        let side = 512;
        let resident = crate::framebuffer::ResidentAttachment::create(
            (side, side),
            None,
            [0.0; 4],
            orbistoun_gpu::TargetEncoding::Unorm8,
        )
        .expect("an attachment");
        let held = targets();
        resident.destroy();
        assert!(
            held >= u64::from(side * side * 4),
            "{held} bytes held for render targets"
        );
    }
}
