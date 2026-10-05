//! Where a thread is, sampled.
//!
//! About once a millisecond each watched thread is suspended, its instruction pointer and the top
//! of its stack read, and it is resumed. A sample in the guest's image is counted by offset, one in
//! host code by module and offset, and one in a system library (most often a wait) by the first
//! return address into this executable on its stack. `ORBISTOUN_PROFILE=1` watches the thread
//! entering the guest and the device thread, and prints the counts every few seconds. The sampler
//! never allocates or locks while a thread is suspended, since that thread may hold the heap's
//! lock.

#[cfg(windows)]
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
#[cfg(windows)]
const GUEST_IMAGE: u64 = crate::DEFAULT_MODULE_BASE;
#[cfg(windows)]
const GUEST_SPAN: u64 = 0x0100_0000_0000;

/// Samples kept per thread between reports: at one a millisecond, far more than a report's worth.
#[cfg(windows)]
const CAPACITY: usize = 1 << 16;
/// How often the counts are printed.
#[cfg(windows)]
const REPORT_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
/// How many buckets a report names per thread.
const SHOWN: usize = 16;
/// The most threads watched at once.
#[cfg(windows)]
const MOST_THREADS: usize = 8;

/// The name of the function holding `address` in this process, from its symbols: this executable's
/// own (a release build keeps them) or a system library's exports. `None` where neither names it.
///
/// Asked only when a report is made, never while a thread is suspended. The symbol handler is not
/// thread-safe, so calls are serialised.
#[cfg(windows)]
fn symbol_of(address: u64) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, STATUS_INFO_LENGTH_MISMATCH};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SYMBOL_INFO, SYMOPT_DEFERRED_LOADS, SYMOPT_UNDNAME, SymCleanup, SymFromAddr,
        SymInitializeW, SymSetOptions,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    /// The longest name kept, in bytes.
    const NAME_BYTES: usize = 512;
    /// The furthest past a symbol an address is still taken to be inside it, in bytes.
    const NEAREST_EXPORT_REACH: u64 = 0x4000;
    /// How many times one report asks the handler to list the modules while they keep changing.
    const INIT_ATTEMPTS: usize = 8;
    // Only success is kept: a failed start is asked again at the next report rather than
    // leaving every report of the run without names.
    static HANDLER: Mutex<bool> = Mutex::new(false);
    let mut initialised = HANDLER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: answers this process's pseudo-handle; takes nothing and cannot fail.
    let process = unsafe { GetCurrentProcess() };
    if !*initialised {
        // SAFETY: sets the handler's options before it is initialised; takes a plain flag word.
        unsafe { SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS) };
        // The executable's own directory is searched for its symbols: the handler's default search
        // is the working directory and `_NT_SYMBOL_PATH`, which a run from elsewhere misses.
        let search: Vec<u16> = own_module()
            .and_then(|(_, path)| {
                std::path::Path::new(&path)
                    .parent()
                    .map(|dir| dir.as_os_str().to_owned())
            })
            .unwrap_or_default()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // Listing the modules fails with `STATUS_INFO_LENGTH_MISMATCH` when another thread loads
        // or unloads one while the list is read; that status means the list changed, so it is
        // read again.
        for _ in 0..INIT_ATTEMPTS {
            // SAFETY: initialises the handler for this process, listing every loaded module;
            // `search` is a NUL-terminated path that outlives the call.
            if unsafe { SymInitializeW(process, search.as_ptr(), 1) } != 0 {
                *initialised = true;
                break;
            }
            // SAFETY: reads this thread's last error; takes nothing.
            let error = unsafe { GetLastError() };
            // SAFETY: releases whatever the failed start left for this process, so the next
            // start begins clean; the handler lock is held, so no other call is using it.
            unsafe { SymCleanup(process) };
            // The handler reports the status as its last error, bit for bit.
            if error != u32::from_ne_bytes(STATUS_INFO_LENGTH_MISMATCH.to_ne_bytes()) {
                break;
            }
        }
    }
    if !*initialised {
        return None;
    }
    // A `SYMBOL_INFO` followed by room for its name, in eight-byte words for its alignment.
    let words = (size_of::<SYMBOL_INFO>() + NAME_BYTES).div_ceil(8);
    let mut buffer = vec![0u64; words];
    let info = buffer.as_mut_ptr().cast::<SYMBOL_INFO>();
    let header = SYMBOL_INFO {
        SizeOfStruct: size_of::<SYMBOL_INFO>() as u32,
        MaxNameLen: NAME_BYTES as u32,
        ..SYMBOL_INFO::default()
    };
    // SAFETY: `info` points at an aligned buffer at least `SYMBOL_INFO` long; the header's two size
    // fields are what the call reads to know how much it may write.
    unsafe { info.write(header) };
    let mut displacement = 0u64;
    // SAFETY: the handler is initialised for this process, and `info` has `MaxNameLen` bytes of
    // name room past its fixed fields.
    if unsafe { SymFromAddr(process, address, &raw mut displacement, info) } == 0 {
        return None;
    }
    // A module without symbols answers its nearest export, however far away: a graphics driver's
    // code comes back as a runtime helper tens of kilobytes off. That is not the function, so it
    // is no name at all.
    if displacement > NEAREST_EXPORT_REACH {
        return None;
    }
    // SAFETY: `info` is the header the call filled in.
    let length = (unsafe { (*info).NameLen } as usize).min(NAME_BYTES);
    let name = info
        .cast::<u8>()
        .wrapping_add(std::mem::offset_of!(SYMBOL_INFO, Name));
    // SAFETY: `name` holds `length` initialised bytes, as above.
    let bytes = unsafe { std::slice::from_raw_parts(name, length) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// This executable's base address and path.
#[cfg(windows)]
fn own_module() -> Option<(u64, std::ffi::OsString)> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: a null name asks for this executable's own module, which is always loaded.
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize as u64;
    let path = std::env::current_exe().ok()?;
    (base != 0).then(|| (base, path.into_os_string()))
}

/// A host address as a report names it: the function holding it, or its module and offset where no
/// symbol does.
#[cfg(windows)]
fn host_place(address: u64) -> Option<String> {
    let (module, offset) = crate::report::host_module_of(address)?;
    Some(match symbol_of(address) {
        Some(symbol) => format!("{module}!{symbol}"),
        None => format!("{module}+{:#x}", offset & !0xff),
    })
}

/// Counts one thread's `samples` into buckets and prints the largest.
#[cfg(windows)]
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
        } else if let Some((module, _)) = crate::report::host_module_of(rip) {
            // A system library - most often a wait or a memory call - is named with the function
            // of this executable that led to it, which is the place to look.
            let called_from = (Some(&module) != own.as_ref())
                .then(|| host_place(caller))
                .flatten()
                .map(|by| format!(" from {by}"))
                .unwrap_or_default();
            format!("{}{called_from}", host_place(rip).unwrap_or(module))
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
#[cfg(windows)]
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
    /// The sampler exists on Windows only.
    pub(super) fn watch(_name: &'static str) {
        tracing::warn!("the profiler samples on Windows only");
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// A function whose address the test looks up.
    #[inline(never)]
    fn a_function_the_profiler_names() -> u64 {
        std::hint::black_box(7)
    }

    /// A sample in this executable is named by its function, so a report says what is slow rather
    /// than where in the binary.
    #[test]
    fn a_host_address_is_named_by_its_function() {
        let address = a_function_the_profiler_names as fn() -> u64 as usize as u64;
        let name = super::symbol_of(address).expect("the symbols name it");
        assert!(name.contains("a_function_the_profiler_names"), "{name}");
    }
}
