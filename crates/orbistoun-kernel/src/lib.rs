//! libkernel HLE: memory, threads, synchronisation, event queues and module loading.
//!
//! Third in the dependency spine, after the container parser and the address space: every
//! subsystem above needs a guest that can allocate memory and spawn threads. The target kernel
//! is FreeBSD-derived and much of libkernel is POSIX under vendor names, so where a function has
//! a documented BSD analogue, that analogue is the specification.
//!
//! Guest threads are real host threads, because guest code reads thread-local storage directly
//! and blocks in its own primitives. Declared arities affect trace fidelity, not correctness
//! (see `orbistoun-hle::ImportDesc`).

pub mod apr;
pub mod direct;
pub mod interrupt;
pub mod mapped;
pub mod sync;
pub mod thread;

use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

/// The user-level threading library, declared here because its objects rest on the same `sync`
/// primitives libkernel's pthread and the C-runtime `_Mtx_*` families do.
///
/// A nested module because each `guest_module!` names its declaration `MODULE`. libSceUlt is a
/// cooperative-threading (fibre) library; its mutexes are ordinary mutexes under a vendor name
/// and stand on their own. The fibre scheduler (`_sceUltUlthreadCreate`) is not built.
pub mod ult {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceUlt" {
            // (mutex, name, optParam): the documented shape. The internal spelling passes an SDK version
            // after it, which is not read.
            "_sceUltMutexCreate" => 3,
            "_sceUltMutexLock" => 1,
            "_sceUltMutexUnlock" => 1,
            "_sceUltMutexTryLock" => 1,
            "_sceUltMutexDestroy" => 1,
            // (cv, name, mutex, optParam): the mutex is bound here rather than at each wait.
            "_sceUltConditionVariableCreate" => 4,
            "_sceUltConditionVariableSignal" => 1,
            "_sceUltConditionVariableSignalAll" => 1,
            "_sceUltConditionVariableWait" => 1,
            "_sceUltConditionVariableDestroy" => 1,
            // (ulthread, name, entry, arg, context, sizeContext) in registers; a runtime and optParam
            // follow on the stack. Only the first is used.
            "_sceUltUlthreadCreate" => 6,
            // Arity two, read off the guest's calls: the third register holds a leftover.
            "sceUltWaitingQueueResourcePoolGetWorkAreaSize" => 2,
            "sceUltUlthreadRuntimeGetWorkAreaSize" => 2,
            // (object, name, count, count, workArea, optParam), the shape both constructors share, read
            // off the guest's own calls.
            "_sceUltWaitingQueueResourcePoolCreate" => 6,
            "_sceUltUlthreadRuntimeCreate" => 6,
            // Three values whose meaning is not established; none is read.
            "sceUltInitialize" => 3,
        }
    }
}

/// The wait-on-address family, under the library name the guest imports it by.
///
/// The kernel module exports more than one library, and a guest names the one it wants:
/// `libkernel_sync_on_address` sits beside `libkernel` and `libkernel_fs`, and a trace labels
/// the calls with it. Both names are confirmed by hash against the export layout.
pub mod sync_on_address {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libkernel_sync_on_address" {
            // (address, value): the two registers guests fill; the following registers read zero or
            // leftovers in every captured call (D573).
            "sceKernelSyncOnAddressWait" => 2,
            // (address, count). The third to fifth registers carry values whose role is not established;
            // none is read.
            "sceKernelSyncOnAddressWake" => 2,
        }
    }
}

guest_module! {
    "libkernel" {
        "sceKernelAllocateDirectMemory" => 6,
        // A module handle, a name, and where to put the address, measured from a guest's calls (D366).
        "sceKernelDlsym" => 3,
        // A device, the request, its size, and whether to block. The request's layout is not
        // published and is not read.
        "sceKernelSendNotificationRequest" => 4,
        "sceKernelReleaseDirectMemory" => 2,
        "sceKernelMapDirectMemory" => 6,
        // Seven arguments; the seventh is a name the trampoline cannot reach, which costs a label in a
        // trace and nothing else.
        "sceKernelMapNamedDirectMemory" => 6,
        // The entry array, how many entries, and where to write how many were mapped. Display
        // bring-up maps its scanout surface through it.
        "sceKernelBatchMap" => 3,
        // A size-prefixed out-structure. It refuses on retail hardware with `0x8002_0006`, because the
        // resource-registration subsystem it depends on is stubbed there.
        "sceKernelMapperGetParam" => 1,
        // Four, for the four arguments the implementation reads; the dump shows the six registers
        // System V passes.
        "sceKernelReserveVirtualRange" => 4,
        "sceKernelVirtualQuery" => 4,
        "sceKernelMprotect" => 3,
        // The signal number and the handler: an inverted call answers EINVAL on the hardware, and a
        // guest's handler compares its first argument to 30.
        "sceKernelInstallExceptionHandler" => 2,
        "sceKernelRemoveExceptionHandler" => 1,
        "sceKernelRaiseException" => 2,
        "sceKernelSetVirtualRangeName" => 3,
        "sceKernelAllocateMainDirectMemory" => 4,
        "sceKernelGetDirectMemorySize" => 0,
        "sceKernelDirectMemoryQuery" => 4,
        "sceKernelGetSystemSwVersion" => 1,
        "scePthreadCreate" => 5,
        "scePthreadJoin" => 2,
        "scePthreadSelf" => 0,
        // The calling thread's unique integer id (FreeBSD `pthread_getthreadid_np`) (D452).
        "scePthreadGetthreadid" => 0,
        // Named by guests themselves and confirmed by hash. Titles print diagnostics naming these,
        // with file and line, and an error return from `sceKernelCreateSema` aborts static
        // initialisation.
        "sceKernelCreateSema" => 6,
        "scePthreadMutexattrInit" => 1,
        "scePthreadCondattrInit" => 1,
        "scePthreadMutexattrSettype" => 2,
        "scePthreadMutexattrGettype" => 2,
        "scePthreadMutexattrGetprotocol" => 2,
        "scePthreadMutexattrSetprotocol" => 2,
        "scePthreadMutexattrDestroy" => 1,
        "scePthreadMutexInit" => 3,
        "scePthreadMutexLock" => 1,
        "scePthreadMutexUnlock" => 1,
        "scePthreadMutexTrylock" => 1,
        "scePthreadMutexDestroy" => 1,
        "vendor_system_version" => 3,
        "sceKernelGetProcessTime" => 0,
        "sceKernelGetProcessTimeCounter" => 0,
        "sceKernelGetProcessTimeCounterFrequency" => 0,
        "scePthreadCondInit" => 3, "scePthreadCondWait" => 2,
        "scePthreadCondSignal" => 1, "scePthreadCondBroadcast" => 1,
        "scePthreadCondDestroy" => 1,
        "scePthreadRwlockInit" => 3, "scePthreadRwlockRdlock" => 1,
        "scePthreadRwlockTryrdlock" => 1, "scePthreadRwlockWrlock" => 1,
        "scePthreadRwlockTrywrlock" => 1, "scePthreadRwlockUnlock" => 1,
        "scePthreadRwlockDestroy" => 1,
        "posix_pthread_rwlock_init" => 2, "posix_pthread_rwlock_rdlock" => 1,
        "posix_pthread_rwlock_tryrdlock" => 1, "posix_pthread_rwlock_wrlock" => 1,
        "posix_pthread_rwlock_trywrlock" => 1, "posix_pthread_rwlock_unlock" => 1,
        "posix_pthread_rwlock_destroy" => 1,
        "scePthreadBarrierInit" => 4, "scePthreadBarrierWait" => 1,
        "scePthreadBarrierDestroy" => 1,
        // The POSIX-signature barrier init takes no name, so it cannot share the vendor entry point;
        // the same split as `posix_pthread_rwlock_init`.
        "posix_pthread_barrier_init" => 3,
        "sceKernelCreateEventFlag" => 5, "sceKernelPollEventFlag" => 5,
        // Arity 2 for both, read off the guest's calls: the third register holds a leftover.
        "sceKernelCreateEqueue" => 2, "sceKernelAddUserEventEdge" => 2,
        "sceKernelWaitEqueue" => 5,
        // Arities read off the guest's calls: Get takes a handle and two adjacent out-parameters, Set a
        // handle, a policy and a param pointer, and the two-argument pair a handle and one value.
        "scePthreadGetschedparam" => 3, "scePthreadSetschedparam" => 3,
        "scePthreadSetprio" => 2, "scePthreadRename" => 2,
        "scePthreadCondattrDestroy" => 1, "pthread_setcancelstate" => 2,
        // `_sigprocmask` takes how, a set and an out-set. `sceKernelUuidCreate` takes only the
        // destination.
        "_sigprocmask" => 3, "sceKernelUuidCreate" => 1,
        "sceKernelWaitEventFlag" => 5,
        "sceKernelSetEventFlag" => 2, "sceKernelClearEventFlag" => 2,
        "sceKernelDeleteEventFlag" => 1,
        "sceKernelPollSema" => 2, "sceKernelSignalSema" => 2,
        "sceKernelWaitSema" => 3, "sceKernelDeleteSema" => 1,
        "sceKernelMunmap" => 2,
        "sceKernelMmap" => 6,
        "sceKernelAvailableFlexibleMemorySize" => 1,
        "sceKernelConfiguredFlexibleMemorySize" => 1,
        "sceKernelMapFlexibleMemory" => 4,
        "sceKernelReleaseFlexibleMemory" => 2,
        "scePthreadAttrInit" => 1, "scePthreadAttrDestroy" => 1,
        "scePthreadAttrSetstacksize" => 2, "scePthreadAttrGetstacksize" => 2,
        // (thread, attr) and (attr, out): the shapes of FreeBSD `pthread_attr_get_np` and
        // `pthread_attr_getstackaddr`, used by garbage collectors to find the stack they scan.
        "scePthreadAttrGet" => 2, "scePthreadAttrGetstackaddr" => 2,
        "scePthreadAttrSetdetachstate" => 2, "scePthreadAttrGetdetachstate" => 2,
        "scePthreadAttrSetschedparam" => 2, "scePthreadAttrGetschedparam" => 2,
        // The thread form, not the attribute form: a subject and a mask (D523).
        "scePthreadSetaffinity" => 2,
        "scePthreadGetaffinity" => 2,
        "scePthreadAttrSetschedpolicy" => 2, "scePthreadAttrSetinheritsched" => 2,
        "scePthreadAttrSetaffinity" => 2, "scePthreadAttrSetguardsize" => 2,
        // The platform's asynchronous file path. A title resolves paths to ids and sizes, builds a
        // command buffer of reads, submits it and waits, so it opens files and reads nothing through
        // the descriptors. Declared so the calls are visible in a trace; each answers the placeholder
        // unless delivery is enabled (see `apr`).
        //
        // `sceKernelAprWaitCommandBuffer` is confirmed only by its hash matching the import. Arity six
        // throughout is the trampoline's full capture, not a claim about the signatures.
        "sceKernelAprResolveFilepathsToIdsAndFileSizes" => 6,
        "sceKernelAprSubmitCommandBufferAndGetResult" => 6,
        "sceKernelAprWaitCommandBuffer" => 6,
        "sceKernelReadTsc" => 0,
        "sceKernelGetTscFrequency" => 0,
        "sceKernelIsStack" => 3,
        "sceKernelGetModuleList" => 3,
        "sceKernelLoadStartModule" => 6,
        // Refused rather than answered, because the structure it fills is not known (D395).
        "sceKernelGetModuleInfo" => 2,
        "sceKernelIsCex" => 0,
        // `Devkit`, not `DevKit`: a NID is a hash of the name, the two are different symbols, and
        // guests import the first (D393).
        "sceKernelIsDevkit" => 0,
        // Two more booleans of the same shape.
        "sceKernelIsNeoMode" => 0,
        "sceKernelIsDevelopmentMode" => 0,
        "sceKernelIsTestKit" => 0,
        "posix_getpagesize" => 0,
        "posix_usleep" => 1,
        "posix_sigemptyset" => 1,
        "posix_sigfillset" => 1,
        "posix_sigaddset" => 2,
        "posix_sigdelset" => 2,
        "posix_sigismember" => 2,
        "sceKernelUsleep" => 1,
        // Arity 2, read off the guest's calls: a clock identifier and a writable stack address.
        "sceKernelClockGettime" => 2,

    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// What the vendor's memory-query info structure holds, in order.
///
/// Three 64-bit fields: where the region starts, where it ends, and its memory type. Written
/// directly into guest memory, so the layout is the contract.
const QUERY_INFO_SIZE: u64 = 24;

/// `sceKernelDirectMemoryQuery(offset, flags, info, info_size)`.
///
/// Answers "what is at this physical offset, and what comes after it". A guest walks the whole
/// map by feeding back the end of each region it is shown, and repeats the walk if it cannot
/// complete it, so this is among the most-called functions in a run.
fn direct_memory_query(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, flags, info, info_size) = (args[0], args[1], args[2], args[3]);

    if info == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    // A small buffer is accepted: the hardware accepts every declared size from 1 to 256 (D398).
    // What is written is capped at what the caller declared, since overrunning a buffer the guest
    // sized cannot be undone; whether the hardware truncates the same way is not established.
    let room = info_size.min(QUERY_INFO_SIZE);

    // The hardware accepts flags 0 and 1 and refuses 2 and 4 with the invalid-argument code.
    if flags > 1 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    let Ok(guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let Some(region) = guard.query(offset) else {
        // Past the end of physical memory. The structure is cleared: the guest ignores the return
        // value and reads the buffer, so a stale answer would make it query the same address forever.
        write_query_info(info, room, &[0, 0, 0]);
        // The hardware answers this with the permission-denied errno, distinct from the invalid
        // argument it uses for a bad flag (D398).
        return u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw());
    };

    // The third field carries the memory type the allocation asked for: on the hardware, pages
    // allocated as types 0, 3 and 10 read back `0x0`, `0x3` and `0xa`. Pinned by
    // `the_third_query_field_is_the_memory_type_the_allocation_asked_for`.
    if marked_query_fields() {
        // Marked fields: each carries a value that names itself, so what the guest does next shows
        // which field it read. The low half of each field stays real so the walk still advances; the
        // high half is the mark.
        write_query_info(
            info,
            room,
            &[
                MARK_FIELD0 | (region.start & MARK_MASK),
                MARK_FIELD1 | (region.end & MARK_MASK),
                MARK_FIELD2 | u64::from(region.memory_type),
            ],
        );
        return OK;
    }
    write_query_info(
        info,
        room,
        &[region.start, region.end, u64::from(region.memory_type)],
    );
    OK
}

/// How much of a marked field is the real value.
///
/// The low forty-eight bits, which covers every address in the range, so a marked walk advances
/// exactly as an unmarked one does.
const MARK_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// Mark for the first field, in the bits the real value cannot reach.
const MARK_FIELD0: u64 = 0xAAAA_0000_0000_0000;
/// Mark for the second.
const MARK_FIELD1: u64 = 0xBBBB_0000_0000_0000;
/// Mark for the third, which carries no address and so is marked whole.
const MARK_FIELD2: u64 = 0xCCCC_0000_0000_0000;

/// Whether the memory-query structure is being written with self-identifying values.
fn marked_query_fields() -> bool {
    MARKED_QUERY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set once during setup, read on every query - so an atomic rather than a lock.
static MARKED_QUERY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks for the memory-query structure to be written with marked values.
///
/// A diagnostic, not a setting: it answers which field the guest reads.
pub fn mark_query_fields(on: bool) {
    MARKED_QUERY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Writes a query result into guest memory.
///
/// Both the success and the end-of-walk paths write it, because a guest that ignores return
/// values reads the buffer either way.
fn write_query_info(info: u64, room: u64, fields: &[u64; 3]) {
    let (Ok(dest), Ok(room)) = (usize::try_from(info), usize::try_from(room)) else {
        return;
    };
    let room = room.min(QUERY_INFO_SIZE as usize);
    // SAFETY: the guest supplied this address and declared it large enough, which is the
    // same contract the real call has. The mapping is identity, so a guest address is a
    // host address; an address the guest has not mapped faults here exactly as it would
    // have faulted in the guest, and the fault reporter names it.
    unsafe {
        std::ptr::copy_nonoverlapping(
            fields.as_ptr().cast::<u8>(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest),
            room,
        );
    }
}

/// `sceKernelGetSystemSwVersion(out)`: the version this call reports, which is not the system
/// firmware.
///
/// The structure is `{ size_t size; char version_string[0x1c]; uint32_t version; }`, 0x28 bytes;
/// the hardware answers 0 and fills it. The hardware reports firmware 12.40 through `kern.version`
/// and syscall 649 ([`vendor_system_version`]) but `13.090.001` (`0x1309_0001`) through this call,
/// so it reads [`machine`]`().software_version`, a profile value separate from
/// `machine.firmware` (D421). An unset value refuses the call rather than inventing a version.
fn get_system_sw_version(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (dest, body) = match sw_version_write(machine().software_version.as_ref(), args[0]) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    // SAFETY: a guest-supplied out pointer, identity-mapped, written from offset 8 for 0x20 bytes
    // - inside the structure the call documents, and not touching the caller's size word. An
    // address the guest has not mapped faults here exactly as it would have in the guest.
    unsafe {
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(dest + 8),
            body.len(),
        );
    }
    0
}

/// Decides what `sceKernelGetSystemSwVersion` does, without touching guest memory: refuse with a
/// code, or the destination and the `0x20` bytes to write from offset 8. Pure, so both the
/// unset refusal and the byte layout are testable without a guest buffer.
fn sw_version_write(
    version: Option<&orbistoun_core::machine::SoftwareVersion>,
    out: u64,
) -> Result<(usize, [u8; 0x20]), u64> {
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    let Some(version) = version else {
        // Unset: refuse, as an unset firmware does, rather than answer a made-up version.
        return Err(vendor(orbistoun_core::errno::NO_ENTRY));
    };
    let dest = usize::try_from(out).map_err(|_| vendor(orbistoun_core::errno::INVALID))?;
    if dest == 0 {
        return Err(vendor(orbistoun_core::errno::INVALID));
    }
    Ok((dest, sw_version_body(version)))
}

/// The `0x20` bytes the call writes from offset 8: the display string, then the packed integer at
/// struct offset `0x24`. The size field at offset 0 is the caller's and is never touched; on the
/// hardware offsets 8..40 change and 0..8 stay as the caller left them.
fn sw_version_body(version: &orbistoun_core::machine::SoftwareVersion) -> [u8; 0x20] {
    let mut body = [0u8; 0x20];
    // `version_string` is `char[0x1c]`; a longer configured string is truncated rather than
    // overrunning the integer that follows.
    let text = version.display.as_bytes();
    let n = text.len().min(0x1c);
    body[..n].copy_from_slice(&text[..n]);
    body[0x1c..0x20].copy_from_slice(&version.packed.to_le_bytes());
    body
}

/// `sceKernelGetDirectMemorySize()`.
///
/// How much physical memory exists, answered from the setting the pool is built from, so the
/// two cannot describe different machines. The setting depends on the guest class (see
/// [`direct::DIRECT_MEMORY_SIZE`]).
fn direct_memory_size(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    direct::configured().pool_bytes
}

/// `std::_Execute_once(once_flag&, callback, context)`: runs a `call_once` initialiser once.
///
/// The callback runs on a fresh stack through [`thread::call_guest`]; without it every `static`
/// guarded by `call_once` stays uninitialised. The flag's first word is `0` before the
/// initialiser has run and `1` after, and a guest constructs a `once_flag` as zero. Success
/// follows the `InitOnce` convention: non-zero is success, and only then is the flag marked
/// done, so a failing callback is retried.
///
/// Not serialised across threads: two threads racing the same fresh flag could both run the
/// initialiser.
fn execute_once(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The flag's first word once the initialiser has completed.
    const DONE: u64 = 1;
    /// Success, in the `InitOnce` convention `call_once` tests against.
    const SUCCESS: u64 = 1;
    let (flag, callback, context) = (args[0], args[1], args[2]);
    if flag == 0 || callback == 0 {
        return 0;
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::read_u64(flag) } == Some(DONE) {
        return SUCCESS;
    }
    // The callback is an `InitOnce`-shaped `int(*)(void*, void*, void**)`: the flag as the handle,
    // the caller's context, and a slot for a context of its own, so a callback that writes through
    // it does not fault on a null.
    let mut leftover: u64 = 0;
    let leftover_ptr = std::ptr::addr_of_mut!(leftover) as u64;
    // SAFETY: `callback` is a guest function pointer `std::call_once` handed over; `call_guest`
    // reserves and guards its own stack.
    let ran = unsafe { thread::call_guest(callback, [flag, context, leftover_ptr]) };
    match ran {
        Some(rc) if rc != 0 => {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::write_u64(flag, DONE) };
            SUCCESS
        }
        _ => 0,
    }
}

// The C-runtime threading primitives the C++ standard library rests on: `_Mtx_*`, `_Cnd_*`,
// `_Xtime_get_ticks`, `_Thrd_sleep`. `std::mutex`, `std::condition_variable` and
// `std::this_thread` lower onto these, so a title using any of them reaches here during static
// construction. The standard library throws on a non-success `_Thrd_result`, so each maps onto
// the primitives in [`sync`] and answers a real result.

/// The `_Thrd_result` codes the standard library branches on, in the runtime's own order.
mod thrd {
    /// The call succeeded. `std::mutex` throws on anything else.
    pub(crate) const SUCCESS: u64 = 0;
    /// The lock was held by another thread: the answer `try_lock` exists to give.
    pub(crate) const BUSY: u64 = 3;
    /// A timed wait reached its deadline without being signalled.
    pub(crate) const TIMEDOUT: u64 = 2;
    /// The handle named nothing this crate created.
    pub(crate) const ERROR: u64 = 4;
}

/// Resolves the [`sync`] handle a `_Mtx_*`/`_Cnd_*` argument names, in either of the two shapes
/// the argument takes.
///
/// The C-runtime spelling passes the handle value the matching `Init` stored; the C11 `mtx_t*`
/// spelling passes a pointer to the storage holding it. This tries the argument as a handle, then
/// as a pointer to one, and returns only a handle `exists` confirms this crate issued.
fn c_runtime_handle(arg: u64, exists: impl Fn(u64) -> bool) -> Option<u64> {
    if exists(arg) {
        return Some(arg);
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let inner = unsafe { guest::read_u64(arg) }?;
    exists(inner).then_some(inner)
}

/// `_Mtx_init(mtx, type)`: constructs a mutex where the guest asked, and answers success.
///
/// The `type` word is read but does not select recursion: every std mutex is created `Allowed`,
/// so a same-thread re-entry cannot raise a false deadlock while the runtime's type bits are
/// unmeasured (D431). Exclusion between different threads is real either way.
fn c_mtx_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mtx = args[0];
    if mtx == 0 {
        return thrd::ERROR;
    }
    let handle = sync::create(sync::Recursion::Allowed, "std::mutex");
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(mtx, handle) } {
        return thrd::ERROR;
    }
    thrd::SUCCESS
}

/// `_Mtx_destroy(mtx)`: releases the object. A word never initialised names nothing, which is
/// not an error to destroy.
fn c_mtx_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = c_runtime_handle(args[0], |h| sync::name_of(h).is_some()) {
        sync::destroy(handle);
    }
    thrd::SUCCESS
}

/// `_Mtx_lock(mtx)`: blocks until the mutex is held by this thread.
fn c_mtx_lock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = c_runtime_handle(args[0], |h| sync::name_of(h).is_some()) else {
        return thrd::ERROR;
    };
    let by = thread::adopt("main");
    match sync::acquire(handle, by, sync::Blocking::Forever) {
        Some(sync::Acquisition::Locked) => thrd::SUCCESS,
        _ => thrd::ERROR,
    }
}

/// `_Mtx_unlock(mtx)`: releases a lock this thread holds.
fn c_mtx_unlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = c_runtime_handle(args[0], |h| sync::name_of(h).is_some()) else {
        return thrd::ERROR;
    };
    let by = thread::adopt("main");
    match sync::unlock(handle, by) {
        Some(true) => thrd::SUCCESS,
        _ => thrd::ERROR,
    }
}

/// `_Mtx_trylock(mtx)`: takes the mutex only if it is free, and says so when it is not; the guest
/// enters a critical section on success alone.
fn c_mtx_trylock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = c_runtime_handle(args[0], |h| sync::name_of(h).is_some()) else {
        return thrd::ERROR;
    };
    let by = thread::adopt("main");
    match sync::acquire(handle, by, sync::Blocking::Never) {
        Some(sync::Acquisition::Locked) => thrd::SUCCESS,
        Some(sync::Acquisition::Busy | sync::Acquisition::Deadlock) => thrd::BUSY,
        None => thrd::ERROR,
    }
}

/// `_Cnd_init(cnd)`: constructs a condition variable where the guest asked.
fn c_cnd_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return thrd::ERROR;
    }
    let handle = sync::create_cond("std::condition_variable");
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return thrd::ERROR;
    }
    thrd::SUCCESS
}

/// `_Cnd_destroy(cnd)`: releases the object. An uninitialised word names nothing to release.
fn c_cnd_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some()) {
        sync::cond_destroy(handle);
    }
    thrd::SUCCESS
}

/// Releases the guest mutex around a wait and retakes it after, for [`c_cnd_wait`] and
/// [`c_cnd_timedwait`]. The two objects are independent here, so a signal landing in the gap is
/// lost where the platform would hold it, as with the POSIX pair.
fn c_cnd_wait_inner(
    cnd_arg: u64,
    mtx_arg: u64,
    timeout: Option<std::time::Duration>,
) -> Option<bool> {
    let cond = c_runtime_handle(cnd_arg, |h| sync::cond_name_of(h).is_some())?;
    let mutex = c_runtime_handle(mtx_arg, |h| sync::name_of(h).is_some());
    let by = thread::adopt("main");
    if let Some(handle) = mutex {
        sync::unlock(handle, by);
    }
    let woken = sync::cond_wait(cond, timeout);
    if let Some(handle) = mutex {
        sync::acquire(handle, by, sync::Blocking::Forever);
    }
    woken
}

/// `_Cnd_wait(cnd, mtx)`: waits until signalled, holding the mutex again on return.
fn c_cnd_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_cnd_wait_inner(args[0], args[1], None) {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// `_Cnd_timedwait(cnd, mtx, xtime)`: waits until signalled or the absolute deadline passes.
///
/// The deadline is an `xtime{ sec, nsec }` since the epoch; the wait is bounded by how far it is
/// ahead of now, so a deadline already past returns at once and a signal still wins if it
/// arrives first. `TIMEDOUT` is reported distinctly from a wake so the guest's predicate loop
/// behaves.
fn c_cnd_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let deadline = read_xtime(args[2]);
    let timeout = deadline.map_or(Some(std::time::Duration::ZERO), duration_until);
    match c_cnd_wait_inner(args[0], args[1], timeout) {
        Some(true) => thrd::SUCCESS,
        Some(false) => thrd::TIMEDOUT,
        None => thrd::ERROR,
    }
}

/// `_Cnd_signal(cnd)`: wakes one waiter.
fn c_cnd_signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some()).and_then(sync::cond_signal)
    {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// `_Cnd_broadcast(cnd)`: wakes every waiter.
fn c_cnd_broadcast(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some())
        .and_then(sync::cond_broadcast)
    {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// Reads an `xtime{ sec: i64, nsec: i64 }` the runtime passes by pointer, as a duration since the
/// epoch. `None` for a null or unreadable pointer.
fn read_xtime(pointer: u64) -> Option<std::time::Duration> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let sec = unsafe { guest::read_u64(pointer) }?;
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let nsec = unsafe { guest::read_u64(pointer + 8) }?;
    Some(std::time::Duration::new(
        sec,
        u32::try_from(nsec % 1_000_000_000).unwrap_or(0),
    ))
}

/// How long from now until an absolute time since the epoch, clamped at zero for a time already
/// past. Uses the host wall clock, as [`xtime_get_ticks`] does.
fn duration_until(target: std::time::Duration) -> Option<std::time::Duration> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Some(target.saturating_sub(now))
}

/// `_Xtime_get_ticks()`: the current time in 100-nanosecond ticks since the epoch, the unit the
/// runtime's `xtime` clock counts in. From the host wall clock; a clock that cannot be read
/// answers zero.
fn xtime_get_ticks(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos() / 100).unwrap_or(u64::MAX))
}

/// `_Thrd_sleep(duration, remaining)`: yields the thread for the requested span.
///
/// The span is read as a relative `{ sec, nsec }` and clamped to one second, because the
/// runtime's absolute-versus-relative convention is not pinned and a mix-up would turn a short
/// retry sleep into a hang. Always answers success.
fn thrd_sleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(span) = read_xtime(args[0]) {
        let capped = span.min(std::time::Duration::from_secs(1));
        std::thread::sleep(capped);
    }
    thrd::SUCCESS
}

// libSceUlt mutexes: ordinary named mutexes on the same `sync` primitives as the pthread and
// `_Mtx_*` families, declared in the `ult` module. The guest stores the handle in the first word
// of its `SceUltMutex`, so `mutex_at` resolves it as for pthread. Created `Allowed`, as the
// `_Mtx_*` family is (D431). Success is zero.

/// How much memory an Ult object needs behind it, per thing it was sized for.
///
/// Both `GetWorkAreaSize` and `Create` are implemented here and nothing is stored in the block
/// (an Ult object's state lives in this crate's table), so the size is a choice: non-zero, so a
/// caller sees a successful sizing, and proportional to the request.
const ULT_WORK_AREA_PER_OBJECT: u64 = 0x80;

/// A fixed part of the work area, so a request for nothing still gets a real block.
const ULT_WORK_AREA_HEADER: u64 = 0x100;

/// `sceUltWaitingQueueResourcePoolGetWorkAreaSize(threads, syncObjects)`.
///
/// Arity two, read off the guest's calls: the third register holds a leftover.
fn ult_pool_work_area_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ULT_WORK_AREA_HEADER
        + args[0]
            .saturating_add(args[1])
            .saturating_mul(ULT_WORK_AREA_PER_OBJECT)
}

/// `sceUltUlthreadRuntimeGetWorkAreaSize(threads, workerThreads)`, arity two on the same
/// evidence.
fn ult_runtime_work_area_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ULT_WORK_AREA_HEADER
        + args[0]
            .saturating_add(args[1])
            .saturating_mul(ULT_WORK_AREA_PER_OBJECT)
}

/// `sceUltInitialize(...)`: brings the library up.
///
/// What the arguments select is not established and none is read; this reports that the library
/// is available, which is all a caller can act on.
fn ult_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `_sceUltWaitingQueueResourcePoolCreate(pool, name, threads, syncObjects, workArea, optParam)`.
///
/// Constructs a named pool where the guest asked. The handle goes in the object's first word, as
/// for `_sceUltMutexCreate`, and the guest reads it back from there. The work area is accepted
/// and not written to (see [`ULT_WORK_AREA_PER_OBJECT`]).
fn ult_pool_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ult_construct(args)
}

/// `_sceUltUlthreadRuntimeCreate(runtime, name, threads, workerThreads, workArea, optParam)`.
///
/// The same shape as the pool. Constructing a runtime does not run threads on it; see
/// [`ult_ulthread_create`].
fn ult_runtime_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ult_construct(args)
}

/// The half a pool and a runtime share: a named object, sized by two counts.
fn ult_construct(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, name) = (args[0], args[1]);
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_ult_object(&unsafe { read_name(name) }, args[2], args[3]);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `_sceUltMutexCreate(mutex, name, optParam)`: constructs a named mutex where the guest asked.
fn ult_mutex_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, name) = (args[0], args[1]);
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create(sync::Recursion::Allowed, &unsafe { read_name(name) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `_sceUltMutexLock(mutex)`: blocks until the mutex is held by this thread.
fn ult_mutex_lock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::acquire(handle, thread::adopt("main"), sync::Blocking::Forever) {
        Some(sync::Acquisition::Locked) => OK,
        _ => u64::from(GuestError::InvalidArgument.as_raw()),
    }
}

/// `_sceUltMutexUnlock(mutex)`: releases a lock this thread holds.
fn ult_mutex_unlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::unlock(handle, thread::adopt("main")) {
        Some(true) => OK,
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NOT_OWNER).as_raw()),
    }
}

/// `_sceUltMutexTryLock(mutex)`: takes the mutex only if free, and says so when it is not.
fn ult_mutex_trylock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::acquire(handle, thread::adopt("main"), sync::Blocking::Never) {
        Some(sync::Acquisition::Locked) => OK,
        Some(sync::Acquisition::Busy | sync::Acquisition::Deadlock) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltMutexDestroy(mutex)`: releases the object and clears the guest's handle. An
/// uninitialised word names nothing to release, which is not an error.
fn ult_mutex_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = mutex_at(args[0]) {
        sync::destroy(handle);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u64(args[0], 0) };
    }
    OK
}

/// The mutex each libSceUlt condition variable was bound to at creation, by condition handle.
///
/// libSceUlt binds the mutex when the variable is created and its `Wait` takes only the variable,
/// unlike the POSIX and C-runtime pairs. This keeps the binding so `Wait` can release and retake
/// the right lock.
fn ult_cond_mutex() -> &'static Mutex<std::collections::BTreeMap<sync::CondHandle, u64>> {
    static MAP: OnceLock<Mutex<std::collections::BTreeMap<sync::CondHandle, u64>>> =
        OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// `_sceUltConditionVariableCreate(cv, name, mutex, optParam)`: constructs a condition variable
/// bound to a mutex, where the guest asked.
fn ult_cond_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, name, mutex) = (args[0], args[1], args[2]);
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_cond(&unsafe { read_name(name) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if let Ok(mut map) = ult_cond_mutex().lock() {
        map.insert(handle, mutex);
    }
    OK
}

/// `_sceUltConditionVariableSignal(cv)`: wakes one waiter.
fn ult_cond_signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_signal) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltConditionVariableSignalAll(cv)`: wakes every waiter.
fn ult_cond_signal_all(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_broadcast) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltConditionVariableWait(cv)`: waits until signalled, releasing the bound mutex around the
/// wait and retaking it after, with the same non-atomicity as the POSIX pair.
fn ult_cond_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(cond) = cond_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let mutex_ptr = ult_cond_mutex()
        .lock()
        .ok()
        .and_then(|map| map.get(&cond).copied())
        .unwrap_or(0);
    let mutex = mutex_at(mutex_ptr);
    let by = thread::adopt("main");
    if let Some(handle) = mutex {
        sync::unlock(handle, by);
    }
    let woken = sync::cond_wait(cond, None);
    if let Some(handle) = mutex {
        sync::acquire(handle, by, sync::Blocking::Forever);
    }
    match woken {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltConditionVariableDestroy(cv)`: releases the object and clears the guest's handle.
fn ult_cond_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = cond_at(args[0]) {
        sync::cond_destroy(handle);
        if let Ok(mut map) = ult_cond_mutex().lock() {
            map.remove(&handle);
        }
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u64(args[0], 0) };
    }
    OK
}

/// A handle for a created libSceUlt thread.
///
/// A non-zero monotonic counter: the threads are created but not run, so nothing dereferences it.
fn next_ult_thread() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// `_sceUltUlthreadCreate(ulthread, name, entry, arg, context, sizeContext, ...)`: creates a
/// cooperative thread.
///
/// Created, not run. A libSceUlt thread runs only when the running thread yields to the
/// scheduler, and no scheduler is built, so this records the thread and answers success. Running
/// the entry synchronously instead would hang on the first blocking wait the worker makes.
fn ult_ulthread_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, next_ult_thread()) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelAllocateMainDirectMemory(len, alignment, memory_type, physical_out)`.
///
/// Reserves from the main pool without the caller choosing a physical address, and writes the
/// address it chose where the caller asked. The first argument is a length in observed calls;
/// the rest follow the FreeBSD-analogous shape. The alignment is honoured and the type is
/// recorded.
fn allocate_main_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (len, alignment, memory_type, out) = (args[0], args[1], args[2], args[3]);

    // Refused rather than answered with a made-up address: a caller asking for nothing, or with
    // nowhere to be told the answer.
    if len == 0 || out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    let Ok(memory_type) = u32::try_from(memory_type) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Ok(mut guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };

    // Validated as passed, before being widened to the pool's minimum; widening first would make
    // every value look like a power of two. Zero means "no preference".
    if alignment != 0 && !alignment.is_power_of_two() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // Never weaker than the pool's own; a larger alignment is a hardware requirement.
    let align = alignment.max(direct::DIRECT_ALIGN);
    let Some(address) = guard.allocate_aligned(len, align, memory_type) else {
        // Out of memory is a real answer, distinct from a missing function, and a guest can shrink its
        // request. The guest gets one code for a full pool, a fragmented one and an unplaceable
        // alignment, so the log says which it was, with the numbers.
        tracing::warn!(
            concat!(
                "sceKernelAllocateMainDirectMemory refused {:#x} at alignment {:#x} ",
                "(asked {:#x}) - largest placeable span is {:#x}, {:#x} free in total, across ",
                "{} region(s): {}"
            ),
            len,
            align,
            alignment,
            guard.largest_free_at(align),
            guard.available(),
            guard.regions().len(),
            direct::describe_regions(guard.regions(), REFUSAL_REGIONS),
        );
        return u64::from(GuestError::NoMemory.as_raw());
    };
    drop(guard);

    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, address) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// How many regions a refused allocation lists before summarising the rest.
///
/// A fragmented pool holds thousands of regions, which would bury the numbers that matter.
const REFUSAL_REGIONS: usize = 12;

/// How many times a placement steps past a conflict before giving up.
///
/// The arena counter steps an address rather than consulting the map, so a base it hands back
/// can already be held by a range the guest reserved at a hint inside the arena. Shared with
/// `sceKernelReserveVirtualRange`.
const CONFLICT_RETRIES: usize = 16;

/// Where guest-requested mappings are placed when the guest expresses no preference.
///
/// Clear of the image, the stacks, the thunk table and the thunk data blocks
/// (`orbistoun_thunk::SUGGESTED_DATA_BASE`), so a stray pointer into any of them is recognisable
/// by its address alone, with terabytes of room below the user-space ceiling (D463).
pub const MAPPING_BASE: u64 = 0x0000_7400_0000_0000;

/// The address space guest mappings live in.
///
/// Separate from the loader's, so a mapping bug cannot overwrite a segment of the image.
fn mappings() -> &'static Mutex<orbistoun_mem::AddressSpace> {
    static SPACE: OnceLock<Mutex<orbistoun_mem::AddressSpace>> = OnceLock::new();
    SPACE.get_or_init(|| Mutex::new(orbistoun_mem::AddressSpace::new()))
}

/// Regions the guest can read that this crate did not map itself: the loaded image, the guest
/// stack, the main-thread TLS block. The loader and worker own them, so [`mappings`] never sees
/// them; `virtual_query` consults this too, because a guest asking about its own code or stack
/// expects a mapping (D446).
fn noted_regions() -> &'static Mutex<Vec<(u64, u64)>> {
    static NOTED: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    NOTED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records a `[base, base + len)` region the guest can read but this crate did not map, so
/// `virtual_query` answers for it. Stored as `(start, end)`; a region already noted is not
/// duplicated, so the worker may call this on every run.
pub fn note_region(base: u64, len: u64) {
    if len == 0 {
        return;
    }
    let end = base.saturating_add(len);
    if let Ok(mut noted) = noted_regions().lock() {
        if !noted.iter().any(|&(b, e)| b == base && e == end) {
            noted.push((base, end));
        }
    }
}

/// Forgets every noted region, so one test does not query another's regions.
#[cfg(test)]
pub fn clear_noted_regions() {
    if let Ok(mut noted) = noted_regions().lock() {
        noted.clear();
    }
}

/// The `[start, end)` of the region containing `addr`, or `None` when nothing maps it.
///
/// Consults the runtime mappings this crate hands out, the regions the worker noted (the image,
/// the TLS block), and the stacks: this thread's own if it has one, else the main stack span
/// (D446).
fn region_containing(addr: u64) -> Option<(u64, u64)> {
    let holds = |(base, len): (u64, u64)| {
        let end = base.saturating_add(len);
        (addr >= base && addr < end).then_some((base, end))
    };
    if let Ok(space) = mappings().lock() {
        if let Some(region) = space
            .regions()
            .iter()
            .find(|r| addr >= r.base && addr < r.base.saturating_add(r.len))
        {
            return Some((region.base, region.base.saturating_add(region.len)));
        }
    }
    if let Ok(noted) = noted_regions().lock() {
        if let Some(&(base, end)) = noted.iter().find(|&&(b, e)| addr >= b && addr < e) {
            return Some((base, end));
        }
    }
    if let Some(span) = thread::this_stack().and_then(holds) {
        return Some(span);
    }
    STACK_SPAN.get().copied().and_then(holds)
}

/// Whether `[base, base + len)` lies wholly inside `region`, given as `[start, end)`.
///
/// Pure, so the rule is testable without a mapping. Saturating: a guest may ask for a length that
/// overflows, and a wrapped sum would report a range as contained.
const fn range_within(base: u64, len: u64, region: (u64, u64)) -> bool {
    let (start, end) = region;
    base >= start && base.saturating_add(len) <= end
}
/// The region wholly covering `[base, base + len)`, from every place one is recorded.
///
/// One region, not the union of several: a range crossing from one region into another spans the
/// gap between them, which nothing placed.
fn region_covering(base: u64, len: u64) -> Option<(u64, u64)> {
    let region = region_containing(base)?;
    range_within(base, len, region).then_some(region)
}

/// Whether `[base, base + len)` is guest memory the guest can read now: one region wholly covering
/// it, and a runtime mapping only if its protection allows reads.
///
/// Answered from the tables as they stand, so memory the guest mapped after entry counts, such as
/// a command buffer built in direct memory.
#[must_use]
pub fn is_guest_readable(base: u64, len: u64) -> bool {
    guest_range_allows(base, len, false)
}

/// Whether `[base, base + len)` is guest memory the guest can write now: a runtime mapping wholly
/// covering it whose protection allows writes.
///
/// Narrower than [`is_guest_readable`]: the noted regions and the stacks carry no protection here,
/// and image text is not writable. The command processor writes guest memory through this, and a
/// write into a read-only page would fault the host.
#[must_use]
pub fn is_guest_writable(base: u64, len: u64) -> bool {
    guest_range_allows(base, len, true)
}

/// Adjacent runtime mappings count as one range, so a surface batch-mapped in 2 MiB pieces is one
/// span. Only regions that touch are joined, each must grant the access, and a gap anywhere
/// refuses the whole range (D446).
fn guest_range_allows(base: u64, len: u64, write: bool) -> bool {
    if len == 0 {
        return false;
    }
    let end = base.saturating_add(len);
    if let Ok(space) = mappings().lock()
        && space
            .regions()
            .iter()
            .any(|r| base >= r.base && base < r.base.saturating_add(r.len))
    {
        let mut cursor = base;
        while cursor < end {
            let Some(region) = space
                .regions()
                .iter()
                .find(|r| cursor >= r.base && cursor < r.base.saturating_add(r.len))
            else {
                return false;
            };
            let allowed = if write {
                region.protection.write
            } else {
                region.protection.read
            };
            if !allowed {
                return false;
            }
            cursor = region.base.saturating_add(region.len);
        }
        return true;
    }
    !write && region_covering(base, len).is_some()
}

/// The next address to place a mapping at.
///
/// Bump-allocated and never reused, so a guest that unmaps and remaps is never handed an address
/// it still holds a stale pointer to.
fn next_mapping_base(len: u64) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(MAPPING_BASE);
    // Stepped by the host's reservation granularity, not the guest page size: Windows rounds a
    // reservation base down to 64 KiB, so a base must satisfy the host.
    let unit = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    // Padded by one unit so two mappings never share one. Saturating rather than panicking.
    let step = len
        .checked_next_multiple_of(unit)
        .unwrap_or(u64::MAX)
        .saturating_add(unit);
    NEXT.fetch_add(step, Ordering::Relaxed)
}

/// Virtual addresses already handed out, as `(address, len)` by the physical offset mapped.
///
/// A guest maps a physical range, fills it, and maps it again expecting its data back, so a second
/// map of the same physical memory returns the first address. An entry holds only while both the
/// physical memory and the mapping exist, and only for maps no longer than the one it records.
fn physical_mappings() -> &'static Mutex<std::collections::BTreeMap<u64, (u64, u64)>> {
    static MAPPED: OnceLock<Mutex<std::collections::BTreeMap<u64, (u64, u64)>>> = OnceLock::new();
    MAPPED.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Forgets every alias whose physical offset lies in `[start, start + len)`: the memory was
/// released, so a later allocation of it is new memory.
fn forget_physical(start: u64, len: u64) {
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.retain(|&physical, _| physical < start || physical - start >= len);
    }
}

/// Forgets every alias whose mapping starts in `[address, address + len)`: the guest unmapped it.
fn forget_mapped(address: u64, len: u64) {
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.retain(|_, &mut (base, _)| base < address || base - address >= len);
    }
    if let Ok(mut reserved) = reservations().lock() {
        reserved.retain(|&(base, _)| base < address || base - address >= len);
    }
    if let Ok(mut flexible) = flexible_mappings().lock() {
        flexible.retain(|&base| base < address || base - address >= len);
    }
    if let Ok(mut requested) = requested_protections().lock() {
        requested.retain(|&(base, _, _)| base < address || base - address >= len);
    }
}

/// Ranges `sceKernelReserveVirtualRange` handed out, as `(base, len)`.
///
/// A reservation is address space, not memory. orbistoun reserves and backs in one step, so a
/// range here with no direct mapping placed in it is answered uncommitted and inaccessible; a
/// guest fills a reservation only when it reads as empty.
fn reservations() -> &'static Mutex<Vec<(u64, u64)>> {
    static RESERVED: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    RESERVED.get_or_init(|| Mutex::new(Vec::new()))
}

/// `sceKernelMapNamedDirectMemory(addr, len, prot, flags, physical, alignment)`: gives the guest a
/// virtual address for physical memory `sceKernelAllocateMainDirectMemory` reserved. The name is
/// the seventh argument and the trampoline spills six, so it is not read.
fn map_named_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Every way out that is not success is recorded here, in one place, so a refused map is not
    // mistaken for one never asked for. The base recorded is what the guest requested, read through
    // `args[0]` (the `void **` the answer is written back through); zero means no preference.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let requested = unsafe { guest::read_u64(args[0]) }.unwrap_or(0);
    let outcome = map_named_direct_memory_inner(args);
    if outcome != OK {
        mapped::note_failed(
            requested,
            args[1],
            &format!("answered {outcome:#x}"),
            requested != 0,
        );
    }
    outcome
}

/// The body, so the refusal note in the caller has one place to sit.
fn map_named_direct_memory_inner(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // On by default; the switch exists for bisecting.
    if !direct::configured().map_direct_memory {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let (out, len, prot, physical, alignment) = (args[0], args[1], args[2], args[4], args[5]);

    // Already mapped and covering what is asked for: the guest gets the address it had, with its
    // data. A longer map is a different mapping.
    if let Ok(mapped) = physical_mappings().lock() {
        if let Some(&(existing, existing_len)) = mapped.get(&physical)
            && existing_len >= len
        {
            drop(mapped);
            // SAFETY: an address the guest passed for this call, valid by its contract.
            return if unsafe { guest::write_u64(out, existing) } {
                OK
            } else {
                u64::from(GuestError::InvalidArgument.as_raw())
            };
        }
    }

    if out == 0 || len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if alignment != 0 && !alignment.is_power_of_two() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // The guest may name the address it wants by leaving it in the destination; zero is
    // "anywhere". Honoured rather than overridden, since a guest given a different address than it
    // asked for corrupts itself.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let requested = unsafe { guest::read_u64(out) }.unwrap_or(0);
    let base = if requested == 0 {
        next_mapping_base(len)
    } else {
        requested
    };
    // Checked, because `next_multiple_of` panics on overflow and a guest may pass any value,
    // including the all-ones word some callers use for "no preference" (D156).
    let align = alignment
        .max(orbistoun_mem::allocation_granularity())
        .max(orbistoun_core::GUEST_PAGE_SIZE);
    let (Some(base), Some(len)) = (
        checked_next_multiple_of(base, align),
        checked_next_multiple_of(len, orbistoun_core::GUEST_PAGE_SIZE),
    ) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };

    let protection = protection_from_guest(prot);
    let Ok(mut space) = mappings().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    // Reserve-then-map is one range, reserved once. A guest carves a range with
    // `sceKernelReserveVirtualRange` and then places physical memory inside it with this call; the
    // reservation already backs those pages, so mapping into it is a re-protect (D460). Reserving
    // again would conflict with the reservation and answer `NoMemory`. An address the guest did not
    // pre-reserve is a fresh mapping.
    let mut base = base;
    let mut placed = if space.owns(base, len) {
        space.protect(base, len, protection)
    } else {
        space.reserve(base, len, protection).map(|_| ())
    };
    // Retried past a conflict only when the guest expressed no preference: the arena counter steps
    // an address, not the map, so a base it hands back can already be held. A guest that named an
    // address is refused rather than moved.
    if placed.is_err() && requested == 0 {
        for _ in 0..CONFLICT_RETRIES {
            let Some(next) = checked_next_multiple_of(next_mapping_base(len), align) else {
                break;
            };
            base = next;
            placed = space.reserve(base, len, protection).map(|_| ());
            if placed.is_ok() {
                break;
            }
        }
    }
    if placed.is_err() {
        // Not recorded here: the wrapper records every way out that is not success.
        return u64::from(GuestError::NoMemory.as_raw());
    }
    drop(space);
    mapping_placed(base, len, protection, requested != 0);
    note_requested_protection(base, len, prot);

    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, base) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.insert(physical, (base, len));
    }
    OK
}

/// `sceKernelBatchMap(entries, count, completed)`: maps a run of direct-memory pieces in one call.
///
/// Display bring-up uses it: one physical block from `sceKernelAllocateMainDirectMemory`,
/// batch-mapped as sixteen 2 MiB pages at the GPU virtual addresses it renders to. Each entry is
/// placed at exactly the `vaddr` it names and never relocated.
///
/// `entries` points at `count` 32-byte records: `vaddr` (8), `paddr` (8), `len` (8), `prot` (1)
/// with three bytes of padding, `flags` (4). `completed` receives how many were mapped, so a
/// caller can tell a partial map from a total failure; the call answers `0` when every entry
/// succeeded and the kernel's error otherwise.
fn batch_map(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Bytes one `obs_batch_map_entry` occupies: `vaddr`(8) `paddr`(8) `len`(8) `prot`(1)+pad(3)
    /// `flags`(4).
    const ENTRY_SIZE: u64 = 32;

    if !direct::configured().map_direct_memory {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let (entries, count, completed) = (args[0], args[1], args[2]);
    if entries == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    let mut done: u64 = 0;
    let mut outcome = OK;
    for index in 0..count {
        let Some(entry) = index
            .checked_mul(ENTRY_SIZE)
            .and_then(|offset| entries.checked_add(offset))
        else {
            outcome = u64::from(GuestError::InvalidArgument.as_raw());
            break;
        };
        let (Some(vaddr), Some(paddr), Some(len), Some(prot_word)) = (
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::read_u64(entry) },
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::read_u64(entry.wrapping_add(8)) },
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::read_u64(entry.wrapping_add(16)) },
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::read_u64(entry.wrapping_add(24)) },
        ) else {
            outcome = u64::from(GuestError::InvalidArgument.as_raw());
            break;
        };
        // `prot` is the low byte of the word at offset 24; the padding and `flags` bytes are not read.
        let placed = map_direct_at(vaddr, paddr, len, prot_word & 0xff);
        if placed != OK {
            outcome = placed;
            break;
        }
        done += 1;
    }
    // Written whatever the outcome, so a stopped batch still says how far it got.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let _ = unsafe { guest::write_u64(completed, done) };
    outcome
}

/// Maps `len` bytes of physical memory `physical` at the exact guest virtual address `base` with
/// `prot`, recording the mapping and its physical alias.
///
/// The placement half of a direct-memory map. [`map_named_direct_memory_inner`] adds the
/// "anywhere" search, the already-mapped return and the `void**` protocol; a batch entry names
/// its own address and needs none of them. Answers `OK` or a [`GuestError`] raw code.
fn map_direct_at(base: u64, physical: u64, len: u64, prot: u64) -> u64 {
    if base == 0 || len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let align = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    let (Some(base), Some(len)) = (
        checked_next_multiple_of(base, align),
        checked_next_multiple_of(len, orbistoun_core::GUEST_PAGE_SIZE),
    ) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let protection = protection_from_guest(prot);
    let Ok(mut space) = mappings().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    // Reserve-then-map, as for the named map: a range the guest already reserved is re-protected in
    // place (D460).
    let placed = if space.owns(base, len) {
        space.protect(base, len, protection)
    } else {
        space.reserve(base, len, protection).map(|_| ())
    };
    if placed.is_err() {
        return u64::from(GuestError::NoMemory.as_raw());
    }
    drop(space);
    mapping_placed(base, len, protection, true);
    note_requested_protection(base, len, prot);
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.insert(physical, (base, len));
    }
    OK
}

/// How many mappings this run filled.
///
/// Counted so a run can show the fill diagnostic fired: a fill that changed nothing and one that
/// never ran produce identical output otherwise (D325).
static FILLED_MAPPINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bytes filled, alongside [`FILLED_MAPPINGS`].
static FILLED_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the direct-memory fill has done, as `(mappings, bytes)`.
///
/// `(0, 0)` from a run that asked for a fill means the diagnostic did not fire.
#[must_use]
pub fn direct_fill_done() -> (u64, u64) {
    use std::sync::atomic::Ordering;
    (
        FILLED_MAPPINGS.load(Ordering::Relaxed),
        FILLED_BYTES.load(Ordering::Relaxed),
    )
}

/// Where a placed module keeps the functions that have to run before its code is usable.
///
/// `array` is the runtime address of `DT_INIT_ARRAY` and `count` how many pointers it holds;
/// `init` is `DT_INIT`, zero when the module has none. All three are addresses in the guest's
/// own space, already offset by wherever the module was placed.
#[derive(Clone, Copy, Debug)]
pub struct ModuleInitialisers {
    /// `DT_INIT`, run before the array. Zero when absent.
    pub init: u64,
    /// `DT_INIT_ARRAY`, as a runtime address.
    pub array: u64,
    /// How many function pointers the array holds.
    pub count: u64,
}

/// What the loader placed, by the library name the module answers to.
static PLACED_MODULES: Mutex<Vec<(String, ModuleInitialisers)>> = Mutex::new(Vec::new());

/// Tells this crate where a placed module's initialisers are, so a later start can run them.
///
/// A module's pages live in the loader's address space and `sceKernelLoadStartModule` is handed
/// only a path, so the loader records them here (D515). The array's contents are read at start
/// time, after relocation has written the pointers.
pub fn note_module_initialisers(library: &str, initialisers: ModuleInitialisers) {
    if let Ok(mut placed) = PLACED_MODULES.lock() {
        if !placed.iter().any(|(name, _)| name == library) {
            placed.push((library.to_owned(), initialisers));
        }
    }
}

/// Runs every placed module's initialisers, whether or not the guest asked for it.
///
/// A diagnostic, off by default: a module starts when the guest starts it (D515), and a module
/// bound only as an import is never started by name. A run with this on shows whether a missing
/// initialisation order explains a wall. Answers how many modules were started and how many
/// initialisers ran, so a run that intervened and did nothing says so (D325).
pub fn start_every_placed_module() -> (usize, u64) {
    let Ok(placed) = PLACED_MODULES.lock() else {
        return (0, 0);
    };
    // Collected first: `run_initialisers` enters guest code, which can call back into this crate,
    // and holding the lock across that would deadlock.
    let all: Vec<(String, ModuleInitialisers)> = placed.clone();
    drop(placed);
    let mut ran = 0;
    for (library, initialisers) in &all {
        let count = run_initialisers(*initialisers);
        started(library, u64::MAX, count);
        ran += count;
    }
    (all.len(), ran)
}

/// The initialisers recorded for the module a guest path names, if any.
///
/// Matched on the leaf of the path: the guest names a module by its sandbox path
/// (`/app0/Media/Modules/Util.prx`) and the loader by its library name, and the file name is
/// the part both share.
fn initialisers_for(path: &str) -> Option<ModuleInitialisers> {
    let leaf = path.rsplit('/').next()?;
    // The loader knows a module by its import library name, which has no extension: the guest asks
    // for `Il2CppUserAssemblies.prx` and the loader placed `Il2CppUserAssemblies`.
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    let placed = PLACED_MODULES.lock().ok()?;
    placed
        .iter()
        // Case-insensitively, as the loader matches a title's files to its import names (D482).
        .find(|(library, _)| library.eq_ignore_ascii_case(stem))
        .map(|(_, initialisers)| *initialisers)
}

/// Runs a placed module's `DT_INIT` and `DT_INIT_ARRAY`, and says how many ran.
///
/// Title modules export no `module_start` and carry `DT_INIT` and `DT_INIT_ARRAY`, the ELF
/// mechanism that reaches a C++ module's static constructors. The order is `DT_INIT`, then the
/// array in ascending order, as the System V ABI specifies.
fn run_initialisers(initialisers: ModuleInitialisers) -> u64 {
    let mut ran = 0;
    if initialisers.init != 0 {
        // SAFETY: `DT_INIT` of a module this loader placed, relocated and protected
        // executable - a function pointer the module itself declared, taking no arguments.
        if unsafe { thread::call_guest(initialisers.init, [0, 0, 0]) }.is_some() {
            ran += 1;
        }
    }
    for slot in 0..initialisers.count {
        let at = initialisers.array.saturating_add(slot.saturating_mul(8));
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let Some(entry) = (unsafe { guest::read_u64(at) }) else {
            // The array is outside anything readable, so the recorded address is wrong; stop.
            break;
        };
        // A null or `-1` is an empty slot the ABI allows, and a runtime skips it.
        if entry == 0 || entry == u64::MAX {
            continue;
        }
        // SAFETY: a pointer out of the module's own `DT_INIT_ARRAY`, which relocation has
        // written, into text this loader protected executable. The three arguments are the
        // `(argc, argv, envp)` an init-array entry is permitted to take and may ignore.
        if unsafe { thread::call_guest(entry, [0, 0, 0]) }.is_some() {
            ran += 1;
        }
    }
    ran
}

/// Every `/app0` module a guest asked to load and start that did not start, with its handle.
///
/// The paths are kept, because a count alone would not say which modules.
static STARTED_NOTHING: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// Records that a module was given a handle and never started.
fn started_nothing(path: &str, handle: u64) {
    if let Ok(mut loads) = STARTED_NOTHING.lock() {
        loads.push((path.to_owned(), handle));
    }
}

/// Every module that was started, and how many initialisers each ran.
static STARTED: Mutex<Vec<(String, u64, u64)>> = Mutex::new(Vec::new());

/// Records that a module was started, and how many of its initialisers ran.
///
/// A count of zero is reported, not treated as success: a module whose array held nothing
/// callable gets the same handle as one that ran every constructor.
fn started(path: &str, handle: u64, ran: u64) {
    if let Ok(mut loads) = STARTED.lock() {
        loads.push((path.to_owned(), handle, ran));
    }
}

/// One line for a run report: the event queues a guest made, and what each was used for.
///
/// [`None`] when none was created. Registrations, waits and deliveries together tell a queue
/// nobody uses from one a thread is stuck on (D524).
#[must_use]
pub fn equeue_summary() -> Option<String> {
    let queues = sync::equeue_summary();
    if queues.is_empty() {
        return None;
    }
    let named = queues
        .iter()
        .map(|q| {
            // The starved case is called out with its reason: a starved queue has no producer, and graphics
            // queues wait on work completions that are posted only for work that ran (D705).
            let verdict = if q.waited > 0 && q.delivered == 0 {
                "  <- waited on, never delivered: nothing produces its events; a graphics queue waits on driver-work completion, not posted yet (D705)"
            } else {
                ""
            };
            format!(
                "{:?} {:#x} ({} registered, {} waits, {} delivered){verdict}",
                q.name, q.handle, q.registered, q.waited, q.delivered
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("{} event queue(s): {named}", queues.len()))
}

/// One line for a run report: which modules were handed a handle and never started.
///
/// [`None`] when the guest asked for none. A gap report, not a diagnostic: a guest calling into
/// one of these modules runs code whose constructors never ran.
#[must_use]
pub fn module_start_summary() -> Option<String> {
    let mut parts = Vec::new();

    if let Ok(started) = STARTED.lock() {
        if !started.is_empty() {
            let named = started
                .iter()
                .map(|(path, handle, ran)| {
                    // `u64::MAX` is the bulk start before entry, which has no handle.
                    let how = if *handle == u64::MAX {
                        "before entry".to_owned()
                    } else {
                        format!("handle {handle:#x}")
                    };
                    format!("{} ({ran} initialiser(s), {how})", leaf_of(path))
                })
                .collect::<Vec<_>>()
                .join(", ");
            // A module that ran nothing is called out separately: its array held nothing callable.
            let empty = started.iter().filter(|(_, _, ran)| *ran == 0).count();
            let mut line = format!("{} module(s) started: {named}", started.len());
            if empty > 0 {
                use std::fmt::Write as _;
                let _ = write!(
                    line,
                    " - {empty} ran NO initialiser, which is not the same as having started"
                );
            }
            parts.push(line);
        }
    }

    // What the loader placed, by name, rather than inferred from import lists or file names.
    if let Ok(placed) = PLACED_MODULES.lock() {
        if !placed.is_empty() {
            let named = placed
                .iter()
                .map(|(library, i)| format!("{library}({} init)", i.count + u64::from(i.init != 0)))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("loader placed with initialisers: {named}"));
        }
    }

    if let Ok(loads) = STARTED_NOTHING.lock() {
        if !loads.is_empty() {
            let named = loads
                .iter()
                .map(|(path, handle)| format!("{} (handle {handle:#x})", leaf_of(path)))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!(
                "{} module(s) got a handle and were NOT started (the loader recorded no initialisers for them): {named}",
                loads.len()
            ));
        }
    }

    (!parts.is_empty()).then(|| parts.join("; "))
}

/// The file name a path ends in, or the whole path when it has no separator.
fn leaf_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// One line for a run report: what the direct-memory fill did.
///
/// [`None`] when no fill was asked for. A run that asked for a fill and reports zero has tested
/// nothing (D325).
#[must_use]
pub fn direct_fill_summary() -> Option<String> {
    orbistoun_env::DIRECT_FILL.get()?;
    let (mappings, bytes) = direct_fill_done();
    Some(if mappings == 0 {
        "direct-memory fill asked for and never fired - nothing was tested".to_owned()
    } else {
        format!("direct-memory fill: {mappings} mapping(s), {bytes} bytes")
    })
}

/// Whether a mapping should be filled, and with what.
///
/// Pure, so the decision is testable without reserving anything. Writable mappings only: writing
/// to read-only or execute-only memory would fault inside the emulator.
const fn fill_for(byte: Option<u8>, protection: orbistoun_mem::Protection) -> Option<u8> {
    match byte {
        Some(byte) if protection.write => Some(byte),
        _ => None,
    }
}

/// The byte every fresh direct-memory mapping is filled with, if a run asked for one.
///
/// Read once, so the behaviour cannot change part-way through a run.
fn direct_fill() -> Option<u8> {
    static FILL: OnceLock<Option<u8>> = OnceLock::new();
    *FILL.get_or_init(|| {
        let raw = orbistoun_env::DIRECT_FILL.get()?;
        let byte = u8::from_str_radix(raw.trim_start_matches("0x"), 16).ok()?;
        // Zero is what the region already is, so asking for it is asking for nothing.
        (byte != 0).then_some(byte)
    })
}

/// Everything that has to happen once a mapping exists, in one place.
///
/// Direct memory, `mmap` and reserved virtual ranges all place guest mappings, and each calls
/// this rather than a list of obligations it could miss.
fn mapping_placed(base: u64, len: u64, protection: orbistoun_mem::Protection, hinted: bool) {
    // The sequence, when a run asked for it: a bump-allocated address depends on everything placed
    // before it, so runs are diffed by order.
    mapped::note(base, len, protection.read, hinted);
    // Published as guest memory the diagnostics may read, so a pointer into a later mapping dumps
    // as a guest address rather than a wild pointer (D387). Readable mappings only: dumping a page
    // the host refuses to read would fault inside the emulator.
    if protection.read {
        orbistoun_thunk::note_readable_range(base, len);
    }
    // The arena's extent, so a fault or a dump can say "guest mappings+0x..." rather than print a
    // bare address.
    note_arena_extent(base, len);
    fill_mapping(base, len, protection);
}

/// How far the mapping arena has been used, as an address one past the last byte placed.
///
/// Zero until the guest maps something, so [`arena_extent`] answers `None` for it.
static ARENA_END: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Records that `[base, base + len)` was placed, if it is inside the arena.
///
/// Mappings at a guest's hinted address (D459) are not counted: stretching the arena to cover a
/// hint far away would name every address between as arena.
fn note_arena_extent(base: u64, len: u64) {
    if let Some(end) = arena_end_of(base, len) {
        ARENA_END.fetch_max(end, std::sync::atomic::Ordering::Relaxed);
    }
}

/// How far the arena reaches once `[base, base + len)` is placed, or `None` if it is not in it.
///
/// Pure, so a test need not bump the shared counter a parallel test reads. Saturating: a wrapped
/// end would shrink the arena.
const fn arena_end_of(base: u64, len: u64) -> Option<u64> {
    if base < MAPPING_BASE {
        return None;
    }
    Some(base.saturating_add(len))
}

/// The span guest mappings have been placed in, or `None` if the guest mapped nothing.
///
/// The arena, not the set of mappings: it includes gaps and padding, so an address inside it was
/// not necessarily mapped. A fault report states separately whether the address was mapped
/// (D489).
#[must_use]
pub fn arena_extent() -> Option<(u64, u64)> {
    arena_span(ARENA_END.load(std::sync::atomic::Ordering::Relaxed))
}

/// The arena span implied by an end address, or `None` for an arena nothing has used.
///
/// `None` rather than a zero-length span, which the reporter already uses for an unused region
/// slot.
const fn arena_span(end: u64) -> Option<(u64, u64)> {
    if end <= MAPPING_BASE {
        return None;
    }
    Some((MAPPING_BASE, end - MAPPING_BASE))
}

/// Fills a fresh mapping, so reading it back is distinguishable from reading a zero.
///
/// Fresh host memory is zero, and so is an out-parameter nobody wrote; the fill separates the
/// two for direct memory as the stack and heap fills do (D325). Writable mappings only.
fn fill_mapping(base: u64, len: u64, protection: orbistoun_mem::Protection) {
    let Some(byte) = fill_for(direct_fill(), protection) else {
        return;
    };
    let (Ok(at), Ok(len)) = (usize::try_from(base), usize::try_from(len)) else {
        return;
    };
    FILLED_MAPPINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    FILLED_BYTES.fetch_add(len as u64, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: `base .. base + len` was just reserved writable by the call above, and
    // nothing else can have unmapped it - the guest is not running on another thread at
    // this point, because it is inside this call.
    unsafe {
        std::ptr::write_bytes(std::ptr::with_exposed_provenance_mut::<u8>(at), byte, len);
    }
}

/// Rounds `value` up to a multiple of `align`, or `None` if that overflows.
///
/// `u64::next_multiple_of` panics, and nothing reachable from a guest call may panic (D156).
const fn checked_next_multiple_of(value: u64, align: u64) -> Option<u64> {
    if align == 0 {
        return None;
    }
    value.checked_next_multiple_of(align)
}

/// Translates the guest's protection bits.
///
/// The values are the published POSIX `PROT_*` set (read 1, write 2, execute 4) a FreeBSD-derived
/// kernel uses; other bits are ignored. A request naming no access becomes read-only, since an
/// untouchable mapping is indistinguishable from a failed one.
fn protection_from_guest(prot: u64) -> orbistoun_mem::Protection {
    /// POSIX `PROT_READ`.
    const READ: u64 = 1;
    /// POSIX `PROT_WRITE`.
    const WRITE: u64 = 2;
    /// POSIX `PROT_EXEC`.
    const EXEC: u64 = 4;

    let (read, write, execute) = (prot & READ != 0, prot & WRITE != 0, prot & EXEC != 0);
    orbistoun_mem::Protection {
        // Readable if asked, when nothing was asked, and when writing was asked (D588): an x86-64 page
        // table entry has no read bit, so a writable page is readable, and `mmap(2)` permits granting
        // more access than requested, as FreeBSD on amd64 does.
        read: read || write || !execute,
        write,
        execute,
    }
}

/// The longest thread or object name read from the guest.
const MAX_NAME: usize = 64;

/// A NUL-terminated guest name of at most [`MAX_NAME`] bytes, lossily decoded; empty for null.
///
/// # Safety
///
/// `address` is under the `orbistoun_mem::guest` contract.
unsafe fn read_name(address: u64) -> String {
    // SAFETY: the caller's contract.
    let bytes = unsafe { guest::read_cstr(address, MAX_NAME) }.unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `scePthreadSelf()`.
///
/// The calling thread is adopted if the guest did not create it, because the process's first
/// thread runs guest code without being created and the guest still asks it this.
fn pthread_self(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread::adopt("main")
}

/// `scePthreadGetthreadid()`: the calling thread's unique integer id.
///
/// FreeBSD's `pthread_getthreadid_np`. The registry handle is a `u64` unique to the thread and
/// stable for its life, so it is the id (D452); the caller is adopted as for `scePthreadSelf`, so
/// the process's first thread gets a real id.
fn pthread_getthreadid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread::adopt("main")
}

/// POSIX thread-specific-data keys: `pthread_key_create` and the family a guest runtime builds
/// thread-local storage on (D453).
///
/// A key is a small integer from a monotonic counter. The bound value is per thread, held in a
/// `thread_local!` map, since a guest thread is a host thread; a thread that never set a key
/// reads null, as POSIX requires. The destructor passed to `pthread_key_create` is not stored and
/// never run, since nothing tears a guest thread down through this layer.
///
/// Reference: POSIX.1-2008 `pthread_key_create`, `pthread_setspecific`, `pthread_getspecific`,
/// `pthread_key_delete`.
static NEXT_TLS_KEY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// This thread's key-to-value bindings. Empty on a fresh thread, so every key reads null until it
/// is set.
#[allow(clippy::type_complexity)]
fn tls_values()
-> &'static std::thread::LocalKey<std::cell::RefCell<std::collections::HashMap<u32, u64>>> {
    thread_local! {
        static VALUES: std::cell::RefCell<std::collections::HashMap<u32, u64>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    &VALUES
}

/// `pthread_key_create(key_out, destructor)`: allocates a thread-specific-data key.
fn pthread_key_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // args[1] is the destructor, which is not stored (see the family note).
    let key = NEXT_TLS_KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u32(args[0], key) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_setspecific(key, value)`: binds a value to a key for the calling thread.
fn pthread_setspecific(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (key, value) = (args[0] as u32, args[1]);
    tls_values().with(|m| m.borrow_mut().insert(key, value));
    OK
}

/// `pthread_getspecific(key)`: the calling thread's value for a key, or null if unset.
fn pthread_getspecific(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let key = args[0] as u32;
    tls_values().with(|m| m.borrow().get(&key).copied().unwrap_or(0))
}

/// `pthread_key_delete(key)`: retires a key. The binding in the calling thread is dropped; a
/// retired id is not reused, so a stale use reads null rather than another key's value.
fn pthread_key_delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let key = args[0] as u32;
    tls_values().with(|m| m.borrow_mut().remove(&key));
    OK
}

/// `scePthreadCreate(thread, attr, entry, arg, name)`.
///
/// Always a real host thread. The stack size and affinity are read from the attribute block (see
/// [`thread_attributes`]); nothing else in it is interpreted.
fn pthread_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, attr, entry, argument, name) = (args[0], args[1], args[2], args[3], args[4]);
    if out == 0 || entry == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(name) };
    let start = thread::Start { entry, argument };
    // The attribute block the guest built through `scePthreadAttrSet*`: on the hardware a live
    // thread reads back the stack size and affinity its block asked for (obSCEne `031-stackattr`).
    let (affinity, stack) = thread_attributes(attr);

    // SAFETY: `entry` is a guest address the guest itself is asking to have called, in a
    // fully relocated image - the same contract the real call has. The thread body runs
    // guest instructions, which is the entire purpose of this emulator, and it runs on a
    // stack of its own so an overrun hits a guard page rather than host frames.
    let spawned = unsafe { thread::spawn(start, &name, affinity, 0, stack) };

    match spawned {
        Ok(handle) => {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            if !unsafe { guest::write_u64(out, handle) } {
                return u64::from(GuestError::InvalidArgument.as_raw());
            }
            OK
        }
        // Reported, not swallowed: a guest told its thread started when it did not would wait on it
        // forever.
        Err(_) => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// The affinity and stack size to spawn a thread with, decoded from a thread attribute block.
///
/// The pure half of [`thread_attributes`]; `stack_field` and `affinity_field` are the raw stored
/// values.
///
/// - Stack size: a size the guest set is honoured, as on the hardware (obSCEne `031-stackattr`:
///   a block asking for `0x181000` runs a thread on exactly that). The fresh default
///   ([`DEFAULT_ATTR_STACK_SIZE`], 64 KiB) and `0` (POSIX "use the default") are not honoured,
///   since a fresh attribute is not measured to bound a real thread and 64 KiB is far below
///   [`orbistoun_mem::stack::DEFAULT_STACK_SIZE`].
/// - Affinity: the mask is carried to the thread record, so `scePthreadAttrGet` reads it back;
///   it is stored, not applied, as [`pthread_attr_setaffinity`] does.
fn spawn_parameters(stack_field: u64, affinity_field: u64) -> (thread::Affinity, u64) {
    let stack = if stack_field != 0 && stack_field != DEFAULT_ATTR_STACK_SIZE {
        stack_field
    } else {
        orbistoun_mem::stack::DEFAULT_STACK_SIZE
    };
    (thread::Affinity(affinity_field), stack)
}

/// Reads the spawn parameters out of a guest thread attribute block, or the defaults for a null or
/// unreadable one.
///
/// `attr` points to the handle [`pthread_attr_init`] substituted, so this reads orbistoun's own
/// attribute object. The decode is [`spawn_parameters`]. Two fields are not read:
///
/// - Detach state: every guest thread is a host thread reclaimed when its body returns, so the
///   detach promise is kept by construction (see [`pthread_detach`]).
/// - Priority: `scePthreadAttrSetschedparam` refuses on a retail attribute on the hardware
///   (`0x8002002d`), so priority arrives through `scePthreadSetprio` instead.
fn thread_attributes(attr: u64) -> (thread::Affinity, u64) {
    if attr == 0 {
        return (
            thread::Affinity::default(),
            orbistoun_mem::stack::DEFAULT_STACK_SIZE,
        );
    }
    let Some(object) = attr_at(attr) else {
        return (
            thread::Affinity::default(),
            orbistoun_mem::stack::DEFAULT_STACK_SIZE,
        );
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let stack_field = unsafe { guest::read_u64(object + ATTR_STACK_SIZE) }.unwrap_or(0);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let affinity_field = unsafe { guest::read_u64(object + ATTR_AFFINITY) }.unwrap_or(0);
    spawn_parameters(stack_field, affinity_field)
}

/// `scePthreadJoin(thread, value)`.
///
/// Carries the joined thread's return value, which it left in `rax`, back through `value`.
fn pthread_join(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, value) = (args[0], args[1]);
    // Checked before use, because a handle is an address and an arbitrary guest value must not be
    // treated as one.
    if !thread::is_issued(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    if !thread::join(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    if value != 0 {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u64(value, thread::exit_value(handle)) };
    }
    OK
}

/// `sceKernelCreateSema(out, name, attr, initial, ceiling, opt)`.
///
/// The first argument is a stack address the guest expects to be filled, and writing it is
/// required (D171). The order of the remaining arguments is inferred from the shape of semaphore
/// interfaces, so the counts are clamped to values a guest can survive rather than trusted.
fn create_semaphore(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out == 0 {
        // Nowhere to put the answer; refused rather than leaving the guest's stack uninitialised.
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // Clamped, not trusted: if the argument order is not as assumed these are other values, and a
    // ceiling of four billion would be a wrong guess the host refuses far from here.
    let initial = u32::try_from(args[3]).unwrap_or(0);
    let ceiling = u32::try_from(args[4])
        .unwrap_or(u32::from(u16::MAX))
        .max(initial);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(args[1]) };

    let handle = sync::create_semaphore(initial, ceiling, &name);
    // Eight bytes. On the hardware, a guard word planted after an `int handle` reads back `0x0`
    // after this call, so eight bytes are written where the documented destination is four. A
    // guest built against the hardware may rely on that, so orbistoun writes the same. One guard
    // word cannot tell a 64-bit handle with a zero upper half from four bytes plus four cleared.
    //
    // SAFETY: the guest supplied this destination, which is the same contract the real call
    // has. Written unaligned because the guest's alignment is its own business, and an
    // address it has not mapped faults here exactly as it would have in the guest.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u64>(out as usize),
            u64::from(handle as u32),
        );
    }
    OK
}

/// The mutex attribute calls: `Init`, `Settype`, `Setprotocol`, `Destroy`.
///
/// Nothing downstream reads the attribute, since the mutexes here are `Condvar`-backed (see
/// `sync`), so accepting the calls is the whole of the work. Nothing is written to the attribute
/// object, so a guest calling a `Get` counterpart would read what its stack held (D171); this is
/// recorded as an assumption in the knowledge file. The names are confirmed by hash.
fn pthread_mutexattr_accept(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePthreadMutexInit(mutex, attr, name)`.
///
/// Writes an opaque handle where the guest expects its lock. The recursion mode comes from the
/// attribute's type (see [`mutex_recursion_from_attr`]).
fn pthread_mutex_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, attr, name) = (args[0], args[1], args[2]);
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(name) };
    let handle = sync::create(mutex_recursion_from_attr(attr), &name);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// The recursion mode a mutex attribute selects, from the type the guest set on it.
///
/// A null attribute is the default, a normal lock. The platform's type values (obSCEne
/// `015-sync/mutex-recursion`) are `2` recursive and `4` error-checking; anything else is a normal
/// lock, which a second acquisition by the owner deadlocks on. The type is read from the object
/// `scePthreadMutexattrSettype` wrote.
fn mutex_recursion_from_attr(attr: u64) -> sync::Recursion {
    if attr == 0 {
        return sync::Recursion::Forbidden;
    }
    let Some(object) = attr_at(attr) else {
        return sync::Recursion::Forbidden;
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    match unsafe { guest::read_u64(object + ATTR_TYPE) } {
        Some(2) => sync::Recursion::Allowed,
        Some(4) => sync::Recursion::Errorcheck,
        _ => sync::Recursion::Forbidden,
    }
}

/// Resolves the lock a guest pointer refers to.
///
/// `None` covers a statically initialised lock, filled with a constant at compile time and never
/// passed to init: the handle there is not one this crate issued. Answering success would let
/// every thread into the critical section at once.
fn mutex_at(pointer: u64) -> Option<sync::MutexHandle> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = unsafe { guest::read_u64(pointer) }?;
    (handle != sync::NO_MUTEX).then_some(handle)
}

/// `scePthreadMutexLock(mutex)`.
fn pthread_mutex_lock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let by = thread::adopt("main");
    match sync::acquire(handle, by, sync::Blocking::Forever) {
        Some(sync::Acquisition::Locked) => OK,
        // Two failures, two codes: a refusal is the guest deadlocking against itself on a non-recursive
        // lock; a miss is a handle naming nothing. This call waits forever, so `Busy` can only be the
        // self-relock.
        Some(sync::Acquisition::Busy | sync::Acquisition::Deadlock) => {
            u64::from(GuestError::InvalidArgument.as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadMutexUnlock(mutex)`.
fn pthread_mutex_unlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let by = thread::adopt("main");
    match sync::unlock(handle, by) {
        Some(true) => OK,
        // Not this thread's lock, or not held. The hardware answers an unlock of a mutex nobody holds
        // with the not-owner errno rather than an invalid argument (D398).
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::NOT_OWNER).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadMutexTrylock(mutex)`.
///
/// Must not answer `OK` when it fails: a guest branches on the answer, and success would send it
/// into a critical section it does not hold.
fn pthread_mutex_trylock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let by = thread::adopt("main");
    match sync::acquire(handle, by, sync::Blocking::Never) {
        Some(sync::Acquisition::Locked) => OK,
        // Held by somebody else, or the owner re-taking a normal lock: the ordinary outcome of this call,
        // answered with the busy errno as on the hardware (D256).
        Some(sync::Acquisition::Busy) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw())
        }
        // The owner re-taking an error-checking lock, which the hardware reports with the
        // invalid-argument errno (`0x8002_0016`) rather than busy.
        Some(sync::Acquisition::Deadlock) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadMutexDestroy(mutex)`.
///
/// The counterpart to `Init`; without it a guest that builds and tears down locks leaks them.
fn pthread_mutex_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // Cleared, so a second destroy or a use after destroy is told the handle is gone.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], sync::NO_MUTEX) };
    OK
}

/// `sceKernelGetProcessTime()`: microseconds since this process began.
///
/// Monotonic and measured from the process's own start: a wall clock would run backwards when
/// the host's clock is corrected, and an epoch would make two runs incomparable.
fn kernel_get_process_time(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Read through `clocks`, so the platform's process time and POSIX's monotonic time are one clock
    // and `ORBISTOUN_CLOCK=logical` governs both (D582). Microseconds; saturating, so an overflow
    // reports a stuck clock rather than a wrapped one.
    u64::try_from(orbistoun_hle::clocks::since_start_nanos() / 1_000).unwrap_or(u64::MAX)
}

/// `sceKernelGetProcessTimeCounter()`: the same elapsed time, counted in ticks.
///
/// A guest uses the microsecond call for printing and timeouts and the counter for dividing by a
/// frequency, so the two must keep the hardware's ratio: across one sleep the hardware read
/// `0x4fbb` microseconds against `0x1f12cd9` ticks.
fn kernel_get_process_time_counter(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ticks_since()
}

/// `sceKernelGetProcessTimeCounterFrequency()`: ticks per second for the counter above.
///
/// The same number the time stamp counter reports; the hardware answers `0x5f25_9b8e` to both.
fn kernel_get_process_time_counter_frequency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    TSC_HZ
}

/// Where the version sits in the structure this call answers with.
///
/// The guest reads a sixteen-bit value at this offset, shifts it left sixteen and compares it
/// against firmware bands (D403).
const SYSTEM_VERSION_AT: usize = 0x16;

/// How big the structure is.
///
/// Larger than the one field guests read, so a guest reading further finds bytes, filled with a
/// value that is obviously not data.
const SYSTEM_VERSION_BYTES: usize = 0x40;

/// The byte every unestablished field of that structure holds.
///
/// Not zero, which is a plausible value for most fields, so a guest acting on an unmodelled field
/// shows up as nonsense.
const SYSTEM_VERSION_FILL: u8 = 0xA5;

/// `syscall(649, kind, length, out)`: what system this is, which a guest needs before it can
/// start (D403).
///
/// Open-toolchain payloads ask for it, read a version out of the answer and pick a code path;
/// without it they print `Unable to initialize rtld` and exit. Read off the guest's own code: the
/// arguments `(2, 8, out)`, the answer is a pointer, and the guest reads sixteen bits at offset
/// 0x16, shifts them and compares against 0x0700FFFF, 0x085FFFFF, 0x093FFFFF and 0x103FFFFF.
/// That the bands are firmware versions is inferred. The rest of the structure is unmodelled
/// and filled with a marker byte.
fn vendor_system_version(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[2];
    let version = machine().firmware;
    if version == 0 {
        // Refused, not answered with zero: zero falls in the lowest band and would send the guest down
        // the path for the oldest system (D397).
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    }
    if out == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    let block = system_version_block(version);
    let Ok(at) = usize::try_from(out) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    // SAFETY: a guest-supplied `void **` under the identity mapping, which the guest
    // passed expecting to be written through - it reads the pointer back immediately.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), block);
    }
    OK
}

/// The structure this run answers with, built once and never freed.
///
/// The guest keeps the pointer and reads through it at any time.
fn system_version_block(version: u16) -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        // From the shared region, so the address the guest is handed repeats (D584). Filled rather than
        // zeroed, so an unestablished field reads as unset rather than as a valid zero.
        let at = orbistoun_mem::blocks::block(SYSTEM_VERSION_BYTES.div_ceil(8));
        let Ok(base) = usize::try_from(at) else {
            return 0;
        };
        let bytes = std::ptr::with_exposed_provenance_mut::<u8>(base);
        // SAFETY: `blocks::block` just handed this address out for at least
        // `SYSTEM_VERSION_BYTES`, and nothing else holds it yet.
        unsafe { std::ptr::write_bytes(bytes, SYSTEM_VERSION_FILL, SYSTEM_VERSION_BYTES) };
        // SAFETY: `SYSTEM_VERSION_AT + 2` is inside the same block, whose length the
        // constants above are declared against.
        let field = unsafe { bytes.add(SYSTEM_VERSION_AT) };
        let value = version.to_le_bytes();
        // SAFETY: two bytes into the block above, from a two-byte array on this frame; the
        // block came from `blocks::block` and overlaps nothing.
        unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), field, 2) };
        at
    })
}

/// What this run presents itself as.
///
/// Held in `orbistoun-core`, because the C library answers questions about the same machine and
/// the two crates cannot see each other (D394).
fn machine() -> &'static orbistoun_core::machine::Machine {
    orbistoun_core::machine::presented()
}

/// `sceKernelIsCex()`: whether this is a retail unit.
///
/// A boolean family must answer, since the placeholder is non-zero and reads as true.
/// orbistoun presents a retail unit, which is what the corpus is built for.
fn is_cex(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_retail())
}

/// `sceKernelIsDevkit()`: false. See [`is_cex`].
///
/// The spelling is `Devkit`, with a lower-case `k`, confirmed by hash; `DevKit` would be a
/// different symbol that no guest imports (D393).
fn is_devkit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_development_kit())
}

/// `sceKernelIsNeoMode()`: false, presenting a base unit rather than the more capable hardware.
fn is_neo_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_faster_revision())
}

/// `sceKernelIsDevelopmentMode()`: false. See [`is_devkit`].
fn is_development_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_development_mode())
}

/// `sceKernelIsTestKit()`: false. See [`is_cex`].
fn is_testkit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_test_kit())
}

/// `posix_getpagesize()`: the guest's page size, not the host's.
///
/// A host with 16K pages still presents the platform's 4K semantics.
fn getpagesize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::GUEST_PAGE_SIZE
}

/// `usleep(microseconds)`: sleeps, and reports success.
///
/// Returning immediately would make frame pacing and poll-with-sleep loops spin.
fn usleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let how_long = std::time::Duration::from_micros(args[0]);
    std::thread::sleep(how_long);
    // The clock advances by the slept span too, so a guest polling between sleeps sees time pass
    // the same way on every run (D582).
    orbistoun_hle::clocks::advance(how_long.as_nanos());
    OK
}

/// `posix_sigemptyset(set)`: clears a signal set.
///
/// A FreeBSD `sigset_t` is four 32-bit words, sixteen bytes, and all of it is cleared.
fn sigemptyset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    for word in 0..SIGSET_WORDS {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if !unsafe { guest::write_u64(args[0].saturating_add(word * 8), 0) } {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    OK
}

/// `_sigprocmask(how, set, oldset)`: examines or changes the blocked-signal mask.
///
/// `how` is FreeBSD's: `SIG_BLOCK` 1, `SIG_UNBLOCK` 2, `SIG_SETMASK` 3. A null `set` is a pure
/// read of the current mask, which guests use as a query. The mask is bookkeeping only.
fn sigprocmask(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// FreeBSD's `SIG_BLOCK`, `SIG_UNBLOCK` and `SIG_SETMASK`.
    const BLOCK: u64 = 1;
    const UNBLOCK: u64 = 2;
    const SETMASK: u64 = 3;

    static MASK: Mutex<[u64; SIGSET_WORDS as usize]> = Mutex::new([0; SIGSET_WORDS as usize]);
    let Ok(mut mask) = MASK.lock() else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };

    // The old mask first: a call that changes and reports must report what it replaced.
    if args[2] != 0 {
        for (index, word) in mask.iter().enumerate() {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            if !unsafe { guest::write_u64(args[2].saturating_add(index as u64 * 8), *word) } {
                return u64::from(GuestError::InvalidArgument.as_raw());
            }
        }
    }
    if args[1] == 0 {
        // A null set is a query, and `how` is not consulted.
        return OK;
    }
    let mut incoming = [0_u64; SIGSET_WORDS as usize];
    for (index, word) in incoming.iter_mut().enumerate() {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let Some(read) = (unsafe { guest::read_u64(args[1].saturating_add(index as u64 * 8)) })
        else {
            return u64::from(GuestError::InvalidArgument.as_raw());
        };
        *word = read;
    }
    match args[0] {
        BLOCK => {
            for (current, add) in mask.iter_mut().zip(incoming) {
                *current |= add;
            }
        }
        UNBLOCK => {
            for (current, remove) in mask.iter_mut().zip(incoming) {
                *current &= !remove;
            }
        }
        SETMASK => *mask = incoming,
        _ => return u64::from(GuestError::InvalidArgument.as_raw()),
    }
    OK
}

/// `sceKernelUuidCreate(out)`: writes a 128-bit identifier.
///
/// A counter rather than random, so two runs compare: unique within a run, repeated across runs.
/// The version and variant bits make it a well-formed version-4 UUID.
fn kernel_uuid_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&count.to_be_bytes());
    bytes[8..].copy_from_slice(&count.wrapping_mul(0x9e37_79b9_7f4a_7c15).to_be_bytes());
    // Version 4 in the high nibble of byte 6, and the RFC 4122 variant in the top two bits of byte 8.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_bytes(args[0], &bytes) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// 64-bit words in a `sigset_t`, which is `__uint32_t __bits[4]` on a FreeBSD-derived system.
const SIGSET_WORDS: u64 = 2;

/// Which word of a set a signal number lives in, and its bit.
///
/// Signals are numbered from one, so signal n is bit n-1. `None` for anything outside the set, so
/// an out-of-range signal is an error rather than a write past the guest's object.
fn signal_bit(signal: u64) -> Option<(u64, u64)> {
    /// Signals a `sigset_t` can hold: four 32-bit words.
    const MAX_SIGNAL: u64 = 128;

    if signal == 0 || signal > MAX_SIGNAL {
        return None;
    }
    let index = signal - 1;
    Some((index / 64, 1_u64 << (index % 64)))
}

/// `posix_sigfillset(set)`: every signal present.
fn sigfillset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    for word in 0..SIGSET_WORDS {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if !unsafe { guest::write_u64(args[0].saturating_add(word * 8), u64::MAX) } {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    OK
}

/// `posix_sigaddset(set, signal)`.
fn sigaddset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Some((word, bit)), true) = (signal_bit(args[1]), args[0] != 0) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let at = args[0].saturating_add(word * 8);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(current) = (unsafe { guest::read_u64(at) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(at, current | bit) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `posix_sigdelset(set, signal)`.
fn sigdelset(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Some((word, bit)), true) = (signal_bit(args[1]), args[0] != 0) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let at = args[0].saturating_add(word * 8);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(current) = (unsafe { guest::read_u64(at) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(at, current & !bit) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `posix_sigismember(set, signal)`: one when present, zero when not.
///
/// Implemented because the placeholder is non-zero and a caller reads it as yes.
fn sigismember(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (Some((word, bit)), true) = (signal_bit(args[1]), args[0] != 0) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(current) = (unsafe { guest::read_u64(args[0].saturating_add(word * 8)) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    u64::from(current & bit != 0)
}

/// Resolves the condition variable a guest pointer refers to.
fn cond_at(pointer: u64) -> Option<sync::CondHandle> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = unsafe { guest::read_u64(pointer) }?;
    (handle != 0).then_some(handle)
}

/// `scePthreadCondInit(cond, attr, name)`.
fn pthread_cond_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_cond(&unsafe { read_name(args[2]) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadCondWait(cond, mutex)`.
///
/// Releases the guest mutex around the wait and retakes it after. Not atomic here, because the
/// two objects are independent: a signal arriving in the gap is lost where on the platform it
/// would not be.
fn pthread_cond_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(cond) = cond_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let mutex = mutex_at(args[1]);
    let by = thread::adopt("main");
    if let Some(handle) = mutex {
        sync::unlock(handle, by);
    }
    let woken = sync::cond_wait(cond, None);
    if let Some(handle) = mutex {
        sync::acquire(handle, by, sync::Blocking::Forever);
    }
    match woken {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadCondSignal(cond)`.
fn pthread_cond_signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_signal) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadCondBroadcast(cond)`.
fn pthread_cond_broadcast(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_broadcast) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadCondDestroy(cond)`.
fn pthread_cond_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = cond_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::cond_destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], 0) };
    OK
}

/// Resolves the read/write lock a guest pointer refers to.
fn rwlock_at(pointer: u64) -> Option<sync::RwlockHandle> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = unsafe { guest::read_u64(pointer) }?;
    (handle != 0).then_some(handle)
}

/// `scePthreadRwlockInit(lock, attr, name)`.
fn pthread_rwlock_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    rwlock_init(args[0], args[2])
}

/// `pthread_rwlock_init(lock, attr)`: the POSIX spelling, which takes no name.
///
/// A separate entry point because the two spellings differ in arity and an implementation cannot
/// see its own: the vendor entry would read `args[2]` from whatever the guest left in `rdx`.
fn posix_pthread_rwlock_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    rwlock_init(args[0], 0)
}

/// What both spellings do, once the name has been resolved by whoever had one.
fn rwlock_init(lock: u64, name: u64) -> u64 {
    if lock == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_rwlock(&unsafe { read_name(name) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(lock, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Answers a lock acquisition.
///
/// A refusal maps to `Busy` rather than an argument error: a lock somebody else holds is the
/// ordinary outcome of a try.
fn acquired(outcome: Option<bool>) -> u64 {
    match outcome {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadRwlockRdlock(lock)`.
fn pthread_rwlock_rdlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, sync::Blocking::Forever)))
}

/// `scePthreadRwlockTryrdlock(lock)`.
fn pthread_rwlock_tryrdlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, sync::Blocking::Never)))
}

/// `scePthreadRwlockWrlock(lock)`.
fn pthread_rwlock_wrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, sync::Blocking::Forever)))
}

/// `scePthreadRwlockTrywrlock(lock)`.
fn pthread_rwlock_trywrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, sync::Blocking::Never)))
}

/// `scePthreadRwlockUnlock(lock)`.
///
/// Not [`acquired`]: here `Some(false)` can only mean the caller released a lock nobody held, so
/// `Busy` would send the guest into a retry loop over its own bug. Answered as
/// `scePthreadMutexUnlock` answers the same mistake.
fn pthread_rwlock_unlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match rwlock_at(args[0]).and_then(sync::rwlock_unlock) {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::InvalidArgument.as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadRwlockDestroy(lock)`.
fn pthread_rwlock_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = rwlock_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::rwlock_destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], 0) };
    OK
}

/// `scePthreadBarrierInit(barrier, attr, count, name)`.
fn pthread_barrier_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    barrier_init(args[0], args[2], args[3])
}

/// `pthread_barrier_init(barrier, attr, count)`: the POSIX spelling, which takes no name.
///
/// A separate entry point for the reason [`posix_pthread_rwlock_init`] is: the vendor entry would
/// read a name from `args[3]`, whatever the guest left in `rcx`.
fn posix_pthread_barrier_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    barrier_init(args[0], args[2], 0)
}

/// What both spellings do, once the name has been resolved by whoever had one.
fn barrier_init(barrier: u64, count: u64, name: u64) -> u64 {
    if barrier == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let needed = u32::try_from(count).unwrap_or(1);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_barrier(needed, &unsafe { read_name(name) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(barrier, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadBarrierWait(barrier)`.
fn pthread_barrier_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(handle) = unsafe { guest::read_u64(args[0]) }.filter(|h| *h != 0) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::barrier_wait(handle) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadBarrierDestroy(barrier)`.
fn pthread_barrier_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(handle) = unsafe { guest::read_u64(args[0]) }.filter(|h| *h != 0) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::barrier_destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], 0) };
    OK
}

/// `sceKernelCreateEventFlag(out, name, attr, initial, param)`.
fn kernel_create_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_event_flag(args[3], &unsafe { read_name(args[1]) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelClockGettime(clock_id, timespec)`: the vendor-named form of `clock_gettime`.
///
/// The clock families, identifiers and monotonic origin come from `orbistoun_hle::clocks`, which
/// POSIX `clock_gettime` also reads, so one clock never answers two elapsed times (D536). Failure
/// differs (D525): POSIX answers `-1`, a `sceKernel*` call a `0x8002_00xx` vendor code. A clock
/// orbistoun has no source for (per-process CPU time, `CLOCK_UPTIME`) is refused with `EINVAL`;
/// the hardware's errno for it is unmeasured.
fn kernel_clock_gettime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some((seconds, nanos)) = orbistoun_hle::clocks::reading(args[0] as i64) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], seconds) } {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    // The second field of the structure, eight bytes on.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[1].saturating_add(8), nanos) } {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    OK
}

/// `sceKernelCreateEqueue(out, name)`.
///
/// `out` is writable and `name` is the guest's own label for the queue; the third register holds
/// a leftover, so the arity is two. As for [`kernel_create_event_flag`], a null out-parameter is
/// refused, the handle is written through it, and `OK` is answered (D524).
fn kernel_create_equeue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = sync::create_equeue(&unsafe { read_name(args[1]) });
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelAddUserEventEdge(equeue, identifier)`.
///
/// Registers a user event against a queue. A bad handle is refused with the vendor `ESRCH`
/// (`0x80020003`) the event-flag family answers on the hardware, rather than success for a queue
/// that does not exist (D524). Delivery is in [`kernel_wait_equeue`].
fn kernel_add_user_event_edge(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if sync::register_event(args[0], args[1]) {
        OK
    } else {
        u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw())
    }
}

/// `scePthreadGetschedparam(thread, policy, param)`.
///
/// Both out-parameters are written four bytes wide: guests pass them four bytes apart on the
/// stack, since a `sched_param` is a single `int`, so an eight-byte write to either clobbers the
/// other (D272). Hands back what was set; a policy nobody set reads zero, meaning "nobody said",
/// since the vendor's default policy is not known.
fn pthread_getschedparam(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(record) = thread::record(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if args[1] == 0 || args[2] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u32(args[1], record.requested_policy as u32) }
        // SAFETY: an address the guest passed for this call, valid by its contract.
        || !unsafe { guest::write_u32(args[2], record.requested_priority as u32) }
    {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadSetschedparam(thread, policy, param)`.
///
/// Stores both, so [`pthread_getschedparam`] hands back what was set. Neither is applied: the host
/// scheduler decides which thread runs (D523). The policy is stored verbatim; guests pass vendor
/// values that are not POSIX constants.
fn pthread_setschedparam(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !thread::is_issued(args[0]) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(priority) = (unsafe { guest::read_u32(args[2]) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let policy = i32::from_ne_bytes((args[1] as u32).to_ne_bytes());
    if thread::set_scheduling(
        args[0],
        Some(policy),
        i32::from_ne_bytes(priority.to_ne_bytes()),
    ) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `scePthreadSetprio(thread, priority)`.
///
/// The priority alone, so the policy is left as it was; [`thread::set_scheduling`] takes an
/// optional one for this.
fn pthread_setprio(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !thread::is_issued(args[0]) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    let priority = i32::from_ne_bytes((args[1] as u32).to_ne_bytes());
    if thread::set_scheduling(args[0], None, priority) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `scePthreadRename(thread, name)`.
///
/// The name is what a trace shows in place of a handle.
fn pthread_rename(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !thread::is_issued(args[0]) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    if args[1] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if thread::rename(args[0], &unsafe { read_name(args[1]) }) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `pthread_setcancelstate(state, oldstate)`.
///
/// Stored and not acted on: orbistoun cancels no threads, and what a caller is promised is the
/// previous value. The out-parameter is optional, as POSIX allows, and four bytes, being an
/// `int` (D272).
fn posix_pthread_setcancelstate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let state = i32::from_ne_bytes((args[0] as u32).to_ne_bytes());
    let Some(previous) = thread::swap_cancel_state(thread::current(), state) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] != 0 && !unsafe { guest::write_u32(args[1], previous as u32) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// FreeBSD `ETIMEDOUT`, under the measured `0x8002_0000` vendor mapping.
///
/// Shared by the event queue and the event flag, the two calls that can time out. The value is
/// the published errno; neither call has been captured timing out on the hardware.
const ETIMEDOUT: u32 = 60;
/// `sceKernelWaitEqueue(equeue, events, wanted, delivered, timeout)`.
///
/// Arity five, with the guest's own argument roles: a queue handle, an event buffer, the number
/// wanted, an out-count, and a pointer to a microsecond timeout (null in observed calls). The
/// delivered structure is `struct kevent`; see [`sync::PendingEvent`].
///
/// A bad handle is refused with the vendor `ESRCH` (`0x80020003`) the rest of this family
/// answers on the hardware.
fn kernel_wait_equeue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !sync::equeue_exists(args[0]) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    let Ok(wanted) = usize::try_from(args[2]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if args[1] == 0 || wanted == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // `arg4` points to a microsecond timeout, and null means wait indefinitely, as for `kevent(2)`,
    // so a guest looping until an event arrives blocks rather than spins.
    let until = if args[4] == 0 {
        sync::Blocking::Forever
    } else {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        match unsafe { guest::read_u64(args[4]) } {
            Some(micros) => sync::Blocking::Until(
                std::time::Instant::now() + std::time::Duration::from_micros(micros),
            ),
            // A timeout pointer that does not read is not a zero timeout.
            None => return u64::from(GuestError::InvalidArgument.as_raw()),
        }
    };
    let Some(events) = sync::wait_events(args[0], wanted, until) else {
        // Nothing arrived within the caller's patience: a timeout, not success with nothing written.
        return u64::from(GuestError::vendor(ETIMEDOUT).as_raw());
    };
    for (index, event) in events.iter().enumerate() {
        let at = args[1].saturating_add((index * sync::EVENT_BYTES) as u64);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if !unsafe { guest::write_bytes(at, &event.to_bytes()) } {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    // The count is written through `arg3` and the return is a status. Zero delivered is written as
    // zero rather than skipped, so a caller never reads a stale count.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[3] != 0 && !unsafe { guest::write_bytes(args[3], &(events.len() as u32).to_le_bytes()) }
    {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Whether a wait mode asks for every bit of the pattern.
///
/// `Some(true)` for `and`, `Some(false)` for `or`, `None` for a mode naming neither, which the
/// hardware answers with `0x80020016`. The higher bits are the clear-on-match behaviour and are not
/// read here.
const fn wait_mode(mode: u64) -> Option<bool> {
    /// Every bit of the pattern must be present.
    const WAIT_AND: u64 = 0x01;
    /// Any bit of the pattern will do.
    const WAIT_OR: u64 = 0x02;

    match mode & (WAIT_AND | WAIT_OR) {
        WAIT_AND => Some(true),
        WAIT_OR => Some(false),
        // Zero names neither. `0x03` names both, which is unmeasured, so it is refused.
        _ => None,
    }
}
/// `sceKernelPollEventFlag(flag, pattern, mode, result, timeout)`.
///
/// A miss is `Busy`, not an error, so a guest does not read an ordinary poll as a broken handle.
fn kernel_poll_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // The handle first, then the mode. On the hardware a poll on a handle never issued answers
    // `0x80020003` even when the mode is also invalid, and a bad mode on a real handle answers
    // `0x80020016`; a mode naming neither `and` nor `or` (such as `0x00`) is that argument error.
    if !sync::event_flag_exists(args[0]) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    let Some(all) = wait_mode(args[2]) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let Some(outcome) = sync::event_flag_poll(args[0], args[1], all) else {
        // ESRCH (`0x80020003`), as the hardware answers.
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    };
    match outcome {
        Some(bits) => {
            if args[3] != 0 {
                // SAFETY: an address the guest passed for this call, valid by its contract.
                unsafe { guest::write_u64(args[3], bits) };
            }
            OK
        }
        None => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
    }
}

/// `sceKernelWaitEventFlag(flag, pattern, mode, result, timeout)`.
///
/// Blocks the calling thread until the pattern is set - `mode` bit `0x01` requires every bit of it
/// (AND), its absence any bit (OR) - clearing all the flag's bits (`0x10`) or just the matched
/// pattern (`0x20`) on success, then answers `OK` and writes the pattern found through `result`. A
/// NULL `timeout` waits indefinitely; otherwise it points at a microsecond count, and a wait that
/// outlives it answers the vendor `ETIMEDOUT`. The blocking counterpart of
/// [`kernel_poll_event_flag`].
fn kernel_wait_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Mode bit: clear every bit of the flag on a successful wait.
    const WAIT_CLEAR_ALL: u64 = 0x10;
    /// Mode bit: clear just the matched pattern on a successful wait.
    const WAIT_CLEAR_PAT: u64 = 0x20;
    let (flag, pattern, mode, result, timeout_ptr) = (args[0], args[1], args[2], args[3], args[4]);
    // NULL waits forever; otherwise the pointer holds a microsecond count.
    let timeout = if timeout_ptr == 0 {
        None
    } else {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::read_u64(timeout_ptr) }.map(std::time::Duration::from_micros)
    };
    // The same refusals as the polling twin, in the same order: the two differ only in whether they
    // wait.
    if !sync::event_flag_exists(flag) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    let Some(all) = wait_mode(mode) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let Some(outcome) = sync::event_flag_wait(
        flag,
        pattern,
        all,
        mode & WAIT_CLEAR_ALL != 0,
        mode & WAIT_CLEAR_PAT != 0,
        timeout,
    ) else {
        // ESRCH for a bad handle, as the rest of the event-flag family answers.
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    };
    match outcome {
        Some(bits) => {
            if result != 0 {
                // SAFETY: an address the guest passed for this call, valid by its contract.
                unsafe { guest::write_u64(result, bits) };
            }
            OK
        }
        None => u64::from(GuestError::vendor(ETIMEDOUT).as_raw()),
    }
}

/// `sceKernelSetEventFlag(flag, pattern)`.
fn kernel_set_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sync::event_flag_set(args[0], args[1]) {
        Some(true) => OK,
        // ESRCH for a bad handle, as the rest of the event-flag family answers.
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw()),
    }
}

/// `sceKernelClearEventFlag(flag, pattern)`.
fn kernel_clear_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sync::event_flag_clear(args[0], args[1]) {
        Some(true) => OK,
        // ESRCH for a bad handle, as the rest of the event-flag family answers.
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw()),
    }
}

/// `sceKernelDeleteEventFlag(flag)`.
fn kernel_delete_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if sync::event_flag_destroy(args[0]) {
        OK
    } else {
        // ESRCH for a bad handle, as the rest of the event-flag family answers.
        u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw())
    }
}

/// Reads a semaphore handle, which is a 32-bit identifier rather than a pointer.
fn sema_at(raw: u64) -> Option<sync::SemaphoreHandle> {
    i32::try_from(raw as i64)
        .ok()
        .filter(|h| *h != sync::NO_SEMAPHORE)
}

/// `sceKernelPollSema(semaphore, need)`: takes `need` without waiting, or none at all.
///
/// On the hardware: asking for 0 answers `0x80020016` (invalid); 2 with 1 available answers
/// `0x80020010` (busy) and takes nothing; 1 of 1 and 2 of 3 succeed. Taking part of a request
/// would leave a caller releasing more than it holds.
fn kernel_poll_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // The handle first, consistent with the order the hardware shows for event flags.
    let Some(handle) = sema_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    // Zero is an argument error on the hardware, not a trivially satisfied request.
    let Some(need) = u32::try_from(args[1]).ok().filter(|n| *n > 0) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    match sync::semaphore_wait(handle, need, sync::Blocking::Never) {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sceKernelSignalSema(semaphore, count)`.
fn kernel_signal_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let count = u32::try_from(args[1]).unwrap_or(1);
    match sema_at(args[0]).and_then(|h| sync::semaphore_signal(h, count)) {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::InvalidArgument.as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sceKernelWaitSema(semaphore, need, timeout)`.
///
/// Takes the same `need` as its polling twin; the two differ only in whether they wait.
fn kernel_wait_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = sema_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let Some(need) = u32::try_from(args[1]).ok().filter(|n| *n > 0) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    match sync::semaphore_wait(handle, need, sync::Blocking::Forever) {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sceKernelDeleteSema(semaphore)`.
fn kernel_delete_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if sema_at(args[0]).is_some_and(sync::semaphore_destroy) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `sceKernelSyncOnAddressWait(address, value)`: sleeps while the 64-bit word at `address` holds
/// `value`, until a wake on the same address.
///
/// The platform's futex wait, modelled on FreeBSD `_umtx_op(2)` with `UMTX_OP_WAIT` (D573):
/// compare, sleep only on equality, and answer success at once when the word already differs;
/// the caller re-reads the word. The compare happens under the wait queue's lock
/// ([`sync::wait_on_address`]), so a wake cannot be lost. Sixty-four bits, because this entry
/// point is shared with the `Wait64` spelling while `Wait32` has its own
/// (`orbistoun-firmware/data/libkernel-vaddrs.txt`).
///
/// A non-zero third register is refused, not waited on: observed calls pass zero, and it may be a
/// timeout in an unknown unit.
fn sync_on_address_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, expected, third) = (args[0], args[1], args[2]);
    if address == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if third != 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // Reachable while it sleeps here: a signal raised on a thread parked in this wait runs its
    // handler and the thread sleeps again; raised anywhere else it is refused (D652).
    let was = thread::set_parked(true);
    let outcome = sync::wait_on_address(
        address,
        expected,
        // SAFETY: an address the guest passed for this call, valid by its contract.
        || unsafe { guest::read_u64(address) },
        sync::Blocking::Forever,
    );
    thread::set_parked(was);
    match outcome {
        Some(sync::AddressWait::Woken | sync::AddressWait::Mismatch) => OK,
        // Unreachable under `Forever`; kept so the match stays total.
        Some(sync::AddressWait::TimedOut) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw())
        }
        None => u64::from(GuestError::InvalidArgument.as_raw()),
    }
}

/// `sceKernelSyncOnAddressWake(address, count)`: wakes up to `count` threads asleep on `address`.
///
/// FreeBSD `_umtx_op(2)` with `UMTX_OP_WAKE`. Success whether or not anybody was asleep: a wake
/// racing its wait is the ordinary case, and a wake with nobody listening is not remembered
/// ([`sync::wake_on_address`]).
fn sync_on_address_wake(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, count) = (args[0], args[1]);
    if address == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    if sync::wake_on_address(address, count).is_some() {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// The POSIX unnamed-semaphore family: `sem_init` and the calls built on it.
///
/// POSIX `sem_init` initialises the guest's `sem_t` in place, where `sceKernelCreateSema` writes a
/// handle to an out-pointer, so the handle is stored in the `sem_t`, as for mutexes and condition
/// variables, and read back by the rest. Guest runtimes assert that `sem_init` returns zero.
///
/// Reference: POSIX.1-2008 `sem_init`/`sem_wait`/`sem_trywait`/`sem_post`/`sem_destroy`; the
/// host primitives are this crate's `sync` semaphores, shared with the vendor calls.
fn posix_sema_at(sem: u64) -> Option<sync::SemaphoreHandle> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    sema_at(unsafe { guest::read_u64(sem) }?)
}

/// `sem_init(sem, pshared, value)`: a POSIX unnamed semaphore, initialised in place.
fn sem_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (sem, value) = (args[0], args[2]);
    if sem == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let initial = u32::try_from(value).unwrap_or(0);
    // POSIX bounds a semaphore only by `SEM_VALUE_MAX`; the largest ceiling stands in for
    // "effectively unbounded", so a `sem_post` is never refused for a ceiling the guest never set.
    let handle = sync::create_semaphore(initial, u32::MAX, "");
    // Stored as a word and read back by `posix_sema_at`; a handle is a small positive id, so it
    // round-trips through `sema_at`'s `i32` unchanged.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(sem, u64::try_from(handle).unwrap_or(0)) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sem_wait(sem)`: takes one, waiting for it.
fn sem_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, sync::Blocking::Forever)) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_trywait(sem)`: takes one only if it is available.
fn sem_trywait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, sync::Blocking::Never)) {
        Some(true) => OK,
        // Empty and bad-handle are both non-zero, which is what a caller testing against zero needs;
        // POSIX distinguishes them by `errno`, which is not invented here.
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
    }
}

/// `sem_post(sem)`: gives one back.
fn sem_post(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).map(|h| sync::semaphore_signal(h, 1)) {
        Some(Some(true)) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_destroy(sem)`: retires the semaphore a `sem_t` names.
fn sem_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if posix_sema_at(args[0]).is_some_and(sync::semaphore_destroy) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// Where a mutex attribute object keeps its type.
///
/// This crate defines the layout, which is sound only because nothing else reads it: the guest
/// allocates the object and hands it to these calls and to `scePthreadMutexInit`, all here
/// (D272).
const ATTR_TYPE: u64 = 0;
/// Offset of the protocol, one word after the type.
const ATTR_PROTOCOL: u64 = 8;

/// Whether a mutex or condition variable is shared between processes.
///
/// Stored and returned, not honoured: a getter answers what its setter wrote (D272), and with one
/// process here, process-shared has nothing to mean.
const MUTEXATTR_PSHARED: u64 = 16;

/// A mutex's priority ceiling, stored on the same terms as [`MUTEXATTR_PSHARED`].
const MUTEXATTR_PRIOCEILING: u64 = 24;

/// Which clock a condition variable's timed waits are measured against.
const CONDATTR_CLOCK: u64 = 16;

/// Whether a condition variable is shared between processes, on the same terms.
const CONDATTR_PSHARED: u64 = 24;

/// `scePthreadCondattrInit(attr)`: allocates a condition-variable attribute object.
///
/// Pointer-to-pointer, like every attribute in this family (D272): this writes the address of a
/// fresh block. Its zeroes read as the default clock and process-private, which is what a freshly
/// initialised condattr holds.
fn pthread_condattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // From the one region every guest-visible handle comes from, so it repeats (D584).
    let handle = orbistoun_mem::blocks::block(4);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrInit(attr)`: allocates an attribute object and hands back its address.
///
/// The argument is a pointer to a pointer: the guest declares `ScePthreadMutexattr attr = NULL`
/// and passes `&attr` (D272).
fn pthread_mutexattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // From the one region every guest-visible handle comes from, so it repeats (D584).
    let handle = orbistoun_mem::blocks::block(4);
    // The hardware reads type 1 back from a freshly initialised attribute (D398).
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(handle + ATTR_TYPE, DEFAULT_MUTEX_TYPE) };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// The type a freshly initialised mutex attribute carries.
///
/// The hardware answers `scePthreadMutexattrGettype` with 1 on an attribute nothing set, and the
/// types behave differently, so the value has consequences (D398).
const DEFAULT_MUTEX_TYPE: u64 = 1;

/// Resolves the attribute object a guest pointer refers to.
fn attr_at(pointer: u64) -> Option<u64> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = unsafe { guest::read_u64(pointer) }?;
    (handle != 0).then_some(handle)
}

/// `scePthreadMutexattrSettype(attr, type)`: stores the type, so `Gettype` reads it back (D272).
fn pthread_mutexattr_settype(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Type zero is refused: on the hardware types 1 to 4 round-trip through `Settype` and `Gettype`
    // and 0 does not. The hardware's return code for it is unmeasured, so this answers orbistoun's
    // own placeholder rather than an invented vendor code.
    if args[1] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(object + ATTR_TYPE, args[1]) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrGettype(attr, out)`.
fn pthread_mutexattr_gettype(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(value) = (unsafe { guest::read_u64(object + ATTR_TYPE) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u32(args[1], value as u32) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrSetprotocol(attr, protocol)`.
fn pthread_mutexattr_setprotocol(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(object + ATTR_PROTOCOL, args[1]) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrGetprotocol(attr, out)`.
fn pthread_mutexattr_getprotocol(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(value) = (unsafe { guest::read_u64(object + ATTR_PROTOCOL) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u32(args[1], value as u32) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelAllocateDirectMemory(searchStart, searchEnd, len, alignment, type, out)`.
///
/// The same pool as [`allocate_main_direct_memory`], with a search range. The pool allocates
/// upward from `searchStart`; an allocation ending past `searchEnd` is refused as out of memory.
fn allocate_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (search_start, search_end, len, alignment, memory_type, out) =
        (args[0], args[1], args[2], args[3], args[4], args[5]);

    if len == 0 || out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if alignment != 0 && !alignment.is_power_of_two() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(memory_type) = u32::try_from(memory_type) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Ok(mut guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let align = alignment.max(direct::DIRECT_ALIGN);
    let start = search_start.next_multiple_of(align);
    let Some(address) = guard.allocate(start, len, memory_type) else {
        return u64::from(GuestError::NoMemory.as_raw());
    };
    // Refused after the fact: the pool decides where a request fits, and a range it cannot satisfy
    // is a real out-of-memory for that range.
    if search_end != 0 && address.saturating_add(len) > search_end {
        guard.release(address, len);
        return u64::from(GuestError::NoMemory.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, address) } {
        guard.release(address, len);
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelReleaseDirectMemory(start, len)`: returns a span to the pool.
fn release_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (start, len) = (args[0], args[1]);
    if len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(mut guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if guard.release(start, len) {
        drop(guard);
        forget_physical(start, len);
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// `sceKernelMunmap(address, len)`: unmaps a span from the guest's address space.
///
/// A null address is refused, so a guest is not told memory was released when it was not.
fn munmap(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, len) = (args[0], args[1]);
    if address == 0 || len == 0 {
        // The vendor `EINVAL` (`0x80020016`) the hardware answers for `sceKernelMunmap(0)`.
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    // The host reservation stays, since releasing a piece would hole an address space the guest
    // believes contiguous. The alias goes, so a later map of the same physical memory is new.
    forget_mapped(address, len);
    OK
}

/// `mmap(addr, len, prot, flags, fd, offset)`: maps pages of memory into guest address space.
///
/// Reference: POSIX.1-2008 `mmap(2)`, FreeBSD `SYS_mmap` (477).
pub fn mmap(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr, len, prot) = (args[0], args[1], args[2]);
    if len == 0 {
        return !0; // MAP_FAILED
    }
    let base = if addr == 0 {
        next_mapping_base(len)
    } else {
        addr
    };
    let align = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    let (Some(base), Some(len)) = (
        checked_next_multiple_of(base, align),
        checked_next_multiple_of(len, orbistoun_core::GUEST_PAGE_SIZE),
    ) else {
        return !0;
    };
    let protection = protection_from_guest(prot);
    let Ok(mut space) = mappings().lock() else {
        return !0;
    };
    if space.reserve(base, len, protection).is_err() {
        return !0;
    }
    drop(space);
    mapping_placed(base, len, protection, addr != 0);
    note_requested_protection(base, len, prot);
    base
}

/// `sceKernelReserveVirtualRange(addr, len, flags, alignment)`: reserves a span of address space.
///
/// `addr` is a `void **`: the value it points at going in is a hint (zero means "anywhere"), and
/// the base reserved is written back through it, where the guest reads it to map memory into the
/// range. The range is reserved and backed in one step, since orbistoun hands out backed memory.
fn reserve_virtual_range(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr_out, len, alignment) = (args[0], args[1], args[3]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if addr_out == 0 || len == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    // The guest's hint, read through the `void **`.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let hint = unsafe { guest::read_u64(addr_out) }.unwrap_or(0);
    let align = alignment
        .max(orbistoun_mem::allocation_granularity())
        .max(orbistoun_core::GUEST_PAGE_SIZE);
    let Some(len) = checked_next_multiple_of(len, orbistoun_core::GUEST_PAGE_SIZE) else {
        return vendor(orbistoun_core::errno::INVALID);
    };
    let protection = orbistoun_mem::Protection::READ_WRITE;
    let Ok(mut space) = mappings().lock() else {
        return vendor(orbistoun_core::errno::INVALID);
    };
    // Honour the hint, fall back on conflict (D443). A guest asks for a specific address because its
    // own allocator addresses the range from there, so a different base strands it. Only if the hint
    // is unavailable does the arena counter supply one, retried past a conflict.
    let mut reserved = None;
    let first = if hint == 0 {
        next_mapping_base(len)
    } else {
        hint
    };
    if let Some(base) = checked_next_multiple_of(first, align) {
        if space.reserve(base, len, protection).is_ok() {
            reserved = Some(base);
        }
    }
    for _ in 0..CONFLICT_RETRIES {
        if reserved.is_some() {
            break;
        }
        if let Some(base) = checked_next_multiple_of(next_mapping_base(len), align) {
            if space.reserve(base, len, protection).is_ok() {
                reserved = Some(base);
            }
        }
    }
    drop(space);
    let Some(base) = reserved else {
        return vendor(orbistoun_core::errno::DENIED);
    };
    mapping_placed(
        base,
        len,
        protection,
        hint != 0 && Some(base) == checked_next_multiple_of(hint, align),
    );
    if let Ok(mut reserved) = reservations().lock() {
        reserved.push((base, len));
    }
    // The reserved base, written back through the `void **`; the status is returned separately.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_u64(addr_out, base) } {
        OK
    } else {
        vendor(orbistoun_core::errno::INVALID)
    }
}

/// `sceKernelVirtualQuery(addr, flags, info, info_size)`: describes the mapping that holds `addr`.
///
/// A guest walks its address space with this before deciding where to place something. The
/// region containing `addr` is found from every place one is recorded (see [`query_region`]) and
/// encoded as `SceKernelVirtualQueryInfo`. An address in no region is answered with the code the
/// hardware answers, not a fabricated mapping.
fn virtual_query(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr, info, size) = (args[0], args[2], args[3]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if info == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    // An address in no region is `EACCES` (`0x8002000d`), not `ENOENT`, as measured by obSCEne
    // `020-memory/virtual-query-unmapped`.
    let Some(region) = query_region(addr) else {
        return vendor(orbistoun_core::errno::DENIED);
    };
    // The whole structure, zeros included: on the hardware every byte of a pre-filled 72-byte
    // buffer changes.
    let encoded = region.encode();
    let len = usize::try_from(size).map_or(encoded.len(), |s| s.min(encoded.len()));
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_bytes(info, &encoded[..len]) } {
        OK
    } else {
        vendor(orbistoun_core::errno::INVALID)
    }
}

/// How many bytes `SceKernelVirtualQueryInfo` is, as measured by obSCEne
/// `020-memory/virtual-query-{mapped,text,stack}`.
const VIRTUAL_QUERY_INFO_BYTES: usize = 72;

/// Where each field of `SceKernelVirtualQueryInfo` sits, from the hardware's own bytes.
///
/// From obSCEne's `020-memory/virtual-query-{mapped,text,stack}`: start and end at 0 and 8, a
/// direct mapping's physical offset at 0x10 (0 for code and stack), the protection as a 32-bit
/// value at 0x18 (3 for read-write, 4 for execute-only text), a 32-bit memory type at 0x1c, a flag
/// byte at 0x20, and a NUL-terminated name from 0x21.
mod vq {
    pub(super) const START: usize = 0x00;
    pub(super) const END: usize = 0x08;
    pub(super) const OFFSET: usize = 0x10;
    pub(super) const PROTECTION: usize = 0x18;
    pub(super) const FLAGS: usize = 0x20;
    pub(super) const NAME: usize = 0x21;
    /// The name's room: from 0x21 to the end of the structure, keeping its terminator.
    pub(super) const NAME_BYTES: usize = super::VIRTUAL_QUERY_INFO_BYTES - NAME;

    /// The flag bits, read off three measured regions: text `0x11`, a direct mapping `0x12`, the main
    /// stack `0x15`. Committed is set on all three, direct only on the direct mapping, stack only on
    /// the stack, and flexible on the two that are not direct.
    pub(super) const FLEXIBLE: u8 = 0x01;
    pub(super) const DIRECT: u8 = 0x02;
    pub(super) const STACK: u8 = 0x04;
    pub(super) const COMMITTED: u8 = 0x10;
}

/// What `sceKernelVirtualQuery` reports about one region.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryRegion {
    start: u64,
    end: u64,
    /// A direct mapping's physical offset; zero otherwise, as measured for code and stack.
    offset: u64,
    /// CPU read 1, write 2, execute 4, as the hardware reports them.
    protection: u32,
    flags: u8,
    /// The hardware's name for the region, where one was measured; empty otherwise.
    name: &'static str,
}

impl QueryRegion {
    /// The 72 bytes the hardware writes, zero wherever this region has nothing to say.
    fn encode(&self) -> [u8; VIRTUAL_QUERY_INFO_BYTES] {
        let mut out = [0_u8; VIRTUAL_QUERY_INFO_BYTES];
        out[vq::START..vq::START + 8].copy_from_slice(&self.start.to_le_bytes());
        out[vq::END..vq::END + 8].copy_from_slice(&self.end.to_le_bytes());
        out[vq::OFFSET..vq::OFFSET + 8].copy_from_slice(&self.offset.to_le_bytes());
        out[vq::PROTECTION..vq::PROTECTION + 4].copy_from_slice(&self.protection.to_le_bytes());
        out[vq::FLAGS] = self.flags;
        let name = self.name.as_bytes();
        let n = name.len().min(vq::NAME_BYTES - 1);
        out[vq::NAME..vq::NAME + n].copy_from_slice(&name[..n]);
        out
    }
}

/// The protection each guest range was last asked for, as `(base, len, prot)`, newest last.
///
/// What the guest asked, not what was granted: `protection_from_guest` widens the grant for the
/// host, and a query answered with the grant would tell the guest a range is set up when it is
/// not, so it would skip the `sceKernelMprotect` that sets it up.
fn requested_protections() -> &'static Mutex<Vec<(u64, u64, u64)>> {
    static REQUESTED: OnceLock<Mutex<Vec<(u64, u64, u64)>>> = OnceLock::new();
    REQUESTED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records the protection a guest asked for over `[base, base + len)`.
fn note_requested_protection(base: u64, len: u64, prot: u64) {
    if let Ok(mut requested) = requested_protections().lock() {
        requested
            .retain(|&(b, l, _)| !(b >= base && b.saturating_add(l) <= base.saturating_add(len)));
        requested.push((base, len, prot));
    }
}

/// The protection last asked for over the range holding `addr`.
fn requested_protection(addr: u64) -> Option<u64> {
    let requested = requested_protections().lock().ok()?;
    requested
        .iter()
        .rev()
        .find(|&&(base, len, _)| addr >= base && addr < base.saturating_add(len))
        .map(|&(_, _, prot)| prot)
}

/// The hardware's protection field for a range asked for with `prot`: the request, as asked.
///
/// Measured for the CPU bits (obSCEne `020-memory/virtual-query-{mapped,text}`). Guest-observed for
/// the GPU bits: guests compare this field with the `0xf2` they want and call `sceKernelMprotect`
/// only when they differ.
const fn reported_protection(prot: u64) -> u32 {
    prot as u32
}

/// Whether a region starting at `start` is a reservation `sceKernelReserveVirtualRange` made.
fn is_reservation(start: u64) -> bool {
    reservations()
        .lock()
        .is_ok_and(|reserved| reserved.iter().any(|&(base, _)| base == start))
}

/// The hardware's protection value for a region's access.
const fn query_protection(protection: orbistoun_mem::Protection) -> u32 {
    (protection.read as u32) | ((protection.write as u32) << 1) | ((protection.execute as u32) << 2)
}

/// Describes the region holding `addr`, from every place one is recorded (D446).
///
/// A direct mapping is answered in full, as `020-memory/virtual-query-mapped` measured: offset,
/// protection, direct and committed, and the name `anon`. The main stack is answered as
/// `virtual-query-stack` measured. Other mappings this crate placed are flexible memory, with
/// protection and the flexible and committed bits. Regions the loader noted and thread stacks get
/// their bounds and zeros, since their per-segment protection and names are not measured.
fn query_region(addr: u64) -> Option<QueryRegion> {
    let within = |base: u64, len: u64| addr >= base && addr < base.saturating_add(len);
    if let Ok(space) = mappings().lock() {
        if let Some(region) = space.regions().iter().find(|r| within(r.base, r.len)) {
            let (start, end) = (region.base, region.base.saturating_add(region.len));
            // What the guest asked for, where the hardware's answer to that is measured; 0 otherwise, never
            // orbistoun's own grant.
            let protection = requested_protection(addr).map_or(0, reported_protection);
            let direct = physical_mappings().lock().ok().and_then(|mapped| {
                mapped
                    .iter()
                    .find(|&(_, &(base, len))| within(base, len))
                    .map(|(&physical, &(base, _))| physical + start.saturating_sub(base))
            });
            let flexible = flexible_mappings()
                .lock()
                .is_ok_and(|bases| bases.contains(&start));
            return Some(match direct {
                Some(offset) if !flexible => QueryRegion {
                    start,
                    end,
                    offset,
                    protection,
                    flags: vq::DIRECT | vq::COMMITTED,
                    name: "anon",
                },
                // A reservation nothing has been mapped into: address space only, no access, not committed. The
                // guest fills such a range only when the query says it is empty.
                _ if !flexible && is_reservation(start) => QueryRegion {
                    start,
                    end,
                    offset: 0,
                    protection: 0,
                    flags: 0,
                    name: "",
                },
                _ => QueryRegion {
                    start,
                    end,
                    offset: 0,
                    protection,
                    flags: vq::FLEXIBLE | vq::COMMITTED,
                    name: "",
                },
            });
        }
    }
    if let Some(&(base, len)) = STACK_SPAN.get() {
        if within(base, len) {
            return Some(QueryRegion {
                start: base,
                end: base.saturating_add(len),
                offset: 0,
                protection: query_protection(orbistoun_mem::Protection::READ_WRITE),
                flags: vq::FLEXIBLE | vq::STACK | vq::COMMITTED,
                name: "main stack",
            });
        }
    }
    let (start, end) = region_containing(addr)?;
    Some(QueryRegion {
        start,
        end,
        offset: 0,
        protection: 0,
        flags: 0,
        name: "",
    })
}

/// `sceKernelSetVirtualRangeName(start, len, name)`: attaches a debug name to a virtual range.
///
/// Accepted and answered `OK` with nothing stored: the name exists for host tools and no
/// guest-readable interface returns it. A null name or a zero-length range is refused as
/// malformed.
fn set_virtual_range_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (start, len, name) = (args[0], args[1], args[2]);
    if start == 0 || len == 0 || name == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    OK
}

/// `sceKernelMprotect(addr, len, prot)`: changes the protection of a range already reserved.
///
/// A guest reserves a span, then calls this before handing it to its own allocator, and acts on
/// the answer. The range must lie wholly inside one region this process placed for the guest:
/// either a mapping from this crate's address space, or a region the loader reported, such as a
/// module's pages (D577). A range covered by nothing is refused, so a bad pointer cannot
/// re-protect this process's own code.
///
/// - The range is kept readable: guests pass values such as `0xf2` with the write bit and without
///   the read bit, and a managed range must stay readable for the guest's allocator.
/// - The high bits, which name GPU access and cache behaviour, are ignored rather than decoded
///   (D008).
fn mprotect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr, len, prot) = (args[0], args[1], args[2]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if addr == 0 || len == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    let Some(len) = checked_next_multiple_of(len, orbistoun_core::GUEST_PAGE_SIZE) else {
        return vendor(orbistoun_core::errno::INVALID);
    };
    let mut protection = protection_from_guest(prot);
    protection.read = true;

    // Which authority covers the range is decided first: `AddressSpace::protect` records a
    // reservation failure when handed a range it does not own, and a module's own memory is not a
    // failure. The guard is scoped so the lock is released before `region_covering` takes it again.
    let owned = {
        let Ok(mut space) = mappings().lock() else {
            return vendor(orbistoun_core::errno::INVALID);
        };
        space
            .owns(addr, len)
            .then(|| space.protect(addr, len, protection))
    };
    if let Some(outcome) = owned {
        return match outcome {
            Ok(()) => {
                note_requested_protection(addr, len, prot);
                OK
            }
            // The range is this crate's own and the host refused it.
            Err(_) => vendor(orbistoun_core::errno::INVALID),
        };
    }

    // Not a mapping this crate handed out: for a guest re-protecting its own module that is the
    // ordinary case (D577). A range inside a region placed for the guest is allowed; a range covered
    // by nothing is refused.
    if region_covering(addr, len).is_none() {
        return vendor(orbistoun_core::errno::INVALID);
    }
    match orbistoun_mem::platform::protect(addr, len, protection) {
        Ok(()) => OK,
        // The region is the guest's and the host would not change it; reported as an invalid mapping,
        // since the guest acts on the answer.
        Err(_) => vendor(orbistoun_core::errno::INVALID),
    }
}

/// `sceKernelAvailableFlexibleMemorySize(out)`.
///
/// Flexible memory is the share an application may map without reserving physical pages first, a
/// separate budget from the direct pool (D444). Answered as the launch figure minus what the guest
/// has mapped ([`direct::flexible_available`]).
fn available_flexible_memory_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], direct::flexible_available()) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelConfiguredFlexibleMemorySize(out)`.
///
/// The configured flexible-memory total, the ceiling that does not move as the guest maps
/// ([`direct::flexible_configured`], D444).
fn configured_flexible_memory_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], direct::flexible_configured()) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelReleaseFlexibleMemory(address, len)`: returns a flexible mapping to the budget.
///
/// The counterpart to [`map_flexible_memory`], completing the map, use and release round trip.
fn release_flexible_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, len) = (args[0], args[1]);
    if address == 0 || len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // The span stays mapped, since orbistoun does not hole a guest's contiguous space, but the budget
    // is credited back so a map and release loop does not drain `available` (D444).
    direct::record_flexible_release(len);
    OK
}

/// `sceKernelMapFlexibleMemory(out, len, prot, flags)`.
///
/// Maps pages and hands back their address in one step; the caller never sees a physical
/// address. Drawn against the flexible budget, not the direct pool, and refused when the budget
/// cannot cover it (D444).
fn map_flexible_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, len, prot, flags) = (args[0], args[1], args[2], args[3]);
    if out == 0 || len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if direct::flexible_available() < len {
        return u64::from(GuestError::NoMemory.as_raw());
    }
    let physical = {
        let Ok(mut guard) = direct::map().lock() else {
            return u64::from(GuestError::Unimplemented.as_raw());
        };
        match guard.allocate_aligned(len, direct::DIRECT_ALIGN, 0) {
            Some(address) => address,
            None => return u64::from(GuestError::NoMemory.as_raw()),
        }
    };
    // Mapped through the direct path, so one implementation decides where guest memory appears.
    let mut mapping = [0_u64; GUEST_ARG_REGISTERS];
    mapping[0] = out;
    mapping[1] = len;
    mapping[2] = prot;
    mapping[3] = flags;
    mapping[4] = physical;
    mapping[5] = direct::DIRECT_ALIGN;
    let result = map_named_direct_memory(&mapping);
    // Charge the budget only once the mapping succeeded.
    if result == OK {
        direct::record_flexible_map(len);
        // Mapped through the direct path, and still flexible memory to a query.
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let base = unsafe { guest::read_u64(out) };
        if let (Some(base), Ok(mut flexible)) = (base, flexible_mappings().lock()) {
            flexible.push(base);
        }
    }
    result
}

/// Bases of mappings `sceKernelMapFlexibleMemory` made: placed through the direct path, so its
/// alias table holds them too, and answered to a query as flexible memory rather than direct.
fn flexible_mappings() -> &'static Mutex<Vec<u64>> {
    static FLEXIBLE: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();
    FLEXIBLE.get_or_init(|| Mutex::new(Vec::new()))
}

/// `scePthreadAttrInit(attr)`: allocates a thread attribute object.
///
/// Pointer-to-pointer, like the mutex attributes (D272).
fn pthread_attr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // Sixteen words: this crate owns the layout (see the field constants), with room to spare.
    let handle = orbistoun_mem::blocks::block(16);
    // A fresh attribute set reports a stack size of `0x10000` and a stack address of `0x0` on the
    // hardware (obSCEne `031-stackattr/fresh-attr-names-no-stack`), so the size is written and the
    // address stays zero.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(handle + ATTR_STACK_SIZE, DEFAULT_ATTR_STACK_SIZE) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[0], handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// What a thread attribute set reports as its stack size before anybody sets one.
///
/// 64 KiB, as the hardware answers (obSCEne `031-stackattr/fresh-attr-names-no-stack`); FreeBSD's
/// default is larger.
const DEFAULT_ATTR_STACK_SIZE: u64 = 0x1_0000;

/// `pthread_attr_getguardsize(attr, out)`, POSIX.1-2008.
fn pthread_attr_getguardsize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_GUARD_SIZE)
}

/// `pthread_attr_getinheritsched(attr, out)`, POSIX.1-2008.
fn pthread_attr_getinheritsched(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_INHERIT_SCHED)
}

/// `pthread_attr_getschedpolicy(attr, out)`, POSIX.1-2008.
fn pthread_attr_getschedpolicy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_SCHED_POLICY)
}

/// `pthread_attr_getscope(attr, out)`, POSIX.1-2008.
fn pthread_attr_getscope(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_SCOPE)
}

/// `pthread_attr_setscope(attr, scope)`, POSIX.1-2008.
fn pthread_attr_setscope(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_SCOPE)
}

/// `pthread_mutexattr_getpshared(attr, out)`, POSIX.1-2008.
fn pthread_mutexattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, MUTEXATTR_PSHARED)
}

/// `pthread_mutexattr_setpshared(attr, pshared)`, POSIX.1-2008.
fn pthread_mutexattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, MUTEXATTR_PSHARED)
}

/// `pthread_mutexattr_getprioceiling(attr, out)`, POSIX.1-2008.
fn pthread_mutexattr_getprioceiling(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, MUTEXATTR_PRIOCEILING)
}

/// `pthread_mutexattr_setprioceiling(attr, ceiling)`, POSIX.1-2008.
fn pthread_mutexattr_setprioceiling(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, MUTEXATTR_PRIOCEILING)
}

/// `pthread_condattr_getclock(attr, out)`, POSIX.1-2008.
fn pthread_condattr_getclock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, CONDATTR_CLOCK)
}

/// `pthread_condattr_setclock(attr, clock)`, POSIX.1-2008.
fn pthread_condattr_setclock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, CONDATTR_CLOCK)
}

/// `pthread_condattr_getpshared(attr, out)`, POSIX.1-2008.
fn pthread_condattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, CONDATTR_PSHARED)
}

/// `pthread_condattr_setpshared(attr, pshared)`, POSIX.1-2008.
fn pthread_condattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, CONDATTR_PSHARED)
}

/// `pthread_condattr_destroy(attr)`, POSIX.1-2008.
///
/// The object is leaked rather than freed, as for the thread attribute: a condition variable built
/// from it may still be live. The storage is small and bounded.
fn pthread_condattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if attr_at(args[0]).is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_equal(a, b)`: whether two thread identifiers name the same thread.
///
/// POSIX.1-2008. Non-zero for equal and zero for not, the opposite sense to `strcmp`.
fn pthread_equal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(args[0] == args[1])
}

// Barrier and read-write lock attributes, and the scheduling calls. The attribute objects follow
// `pthread_attr_init`: this crate allocates one, hands the guest a handle, and each accessor reads
// or writes a field by offset. Storing a value and answering it later is the contract (D272).

/// Whether a barrier or read-write lock is shared between processes.
///
/// Stored and answered, not acted on (see [`MUTEXATTR_PSHARED`]).
const LOCKATTR_PSHARED: u64 = 0;

/// A read-write lock's kind, from the non-portable `gettype_np`/`settype_np` pair.
const LOCKATTR_TYPE: u64 = 8;

/// Allocates an attribute object and hands the guest a handle to it.
///
/// Shared by the barrier and read-write-lock attribute initialisers; four words, as for the
/// condition-variable and mutex attributes.
fn lockattr_init(pointer: u64) -> u64 {
    if pointer == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // From the one region every guest-visible handle comes from, so it repeats (D584).
    let handle = orbistoun_mem::blocks::block(4);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(pointer, handle) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Accepts a destroy without reclaiming the object, as the other attribute destroys do: something
/// built from it may still be live.
fn lockattr_destroy(pointer: u64) -> u64 {
    if attr_at(pointer).is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_barrierattr_init(attr)`, POSIX.1-2008.
fn pthread_barrierattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_init(args[0])
}

/// `pthread_barrierattr_destroy(attr)`, POSIX.1-2008.
fn pthread_barrierattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_destroy(args[0])
}

/// `pthread_barrierattr_getpshared(attr, out)`, POSIX.1-2008.
fn pthread_barrierattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_PSHARED)
}

/// `pthread_barrierattr_setpshared(attr, pshared)`, POSIX.1-2008.
fn pthread_barrierattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_init(attr)`, POSIX.1-2008.
fn pthread_rwlockattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_init(args[0])
}

/// `pthread_rwlockattr_destroy(attr)`, POSIX.1-2008.
fn pthread_rwlockattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_destroy(args[0])
}

/// `pthread_rwlockattr_getpshared(attr, out)`, POSIX.1-2008.
fn pthread_rwlockattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_setpshared(attr, pshared)`, POSIX.1-2008.
fn pthread_rwlockattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_gettype_np(attr, out)`: the non-portable kind accessor.
///
/// Reference: FreeBSD `pthread_rwlockattr_settype_np(3)`. Stored and answered; the values are
/// not interpreted.
fn pthread_rwlockattr_gettype_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_TYPE)
}

/// `pthread_rwlockattr_settype_np(attr, kind)`: the non-portable kind setter.
fn pthread_rwlockattr_settype_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_TYPE)
}

/// `pthread_yield()`: offers the processor to another thread.
///
/// Reference: FreeBSD `pthread_yield(3)`; `sched_yield(2)` is the POSIX spelling. A hint by
/// definition, so handing it to the host scheduler is the whole implementation.
fn pthread_yield(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::yield_now();
    OK
}

/// `sched_yield()`, POSIX.1-2008, the same as [`pthread_yield`].
fn sched_yield(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::yield_now();
    OK
}

/// `pthread_getconcurrency()`: the concurrency level a guest asked for.
///
/// Reference: POSIX.1-2008. Zero unless the guest has set one, meaning "the system decides".
fn pthread_getconcurrency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    concurrency().load(std::sync::atomic::Ordering::Relaxed)
}

/// `pthread_setconcurrency(level)`: records the hint so the getter can answer it.
///
/// Reference: POSIX.1-2008, which permits ignoring the value; nothing else reads it.
fn pthread_setconcurrency(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    concurrency().store(args[0], std::sync::atomic::Ordering::Relaxed);
    OK
}

/// The concurrency hint, which only its own two accessors read.
fn concurrency() -> &'static std::sync::atomic::AtomicU64 {
    static LEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    &LEVEL
}

/// `pthread_cond_timedwait(cond, mutex, abstime)`, POSIX.1-2008.
///
/// `abstime` is an absolute deadline: a caller re-passes the same deadline around its wakeup
/// loop, so treating it as relative would restart the clock every turn. A deadline already past
/// is a zero wait. The same non-atomicity as [`pthread_cond_wait`] applies.
fn pthread_cond_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let deadline = read_xtime(args[2]);
    let timeout = deadline.map_or(std::time::Duration::ZERO, |d| {
        duration_until(d).unwrap_or(std::time::Duration::ZERO)
    });
    cond_timedwait(args[0], args[1], timeout)
}

/// The body both timed condition waits share, once the timeout is a plain span.
fn cond_timedwait(cond: u64, mutex: u64, timeout: std::time::Duration) -> u64 {
    let Some(handle) = cond_at(cond) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let held = mutex_at(mutex);
    let by = thread::adopt("main");
    if let Some(m) = held {
        sync::unlock(m, by);
    }
    let woken = sync::cond_wait(handle, Some(timeout));
    if let Some(m) = held {
        sync::acquire(m, by, sync::Blocking::Forever);
    }
    match woken {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `pthread_once(control, routine)`: runs an initialiser exactly once.
///
/// Reference: POSIX.1-2008 `pthread_once(3)`. The routine takes no arguments and returns
/// nothing, unlike `_Execute_once`'s `InitOnce`-shaped callback, so it does not forward there.
/// The flag is marked done after the routine returns. Not serialised across threads, as for
/// `_Execute_once`.
fn pthread_once(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The flag's value once the initialiser has completed.
    const DONE: u64 = 1;
    let (control, routine) = (args[0], args[1]);
    if control == 0 || routine == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::read_u64(control) } == Some(DONE) {
        return OK;
    }
    // SAFETY: `routine` is a guest function pointer the caller handed over, and `call_guest`
    // runs it on a fresh stack through the thread registry's reentrant call - the same contract
    // `_Execute_once` relies on. It takes no arguments, so the registers are left zero.
    let ran = unsafe { thread::call_guest(routine, [0, 0, 0]) };
    if ran.is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(control, DONE) };
    OK
}

// The timed acquisitions. The POSIX calls take an absolute deadline and FreeBSD's `_np`
// spellings a relative span. Reading an absolute time as relative restarts the clock every
// turn; reading a relative span as absolute makes every wait expire at once. Neither fails
// loudly, so each has its own reader.
//
// Reference: POSIX.1-2008 for `pthread_mutex_timedlock`, `pthread_rwlock_timedrdlock`,
// `pthread_rwlock_timedwrlock`, `sem_timedwait` and `sem_getvalue`; Solaris `sem_timedwait(3C)`
// and `pthread_cond_timedwait(3C)` for the `sem_reltimedwait_np` and
// `pthread_cond_reltimedwait_np` spellings and their arities.

/// Turns an absolute `timespec` into the moment a wait should give up.
///
/// `None` for a pointer that cannot be read, which the caller reports rather than treating as
/// "no timeout".
fn deadline_at(pointer: u64) -> Option<sync::Blocking> {
    let remaining = duration_until(read_xtime(pointer)?)?;
    Some(patience_for(remaining))
}

/// Turns a relative `timespec` into the same, for FreeBSD's `_np` spellings.
fn deadline_after(pointer: u64) -> Option<sync::Blocking> {
    Some(patience_for(read_xtime(pointer)?))
}

/// A span from now, as a deadline.
///
/// A span so large the host clock cannot represent the moment becomes "wait forever", which is
/// what the guest meant.
fn patience_for(span: std::time::Duration) -> sync::Blocking {
    std::time::Instant::now()
        .checked_add(span)
        .map_or(sync::Blocking::Forever, sync::Blocking::Until)
}

/// Answers an acquisition that was given a deadline.
///
/// `Some(false)` is a timeout here and busy in [`acquired`]; only the caller knows which patience
/// it asked for.
fn timed_out(outcome: Option<bool>) -> u64 {
    match outcome {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `pthread_mutex_timedlock(mutex, abstime)`, POSIX.1-2008.
fn pthread_mutex_timedlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    match sync::acquire(handle, thread::adopt("main"), until) {
        Some(sync::Acquisition::Locked) => OK,
        Some(sync::Acquisition::Busy) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw())
        }
        // The owner re-taking an error-checking lock, which no waiting resolves and which the hardware
        // answers with the invalid-argument errno.
        Some(sync::Acquisition::Deadlock) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `pthread_rwlock_timedrdlock(lock, abstime)`, POSIX.1-2008.
fn pthread_rwlock_timedrdlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, until)))
}

/// `pthread_rwlock_timedwrlock(lock, abstime)`, POSIX.1-2008.
fn pthread_rwlock_timedwrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, until)))
}

/// `sem_timedwait(sem, abstime)`, POSIX.1-2008, an absolute deadline.
///
/// Answers the code directly rather than `-1` with `errno`, as the rest of the `sem_*` family
/// here does: no guest `errno` is maintained. A caller testing against zero branches correctly.
fn sem_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, until)))
}

/// `sem_reltimedwait_np(sem, reltime)`: the same wait, given a span instead.
fn sem_reltimedwait_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_after(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, until)))
}

/// `sem_getvalue(sem, sval)`: how many the semaphore has free.
///
/// Reference: POSIX.1-2008. Four bytes, because `sval` is an `int *` (D272). The standard permits
/// a negative count of waiters or zero; this answers the count. The value may be stale when the
/// caller sees it, as the standard allows.
fn sem_getvalue(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (sem, out) = (args[0], args[1]);
    if out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Some(value) = posix_sema_at(sem).and_then(sync::semaphore_value) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_u32(out, value) } {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
    }
}

/// `pthread_cond_reltimedwait_np(cond, mutex, reltime)`: a relative timed wait.
///
/// The body of [`pthread_cond_timedwait`] with the relative time reader.
fn pthread_cond_reltimedwait_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let timeout = read_xtime(args[2]).unwrap_or(std::time::Duration::ZERO);
    cond_timedwait(args[0], args[1], timeout)
}

/// Fields a thread attribute object holds, by offset.
///
/// This crate defines the layout, which is sound because every call that touches one is here.
const ATTR_STACK_SIZE: u64 = 0;
/// Detach state, one word on.
const ATTR_DETACH: u64 = 8;
/// Scheduling priority, one further.
const ATTR_PRIORITY: u64 = 16;
/// Scheduling policy, one further.
const ATTR_SCHED_POLICY: u64 = 24;
/// Whether a thread inherits its creator's scheduling, one further.
const ATTR_INHERIT_SCHED: u64 = 32;
/// The CPU affinity mask, one further.
const ATTR_AFFINITY: u64 = 40;
/// The guard-page size, one further.
const ATTR_GUARD_SIZE: u64 = 48;

/// Contention scope - whether the thread competes for processor time within its process or
/// across the system.
const ATTR_SCOPE: u64 = 56;
/// The lowest address of the stack, one further, pointer-wide like the size.
const ATTR_STACK_ADDR: u64 = 64;

/// Stores one field of a thread attribute object.
fn attr_set(args: &[u64; GUEST_ARG_REGISTERS], field: u64) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(object + field, args[1]) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Reads one field of a thread attribute object into a guest `int`.
fn attr_get(args: &[u64; GUEST_ARG_REGISTERS], field: u64) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(value) = (unsafe { guest::read_u64(object + field) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Four bytes: the out-parameter is an `int` (D272).
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u32(args[1], value as u32) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadAttrSetstacksize(attr, size)`.
fn pthread_attr_setstacksize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_STACK_SIZE)
}

/// `scePthreadAttrGetstacksize(attr, out)`.
///
/// A size, so the out-parameter is pointer-width rather than an `int`.
fn pthread_attr_getstacksize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(value) = (unsafe { guest::read_u64(object + ATTR_STACK_SIZE) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], value) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadAttrGet(thread, attr)`: fills an attribute object with a running thread's
/// attributes.
///
/// FreeBSD's `pthread_attr_get_np(3)` is the reference: the object must already be initialised,
/// and it comes back describing the thread as it is. Garbage collectors call `scePthreadSelf`,
/// then this, then read the stack address and size to find the stack they scan. The stack comes
/// from the thread's record, or for the thread the guest was entered on, from the span the worker
/// reported. A thread with neither is refused rather than given a made-up stack.
fn pthread_attr_get(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, attr) = (args[0], args[1]);
    let Some(object) = attr_at(attr) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(record) = thread::record(handle) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let entered = handle == thread::current() && thread::this_stack().is_none();
    let stack =
        thread::stack_of(handle).or_else(|| entered.then(|| STACK_SPAN.get().copied()).flatten());
    let Some((base, len)) = stack else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let fields = [
        (ATTR_STACK_ADDR, base),
        (ATTR_STACK_SIZE, len),
        (
            ATTR_PRIORITY,
            u64::from(u32::from_ne_bytes(record.requested_priority.to_ne_bytes())),
        ),
        (
            ATTR_SCHED_POLICY,
            u64::from(u32::from_ne_bytes(record.requested_policy.to_ne_bytes())),
        ),
        (ATTR_AFFINITY, record.effective_affinity.0),
        (ATTR_GUARD_SIZE, orbistoun_mem::stack::GUARD_SIZE),
    ];
    for (field, value) in fields {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if !unsafe { guest::write_u64(object + field, value) } {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    OK
}

/// `scePthreadAttrGetstackaddr(attr, out)`: the lowest address of the stack an attribute names.
///
/// FreeBSD `pthread_attr_getstackaddr(3)`: the value set on the object, null on one nothing has
/// set, so a fresh object answers zero and success. Written pointer-wide.
fn pthread_attr_getstackaddr(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(value) = (unsafe { guest::read_u64(object + ATTR_STACK_ADDR) }) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], value) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadAttrSetdetachstate(attr, state)`.
fn pthread_attr_setdetachstate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_DETACH)
}

/// `scePthreadAttrGetdetachstate(attr, out)`.
fn pthread_attr_getdetachstate(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_DETACH)
}

/// `scePthreadAttrSetschedparam(attr, param)`.
fn pthread_attr_setschedparam(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_PRIORITY)
}

/// `scePthreadAttrGetschedparam(attr, out)`.
fn pthread_attr_getschedparam(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_PRIORITY)
}

/// `scePthreadAttrSetschedpolicy(attr, policy)`: stored, so a later get reads it back.
fn pthread_attr_setschedpolicy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_SCHED_POLICY)
}

/// `scePthreadAttrSetinheritsched(attr, inherit)`: stored, so a later get reads it back.
fn pthread_attr_setinheritsched(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_INHERIT_SCHED)
}

/// `scePthreadAttrSetaffinity(attr, mask)`.
///
/// The mask is stored, not applied: the host scheduler places guest threads (D523). A guest
/// reading the attribute back gets what it set.
fn pthread_attr_setaffinity(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_AFFINITY)
}

/// `scePthreadSetaffinity(thread, mask)`: the running-thread form.
///
/// Accepted and not applied (D523). There is nowhere to store it, so a later
/// `scePthreadGetaffinity` does not read it back. Answers `Ok` rather than the placeholder, which
/// a caller testing against zero would read as a refusal. The thread handle is not checked.
fn pthread_setaffinity(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePthreadGetaffinity(thread, mask)`: reads back the affinity recorded for a thread.
///
/// Answers the `requested_affinity` captured at thread creation (D523), not the effective mask,
/// which under the default `Observe` policy is always zero (D150). A thread with no record, or
/// that pinned nothing, reads zero, the guest's own "anywhere" ([`thread::Affinity::is_unset`]).
fn pthread_getaffinity(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mask = thread::record(args[0]).map_or(0, |record| record.requested_affinity.0);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], mask) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadAttrSetguardsize(attr, size)`: stored, so a later get reads it back.
fn pthread_attr_setguardsize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_GUARD_SIZE)
}

/// `scePthreadAttrDestroy(attr)`.
///
/// The block is leaked rather than freed, like every handle here, so a use after destroy reads a
/// stale object rather than freed memory.
fn pthread_attr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if attr_at(args[0]).is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], 0) };
    OK
}

/// Ticks per second the guest is told its counter runs at.
///
/// The rate the hardware reports: `0x5f25_9b8e`, about 1.596 GHz. The same hardware run
/// cross-checks it, with a sleep advancing the counter by `0x1f12cd9` ticks against `0x4fbb`
/// microseconds. A later run answered a value five ticks higher, which is measurement jitter.
const TSC_HZ: u64 = 0x5f25_9b8e;

/// Nanoseconds in a second, for converting the host's clock to the target's rate.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// `sceKernelReadTsc()`: the time stamp counter.
///
/// It advances; a constant would make every elapsed measurement zero.
fn read_tsc(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ticks_since()
}

/// Host time since `origin`, expressed in the target's own ticks.
///
/// Scaled to [`TSC_HZ`], so the counter agrees with `GetTscFrequency`, which a guest divides by.
/// Computed in `u128` because nanoseconds times the frequency overflows sixty-four bits in about
/// eleven seconds, and saturated so an overflow reports a stuck clock rather than a wrapped one.
fn ticks_since() -> u64 {
    // The source every other clock here reads, so conversions between them agree and one setting
    // decides whether all of them repeat (D582).
    let nanos = orbistoun_hle::clocks::since_start_nanos();
    u64::try_from(nanos * u128::from(TSC_HZ) / NANOS_PER_SECOND).unwrap_or(u64::MAX)
}

/// `sceKernelGetTscFrequency()`: ticks per second, matching [`read_tsc`].
fn get_tsc_frequency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    TSC_HZ
}

/// The guest stack, as the worker mapped it.
static STACK_SPAN: OnceLock<(u64, u64)> = OnceLock::new();

/// Records where the guest's stack is, so `sceKernelIsStack` can answer.
///
/// Told rather than derived, since this crate does not place the stack.
pub fn note_stack_span(base: u64, len: u64) {
    let _ = STACK_SPAN.set((base, len));
}

/// `sceKernelIsStack(address, low, high)`: where the calling thread's stack is.
///
/// Not a predicate. On the hardware it takes three arguments, returns `0` for a local and a
/// static alike, and writes the stack bounds through the two pointers after the address
/// (obSCEne `010-kernel/is-stack`, `031-stackattr/address-is-the-base`). The bounds are the
/// calling thread's: its own recorded span if it has one, else the main stack. Whether the
/// hardware writes the bounds for an address outside the stack is unmeasured; they are written
/// regardless, since they describe the caller's stack rather than the address.
fn is_stack(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let span = thread::this_stack().or_else(|| STACK_SPAN.get().copied());
    if let Some((base, len)) = span {
        // Null means the caller does not want that bound.
        if args[1] != 0 {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::write_u64(args[1], base) };
        }
        if args[2] != 0 {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::write_u64(args[2], base.saturating_add(len)) };
        }
    }
    // Zero whether or not the address is in the stack, as on the hardware.
    OK
}

/// The modules this process has loaded, as the guest should see them: the executable the loader
/// placed.
static LOADED_MODULES: OnceLock<Vec<(u64, String)>> = OnceLock::new();

/// Records which modules are loaded, for the guest to enumerate.
///
/// Told rather than derived, since this crate does not do the loading.
pub fn note_loaded_modules(modules: Vec<(u64, String)>) {
    let _ = LOADED_MODULES.set(modules);
}

/// `sceKernelGetModuleList(handles, max, written)`.
///
/// Reports the modules the loader placed, never a plausible-looking list.
fn get_module_list(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, max, written) = (args[0], args[1], args[2]);
    if out == 0 || written == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let loaded = LOADED_MODULES.get().map_or(&[][..], Vec::as_slice);
    let count = usize::try_from(max).unwrap_or(0).min(loaded.len());
    for (index, (handle, _)) in loaded.iter().take(count).enumerate() {
        // Handles are written as `int`, four bytes: the guest's array is `SceKernelModule[]` (D272).
        let Ok(at) = usize::try_from(out.saturating_add((index * 4) as u64)) else {
            return u64::from(GuestError::InvalidArgument.as_raw());
        };
        // SAFETY: a guest-supplied array under the identity mapping, written within
        // the element count the guest itself declared.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u32>(at),
                *handle as u32,
            );
        }
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u32(written, count as u32) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelGetModuleInfo(handle, info)`: refused, because the structure is not derivable.
///
/// The answer is a `SceKernelModuleInfo`, whose field offsets no lawful source here describes,
/// so filling it would put guessed values where a guest reads a name (D395). It is refused with
/// `0x8002_0016` (`INVALID`), the code the hardware answers in obSCEne `110-modules`. A byte dump
/// of the structure from the hardware is what would let it be implemented; the handle and name
/// are already in `LOADED_MODULES`.
fn get_module_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if describing_module_info() {
        return describe_with_markers(args[1]);
    }
    u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
}

/// Where a marker in an undescribed structure points.
///
/// Its own base, so a fault address says *this structure, this offset* rather than being
/// mistaken for a handoff sentinel or a content marker.
const DESCRIBED_BASE: u64 = 0x0000_5E2B_0000_0000;

/// How much of a structure to fill, when nothing says how big it is.
///
/// The caller's own size field is not trusted, so a fixed, generous span is filled. A guest that
/// declared less gets more written than it asked for, which is why this is a diagnostic.
const DESCRIBED_WORDS: usize = 64;

/// Whether this run was asked to describe what it cannot describe.
fn describing_module_info() -> bool {
    orbistoun_env::DESCRIBE.get().as_deref() == Some("module-info")
}

/// Fills a structure with markers that name their own offset, and reports success.
///
/// A layout that cannot be derived can be measured: a guest that reads a field and uses it as a
/// pointer, a length or a handle stops on an address that decodes back to the offset it was read
/// from (D390). A diagnostic, recorded as intervening: it writes guest memory and answers success
/// for something that did not happen.
fn describe_with_markers(at: u64) -> u64 {
    let Ok(base) = usize::try_from(at) else {
        return FAILED_STATUS;
    };
    if base == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    for word in 0..DESCRIBED_WORDS {
        let offset = word * 8;
        let says = DESCRIBED_BASE + offset as u64;
        // SAFETY: a guest-supplied structure under the identity mapping, which the
        // guest passed to be written through - this writes further into it than it may have
        // asked for, which is why the setting says so and is not a default.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(base + offset),
                says,
            );
        }
    }
    OK
}

/// What a call answers when it will not do what it was asked.
///
/// Negative, as this FreeBSD-derived kernel reports failure, so it can never be read as a small
/// successful value.
const FAILED_STATUS: u64 = -1_i64 as u64;

/// `sceKernelLoadStartModule(path, argc, argv, flags, opt, result)`.
///
/// The answers per kind of path are measured (obSCEne `110-modules/load`):
///
/// - libkernel is always resident and answers its well-known handle.
/// - A firmware module is the platform's own copy; it is not loaded again and answers the
///   not-found errno.
/// - An `/app0` module is the title's own, and gets a fresh non-negative handle. The value need not
///   match the hardware's, whose handles depend on what its loader had already placed.
/// - Anything else is refused.
///
/// The module is placed before the guest runs; starting it runs its initialisers (D515).
fn load_start_module(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let path = unsafe { read_name(args[0]) };

    // libkernel is always resident, at the handle every guest reaches it by.
    if path.contains("libkernel") {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u32(args[5], 0) };
        return LIBKERNEL_MODULE_HANDLE;
    }

    // A firmware module is the platform's own copy; the hardware answers `0x8002_0002` for `/system`
    // paths (obSCEne `110-modules/load`). The same rule covers every directory in
    // `FIRMWARE_MODULE_DIRECTORIES`; the `/system_ex` case is unmeasured.
    if FIRMWARE_MODULE_DIRECTORIES
        .iter()
        .any(|dir| path.starts_with(dir))
    {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    }

    // A title's own module, under `/app0`, gets a fresh non-negative handle, which a guest keys its
    // later calls on.
    if path.starts_with("/app0/") {
        let handle = NEXT_MODULE_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u32(args[5], 0) };
        // The module is already placed, relocated and protected by `place_title_modules`, so starting it
        // runs its `DT_INIT` and `DT_INIT_ARRAY` (D515). A module the loader never recorded is reported
        // as unstarted.
        match initialisers_for(&path) {
            Some(initialisers) => started(&path, handle, run_initialisers(initialisers)),
            None => started_nothing(&path, handle),
        }
        return handle;
    }

    // Anything else is not a module location this kernel recognises, and is refused rather than
    // reported as loaded.
    u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw())
}

/// Where the platform keeps its own modules, so a request for one is refused rather than loaded.
///
/// A list, because `/system_ex` is not under `/system`.
const FIRMWARE_MODULE_DIRECTORIES: &[&str] = &[
    // The application tier, mapped into every game sandbox.
    "/system/common/lib/",
    // The system-application tier. Not under `/system/`.
    "/system_ex/common_ex/lib/",
    // Privileged services.
    "/system/priv/lib/",
];

/// libkernel's module handle, the one well-known value in the space, confirmed on the hardware.
const LIBKERNEL_MODULE_HANDLE: u64 = 0x2001;

/// The next handle handed to a freshly loaded `/app0` module. Starts clear of the low handles
/// the loader's own placed modules use and of `LIBKERNEL_MODULE_HANDLE`.
static NEXT_MODULE_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x40);

/// Whether this is the first time a name has been looked up this run.
///
/// A guest with several threads or a second initialisation pass resolves a name again, and a
/// line per call would bury the list in repeats.
fn first_time_asked(name: &str) -> bool {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    static ASKED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let Ok(mut guard) = ASKED.lock() else {
        // A poisoned lock means another thread panicked holding it; reporting again is harmless.
        return true;
    };
    guard
        .get_or_insert_with(Default::default)
        .insert(name.to_owned())
}

/// What the guest's own binaries export, by NID, at the address they were placed.
///
/// `sceKernelDlsym` is handed a name and export tables are keyed by a hash of it, which cannot be
/// reversed, so the loader registers what it placed and the kernel hashes at the call (D517).
static GUEST_EXPORTS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

/// The hash suffix the loader is using, so a name can be turned into the NID it exports under.
static NID_SUFFIX: OnceLock<Vec<u8>> = OnceLock::new();

/// Tells this crate what a guest binary exports, so `sceKernelDlsym` can answer for it.
///
/// `exports` are `(nid, address)` with the address already offset by wherever the module was
/// placed. Called once per placed binary, including the executable, whose exports a guest may ask
/// for by name (D517).
pub fn note_guest_exports(suffix: &[u8], exports: &[(u64, u64)]) {
    let _ = NID_SUFFIX.set(suffix.to_vec());
    if let Ok(mut known) = GUEST_EXPORTS.lock() {
        for &(nid, address) in exports {
            if !known.iter().any(|&(n, _)| n == nid) {
                known.push((nid, address));
            }
        }
    }
}

/// The address a guest binary exports `name` at, if one does.
fn guest_export(name: &str) -> Option<u64> {
    let suffix = NID_SUFFIX.get()?;
    let nid = orbistoun_nid::NidHasher::new(suffix.clone())
        .hash(name)
        .as_raw();
    let known = GUEST_EXPORTS.lock().ok()?;
    known
        .iter()
        .find(|&&(candidate, _)| candidate == nid)
        .map(|&(_, address)| address)
}

/// Whether libkernel is where this run says a name lives, or whether nothing says.
///
/// A name libkernel declares is `true`, a name declared elsewhere is `false`, and a name nothing
/// has a verdict on is also `true`, so the refusal built on this can only correct a measured wrong
/// success, never invent a failure.
fn libkernel_exports(name: &str) -> bool {
    orbistoun_thunk::name_is_libkernel(name).unwrap_or(true)
}

/// `sceKernelDlsym(module, name, address)`: the address of a function, by name, at run time.
///
/// Open-toolchain payload runtimes resolve their C library one name at a time through this call
/// (D365). A name is looked up in the same stub table imports resolve into, so a function reached
/// this way and by import is the same address with the same trace and implementation. A name
/// with no implementation gets a failure and no write.
fn dlsym(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (module, name, out) = (args[0], args[1], args[2]);
    // The module handle is checked before the name. It is a 32-bit `SceKernelModule`, so only its
    // low word is meaningful. Every handle this kernel issues is non-negative, so a negative one
    // (such as the `-1` obSCEne `060-module/dlsym-rejects-bad-handle` passes) names no module and
    // earns `ESRCH` (`0x80020003`).
    if (module as u32 as i32) < 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    if name == 0 || out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(name) };
    // The stub table first, then the guest's own exports (D517). A name the guest's own binary
    // exports is always looked up. On a title's route the stub table is not consulted: the hardware
    // does not hand a launched title platform functions by name, while a payload's runtime needs them
    // (D669).
    let resolves_by_name =
        orbistoun_core::route::resolves_by_name(orbistoun_core::route::presented());
    let from_stubs = if resolves_by_name {
        orbistoun_thunk::name_thunk(&name)
    } else {
        None
    };

    // A module handle narrows the answer for libkernel's handle: the hardware answers `0x80020003`
    // when libkernel is asked for a name it does not export, such as `memcpy` (obSCEne
    // `110-modules/symbol`). Narrowed only downward: it applies where a name is declared here and
    // declared in another library, so it cannot turn a working resolution into a failure.
    if module == LIBKERNEL_MODULE_HANDLE && from_stubs.is_some() && !libkernel_exports(&name) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    // Only when a run asked for it (`ORBISTOUN_DLSYM_STUBS`), a stub for a name this project declares
    // and does not implement. Empty in an ordinary run, and last, so it never shadows an
    // implementation or the guest's own export.
    let address = from_stubs
        .or_else(|| guest_export(&name))
        .or_else(|| orbistoun_thunk::declared_thunk(&name));

    // Every distinct name, once, answered or not: a payload's resolution pass states what its
    // runtime is built from (D366).
    if first_time_asked(&name) {
        // Three verdicts: a title's refusal is not "nothing implements it", since the function is
        // reachable by import and the platform does not hand it out by name (D669).
        let verdict = match address {
            Some(at) => format!("answered {at:#x}"),
            None if !resolves_by_name => {
                "which a title's route does not resolve by name, as the console does not".to_owned()
            }
            None => "which nothing here implements".to_owned(),
        };
        if address.is_some() {
            tracing::debug!("the guest asked for the address of {name} - {verdict}");
        } else {
            tracing::warn!("the guest asked for the address of {name} - {verdict}");
        }
        let line = format!("orbistoun: the guest asked for the address of {name} - {verdict}");
        // And to the kernel log, which the guest's log service forwards (D389).
        orbistoun_core::klog::note(&line);
    }

    let Some(address) = address else {
        // The hardware's refusal where the route explains it, the placeholder where it does not. The
        // placeholder means "nothing here implements this"; a title asking for a platform name gets
        // `0x8002_0003` on the hardware, since the function exists and the platform does not hand it out
        // by name (D669).
        return u64::from(if resolves_by_name {
            GuestError::Unimplemented.as_raw()
        } else {
            orbistoun_core::route::NAME_NOT_RESOLVED
        });
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(out, address) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelSendNotificationRequest(device, request, size, blocking)`: the on-screen notice a
/// payload shows to say it started.
///
/// Answers success, since failing a cosmetic call would send a working payload down an error
/// path. The message is not read: the request layout is a vendor structure no lawful source here
/// describes. What is reported, once per run, is that a notification was asked for and its size.
fn send_notification_request(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!(
            "the guest asked to show a notification ({} bytes at {:#x}) - accepted, and the message is not decoded because the structure is not published",
            args[2],
            args[1]
        );
    }
    OK
}

/// `pthread_create(thread, attr, start, arg)`: the POSIX spelling, which takes no name.
///
/// Three vendor thread calls end in a name their POSIX twins do not have:
/// `scePthreadCreate(..., name)`, `scePthreadCondInit(..., name)` and
/// `scePthreadMutexInit(..., name)`. Delegating the POSIX spelling to the vendor one would read
/// an uninitialised argument register as a string pointer, so each POSIX spelling passes no name.
fn posix_pthread_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_create(&[args[0], args[1], args[2], args[3], 0, 0])
}

/// `pthread_cond_init(cond, attr)`: two arguments, and no name. See [`posix_pthread_create`].
fn posix_pthread_cond_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_cond_init(&[args[0], args[1], 0, 0, 0, 0])
}

/// `pthread_mutex_init(mutex, attr)`: two arguments, and no name. See [`posix_pthread_create`].
fn posix_pthread_mutex_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_mutex_init(&[args[0], args[1], 0, 0, 0, 0])
}

/// `pthread_detach(thread)`: says nobody will join this thread.
///
/// Every guest thread here is a host thread reclaimed when its body returns, so the promise is
/// kept by construction. A handle nobody issued is still refused.
///
/// Reference: POSIX.1-2008 `pthread_detach(3)`.
fn pthread_detach(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if thread::is_issued(args[0]) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `pthread_exit(value)`: ends the calling thread, and never returns.
///
/// On the main thread this ends the program, reported as a deliberate guest exit rather than a
/// fault (D177). On any other thread it ends only that thread: guest frames cannot be unwound, so
/// the thread is parked, which stops it executing guest code, and its stack is not reclaimed. The
/// process's first thread is adopted rather than spawned, which is how the two are told apart.
///
/// Reference: POSIX.1-2008 `pthread_exit(3)`.
fn pthread_exit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if thread::current() == thread::adopt("main") {
        orbistoun_core::stop(orbistoun_core::StopReason::Exited, args[0])
    } else {
        tracing::warn!(
            "a guest thread ended itself with pthread_exit - parked rather than unwound, because nothing here can unwind guest frames"
        );
        loop {
            std::thread::park();
        }
    }
}

/// Handlers a guest has installed, by signal number.
///
/// A map rather than an array, because the valid signal range is not measured (see
/// [`raise_exception`]).
fn installed_handlers() -> &'static Mutex<std::collections::BTreeMap<u64, u64>> {
    static HANDLERS: OnceLock<Mutex<std::collections::BTreeMap<u64, u64>>> = OnceLock::new();
    HANDLERS.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// `sceKernelInstallExceptionHandler(signum, handler)`: registers a function to run on a signal.
///
/// Measured by obSCEne `030-thread/exception-handler`: installing for signal 30 answers `0x0`;
/// installing again, with another handler or with null, answers `0x80020023` (errno 35). Null is
/// not an uninstall, because the duplicate check comes first; `sceKernelRemoveExceptionHandler`
/// is the way out. Argument order is measured: an inverted call answers `EINVAL`, and guest
/// handlers expect to be handed 30 in `edi`.
fn install_exception_handler(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Installed here rather than during setup: delivery matters only once a guest has a handler, a
    // `OnceLock` makes repeat calls free, and nothing can raise before something installs.
    sync::install_signal_delivery(sync::SignalDelivery {
        pending: signal_is_pending,
        deliver: deliver_pending_signal,
    });
    let (signum, handler) = (args[0], args[1]);
    let Ok(mut handlers) = installed_handlers().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if handlers.contains_key(&signum) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::AGAIN).as_raw());
    }
    handlers.insert(signum, handler);
    0
}

/// `sceKernelRemoveExceptionHandler(signum)`: undoes an install.
///
/// Measured as `0x0` for a signal that has a handler. Removing a handler never installed is
/// unmeasured and answers the placeholder.
fn remove_exception_handler(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let signum = args[0];
    let Ok(mut handlers) = installed_handlers().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if handlers.remove(&signum).is_some() {
        return 0;
    }
    u64::from(GuestError::Unimplemented.as_raw())
}

/// Runs whatever signal is waiting on the calling thread.
///
/// Installed into [`sync`] as the delivery half of its hook pair, so a thread asleep in a wait can
/// run a handler raised on it from elsewhere. Silent when nothing is pending, the ordinary case.
fn deliver_pending_signal() {
    let Some(signum) = thread::take_pending() else {
        return;
    };
    let installed = match installed_handlers().lock() {
        Ok(handlers) => handlers.get(&signum).copied(),
        Err(_) => None,
    };
    let Some(handler) = installed else {
        // The handler was removed between the raise and the wake. The hardware would take the default
        // action; ending the process for a race is worse than dropping a signal nothing listens for.
        return;
    };
    // The same value in both, as measured: `rsi` and `rdx` carry the identical pointer.
    // SAFETY: `handler` is a guest function pointer the guest passed to
    // `sceKernelInstallExceptionHandler`; `call_guest_placing` reserves and guards its own stack,
    // and the context is written into the low end of that same stack - inside the span it is
    // given, and released with it.
    let _ = unsafe {
        thread::call_guest_placing(handler, |low, _len| {
            let context = exception_context(signum, low);
            [signum, context, context]
        })
    };
}

/// How far into the handler's stack the context is placed.
///
/// On the stack, as the hardware places it, because guests scan forward from the context to the
/// end of its allocation, and a stack bounds that scan with memory the guest owns. The low end,
/// because a stack grows down and the frames in use are at the top.
const CONTEXT_INTO_STACK: u64 = 0x1000;

/// Offset of the signal number within the context, as measured by obSCEne
/// `030-thread/exception-handler`.
const CONTEXT_SIGNAL: u64 = 0x48;

/// Offset of a pointer handlers read, which refers to another address on the same stack.
///
/// On the hardware it points `0x7e8` past the context; only the pointer's existence and
/// reachability are facts, not what it holds.
const CONTEXT_INNER: u64 = 0xf8;

/// How far past the context the measured inner pointer pointed.
const CONTEXT_INNER_DELTA: u64 = 0x7e8;

/// How much of the context is written.
///
/// obSCEne dumped `0x180` bytes from the pointer a real handler was given; the stack beyond it is
/// already zero.
const CONTEXT_WRITTEN: u64 = 0x180;

/// Builds the block a handler is handed in `rsi` and `rdx`, and answers its address.
///
/// Reproduced from the hardware: the block is on the stack, the signal number is at `+0x48`, a
/// pointer at `+0xf8` refers to another address on the same stack, and `rsi` and `rdx` carry the
/// same value. Everything else is zero, including the register frame at `+0x100..+0x170`, which
/// orbistoun has no state for; a zero makes a guest read a null and check rather than resume onto
/// plausible values (D323).
fn exception_context(signum: u64, low: u64) -> u64 {
    let base = low.saturating_add(CONTEXT_INTO_STACK);
    for offset in (0..CONTEXT_WRITTEN).step_by(8) {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u64(base + offset, 0) };
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(base + CONTEXT_SIGNAL, signum) };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(base + CONTEXT_INNER, base + CONTEXT_INNER_DELTA) };
    base
}

/// Whether the calling thread has a signal waiting: the cheap half of the hook pair.
fn signal_is_pending() -> bool {
    thread::signal_pending()
}

/// `sceKernelRaiseException(thread, signum)`: runs a thread's handler for a signal.
///
/// Measured by obSCEne `030-thread/exception-handler`: `raise(self, 30)` with a handler answers
/// `0x0`; `raise(self, 31)` answers `0x8002_0016` (`EINVAL`); `raise(1, 30)` answers `0x8002_0003`
/// (`ESRCH`); `raise(self, 30)` with no handler does not return, the process dies of the signal.
/// Delivery to the calling thread is synchronous: a flag the handler sets reads `1` on the line
/// after `raise` returns.
///
/// The handler receives the signal number in `rdi` and a context pointer in `rsi` and `rdx`; on
/// the calling thread the context is null, so a handler that walks it faults at a named address
/// (D323). Only 30 and 31 are measured; any other number answers the placeholder.
fn raise_exception(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The one signal measured as accepted.
    const DELIVERABLE: u64 = 30;
    /// Measured as refused, with a valid thread handle.
    const REFUSED: u64 = 31;
    let (thread, signum) = (args[0], args[1]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if signum == REFUSED {
        return vendor(orbistoun_core::errno::INVALID);
    }
    if signum != DELIVERABLE {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if !thread::is_issued(thread) {
        return vendor(orbistoun_core::errno::NO_SUCH);
    }
    let installed = match installed_handlers().lock() {
        Ok(handlers) => handlers.get(&signum).copied(),
        Err(_) => return u64::from(GuestError::Unimplemented.as_raw()),
    };
    let Some(handler) = installed else {
        // The process dies of the signal on the hardware; stopping the guest emulates that, and names
        // the signal.
        orbistoun_core::stop(orbistoun_core::StopReason::Signalled, signum);
    };
    // Cross-thread: the hardware delivers whatever the target is doing, but orbistoun can reach only
    // a target parked in a wait it owns. `raise_pending` answers whether it is, and a target
    // anywhere else is refused (D652).
    if thread != thread::current() {
        if !thread::raise_pending(thread, signum) {
            return u64::from(GuestError::Unimplemented.as_raw());
        }
        sync::nudge_address_waiters();
        return 0;
    }
    // SAFETY: `handler` is a guest function pointer the guest itself passed to
    // `sceKernelInstallExceptionHandler`; `call_guest` reserves and guards its own stack. The
    // context arguments are null, which the handler may test but must not dereference.
    let ran = unsafe { thread::call_guest(handler, [signum, 0, 0]) };
    match ran {
        Some(_) => 0,
        None => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `sceKernelMapperGetParam(out)`: fills a size-prefixed structure and answers `0`.
///
/// Measured on firmware 12.40 (obSCEne `137-kernelcall/mapper-param`): answers `0` and fills the
/// 48 bytes after the 56-byte structure's leading size quadword with [`MAPPER_PARAM`], leaving the
/// size word as the caller wrote it. Guests abort on a non-zero answer. Only the bytes the
/// caller's size declares are written.
fn mapper_get_param(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let Some(declared) = (unsafe { guest::read_u64(out) }) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let room = usize::try_from(declared)
        .unwrap_or(0)
        .min(MAPPER_PARAM.len() + 8);
    let body = room.saturating_sub(8);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if body > 0 && !unsafe { guest::write_bytes(out + 8, &MAPPER_PARAM[..body]) } {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    OK
}

/// The 48 bytes `sceKernelMapperGetParam` writes after the size quadword, as the hardware writes
/// them: `0x80000000000`, then `0x88000000000` three times, then `0x40` and `0x2663`.
const MAPPER_PARAM: [u8; 48] = {
    let words: [u64; 6] = [
        0x0000_0800_0000_0000,
        0x0000_0880_0000_0000,
        0x0000_0880_0000_0000,
        0x0000_0880_0000_0000,
        0x40,
        0x2663,
    ];
    let mut bytes = [0u8; 48];
    let mut i = 0;
    while i < 48 {
        bytes[i] = words[i / 8].to_le_bytes()[i % 8];
        i += 1;
    }
    bytes
};

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived, and a table written in hashes could not be
/// read by a person or checked against the declarations above.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    TABLE
}

/// Every implementation, as one table; [`implementations`] is the interface other crates call.
const TABLE: &[(&str, GuestFn)] = &[
    ("sceKernelDirectMemoryQuery", direct_memory_query),
    ("sceKernelGetSystemSwVersion", get_system_sw_version),
    ("sceKernelDlsym", dlsym),
    ("pthread_detach", pthread_detach),
    ("pthread_setcancelstate", posix_pthread_setcancelstate),
    ("pthread_exit", pthread_exit),
    // POSIX thread-specific-data keys, with no vendor twin, served under their POSIX names via
    // `orbistoun-posix` (D453).
    ("pthread_key_create", pthread_key_create),
    ("pthread_setspecific", pthread_setspecific),
    ("pthread_getspecific", pthread_getspecific),
    ("pthread_key_delete", pthread_key_delete),
    // The three whose POSIX spelling is one argument shorter than the vendor one.
    ("pthread_create", posix_pthread_create),
    ("pthread_attr_getguardsize", pthread_attr_getguardsize),
    ("pthread_attr_getinheritsched", pthread_attr_getinheritsched),
    ("pthread_attr_getschedpolicy", pthread_attr_getschedpolicy),
    ("pthread_attr_getscope", pthread_attr_getscope),
    ("pthread_attr_setscope", pthread_attr_setscope),
    ("pthread_mutexattr_getpshared", pthread_mutexattr_getpshared),
    ("pthread_mutexattr_setpshared", pthread_mutexattr_setpshared),
    (
        "pthread_mutexattr_getprioceiling",
        pthread_mutexattr_getprioceiling,
    ),
    (
        "pthread_mutexattr_setprioceiling",
        pthread_mutexattr_setprioceiling,
    ),
    ("pthread_condattr_getclock", pthread_condattr_getclock),
    ("pthread_condattr_setclock", pthread_condattr_setclock),
    ("pthread_condattr_getpshared", pthread_condattr_getpshared),
    ("pthread_condattr_setpshared", pthread_condattr_setpshared),
    ("pthread_condattr_destroy", pthread_condattr_destroy),
    ("pthread_equal", pthread_equal),
    ("pthread_cond_init", posix_pthread_cond_init),
    ("pthread_mutex_init", posix_pthread_mutex_init),
    (
        "sceKernelSendNotificationRequest",
        send_notification_request,
    ),
    ("sceKernelGetDirectMemorySize", direct_memory_size),
    (
        "sceKernelAllocateMainDirectMemory",
        allocate_main_direct_memory,
    ),
    ("sceKernelMapNamedDirectMemory", map_named_direct_memory),
    ("sceKernelBatchMap", batch_map),
    ("sceKernelMapperGetParam", mapper_get_param),
    ("scePthreadCreate", pthread_create),
    ("scePthreadJoin", pthread_join),
    ("scePthreadSelf", pthread_self),
    ("scePthreadGetthreadid", pthread_getthreadid),
    ("sceKernelCreateSema", create_semaphore),
    ("scePthreadMutexattrInit", pthread_mutexattr_init),
    ("scePthreadCondattrInit", pthread_condattr_init),
    ("scePthreadMutexattrSettype", pthread_mutexattr_settype),
    ("scePthreadMutexattrGettype", pthread_mutexattr_gettype),
    (
        "scePthreadMutexattrSetprotocol",
        pthread_mutexattr_setprotocol,
    ),
    (
        "scePthreadMutexattrGetprotocol",
        pthread_mutexattr_getprotocol,
    ),
    ("scePthreadMutexattrDestroy", pthread_mutexattr_accept),
    ("scePthreadMutexInit", pthread_mutex_init),
    ("scePthreadMutexLock", pthread_mutex_lock),
    ("scePthreadMutexUnlock", pthread_mutex_unlock),
    ("scePthreadMutexTrylock", pthread_mutex_trylock),
    ("scePthreadMutexDestroy", pthread_mutex_destroy),
    ("vendor_system_version", vendor_system_version),
    ("sceKernelGetProcessTime", kernel_get_process_time),
    (
        "sceKernelGetProcessTimeCounter",
        kernel_get_process_time_counter,
    ),
    (
        "sceKernelGetProcessTimeCounterFrequency",
        kernel_get_process_time_counter_frequency,
    ),
    ("scePthreadCondInit", pthread_cond_init),
    ("scePthreadCondWait", pthread_cond_wait),
    ("scePthreadCondSignal", pthread_cond_signal),
    ("scePthreadCondBroadcast", pthread_cond_broadcast),
    ("scePthreadCondDestroy", pthread_cond_destroy),
    ("scePthreadRwlockInit", pthread_rwlock_init),
    ("scePthreadRwlockRdlock", pthread_rwlock_rdlock),
    ("scePthreadRwlockTryrdlock", pthread_rwlock_tryrdlock),
    ("scePthreadRwlockWrlock", pthread_rwlock_wrlock),
    ("scePthreadRwlockTrywrlock", pthread_rwlock_trywrlock),
    ("scePthreadRwlockUnlock", pthread_rwlock_unlock),
    ("scePthreadRwlockDestroy", pthread_rwlock_destroy),
    ("posix_pthread_rwlock_init", posix_pthread_rwlock_init),
    ("posix_pthread_rwlock_rdlock", pthread_rwlock_rdlock),
    ("posix_pthread_rwlock_tryrdlock", pthread_rwlock_tryrdlock),
    ("posix_pthread_rwlock_wrlock", pthread_rwlock_wrlock),
    ("posix_pthread_rwlock_trywrlock", pthread_rwlock_trywrlock),
    ("posix_pthread_rwlock_unlock", pthread_rwlock_unlock),
    ("posix_pthread_rwlock_destroy", pthread_rwlock_destroy),
    // The barrier and read-write lock attribute accessors.
    ("pthread_barrierattr_init", pthread_barrierattr_init),
    ("pthread_barrierattr_destroy", pthread_barrierattr_destroy),
    (
        "pthread_barrierattr_getpshared",
        pthread_barrierattr_getpshared,
    ),
    (
        "pthread_barrierattr_setpshared",
        pthread_barrierattr_setpshared,
    ),
    ("pthread_rwlockattr_init", pthread_rwlockattr_init),
    ("pthread_rwlockattr_destroy", pthread_rwlockattr_destroy),
    (
        "pthread_rwlockattr_getpshared",
        pthread_rwlockattr_getpshared,
    ),
    (
        "pthread_rwlockattr_setpshared",
        pthread_rwlockattr_setpshared,
    ),
    (
        "pthread_rwlockattr_gettype_np",
        pthread_rwlockattr_gettype_np,
    ),
    (
        "pthread_rwlockattr_settype_np",
        pthread_rwlockattr_settype_np,
    ),
    ("pthread_yield", pthread_yield),
    ("sched_yield", sched_yield),
    ("pthread_getconcurrency", pthread_getconcurrency),
    ("pthread_setconcurrency", pthread_setconcurrency),
    // The POSIX timed wait and once-only initialiser.
    ("pthread_cond_timedwait", pthread_cond_timedwait),
    ("pthread_once", pthread_once),
    // The timed acquisitions.
    ("pthread_mutex_timedlock", pthread_mutex_timedlock),
    ("pthread_rwlock_timedrdlock", pthread_rwlock_timedrdlock),
    ("pthread_rwlock_timedwrlock", pthread_rwlock_timedwrlock),
    ("sem_timedwait", sem_timedwait),
    ("sem_reltimedwait_np", sem_reltimedwait_np),
    ("sem_getvalue", sem_getvalue),
    ("pthread_cond_reltimedwait_np", pthread_cond_reltimedwait_np),
    ("scePthreadBarrierInit", pthread_barrier_init),
    ("posix_pthread_barrier_init", posix_pthread_barrier_init),
    ("scePthreadBarrierWait", pthread_barrier_wait),
    ("scePthreadBarrierDestroy", pthread_barrier_destroy),
    ("sceKernelCreateEventFlag", kernel_create_event_flag),
    ("sceKernelCreateEqueue", kernel_create_equeue),
    ("sceKernelAddUserEventEdge", kernel_add_user_event_edge),
    ("sceKernelWaitEqueue", kernel_wait_equeue),
    ("scePthreadGetschedparam", pthread_getschedparam),
    ("scePthreadSetschedparam", pthread_setschedparam),
    ("scePthreadSetprio", pthread_setprio),
    ("scePthreadRename", pthread_rename),
    ("_sigprocmask", sigprocmask),
    ("sceKernelUuidCreate", kernel_uuid_create),
    // Nothing reads a condattr, so destroying one is accepting the call, as for the mutexattr form.
    // The block is never freed.
    ("scePthreadCondattrDestroy", pthread_mutexattr_accept),
    ("sceKernelPollEventFlag", kernel_poll_event_flag),
    ("sceKernelWaitEventFlag", kernel_wait_event_flag),
    ("sceKernelSetEventFlag", kernel_set_event_flag),
    ("sceKernelClearEventFlag", kernel_clear_event_flag),
    ("sceKernelDeleteEventFlag", kernel_delete_event_flag),
    ("sceKernelPollSema", kernel_poll_sema),
    ("sceKernelSignalSema", kernel_signal_sema),
    ("sceKernelWaitSema", kernel_wait_sema),
    ("sceKernelDeleteSema", kernel_delete_sema),
    ("sceKernelSyncOnAddressWait", sync_on_address_wait),
    ("sceKernelSyncOnAddressWake", sync_on_address_wake),
    // POSIX unnamed semaphores, with no vendor twin, served under their POSIX names via
    // `orbistoun-posix`.
    ("sem_init", sem_init),
    ("sem_wait", sem_wait),
    ("sem_trywait", sem_trywait),
    ("sem_post", sem_post),
    ("sem_destroy", sem_destroy),
    ("sceKernelAllocateDirectMemory", allocate_direct_memory),
    ("sceKernelMapDirectMemory", map_named_direct_memory),
    ("sceKernelReleaseDirectMemory", release_direct_memory),
    ("sceKernelMunmap", munmap),
    ("sceKernelReserveVirtualRange", reserve_virtual_range),
    ("sceKernelVirtualQuery", virtual_query),
    ("sceKernelMprotect", mprotect),
    (
        "sceKernelInstallExceptionHandler",
        install_exception_handler,
    ),
    ("sceKernelRemoveExceptionHandler", remove_exception_handler),
    ("sceKernelRaiseException", raise_exception),
    ("sceKernelSetVirtualRangeName", set_virtual_range_name),
    ("mmap", mmap),
    ("sceKernelMmap", mmap),
    (
        "_ZSt13_Execute_onceRSt9once_flagPFiPvS1_PS1_ES1_",
        execute_once,
    ),
    // The C-runtime threading family the C++ standard library lowers onto: declared in
    // `orbistoun-libc`, implemented here beside the thread registry and `sync`.
    ("_Mtx_init", c_mtx_init),
    ("_Mtx_destroy", c_mtx_destroy),
    ("_Mtx_lock", c_mtx_lock),
    ("_Mtx_unlock", c_mtx_unlock),
    ("_Mtx_trylock", c_mtx_trylock),
    ("_Cnd_init", c_cnd_init),
    ("_Cnd_destroy", c_cnd_destroy),
    ("_Cnd_wait", c_cnd_wait),
    ("_Cnd_timedwait", c_cnd_timedwait),
    ("_Cnd_signal", c_cnd_signal),
    ("_Cnd_broadcast", c_cnd_broadcast),
    ("_Xtime_get_ticks", xtime_get_ticks),
    ("_Thrd_sleep", thrd_sleep),
    // libSceUlt mutexes, declared in the `ult` module.
    ("_sceUltMutexCreate", ult_mutex_create),
    ("sceUltInitialize", ult_initialize),
    (
        "sceUltWaitingQueueResourcePoolGetWorkAreaSize",
        ult_pool_work_area_size,
    ),
    (
        "sceUltUlthreadRuntimeGetWorkAreaSize",
        ult_runtime_work_area_size,
    ),
    ("_sceUltWaitingQueueResourcePoolCreate", ult_pool_create),
    ("_sceUltUlthreadRuntimeCreate", ult_runtime_create),
    ("_sceUltMutexLock", ult_mutex_lock),
    ("_sceUltMutexUnlock", ult_mutex_unlock),
    ("_sceUltMutexTryLock", ult_mutex_trylock),
    ("_sceUltMutexDestroy", ult_mutex_destroy),
    ("_sceUltConditionVariableCreate", ult_cond_create),
    ("_sceUltConditionVariableSignal", ult_cond_signal),
    ("_sceUltConditionVariableSignalAll", ult_cond_signal_all),
    ("_sceUltConditionVariableWait", ult_cond_wait),
    ("_sceUltConditionVariableDestroy", ult_cond_destroy),
    ("_sceUltUlthreadCreate", ult_ulthread_create),
    (
        "sceKernelAvailableFlexibleMemorySize",
        available_flexible_memory_size,
    ),
    (
        "sceKernelConfiguredFlexibleMemorySize",
        configured_flexible_memory_size,
    ),
    ("sceKernelMapFlexibleMemory", map_flexible_memory),
    ("sceKernelReleaseFlexibleMemory", release_flexible_memory),
    ("scePthreadAttrInit", pthread_attr_init),
    ("scePthreadAttrDestroy", pthread_attr_destroy),
    ("scePthreadAttrSetstacksize", pthread_attr_setstacksize),
    ("scePthreadAttrGetstacksize", pthread_attr_getstacksize),
    ("scePthreadAttrGet", pthread_attr_get),
    ("scePthreadAttrGetstackaddr", pthread_attr_getstackaddr),
    ("scePthreadAttrSetdetachstate", pthread_attr_setdetachstate),
    ("scePthreadAttrGetdetachstate", pthread_attr_getdetachstate),
    ("scePthreadAttrSetschedparam", pthread_attr_setschedparam),
    ("scePthreadAttrGetschedparam", pthread_attr_getschedparam),
    ("scePthreadSetaffinity", pthread_setaffinity),
    ("scePthreadGetaffinity", pthread_getaffinity),
    ("scePthreadAttrSetschedpolicy", pthread_attr_setschedpolicy),
    (
        "scePthreadAttrSetinheritsched",
        pthread_attr_setinheritsched,
    ),
    ("scePthreadAttrSetaffinity", pthread_attr_setaffinity),
    ("scePthreadAttrSetguardsize", pthread_attr_setguardsize),
    // Reported rather than answered; see each handler.
    (
        "sceKernelAprResolveFilepathsToIdsAndFileSizes",
        apr_resolve_filepaths,
    ),
    (
        "sceKernelAprSubmitCommandBufferAndGetResult",
        apr_submit_command_buffer,
    ),
    ("sceKernelAprWaitCommandBuffer", apr_wait_command_buffer),
    ("sceKernelReadTsc", read_tsc),
    ("sceKernelGetTscFrequency", get_tsc_frequency),
    ("sceKernelIsStack", is_stack),
    ("sceKernelGetModuleList", get_module_list),
    ("sceKernelLoadStartModule", load_start_module),
    ("sceKernelIsCex", is_cex),
    ("sceKernelGetModuleInfo", get_module_info),
    ("sceKernelIsDevkit", is_devkit),
    ("sceKernelIsNeoMode", is_neo_mode),
    ("sceKernelIsDevelopmentMode", is_development_mode),
    ("sceKernelIsTestKit", is_testkit),
    ("posix_getpagesize", getpagesize),
    ("posix_usleep", usleep),
    ("sceKernelUsleep", usleep),
    ("sceKernelClockGettime", kernel_clock_gettime),
    ("posix_sigemptyset", sigemptyset),
    ("posix_sigfillset", sigfillset),
    ("posix_sigaddset", sigaddset),
    ("posix_sigdelset", sigdelset),
    ("posix_sigismember", sigismember),
];

/// How much of a command buffer's storage to scan for content: eight kibibytes of words.
const STORAGE_SCAN_WORDS: u64 = 1024;

/// How many non-zero words to name.
///
/// Enough to see a twenty-byte command whole, and few enough to keep the report off the guest's
/// stack.
const MOST_STORAGE_REPORTED: usize = 16;

/// How far a guest path is followed when reporting one: longer than [`MAX_NAME`], because an
/// asset path's distinguishing part is at its end.
const MAX_PATH: usize = 256;

/// A NUL-terminated guest path of at most [`MAX_PATH`] bytes, lossily decoded; empty for null.
///
/// # Safety
///
/// `address` is under the `orbistoun_mem::guest` contract.
unsafe fn read_path(address: u64) -> String {
    // SAFETY: the caller's contract.
    let bytes = unsafe { guest::read_cstr(address, MAX_PATH) }.unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `sceKernelAprResolveFilepathsToIdsAndFileSizes(paths, count, ids, sizes, statuses, options)`.
///
/// Measured with signature-filled out-arrays (obSCEne `040-file/apr-resolve-filepaths`): `ids` is
/// `u32` per entry, `sizes` `u64`, `statuses` `u32`, and `options` may be null and is never
/// written. An unresolved path gets id `0xffffffff`, size 0 and status 0, and the call returns -1.
/// Each entry is written at its full width.
///
/// A path the title's own index names resolves to the index's identifier and size. Any other path
/// gets the unresolved answer, since nothing gives the id the hardware assigns a file outside an
/// index.
fn apr_resolve_filepaths(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (array, count, ids, sizes, statuses) = (args[0], args[1], args[2], args[3], args[4]);
    let mut paths = Vec::new();
    let mut all_resolved = true;
    for entry in 0..count {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let path = unsafe { guest::read_u64(array + entry * 8) }
            // SAFETY: each entry of the guest's path array is a path under the call's contract.
            .map(|path| unsafe { read_path(path) })
            .unwrap_or_default();
        let answer = apr::look_up(&path);
        let (id, size) = answer.map_or((u32::MAX, 0), |(id, size)| (id as u32, size));
        all_resolved &= answer.is_some();
        // Written per entry, at the measured widths.
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u32(ids + entry * 4, id) };
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u64(sizes + entry * 8, size) };
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u32(statuses + entry * 4, 0) };
        if entry < MOST_PATHS_REPORTED {
            match answer {
                Some((id, size)) => {
                    tracing::debug!("the index has {path} as entry {id}, {size} byte(s)");
                }
                None => {
                    tracing::debug!("the index does not name {path}; answered unresolved");
                }
            }
            paths.push(path);
        }
        // The first unresolved path ends the call and later slots keep what the caller put there, as
        // the `sceKernelAprResolveFilepathsToIdsAndFileSizes` record in `libkernel.toml` measures.
        if answer.is_none() {
            break;
        }
    }
    apr::note_resolved(paths);
    if all_resolved {
        OK
    } else {
        // `-1`, as the hardware answers for a path it could not resolve.
        u64::from(u32::MAX)
    }
}

/// How many paths one call reports, so a title resolving thousands does not flood the log.
const MOST_PATHS_REPORTED: u64 = 16;

/// `sceKernelAprSubmitCommandBufferAndGetResult(buffer, ...)`: reported, not answered.
///
/// Prints the command buffer's header. The platform exports `sceAmprCommandBufferGetSize`,
/// `GetNumCommands` and `GetCurrentOffset`, so the first three words are a size, a count and an
/// offset in an unknown order; they are printed unlabelled.
fn apr_submit_command_buffer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let buffer = args[0];
    let words: Vec<String> = (0..8)
        // SAFETY: an address the guest passed for this call, valid by its contract.
        .filter_map(|i| unsafe { guest::read_u64(buffer + i * 8) })
        .map(|w| format!("{w:#x}"))
        .collect();
    tracing::warn!(
        "the guest submitted an asynchronous file command buffer at {buffer:#x} - first words {}",
        words.join(" ")
    );
    // Where the non-zero words are. The header claims one command of twenty bytes and the storage
    // it names is empty, so the scan reports offsets rather than assuming a layout.
    let mut nonzero = Vec::new();
    for word in -8_i64..64 {
        let at = buffer.wrapping_add_signed(word * 8);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if let Some(value) = unsafe { guest::read_u64(at) } {
            if value != 0 {
                nonzero.push(format!("{:+#x}:{value:#x}", word * 8));
            }
        }
    }
    if !nonzero.is_empty() {
        tracing::debug!("non-zero words around it: {}", nonzero.join(" "));
    }
    // The delivery experiment, off unless asked for: what the buffer means is not established, so
    // delivering the resolved file into it is a guess, declared as intervening.
    if orbistoun_env::APR_DELIVER.is_set() {
        deliver_resolved_file(buffer);
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if let Some(inner) = unsafe { guest::read_u64(buffer + 0x10) } {
        if orbistoun_thunk::readable_span(inner, 32) {
            let head: Vec<String> = (0..8)
                // SAFETY: an address the guest passed for this call, valid by its contract.
                .filter_map(|i| unsafe { guest::read_u64(inner + i * 8) })
                .map(|w| format!("{w:#x}"))
                .collect();
            tracing::debug!("what it points at, {inner:#x}: {}", head.join(" "));
            // The whole storage, not its first words, so a command written at an offset is found.
            let mut found = Vec::new();
            for word in 0..(STORAGE_SCAN_WORDS) {
                // SAFETY: an address the guest passed for this call, valid by its contract.
                let Some(value) = (unsafe { guest::read_u64(inner + word * 8) }) else {
                    break;
                };
                if value != 0 {
                    found.push(format!("{:+#x}:{value:#x}", word * 8));
                    if found.len() >= MOST_STORAGE_REPORTED {
                        break;
                    }
                }
            }
            if found.is_empty() {
                tracing::debug!("and the first {STORAGE_SCAN_WORDS} words of it are all zero");
            } else {
                tracing::debug!("non-zero in it: {}", found.join(" "));
            }
        } else if let Some((start, end)) = region_containing(inner) {
            // Mapped, but not published to the dump: `region_containing` consults the live map, so it can
            // tell the two apart.
            tracing::debug!(
                "it points at {inner:#x}, inside a mapping of {start:#x}..{end:#x} that nothing published for reading"
            );
        } else {
            tracing::debug!(
                "it points at {inner:#x}, which this run never mapped - the guest is holding a buffer it was not given"
            );
        }
    }
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceKernelAprWaitCommandBuffer(...)`: reported, not answered.
///
/// The guest's own wrapper prints `waitCommandBufferCompletion error=%d` with whatever this
/// returns.
fn apr_wait_command_buffer(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    tracing::warn!("the guest waited on an asynchronous file command buffer");
    u64::from(GuestError::Unimplemented.as_raw())
}
/// Reads the last resolved file into the buffer a command header names.
///
/// The header's third and fourth words are a length and an address matching a guest mapping,
/// so they are taken as a buffer and its size; the file is the last one resolved, since the
/// command storage is empty. Both are guesses, and the run report carries the caveat. Separate
/// from the submit handler, which only observes.
fn deliver_resolved_file(buffer: u64) {
    let Some(path) = apr::last_resolved() else {
        tracing::warn!("asked to deliver a file, and no resolve named one");
        return;
    };
    // SAFETY: two words of the command header the guest submitted, valid by the call's contract.
    let most = unsafe { guest::read_u64(buffer + 0x0c) };
    // SAFETY: as above.
    let into = unsafe { guest::read_u64(buffer + 0x10) };
    let (Some(most), Some(into)) = (most, into) else {
        tracing::warn!("asked to deliver {path}, and the command header could not be read");
        return;
    };
    // The length shares a word with the count above it, so only the low half is the size.
    let most = most & 0xFFFF_FFFF;
    match apr::deliver(&path, into, most) {
        Some(got) => {
            tracing::info!("delivered {got} byte(s) of {path} into {into:#x} (up to {most:#x})");
        }
        None => {
            tracing::warn!("asked to deliver {path}, and nothing installed a reader for it");
        }
    }
}

#[cfg(test)]
mod tests {

    /// Makes a thread this crate has issued, for the calls that check a handle.
    fn a_thread(name: &str) -> u64 {
        super::thread::register(
            name,
            super::thread::Affinity(0),
            0,
            super::thread::AffinityPolicy::Observe,
            4,
        )
        .expect("the thread table accepted a registration")
    }

    fn args(values: [u64; 4]) -> [u64; GUEST_ARG_REGISTERS] {
        let mut out = [0; GUEST_ARG_REGISTERS];
        out[..4].copy_from_slice(&values);
        out
    }

    /// Serialises the tests that mutate the process-global noted-regions list
    /// ([`super::note_region`]/[`super::clear_noted_regions`]).
    ///
    /// Tests run in parallel in one process, so one test's `clear` could wipe another's `note`
    /// before its assertion. Poison is recovered so a panicking holder does not fail the other test.
    fn noted_regions_serial() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `scePthreadGetaffinity` reads back the mask a thread was recorded with; an unknown handle is
    /// "anywhere" (zero), not an error; a null out-parameter is refused.
    #[test]
    fn getaffinity_reads_back_the_recorded_mask() {
        let handle = super::thread::register(
            "affine",
            super::thread::Affinity(0b1010),
            0,
            super::thread::AffinityPolicy::Observe,
            4,
        )
        .expect("the thread table accepted a registration");
        let mut out = [0_u64; 1];
        let dst = out.as_mut_ptr() as u64;

        assert_eq!(
            super::pthread_getaffinity(&args([handle, dst, 0, 0])),
            super::OK
        );
        assert_eq!(
            out[0], 0b1010,
            "the recorded requested affinity is read back"
        );

        out[0] = 0x99;
        assert_eq!(
            super::pthread_getaffinity(&args([0xdead_beef, dst, 0, 0])),
            super::OK
        );
        assert_eq!(
            out[0], 0,
            "a handle with no record is anywhere, not an error"
        );

        assert_ne!(
            super::pthread_getaffinity(&args([handle, 0, 0, 0])),
            super::OK,
            "a null out-parameter is refused"
        );
    }

    /// A set stack size and affinity are honoured; a fresh or zero size is not.
    ///
    /// A set size passes through, as on the hardware; a fresh attribute's 64 KiB default and POSIX's
    /// `0` fall back to the 8 MiB stack. The affinity mask is carried to the thread record. The
    /// fresh-attribute case is the negative that a passthrough of every value would fail.
    #[test]
    fn a_set_stack_size_and_affinity_are_honoured_and_a_fresh_one_is_not() {
        use super::thread::Affinity;
        let default_stack = orbistoun_mem::stack::DEFAULT_STACK_SIZE;

        assert_eq!(
            super::spawn_parameters(0x0018_1000, 0x5),
            (Affinity(0x5), 0x0018_1000),
            "a size and affinity the guest set are passed through unchanged"
        );
        assert_eq!(
            super::spawn_parameters(super::DEFAULT_ATTR_STACK_SIZE, 0),
            (Affinity(0), default_stack),
            "a fresh attribute's 64 KiB default is not honoured over the 8 MiB stack"
        );
        assert_eq!(
            super::spawn_parameters(0, 0),
            (Affinity(0), default_stack),
            "a zero size falls back to the default rather than reserving nothing"
        );
    }

    /// A work-area size is something a caller can allocate.
    ///
    /// A size is a number the caller spends, so it must be spendable and proportional; the test pins
    /// those two properties, not a value the hardware needs.
    #[test]
    fn a_work_area_size_is_something_a_caller_can_spend() {
        for sizer in [
            super::ult_pool_work_area_size as fn(&[u64; GUEST_ARG_REGISTERS]) -> u64,
            super::ult_runtime_work_area_size,
        ] {
            let modest = sizer(&args([16, 16, 0, 0]));
            assert!(modest > 0, "zero would read as a failed sizing");
            assert!(
                modest < 0x10_0000,
                "sixteen threads asked for {modest:#x} bytes - a caller mallocs this"
            );
            assert_ne!(
                modest,
                u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
                "the placeholder is the exact value that cost 2 GiB"
            );
            // Proportional, and asking for nothing still yields a real block.
            assert!(
                sizer(&args([64, 64, 0, 0])) > modest,
                "more asked, more given"
            );
            assert!(
                sizer(&args([0, 0, 0, 0])) > 0,
                "a request for nothing still gets a block"
            );
        }
    }

    /// A constructed Ult object leaves a handle this crate issued in its first word, as
    /// `_sceUltMutexCreate` does, so a later call can check it (D524).
    #[test]
    fn constructing_an_ult_object_leaves_a_real_handle_in_it() {
        let mut object: u64 = 0;
        let at = std::ptr::from_mut(&mut object) as usize as u64;
        let name = b"waiting queue\0";
        let name_at = name.as_ptr() as usize as u64;

        assert_eq!(
            super::ult_pool_create(&args([at, name_at, 16, 16])),
            super::OK
        );
        assert_ne!(object, 0, "the object's first word was left empty");
        assert!(
            super::sync::ult_object_exists(object),
            "the guest was handed a value this crate never issued"
        );

        let named: Vec<String> = super::sync::ult_object_summary()
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert!(
            named.iter().any(|n| n == "waiting queue"),
            "the guest's own name for it was dropped: {named:?}"
        );
    }

    /// A null object is refused rather than written through.
    ///
    /// `write_word` also refuses a null destination, so removing only the explicit check does not
    /// fail this test; the explicit check is the call's own refusal.
    #[test]
    fn constructing_an_ult_object_into_nothing_is_refused() {
        assert_ne!(
            super::ult_runtime_create(&args([0, 0, 16, 3])),
            super::OK,
            "a null destination was accepted, which is a write through null"
        );
    }

    /// The two out-parameters are four bytes apart, and neither write clobbers the other.
    ///
    /// A `sched_param` is a single `int`, so a caller putting a policy and a param on its stack
    /// together puts them adjacent; an eight-byte write to either destroys the other (D272).
    #[test]
    fn adjacent_schedparam_out_parameters_do_not_overwrite_each_other() {
        let thread = a_thread("scheduled");
        // Two adjacent `int`s with a sentinel either side, laid out as the guest lays them out.
        let mut slots: [u32; 4] = [0xdead_beef, 0, 0, 0xfeed_face];
        let param = std::ptr::from_mut(&mut slots[1]) as usize as u64;
        let policy = param + 4;

        assert!(super::thread::set_scheduling(thread, Some(0x4000), 0x100));
        assert_eq!(
            super::pthread_getschedparam(&args([thread, policy, param, 0])),
            super::OK
        );

        assert_eq!(slots[1], 0x100, "the priority landed in the param slot");
        assert_eq!(
            slots[2], 0x4000,
            "and the policy in the one four bytes above"
        );
        assert_eq!(
            slots[0], 0xdead_beef,
            "a write ran backwards past the param"
        );
        assert_eq!(slots[3], 0xfeed_face, "a write ran past the policy");
    }

    /// A get hands back what a set was given, policy and priority both.
    #[test]
    fn scheduling_set_on_a_thread_is_what_comes_back() {
        let thread = a_thread("round-trip");
        let mut param: u32 = 0x2a;
        let param_at = std::ptr::from_mut(&mut param) as usize as u64;
        assert_eq!(
            super::pthread_setschedparam(&args([thread, 0x4000, param_at, 0])),
            super::OK
        );

        let record = super::thread::record(thread).expect("the thread is known");
        assert_eq!(record.requested_policy, 0x4000, "the policy was stored");
        assert_eq!(
            record.requested_priority, 0x2a,
            "and the priority read from *param"
        );
    }

    /// Setting a priority alone does not reset the policy.
    #[test]
    fn setting_a_priority_leaves_the_policy_alone() {
        let thread = a_thread("prio-only");
        assert!(super::thread::set_scheduling(thread, Some(0x4000), 1));
        assert_eq!(
            super::pthread_setprio(&args([thread, 0x200, 0, 0])),
            super::OK
        );

        let record = super::thread::record(thread).expect("the thread is known");
        assert_eq!(record.requested_priority, 0x200, "the priority moved");
        assert_eq!(record.requested_policy, 0x4000, "and the policy did not");
    }

    /// A handle this crate never issued is refused by every one of them.
    ///
    /// Handles are addresses, so accepting an arbitrary guest value would be a write through a bad
    /// pointer. Asserted across the whole set. `thread::rename` also refuses, so removing only the
    /// `is_issued` preamble from `pthread_rename` does not fail this test.
    #[test]
    fn none_of_the_scheduling_calls_believe_an_unissued_handle() {
        let bogus = 0xdead_beef_0bad_0bad_u64;
        let mut scratch: [u32; 4] = [0; 4];
        let at = std::ptr::from_mut(&mut scratch[0]) as usize as u64;
        for (name, answer) in [
            (
                "getschedparam",
                super::pthread_getschedparam(&args([bogus, at, at + 4, 0])),
            ),
            (
                "setschedparam",
                super::pthread_setschedparam(&args([bogus, 0, at, 0])),
            ),
            ("setprio", super::pthread_setprio(&args([bogus, 1, 0, 0]))),
            ("rename", super::pthread_rename(&args([bogus, at, 0, 0]))),
        ] {
            assert_ne!(answer, super::OK, "{name} accepted a handle nothing issued");
        }
        assert_eq!(scratch, [0; 4], "and none of them wrote through it");
    }

    /// Readability is answered live, and only for a range one region wholly covers.
    ///
    /// A region noted after the first question is found the next time, and a range that starts
    /// inside a region and runs off its end is not readable.
    #[test]
    fn readability_is_answered_from_the_tables_as_they_stand() {
        let _serial = noted_regions_serial();
        super::clear_noted_regions();
        let (base, len) = (0x4000_0060_0000_u64, 0x1_0000_u64);
        assert!(!super::is_guest_readable(base, 0x100), "nothing there yet");
        super::note_region(base, len);
        assert!(
            super::is_guest_readable(base, 0x100),
            "noted afterwards, answered now"
        );
        assert!(
            super::is_guest_readable(base + len - 4, 4),
            "the last word is inside"
        );
        assert!(
            !super::is_guest_readable(base + len - 4, 8),
            "running off the end is not covered"
        );
        assert!(
            !super::is_guest_readable(base, 0),
            "an empty range is not a read"
        );
        assert!(
            !super::is_guest_writable(base, 0x100),
            "a noted region carries no protection, so no write is vouched for"
        );
    }

    /// Hex to bytes, for the hardware's own records.
    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// The encoder reproduces obSCEne `020-memory/virtual-query-mapped` and `-stack` byte for byte.
    #[test]
    fn a_query_encodes_what_the_console_wrote() {
        let mapped = super::QueryRegion {
            start: 0x2_0085_0000,
            end: 0x2_0086_0000,
            offset: 0x2a2_0000,
            protection: 3,
            flags: super::vq::DIRECT | super::vq::COMMITTED,
            name: "anon",
        };
        let console = hex(concat!(
            "00008500020000000000860002000000",
            "0000a202000000000300000000000000",
            "12616e6f6e0000000000000000000000",
            "00000000000000000000000000000000",
            "0000000000000000"
        ));
        assert_eq!(mapped.encode().to_vec(), console);

        let stack = super::QueryRegion {
            start: 0x7_eedf_c000,
            end: 0x7_eeff_c000,
            offset: 0,
            protection: 3,
            flags: super::vq::FLEXIBLE | super::vq::STACK | super::vq::COMMITTED,
            name: "main stack",
        };
        let console = hex(concat!(
            "00c0dfee0700000000c0ffee07000000",
            "00000000000000000300000000000000",
            "156d61696e20737461636b0000000000",
            "00000000000000000000000000000000",
            "0000000000000000"
        ));
        assert_eq!(stack.encode().to_vec(), console);
    }

    /// An unresolved path fills each slot at the width obSCEne `040-file/apr-resolve-filepaths`
    /// measured.
    #[test]
    fn an_unresolved_path_fills_every_slot_at_its_measured_width() {
        let path = b"/app0/not-in-any-index.json\0";
        let pointers = [path.as_ptr() as usize as u64];
        let (mut ids, mut sizes, mut statuses) = ([0xa1_u8; 32], [0xb2_u8; 32], [0xc3_u8; 32]);
        let at = |b: &mut [u8; 32]| std::ptr::from_mut(&mut b[0]) as usize as u64;
        let answer = super::apr_resolve_filepaths(&[
            pointers.as_ptr() as usize as u64,
            1,
            at(&mut ids),
            at(&mut sizes),
            at(&mut statuses),
            0,
        ]);
        assert_eq!(answer, u64::from(u32::MAX), "-1, as the console answered");
        assert_eq!(&ids[..4], &[0xff; 4]);
        assert_eq!(&ids[4..], &[0xa1; 28]);
        assert_eq!(&sizes[..8], &[0; 8], "all eight bytes of the size");
        assert_eq!(&sizes[8..], &[0xb2; 24]);
        assert_eq!(&statuses[..4], &[0; 4]);
        assert_eq!(&statuses[4..], &[0xc3; 28]);
    }

    /// The first unresolved path ends the call, leaving later entries' slots untouched.
    #[test]
    fn an_unresolved_path_leaves_the_later_slots_untouched() {
        let (first, second) = (b"/app0/missing-one\0", b"/app0/missing-two\0");
        let pointers = [
            first.as_ptr() as usize as u64,
            second.as_ptr() as usize as u64,
        ];
        let (mut ids, mut sizes, mut statuses) = ([0xa1_u8; 8], [0xb2_u8; 16], [0xc3_u8; 8]);
        let at = |b: &mut [u8]| b.as_mut_ptr() as usize as u64;
        let answer = super::apr_resolve_filepaths(&[
            pointers.as_ptr() as usize as u64,
            2,
            at(&mut ids),
            at(&mut sizes),
            at(&mut statuses),
            0,
        ]);
        assert_eq!(answer, u64::from(u32::MAX));
        assert_eq!(&ids[..4], &[0xff; 4], "the first entry is answered");
        assert_eq!(&ids[4..], &[0xa1; 4], "the second is untouched");
        assert_eq!(&sizes[8..], &[0xb2; 8]);
        assert_eq!(&statuses[4..], &[0xc3; 4]);
    }

    /// The mapper answers what the hardware wrote: `rc 0`, the size quadword untouched, the 48
    /// bytes after it byte for byte, and a smaller declared size is never overrun.
    #[test]
    fn the_mapper_fills_what_the_console_filled() {
        let mut buffer = [0u8; 64];
        buffer[..8].copy_from_slice(&0x38u64.to_le_bytes());
        buffer[56..].copy_from_slice(&[0xee; 8]);
        let at = buffer.as_mut_ptr() as usize as u64;
        assert_eq!(super::mapper_get_param(&args([at, 0, 0, 0])), super::OK);
        let console = hex(concat!(
            "38000000000000000000000000080000",
            "00000000800800000000000080080000",
            "00000000800800004000000000000000",
            "6326000000000000"
        ));
        assert_eq!(&buffer[..56], console.as_slice());
        assert_eq!(&buffer[56..], &[0xee; 8], "nothing past the declared size");

        let mut small = [0u8; 24];
        small[..8].copy_from_slice(&0x18u64.to_le_bytes());
        let at = small.as_mut_ptr() as usize as u64;
        assert_eq!(super::mapper_get_param(&args([at, 0, 0, 0])), super::OK);
        assert_eq!(
            &small[8..],
            &console[8..24],
            "only what the declared size has room for"
        );
    }

    /// A fresh reservation queries as uncommitted and inaccessible, so a guest fills it.
    #[test]
    fn a_fresh_reservation_is_not_committed() {
        let mut slot: u64 = 0;
        let out = std::ptr::from_mut(&mut slot) as usize as u64;
        assert_eq!(
            super::reserve_virtual_range(&[out, 0x10_0000, 0, 0x4_0000, 0, 0]),
            super::OK
        );
        let region = super::query_region(slot).expect("the reservation is a region");
        let bytes = region.encode();
        assert_eq!(
            bytes[super::vq::FLAGS] & super::vq::COMMITTED,
            0,
            "not committed"
        );
        assert_eq!(
            &bytes[super::vq::PROTECTION..super::vq::PROTECTION + 4],
            &[0; 4]
        );
        assert_eq!(region.start, slot);
    }

    /// An address in no region is `EACCES`, the code the hardware answers (obSCEne
    /// `020-memory/virtual-query-unmapped`).
    #[test]
    fn a_query_of_nothing_is_refused_as_the_console_refuses_it() {
        let mut info = [0_u8; super::VIRTUAL_QUERY_INFO_BYTES];
        let at = std::ptr::from_mut(&mut info[0]) as usize as u64;
        assert_eq!(
            super::virtual_query(&args([0x0000_0000_0001_0000, 0, at, 72])),
            u64::from(super::GuestError::vendor(orbistoun_core::errno::DENIED).as_raw())
        );
    }

    /// A region the worker notes, the image or a stack, is found by [`super::region_containing`], so
    /// `sceKernelVirtualQuery` answers for the guest's own code and stack (D446).
    #[test]
    fn a_noted_region_is_found_and_a_gap_is_not() {
        let _serial = noted_regions_serial();
        super::clear_noted_regions();
        super::note_region(0x4000_0040_0000, 0x10_0000);
        // Inside, at the low edge, and one before the high edge: all held.
        assert_eq!(
            super::region_containing(0x4000_0045_0000),
            Some((0x4000_0040_0000, 0x4000_0050_0000))
        );
        assert!(
            super::region_containing(0x4000_0040_0000).is_some(),
            "the base is inside"
        );
        assert!(
            super::region_containing(0x4000_0050_0000 - 1).is_some(),
            "the last byte is inside"
        );
        // The end is exclusive, and an address in no region is not invented.
        assert!(
            super::region_containing(0x4000_0050_0000).is_none(),
            "the end is past it"
        );
        assert!(
            super::region_containing(0x1234_0000).is_none(),
            "an unnoted gap"
        );
        // Noting the same region twice does not duplicate or change the answer.
        super::note_region(0x4000_0040_0000, 0x10_0000);
        assert_eq!(
            super::region_containing(0x4000_0045_0000),
            Some((0x4000_0040_0000, 0x4000_0050_0000))
        );
        super::clear_noted_regions();
        assert!(
            super::region_containing(0x4000_0045_0000).is_none(),
            "cleared regions are gone"
        );
    }

    /// The resolver refuses rather than inventing an address (D366).
    ///
    /// A name nothing implements has no stub, and answering one would hand the guest something to
    /// call that is not what it asked for.
    #[test]
    fn a_name_nothing_implements_is_refused_and_nothing_is_written() {
        let name = c"sceKernelDefinitelyNotAFunction";
        let mut out = 0xDEAD_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 1;
        args[1] = name.as_ptr() as usize as u64;
        args[2] = std::ptr::addr_of_mut!(out) as usize as u64;

        assert_ne!(super::dlsym(&args), 0, "a failure, not a plausible address");
        assert_eq!(out, 0xDEAD, "and the caller's slot is left alone");
    }

    /// A null name or a null destination is refused before anything is read or written.
    #[test]
    fn the_resolver_refuses_a_null_name_or_a_null_destination() {
        let name = c"getpid";
        let mut with_no_name = [0_u64; GUEST_ARG_REGISTERS];
        with_no_name[2] = 8;
        assert_ne!(super::dlsym(&with_no_name), 0);

        let mut with_no_destination = [0_u64; GUEST_ARG_REGISTERS];
        with_no_destination[1] = name.as_ptr() as usize as u64;
        assert_ne!(super::dlsym(&with_no_destination), 0);
    }

    /// A name is reported once, however often it is asked for.
    #[test]
    fn a_name_is_reported_the_first_time_and_not_after() {
        let name = "sceKernelSomeNameOnlyThisTestUses";
        assert!(super::first_time_asked(name), "the first ask is news");
        assert!(!super::first_time_asked(name), "the second is not");
    }
    use super::{QUERY_INFO_SIZE, direct, implementations};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    #[test]
    fn allocating_main_memory_answers_an_address_the_guest_can_use() {
        // Physical offset zero is a real place in this pool, and the first allocation may land there,
        // so the address is not asserted non-zero.
        let mut first = u64::MAX;
        let mut second = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x10000;

        args[3] = std::ptr::addr_of_mut!(first) as usize as u64;
        assert_eq!(
            super::allocate_main_direct_memory(&args),
            0,
            "should succeed"
        );
        args[3] = std::ptr::addr_of_mut!(second) as usize as u64;
        assert_eq!(super::allocate_main_direct_memory(&args), 0, "and again");

        assert!(first < direct::DIRECT_MEMORY_SIZE, "inside the pool");
        assert!(second < direct::DIRECT_MEMORY_SIZE, "inside the pool");
        assert_ne!(
            first, second,
            "two allocations must not be handed the same memory"
        );
    }

    #[test]
    fn an_allocation_with_nowhere_to_report_the_address_is_refused() {
        // The address is the whole answer.
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x10000;
        assert_ne!(super::allocate_main_direct_memory(&args), 0);
    }

    #[test]
    fn a_zero_length_allocation_is_refused_rather_than_answered() {
        let mut physical = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_ne!(super::allocate_main_direct_memory(&args), 0);
        assert_eq!(physical, 0, "and nothing was invented");
    }

    #[test]
    fn a_requested_alignment_is_honoured() {
        // A stronger alignment is a hardware requirement.
        let mut physical = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x10000;
        args[1] = 1 << 21;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;

        assert_eq!(super::allocate_main_direct_memory(&args), 0);
        assert_eq!(physical % (1 << 21), 0, "the alignment was asked for");
    }

    #[test]
    fn an_impossible_alignment_is_refused_rather_than_rounded() {
        // A non-power-of-two alignment is refused; the check runs before widening to the pool's minimum.
        let mut physical = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x10000;
        args[1] = 3;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_ne!(super::allocate_main_direct_memory(&args), 0);
        assert_eq!(physical, u64::MAX, "and nothing was written back");
    }

    #[test]
    fn no_requested_alignment_means_no_preference_rather_than_an_error() {
        // Zero means no alignment requirement, the ordinary case.
        let mut physical = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x10000;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_eq!(super::allocate_main_direct_memory(&args), 0);
        assert_ne!(physical, u64::MAX, "an address was written");
    }

    #[test]
    fn mapping_direct_memory_hands_back_a_usable_address() {
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        // Allocate physical memory, then map it to reach it.
        let mut addr = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(addr) as usize as u64;
        args[1] = 0x10000;
        args[2] = 3; // read | write

        assert_eq!(super::map_named_direct_memory(&args), 0, "should map");
        assert_ne!(addr, 0, "and the guest must be told where");

        // Mapped for real, not just recorded: a bookkeeping entry without memory would fault on first
        // touch.
        // SAFETY: the address was just reserved read-write by this call, and the length
        // written is well inside the region asked for.
        unsafe { std::ptr::write_volatile(addr as usize as *mut u64, 0x1234) };
        // SAFETY: as above - reading back what was just written to a live reservation.
        let read_back = unsafe { std::ptr::read_volatile(addr as usize as *const u64) };
        assert_eq!(read_back, 0x1234);
    }

    /// Mappings that touch are one range; a gap or a read-only piece refuses a write across them.
    ///
    /// Two writable 64 KiB pieces back to back, then a read-only one: a write across the first two is
    /// allowed, into the third is not (a read is), and past the third, a gap, nothing is.
    #[test]
    fn adjacent_mappings_are_one_range_and_a_gap_or_a_read_only_piece_is_not() {
        let base = 0x6f00_0000_0000_u64;
        let piece = 0x1_0000_u64;
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        assert_eq!(
            super::map_direct_at(base, 0x7_0000_0000, piece, 3),
            super::OK
        );
        assert_eq!(
            super::map_direct_at(base + piece, 0x7_0001_0000, piece, 3),
            super::OK
        );
        assert_eq!(
            super::map_direct_at(base + 2 * piece, 0x7_0002_0000, piece, 1),
            super::OK
        );

        assert!(
            super::is_guest_writable(base + piece - 8, 16),
            "a write straddling two writable pieces"
        );
        assert!(
            super::is_guest_writable(base, 2 * piece),
            "both pieces whole"
        );
        assert!(
            !super::is_guest_writable(base + 2 * piece - 8, 16),
            "into the read-only piece"
        );
        assert!(
            super::is_guest_readable(base, 3 * piece),
            "all three are readable"
        );
        assert!(
            !super::is_guest_readable(base, 3 * piece + 8),
            "past the last piece is a gap"
        );
    }

    #[test]
    fn different_physical_ranges_never_share_an_address() {
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        // Reusing an address a guest still holds a pointer into would corrupt it.
        let (mut first, mut second) = (0_u64, 0_u64);
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0x10000;
        args[2] = 3;

        args[4] = 0x1_0000_0000;
        args[0] = std::ptr::addr_of_mut!(first) as usize as u64;
        assert_eq!(super::map_named_direct_memory(&args), 0);
        args[4] = 0x2_0000_0000;
        args[0] = std::ptr::addr_of_mut!(second) as usize as u64;
        assert_eq!(super::map_named_direct_memory(&args), 0);
        assert_ne!(first, second, "different memory, different addresses");
    }

    #[test]
    fn one_physical_range_always_maps_to_the_same_address() {
        // The aliasing property: a guest maps a range, loads data into it, and maps the same range again
        // expecting the data to be there.
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        let (mut first, mut again) = (0_u64, 0_u64);
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0x10000;
        args[2] = 3;
        args[4] = 0x9_0000_0000;

        args[0] = std::ptr::addr_of_mut!(first) as usize as u64;
        assert_eq!(super::map_named_direct_memory(&args), 0);

        // Write through the address the guest was given, then map the same physical range again.
        // SAFETY: the address was just mapped read-write by the call above.
        unsafe { std::ptr::write_volatile(first as usize as *mut u64, 0xFEED_FACE) };

        args[0] = std::ptr::addr_of_mut!(again) as usize as u64;
        assert_eq!(super::map_named_direct_memory(&args), 0);
        assert_eq!(again, first, "the same memory answers the same address");
        // SAFETY: as above - the same live mapping.
        let read_back = unsafe { std::ptr::read_volatile(again as usize as *const u64) };
        assert_eq!(read_back, 0xFEED_FACE, "and the data survived");
    }

    /// An alias does not outlive its memory or its mapping, and never answers a longer map.
    #[test]
    fn an_alias_is_forgotten_with_its_memory_and_never_answers_a_longer_map() {
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        let map = |physical: u64, len: u64| {
            let mut at = 0_u64;
            let mut args = [0_u64; GUEST_ARG_REGISTERS];
            args[0] = std::ptr::addr_of_mut!(at) as usize as u64;
            args[1] = len;
            args[2] = 3;
            args[4] = physical;
            assert_eq!(super::map_named_direct_memory(&args), 0, "maps");
            at
        };
        let writable_to = |base: u64, len: u64| {
            // SAFETY: the last word of a range the map above answered as mapped read-write.
            unsafe { std::ptr::write_volatile((base + len - 8) as usize as *mut u64, 1) };
        };

        let mut physical = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0x4_0000;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_eq!(super::allocate_main_direct_memory(&args), 0, "allocated");

        let short = map(physical, 0x1_0000);
        let long = map(physical, 0x4_0000);
        writable_to(long, 0x4_0000);
        assert_eq!(
            map(physical, 0x1_0000),
            long,
            "the longer one now covers it"
        );

        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = physical;
        args[1] = 0x4_0000;
        assert_eq!(super::release_direct_memory(&args), 0, "released");
        let after_release = map(physical, 0x1_0000);
        assert_ne!(
            after_release, long,
            "released memory is not the old mapping"
        );
        assert_ne!(after_release, short);

        args[0] = after_release;
        args[1] = 0x1_0000;
        assert_eq!(super::munmap(&args), 0);
        assert_ne!(
            map(physical, 0x1_0000),
            after_release,
            "an unmapped alias is not handed back"
        );
    }

    #[test]
    fn a_mapping_with_nowhere_to_report_the_address_is_refused() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0x10000;
        assert_ne!(super::map_named_direct_memory(&args), 0);
    }

    #[test]
    fn a_protection_naming_nothing_is_readable_rather_than_unreachable() {
        // A mapping the guest cannot touch is indistinguishable from a failed one.
        let none = super::protection_from_guest(0);
        assert!(none.read, "no request must not mean no access");
        assert!(!none.write);
    }

    #[test]
    fn the_posix_protection_bits_are_translated_in_the_right_order() {
        // Read 1, write 2, execute 4: the published POSIX values.
        let rw = super::protection_from_guest(3);
        assert!(rw.read && rw.write && !rw.execute);
        let rx = super::protection_from_guest(5);
        assert!(rx.read && !rx.write && rx.execute);
    }

    #[test]
    fn a_hostile_mapping_request_is_refused_rather_than_panicking() {
        direct::configure(direct::Settings {
            map_direct_memory: true,
            ..direct::Settings::default()
        });
        // Nothing reachable from a guest call may panic (D156). The all-ones word is what a caller
        // passes to mean "no preference", and rounding it up to an alignment overflows.
        let mut addr = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(addr) as usize as u64;
        args[1] = 0x10000;
        args[2] = 3;
        assert_ne!(
            super::map_named_direct_memory(&args),
            0,
            "refused, not crashed"
        );

        // And the same for a length and an alignment at the top of the range.
        let mut ok_addr = 0_u64;
        args[0] = std::ptr::addr_of_mut!(ok_addr) as usize as u64;
        args[1] = u64::MAX;
        assert_ne!(super::map_named_direct_memory(&args), 0);
        args[1] = 0x10000;
        args[5] = 1_u64 << 63;
        assert_ne!(super::map_named_direct_memory(&args), 0);
    }

    #[test]
    fn a_hostile_allocation_request_is_refused_rather_than_panicking() {
        // Rounding a length up to the pool alignment overflows too.
        let mut physical = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = u64::MAX;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_ne!(super::allocate_main_direct_memory(&args), 0);
        assert_eq!(physical, u64::MAX, "and nothing was written back");
    }

    #[test]
    fn asking_who_you_are_never_answers_no_thread() {
        // The process's first thread runs guest code without being created, and the guest still asks;
        // answering zero would make every unadopted thread compare equal.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        let me = super::pthread_self(&args);
        assert_ne!(me, super::thread::NO_THREAD);
        assert_eq!(super::pthread_self(&args), me, "and the answer is stable");
    }

    #[test]
    fn creating_a_thread_with_nowhere_to_put_the_handle_is_refused() {
        // Writing to address zero would fault inside the emulator rather than name the guest's mistake.
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[2] = 0x1000; // a plausible entry
        assert_ne!(super::pthread_create(&args), 0);
    }

    #[test]
    fn creating_a_thread_with_no_entry_point_is_refused() {
        let mut out = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(out) as usize as u64;
        assert_ne!(super::pthread_create(&args), 0, "nothing to run");
        assert_eq!(out, 0, "and no handle was invented");
    }

    #[test]
    fn a_lock_can_be_made_taken_and_released_through_the_guest_interface() {
        // The whole path: a handle written into guest memory, then read back on every later call.
        let mut slot = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(slot) as usize as u64;

        assert_eq!(super::pthread_mutex_init(&args), 0);
        assert_ne!(slot, 0, "a handle must have been written into guest memory");

        assert_eq!(super::pthread_mutex_lock(&args), 0);
        assert_eq!(super::pthread_mutex_unlock(&args), 0);
    }

    #[test]
    fn a_statically_initialised_lock_is_reported_rather_than_faked() {
        // A lock initialised at compile time names nothing this crate made; success would let every
        // thread into the critical section.
        let mut slot = 0x1234_5678_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(slot) as usize as u64;
        assert_ne!(super::pthread_mutex_lock(&args), 0, "not a lock we made");
    }

    #[test]
    fn joining_a_thread_that_does_not_exist_is_refused() {
        // Blocking forever would look identical to a guest deadlock.
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = u64::MAX;
        assert_ne!(super::pthread_join(&args), 0);
    }

    /// Set by guest code in the test below, to prove it really ran.
    static REACHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Called *from generated guest machine code*, under System V.
    extern "sysv64" fn mark(value: u64, _b: u64, _c: u64, _d: u64, _e: u64, _f: u64) -> u64 {
        REACHED.store(value, std::sync::atomic::Ordering::SeqCst);
        0
    }

    #[test]
    fn a_created_thread_really_executes_guest_code_and_can_be_joined() {
        // Generates real machine code, hands it to the guest interface, and passes only if that code
        // ran on a thread of its own.
        let marker = 0x00C0_FFEE_u64;
        let entry = mark as *const () as usize as u64;
        let code = orbistoun_abi::emit_call_with_six_args(entry, [marker, 0, 0, 0, 0, 0]);
        let buffer = orbistoun_abi::exec::ExecutableBuffer::new(&code).expect("map code");

        let mut handle = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(handle) as usize as u64;
        args[2] = buffer.address();

        assert_eq!(super::pthread_create(&args), 0, "the thread should start");
        assert_ne!(handle, 0, "and the guest should be given its handle");

        let mut join_args = [0_u64; GUEST_ARG_REGISTERS];
        join_args[0] = handle;
        assert_eq!(super::pthread_join(&join_args), 0, "and be joinable");

        assert_eq!(
            REACHED.load(std::sync::atomic::Ordering::SeqCst),
            marker,
            "the guest code must actually have run"
        );
    }

    /// Every name this crate's own `guest_module!` blocks declare: `libkernel`, and the
    /// wait-on-address library declared beside it under the name the guest imports it by.
    fn declared_here() -> Vec<&'static str> {
        [super::MODULE, super::sync_on_address::MODULE]
            .iter()
            .flat_map(|m| m.imports.iter().map(|i| i.name))
            .collect()
    }

    #[test]
    fn every_implementation_is_also_declared_here_or_says_why_not() {
        /// Implemented here and declared in another library, deliberately.
        ///
        /// Where a symbol is declared is a claim about the target; where its code lives is a claim about
        /// this repository (D367). These are declared where titles import them from, and implemented here
        /// beside the thread registry and `sync`.
        const DECLARED_ELSEWHERE: &[&str] = &[
            "pthread_detach",
            "pthread_exit",
            "mmap",
            // `std::call_once`'s engine: a libc symbol, implemented here because it runs a guest callback
            // through the thread registry's reentrant call.
            "_ZSt13_Execute_onceRSt9once_flagPFiPvS1_PS1_ES1_",
            // The C-runtime threading family: libc symbols, implemented here beside the thread registry
            // and `sync`.
            "_Mtx_init",
            "_Mtx_destroy",
            "_Mtx_lock",
            "_Mtx_unlock",
            "_Mtx_trylock",
            "_Cnd_init",
            "_Cnd_destroy",
            "_Cnd_wait",
            "_Cnd_timedwait",
            "_Cnd_signal",
            "_Cnd_broadcast",
            "_Xtime_get_ticks",
            "_Thrd_sleep",
            // libSceUlt mutexes: declared in the `ult` module, implemented here beside `sync`.
            "_sceUltMutexCreate",
            "_sceUltMutexLock",
            "_sceUltMutexUnlock",
            "_sceUltMutexTryLock",
            "_sceUltMutexDestroy",
            "_sceUltConditionVariableCreate",
            "_sceUltConditionVariableSignal",
            "_sceUltConditionVariableSignalAll",
            "_sceUltConditionVariableWait",
            "_sceUltConditionVariableDestroy",
            "_sceUltUlthreadCreate",
            // The runtime and pool setup: declared in the `ult` module, implemented beside the table that
            // holds them.
            "sceUltInitialize",
            "sceUltWaitingQueueResourcePoolGetWorkAreaSize",
            "sceUltUlthreadRuntimeGetWorkAreaSize",
            "_sceUltWaitingQueueResourcePoolCreate",
            "_sceUltUlthreadRuntimeCreate",
            // The POSIX spellings of calls declared here under vendor names, separate because the POSIX
            // signature is one argument shorter.
            "pthread_create",
            "pthread_cond_init",
            "pthread_mutex_init",
            // Thread-specific-data keys and POSIX unnamed semaphores, declared in the POSIX module and
            // implemented beside the thread registry and the vendor semaphore primitives (D453).
            "pthread_key_create",
            "pthread_setspecific",
            "pthread_getspecific",
            "pthread_key_delete",
            "sem_init",
            "sem_wait",
            "sem_trywait",
            "sem_post",
            "sem_destroy",
            // The timed acquisitions, with no vendor twins: declared in the POSIX module, implemented beside
            // the primitives whose deadlines they carry.
            "pthread_mutex_timedlock",
            "pthread_rwlock_timedrdlock",
            "pthread_rwlock_timedwrlock",
            "sem_timedwait",
            "sem_reltimedwait_np",
            "sem_getvalue",
            "pthread_cond_reltimedwait_np",
            // The POSIX timed wait and once-only initialiser: declared in the POSIX module, implemented
            // beside the condition variables and the C++ runtime once-flag.
            "pthread_cond_timedwait",
            "pthread_once",
            // The barrier and read-write lock attribute accessors, with no vendor twins: declared in the
            // POSIX module, implemented beside the locks they configure.
            "pthread_barrierattr_init",
            "pthread_barrierattr_destroy",
            "pthread_barrierattr_getpshared",
            "pthread_barrierattr_setpshared",
            "pthread_rwlockattr_init",
            "pthread_rwlockattr_destroy",
            "pthread_rwlockattr_getpshared",
            "pthread_rwlockattr_setpshared",
            "pthread_rwlockattr_gettype_np",
            "pthread_rwlockattr_settype_np",
            "pthread_yield",
            "sched_yield",
            "pthread_getconcurrency",
            "pthread_setconcurrency",
            // The attribute accessors and `pthread_equal`, with no vendor twins: declared in the POSIX
            // module, implemented beside the attribute object they read and write.
            "pthread_attr_getguardsize",
            "pthread_attr_getinheritsched",
            "pthread_attr_getschedpolicy",
            "pthread_attr_getscope",
            "pthread_attr_setscope",
            "pthread_mutexattr_getpshared",
            "pthread_mutexattr_setpshared",
            "pthread_mutexattr_getprioceiling",
            "pthread_mutexattr_setprioceiling",
            "pthread_condattr_getclock",
            "pthread_condattr_setclock",
            "pthread_condattr_getpshared",
            "pthread_condattr_setpshared",
            "pthread_condattr_destroy",
            "pthread_equal",
        ];

        // Resolution goes through the declared symbol list, so an implementation nobody declared can
        // never be reached.
        let declared = declared_here();
        for (name, _) in implementations() {
            assert!(
                declared.contains(name) || DECLARED_ELSEWHERE.contains(name),
                "{name} is implemented but not declared in guest_module!"
            );
        }
        for name in DECLARED_ELSEWHERE {
            assert!(
                !declared.contains(name),
                "{name} is declared here after all - remove it from the exceptions"
            );
        }
    }

    #[test]
    fn a_query_writes_the_region_into_guest_memory() {
        let mut info = [0_u64; 3];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[2] = info.as_mut_ptr() as usize as u64;
        args[3] = QUERY_INFO_SIZE;

        assert_eq!(super::direct_memory_query(&args), 0, "should succeed");
        assert_eq!(info[0], 0, "the first region starts at zero");
        assert!(info[1] > 0, "and ends somewhere");
    }

    #[test]
    fn a_query_with_no_destination_is_refused_rather_than_writing_to_null() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[3] = QUERY_INFO_SIZE;
        assert_ne!(super::direct_memory_query(&args), 0);
    }

    /// The software-version body is the configured display string then its packed integer, and it
    /// does not touch the size word.
    ///
    /// Pins the encoding: string from offset 8, packed integer at struct offset 0x24, size word
    /// (0..8) untouched, using the reference profile's `13.090.001` / `0x1309_0001`.
    #[test]
    fn the_software_version_body_is_the_configured_string_and_packed_int() {
        let version = orbistoun_core::machine::SoftwareVersion {
            display: "13.090.001".to_owned(),
            packed: 0x1309_0001,
        };
        let body = super::sw_version_body(&version);
        let string_end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
        assert_eq!(
            &body[..string_end],
            b"13.090.001",
            "the configured display string, written from offset 8"
        );
        let packed = u32::from_le_bytes(body[0x1c..0x20].try_into().unwrap());
        assert_eq!(
            packed, 0x1309_0001,
            "the packed integer at struct offset 0x24"
        );
    }

    /// A configured string longer than the field is truncated, not overrun into the integer.
    #[test]
    fn an_over_long_software_string_is_truncated_to_the_field() {
        let version = orbistoun_core::machine::SoftwareVersion {
            display: "x".repeat(64),
            packed: 0xAABB_CCDD,
        };
        let body = super::sw_version_body(&version);
        assert_eq!(&body[..0x1c], &[b'x'; 0x1c], "the string fills its field");
        let packed = u32::from_le_bytes(body[0x1c..0x20].try_into().unwrap());
        assert_eq!(
            packed, 0xAABB_CCDD,
            "and the integer that follows is intact"
        );
    }

    /// An unset software version refuses the call rather than inventing one, and a null destination
    /// refuses too.
    #[test]
    fn an_unset_or_null_software_version_refuses() {
        let version = orbistoun_core::machine::SoftwareVersion {
            display: "13.090.001".to_owned(),
            packed: 0x1309_0001,
        };
        assert!(
            super::sw_version_write(None, 0x1000).is_err(),
            "no configured version must refuse, not answer 12.40 or anything else"
        );
        assert!(
            super::sw_version_write(Some(&version), 0).is_err(),
            "a null destination is refused rather than written through"
        );
        assert!(
            super::sw_version_write(Some(&version), 0x1000).is_ok(),
            "a set version and a real destination succeed"
        );
    }

    /// A structure smaller than the whole one is accepted, and nothing past it is touched.
    ///
    /// The hardware accepts every declared size (D398). The guard word checks that the write stops
    /// where the caller said, which a return code alone would not.
    #[test]
    fn a_short_structure_is_accepted_and_not_overrun() {
        let mut info = [0_u64; 4];
        info[3] = GUARD;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[2] = info.as_mut_ptr() as usize as u64;
        args[3] = QUERY_INFO_SIZE - 1;
        assert_eq!(
            super::direct_memory_query(&args),
            0,
            "the console accepts a short buffer, so this must too"
        );
        assert_eq!(
            info[3], GUARD,
            "the write ran past what the caller declared"
        );
    }

    /// A flag the hardware refuses is refused here, with the code it uses: 0 and 1 are accepted, 2
    /// and 4 rejected (D398).
    #[test]
    fn an_undefined_query_flag_is_refused() {
        let mut info = [0_u64; 3];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 2;
        args[2] = info.as_mut_ptr() as usize as u64;
        args[3] = QUERY_INFO_SIZE;
        assert_eq!(
            super::direct_memory_query(&args),
            u64::from(orbistoun_core::GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
        );
    }

    /// A value no field of a real answer can hold, so an overrun is visible rather than lucky.
    const GUARD: u64 = 0xDEAD_BEEF_DEAD_BEEF;

    #[test]
    fn the_reported_size_matches_the_model_being_walked() {
        // Two answers about the same machine, which must agree or a guest sizes its heaps against memory
        // the walk never shows. Both read the `pool_bytes` setting.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(
            super::direct_memory_size(&args),
            direct::configured().pool_bytes
        );
    }

    /// The default pool is the retail figure, and the homebrew figure is a different measured value.
    ///
    /// On the hardware `sceKernelGetDirectMemorySize` answers twelve gibibytes to a retail title and
    /// five to a homebrew payload; retail is the default (D697).
    #[test]
    fn the_default_pool_is_the_retail_size_and_homebrew_is_a_distinct_measured_size() {
        assert_eq!(
            direct::Settings::default().pool_bytes,
            0x3_0000_0000,
            "the retail default is twelve gibibytes (REQ-...5d1c)"
        );
        assert_eq!(
            direct::HOMEBREW_DIRECT_MEMORY_SIZE,
            0x1_4000_0000,
            "the homebrew figure is five gibibytes (D398)"
        );
        assert!(
            direct::Settings::default().pool_bytes > direct::HOMEBREW_DIRECT_MEMORY_SIZE,
            "retail hands out more than homebrew - the whole reason PPSA04263's 8.5 GiB fits"
        );
    }

    #[test]
    fn mmap_allocates_valid_guest_memory() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 0; // addr = anywhere
        args[1] = 0x4000; // len = 16 KiB
        args[2] = 3; // prot = READ | WRITE
        args[3] = 0x1002; // flags = MAP_PRIVATE | MAP_ANON
        args[4] = !0; // fd = -1
        let addr = super::mmap(&args);
        assert_ne!(
            addr, !0,
            "mmap should succeed and return non-MAP_FAILED address"
        );
        assert_ne!(addr, 0, "mmap should return a non-null address");
    }

    #[test]
    fn mmap_rejects_zero_length() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = 0; // len = 0
        let addr = super::mmap(&args);
        assert_eq!(addr, !0, "mmap of length 0 must return MAP_FAILED");
    }

    #[test]
    fn a_thread_describes_its_own_stack_and_a_stranger_is_refused() {
        // The sequence a garbage collector runs to find the bottom of the stack it scans.
        super::note_stack_span(0x6000_0000_1000, 8 * 1024 * 1024);
        let span = super::STACK_SPAN.get().copied().expect("a main span");
        let me = super::thread::adopt("attr-get");
        let mut attr: u64 = 0;
        let attr_at = std::ptr::from_mut(&mut attr) as usize as u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = attr_at;
        assert_eq!(super::pthread_attr_init(&args), 0);

        args = [me, attr_at, 0, 0, 0, 0];
        assert_eq!(
            super::pthread_attr_get(&args),
            0,
            "the calling thread is describable"
        );

        let (mut address, mut size) = (1_u64, 1_u64);
        args = [
            attr_at,
            std::ptr::from_mut(&mut address) as usize as u64,
            0,
            0,
            0,
            0,
        ];
        assert_eq!(super::pthread_attr_getstackaddr(&args), 0);
        args = [
            attr_at,
            std::ptr::from_mut(&mut size) as usize as u64,
            0,
            0,
            0,
            0,
        ];
        assert_eq!(super::pthread_attr_getstacksize(&args), 0);
        assert_eq!(
            (address, size),
            span,
            "the span the worker placed, not a made-up one"
        );

        // A handle nobody registered is refused rather than given a stack that exists nowhere.
        args = [0xdead_0000, attr_at, 0, 0, 0, 0];
        assert_eq!(
            super::pthread_attr_get(&args),
            u64::from(super::GuestError::InvalidHandle.as_raw())
        );
    }

    /// One static for this whole module (D324): an instance declared inside a test would start its
    /// own cursor at zero and hand a neighbour the same address.
    static RANGE: orbistoun_mem::test_bases::Range =
        orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::KERNEL);

    #[test]
    fn a_range_is_covered_only_when_one_region_holds_all_of_it() {
        // Pure, so the rule is checked without a mapping. Both edges, because an off-by-one either way
        // refuses a guest's own module or lets a range run off the end of one.
        let region = (0x1_0000, 0x1_2000);
        assert!(
            super::range_within(0x1_0000, 0x2000, region),
            "an exact fit"
        );
        assert!(super::range_within(0x1_0800, 0x800, region), "inside");
        assert!(
            !super::range_within(0x1_0000, 0x2001, region),
            "one byte past the end is not covered"
        );
        assert!(
            !super::range_within(0x0_FFFF, 0x10, region),
            "starting before the region is not covered"
        );
        assert!(
            !super::range_within(0x1_0800, u64::MAX, region),
            "a length that would overflow must not wrap into looking contained"
        );
    }

    /// A guest may re-protect a region somebody else placed for it, its own module's pages, and a
    /// range covered by nothing is still refused (D577).
    #[test]
    fn a_noted_region_can_be_re_protected_and_a_gap_cannot() {
        let _serial = noted_regions_serial();
        let base = RANGE.take();
        let len = orbistoun_core::GUEST_PAGE_SIZE * 4;

        // Reserved through an address space of this test's own, so the global `mappings()` does not own
        // it: the region exists and this crate did not make it. Held for the whole test, because
        // dropping it releases the pages.
        let mut placed_elsewhere = orbistoun_mem::AddressSpace::new();
        placed_elsewhere
            .reserve(base, len, orbistoun_mem::Protection::READ_WRITE)
            .expect("a test range to stand in for a placed module");

        // Write, as a guest asks for it: the low bits are POSIX `PROT_*`.
        let args: [u64; GUEST_ARG_REGISTERS] = [base, len, 0x2, 0, 0, 0];

        super::clear_noted_regions();
        assert_ne!(
            super::mprotect(&args),
            super::OK,
            concat!(
                "a region nothing has reported is refused, which is the guard that stops a typo ",
                "re-protecting this process's own code"
            )
        );

        super::note_region(base, len);
        assert_eq!(
            super::mprotect(&args),
            super::OK,
            "and a region this process placed for the guest is its to re-protect"
        );

        // Past the end of the reported region: covered by nothing, so still refused.
        let past = [base, len + orbistoun_core::GUEST_PAGE_SIZE, 0x2, 0, 0, 0];
        assert_ne!(
            super::mprotect(&past),
            super::OK,
            "a range running off the end of the region is not covered by it"
        );
        super::clear_noted_regions();
    }

    /// A mapping the guest was given at its own address is not the arena.
    ///
    /// A guest that asks for `0x5000_0000_0000` is honoured there (D459); folding that into the arena
    /// would stretch it across terabytes nothing placed anything in.
    #[test]
    fn only_a_mapping_inside_the_arena_stretches_it() {
        assert_eq!(
            super::arena_end_of(super::MAPPING_BASE, 0x1000),
            Some(super::MAPPING_BASE + 0x1000),
            "the first arena mapping sets the extent"
        );
        assert_eq!(
            super::arena_end_of(0x5000_0000_0000, 0x1000),
            None,
            "an address the guest named for itself is outside the arena"
        );
        assert_eq!(
            super::arena_end_of(super::MAPPING_BASE - 1, 0x1000),
            None,
            "one byte below the base is below it"
        );
        assert_eq!(
            super::arena_end_of(super::MAPPING_BASE, u64::MAX),
            Some(u64::MAX),
            "a length that would overflow saturates rather than wrapping to a shorter arena"
        );
    }

    /// An arena nothing has used answers nothing, rather than a span of length zero, which the
    /// reporter already uses for an unused slot.
    #[test]
    fn an_unused_arena_is_nothing_rather_than_an_empty_span() {
        assert_eq!(super::arena_span(0), None, "nothing mapped");
        assert_eq!(
            super::arena_span(super::MAPPING_BASE),
            None,
            "the base alone is not a span"
        );
        assert_eq!(
            super::arena_span(super::MAPPING_BASE + 0x2_0000),
            Some((super::MAPPING_BASE, 0x2_0000)),
            "the span runs from the base to how far the guest reached"
        );
    }
}
