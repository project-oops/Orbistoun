//! What the device was handed most recently, so a lost device (`VK_ERROR_DEVICE_LOST`) names the
//! guest work it was running rather than leaving it to be found by bisecting.
//!
//! Every queue submission takes a serial and an entry in a fixed ring: whether it held draws, a
//! dispatch or a copy, the programs that ran and their grid or draw counts. An entry is complete
//! once its fence, or a device wait after it, returned. The first loss anywhere logs the latest
//! entries with the incomplete ones marked, what `VK_EXT_device_fault` reports where the device
//! has it, and calls the ending the worker installs.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use ash::vk;
use orbistoun_gpu::ResourceId;

use crate::compute::DispatchError;

/// Submissions the ring keeps.
const KEPT: usize = 32;
/// Submissions a loss logs, the most recent last.
const LOGGED: usize = 16;
/// Programs an entry names, the most recently noted kept.
const PROGRAMS: usize = 4;

/// A program a submission ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Program {
    /// A guest compute shader, by its address.
    Compute(u64),
    /// A module made resident for the backend, named at a loss by the guest shader the pipeline
    /// prepared it from ([`orbistoun_gpu::pipeline::shader_name`]).
    Module(ResourceId),
}

/// What a submission held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A pass of draws.
    Draws,
    /// One compute dispatch.
    Dispatch,
    /// A copy, upload or read-back the host made for itself.
    Copy,
}

/// The guest work noted for a submission not yet made, or held by one that was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Work {
    /// The dispatch's grid, in workgroups.
    groups: Option<[u32; 3]>,
    /// Draws carried out.
    draws: u32,
    /// Vertices or indices over every draw and instance.
    elements: u64,
    /// The latest distinct programs, oldest first.
    programs: [Option<Program>; PROGRAMS],
    /// Programs pushed out of `programs` by later ones.
    dropped: u32,
}

impl Work {
    const NONE: Self = Self {
        groups: None,
        draws: 0,
        elements: 0,
        programs: [None; PROGRAMS],
        dropped: 0,
    };

    /// Keeps `program` as the latest, moving it there if it is already kept.
    fn name(&mut self, program: Program) {
        if let Some(at) = self.programs.iter().position(|p| *p == Some(program)) {
            self.programs[at..].rotate_left(1);
            self.programs[PROGRAMS - 1] = None;
        } else if self.programs[PROGRAMS - 1].is_some() {
            self.programs.rotate_left(1);
            self.programs[PROGRAMS - 1] = None;
            self.dropped += 1;
        }
        let free = self.programs.iter().position(Option::is_none);
        if let Some(free) = free {
            self.programs[free] = Some(program);
        }
    }
}

/// One submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    serial: u64,
    kind: Kind,
    work: Work,
    complete: bool,
}

/// The latest submissions, and the work noted for the next ones.
struct Ring {
    entries: [Option<Entry>; KEPT],
    /// The serial the next submission takes.
    next: u64,
    /// Draws noted since the last pass was submitted.
    draws: Work,
    /// The dispatch noted for the next dispatch submission.
    dispatch: Work,
}

impl Ring {
    const fn new() -> Self {
        Self {
            entries: [None; KEPT],
            next: 1,
            draws: Work::NONE,
            dispatch: Work::NONE,
        }
    }

    fn note_dispatch(&mut self, program: Program, groups: [u32; 3]) {
        self.dispatch = Work::NONE;
        self.dispatch.groups = Some(groups);
        self.dispatch.name(program);
    }

    fn note_draws(&mut self, programs: [ResourceId; 2], draws: u32, elements: u64) {
        self.draws.draws = self.draws.draws.saturating_add(draws);
        self.draws.elements = self.draws.elements.saturating_add(elements);
        for program in programs {
            self.draws.name(Program::Module(program));
        }
    }

    /// Records a submission of `kind`, taking the work noted for it, and answers its serial.
    fn submit(&mut self, kind: Kind) -> u64 {
        let work = match kind {
            Kind::Draws => std::mem::replace(&mut self.draws, Work::NONE),
            Kind::Dispatch => std::mem::replace(&mut self.dispatch, Work::NONE),
            Kind::Copy => Work::NONE,
        };
        let serial = self.next;
        self.next += 1;
        self.entries[slot(serial)] = Some(Entry {
            serial,
            kind,
            work,
            complete: false,
        });
        serial
    }

    fn complete(&mut self, serial: u64) {
        if let Some(entry) = &mut self.entries[slot(serial)]
            && entry.serial == serial
        {
            entry.complete = true;
        }
    }

    fn complete_all(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.complete = true;
        }
    }

    /// The latest `count` entries, oldest first.
    fn latest(&self, count: usize) -> impl Iterator<Item = &Entry> {
        let first = self.next.saturating_sub(count.min(KEPT) as u64).max(1);
        (first..self.next).filter_map(|serial| {
            self.entries[slot(serial)]
                .as_ref()
                .filter(|entry| entry.serial == serial)
        })
    }
}

/// Where `serial` sits in the ring.
fn slot(serial: u64) -> usize {
    usize::try_from(serial % KEPT as u64).unwrap_or_default()
}

fn ring() -> std::sync::MutexGuard<'static, Ring> {
    static RING: Mutex<Ring> = Mutex::new(Ring::new());
    RING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Notes the guest compute shader at `program` and its grid for the next dispatch submitted.
pub fn note_dispatch(program: u64, groups: [u32; 3]) {
    ring().note_dispatch(Program::Compute(program), groups);
}

/// The guest compute shader noted for the next dispatch, if one was: what a compute pipeline
/// compiled before that dispatch is submitted was translated from.
pub(crate) fn noted_compute() -> Option<u64> {
    ring()
        .dispatch
        .programs
        .iter()
        .rev()
        .flatten()
        .find_map(|program| match program {
            Program::Compute(address) => Some(*address),
            Program::Module(_) => None,
        })
}

/// Notes the compute module `module` and its grid for the next dispatch submitted.
pub(crate) fn note_module_dispatch(module: ResourceId, groups: [u32; 3]) {
    ring().note_dispatch(Program::Module(module), groups);
}

/// Notes a run of draws through the `vertex` and `fragment` modules for the next pass submitted.
pub(crate) fn note_draws(modules: [ResourceId; 2], draws: u32, elements: u64) {
    ring().note_draws(modules, draws, elements);
}

/// Records a submission of `kind` about to be made, answering its serial.
pub(crate) fn submit(kind: Kind) -> u64 {
    ring().submit(kind)
}

/// Records that the submission `serial` completed: its fence signalled.
pub(crate) fn complete(serial: u64) {
    ring().complete(serial);
}

/// Waits for the device to be idle, and records every submission so far complete.
pub(crate) fn wait_idle(device: &ash::Device) -> Result<(), DispatchError> {
    // SAFETY: the device is live; the caller holds the session, so nothing submits meanwhile.
    unsafe { device.device_wait_idle() }.map_err(vulkan("device_wait_idle"))?;
    ring().complete_all();
    Ok(())
}

/// The error for the Vulkan call `stage` failing with a result; a lost device is reported the
/// first time one is met.
pub(crate) fn vulkan(stage: &'static str) -> impl FnOnce(vk::Result) -> DispatchError {
    move |result| {
        if result == vk::Result::ERROR_DEVICE_LOST {
            lost(stage);
        }
        DispatchError::Vulkan(stage, result)
    }
}

/// Whether the device has been lost.
static LOST: AtomicBool = AtomicBool::new(false);

/// What ends the run once the device is lost.
static ENDING: OnceLock<fn()> = OnceLock::new();

/// The reader of `VK_EXT_device_fault`'s report, where the device was created with it.
static FAULT: OnceLock<ash::ext::device_fault::Device> = OnceLock::new();

/// Whether the device has been lost: every later submission fails.
#[must_use]
pub fn device_lost() -> bool {
    LOST.load(Ordering::Relaxed)
}

/// Installs what ends the run once the device is lost. It is called once, after the loss is
/// logged, on the thread that met it while that thread holds the device: it must not touch the
/// device, and does well to end the run from another thread. First install wins.
pub fn on_device_lost(ending: fn()) {
    let _ = ENDING.set(ending);
}

/// Keeps the reader of the device's fault report: the device was created with
/// `VK_EXT_device_fault` and its `deviceFault` feature.
pub(crate) fn keep_fault_reader(instance: &ash::Instance, device: &ash::Device) {
    let _ = FAULT.set(ash::ext::device_fault::Device::new(instance, device));
    tracing::debug!("VK_EXT_device_fault enabled: a lost device reports what faulted");
}

/// Reports the loss met at `stage`, once.
fn lost(stage: &'static str) {
    if LOST.swap(true, Ordering::SeqCst) {
        return;
    }
    let submissions = {
        let ring = ring();
        describe(stage, ring.latest(LOGGED), &name_program)
    };
    tracing::error!("{submissions}");
    if let Some(reader) = FAULT.get() {
        tracing::error!("{}", read_fault(reader));
    }
    if HELD.load(Ordering::SeqCst) {
        DEFERRED.store(true, Ordering::SeqCst);
    } else {
        end();
    }
}

/// Calls the installed ending, if any.
fn end() {
    if let Some(ending) = ENDING.get() {
        ending();
    }
}

/// Whether a dispatch holds the ending back: it still has the inputs a capture of the loss needs.
static HELD: AtomicBool = AtomicBool::new(false);

/// Whether a loss met while the ending was held back still has to end the run.
static DEFERRED: AtomicBool = AtomicBool::new(false);

/// Holds the run's ending back while it lives: a loss met meanwhile ends the run when it drops, so
/// the dispatch that met it can first write itself out (`capture`). Dispatches are made one at a
/// time under the session, so one is held at a time.
#[derive(Debug)]
pub(crate) struct EndingHeld(());

/// Holds the run's ending back until the answer drops.
pub(crate) fn hold_ending() -> EndingHeld {
    HELD.store(true, Ordering::SeqCst);
    EndingHeld(())
}

impl Drop for EndingHeld {
    fn drop(&mut self) {
        HELD.store(false, Ordering::SeqCst);
        if DEFERRED.swap(false, Ordering::SeqCst) {
            end();
        }
    }
}

/// A program as the draw-program listings name it: `compute-74005b87ce00`, `vertex-74006000`.
fn name_program(program: Program) -> String {
    match program {
        Program::Compute(address) => format!("compute-{address:x}"),
        Program::Module(id) => {
            orbistoun_gpu::pipeline::shader_name(id).unwrap_or_else(|| format!("module {}", id.0))
        }
    }
}

/// The loss at `stage` and the submissions before it, one line each, oldest first, `*` on each not
/// known complete.
fn describe<'a>(
    stage: &str,
    entries: impl Iterator<Item = &'a Entry>,
    name: &dyn Fn(Program) -> String,
) -> String {
    let mut text = format!(
        concat!(
            "the graphics device was lost (VK_ERROR_DEVICE_LOST from {stage}); its latest ",
            "submissions, oldest first, * where not known complete:"
        ),
        stage = stage
    );
    let mut running = 0;
    for entry in entries {
        running += usize::from(!entry.complete);
        let mark = if entry.complete { ' ' } else { '*' };
        let _ = write!(text, "\n  {mark} #{} {}", entry.serial, held(entry, name));
    }
    if running == 0 {
        text.push_str("\n  every submission had completed");
    }
    text
}

/// What one submission held, in words.
fn held(entry: &Entry, name: &dyn Fn(Program) -> String) -> String {
    let work = &entry.work;
    let mut text = match entry.kind {
        Kind::Copy => return "copy".to_owned(),
        Kind::Dispatch => match work.groups {
            Some([x, y, z]) => format!("dispatch {x}x{y}x{z} groups"),
            None => "dispatch".to_owned(),
        },
        Kind::Draws => format!("{} draws, {} elements", work.draws, work.elements),
    };
    let programs: Vec<String> = work.programs.iter().flatten().map(|p| name(*p)).collect();
    if programs.is_empty() {
        text.push_str(", program not noted");
    } else {
        let _ = write!(text, ": {}", programs.join(" "));
    }
    if work.dropped > 0 {
        let _ = write!(text, " (and {} before)", work.dropped);
    }
    text
}

/// What the device says of its fault through `VK_EXT_device_fault`, in words.
fn read_fault(reader: &ash::ext::device_fault::Device) -> String {
    let get = reader.fp().get_device_fault_info_ext;
    let mut counts = vk::DeviceFaultCountsEXT::default();
    // SAFETY: the device was created with the extension and its feature; a null info asks only for
    // the counts.
    let counted = unsafe { get(reader.device(), &raw mut counts, std::ptr::null_mut()) };
    if counted != vk::Result::SUCCESS && counted != vk::Result::INCOMPLETE {
        return format!("the device's fault report could not be read ({counted:?})");
    }
    let mut addresses =
        vec![vk::DeviceFaultAddressInfoEXT::default(); counts.address_info_count as usize];
    let mut vendors =
        vec![vk::DeviceFaultVendorInfoEXT::default(); counts.vendor_info_count as usize];
    // The vendor's binary dump needs a feature of its own, not enabled.
    counts.vendor_binary_size = 0;
    let mut info = vk::DeviceFaultInfoEXT {
        p_address_infos: addresses.as_mut_ptr(),
        p_vendor_infos: vendors.as_mut_ptr(),
        ..Default::default()
    };
    // SAFETY: the arrays hold the counts just read, set in `counts`, and outlive the call; no
    // binary is asked for.
    let read = unsafe { get(reader.device(), &raw mut counts, &raw mut info) };
    if read != vk::Result::SUCCESS && read != vk::Result::INCOMPLETE {
        return format!("the device's fault report could not be read ({read:?})");
    }
    let addresses: Vec<(vk::DeviceFaultAddressTypeEXT, u64, u64)> = addresses
        .iter()
        .take(counts.address_info_count as usize)
        .map(|a| (a.address_type, a.reported_address, a.address_precision))
        .collect();
    let vendors: Vec<(String, u64, u64)> = vendors
        .iter()
        .take(counts.vendor_info_count as usize)
        .map(|v| {
            (
                text_of(&v.description),
                v.vendor_fault_code,
                v.vendor_fault_data,
            )
        })
        .collect();
    describe_fault(&text_of(&info.description), &addresses, &vendors)
}

/// A fixed, nul-terminated description as text.
fn text_of(chars: &[core::ffi::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| c.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The device's fault report: its description, each address it names and what kind of access
/// faulted there, and each vendor record.
fn describe_fault(
    description: &str,
    addresses: &[(vk::DeviceFaultAddressTypeEXT, u64, u64)],
    vendors: &[(String, u64, u64)],
) -> String {
    let mut text = format!("the device's fault report: {description}");
    if addresses.is_empty() && vendors.is_empty() {
        text.push_str("\n  it names no address and no vendor record");
    }
    for &(kind, address, precision) in addresses {
        // The address is exact to its precision, a power of two (VkDeviceFaultAddressInfoEXT).
        let low = address & !precision.saturating_sub(1);
        let high = address | precision.saturating_sub(1);
        let _ = write!(
            text,
            "\n  {} at {address:#x} (within {low:#x}..={high:#x})",
            address_kind(kind)
        );
    }
    for (description, code, data) in vendors {
        let _ = write!(
            text,
            "\n  vendor: {description} (code {code:#x}, data {data:#x})"
        );
    }
    text
}

/// What a fault address names, in words.
fn address_kind(kind: vk::DeviceFaultAddressTypeEXT) -> &'static str {
    match kind {
        vk::DeviceFaultAddressTypeEXT::NONE => "no address",
        vk::DeviceFaultAddressTypeEXT::READ_INVALID => "an invalid read",
        vk::DeviceFaultAddressTypeEXT::WRITE_INVALID => "an invalid write",
        vk::DeviceFaultAddressTypeEXT::EXECUTE_INVALID => "an invalid execute",
        vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_UNKNOWN => "an instruction pointer",
        vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_INVALID => {
            "the instruction pointer of an invalid instruction"
        }
        vk::DeviceFaultAddressTypeEXT::INSTRUCTION_POINTER_FAULT => {
            "the instruction pointer of a fault"
        }
        _ => "an address of an unnamed kind",
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Kind, Program, Ring, describe, describe_fault, held};
    use ash::vk;
    use orbistoun_gpu::ResourceId;

    fn names(program: Program) -> String {
        match program {
            Program::Compute(address) => format!("compute-{address:x}"),
            Program::Module(id) => format!("module-{}", id.0),
        }
    }

    fn running(ring: &Ring) -> Vec<u64> {
        ring.latest(super::KEPT)
            .filter(|entry| !entry.complete)
            .map(|entry| entry.serial)
            .collect()
    }

    /// A submission is in flight until its own fence or a device wait completes it; completing one
    /// leaves the others in flight.
    #[test]
    fn a_submission_is_in_flight_until_it_is_completed() {
        let mut ring = Ring::new();
        let first = ring.submit(Kind::Draws);
        let second = ring.submit(Kind::Dispatch);
        let third = ring.submit(Kind::Copy);
        assert_eq!(running(&ring), [first, second, third]);
        ring.complete(second);
        assert_eq!(running(&ring), [first, third]);
        ring.complete_all();
        assert!(running(&ring).is_empty());
        let fourth = ring.submit(Kind::Draws);
        assert_eq!(
            running(&ring),
            [fourth],
            "a wait completes only what came before it"
        );
    }

    /// The ring keeps the latest submissions, oldest first, and completing a serial it no longer
    /// holds marks nothing.
    #[test]
    fn the_ring_keeps_the_latest_submissions_in_order() {
        let mut ring = Ring::new();
        let serials: Vec<u64> = (0..super::KEPT + 5)
            .map(|_| ring.submit(Kind::Copy))
            .collect();
        let kept: Vec<u64> = ring.latest(super::KEPT).map(|entry| entry.serial).collect();
        assert_eq!(kept, serials[5..]);
        let latest: Vec<u64> = ring.latest(3).map(|entry| entry.serial).collect();
        assert_eq!(latest, serials[serials.len() - 3..]);
        ring.complete(serials[0]);
        assert_eq!(
            running(&ring).len(),
            super::KEPT,
            "an evicted serial marks nothing"
        );
    }

    /// A submission takes the work noted for its kind and no other: a dispatch does not carry away
    /// the draws of a pass still open, and a pass collects every draw noted since the last.
    #[test]
    fn a_submission_takes_the_work_noted_for_its_kind() {
        let mut ring = Ring::new();
        ring.note_draws([ResourceId(1), ResourceId(2)], 3, 300);
        ring.note_dispatch(Program::Compute(0x7400_5b87_ce00), [64, 1, 1]);
        let dispatch = ring.submit(Kind::Dispatch);
        ring.note_draws([ResourceId(1), ResourceId(3)], 1, 6);
        let pass = ring.submit(Kind::Draws);
        let entries: Vec<&Entry> = ring.latest(2).collect();
        assert_eq!(entries[0].serial, dispatch);
        assert_eq!(
            held(entries[0], &names),
            "dispatch 64x1x1 groups: compute-74005b87ce00"
        );
        assert_eq!(entries[1].serial, pass);
        assert_eq!(
            held(entries[1], &names),
            "4 draws, 306 elements: module-2 module-1 module-3"
        );
        let next = ring.submit(Kind::Dispatch);
        let entry = ring.latest(1).next().expect("kept");
        assert_eq!(entry.serial, next);
        assert_eq!(
            held(entry, &names),
            "dispatch, program not noted",
            "a note is taken once"
        );
    }

    /// An entry names its latest programs and counts those pushed out before them.
    #[test]
    fn an_entry_names_its_latest_programs() {
        let mut ring = Ring::new();
        for id in 1..=6 {
            ring.note_draws([ResourceId(id), ResourceId(100)], 1, 3);
        }
        ring.submit(Kind::Draws);
        let entry = ring.latest(1).next().expect("kept");
        assert_eq!(
            held(entry, &names),
            "6 draws, 18 elements: module-4 module-5 module-6 module-100 (and 3 before)"
        );
    }

    /// The loss is logged with the stage that met it and one line per submission, the ones not
    /// known complete marked, and says so when none was running.
    #[test]
    fn the_loss_lists_the_submissions_marking_those_in_flight() {
        let mut ring = Ring::new();
        ring.submit(Kind::Copy);
        ring.complete_all();
        ring.note_dispatch(Program::Compute(0x4000_03ce_6000), [8, 4, 1]);
        ring.submit(Kind::Dispatch);
        let text = describe("wait_for_fences", ring.latest(super::LOGGED), &names);
        assert_eq!(
            text,
            concat!(
                "the graphics device was lost (VK_ERROR_DEVICE_LOST from wait_for_fences); its ",
                "latest submissions, oldest first, * where not known complete:\n    #1 copy\n",
                "  * #2 dispatch 8x4x1 groups: compute-400003ce6000"
            )
        );
        ring.complete_all();
        let text = describe("device_wait_idle", ring.latest(super::LOGGED), &names);
        assert!(
            text.ends_with("\n  every submission had completed"),
            "{text}"
        );
    }

    /// The fault report names each address with its kind and the range its precision allows, and
    /// each vendor record with its code and data.
    #[test]
    fn the_fault_report_names_addresses_and_vendor_records() {
        let text = describe_fault(
            "page fault",
            &[(
                vk::DeviceFaultAddressTypeEXT::READ_INVALID,
                0x1234_5678,
                0x1000,
            )],
            &[("MMU fault".to_owned(), 0x1f, 0x2a)],
        );
        assert_eq!(
            text,
            concat!(
                "the device's fault report: page fault\n  an invalid read at 0x12345678 (within ",
                "0x12345000..=0x12345fff)\n  vendor: MMU fault (code 0x1f, data 0x2a)"
            )
        );
        let empty = describe_fault("hang", &[], &[]);
        assert!(empty.ends_with("it names no address and no vendor record"));
    }
}
