//! Where a thread actually is, sampled (worklog 852).
//!
//! The perf phases measure orbistoun's own work, and everything they do not measure has been
//! reported as the guest's. That is a remainder, not a measurement. This asks the threads directly:
//! about once a millisecond each watched thread is suspended, its instruction pointer and the top
//! of its stack read, and it is resumed.
//!
//! - A sample in the guest's image is counted by offset, which a disassembly or link map names.
//! - A sample in host code is counted by module and offset.
//! - A sample in a system library, most often a wait, is named by the first return address into
//!   this executable on its stack, so a wait says whose it is.
//!
//! `ORBISTOUN_PROFILE=1` turns it on: the thread entering the guest and the device thread are
//! watched, and the counts are printed every few seconds, per thread. The sampler never allocates
//! or takes a lock while a thread is suspended - that thread may hold the heap's lock, and waiting
//! on it from here would hang both.

use std::sync::Mutex;

/// Whether `ORBISTOUN_PROFILE` asked for sampling: `1`, or how many buckets to show.
fn asked() -> bool {
    shown().is_some()
}

/// How many buckets a report names per thread: [`SHOWN`] for `1`, the number itself for a larger
/// one, `None` when sampling was not asked for.
fn shown() -> Option<usize> {
    let value: usize = orbistoun_env::PROFILE.get()?.trim().parse().ok()?;
    match value {
        0 => None,
        1 => Some(SHOWN),
        many => Some(many),
    }
}

/// Watches the calling thread under `name`, when `ORBISTOUN_PROFILE` asks, starting the sampler
/// the first time.
pub fn watch_this_thread(name: &'static str) {
    if asked() {
        imp::watch(name);
    }
}

/// The guest's fixed image base, and how far past it a sample still counts as the guest's.
const GUEST_IMAGE: u64 = crate::DEFAULT_MODULE_BASE;
const GUEST_SPAN: u64 = 0x0100_0000_0000;

/// Samples kept per thread between reports: at one a millisecond, far more than a report's worth.
const CAPACITY: usize = 1 << 16;
/// How often the counts are printed.
const REPORT_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// How many buckets a report names per thread.
const SHOWN: usize = 16;
/// The most threads watched at once.
const MOST_THREADS: usize = 8;

/// Counts one thread's `samples` into buckets and prints the largest.
fn report(thread: &str, samples: &[(u64, u64)]) {
    use std::collections::HashMap;
    let mut buckets: HashMap<String, usize> = HashMap::new();
    let mut guest = 0usize;
    let own = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    for &(rip, caller) in samples {
        let name = if (GUEST_IMAGE..GUEST_IMAGE + GUEST_SPAN).contains(&rip) {
            guest += 1;
            format!("guest+{:#x}", (rip - GUEST_IMAGE) & !0x3f)
        } else if let Some((module, offset)) = crate::report::host_module_of(rip) {
            let called_from = (Some(&module) != own.as_ref())
                .then(|| crate::report::host_module_of(caller))
                .flatten()
                .map(|(by, at)| format!(" from {by}+{at:#x}"))
                .unwrap_or_default();
            format!("{module}+{:#x}{called_from}", offset & !0xff)
        } else {
            format!("unmapped {:#x}", rip & !0xfff)
        };
        *buckets.entry(name).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = buckets.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total = samples.len().max(1);
    let percent = |count: usize| {
        let tenths = count * 1000 / total;
        format!("{:3}.{}%", tenths / 10, tenths % 10)
    };
    let lines: Vec<String> = ranked
        .iter()
        .take(shown().unwrap_or(SHOWN))
        .map(|(name, count)| format!("{} {name}", percent(*count)))
        .collect();
    tracing::info!(
        "profile of the {thread} thread, {} samples, {} in the guest's image:\n  {}",
        samples.len(),
        percent(guest),
        lines.join("\n  ")
    );
}

/// The threads watched, by name. Taken only to copy the list, never while a thread is suspended.
static WATCHED: Mutex<Vec<(&'static str, usize)>> = Mutex::new(Vec::new());

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, GetThreadContext};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, ResumeThread, SuspendThread,
    };

    /// `CONTEXT_CONTROL` for x86-64: the instruction and stack pointers, and nothing that costs
    /// more to read (`winnt.h`, `CONTEXT_AMD64 | 0x1`).
    const CONTEXT_CONTROL: u32 = 0x0010_0001;

    /// Stack words copied per sample, from the stack pointer up.
    const STACK_WORDS: usize = 64;

    /// `CONTEXT` as the platform requires it: sixteen-byte aligned.
    #[repr(C, align(16))]
    struct Aligned(CONTEXT);

    pub(super) fn watch(name: &'static str) {
        let mut handle: HANDLE = std::ptr::null_mut();
        // SAFETY: answers this process's pseudo-handle; takes nothing and cannot fail.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: answers this thread's pseudo-handle; takes nothing and cannot fail.
        let this_thread = unsafe { GetCurrentThread() };
        // SAFETY: duplicates the pseudo-handle into a real one the sampler can hold; every argument
        // is a live handle or an out-parameter that outlives the call.
        let ok = unsafe {
            DuplicateHandle(
                process,
                this_thread,
                process,
                &raw mut handle,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            tracing::warn!("the profiler could not hold the {name} thread");
            return;
        }
        let first = {
            let mut watched = super::WATCHED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if watched.len() >= super::MOST_THREADS {
                return;
            }
            watched.push((name, handle as usize));
            watched.len() == 1
        };
        if first {
            let _ = std::thread::Builder::new()
                .name("orbistoun-profile".to_owned())
                .spawn(sample);
        }
    }

    fn sample() {
        let own = own_image();
        let mut samples: Vec<Vec<(u64, u64)>> = (0..super::MOST_THREADS)
            .map(|_| Vec::with_capacity(super::CAPACITY))
            .collect();
        let mut since = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1));
            // Copied out before any thread is suspended, so the lock is never held across one.
            let watched: Vec<(&'static str, usize)> = super::WATCHED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            for (slot, &(_, handle)) in watched.iter().enumerate() {
                let Some(into) = samples.get_mut(slot) else {
                    continue;
                };
                if let Some((rip, stack)) = instruction_and_stack(handle as HANDLE)
                    && into.len() < into.capacity()
                {
                    let caller = stack
                        .iter()
                        .copied()
                        .find(|word| own.contains(word))
                        .unwrap_or(0);
                    into.push((rip, caller));
                }
            }
            if since.elapsed() >= super::REPORT_EVERY {
                for (slot, &(name, _)) in watched.iter().enumerate() {
                    if let Some(taken) = samples.get_mut(slot) {
                        super::report(name, taken);
                        taken.clear();
                    }
                }
                since = std::time::Instant::now();
            }
        }
    }

    /// The address range of this executable's image, from its own headers.
    fn own_image() -> std::ops::Range<u64> {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        // SAFETY: a null name asks for this executable's own module, which is always loaded.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        if base == 0 {
            return 0..0;
        }
        // The image's size is in its optional header: `e_lfanew` at 0x3c, then the PE signature
        // (4 bytes) and file header (20), and `SizeOfImage` 56 bytes into the optional header.
        let at = |offset: usize| {
            let pointer = std::ptr::with_exposed_provenance::<u32>(base + offset);
            // SAFETY: both reads are inside this executable's mapped headers, which the loader
            // keeps readable for the process's life.
            unsafe { pointer.read_unaligned() }
        };
        let header = at(0x3c) as usize;
        let size = at(header + 4 + 20 + 56);
        base as u64..base as u64 + u64::from(size)
    }

    /// A thread's instruction pointer and the words at its stack pointer, read while it is
    /// suspended. Nothing between the suspend and the resume allocates or locks.
    fn instruction_and_stack(thread: HANDLE) -> Option<(u64, [u64; STACK_WORDS])> {
        // SAFETY: every field of `CONTEXT` is plain data, so all-zero is a valid value, which the
        // call below overwrites.
        let mut context = Aligned(unsafe { std::mem::zeroed() });
        context.0.ContextFlags = CONTEXT_CONTROL;
        // SAFETY: `thread` is a real handle to a live thread of this process, held for the
        // process's life.
        if unsafe { SuspendThread(thread) } == u32::MAX {
            return None;
        }
        // SAFETY: the thread is suspended and `context` is aligned, sized and flagged as the call
        // requires.
        let read = unsafe { GetThreadContext(thread, &raw mut context.0) };
        let mut stack = [0u64; STACK_WORDS];
        if read != 0 && context.0.Rsp != 0 {
            let from = std::ptr::with_exposed_provenance::<u64>(context.0.Rsp as usize);
            // SAFETY: the words from a suspended thread's stack pointer upward are that thread's
            // live, committed stack; `STACK_WORDS` of them is far less than any frame chain below
            // the stack's top, and the thread cannot change them while suspended.
            unsafe { std::ptr::copy_nonoverlapping(from, stack.as_mut_ptr(), STACK_WORDS) };
        }
        // SAFETY: resumes the suspension above, once.
        unsafe { ResumeThread(thread) };
        (read != 0).then_some((context.0.Rip, stack))
    }
}

#[cfg(not(windows))]
mod imp {
    /// No sampler away from Windows yet.
    pub(super) fn watch(_name: &'static str) {
        tracing::warn!("the profiler samples on Windows only");
    }
}
