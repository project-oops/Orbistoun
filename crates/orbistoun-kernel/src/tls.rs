//! Thread-local storage of the modules a title ships, served by module id (D763).
//!
//! The executable's block sits below the thread pointer, where its `TPOFF64` relocations reach
//! it. A module the title ships has a block of its own per thread, found through
//! `__tls_get_addr(&{module, offset})`, as the ELF thread-local storage ABI's general and local
//! dynamic models read it: allocated the first time a thread asks, from the module's template
//! (its `.tdata`, then `.tbss` as zeroes), and kept for the thread's life.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The executable's module id, which the loader's relocation answers for it.
pub const MAIN_MODULE: u64 = 1;

/// Where per-thread blocks of title modules are reserved, one after another.
///
/// Its own arena, clear of the thread-local blocks of the executable (`0x6900...`, `0x6A00...`)
/// and the policy regions (`0x6B00...`), so an address names its kind.
pub const DYNAMIC_TLS_BASE: u64 = 0x0000_6C00_0000_0000;

/// The span each block reservation advances by, at least: room for the block and the guards a
/// reservation carries, so neighbouring blocks never share a page.
const BLOCK_STEP: u64 = 0x10_0000;

/// What every thread's copy of one module's block starts as.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Template {
    /// Bytes the block occupies, `.tbss` included.
    size: u64,
    /// The `.tdata` image copied to the start of every block.
    init: Vec<u8>,
}

/// Every title module's template, by module id.
fn templates() -> &'static Mutex<BTreeMap<u64, Template>> {
    static TEMPLATES: OnceLock<Mutex<BTreeMap<u64, Template>>> = OnceLock::new();
    TEMPLATES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The executable's block size: how far below the thread pointer its block begins. Zero until
/// the executable's block is built, and for an executable with no thread-locals.
static MAIN_BLOCK_SIZE: AtomicU64 = AtomicU64::new(0);

/// The next block reservation, bump-allocated and never reused.
static NEXT_BLOCK: AtomicU64 = AtomicU64::new(DYNAMIC_TLS_BASE);

thread_local! {
    /// This thread's block of each title module it has asked for.
    static BLOCKS: RefCell<BTreeMap<u64, u64>> = const { RefCell::new(BTreeMap::new()) };
}

/// Records where the executable's block sits: `block_size` below the thread pointer.
pub fn register_main(block_size: u64) {
    MAIN_BLOCK_SIZE.store(block_size, Ordering::Release);
}

/// Records title module `id`'s template: a block of `size` bytes starting with `init`.
pub fn register_module(id: u64, size: u64, init: Vec<u8>) {
    if let Ok(mut table) = templates().lock() {
        table.insert(id, Template { size, init });
    }
}

/// The address of `offset` in module `module`'s block for the calling thread, whose thread
/// pointer is `thread_pointer`.
///
/// `None` for a module nothing registered, which includes module zero, an unrelocated index.
pub fn address_of(module: u64, offset: u64, thread_pointer: Option<u64>) -> Option<u64> {
    if module == MAIN_MODULE {
        let below = MAIN_BLOCK_SIZE.load(Ordering::Acquire);
        return Some(thread_pointer?.checked_sub(below)?.wrapping_add(offset));
    }
    if let Some(block) = BLOCKS.with(|blocks| blocks.borrow().get(&module).copied()) {
        return Some(block.wrapping_add(offset));
    }
    let template = templates().lock().ok()?.get(&module).cloned()?;
    let block = reserve_block(&template)?;
    BLOCKS.with(|blocks| blocks.borrow_mut().insert(module, block));
    Some(block.wrapping_add(offset))
}

/// Reserves one block, writes its `.tdata`, and answers its address. The rest reads as zero,
/// as a fresh reservation does.
fn reserve_block(template: &Template) -> Option<u64> {
    let step = template.size.div_ceil(BLOCK_STEP).saturating_add(1) * BLOCK_STEP;
    let at = NEXT_BLOCK.fetch_add(step, Ordering::Relaxed);
    let region = orbistoun_mem::stack::GuestStack::reserve(at, template.size).ok()?;
    let block = region.lowest_usable();
    // Kept for the run: a thread may hand a pointer into its block to another thread.
    let span = region.len();
    std::mem::forget(region);
    crate::note_region(block, span);
    if template.init.is_empty() {
        return Some(block);
    }
    // SAFETY: `block` is the start of a span just reserved read-write for at least `size` bytes,
    // and the init image is no longer than the block it starts.
    let written = unsafe { orbistoun_mem::guest::write_bytes(block, &template.init) };
    written.then_some(block)
}

#[cfg(test)]
mod tests {
    /// The executable's variables are below the thread pointer, at the offset the relocation
    /// gave, as its own `TPOFF64` code reads them.
    #[test]
    fn the_executable_s_block_is_below_the_thread_pointer() {
        super::register_main(0xa0);
        assert_eq!(
            super::address_of(super::MAIN_MODULE, 0x18, Some(0x6900_0000_10a0)),
            Some(0x6900_0000_1018)
        );
    }

    /// A title module's block is made for a thread when it first asks, starts with the module's
    /// `.tdata`, and is the same block every later time that thread asks.
    #[test]
    fn a_title_module_s_block_is_made_once_per_thread_from_its_template() {
        super::register_module(7, 0x468, vec![0x11, 0x22, 0x33]);
        let first = super::address_of(7, 0, None).expect("a registered module has a block");
        assert_eq!(super::address_of(7, 0x40, None), Some(first + 0x40));
        // SAFETY: `first` is the block just reserved for this thread.
        let init = unsafe { orbistoun_mem::guest::read_u32(first) };
        assert_eq!(init, Some(0x0033_2211));
        let other = std::thread::spawn(|| super::address_of(7, 0, None))
            .join()
            .expect("the other thread returns");
        assert!(other.is_some_and(|b| b != first), "each thread has its own");
    }

    /// An index the loader never relocated names no module, and is not answered with a block.
    #[test]
    fn an_unregistered_module_has_no_block() {
        assert_eq!(super::address_of(0, 8, Some(0x6900_0000_1000)), None);
        assert_eq!(super::address_of(0x55, 8, None), None);
    }
}
