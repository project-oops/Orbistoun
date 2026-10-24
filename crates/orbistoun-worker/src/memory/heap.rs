//! The process's heap, counted: every allocation through the system allocator, live bytes kept in
//! one counter, so a breakdown can tell what the named holders account for from what they do not.
//! With `ORBISTOUN_HOST_HEAP_SAMPLE` set, each large block is logged with where it was made.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Live bytes allocated through [`Counting`].
static LIVE: AtomicU64 = AtomicU64::new(0);

/// The size from which a block is logged; `usize::MAX` while sampling is off.
static SAMPLED_FROM: AtomicUsize = AtomicUsize::new(usize::MAX);

thread_local! {
    /// Whether this thread is logging a sample, whose own allocations are not sampled.
    static SAMPLING: Cell<bool> = const { Cell::new(false) };
}

/// The system allocator, counting what it hands out.
struct Counting;

// SAFETY: every call is passed to the system allocator unchanged, which upholds `GlobalAlloc`'s
// contract; the counter is a side effect that allocates nothing, and a sample's allocations are
// made through this allocator again with sampling off on that thread.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, passed on as `GlobalAlloc::alloc` requires.
        let at = unsafe { System.alloc(layout) };
        if at.is_null() {
            report_failure(layout.size());
        } else {
            LIVE.fetch_add(layout.size() as u64, Ordering::Relaxed);
            sample("allocated", at, layout.size());
        }
        at
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let at = unsafe { System.alloc_zeroed(layout) };
        if at.is_null() {
            report_failure(layout.size());
        } else {
            LIVE.fetch_add(layout.size() as u64, Ordering::Relaxed);
            sample("allocated", at, layout.size());
        }
        at
    }

    unsafe fn dealloc(&self, at: *mut u8, layout: Layout) {
        // SAFETY: the caller's block, allocated by this allocator with this layout.
        unsafe { System.dealloc(at, layout) };
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        sample("freed", at, layout.size());
    }

    unsafe fn realloc(&self, at: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller's block and layout, and a new size the contract allows.
        let moved = unsafe { System.realloc(at, layout, new_size) };
        if moved.is_null() {
            report_failure(new_size);
        } else {
            LIVE.fetch_add(new_size as u64, Ordering::Relaxed);
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            if new_size > layout.size() {
                sample("grown to", moved, new_size);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The heap's live bytes now.
pub(crate) fn live() -> u64 {
    LIVE.load(Ordering::Relaxed)
}

/// Says why an allocation of `bytes` failed, once, before Rust aborts the process: the host's error,
/// the system's commit against its limit, this process's private bytes and the heap's live count.
/// Runs 291, 293 and 300 died on a few hundred KiB with 20-30 GiB private and the system far from
/// its commit limit, so the failure is not plain exhaustion and the abort alone says nothing.
/// Formats into the stack and writes to stderr directly: the heap has just refused.
fn report_failure(bytes: usize) {
    use std::io::Write as _;
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut line = StackLine::default();
    let host = host_memory();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "orbistoun: an allocation of {bytes} bytes failed: host error {}, system commit {} of {} MiB, this process {} MiB private, heap {} MiB live\n",
            host.error,
            host.committed >> 20,
            host.commit_limit >> 20,
            host.private >> 20,
            live() >> 20
        ),
    );
    let _ = std::io::stderr().write_all(line.text());
}

/// A line formatted without the heap.
struct StackLine {
    bytes: [u8; 320],
    len: usize,
}

impl Default for StackLine {
    fn default() -> Self {
        Self {
            bytes: [0; 320],
            len: 0,
        }
    }
}

impl StackLine {
    fn text(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl core::fmt::Write for StackLine {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = self.bytes.len() - self.len;
        let taken = s.len().min(room);
        self.bytes[self.len..self.len + taken].copy_from_slice(&s.as_bytes()[..taken]);
        self.len += taken;
        Ok(())
    }
}

/// What the host says about memory as an allocation fails.
#[derive(Default)]
struct HostMemory {
    error: u32,
    committed: u64,
    commit_limit: u64,
    private: u64,
}

#[cfg(windows)]
fn host_memory() -> HostMemory {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: reads this thread's last error; no arguments.
    let error = unsafe { GetLastError() };
    // SAFETY: an all-zero structure is valid; the call fills it once its length is set.
    let mut status: MEMORYSTATUSEX = unsafe { core::mem::zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: a structure of the size its length names, alive for the call.
    let system = unsafe { GlobalMemoryStatusEx(&raw mut status) } != 0;
    // SAFETY: an all-zero structure is valid; the call fills it.
    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { core::mem::zeroed() };
    // SAFETY: returns this process's pseudo-handle; no arguments.
    let this = unsafe { GetCurrentProcess() };
    // SAFETY: this process's pseudo-handle and a structure of the size passed, alive for the call.
    let process = unsafe {
        GetProcessMemoryInfo(
            this,
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    } != 0;
    HostMemory {
        error,
        committed: if system {
            status.ullTotalPageFile - status.ullAvailPageFile
        } else {
            0
        },
        commit_limit: if system { status.ullTotalPageFile } else { 0 },
        private: if process {
            counters.PrivateUsage as u64
        } else {
            0
        },
    }
}

#[cfg(not(windows))]
fn host_memory() -> HostMemory {
    HostMemory::default()
}

/// Starts logging blocks of at least the MiB `ORBISTOUN_HOST_HEAP_SAMPLE` names, 64 when it names
/// none; nothing when it is unset.
pub(crate) fn sample_from_env() {
    /// The size logged from when the variable names none.
    const DEFAULT_MIB: usize = 64;
    let Some(value) = orbistoun_env::HOST_HEAP_SAMPLE.get() else {
        return;
    };
    let mib = value.trim().parse().unwrap_or(DEFAULT_MIB).max(1);
    SAMPLED_FROM.store(mib.saturating_mul(1 << 20), Ordering::Relaxed);
}

/// Logs `what` happened to the block of `bytes` at `at`, with the frames of this project's code
/// that led to it, when it is large enough to be sampled.
#[allow(clippy::cast_precision_loss)] // a block's size shown to a tenth of a MiB
fn sample(what: &str, at: *mut u8, bytes: usize) {
    /// How many of the project's frames a sample shows.
    const FRAMES: usize = 8;
    if bytes < SAMPLED_FROM.load(Ordering::Relaxed) || SAMPLING.with(Cell::get) {
        return;
    }
    SAMPLING.with(|sampling| sampling.set(true));
    let trace = std::backtrace::Backtrace::force_capture().to_string();
    let frames: Vec<&str> = trace
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("orbistoun") && !line.contains("memory::heap"))
        .filter(|line| !line.starts_with("at "))
        .take(FRAMES)
        .collect();
    tracing::info!(
        "host heap: {what} {:.1} MiB at {at:p}: {}",
        bytes as f64 / f64::from(1u32 << 20),
        frames.join(" <- ")
    );
    SAMPLING.with(|sampling| sampling.set(false));
}

#[cfg(test)]
mod tests {
    /// A block allocated is counted while it lives and no longer once dropped. Other tests
    /// allocate meanwhile, so the counts are compared with room for theirs.
    #[test]
    fn the_heap_counts_what_it_holds() {
        const BLOCK: u64 = 512 << 20;
        const ROOM: u64 = 256 << 20;
        let before = super::live();
        let block = vec![1u8; BLOCK as usize];
        let during = super::live();
        assert!(
            during >= BLOCK && during + ROOM >= before + BLOCK,
            "counted"
        );
        drop(block);
        assert!(
            super::live() < during.saturating_sub(BLOCK) + ROOM,
            "and released"
        );
    }
}
