//! libkernel HLE - memory syscalls, threads, and synchronisation.
//!
//! Third in the dependency spine, after the container parser and the address
//! space. Nothing in the subsystem crates above is ever reached until a guest has
//! allocated memory and spawned threads through this layer, which is why it is
//! built before audio or video however tempting those look.
//!
//! # FreeBSD is the reference
//!
//! The target kernel is FreeBSD-derived and a large fraction of libkernel is
//! POSIX with vendor naming. Where a function has a documented BSD analogue, that
//! analogue is the specification - a lawful, citable reference, and the reason
//! this crate should need less guesswork than any other.
//!
//! Guest threads must be real host threads. A green-threaded or pooled
//! implementation cannot work: guest code reads thread-local storage directly and
//! blocks in its own primitives.
//!
//! # Status
//!
//! Mostly declarations. Arities are provisional and affect trace fidelity, not
//! correctness - see `orbistoun-hle::ImportDesc`.
//!
//! Direct memory is implemented ([`direct`]), because measurement said so: across four
//! commercial executables `sceKernelDirectMemoryQuery` is 99.9% of every call a guest
//! makes. Everything else here still reports honestly that it is not written.

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
/// A nested module for the reason [`thread`] is separate: one crate, more than one library, and each
/// `guest_module!` names its declaration `MODULE`. libSceUlt is a cooperative-threading (fibre)
/// library; its **mutexes** are the part a title reaches first - a JSON initialiser creates one named
/// `"ultmtx"` for thread-safety during static init (PPSA28061), and a stub answering the placeholder
/// stranded it. The fibre scheduler itself (`_sceUltUlthreadCreate`) is a larger subsystem still
/// unbuilt; the mutexes stand on their own because they are ordinary mutexes with a vendor name.
pub mod ult {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceUlt" {
            // (mutex, name, optParam): the SDK-documented shape. The internal spelling spills an SDK
            // version after it, which this does not read; three is what carries meaning (assumed).
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
            // (ulthread, name, entry, arg, context, sizeContext) in registers; a runtime and
            // optParam follow on the stack. Six is what is readable here, and only the first is used.
            "_sceUltUlthreadCreate" => 6,
            // Arity two, fixed by the run: the third register holds orbistoun's own placeholder,
            // left by an earlier stub (D516's evidence, D564).
            "sceUltWaitingQueueResourcePoolGetWorkAreaSize" => 2,
            "sceUltUlthreadRuntimeGetWorkAreaSize" => 2,
            // (object, name, count, count, workArea, optParam) - the shape both constructors
            // share, read off the guest's own calls.
            "_sceUltWaitingQueueResourcePoolCreate" => 6,
            "_sceUltUlthreadRuntimeCreate" => 6,
            // Three values whose meaning is unestablished; nothing here reads them.
            "sceUltInitialize" => 3,
        }
    }
}

/// The wait-on-address family, under the library name the guest imports it by.
///
/// The platform's kernel module exports more than one library, and a guest names the one it
/// wants: `libkernel_sync_on_address` sits beside `libkernel` and `libkernel_fs`, and a trace
/// labels the calls with it. The two names were proved by hash against the 12.40 export layout
/// this tree already carried, after a search of three and a half billion generated candidates
/// had missed them for a word-order guess (D572).
pub mod sync_on_address {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libkernel_sync_on_address" {
            // (address, value): the two registers PPSA25872 fills. Across the 48 calls the ring
            // held and the 90 it dumped, the next three read zero on every one and the sixth
            // held a leftover host address on some - so two is what carries meaning (D573).
            "sceKernelSyncOnAddressWait" => 2,
            // (address, count): a count of one in both captured calls, with the third and
            // fourth registers also one and the fifth 0x7ffffffe. Whether those are arguments
            // or leftovers is unestablished; nothing here reads them.
            "sceKernelSyncOnAddressWake" => 2,
        }
    }
}

guest_module! {
    "libkernel" {
        "sceKernelAllocateDirectMemory" => 6,
        // Three: a module handle, a name, and where to put the address. Measured from a
        // guest that called it, not assumed from the shape of the name (D365, D366).
        "sceKernelDlsym" => 3,
        // Four: a device, the request, its size, and whether to block. The request's own
        // layout is not published, and nothing here reads into it.
        "sceKernelSendNotificationRequest" => 4,
        "sceKernelReleaseDirectMemory" => 2,
        "sceKernelMapDirectMemory" => 6,
        // Seven arguments in truth; the seventh is a name this trampoline cannot reach,
        // which costs a label in a trace and nothing else.
        "sceKernelMapNamedDirectMemory" => 6,
        // Three: the entry array, how many entries, and where to write how many were mapped.
        // The console's batch direct-memory map; our own SDK's display init calls it with
        // sixteen 2 MiB entries to bring up the scanout surface (agc_display.c).
        "sceKernelBatchMap" => 3,
        // One: a size-prefixed out-structure. It refuses on retail (`0x8002_0006`) because the AGC
        // resource-registration subsystem it depends on is stubbed there (obSCEne 7b3c, D642/D643).
        "sceKernelMapperGetParam" => 1,
        // Four, for the four arguments the implementation reads. A dump shows six registers
        // because that is how many System V passes, not because the function takes six (D294).
        "sceKernelReserveVirtualRange" => 4,
        "sceKernelVirtualQuery" => 4,
        "sceKernelMprotect" => 3,
        // Two: the signal number and the handler. Both measured - an inverted call answers
        // EINVAL on hardware, and the guest's own handler compares its first argument to 30.
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
        // The calling thread's unique integer id (FreeBSD `pthread_getthreadid_np`), asked
        // 22.5k times in one PPSA21564 boot. Unimplemented it answered the placeholder, so
        // every thread believed it shared one id (D452).
        "scePthreadGetthreadid" => 0,
        // Named by the guest itself and confirmed by hash (D187). Two titles print
        // their own diagnostics naming these four, with the file and line they were
        // called from, once `printf` exists to carry the message.
        // Named from a *third* title's own bytes and confirmed by hash (D193). Its error
        // return is the whole of what aborted two titles during static initialisation.
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
        // The POSIX-signature barrier init, which takes no name and so cannot share the
        // vendor entry point (D385). Declared here beside `posix_pthread_rwlock_init`,
        // which is the same split for the same reason.
        "posix_pthread_barrier_init" => 3,
        "sceKernelCreateEventFlag" => 5, "sceKernelPollEventFlag" => 5,
        // Arity 2 for both, from the arguments PPSA02664 passes: the third register holds
        // `0x7fff0001` - orbistoun's own placeholder, left by an earlier stub - which is what
        // fixes the count rather than a guess (D516, D524).
        "sceKernelCreateEqueue" => 2, "sceKernelAddUserEventEdge" => 2,
        "sceKernelWaitEqueue" => 5,
        // Arities read off the guest's own calls: Get takes a handle and two adjacent
        // out-parameters, Set a handle, a policy and a param pointer, and the two-argument
        // pair a handle and one value (D561).
        "scePthreadGetschedparam" => 3, "scePthreadSetschedparam" => 3,
        "scePthreadSetprio" => 2, "scePthreadRename" => 2,
        "scePthreadCondattrDestroy" => 1, "pthread_setcancelstate" => 2,
        // `_sigprocmask` takes how, a set and an out-set; the guest passes 1, NULL and a stack
        // pointer. `sceKernelUuidCreate` takes only the destination (D562).
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
        // (thread, attr) and (attr, out): the shapes of FreeBSD pthread_attr_get_np and
        // pthread_attr_getstackaddr, and what three titles pass, once each, to find the stack
        // their collector scans (D575).
        "scePthreadAttrGet" => 2, "scePthreadAttrGetstackaddr" => 2,
        "scePthreadAttrSetdetachstate" => 2, "scePthreadAttrGetdetachstate" => 2,
        "scePthreadAttrSetschedparam" => 2, "scePthreadAttrGetschedparam" => 2,
        // The **thread** form, not the attribute form. Arity 2 for the same reason the
        // attribute form has it - a subject and a mask - and PPSA02664 passes a small
        // bitmask in the second register across 62 calls, with leftovers after it (D523).
        "scePthreadSetaffinity" => 2,
        "scePthreadGetaffinity" => 2,
        "scePthreadAttrSetschedpolicy" => 2, "scePthreadAttrSetinheritsched" => 2,
        "scePthreadAttrSetaffinity" => 2, "scePthreadAttrSetguardsize" => 2,
        // The platform's asynchronous file path, and the wall PPSA03416 dies behind. Unity's
        // `LocalFileSystemPS5` resolves paths to ids and sizes, builds a command buffer of
        // reads, submits it and waits - so a title opens the files it wants and reads nothing
        // through the descriptors, which is the "five opens, one read of zero bytes" D578 could
        // not explain (worklog 426).
        //
        // **Declared so the calls can be seen, not because they are implemented.** Each answers
        // the placeholder and says what it was asked; a stub the guest cannot tell from a
        // working implementation is what principle 3 forbids, and reporting is not answering.
        //
        // `sceKernelAprWaitCommandBuffer` was a bare hash - `0x23020f8e2805acae`, on
        // `symbols/wanted.txt` with 8,328 others. The guest's own wrapper prints
        // `waitCommandBufferCompletion`, its sibling wrapper `submitCommandBufferAndGetResult` is
        // the platform name with `sceKernelApr` prefixed and nothing else changed, and the same
        // transformation plus the obvious truncation hashes to the import exactly. Confirmed by
        // the hash agreeing and nothing else, which is the only confirmation there is (D587).
        //
        // Arity six throughout: the trampoline's full capture, which is not a claim about how
        // many arguments these take. The reasoning is `orbistoun-gpu`'s `agc` module in full
        // (D504) - a wrong arity degrades a trace, a wrong name is a shim nothing can reach.
        "sceKernelAprResolveFilepathsToIdsAndFileSizes" => 6,
        "sceKernelAprSubmitCommandBufferAndGetResult" => 6,
        "sceKernelAprWaitCommandBuffer" => 6,
        "sceKernelReadTsc" => 0,
        "sceKernelGetTscFrequency" => 0,
        "sceKernelIsStack" => 3,
        "sceKernelGetModuleList" => 3,
        "sceKernelLoadStartModule" => 6,
        // Refused rather than answered, because the structure it fills is not derivable -
        // but refused *honestly*, which a placeholder is not (D395).
        "sceKernelGetModuleInfo" => 2,
        "sceKernelIsCex" => 0,
        // **`Devkit`, not `DevKit`.** A NID is a hash of the name, so the two are different
        // symbols - and the guest imports the first. D271 answered this family correctly for
        // a spelling nothing ever asks for, so the real import kept landing on a stub whose
        // placeholder is non-zero, which reads as *true* (D393).
        "sceKernelIsDevkit" => 0,
        // Two more booleans of the same shape, imported by the conformance probe and
        // answering a placeholder until now.
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
        // Arity 2, from the arguments PPSA02664 passes: a clock identifier and a writable
        // stack address, with the third and fourth registers holding the same leftover
        // (D536).
        "sceKernelClockGettime" => 2,

    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// What the vendor's memory-query info structure holds, in order.
///
/// Three 64-bit fields: where the region starts, where it ends, and whether anything
/// has taken it. Written directly into guest memory, so the layout is the contract.
const QUERY_INFO_SIZE: u64 = 24;

/// `sceKernelDirectMemoryQuery(offset, flags, info, info_size)`.
///
/// Answers "what is at this physical offset, and what comes after it". A guest walks the
/// whole map by feeding back the end of each region it is shown.
///
/// **This is the function four commercial executables spend 99.9% of their calls on.**
/// Left unimplemented it returns an error, the walk never completes, and the guest asks
/// again forever - four hundred million times in ten seconds, in one measured case.
fn direct_memory_query(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (offset, flags, info, info_size) = (args[0], args[1], args[2], args[3]);

    if info == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    // **A small buffer is accepted, because the console accepts one.** This used to refuse
    // anything shorter than the structure, on the reasoning that a caller passing less wanted
    // a different layout. A conformance run swept the declared size from 1 to 256 on a target
    // console and every single one came back successful - so the refusal was this project's
    // idea, not the platform's, and a guest sizing its buffer some other way got an error here
    // that it would not have got there (D398).
    //
    // What is written is capped at what the caller declared. Whether the console truncates the
    // same way or writes the whole structure regardless is not established - the run recorded
    // the return code, not the damage - and of the two, overrunning a buffer the guest sized is
    // the one that cannot be undone.
    let room = info_size.min(QUERY_INFO_SIZE);

    // Flags beyond the two the console accepts. It answered 0 and 1, and refused 2 and 4 with
    // the invalid-argument code - so this is a measured boundary rather than a guess about
    // which bits mean something.
    if flags > 1 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    let Ok(guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let Some(region) = guard.query(offset) else {
        // Past the end of physical memory.
        //
        // **The structure is cleared, and that is the part that matters.** The guest
        // ignores the return value entirely - ten candidate error codes, spanning both
        // signs, changed nothing. What it reads is the buffer, so leaving the previous
        // answer in place made it advance to the same address forever. Clearing it made
        // the walk terminate and restart, which is how we know (D083).
        write_query_info(info, room, &[0, 0, 0]);
        // The console answers this one with the permission-denied errno, not the invalid
        // argument it uses for a bad flag - the two cases are distinguishable there, so they
        // are distinguishable here (D398).
        return u64::from(GuestError::vendor(orbistoun_core::errno::DENIED).as_raw());
    };

    // The third field carries the memory type, not whether the span is taken.
    //
    // **It was a boolean here and the console does not return a boolean.** A conformance run
    // read `3` from it for the region at the bottom of the map, which no `0` or `1` can be, so
    // the previous meaning was provably not the platform's (D083, D398).
    //
    // **Settled, and this model was right before it could be checked.** What `3` denoted - a
    // type, or some other state - stayed open until a capture did the one run that separates
    // them: allocate a page with each of `WB_ONION` (0), `WC_GARLIC` (3) and `WB_GARLIC` (10)
    // and read the field back for each. It answered `0x0`, `0x3` and `0xa` - the type asked
    // for, every time. Three distinct answers also rule out the other reading, that the field
    // is state and the type is somewhere else. Claimed by
    // `the_third_query_field_is_the_memory_type_the_allocation_asked_for` (D507).
    if marked_query_fields() {
        // **Dyed banknotes.** Each field carries a value that names itself, so whatever the
        // guest does next says which one it read - no watchpoint and no new machinery, only
        // different bytes. It is the standard black-box move and the cheapest thing on the
        // list, and it has already worked here by accident: the guest's next query offset
        // is the `end` value, which is how field 1 is known to be the one it walks by.
        // Nobody set out to learn that (D220).
        //
        // The low half of each field is kept real so the walk still advances and the guest
        // is not simply handed nonsense; the high half is the dye.
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
/// The low forty-eight bits, which covers every address in an eight-gigabyte range with
/// room to spare - so a marked walk still advances exactly as an unmarked one does. A dye
/// that broke the walk would answer a different question from the one being asked.
const MARK_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
/// Dye for the first field, in the bits the real value cannot reach.
const MARK_FIELD0: u64 = 0xAAAA_0000_0000_0000;
/// Dye for the second.
const MARK_FIELD1: u64 = 0xBBBB_0000_0000_0000;
/// Dye for the third, which carries no address and so is dyed whole.
const MARK_FIELD2: u64 = 0xCCCC_0000_0000_0000;

/// Whether the memory-query structure is being written with self-identifying values.
fn marked_query_fields() -> bool {
    MARKED_QUERY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set once during setup, read on every query - so an atomic rather than a lock.
static MARKED_QUERY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks for the memory-query structure to be written with marked values.
///
/// A diagnostic, not a setting: it answers "which field does the guest read?" once. See
/// `orbistoun_worker::experiment` for why those are separated (D220).
pub fn mark_query_fields(on: bool) {
    MARKED_QUERY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Writes a query result into guest memory.
///
/// Split out because both the success and the end-of-walk paths must write it. Only the
/// success path did, which left a guest that ignores return values reading the previous
/// answer forever.
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

/// `sceKernelGetSystemSwVersion(out)` - the version this *call* reports, which is not the
/// system firmware.
///
/// The structure is `{ size_t size; char version_string[0x1c]; uint32_t version; }`, 0x28 bytes.
/// Left unimplemented the call refused, which `130-layout/system-software-version` recorded;
/// hardware answers 0 and fills the struct.
///
/// # The number here is not the firmware, and it comes from the profile
///
/// The reference console runs system software **12.40** - what its `kern.version` banner says
/// (`releases/12.40`), what obSCEne's `sysinfo` header carries, and what syscall 649
/// ([`vendor_system_version`]) answers from `machine.firmware`. But `sceKernelGetSystemSwVersion`
/// is a *different* call reporting a *different* number: obSCEne measured `13.090.001` with the
/// packed integer `0x1309_0001`, the same across three module runs. So this reads
/// [`machine`]`().software_version` - a configured value like the firmware, not a constant (D420,
/// principle 5) - and an unset one refuses the call rather than inventing a version. Wiring it to
/// `machine.firmware` instead would make it answer 12.40, the reconciliation an earlier cut made
/// and hardware refutes.
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
/// unset-refusal and the byte layout are pinned without a guest buffer.
fn sw_version_write(
    version: Option<&orbistoun_core::machine::SoftwareVersion>,
    out: u64,
) -> Result<(usize, [u8; 0x20]), u64> {
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    let Some(version) = version else {
        // Unset - refuse, exactly as an unset firmware does, rather than answer a made-up version.
        return Err(vendor(orbistoun_core::errno::NO_ENTRY));
    };
    let dest = usize::try_from(out).map_err(|_| vendor(orbistoun_core::errno::INVALID))?;
    if dest == 0 {
        return Err(vendor(orbistoun_core::errno::INVALID));
    }
    Ok((dest, sw_version_body(version)))
}

/// The `0x20` bytes the call writes from offset 8: the display string, then the packed integer at
/// struct offset `0x24`. The size field at offset 0 is the caller's and is never touched - measured
/// by obSCEne's `130-layout/system-software-version` dump, where offsets 8..40 change and 0..8 stay
/// as the caller left them.
fn sw_version_body(version: &orbistoun_core::machine::SoftwareVersion) -> [u8; 0x20] {
    let mut body = [0u8; 0x20];
    // The structure's `version_string` is `char[0x1c]`; a longer configured string is truncated to
    // it rather than overrunning into the integer that follows.
    let text = version.display.as_bytes();
    let n = text.len().min(0x1c);
    body[..n].copy_from_slice(&text[..n]);
    body[0x1c..0x20].copy_from_slice(&version.packed.to_le_bytes());
    body
}

/// `sceKernelGetDirectMemorySize()`.
///
/// How much physical memory exists. Answered from the same setting the pool is built from, so the
/// two cannot describe different machines (D398) - and that setting is guest-dependent: twelve
/// gibibytes for a retail title, five for homebrew, both measured (`REQ-...5d1c`, D398).
fn direct_memory_size(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    direct::configured().pool_bytes
}

/// `std::_Execute_once(once_flag&, callback, context)` - runs a `call_once` initialiser once.
///
/// # The one call that has to run guest code back
///
/// `std::call_once` reaches this: it hands a once-flag, an `InitOnce`-shaped callback, and a
/// context, and expects the callback run exactly once. Stubbed, it answered a placeholder and the
/// callback never ran - so every `static` guarded by a `call_once`, which in a C++ program is most
/// of them, stayed uninitialised and the guest read a null out of it (PPSA25872, PPSA28061). This
/// runs the callback on a fresh stack through [`thread::call_guest`], the reentrant call this is the
/// first user of.
///
/// The flag is this implementation's to define, and it defines it minimally: the first word is `0`
/// before the initialiser has run and `1` after. A guest constructs a `once_flag` as zero, so an
/// unrun flag reads as not-run with no cooperation. Success follows the console's `InitOnce`
/// convention the callback is written to - non-zero is success, and only then is the flag marked
/// done, so a callback that fails is retried rather than recorded complete.
///
/// **Not yet serialised across threads.** Two threads racing the same fresh flag could both run the
/// initialiser; nothing measured does, and a per-flag guard is the fix when something does. Running
/// the callback is the fix that matters now - a placeholder that never ran it was the wall.
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
    // the caller's context, and somewhere to leave a context of its own - a slot for the last so a
    // callback that writes through it does not fault on a null.
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

// The C-runtime threading primitives the C++ standard library rests on - `_Mtx_*`, `_Cnd_*`,
// `_Xtime_get_ticks`, `_Thrd_sleep`. A guest built against this runtime does not call the POSIX
// `scePthreadMutex*` directly; `std::mutex`, `std::condition_variable` and `std::this_thread` lower
// onto these instead, so a title that uses any of them reaches here during static construction. The
// same family as [`execute_once`], and the reason it was the wall: stubbed, each answered the
// `Unimplemented` placeholder, and the standard library reads their return as a `_Thrd_result` and
// *throws* a non-success one - `_Throw_C_error(0x7fff0001)`, a placeholder turned into an exception
// the guest cannot unwind (the D125 shape, measured as the wall past `sceKernelVirtualQuery` on
// PPSA25872). Each maps onto the honest primitives already in [`sync`], so the mutual exclusion is
// real rather than a success-returning stub.

/// The `_Thrd_result` codes the standard library branches on, in the runtime's own order.
mod thrd {
    /// The call succeeded. `std::mutex` throws on anything else, so this is the value that matters.
    pub(crate) const SUCCESS: u64 = 0;
    /// The lock was held by another thread - the answer `try_lock` exists to give.
    pub(crate) const BUSY: u64 = 3;
    /// A timed wait reached its deadline without being signalled.
    pub(crate) const TIMEDOUT: u64 = 2;
    /// The handle named nothing this crate created - a real gap, kept distinct from success.
    pub(crate) const ERROR: u64 = 4;
}

/// Resolve the [`sync`] handle a `_Mtx_*`/`_Cnd_*` argument names, tolerant of the two shapes the
/// argument takes across runtimes.
///
/// The C-runtime spelling passes the handle *value* the matching `Init` stored; the C11 `mtx_t*`
/// spelling passes a *pointer to the storage* holding it. Rather than commit to one and mis-resolve
/// the other, this tries the argument as a handle first and, failing that, as a pointer to one -
/// and only ever returns a handle `exists` confirms this crate handed out, so a stale or constant
/// word can never be mistaken for a live object.
fn c_runtime_handle(arg: u64, exists: impl Fn(u64) -> bool) -> Option<u64> {
    if exists(arg) {
        return Some(arg);
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let inner = unsafe { guest::read_u64(arg) }?;
    exists(inner).then_some(inner)
}

/// `_Mtx_init(mtx, type)` - construct a mutex where the guest asked, and answer success.
///
/// The `type` word is read but not yet used to distinguish recursion: every std mutex is created
/// `Allowed` during bring-up, so a same-thread re-entry - undefined in a correct program and
/// therefore never relied on - cannot raise a false deadlock before the runtime's type bits are
/// measured the way the POSIX ones were (`015-sync/mutex-recursion`). The mutual exclusion between
/// *different* threads is real regardless, which is the property a std mutex is used for.
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

/// `_Mtx_destroy(mtx)` - release the object, matching `Init`. A word never initialised names
/// nothing, which is not an error to destroy.
fn c_mtx_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = c_runtime_handle(args[0], |h| sync::name_of(h).is_some()) {
        sync::destroy(handle);
    }
    thrd::SUCCESS
}

/// `_Mtx_lock(mtx)` - block until the mutex is held by this thread.
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

/// `_Mtx_unlock(mtx)` - release a lock this thread holds.
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

/// `_Mtx_trylock(mtx)` - take the mutex only if it is free, and *say so when it is not*. The one
/// that must never answer success on failure: the guest enters a critical section on success alone.
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

/// `_Cnd_init(cnd)` - construct a condition variable where the guest asked.
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

/// `_Cnd_destroy(cnd)` - release the object. An uninitialised word names nothing to release.
fn c_cnd_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(handle) = c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some()) {
        sync::cond_destroy(handle);
    }
    thrd::SUCCESS
}

/// Release the guest mutex around a wait and retake it after - shared by [`c_cnd_wait`] and
/// [`c_cnd_timedwait`], and carrying the same non-atomicity the POSIX pair records: the two objects
/// are independent here, so a signal landing in the gap is lost where the platform would hold it.
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

/// `_Cnd_wait(cnd, mtx)` - wait until signalled, holding the mutex again on return.
fn c_cnd_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_cnd_wait_inner(args[0], args[1], None) {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// `_Cnd_timedwait(cnd, mtx, xtime)` - wait until signalled or the absolute deadline passes.
///
/// The deadline is an `xtime{ sec, nsec }` in seconds and nanoseconds since the epoch; the wait is
/// bounded by how far it is ahead of now, so a deadline already past returns at once and a signal
/// still wins if it arrives first. Reports `TIMEDOUT` distinctly from a spurious wake so the guest's
/// predicate loop behaves.
fn c_cnd_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let deadline = read_xtime(args[2]);
    let timeout = deadline.map_or(Some(std::time::Duration::ZERO), duration_until);
    match c_cnd_wait_inner(args[0], args[1], timeout) {
        Some(true) => thrd::SUCCESS,
        Some(false) => thrd::TIMEDOUT,
        None => thrd::ERROR,
    }
}

/// `_Cnd_signal(cnd)` - wake one waiter.
fn c_cnd_signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some()).and_then(sync::cond_signal)
    {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// `_Cnd_broadcast(cnd)` - wake every waiter.
fn c_cnd_broadcast(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match c_runtime_handle(args[0], |h| sync::cond_name_of(h).is_some())
        .and_then(sync::cond_broadcast)
    {
        Some(_) => thrd::SUCCESS,
        None => thrd::ERROR,
    }
}

/// Read an `xtime{ sec: i64, nsec: i64 }` the runtime passes by pointer, as a duration since the
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
/// past. The host wall clock, read once - the same clock [`xtime_get_ticks`] answers from.
fn duration_until(target: std::time::Duration) -> Option<std::time::Duration> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Some(target.saturating_sub(now))
}

/// `_Xtime_get_ticks()` - the current time in 100-nanosecond ticks since the epoch, the unit the
/// runtime's `xtime` clock counts in. Answered from the host wall clock; a clock that cannot be read
/// answers zero rather than a fabricated time.
fn xtime_get_ticks(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos() / 100).unwrap_or(u64::MAX))
}

/// `_Thrd_sleep(duration, remaining)` - yield the thread for the requested span.
///
/// The span is read as a relative `{ sec, nsec }` and **clamped to one second** before sleeping: an
/// absolute-versus-relative mix-up in a runtime whose exact convention is not yet pinned would
/// otherwise turn a short retry-sleep into a multi-year hang, and a title that genuinely needs a
/// longer sleep than that does not exist among those measured. Always answers success.
fn thrd_sleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if let Some(span) = read_xtime(args[0]) {
        let capped = span.min(std::time::Duration::from_secs(1));
        std::thread::sleep(capped);
    }
    thrd::SUCCESS
}

// libSceUlt mutexes - ordinary named mutexes on the same `sync` primitives as the pthread and
// `_Mtx_*` families, one vendor library over. Declared in the `ult` module; the guest stores the
// handle in the first word of its `SceUltMutex`, exactly as the pthread pair does, so `mutex_at`
// resolves it the same way. Created `Allowed` for the reason the `_Mtx_*` family is (D431): a
// same-thread re-entry during single-threaded init cannot raise a false deadlock, and cross-thread
// exclusion is real regardless. Success is zero, the value the JSON initialiser that first needs this
// checks against.

/// How much memory an Ult object needs behind it, per thing it was sized for.
///
/// # orbistoun's own number, because it owns both ends
///
/// The guest asks `GetWorkAreaSize`, allocates that much, and hands the block to `Create`. Both
/// halves are implemented here, and **nothing is stored in the block** - an Ult object's state
/// lives in this crate's own table, the way every handle family here works. So the size is a
/// choice rather than a measurement, and it is chosen to be:
///
/// - **non-zero**, so a caller checking for a failed sizing sees success;
/// - **proportional to the request**, so a caller that sanity-checks "more threads, more memory"
///   is not surprised.
///
/// Unimplemented, `GetWorkAreaSize` answered the placeholder `0x7fff_0001` - and PPSA28061
/// `malloc`ed it. **Twice.** That is 2 GiB a piece, and this emulator's `malloc` served both
/// (D564).
const ULT_WORK_AREA_PER_OBJECT: u64 = 0x80;

/// A fixed part of the work area, so a request for nothing still gets a real block.
const ULT_WORK_AREA_HEADER: u64 = 0x100;

/// `sceUltWaitingQueueResourcePoolGetWorkAreaSize(threads, syncObjects)`.
///
/// **Arity two, fixed by the run**: the third register holds `0x7fff_0001`, orbistoun's own
/// placeholder left by an earlier stub - the same evidence that fixed `sceKernelCreateEqueue`
/// (D516) and `scePthreadSetaffinity` (D523). PPSA28061 asks for 16 and 16.
fn ult_pool_work_area_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ULT_WORK_AREA_HEADER
        + args[0]
            .saturating_add(args[1])
            .saturating_mul(ULT_WORK_AREA_PER_OBJECT)
}

/// `sceUltUlthreadRuntimeGetWorkAreaSize(threads, workerThreads)`.
///
/// Arity two on the same evidence. PPSA28061 asks for 16 threads and 3 workers.
fn ult_runtime_work_area_size(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ULT_WORK_AREA_HEADER
        + args[0]
            .saturating_add(args[1])
            .saturating_mul(ULT_WORK_AREA_PER_OBJECT)
}

/// `sceUltInitialize(...)` - bring the library up.
///
/// PPSA28061 passes `0x1c`, a pointer to a structure whose first word is `0x18`, and `1`. **What
/// any of them select is unestablished** and nothing here reads them: this reports that the
/// library is available, which is the only thing a caller can act on, and the arguments are
/// recorded in the knowledge file rather than guessed at (D564).
fn ult_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `_sceUltWaitingQueueResourcePoolCreate(pool, name, threads, syncObjects, workArea, optParam)`.
///
/// Constructs a named pool where the guest asked. **The handle goes in the object's first word**,
/// which is this library's own convention here - the same one `_sceUltMutexCreate` follows, and
/// the guest reads it back from there.
///
/// The work area is accepted and **not written to**; see [`ULT_WORK_AREA_PER_OBJECT`].
fn ult_pool_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ult_construct(args)
}

/// `_sceUltUlthreadRuntimeCreate(runtime, name, threads, workerThreads, workArea, optParam)`.
///
/// The same shape as the pool, and PPSA28061 builds one called `"sample runtime"` immediately
/// after a `"waiting queue"`.
///
/// **This is not the fibre scheduler.** Constructing a runtime is not running threads on it -
/// `_sceUltUlthreadCreate` remains unbuilt, and a guest that gets this far and then creates a
/// fibre will stop there instead. That is a better place to stop than a 2 GiB allocation.
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

/// `_sceUltMutexCreate(mutex, name, optParam)` - construct a named mutex where the guest asked.
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

/// `_sceUltMutexLock(mutex)` - block until the mutex is held by this thread.
fn ult_mutex_lock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::acquire(handle, thread::adopt("main"), sync::Blocking::Forever) {
        Some(sync::Acquisition::Locked) => OK,
        _ => u64::from(GuestError::InvalidArgument.as_raw()),
    }
}

/// `_sceUltMutexUnlock(mutex)` - release a lock this thread holds.
fn ult_mutex_unlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::unlock(handle, thread::adopt("main")) {
        Some(true) => OK,
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NOT_OWNER).as_raw()),
    }
}

/// `_sceUltMutexTryLock(mutex)` - take the mutex only if free, and say so when it is not.
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

/// `_sceUltMutexDestroy(mutex)` - release the object and clear the guest's handle. An uninitialised
/// word names nothing to release, which is not an error.
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
/// libSceUlt binds the mutex when the variable is *created*, and its `Wait` takes only the variable -
/// unlike the POSIX and C-runtime pairs, whose wait is handed the mutex every time. This remembers the
/// binding so `Wait` can release and retake the right lock.
fn ult_cond_mutex() -> &'static Mutex<std::collections::BTreeMap<sync::CondHandle, u64>> {
    static MAP: OnceLock<Mutex<std::collections::BTreeMap<sync::CondHandle, u64>>> =
        OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// `_sceUltConditionVariableCreate(cv, name, mutex, optParam)` - construct a condition variable bound
/// to a mutex, where the guest asked.
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

/// `_sceUltConditionVariableSignal(cv)` - wake one waiter.
fn ult_cond_signal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_signal) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltConditionVariableSignalAll(cv)` - wake every waiter.
fn ult_cond_signal_all(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match cond_at(args[0]).and_then(sync::cond_broadcast) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `_sceUltConditionVariableWait(cv)` - wait until signalled, releasing the bound mutex around the
/// wait and retaking it after, the same non-atomicity the POSIX pair records.
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

/// `_sceUltConditionVariableDestroy(cv)` - release the object and clear the guest's handle.
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
/// A monotonic counter, distinct and non-zero, because the threads are created but not yet run (no
/// cooperative scheduler is built), so nothing dereferences the handle.
fn next_ult_thread() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// `_sceUltUlthreadCreate(ulthread, name, entry, arg, context, sizeContext, ...)` - create a
/// cooperative thread.
///
/// **Created, not yet run.** A libSceUlt thread is cooperative: it does not run until the running
/// thread yields it to the scheduler, and no scheduler is built here yet - so this records the thread
/// as created and answers success without running its entry. That is honest for the state a title
/// checks at creation (the JSON initialiser creates a worker and continues without yielding, so it
/// proceeds); a title that then *yields* expecting the thread to run reaches a gap this leaves named
/// rather than a fault. Running the entry synchronously instead would hang on the first blocking wait
/// a worker makes, which is why it is not done - the scheduler is the honest fix, and the next step.
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
/// Reserves from the main pool without the caller choosing a physical address, and writes
/// the address it chose where the caller asked.
///
/// # Why this one, and how the arguments were established
///
/// The ordered call tail put it two calls before a `memset` through a null pointer, twice
/// (D154):
///
/// ```text
/// sceKernelAllocateMainDirectMemory(0x1fe0000)
/// printf(...)
/// memset(0x0) x3
/// ```
///
/// A guest asking for memory, being refused, printing something, and then clearing a
/// buffer it never got. The first argument is a length in both observed calls -
/// `0x1fe0000` and `0x10000`, both plausible sizes and both multiples of the allocation
/// alignment - which fixes the argument order against the POSIX-shaped signature the
/// vendor name implies.
///
/// The remaining arguments follow the FreeBSD-analogous shape: an alignment, a memory
/// type, and a destination for the physical address. **The alignment is honoured and the
/// type is recorded**; neither has been separately verified, and a guest that depends on
/// a specific alignment would fail here in a way nothing yet distinguishes.
fn allocate_main_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (len, alignment, memory_type, out) = (args[0], args[1], args[2], args[3]);

    // Refused rather than answered with a made-up address. A caller asking for nothing,
    // or with nowhere to be told the answer, is a caller whose expectations this cannot
    // meet.
    if len == 0 || out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    let Ok(memory_type) = u32::try_from(memory_type) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Ok(mut guard) = direct::map().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };

    // Validated as the caller passed it, *before* being widened to the pool's own
    // minimum. Widening first makes every nonsense value look like a power of two and the
    // check unreachable - which it was, until a test said so.
    //
    // Zero means "no preference", which is the ordinary case and not an error.
    if alignment != 0 && !alignment.is_power_of_two() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // Never weaker than the pool's own: a guest asking for a larger alignment is asking
    // because its hardware needs it.
    let align = alignment.max(direct::DIRECT_ALIGN);
    let Some(address) = guard.allocate_aligned(len, align, memory_type) else {
        // Out of memory is a real answer and distinct from not being written: a guest
        // that gets this can shrink its request, and one that gets `Unimplemented`
        // cannot tell the difference between a full pool and a missing function.
        //
        // **Said out loud, with the numbers that decided it.** The guest gets one error code
        // for a full pool, a fragmented one and an alignment nothing can place, and the three
        // want different fixes - so the branch that knows which it was is the branch that has
        // to say so (CLAUDE.md principle 3). PPSA04263 is why: it asks for one span, is
        // refused, and faults on the next instruction, and nothing in the run said whether the
        // pool was short or the alignment was impossible.
        eprintln!(
            concat!(
                "orbistoun: sceKernelAllocateMainDirectMemory refused {:#x} at alignment {:#x} ",
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
/// A pool a guest has fragmented holds thousands of regions, and a refusal that printed all of
/// them would bury the three numbers that matter in its own output. Seven is what PPSA04263's
/// refusal has - enough that the interesting case prints whole.
const REFUSAL_REGIONS: usize = 12;

/// How many times a placement steps past a conflict before giving up.
///
/// The arena counter steps an address rather than the map, so a base it hands back can already
/// be held - by a range the guest reserved at a hint inside the arena. Sixteen is what
/// `sceKernelReserveVirtualRange` has always used, and naming it once stops the two paths
/// drifting apart again (D604).
const CONFLICT_RETRIES: usize = 16;

/// Where guest-requested mappings are placed when the guest expresses no preference.
///
/// Clear of the image, the stacks, the thunk table **and the thunk data blocks**, so a stray
/// pointer into any of them is still recognisable by its address alone.
///
/// **It was `0x7200…`, which is `orbistoun_thunk::SUGGESTED_DATA_BASE` exactly** - the data
/// blocks are reserved there at load, so the first guest map that expressed no preference
/// landed on top of them: `VirtualAlloc` refused the address, `reserve` reported a conflict,
/// and `map` answered `NoMemory`, which the guest read as out-of-memory and faulted through
/// the null it kept (PPSA04263, `image+0x2ba64c1`). A title that reserves with a hint first
/// (PPSA02664) never reached this base and so never hit it. Moved a clear terabyte above the
/// data blocks; the two arenas each keep multiple terabytes of room below the `0x7FFF…`
/// user-space ceiling (worklog 285).
pub const MAPPING_BASE: u64 = 0x0000_7400_0000_0000;

/// The address space guest mappings live in.
///
/// Separate from the loader's: this holds what the *guest* asked for at runtime, and
/// keeping the two apart means a mapping bug cannot silently overwrite a segment of the
/// image.
fn mappings() -> &'static Mutex<orbistoun_mem::AddressSpace> {
    static SPACE: OnceLock<Mutex<orbistoun_mem::AddressSpace>> = OnceLock::new();
    SPACE.get_or_init(|| Mutex::new(orbistoun_mem::AddressSpace::new()))
}

/// Regions the guest can read that this crate did not map itself - the loaded image, the guest
/// stack, the main-thread TLS block. They live in address spaces the loader and worker own, so
/// [`mappings`] never sees them; `virtual_query` consults this alongside it, because a guest
/// asking about its own code or stack expects a mapping, not "nothing here" (D446).
fn noted_regions() -> &'static Mutex<Vec<(u64, u64)>> {
    static NOTED: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    NOTED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records a `[base, base + len)` region the guest can read but this crate did not map, so
/// `virtual_query` answers for it. Stored as `(start, end)`; a region already noted is not
/// duplicated, so the worker may call this on every run without the list growing without bound.
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

/// Forgets every noted region. For tests, so one does not query another's regions.
#[cfg(test)]
pub fn clear_noted_regions() {
    if let Ok(mut noted) = noted_regions().lock() {
        noted.clear();
    }
}

/// The `[start, end)` of the region containing `addr`, or `None` when nothing maps it.
///
/// Consults every place a guest-readable region is recorded: the runtime mappings this crate
/// hands out, the regions the worker noted (the image, the TLS block), and the stacks - this
/// thread's own if it has one, else the main stack span. A guest querying any address it can
/// legitimately touch is then answered, which is what `sceKernelVirtualQuery` and `is_stack`
/// need and what `mappings` alone could not give (D446).
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
/// Pure, so the containment rule is testable without any mapping existing - the shape
/// principle 8 asks for, and the same split [`region_containing`] already has from the tables
/// it reads. Saturating throughout: a guest is free to ask for a length that overflows, and an
/// overflow that wrapped would report a range as contained by a region it dwarfs.
const fn range_within(base: u64, len: u64, region: (u64, u64)) -> bool {
    let (start, end) = region;
    base >= start && base.saturating_add(len) <= end
}
/// The region wholly covering `[base, base + len)`, from every place one is recorded.
///
/// [`region_containing`] answers for a single address; a protection change is about a span, and
/// a span that begins in a placed region and runs off its end is not one that region covers.
/// **One region, not the union of several**: a range crossing from one into another spans the
/// gap between them, and the gap is address space nothing placed.
fn region_covering(base: u64, len: u64) -> Option<(u64, u64)> {
    let region = region_containing(base)?;
    range_within(base, len, region).then_some(region)
}

/// Whether `[base, base + len)` is guest memory the guest can read **right now** - one region wholly
/// covering it, from every place a region is recorded, and a runtime mapping only if its protection
/// allows reads.
///
/// **Live, not a snapshot.** The graphics submit path used to read a region list the worker took
/// before entering the guest, so a command buffer the guest built in direct memory it mapped later -
/// every GL context's, which allocates after entry - fell outside every region and walked to zero
/// packets (worklog 814). This answers from the tables as they stand when it is asked (worklog 815).
#[must_use]
pub fn is_guest_readable(base: u64, len: u64) -> bool {
    guest_range_allows(base, len, false)
}

/// Whether `[base, base + len)` is guest memory the guest can **write** right now - a runtime
/// mapping wholly covering it whose protection allows writes.
///
/// Narrower than [`is_guest_readable`] on purpose: the noted regions (the image, TLS) and the stacks
/// carry no protection here, and the image's text is not writable, so a write is vouched for only
/// where the mapping itself says so. The graphics command processor writes guest memory through this -
/// a fill, a copy, a fence (worklog 816) - and a write into a read-only page would fault the host on
/// the guest's behalf.
#[must_use]
pub fn is_guest_writable(base: u64, len: u64) -> bool {
    guest_range_allows(base, len, true)
}

/// **Adjacent runtime mappings count as one range.** A guest that batch-maps a surface in 2 MiB
/// pieces - the SDK's scanout buffers are sixteen of them end to end - holds one continuous span, and
/// an 8 MiB fill of a 1080p target crosses four; requiring a single region refused it (worklog 816).
/// Only regions that *touch* are joined, each must grant the access, and a gap anywhere refuses the
/// whole range, which is the rule `region_covering` protects (D446).
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
/// Bump-allocated and never reused. A guest that unmaps and remaps would otherwise be
/// handed an address it still holds a stale pointer to, and the resulting corruption
/// would look like anything except a mapping bug.
fn next_mapping_base(len: u64) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(MAPPING_BASE);
    // Stepped by the *host's* reservation granularity, not the guest page size. Those
    // are the same on Unix and differ on Windows, which rounds a reservation base down to
    // 64 KiB - so a guest-page-aligned base comes back at a different address and the
    // address space correctly refuses it. Chosen addresses have to satisfy the host.
    let unit = orbistoun_mem::allocation_granularity().max(orbistoun_core::GUEST_PAGE_SIZE);
    // Padded by one unit beyond the request so two mappings never share one. Saturating
    // rather than panicking, for the reason `checked_next_multiple_of` exists.
    let step = len
        .checked_next_multiple_of(unit)
        .unwrap_or(u64::MAX)
        .saturating_add(unit);
    NEXT.fetch_add(step, Ordering::Relaxed)
}

/// Virtual addresses already handed out, as `(address, len)` by the physical offset mapped.
///
/// A guest maps a physical range, fills it, and maps the same range again expecting its data
/// back, so a second map of the same physical memory returns the first address (D174). Two
/// simultaneous mappings of one range share one address; distinct addresses would need a shared
/// memory object. An entry holds only while both the physical memory and the mapping exist, and
/// only for maps no longer than the one it records - a longer map is a different mapping.
fn physical_mappings() -> &'static Mutex<std::collections::BTreeMap<u64, (u64, u64)>> {
    static MAPPED: OnceLock<Mutex<std::collections::BTreeMap<u64, (u64, u64)>>> = OnceLock::new();
    MAPPED.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Forgets every alias whose physical offset lies in `[start, start + len)` - the memory was
/// released, so a later allocation of it is new memory with nothing to keep.
fn forget_physical(start: u64, len: u64) {
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.retain(|&physical, _| physical < start || physical - start >= len);
    }
}

/// Forgets every alias whose mapping starts in `[address, address + len)` - the guest unmapped it.
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
/// A reservation is address space, not memory, and a guest queries which it has. orbistoun
/// reserves and backs in one step, so a range here that no direct mapping has been placed in is
/// answered uncommitted and inaccessible - a guest fills a reservation only when it reads as empty.
fn reservations() -> &'static Mutex<Vec<(u64, u64)>> {
    static RESERVED: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    RESERVED.get_or_init(|| Mutex::new(Vec::new()))
}

/// `sceKernelMapNamedDirectMemory(addr, len, prot, flags, physical, alignment)` - gives the guest
/// a virtual address for physical memory `sceKernelAllocateMainDirectMemory` reserved (D155). The
/// name is the seventh argument and the trampoline spills six, so it is not read.
fn map_named_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // **Every way out that is not success is recorded, from one place.** The refusal note started
    // on the one failure path anybody had looked at, and this function has a dozen - so a map the
    // guest asked for and did not get still looked identical to one it never asked for, which is
    // the exact question the record was extended to answer (D602, D603).
    // **The address the guest asked for, not the argument that points at it.** `args[0]` is the
    // `void **` the answer is written back through - a stack address - and recording it as a base
    // put a guest stack pointer in a list of mappings. Read through it for what the guest
    // actually requested, which is zero when it expressed no preference (D604).
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

/// The body, so the refusal note above has one place to sit.
fn map_named_direct_memory_inner(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // On by default. The switch remains because turning a subsystem off is a useful thing
    // to be able to do while bisecting - it was off for one afternoon while a fault inside
    // this function went unexplained, and the cause turned out to be elsewhere (D178).
    if !direct::configured().map_direct_memory {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let (out, len, prot, physical, alignment) = (args[0], args[1], args[2], args[4], args[5]);

    // Already mapped, and covering what is asked for: the guest gets the address it had, and its
    // data with it. A longer map is a different mapping - answering it with the shorter one would
    // leave the rest of what the guest was promised unmapped.
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

    // The guest may name an address it wants by leaving it in the destination; zero is
    // "anywhere". Honoured rather than overridden - a guest that asked for an address and
    // silently got a different one corrupts itself in ways that look like anything except
    // a mapping bug.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let requested = unsafe { guest::read_u64(out) }.unwrap_or(0);
    let base = if requested == 0 {
        next_mapping_base(len)
    } else {
        requested
    };
    // **Checked, because a panic here is undefined behaviour.** This runs on a frame the
    // guest called into through a `sysv64` boundary, and an unwind across that is not
    // something the language defines - it does not surface as a panic message, it
    // surfaces as an unattributable fault somewhere in host code.
    //
    // `next_multiple_of` panics on overflow, and a guest is entitled to pass any value at
    // all - including the all-ones word some callers use to mean "no preference" (D156).
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
    // **Reserve-then-map is one range, reserved once.** A guest commonly carves a virtual
    // range with `sceKernelReserveVirtualRange` and *then* places physical memory inside it
    // with this call, at the address it was handed. On orbistoun's identity-mapped model the
    // reservation already backs those pages, so mapping into it is a re-protect, not a second
    // reservation - and reserving again conflicts with the reservation that already holds the
    // range and answers `NoMemory`, which the guest reads as out-of-memory and then writes
    // through the null pointer it kept (the `image+0xafcc08` wall, made legible by the return
    // a call now records - D459, D460). An address the guest did *not* pre-reserve is a fresh
    // mapping and still reserved.
    let mut base = base;
    let mut placed = if space.owns(base, len) {
        space.protect(base, len, protection)
    } else {
        space.reserve(base, len, protection).map(|_| ())
    };
    // **Retried past a conflict, but only when the guest expressed no preference.**
    // `sceKernelReserveVirtualRange` has done this since it was written, for the reason its own
    // comment gives: *the counter steps an address, not the map, so a base it hands back can
    // already be held*. This call had the identical hazard and no mitigation - it took one
    // address and answered `NoMemory`, and a guest asking for thirty-two mebibytes *anywhere*
    // was told there was none (D604).
    //
    // A guest that named an address is refused as before. It asked for somewhere specific, and
    // quietly moving it is the corruption the paragraph above refuses.
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
        // **Not recorded here.** The wrapper above records every way out that is not success,
        // and a note at both levels put one refusal in the list twice - visible only because the
        // list showed two entries sharing a call ordinal. One place, which is the rule the
        // successful side already follows (D604).
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

/// `sceKernelBatchMap(entries, count, completed)` - maps a run of direct-memory pieces in one call.
///
/// # Why a display needs it
///
/// It is how a scanout surface is brought up: the AGC display init allocates one physical block
/// with `sceKernelAllocateMainDirectMemory` and then batch-maps sixteen 2 MiB pages of it into the
/// GPU virtual range, at the addresses it will render to. Until this landed the whole call fell on
/// a stub, so the map failed, and the display reported itself not ready one step before any pixel
/// (the open-toolchain cube, worklog 806).
///
/// Every entry names **where** it wants the memory (`vaddr`), **which** physical offset (`paddr`),
/// **how much** (`len`) and its **protection** (`prot`). So unlike the "anywhere" named map, a
/// batch entry is placed at exactly the vaddr it asks for and never relocated.
///
/// # The entry layout
///
/// `entries` points at `count` records of the console's descriptor: `vaddr` (8), `paddr` (8),
/// `len` (8), `prot` (1) with three bytes of padding, `flags` (4) - 32 bytes each. `completed`
/// receives how many were mapped, so a caller can tell a partial map from a total failure; the
/// call answers `0` when every entry succeeded and the kernel's error otherwise.
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
        // `prot` is the low byte of the word at offset 24; the three padding bytes and the four
        // `flags` bytes follow it and are not read - nothing measured varies on them.
        let placed = map_direct_at(vaddr, paddr, len, prot_word & 0xff);
        if placed != OK {
            outcome = placed;
            break;
        }
        done += 1;
    }
    // **Written whatever the outcome.** The count is how a caller tells a partial map from a total
    // failure, so a stopped batch must still say how far it got - our own SDK reads it back.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let _ = unsafe { guest::write_u64(completed, done) };
    outcome
}

/// Maps `len` bytes of physical memory `physical` at the exact guest virtual address `base` with
/// `prot`, recording the mapping and its physical alias.
///
/// The placement half of a direct-memory map, at a named address that is never relocated. It is
/// the shared shape of [`map_named_direct_memory_inner`], which adds the "anywhere" search, the
/// already-mapped-return and the `void**` protocol on top; a batch entry needs none of those,
/// because it always names its own address. Answers `OK` or a [`GuestError`] raw code.
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
    // Reserve-then-map, the same as the named map: a range the guest already reserved is
    // re-protected in place rather than reserved a second time (D459).
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

/// How many mappings this run filled, and how many bytes.
///
/// **Counted so the diagnostic can be shown to have run.** A poison that changes nothing
/// and a poison that never executed produce identical output, and reading the first as the
/// second is how a class gets recorded as eliminated when it was never tested (D325).
static FILLED_MAPPINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Bytes filled, alongside [`FILLED_MAPPINGS`].
static FILLED_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What the direct-memory fill has actually done, as `(mappings, bytes)`.
///
/// `(0, 0)` from a run that asked for a fill means the diagnostic **did not fire**, and
/// any conclusion drawn from that run is about nothing.
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
/// # Why the loader tells the kernel rather than the kernel asking
///
/// The same reason [`note_region`] exists: a module's pages live in the loader's address
/// space, which this crate never sees. `sceKernelLoadStartModule` is handed a **path** and
/// nothing else, so without this it cannot find the module the guest is naming (D515).
///
/// The **contents** of the array are deliberately not read here. It holds function pointers
/// that relocation writes, and a value read before relocation is the unrelocated one - so the
/// address is recorded and the pointers are read at start time, which is after linking.
pub fn note_module_initialisers(library: &str, initialisers: ModuleInitialisers) {
    if let Ok(mut placed) = PLACED_MODULES.lock() {
        if !placed.iter().any(|(name, _)| name == library) {
            placed.push((library.to_owned(), initialisers));
        }
    }
}

/// Runs every placed module's initialisers, whether or not the guest asked for it.
///
/// # The question this answers, and why it is a diagnostic rather than behaviour
///
/// D515 starts a module when the guest calls `sceKernelLoadStartModule`, which is what that
/// function's name says it does. It left open whether a module bound as an **import** - one
/// the guest never asks to load, because the loader placed it and resolved against it - has
/// its initialisers run at process start on the console. `libc.prx` is exactly that: placed,
/// carrying one initialiser, and never started, because nothing ever loads it by name.
///
/// **Starting everything to make a title work would be inventing behaviour** (D515 says so in
/// those words). So this is off by default and asks a question instead: if a run with it
/// reaches further, an ordering is missing and the next job is to find out which module and
/// when; if it changes nothing, a whole explanation is eliminated rather than argued (D519).
///
/// Answers how many modules were started and how many initialisers ran, so a run that
/// intervened and did nothing says so rather than reading as an elimination (D325).
pub fn start_every_placed_module() -> (usize, u64) {
    let Ok(placed) = PLACED_MODULES.lock() else {
        return (0, 0);
    };
    // Collected first: `run_initialisers` enters guest code, which can call back into this
    // crate, and holding the lock across that would deadlock on the first module that does.
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
/// Matched on the **leaf** of the path. The guest names a module by its sandbox path
/// (`/app0/Media/Modules/PS5Util.prx`) and the loader knows it by the library name it
/// answers to; the file name is the one part both agree on, and it is what a title's own
/// modules are unique by within a title.
fn initialisers_for(path: &str) -> Option<ModuleInitialisers> {
    let leaf = path.rsplit('/').next()?;
    // The loader knows a module by the **import library name** it answers to, which carries no
    // extension: the guest asks for `Il2CppUserAssemblies.prx` and the loader placed
    // `Il2CppUserAssemblies`. Comparing the two directly matches nothing, which is what the
    // first version of this did - and it reported "no initialisers recorded" for a module that
    // had them, which reads as a missing module rather than a missing comparison (D515).
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    let placed = PLACED_MODULES.lock().ok()?;
    placed
        .iter()
        // Case-insensitively, because that is how the loader itself matches a title's files
        // to its import names - one title in the corpus disagrees with its own spelling (D482).
        .find(|(library, _)| library.eq_ignore_ascii_case(stem))
        .map(|(_, initialisers)| *initialisers)
}

/// Runs a placed module's `DT_INIT` and `DT_INIT_ARRAY`, and says how many ran.
///
/// # Why this is what `Start` means here
///
/// Neither module a real title asked for exports `module_start`, and both carry `DT_INIT`
/// and `DT_INIT_ARRAY` - the ordinary ELF mechanism, which is what a C++ module's static
/// constructors are reached through. Running them is what makes the globals those
/// constructors fill non-null, which is what the guest reads immediately afterwards (D514).
///
/// **Order is `DT_INIT` then the array, and the array in ascending order.** That is the
/// System V ABI's own order, and the constructors within one module depend on it.
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
            // The array is outside anything readable, so the recorded address is wrong
            // rather than the module being empty. Stop, rather than walk further into it.
            break;
        };
        // A null or a `-1` is a legitimate empty slot the ABI allows, and skipping them is
        // what a real runtime does rather than calling through them.
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

/// Every `/app0` module a guest asked to load and start, and the handle it was given.
///
/// A `Mutex` rather than an atomic because the paths are what make the report worth
/// reading, and a count alone would say a module was not started without saying which.
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
/// **A count of zero is reported rather than treated as success.** A module whose array was
/// found and held nothing callable and a module that ran every constructor it has produce the
/// same handle, and only one of them is a module the guest can use.
fn started(path: &str, handle: u64, ran: u64) {
    if let Ok(mut loads) = STARTED.lock() {
        loads.push((path.to_owned(), handle, ran));
    }
}

/// One line for a run report: the event queues a guest made, and what each was used for.
///
/// [`None`] when none was created, so a quiet run stays quiet.
///
/// **A queue with nothing registered is worth saying out loud.** `sceKernelAddUserEventEdge`
/// refuses a handle no queue answers, and a run where every registration was refused looks
/// exactly like a run that made none - which is the ambiguity that makes a check worse than no
/// check (D325, D524).
///
/// **And so is a queue that is waited on and never delivers.** Registrations alone cannot tell a
/// queue nobody uses from a thread that is stuck on one: both read as a name and a count. The
/// waits and the deliveries beside them make the difference visible in every run, of every
/// title, without anybody dumping an argument - which is how this was found the first time
/// (D615).
#[must_use]
pub fn equeue_summary() -> Option<String> {
    let queues = sync::equeue_summary();
    if queues.is_empty() {
        return None;
    }
    let named = queues
        .iter()
        .map(|q| {
            // The starvation case is called out rather than left to arithmetic, and now names the
            // reason: a starved queue has no producer. The graphics queues that starve here wait on
            // driver-work completion, which orbistoun does not post until it executes the work
            // (D705) - so a reader learns why from the line rather than reaching for D615.
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
/// [`None`] when the guest asked for none, so a quiet run stays quiet.
///
/// **This is a gap report, not a diagnostic.** Nothing here is switched on by a variable and
/// nothing intervenes; it says what `sceKernelLoadStartModule` did not do. Until something
/// runs `DT_INIT_ARRAY`, a guest that calls into one of these modules is running code whose
/// constructors never ran - which surfaces as a null field read somewhere with no connection
/// to the load (D514).
#[must_use]
pub fn module_start_summary() -> Option<String> {
    let mut parts = Vec::new();

    if let Ok(started) = STARTED.lock() {
        if !started.is_empty() {
            let named = started
                .iter()
                .map(|(path, handle, ran)| {
                    // `u64::MAX` is the bulk start before entry, which has no handle because
                    // no guest asked for it. Rendering it as one would invent a handle.
                    let how = if *handle == u64::MAX {
                        "before entry".to_owned()
                    } else {
                        format!("handle {handle:#x}")
                    };
                    format!("{} ({ran} initialiser(s), {how})", leaf_of(path))
                })
                .collect::<Vec<_>>()
                .join(", ");
            // A module that ran nothing is called out separately: its array was found and
            // held nothing callable, which is not the same as having started.
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

    // What the loader placed, named. The set is small, and every wrong conclusion in this
    // area so far came from inferring it from something else - an import list that does not
    // contain it, a filename that is not the library name (D515).
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

/// One line for a run report: what the direct-memory fill actually did.
///
/// [`None`] when no fill was asked for, so a quiet run stays quiet - the same shape as
/// `console::summarise`.
///
/// **A run that asked for a fill and reports zero has tested nothing.** A poison that
/// changed nothing and a poison that never fired produce identical output, and reading the
/// second as the first is how a class gets recorded as eliminated without ever having been
/// tried (D325).
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
/// Pure, so the decision is testable without reserving anything. **Writable mappings
/// only**: a guest may ask for read-only or execute-only memory, and writing to it would
/// fault inside the emulator - turning a diagnostic into a crash that reads as the guest's.
const fn fill_for(byte: Option<u8>, protection: orbistoun_mem::Protection) -> Option<u8> {
    match byte {
        Some(byte) if protection.write => Some(byte),
        _ => None,
    }
}

/// The byte every fresh direct-memory mapping is filled with, if a run asked for one.
///
/// Read once. A diagnostic that re-read its variable could change behaviour part-way
/// through a run, which makes the run unreproducible in the one dimension it exists to
/// measure.
fn direct_fill() -> Option<u8> {
    static FILL: OnceLock<Option<u8>> = OnceLock::new();
    *FILL.get_or_init(|| {
        let raw = orbistoun_env::DIRECT_FILL.get()?;
        let byte = u8::from_str_radix(raw.trim_start_matches("0x"), 16).ok()?;
        // Zero is what the region already is, so asking for it is asking for nothing -
        // and a diagnostic that silently does nothing is worse than one that is off.
        (byte != 0).then_some(byte)
    })
}

/// Everything that has to happen once a mapping exists, in the one place it can be seen.
///
/// # Why this is a function and not two calls at each site
///
/// Three paths place guest mappings - direct memory, `mmap`, and a reserved virtual range -
/// and each ended in a bare `fill_mapping`. Adding a second obligation to "a mapping now
/// exists" as three more call sites is the hazard that has already been paid for twice in
/// this project: a reporter wired into the fault path and not the clean-exit path worked for
/// a title that crashed and not for one that stopped (worklog 425). One function means the
/// fourth mapping path has one thing to call rather than a list to remember.
fn mapping_placed(base: u64, len: u64, protection: orbistoun_mem::Protection, hinted: bool) {
    // **The sequence, when a run asked for it.** A bump-allocated address is a function of
    // everything placed before it, so a run whose addresses differ from another's can only be
    // diffed by the order - and the failures were the only half ever reported (D581).
    mapped::note(base, len, protection.read, hinted);
    // **Published as guest memory the diagnostics may read, because that is what it is.**
    // The readable ranges are installed before the guest is entered - the image and the main
    // stack - so a pointer into anything the guest mapped *afterwards* dumped as `no region
    // this run mapped, and address-shaped`, which reads as a wild pointer and is an ordinary
    // heap address. Thread stacks had exactly this blind spot and were fixed the same way
    // (D387); guest mappings are the other half of it, and they are where an allocator puts
    // the structures a call is handed (D579).
    //
    // **Readable mappings only, and that is the safety precondition rather than a filter.**
    // A guest may ask for write-only or execute-only memory; publishing it would let a dump
    // read a page the host refuses, turning a diagnostic into a fault inside the emulator
    // with no relation to the guest.
    if protection.read {
        orbistoun_thunk::note_readable_range(base, len);
    }
    // The arena's extent, so a fault or a dump can say *guest mappings+0x…* rather than
    // print a bare address that looks the same as a count.
    note_arena_extent(base, len);
    fill_mapping(base, len, protection);
}

/// How far the mapping arena has been used, as an address one past the last byte placed.
///
/// Zero until the guest maps something, which is why [`arena_extent`] answers `None` for it:
/// *nothing was mapped* and *the arena starts at zero* are different findings.
static ARENA_END: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Records that `[base, base + len)` was placed, if it is inside the arena.
///
/// A guest that asks for a specific address is honoured wherever it points (D459), and those
/// mappings are deliberately *not* counted: stretching the arena to cover a hint at
/// `0x5000…` would name every address between the two as arena, which is a span nothing
/// placed anything in.
fn note_arena_extent(base: u64, len: u64) {
    if let Some(end) = arena_end_of(base, len) {
        ARENA_END.fetch_max(end, std::sync::atomic::Ordering::Relaxed);
    }
}

/// How far the arena reaches once `[base, base + len)` is placed, or `None` if it is not in it.
///
/// The decision, with the shared counter left outside it - a test that had to bump the global
/// would change what a guest running beside it sees, and the suite runs on parallel threads
/// (principle 8). Saturating, because a guest is free to ask for a length that overflows and a
/// wrapped end would shrink the arena rather than grow it.
const fn arena_end_of(base: u64, len: u64) -> Option<u64> {
    if base < MAPPING_BASE {
        return None;
    }
    Some(base.saturating_add(len))
}

/// The span guest mappings have been placed in, or `None` if the guest mapped nothing.
///
/// The **arena**, not the set of mappings: it spans the gaps between them and the padding
/// unit `next_mapping_base` leaves, so an address inside it was not necessarily mapped. That
/// is what the name says - *where guest mappings go* - and a fault report states separately
/// whether the address was mapped, so the two together do not overstate (the same
/// concession D489 made for the title's modules, for the same reason).
#[must_use]
pub fn arena_extent() -> Option<(u64, u64)> {
    arena_span(ARENA_END.load(std::sync::atomic::Ordering::Relaxed))
}

/// The arena span implied by an end address, or `None` for an arena nothing has used.
///
/// **`None` rather than a zero-length span.** *Nothing was mapped* and *a region exists and is
/// empty* are different findings, and a zero length is already how the reporter spells an
/// unused region slot - so returning one would register the region and leave it unnamed
/// anyway, with nothing saying which of the two had happened.
const fn arena_span(end: u64) -> Option<(u64, u64)> {
    if end <= MAPPING_BASE {
        return None;
    }
    Some((MAPPING_BASE, end - MAPPING_BASE))
}

/// Fills a fresh mapping, so reading it back is distinguishable from reading a zero.
///
/// # The question this answers
///
/// Fresh host memory is zero, and so is an out-parameter nobody wrote. A guest that reads
/// `0x0` and dies has not said which of those happened - and for `PPSA28061` that
/// distinction is the whole of what is left, three other classes having been eliminated
/// (`docs/BACKLOG.md`). The stack and the heap already have this; direct memory is the
/// third place, and the one neither of those covers (D325).
///
/// **Writable mappings only.** A guest may ask for read-only or execute-only memory, and
/// writing to it would fault inside the emulator - turning a diagnostic into a crash that
/// looks like the guest's fault.
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
/// Exists because `u64::next_multiple_of` **panics**, and nothing reachable from a guest
/// call may panic: the frame was entered across a `sysv64` boundary, and unwinding
/// through it is undefined. The failure does not look like a panic - it looks like a
/// fault in host code with nothing to attribute it to (D156).
const fn checked_next_multiple_of(value: u64, align: u64) -> Option<u64> {
    if align == 0 {
        return None;
    }
    value.checked_next_multiple_of(align)
}

/// Translates the guest's protection bits.
///
/// The values are the System V / POSIX `PROT_*` set - read 1, write 2, execute 4 - which
/// is published and is what a FreeBSD-derived kernel uses. Anything the guest asks for
/// that is not one of those is ignored rather than guessed at.
///
/// A request naming no access at all becomes read-only rather than nothing: a mapping the
/// guest cannot touch is indistinguishable from a failed mapping, and it would fault at a
/// place with no relation to the cause.
fn protection_from_guest(prot: u64) -> orbistoun_mem::Protection {
    /// POSIX `PROT_READ`.
    const READ: u64 = 1;
    /// POSIX `PROT_WRITE`.
    const WRITE: u64 = 2;
    /// POSIX `PROT_EXEC`.
    const EXEC: u64 = 4;

    let (read, write, execute) = (prot & READ != 0, prot & WRITE != 0, prot & EXEC != 0);
    orbistoun_mem::Protection {
        // Readable if asked, and also when nothing was asked at all.
        //
        // **And when writing was asked for, because this architecture has no other option.**
        // An x86-64 page table entry has a write bit and a no-execute bit and no read bit at
        // all: a writable page is readable, and there is no encoding that is not. POSIX
        // anticipates exactly this - `mmap(2)` says an implementation may permit accesses
        // other than those requested - and FreeBSD on amd64 grants read with write for the
        // same reason.
        //
        // Recording it as unreadable made orbistoun stricter than the machine it presents,
        // and the cost was not theoretical: PPSA03416 maps its asynchronous-file destination
        // buffer with write alone, so the mapping was never published as readable, so the
        // argument dump could not show what the guest had put in it - and the buffer was read
        // as one the guest had never been given at all (D580's error, corrected in D588).
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
/// Who is asking. The calling thread is *adopted* if the guest did not create it, because
/// the process's first thread runs guest code without ever having been created and the
/// guest still asks it this.
fn pthread_self(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread::adopt("main")
}

/// `scePthreadGetthreadid()` - the calling thread's unique integer id.
///
/// FreeBSD's `pthread_getthreadid_np`: a per-thread integer, distinct in *type* from the
/// `ScePthread` handle [`pthread_self`] answers but serving the same identity question a guest
/// asks - "which thread am I / are these the same thread". The registry handle is already a
/// `u64` unique to the thread and stable for its life, so it is that id; the caller is adopted
/// for the same reason `scePthreadSelf` adopts it, so the process's first thread - which runs
/// guest code without ever being created - gets a real id rather than "no thread". Answering
/// the placeholder here made every thread report one shared id, which a guest keying anything
/// on thread identity reads as a single thread (D452).
fn pthread_getthreadid(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    thread::adopt("main")
}

/// POSIX thread-specific-data keys - `pthread_key_create` and the family a guest's own runtime
/// builds thread-local storage on.
///
/// # Why these, and why now
///
/// Unity ships Intel TBB, whose task scheduler keeps its per-thread state through
/// `pthread_key_create`/`pthread_setspecific`. Unimplemented, `pthread_key_create` answered the
/// placeholder - which TBB read as key allocation failing, so it threw
/// `TBB failed to initialize task scheduler TLS` and, with exceptions disabled, aborted (D453).
/// This is the C-runtime scheduler's foundation, reached long before any frame.
///
/// # The model
///
/// A key is a small integer from a monotonic counter. The value bound to it is **per thread**,
/// which a `thread_local!` map gives directly - a guest thread is a host thread here (D014), so
/// the two coincide, and a thread that never set a key reads null, exactly as POSIX requires.
///
/// The destructor `pthread_key_create` is handed is **recorded nowhere and never run**: nothing
/// tears a guest thread down through this layer yet, so there is no moment for it to fire, and
/// firing one at the wrong time is worse than not. Recorded rather than pretended.
///
/// Reference: POSIX.1-2008 `pthread_key_create`, `pthread_setspecific`, `pthread_getspecific`,
/// `pthread_key_delete`.
static NEXT_TLS_KEY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// This thread's key -> value bindings. Empty on a fresh thread, so every key reads null until
/// it is set. The `LocalKey`-returning shape matches `thread::current_handle` (D014).
#[allow(clippy::type_complexity)]
fn tls_values()
-> &'static std::thread::LocalKey<std::cell::RefCell<std::collections::HashMap<u32, u64>>> {
    thread_local! {
        static VALUES: std::cell::RefCell<std::collections::HashMap<u32, u64>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    &VALUES
}

/// `pthread_key_create(key_out, destructor)` - allocate a thread-specific-data key.
fn pthread_key_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // args[1] is the destructor, which is intentionally not stored - see the family note.
    let key = NEXT_TLS_KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u32(args[0], key) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_setspecific(key, value)` - bind a value to a key for the calling thread.
fn pthread_setspecific(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (key, value) = (args[0] as u32, args[1]);
    tls_values().with(|m| m.borrow_mut().insert(key, value));
    OK
}

/// `pthread_getspecific(key)` - the calling thread's value for a key, or null if unset.
fn pthread_getspecific(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let key = args[0] as u32;
    tls_values().with(|m| m.borrow().get(&key).copied().unwrap_or(0))
}

/// `pthread_key_delete(key)` - retire a key. The binding in the calling thread is dropped; a
/// retired id is not reused, so a stale use reads null rather than another key's value.
fn pthread_key_delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let key = args[0] as u32;
    tls_values().with(|m| m.borrow_mut().remove(&key));
    OK
}

/// `scePthreadCreate(thread, attr, entry, arg, name)`.
///
/// A real host thread, always - principle 6, and not negotiable. The attribute block is
/// **ignored rather than parsed**: its layout is not known from any lawful source, and
/// reading fields out of it by guessing offsets would produce a stack size or a detach
/// state that looks deliberate and is not. What is honoured is what the arguments state
/// directly.
fn pthread_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, attr, entry, argument, name) = (args[0], args[1], args[2], args[3], args[4]);
    if out == 0 || entry == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(name) };
    let start = thread::Start { entry, argument };
    // The attribute block the guest built through `scePthreadAttrSet*`, honoured where obSCEne
    // measured that the console honours it (`031-stackattr`, REQ-...c2e9): a live thread read back
    // the stack size and affinity a block asked for. This call had ignored `attr` entirely, so 125
    // threads across four titles ran on the 8 MiB default with default affinity regardless of what
    // their blocks set - see [`thread_attributes`] for which fields are read and which are not.
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
        // Reported, not swallowed. A guest told its thread started when it did not will
        // wait on something that will never happen, and the hang gets attributed to
        // whatever it was waiting for.
        Err(_) => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// The affinity and stack size to spawn a thread with, decoded from a thread attribute block.
///
/// The pure half of [`thread_attributes`], so the field decisions can be tested without a guest
/// memory to read them out of. `stack_field` and `affinity_field` are the raw stored values.
///
/// - **stack size:** a size the guest set is honoured, which is what obSCEne measured the console
///   doing (`031-stackattr`, REQ-...c2e9: a block asking for `0x181000` produced a thread that ran
///   on exactly `0x181000`). A block nothing set carries the fresh default
///   ([`DEFAULT_ATTR_STACK_SIZE`], 64 KiB), and a size of `0` is POSIX's "use the default" - neither
///   is honoured, because a fresh attribute is *not* measured to bound a real thread and 64 KiB is
///   far below orbistoun's 8 MiB [`orbistoun_mem::stack::DEFAULT_STACK_SIZE`]; shrinking a working
///   thread to it would invent an overflow the console does not have.
/// - **affinity:** the mask is carried through, so a thread created from a block reads its affinity
///   back through `scePthreadAttrGet` - the "stored, not applied" bargain
///   [`pthread_attr_setaffinity`] already strikes, now reaching the thread record rather than
///   stopping at the block.
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
/// `attr` is the guest's `ScePthreadAttr` - a pointer to the handle [`pthread_attr_init`]
/// substituted, so this reads orbistoun's own attribute object (its field constants), not the raw
/// console layout. The decode is [`spawn_parameters`]; this only fetches the fields.
///
/// Two of the block's fields are deliberately not read:
/// - **detach state** is not acted on, for the reason [`pthread_detach`] gives: every guest thread
///   here is a host thread the runtime already reclaims when its body returns, so the detach promise
///   is kept by construction and there is nothing to do with the state but store it for readback.
/// - **priority** is not carried in the block on the console - obSCEne measured
///   `scePthreadAttrSetschedparam` refusing on a retail attribute (`0x8002002d`, REQ-...c2e9) - so it
///   is left unread here; a thread's priority arrives through `scePthreadSetprio`, not `attr`.
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
/// Carries the joined thread's return value back through `value`. The guest thread function's
/// return is in `rax` when its body returns; `thread::spawn` now keeps it and `thread::join`
/// makes it available once the host thread ends, so a guest that returns a result from a thread
/// and reads it after the join gets it rather than a zero (030-thread/join).
fn pthread_join(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, value) = (args[0], args[1]);
    // Checked before use, because a handle is an address now: an arbitrary guest value
    // arriving here must never be treated as one.
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
/// # The one import that aborted two titles
///
/// Unimplemented, this answered the placeholder error code and both Unity titles gave up
/// during static initialisation, forty-five calls in. Answering success alone takes them to
/// two hundred and twenty. The name could not be generated - the vocabulary held
/// `Semaphore` and the vendor wrote `Sema` - and was read out of a different title's data
/// entirely (D193).
///
/// # What is established and what is not
///
/// The first argument is a stack address the guest expects to be filled: that much is
/// observed, and **writing it is the entire point**. A stub that reported success without
/// writing would hand the guest whatever its stack held and produce a failure with no
/// signature anywhere, which is D171 exactly.
///
/// The remaining arguments are *inferred from the shape of every semaphore interface*, and
/// that inference is recorded as an assumption rather than stated as a fact. If the real
/// order differs, the counts are wrong and nothing here would notice - so the counts are
/// clamped to something a guest can survive rather than trusted.
fn create_semaphore(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[0];
    if out == 0 {
        // Nowhere to put the answer. Refused rather than dropped, because a guest that
        // then reads its uninitialised stack is the failure this whole function avoids.
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // Clamped, not trusted. If the argument order is not what is assumed, these are some
    // other value entirely - and a ceiling of four billion would turn a wrong guess into
    // an allocation the host refuses, far from here.
    let initial = u32::try_from(args[3]).unwrap_or(0);
    let ceiling = u32::try_from(args[4])
        .unwrap_or(u32::from(u16::MAX))
        .max(initial);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(args[1]) };

    let handle = sync::create_semaphore(initial, ceiling, &name);
    // **Eight bytes, and that reverses D210 on a measurement.**
    //
    // D210 narrowed this to four, reasoning from public interface documentation that the
    // destination is an `int *` rather than a `void **`. A conformance run has now planted a
    // `0xA5A5A5A5` guard in the word *after* an `int handle`, called this, and read the guard
    // back as **`0x0`** - so the console writes eight bytes where it was given four, and
    // obSCEne's own check reports it as a failure for that reason.
    //
    // Documentation lost to a measurement, which is this project's oracle ordering working
    // rather than being overridden: `Measured` outranks `Published` precisely because a
    // documented layout and a real one have already diverged once here (D468, the ctype
    // tables).
    //
    // **A guest is built against the console, so orbistoun has to do what the console does.**
    // Writing four bytes leaves the neighbour a title has allowed to be clobbered holding its
    // old value, which is a difference a title can see and cannot be told about.
    //
    // The high half is zero because that is what the guard read: `0x0`, not the top half of a
    // handle. Whether the console writes a 64-bit handle whose upper half happens to be zero
    // or writes four bytes and clears four more is not distinguishable from one guard word,
    // and both produce this (D509).
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
/// # What these are and why accepting is enough
///
/// A guest builds an attribute object, sets a type and a protocol on it, hands it to
/// `scePthreadMutexInit` and destroys it. Our mutexes are host `Condvar`-backed and
/// recursive-safe by construction (see `sync`), so nothing downstream reads the attribute -
/// accepting the calls and reporting success is the whole of the work.
///
/// **Nothing is written to the attribute object**, and that is a live risk rather than a
/// decision to be comfortable with: it is the D171 shape exactly, an out-parameter left
/// untouched. It holds only while nothing reads the attribute back. A guest calling a
/// `Get` counterpart would read whatever its stack held, and no trace would show why.
/// Recorded as an assumption in the knowledge file rather than as a comment nobody counts.
///
/// # How they were named, because it is unusual
///
/// Not generated and not consulted. Both titles *print* these names themselves, with the
/// source file and line, as soon as `printf` is implemented - they were reporting the
/// error the whole time and the emulator was discarding the message. The hash then confirms
/// each name independently, so the guest's claim is checked rather than believed (D187).
fn pthread_mutexattr_accept(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePthreadMutexInit(mutex, attr, name)`.
///
/// Writes an opaque handle where the guest expects its lock. As with thread creation the
/// attribute block is not parsed, so the recursion mode is the default rather than
/// whatever the guest asked for - stated in the trace, and the first thing to suspect if
/// a title deadlocks on a lock it takes twice.
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
/// A null attribute is the default, a normal lock. The type values are the platform's own,
/// measured in `015-sync/mutex-recursion`: `2` is recursive and `4` is error-checking; anything
/// else is a normal lock, which a second acquisition by the owner deadlocks on. The type is read
/// from the attribute object `scePthreadMutexattrSettype` wrote it into.
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
/// `None` covers the case worth naming: a **statically initialised** lock, where the
/// guest filled the location with a constant at compile time and never called init. The
/// handle there is not one this crate handed out, so it names nothing. Reporting that
/// honestly is the whole point - a stub returning success would let every thread through
/// the critical section at once, and the corruption would be blamed on whatever the lock
/// was protecting (principle 3).
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
        // Two different failures, deliberately given two different codes. A refusal is
        // the guest deadlocking against itself on a non-recursive lock; a miss is a
        // handle naming nothing. Collapsing them would make the trace unable to tell a
        // guest bug from a gap in this crate. **Neither can be a timeout here** - this
        // call waits forever, so `Busy` can only be the self-relock.
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
        // Not this thread's lock to release, or not held at all. **The console was asked this
        // exact question** - unlock a mutex nobody holds - and answered with the not-owner
        // errno rather than an invalid argument, which is a distinction a guest can act on
        // (D398).
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::NOT_OWNER).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadMutexTrylock(mutex)`.
///
/// **The one that must not answer `OK` when it fails.** `lock` blocks until it has the
/// mutex, so success is the only interesting answer; `trylock` exists precisely to report
/// that it could *not* take it, and a guest branches on that. A stub reporting success
/// would send it into a critical section it does not hold - which is worse than the missing
/// implementation it replaced, because nothing in the trace would say so (principle 3).
fn pthread_mutex_trylock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    let by = thread::adopt("main");
    match sync::acquire(handle, by, sync::Blocking::Never) {
        Some(sync::Acquisition::Locked) => OK,
        // Held by somebody else, or the owner re-taking a normal lock, which is the ordinary
        // outcome of this call rather than a misuse of it - the reason D256 gave it a code of
        // its own. **The console returned the busy errno** when asked to take a lock already
        // held, so the distinction is made with the value the machine uses (D398).
        Some(sync::Acquisition::Busy) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw())
        }
        // The owner re-taking an error-checking lock, which the console reports with the
        // invalid-argument errno (`0x8002_0016`) rather than the busy a normal lock gives -
        // a distinct code for a distinct condition, measured in 015-sync/mutex-recursion (D416).
        Some(sync::Acquisition::Deadlock) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadMutexDestroy(mutex)`.
///
/// The counterpart to `Init`, and the reason a run leaks lock objects without it. The
/// conformance probe asks for it seventeen times in one run - more than any other missing
/// entry - because it builds and tears down a lock per check (D256).
fn pthread_mutex_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = mutex_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    // Cleared, so a guest that destroys twice is told the handle is gone rather than being
    // handed a freed one - and so a use-after-destroy shows up here rather than as
    // corruption somewhere the lock was protecting.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    unsafe { guest::write_u64(args[0], sync::NO_MUTEX) };
    OK
}

/// `sceKernelGetProcessTime()` - microseconds since this process began.
///
/// # Monotonic, and measured from the guest's own start
///
/// Wall-clock would let a title see time run backwards when the host's clock is corrected,
/// and an epoch-based value would make two runs of the same title incomparable. Neither is
/// what the name says: this is *process* time, so it starts when the process does.
///
/// **Not the same clock as the run's call budget.** That one exists to make a run
/// reproducible; this one is a value the guest reads and branches on, and pinning it would
/// stop any title that waits for time to pass (D256).
fn kernel_get_process_time(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // **Read through `clocks`, not from an `Instant` of its own.** This kept a second origin
    // and a second source, so the platform's process time and POSIX's monotonic time were two
    // different clocks - and under `ORBISTOUN_CLOCK=logical` only one of them would have
    // repeated, which is the half-fix that looks like a fix (D582).
    //
    // Microseconds: the unit every `GetProcessTime` in this family reports, and the one the
    // probe's own check compares two readings in. Saturating, so a run long enough to
    // overflow reports a stuck clock rather than a wrapped one.
    u64::try_from(orbistoun_hle::clocks::since_start_nanos() / 1_000).unwrap_or(u64::MAX)
}

/// `sceKernelGetProcessTimeCounter()` - the same elapsed time, counted in ticks.
///
/// # Why this is a separate clock from `GetProcessTime`
///
/// It is the same span measured in different units, and a guest uses both: the microsecond
/// call for anything it will print or compare against a timeout, the counter for anything it
/// will divide by a frequency. A run on a target console read them across one sleep and got
/// `0x4fbb` microseconds against `0x1f12cd9` ticks for the same interval, which is the ratio
/// this pair has to preserve or a guest converting between them lands somewhere else.
///
/// Measured from process start for the same reason its microsecond twin is (D256, D398).
fn kernel_get_process_time_counter(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ticks_since()
}

/// `sceKernelGetProcessTimeCounterFrequency()` - ticks per second for the counter above.
///
/// **The same number the time stamp counter reports**, which is not an assumption made here
/// for tidiness: the console answered both calls in one run and returned `0x5f25_9b8e` to
/// each. Answering them differently would be inventing a distinction the machine does not
/// make (D398).
fn kernel_get_process_time_counter_frequency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    TSC_HZ
}

/// Where the version sits in the structure this call answers with.
///
/// Read straight off the guest: it takes the pointer, reads a sixteen-bit value at this
/// offset, shifts it left sixteen and compares the result against firmware bands. Nothing
/// here chose the offset (D403).
const SYSTEM_VERSION_AT: usize = 0x16;

/// How big the structure is.
///
/// Larger than the one field anybody has been seen to read, so a guest reaching further finds
/// something rather than running off the end - and filled with a value that is obviously not
/// data, so a guest that *acts* on another field shows up as nonsense rather than as a
/// plausible answer.
const SYSTEM_VERSION_BYTES: usize = 0x40;

/// The byte every unestablished field of that structure holds.
///
/// Not zero. Zero is a legitimate value for most things, so a guest reading an unmodelled
/// field would get a plausible answer and this would never hear about it.
const SYSTEM_VERSION_FILL: u8 = 0xA5;

/// `syscall(649, kind, length, out)` - what system this is, which a guest needs before it can
/// start.
///
/// # Why this exists at all
///
/// Four open-toolchain payloads stop dead without it. Each asks for it, reads a version out of
/// the answer, and picks a code path; with nothing to read they print `Unable to initialize
/// rtld` and exit. It is the single call that was blocking every one of them (D403).
///
/// # What is known, and what is inferred
///
/// **Known:** the number, the arguments `(2, 8, out)`, that the answer is a *pointer*, and
/// that the guest reads sixteen bits at offset 0x16 of what it points at and compares them
/// against 0x0700FFFF, 0x085FFFFF, 0x093FFFFF and 0x103FFFFF once shifted. All of that is read
/// off the guest's own instructions.
///
/// **Inferred:** that those bands are firmware versions and the field is therefore this
/// system's version. Nothing documents it. It is checkable, which is the point - a value in a
/// different band sends the guest down a different branch, and a run can watch which.
///
/// **Not known:** everything else in the structure. Two fields are not modelled because
/// nothing has been seen reading them, and the fill makes that visible rather than quiet.
fn vendor_system_version(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[2];
    let version = machine().firmware;
    if version == 0 {
        // **Refused, not answered with zero.** Zero is inside the lowest band the guest tests,
        // so it would not fail - it would send the guest down the path meant for the oldest
        // system there is, and the run would look like it worked (D397).
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    }
    if out == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }

    let block = system_version_block(version);
    let Ok(at) = usize::try_from(out) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    // SAFETY: a guest-supplied `void **` under the identity mapping (D014), which the guest
    // passed expecting to be written through - it reads the pointer back immediately.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), block);
    }
    OK
}

/// The structure this run answers with, built once and never freed.
///
/// Leaked deliberately: the guest keeps the pointer and reads through it whenever it likes, so
/// the lifetime has to outlast anything this side can scope.
fn system_version_block(version: u16) -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        // From the shared region, so the address the guest is handed repeats (D584). Filled
        // rather than zeroed, so a field nobody has established is recognisable as unset
        // rather than reading as a valid zero.
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
/// **Told rather than derived**, exactly as the stack span and the module list are - and held
/// in `orbistoun-core` rather than here, because the C library answers questions about the
/// same machine and the two crates cannot see each other (D394, D397).
fn machine() -> &'static orbistoun_core::machine::Machine {
    orbistoun_core::machine::presented()
}

/// `sceKernelIsCex()` - whether this is a retail console.
///
/// # Why answering at all is the fix
///
/// This family reports a boolean, and an unimplemented one answered the placeholder error
/// code - which is non-zero, which reads as **true**. So the platform claimed to be a
/// retail unit *and* a devkit *and* a test kit at once, and the conformance probe caught
/// exactly that (D271). It is the D125 shape in a boolean: an error code in a register the
/// caller reads as data.
///
/// **Assumed, not established**: orbistoun presents a retail console, because that is what
/// the corpus is built for. A title taking a devkit path would behave differently and
/// nothing in a trace would say which it took.
fn is_cex(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_retail())
}

/// `sceKernelIsDevkit()` - false, and deliberately so. See [`is_cex`].
///
/// **The spelling is the whole point.** It is `Devkit`, with a lower-case `k`, and this was
/// declared as `DevKit` - a different string, therefore a different hash, therefore a
/// different symbol. D271 fixed the family's answer and fixed it for a name no guest imports,
/// so the real one went on landing on an unimplemented stub whose placeholder is non-zero and
/// reads as *true*. The probe reported the platform as both a retail unit and a devkit for as
/// long as the fix had been in (D393).
///
/// Confirmed by hash rather than chosen: the symbol database named a real import with it.
fn is_devkit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_development_kit())
}

/// `sceKernelIsNeoMode()` - false, presenting a base console.
///
/// Same shape and the same danger: unimplemented it answered a placeholder, which is non-zero,
/// which tells a guest it is on the more capable hardware and may take a path that expects it.
///
/// **Assumed, not established**, like the retail answer it sits beside: orbistoun presents one
/// machine and this is the half of it that says which.
fn is_neo_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_faster_revision())
}

/// `sceKernelIsDevelopmentMode()` - false. See [`is_devkit`].
fn is_development_mode(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_development_mode())
}

/// `sceKernelIsTestKit()` - false, and deliberately so. See [`is_cex`].
fn is_testkit(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(machine().is_test_kit())
}

/// `posix_getpagesize()` - the guest's page size, not the host's.
///
/// A host with 16K pages still has to present the platform's 4K semantics, which is why
/// this reads the constant rather than asking the operating system.
fn getpagesize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    orbistoun_core::GUEST_PAGE_SIZE
}

/// `usleep(microseconds)` - sleeps, and reports success.
///
/// **Actually sleeps.** Returning immediately would make a title's frame pacing run as fast
/// as the host can loop, and a guest that polls with a short sleep between attempts would
/// spin instead - which is the same class of wrong as a stub reporting success it did not
/// achieve.
fn usleep(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let how_long = std::time::Duration::from_micros(args[0]);
    std::thread::sleep(how_long);
    // **And the clock has to agree that it happened.** `orbistoun-libc`'s three sleeps were
    // taught this and this one was not, which is the same wiring hazard as a reporter installed
    // on some of the ways a run can end: a guest polling with a short sleep between attempts
    // reads a clock that barely moved and polls again, a different number of times each run
    // (D582).
    orbistoun_hle::clocks::advance(how_long.as_nanos());
    OK
}

/// `posix_sigemptyset(set)` - clears a signal set.
///
/// **Sixteen bytes, not eight.** A `sigset_t` on a FreeBSD-derived system is four 32-bit
/// words, so clearing one word left three quarters of the set holding whatever the guest's
/// stack did - and the probe found a signal still in a set it had just emptied (D271).
///
/// Nothing here delivers signals, so a guest that builds a set and installs a handler will
/// find the handler never runs. Recorded as an assumption rather than implied by this
/// reporting success.
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

/// `_sigprocmask(how, set, oldset)` - examine or change the blocked-signal mask.
///
/// # What the guest asks for, and what it is told
///
/// PPSA02664 passes `how = 1` with a **null** `set` - the classic query idiom: `SIG_BLOCK` with
/// nothing to block is a pure read of the current mask. Answering it with a placeholder left the
/// guest reading its own stack as a signal mask.
///
/// `how` is FreeBSD's, which is citable: `SIG_BLOCK` 1, `SIG_UNBLOCK` 2, `SIG_SETMASK` 3.
///
/// **The mask is bookkeeping and nothing else.** Nothing here delivers signals, so blocking one
/// changes no behaviour - the same statement `posix_sigemptyset` already makes, and it is
/// recorded rather than implied by this reporting success (D271, D562).
fn sigprocmask(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// FreeBSD's `SIG_BLOCK`, `SIG_UNBLOCK` and `SIG_SETMASK`.
    const BLOCK: u64 = 1;
    const UNBLOCK: u64 = 2;
    const SETMASK: u64 = 3;

    static MASK: Mutex<[u64; SIGSET_WORDS as usize]> = Mutex::new([0; SIGSET_WORDS as usize]);
    let Ok(mut mask) = MASK.lock() else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };

    // The old mask first: a call that changes and reports must report what it replaced, not
    // what it installed.
    if args[2] != 0 {
        for (index, word) in mask.iter().enumerate() {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            if !unsafe { guest::write_u64(args[2].saturating_add(index as u64 * 8), *word) } {
                return u64::from(GuestError::InvalidArgument.as_raw());
            }
        }
    }
    if args[1] == 0 {
        // A null set is a query, and `how` is not consulted - which is what the guest does.
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

/// `sceKernelUuidCreate(out)` - writes a 128-bit identifier.
///
/// # Deterministic on purpose, which is a real trade
///
/// A UUID is meant to be unpredictable; this one is a counter. **Reproducibility wins here**,
/// because the only progress measure this project has is whether one run reached further than
/// the last, and a value that changed every run would be a difference between two runs that
/// meant nothing (the same argument D256 makes for process time).
///
/// Unique within a run, which is what a guest using one as a key needs. Repeated across runs,
/// which no guest here can observe and a person comparing two traces very much can.
///
/// The version and variant bits are set so it **is** a well-formed version-4 UUID: a guest that
/// checks them gets the right answer, and one that does not is unaffected (D562).
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
    // Version 4 in the high nibble of byte 6, and the RFC 4122 variant in the top two bits of
    // byte 8 - the two fields a reader checks to decide it is looking at a UUID at all.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_bytes(args[0], &bytes) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Words in a `sigset_t`, which is `__uint32_t __bits[4]` on a FreeBSD-derived system.
const SIGSET_WORDS: u64 = 2;

/// Which word of a set a signal number lives in, and its bit.
///
/// Signals are numbered from one, so signal *n* is bit *n-1*. Returns `None` for anything
/// outside the set, which is what makes an out-of-range signal an error rather than a
/// write past the end of the guest's object.
fn signal_bit(signal: u64) -> Option<(u64, u64)> {
    /// Signals a `sigset_t` can hold: four 32-bit words.
    const MAX_SIGNAL: u64 = 128;

    if signal == 0 || signal > MAX_SIGNAL {
        return None;
    }
    let index = signal - 1;
    Some((index / 64, 1_u64 << (index % 64)))
}

/// `posix_sigfillset(set)` - every signal present.
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

/// `posix_sigismember(set, signal)` - one when present, zero when not.
///
/// **This one had to exist for `sigemptyset` to be believed.** Unimplemented, it answered
/// the placeholder error code - which is non-zero, which a caller reads as *yes* - so a set
/// that had just been emptied reported every signal still in it, and the failure was
/// attributed to the function that did the emptying (D271).
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
/// **Releases the guest mutex around the wait and retakes it after**, which is what the
/// interface promises. It is not atomic here, because the two objects are independent in
/// this crate - a signal arriving in the gap is lost where on the platform it would not
/// be. Recorded on the entry rather than left for a hang to reveal.
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

/// `pthread_rwlock_init(lock, attr)` - the POSIX spelling, which **takes no name**.
///
/// A separate entry point rather than the same one, because the difference between the two
/// is arity and an implementation cannot see its own. Bound to both, this read `args[2]` on
/// a two-argument call - whatever the guest left in `rdx` - and dereferenced it. The probe
/// handed it `0x3f` and killed the emulator, in the emulator's own code, on a call that was
/// perfectly well formed (D282).
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
/// A refusal maps to `Busy` rather than an argument error: a lock somebody else holds is
/// the ordinary outcome of a try, not a misuse of the call.
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
/// **Deliberately not [`acquired`]**, which the four acquiring calls share. There
/// `Some(false)` means somebody else holds the lock, and `Busy` is the ordinary outcome of
/// asking. Here it can only mean the caller released a lock nobody held - the release path
/// has no contention branch - so answering `Busy` would name a cause this branch did not
/// determine, and would send a guest into a retry loop over a bug in itself.
///
/// `scePthreadMutexUnlock` already answers this way for the same situation, and the two
/// disagreeing about the same guest mistake is worse than either answer alone.
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

/// `pthread_barrier_init(barrier, attr, count)` - the POSIX spelling, which takes no name.
///
/// A separate entry point rather than the same one, for the reason
/// [`posix_pthread_rwlock_init`] is: the difference between the two spellings is arity, and an
/// implementation cannot see its own. Bound to both, this would read `args[3]` on a
/// three-argument call - whatever the guest happened to leave in `rcx` - and read a name out
/// of it. That is the fault D385 cost an evening to, and it is why batch 4 refused to delegate
/// this one to its vendor twin (worklog 305).
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

/// `sceKernelClockGettime(clock_id, timespec)` - the vendor-named form of `clock_gettime`.
///
/// # What is shared and what is not
///
/// The clock families, the identifiers and the monotonic origin are `orbistoun_hle::clocks`,
/// which POSIX `clock_gettime` also reads. That is deliberate: **a monotonic clock read through
/// two names must not answer two different elapsed times**, and it would have, had the thirty
/// lines been copied into this crate instead (D536).
///
/// What differs is failure, as it does for `stat` and `pread` (D525, D526). POSIX answers `-1`;
/// a `sceKernel*` call answers a `0x8002_00xx` vendor code, and a caller testing for that would
/// not recognise `-1`.
///
/// `EINVAL` for a clock this cannot answer, which is what the refusal *is*: the identifier
/// named a clock - the per-process CPU time, `CLOCK_UPTIME` - that orbistoun has no honest
/// source for. **Which errno the console answers is unmeasured**, so the test pins the family
/// and the sign, not a code no run has established.
fn kernel_clock_gettime(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some((seconds, nanos)) = orbistoun_hle::clocks::reading(args[0] as i64) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], seconds) } {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    // The second field of the structure the caller described, eight bytes on.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(args[1].saturating_add(8), nanos) } {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    OK
}

/// `sceKernelCreateEqueue(out, name)`.
///
/// # The shape came from the run
///
/// `arg0` is writable and in the guest's own mapping arena; `arg1` is a name the guest wrote
/// itself - PPSA02664 passes `"eq to wait flip"` and `"flip equeu"`, which say what the queue is
/// for. The third register holds `0x7fff_0001`, orbistoun's own placeholder left by an earlier
/// stub, and that is what fixes the arity at two rather than a guess (D516, D524).
///
/// Same shape as [`kernel_create_event_flag`], deliberately: a null out-parameter is refused, the
/// handle is written through it, and `OK` is answered. Unimplemented, **the out-parameter was
/// never written**, so the guest's handle kept whatever it held - which is how every later call
/// against that queue was handed a value orbistoun never gave out.
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
/// Registers a user event against a queue. PPSA02664 registers two, identifiers `0x1` and `0x2`,
/// against the queue it created for flips.
///
/// **The handle is checked and a bad one is refused**, with the same vendor `ESRCH` the event-flag
/// family answers - measured by obSCEne's `015-sync/event-flag-rejects-bad-handle` (`0x80020003`),
/// not the placeholder a guest would fail to recognise (D125). A registration against a queue
/// nobody created is a guest holding a handle orbistoun never issued, and reporting success for it
/// would promise delivery from a queue that does not exist.
///
/// **Delivery exists now, and the note that said it did not was stale.** This said
/// `sceKernelWaitEqueue` was *"called zero times by this title since the flip count stopped
/// lying (D516)"*. The title calls it **839 times** in an honest run and 12,924 past the shader
/// wall, waiting on a completion nothing posted - see [`kernel_wait_equeue`] (D560).
fn kernel_add_user_event_edge(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if sync::register_event(args[0], args[1]) {
        OK
    } else {
        u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw())
    }
}

/// `scePthreadGetschedparam(thread, policy, param)`.
///
/// # Four-byte writes, and the guest is why
///
/// PPSA02664 passes `policy` at `0x…c86c` and `param` at `0x…c868` - **four bytes apart**. An
/// eight-byte write to either takes the other with it, which is D272's lesson arriving in the
/// one place it is unmissable: the two out-parameters are adjacent by construction, because a
/// `sched_param` is a single `int` and the caller put both on its stack together.
///
/// Hands back what was set. Where nothing set a policy it is zero, and **zero here means "nobody
/// said" rather than a known default** - no lawful source gives the vendor's default policy, and
/// inventing one would be a constant a guest could branch on (D561).
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
/// Stores both, so [`pthread_getschedparam`] hands back what a caller set - which is the whole
/// of what a setter promises, and is what D523 said would be needed the moment anything read one
/// back. **Neither is applied**: which host thread runs when is the host scheduler's to decide,
/// exactly as `scePthreadAttrSetaffinity` already records for affinity.
///
/// The policy is stored verbatim. PPSA02664 passes `0x4000`, which is no POSIX constant, so
/// nothing here knows what it selects and nothing here pretends to.
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
/// The priority alone, so **the policy is left as it was** rather than reset to a value the
/// caller never mentioned - which is why [`thread::set_scheduling`] takes an optional one.
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
/// **Worth more than its one call suggests.** The name is what a trace shows in place of a
/// handle, so a title renaming a thread is telling the reader what that thread is for - the same
/// reason `sceKernelCreateEqueue`'s names made the event queues readable (D522, D524).
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
/// POSIX, and one of the few in this family that genuinely has a POSIX analogue of the same name
/// - D540 is the warning against assuming that, and it does not apply here.
///
/// **Stored and not acted on.** Orbistoun cancels no threads, so there is nothing for the state
/// to gate; what a caller is promised is the previous value, and it gets it. PPSA02664 passes
/// `1` - disable on FreeBSD - around a section it does not want interrupted, which costs nothing
/// to honour when nothing interrupts.
///
/// The out-parameter is optional, as POSIX allows, and **four bytes**: it is an `int` (D272).
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
/// Shared by the two calls that can run out of patience - the event queue and the event flag -
/// because a timeout is one condition and two constants for it would drift. The value is the
/// published errno, not a measured one: no capture has caught either call timing out (D613).
const ETIMEDOUT: u32 = 60;
/// `sceKernelWaitEqueue(equeue, events, wanted, delivered, timeout)`.
///
/// # The shape came from the run, and the layout from FreeBSD
///
/// Arity five, and the argument roles are the guest's own: `arg0` a queue handle orbistoun
/// issued, `arg1` a buffer it had just zeroed, `arg2` the number wanted (PPSA02664 passes 1),
/// `arg3` an out-count, `arg4` a microsecond timeout pointer - NULL in every observed call.
/// The delivered structure is `struct kevent`; see [`sync::PendingEvent`] for the layout and for
/// which part of it is citable and which is not.
///
/// # It does not block, and that is a decision rather than an omission
///
/// A NULL timeout means an indefinite wait on hardware. Blocking here would hand the emulator a
/// way to stop for ever with nothing able to wake it - the guest's own thread is the one that
/// would have posted the completion in most of the paths this title takes - and a hang destroys
/// a run's evidence where a busy loop merely costs time. So a queue with nothing ready reports
/// **zero delivered**, which is what `kevent` reports on a timeout anyway, and the guest polls
/// again.
///
/// That is honest but not free: it is why the call count is five figures. If a title ever needs a
/// real block, it needs a poster on another thread first, and that is the thing to build (D560).
///
/// **A bad handle is refused** with the vendor `ESRCH` the rest of this family answers -
/// `0x80020003`, measured by obSCEne's `015-sync/event-flag-rejects-bad-handle` - rather than a
/// placeholder a guest would not recognise (D125).
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

    // **It waits now.** `arg4` is a pointer to a microsecond timeout, and null means wait
    // indefinitely - the shape `kevent(2)` has. This collected whatever happened to be pending
    // and answered success either way, so a guest looping until an event arrives never blocked:
    // 3,853 waits against 44 flips in one run, every one of them reading an event array nothing
    // had written. An out-parameter left as the caller set it, under a success code (D613).
    let until = if args[4] == 0 {
        sync::Blocking::Forever
    } else {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        match unsafe { guest::read_u64(args[4]) } {
            Some(micros) => sync::Blocking::Until(
                std::time::Instant::now() + std::time::Duration::from_micros(micros),
            ),
            // A timeout pointer that does not read is not a zero timeout. Refusing says so.
            None => return u64::from(GuestError::InvalidArgument.as_raw()),
        }
    };
    let Some(events) = sync::wait_events(args[0], wanted, until) else {
        // Nothing arrived within the caller's patience. **Not success with nothing written** -
        // that is the answer this call used to give, and it is the reason a guest spun.
        return u64::from(GuestError::vendor(ETIMEDOUT).as_raw());
    };
    for (index, event) in events.iter().enumerate() {
        let at = args[1].saturating_add((index * sync::EVENT_BYTES) as u64);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        if !unsafe { guest::write_bytes(at, &event.to_bytes()) } {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    // The count is written through `arg3` and the return is a status - the shape the argument
    // roles establish. Zero delivered is written as zero rather than skipped: a caller reading
    // a stale count would act on an event it was never given.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[3] != 0 && !unsafe { guest::write_bytes(args[3], &(events.len() as u32).to_le_bytes()) }
    {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Whether a wait mode asks for every bit of the pattern, or refuses to say.
///
/// `Some(true)` for `and`, `Some(false)` for `or`, `None` for a mode naming neither - which a
/// console answers `0x80020016` to rather than picking one (D610).
///
/// The higher bits are the clear-on-match behaviour and are not read here: nothing has
/// measured what they do, and a poll that did not match clears nothing under any of them.
const fn wait_mode(mode: u64) -> Option<bool> {
    /// Every bit of the pattern must be present.
    const WAIT_AND: u64 = 0x01;
    /// Any bit of the pattern will do.
    const WAIT_OR: u64 = 0x02;

    match mode & (WAIT_AND | WAIT_OR) {
        WAIT_AND => Some(true),
        WAIT_OR => Some(false),
        // Zero names neither. `0x03` names both, which nothing has measured - refusing it is
        // the honest answer rather than choosing whichever this file happens to test first.
        _ => None,
    }
}
/// `sceKernelPollEventFlag(flag, pattern, mode, result, timeout)`.
///
/// **A miss is not an error.** Polling asks whether the pattern is set right now, and
/// answering an argument error when it is not would make a guest read an ordinary poll as
/// a broken handle.
fn kernel_poll_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // **A mode naming neither is an argument error, and this used to read it as `or`.**
    // `016-syncbounds/event-flag-waitmode` polled one bit of a two-bit pattern under four
    // modes: `0x00` answered `0x80020016`, `0x01` and `0x11` answered busy, `0x02` answered
    // ok. Every one but the first already matched; the first matched because `mode & 0x01`
    // is zero for `0x00` as well as for `0x02`, so "no mode at all" and "either will do"
    // were the same branch (D610).
    // **The handle first, then the mode**, because two measurements together fix the order: a
    // poll on a handle the console never issued answers `0x80020003` with the mode invalid as
    // well, and a bad mode on a real handle answers `0x80020016`. Checking the mode first is
    // right in isolation and wrong for exactly one of the two cases (D610).
    if !sync::event_flag_exists(args[0]) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    let Some(all) = wait_mode(args[2]) else {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    };
    let Some(outcome) = sync::event_flag_poll(args[0], args[1], all) else {
        // ESRCH, the vendor code obSCEne's `015-sync/event-flag-rejects-bad-handle` measured
        // (`0x80020003`), not the `0x7fff…` placeholder a guest would fail to recognise (D125).
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
/// outlives it answers the vendor `ETIMEDOUT`.
///
/// The blocking counterpart of [`kernel_poll_event_flag`], and its absence was the wall PPSA04263
/// spun against: unimplemented, the guest called it 304,583 times against a placeholder instead of
/// parking on the event another thread would set (worklog 288).
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
    // The same refusal its polling twin makes, for the same reason: the two differ in whether
    // they wait and in nothing else, so a mode one refuses and the other reads as `or` would be
    // a distinction the platform does not make (D610).
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
        // ESRCH for a bad handle, as the rest of the event-flag family answers (measured on Poll).
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
        // ESRCH for a bad handle, the code its sibling `PollEventFlag` was measured returning
        // (the whole family fails a handle lookup the same way).
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw()),
    }
}

/// `sceKernelClearEventFlag(flag, pattern)`.
fn kernel_clear_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sync::event_flag_clear(args[0], args[1]) {
        Some(true) => OK,
        // ESRCH for a bad handle, as the rest of the event-flag family (measured on Poll).
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw()),
    }
}

/// `sceKernelDeleteEventFlag(flag)`.
fn kernel_delete_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if sync::event_flag_destroy(args[0]) {
        OK
    } else {
        // ESRCH for a bad handle, as the rest of the event-flag family (measured on Poll).
        u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw())
    }
}

/// Reads a semaphore handle, which is a 32-bit identifier rather than a pointer.
fn sema_at(raw: u64) -> Option<sync::SemaphoreHandle> {
    i32::try_from(raw as i64)
        .ok()
        .filter(|h| *h != sync::NO_SEMAPHORE)
}

/// `sceKernelPollSema(semaphore, need)` - takes `need` without waiting, or none at all.
///
/// # What the console answered, and what this used to
///
/// `016-syncbounds/sema-count` measured all four interesting cases, and this ignored `need`
/// entirely - it took one whatever was asked for. Two of the four were wrong (D610):
///
/// | asked | available | console | this, before |
/// |---|---|---|---|
/// | 0 | 0 | `0x80020016` invalid | `0x80020010` busy |
/// | 1 | 1 | ok | ok |
/// | 2 | 1 | `0x80020010` busy | **ok**, having taken one |
/// | 2 | 3 | ok | ok, having taken one |
///
/// The third row is the one that matters: a caller told it holds two when it holds one goes on
/// to release two, and the count runs away upward from there.
fn kernel_poll_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // **The handle first**, matching the one ordering the platform has actually shown: a poll
    // on an event flag the console never issued answers the bad-handle code even when the mode
    // is also wrong. Nothing measures a bad handle *and* a count of zero together, so this is
    // consistency with the measured case rather than a measurement of its own (D610).
    let Some(handle) = sema_at(args[0]) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    // **Zero is an argument error, not a trivially satisfied request.** The console answers
    // `0x80020016` to it, which is the only way a caller finds out it asked for nothing - and
    // `wait_while` would treat `*c < 0` as already satisfied and answer success.
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
/// **The same `need` its polling twin takes.** Nothing measured this one - a check that blocks
/// forever is not something a conformance probe can run - but the two calls differ in whether
/// they wait and in nothing else, so a count honoured by one and ignored by the other would be
/// a distinction neither the platform nor the name makes (D610).
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

/// `sceKernelSyncOnAddressWait(address, value)` - sleep while the 64-bit word at `address`
/// holds `value`, until a wake on the same address.
///
/// **The platform's futex wait, and the function 78% of every call this project had recorded
/// went to** (D566). Unimplemented it answered the placeholder, and PPSA25872 asked again 2.27
/// million times in twelve seconds - on thirteen distinct words, one call site, thirteen threads
/// created - because a wait that returns without waiting is a busy loop by construction.
///
/// Modelled on FreeBSD's `_umtx_op(2)` with `UMTX_OP_WAIT`, the lawful reference for the shape:
/// compare, sleep only on equality, and answer success at once when the word already differs.
/// The caller re-reads the word afterwards - that is the contract, because the wake may have
/// come first. The compare happens under the wait queue's lock ([`sync::wait_on_address`]), so
/// a wake landing between the guest's write and its wake call cannot be lost.
///
/// Sixty-four bits, because in the 12.40 layout this entry point is shared with the `Wait64`
/// spelling while `Wait32` has its own (`orbistoun-firmware/data/libkernel-vaddrs.txt`).
///
/// **A non-zero third register is refused, not waited on.** Every observed call passes zero
/// there. It may be a timeout in a unit nothing here has established, and modelling it as
/// "forever" would be a plausible answer to a question that was not read (principle 3); the
/// placeholder is loud in a trace and names the call to look at.
fn sync_on_address_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, expected, third) = (args[0], args[1], args[2]);
    if address == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if third != 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // **Reachable while it sleeps here.** A signal raised on a thread parked in this wait runs
    // its handler and the thread goes back to sleep; raised on a thread that is anywhere else it
    // is refused rather than lost, so the flag has to be set exactly around the sleep (D652).
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
        // Unreachable under `Forever`, and kept so the match stays total for the day a timeout
        // is modelled: the published errno under the measured vendor base.
        Some(sync::AddressWait::TimedOut) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw())
        }
        None => u64::from(GuestError::InvalidArgument.as_raw()),
    }
}

/// `sceKernelSyncOnAddressWake(address, count)` - wake up to `count` threads asleep on `address`.
///
/// FreeBSD `_umtx_op(2)` with `UMTX_OP_WAKE`. Success whether or not anybody was asleep: a wake
/// racing its wait is the ordinary case for a futex, whose word carries the state, and a wake
/// with nobody listening is deliberately not remembered ([`sync::wake_on_address`]). PPSA25872
/// calls it thirteen times, once per thread it created, on words 0x30 below the ones its waits
/// name - which of its threads sleep on which word is not something a trace records.
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

/// The POSIX unnamed-semaphore family - `sem_init` and the calls a guest's own concurrency
/// primitives are built on.
///
/// # Why these, and why the handle lives in the `sem_t`
///
/// A `sem_t` is the guest's own storage; POSIX `sem_init` initialises *in place*, where the
/// vendor `sceKernelCreateSema` writes a handle to a separate out-pointer. So this keeps the
/// same "handle stored in the object" model the mutex and condition variable already use
/// (`cond_at` reads a handle back out of the guest's object): `sem_init` creates the host
/// semaphore and writes its handle into the `sem_t`, and the rest read it back.
///
/// PPSA21564's engine builds its `Cond` on a semaphore, and unimplemented `sem_init` answered
/// the placeholder - which the engine asserted was zero (`Cond.cpp:212: rc == 0`) and then
/// aborted (D455). The success path answering zero is the whole point.
///
/// Reference: POSIX.1-2008 `sem_init`/`sem_wait`/`sem_trywait`/`sem_post`/`sem_destroy`; the
/// host primitives are `orbistoun-kernel`'s own `sync` semaphores, shared with the vendor
/// calls above.
fn posix_sema_at(sem: u64) -> Option<sync::SemaphoreHandle> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    sema_at(unsafe { guest::read_u64(sem) }?)
}

/// `sem_init(sem, pshared, value)` - POSIX unnamed semaphore, initialised in place.
fn sem_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (sem, value) = (args[0], args[2]);
    if sem == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let initial = u32::try_from(value).unwrap_or(0);
    // POSIX bounds a semaphore only by `SEM_VALUE_MAX`; the largest ceiling this pool accepts
    // stands in for "effectively unbounded", so a `sem_post` is never refused for a ceiling
    // the guest never set.
    let handle = sync::create_semaphore(initial, u32::MAX, "");
    // Stored as a word and read back by `posix_sema_at`; a fresh handle is a small positive
    // id, so this round-trips through `sema_at`'s `i32` unchanged.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if !unsafe { guest::write_u64(sem, u64::try_from(handle).unwrap_or(0)) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sem_wait(sem)` - take one, waiting for it.
fn sem_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, sync::Blocking::Forever)) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_trywait(sem)` - take one only if it is available.
fn sem_trywait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, sync::Blocking::Never)) {
        Some(true) => OK,
        // Available-but-empty and bad-handle are both non-zero, which is what a caller that
        // tests the result against zero needs; POSIX distinguishes them by `errno`, which
        // this project does not invent (see the `orbistoun-posix` note).
        _ => u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw()),
    }
}

/// `sem_post(sem)` - give one back.
fn sem_post(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).map(|h| sync::semaphore_signal(h, 1)) {
        Some(Some(true)) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_destroy(sem)` - retire the semaphore a `sem_t` names.
fn sem_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if posix_sema_at(args[0]).is_some_and(sync::semaphore_destroy) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// Where a mutex attribute object keeps its type, and where its protocol goes.
///
/// **This crate defines the layout, and that is defensible only because nothing else reads
/// it.** The real one is not known from any lawful source; what is known is that the guest
/// allocates the object and hands it to these calls and to `scePthreadMutexInit`, all of
/// which are here. A guest that inspected the bytes itself would see an invention - which
/// is why the fields are the two this crate is told about and nothing more (D272).
const ATTR_TYPE: u64 = 0;
/// Offset of the protocol, one word after the type.
const ATTR_PROTOCOL: u64 = 8;

/// Whether a mutex or condition variable is shared between processes.
///
/// **Stored and returned, not honoured** - and correctly so: the attribute object's contract
/// is that a getter answers what its setter wrote (D272), while whether a *lock* acts on the
/// value is the lock's business. There is one process here, so process-shared has nothing to
/// mean; `scePthreadMutexInit` already declares that it does not read the attribute block.
const MUTEXATTR_PSHARED: u64 = 16;

/// A mutex's priority ceiling, stored on the same terms as [`MUTEXATTR_PSHARED`].
const MUTEXATTR_PRIOCEILING: u64 = 24;

/// Which clock a condition variable's timed waits are measured against.
const CONDATTR_CLOCK: u64 = 16;

/// Whether a condition variable is shared between processes, on the same terms.
const CONDATTR_PSHARED: u64 = 24;

/// `scePthreadCondattrInit(attr)` - allocates a condition-variable attribute object.
///
/// The same pointer-to-pointer shape as every attribute in this family (D272): the guest
/// declares one null and passes its address, so this hands back the address of a fresh block.
/// The object carries no field anything reads yet - a condattr holds a clock and a pshared
/// flag, and nothing observed sets either - so an empty block is one a matching `CondInit`
/// accepts, and its zeroes read as the default clock and process-private, which is what a
/// freshly initialised condattr holds.
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

/// `scePthreadMutexattrInit(attr)` - allocates an attribute object and hands back its
/// address.
///
/// **The argument is a pointer to a pointer**, as it is for every object in this family:
/// the guest declares `ScePthreadMutexattr attr = NULL` and passes `&attr`. Treating that
/// as the object itself meant `Settype` overwrote the guest's pointer variable with a type
/// value, and the round-trip that followed spun for fourteen million calls (D272).
fn pthread_mutexattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // From the one region every guest-visible handle comes from, so it repeats (D584).
    let handle = orbistoun_mem::blocks::block(4);
    // **Not zero, which is what an empty block would have said.** A conformance run read the
    // type back out of a freshly initialised attribute on a target console and got 1, so a
    // guest that initialises an attribute and asks what it holds was being told the wrong
    // thing here - and a guest that *acts* on the answer builds a different kind of lock
    // (D398).
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
/// Measured: a target console answered `scePthreadMutexattrGettype` with 1 on an attribute
/// nothing had set. The same run showed the types are not interchangeable - one of them
/// re-acquires without blocking and the others refuse - so this is a value with consequences
/// rather than a tag (D398).
const DEFAULT_MUTEX_TYPE: u64 = 1;

/// Resolves the attribute object a guest pointer refers to.
fn attr_at(pointer: u64) -> Option<u64> {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let handle = unsafe { guest::read_u64(pointer) }?;
    (handle != 0).then_some(handle)
}

/// `scePthreadMutexattrSettype(attr, type)` - and it is now actually stored.
///
/// It used to accept the call and write nothing, so a `Gettype` counterpart read whatever
/// the guest's stack held. That is the D171 shape - an out-parameter left untouched - and
/// the conformance probe named it exactly: *the attribute object is inert* (D272).
fn pthread_mutexattr_settype(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // **Type zero is refused, and that is measured.** A conformance run set each of 0..4 on one
    // attribute object and read it back: 1, 2, 3 and 4 round-trip, and 0 does not - the probe
    // records its refusal marker rather than a value. Storing it here made a guest that asked
    // for a type the console will not give it carry on believing it had one.
    //
    // **The refusal is measured; the code is not.** No run recorded what `Settype` answers for
    // zero, so this is orbistoun's own placeholder rather than a vendor code invented to fill
    // the gap - loud in a trace, and impossible to mistake for a real errno (D398, D509).
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
/// The same pool as [`allocate_main_direct_memory`], with a search range in front. The
/// range is honoured as a lower bound only: the pool allocates upward from `searchStart`,
/// and an upper bound it cannot satisfy shows up as the allocation failing rather than as
/// an address outside the range (D273).
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
    // Refused after the fact rather than before: the pool decides where it can fit a
    // request, and a range this cannot satisfy is a real out-of-memory for that range.
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

/// `sceKernelReleaseDirectMemory(start, len)` - returns a span to the pool.
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

/// `sceKernelMunmap(address, len)` - unmaps a span from the guest's address space.
///
/// **Refuses a null address**, which is the case the probe checks from the failure side:
/// answering success for an unmap of nothing would tell a guest its memory was released
/// when it was not.
fn munmap(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, len) = (args[0], args[1]);
    if address == 0 || len == 0 {
        // The vendor `EINVAL`, not the placeholder: obSCEne's `020-memory/unmap-rejects-null`
        // measured `sceKernelMunmap(0)` answering `0x80020016` on hardware, and a guest that checks
        // for that exact code would never match the `0x7fff…` this used to return (D125).
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    // The host reservation stays: orbistoun maps the guest's whole span once at load, and
    // releasing a piece would hole an address space the guest believes contiguous (D273). The
    // alias goes, so a later map of the same physical memory is a new mapping.
    forget_mapped(address, len);
    OK
}

/// `mmap(addr, len, prot, flags, fd, offset)` - maps pages of memory into guest address space.
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

/// `sceKernelReserveVirtualRange(addr, len, flags, alignment)` - reserve a span of address space.
///
/// `addr` is a `void **`: the value it points at going in is a hint (zero means "anywhere"), and
/// the base actually reserved is written back through it. A guest reserves a range so it can map
/// memory into it, and it reads the base out of `*addr` to do so.
///
/// **The write-back is the whole call.** Stranded on the stub-everything placeholder this wrote
/// nothing, the guest read an uninitialised stack slot as the base - often zero - and wrote through
/// it into a fault with no relation to the reservation (the D125 shape). Here the range is reserved
/// against the same address space `mmap` uses and the base is written where the guest will look for
/// it. It is reserved *and* backed, one step rather than the console's reserve-then-map two, because
/// orbistoun hands out backed memory - a guest that writes into what it reserved finds memory there.
fn reserve_virtual_range(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr_out, len, alignment) = (args[0], args[1], args[3]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if addr_out == 0 || len == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    // **A fresh range this owns, not the guest's hint.** The hint names where the guest would like
    // the range; honouring a low, specific one put the reservation where the guest's own allocator
    // then wrote just outside it. orbistoun hands out address space from its own high arena.
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
    // **Honour the hint, fall back on conflict.** A guest asks for a *specific* address because its
    // own allocator addresses the range from there - `0x5000_0000_0000` for the two titles measured
    // - so reserving elsewhere and writing a different base back strands it. The hint is tried first;
    // only if it is unavailable does the mapping arena's counter supply one, retried past a conflict
    // (the counter steps an address, not the map, so a base it hands back can already be held).
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
    // The reserved base, written back through the `void **` the guest passed - the documented shape,
    // status returned separately as success.
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_u64(addr_out, base) } {
        OK
    } else {
        vendor(orbistoun_core::errno::INVALID)
    }
}

/// `sceKernelVirtualQuery(addr, flags, info, info_size)` - describe the mapping that holds `addr`.
///
/// A guest walks its address space with this - "what is mapped here, and how far does it run" -
/// before it decides where to place something. Stubbed, it answered a placeholder and the guest
/// read a mapping that was not there, computing a bad address and writing through it (the D125 shape,
/// and the wall past `sceKernelReserveVirtualRange` for the titles measured).
///
/// The address space already holds the regions `mmap` and the reservation reserved, so this finds
/// the one containing `addr` and reports **where it starts and ends** - the two fields a guest reads
/// to bound a mapping, at offsets 0 and 8. The rest of `SceKernelVirtualQueryInfo` - a protection, a
/// type, a name - has no lawful layout here and is left as the caller prepared it, exactly as
/// `sceVideoOutGetResolutionStatus` writes only the fields it can cite. An address in no region is
/// answered with the code the console answers for one, not a fabricated mapping.
fn virtual_query(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (addr, info, size) = (args[0], args[2], args[3]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if info == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    // An address in no region is `EACCES` (`0x8002000d`), not `ENOENT` - measured: obSCEne
    // `020-memory/virtual-query-unmapped`.
    let Some(region) = query_region(addr) else {
        return vendor(orbistoun_core::errno::DENIED);
    };
    // The whole structure, zeros included, as the console writes it: the probe pre-filled 72
    // bytes with `0xAA` and every one changed. A field left as the caller prepared it is a value
    // the console never hands back.
    let encoded = region.encode();
    let len = usize::try_from(size).map_or(encoded.len(), |s| s.min(encoded.len()));
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_bytes(info, &encoded[..len]) } {
        OK
    } else {
        vendor(orbistoun_core::errno::INVALID)
    }
}

/// How many bytes `SceKernelVirtualQueryInfo` is - measured: obSCEne
/// `020-memory/virtual-query-{mapped,text,stack}`, the same extent for all three.
const VIRTUAL_QUERY_INFO_BYTES: usize = 72;

/// Where each field of `SceKernelVirtualQueryInfo` sits, from the console's own bytes.
///
/// Measured in obSCEne's `020-memory/virtual-query-{mapped,text,stack}`: start and end at 0 and 8,
/// a direct mapping's physical offset at 0x10 (`0x2a20000` for its mapping, 0 for code and stack),
/// the protection as a 32-bit value at 0x18 (3 for read-write, 4 for execute-only text), a 32-bit
/// memory type at 0x1c (0 in all three), a flag byte at 0x20, and a NUL-terminated name from 0x21.
mod vq {
    pub(super) const START: usize = 0x00;
    pub(super) const END: usize = 0x08;
    pub(super) const OFFSET: usize = 0x10;
    pub(super) const PROTECTION: usize = 0x18;
    pub(super) const FLAGS: usize = 0x20;
    pub(super) const NAME: usize = 0x21;
    /// The name's room: from 0x21 to the end of the structure, keeping its terminator.
    pub(super) const NAME_BYTES: usize = super::VIRTUAL_QUERY_INFO_BYTES - NAME;

    /// The flag bits, read off three measured regions: text `0x11`, a direct mapping `0x12`, the
    /// main stack `0x15`. Committed is set on all three; direct only on the direct mapping; stack
    /// only on the stack; and flexible on the two that are not direct.
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
    /// CPU read 1, write 2, execute 4 - the values the console reported (3 and 4).
    protection: u32,
    flags: u8,
    /// The console's name for the region, where one was measured; empty otherwise.
    name: &'static str,
}

impl QueryRegion {
    /// The 72 bytes the console writes, zero wherever this region has nothing to say.
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

/// The protection each guest range was last *asked* for, as `(base, len, prot)`, newest last.
///
/// What the guest asked, not what orbistoun granted: `protection_from_guest` widens the grant for
/// the host's sake, and a query answered with the grant tells the guest a range is set up when it
/// is not, so it skips the `sceKernelMprotect` that sets it up.
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

/// The console's protection field for a range asked for with `prot`: the request, as asked.
///
/// Measured for the CPU bits (obSCEne `020-memory/virtual-query-{mapped,text}`). Guest-observed for
/// the GPU bits: PPSA25872 compares this field with the `0xf2` it wants and calls `sceKernelMprotect`
/// only when they differ (`image+0x1ae28b4`), which CPU bits alone could never equal.
const fn reported_protection(prot: u64) -> u32 {
    prot as u32
}

/// Whether a region starting at `start` is a reservation `sceKernelReserveVirtualRange` made.
fn is_reservation(start: u64) -> bool {
    reservations()
        .lock()
        .is_ok_and(|reserved| reserved.iter().any(|&(base, _)| base == start))
}

/// The console's protection value for a region's access.
const fn query_protection(protection: orbistoun_mem::Protection) -> u32 {
    (protection.read as u32) | ((protection.write as u32) << 1) | ((protection.execute as u32) << 2)
}

/// Describes the region holding `addr`, from every place one is recorded.
///
/// **What is measured and what is not, field by field.** A direct mapping (one this crate
/// aliased to a physical offset) is answered in full, as `020-memory/virtual-query-mapped` was:
/// its offset, its protection, direct and committed, and the name `anon` a mapping given none
/// carries. The main stack is answered as `virtual-query-stack` was. Any other mapping this crate
/// placed is flexible memory, and gets its protection and the flexible and committed bits - the
/// bits are the measured ones, their application to flexible mappings is read off the text and
/// stack samples rather than measured on one. Regions the loader noted (the image, modules) and
/// thread stacks are answered with their bounds and zeros: their per-segment protection and names
/// are not measured here, and zero is what the console writes where it has nothing to say.
fn query_region(addr: u64) -> Option<QueryRegion> {
    let within = |base: u64, len: u64| addr >= base && addr < base.saturating_add(len);
    if let Ok(space) = mappings().lock() {
        if let Some(region) = space.regions().iter().find(|r| within(r.base, r.len)) {
            let (start, end) = (region.base, region.base.saturating_add(region.len));
            // What the guest asked for, where the console's answer to that is measured; 0 where
            // nothing was asked or the answer is not measured - never orbistoun's own grant.
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
                // A reservation nothing has been mapped into: address space only, no access, not
                // committed. Assumed from what reserving means and from the guest, which fills such
                // a range only when the query says it is empty.
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

/// `sceKernelSetVirtualRangeName(start, len, name)` - attaches a debug name to a virtual range.
///
/// **Accepted and answered `OK`, with nothing stored.** The name is advisory: it exists for host
/// profiling and crash tools to label a range, and no guest-readable interface hands it back, so a
/// guest cannot observe whether it was kept. Recording it would buy nothing the guest can see, and
/// refusing it would fail a call that succeeds on hardware - the same shape as `sceKernelMunmap`
/// answering `OK` without tearing a reservation down (D273). A null name or a zero-length range is
/// the one thing refused, being a malformed request rather than an unobservable one.
fn set_virtual_range_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (start, len, name) = (args[0], args[1], args[2]);
    if start == 0 || len == 0 || name == 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    OK
}

/// `sceKernelMprotect(addr, len, prot)` - change the protection of a range already reserved.
///
/// A guest reserves a span, then calls this to make it usable before handing it to its own
/// allocator. Stubbed, it answered a placeholder error, and the guest that reads the return -
/// PPSA02664 does - concluded the range was not usable and handed its `tlsf` allocator a pool
/// size of zero, which `tlsf_add_pool` rejects (`size must be between 0x28 and 0x100000000`).
/// With no pool, the next allocation returned null and the guest wrote through it: a fault at
/// its allocator (`image+0xafcc08`) with no visible relation to the missing `mprotect`.
///
/// **Two places hold a guest region, and both are consulted.** The address space this crate
/// hands mappings out of is one; the other is the set of regions somebody else placed and
/// reported here - a module's pages, the executable's image (D446, D489). A guest re-protecting
/// its own module is asking about the second, and refusing it because the first had never heard
/// of it was a wall: PPSA03416 asks for 256 MiB across its own module, checks the answer, and on
/// the failure path calls a routine its compiler believes never returns (D577).
///
/// A typo still cannot re-protect this process's own code, which is what
/// `AddressSpace::protect` refusing an unowned range was for: the range has to lie wholly inside
/// **one** region this process placed for the guest, and a range covered by nothing is refused
/// exactly as before.
///
/// Two honest simplifications remain, both from orbistoun's identity-mapped model rather than
/// guessed:
///
/// - **The range is kept readable.** [`protection_from_guest`] reads the low bits as POSIX
///   `PROT_*`, and the value a guest passes here (`0xf2` for PPSA02664) has the write bit without
///   the read bit - which would drop a range the guest is actively managing below what its own
///   allocator needs to read back. A managed range must stay readable (the same reasoning
///   `protection_from_guest` already applies to a request naming no access at all), so read is
///   forced on.
/// - **The high bits are ignored, not decoded.** Those name GPU access and cache behaviour, for
///   which orbistoun has no model and no citable layout; inventing one is the error D008 forbids.
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

    // **Which authority covers the range is decided before either is asked to act.**
    // `AddressSpace::protect` records a reservation failure for the report when it is handed a
    // range it does not own (worklog 284), and a range this crate never mapped is not a failure
    // here - it is the other case below. Asking it first and falling through would file a
    // diagnostic about every successful protection of a module's own memory.
    //
    // The guard is scoped so the lock is released before `region_covering`, which takes it
    // again through `region_containing`.
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
            // The range is this crate's own and the host still refused it.
            Err(_) => vendor(orbistoun_core::errno::INVALID),
        };
    }

    // Not a mapping this crate handed out - and for a guest re-protecting its own module that
    // is the ordinary case, not an error. A module's pages are placed elsewhere and reported
    // here (D446, D489), which is the same set of regions `sceKernelVirtualQuery` and
    // `sceKernelIsStack` already answer from. A guest asking to make its own text writable is
    // asking for something the platform allows.
    //
    // **Refusing it was a wall.** PPSA03416 asks for 256 MiB of write access across its own
    // module, checks the result, and on the failure path calls a routine its compiler believes
    // never returns - which returns, onto the trap placed after it. Answered, the title reaches
    // 192 imports instead of 186 (D577).
    //
    // Containment is what keeps the original guard: `AddressSpace::protect` refuses an unowned
    // range so that a typo cannot re-protect this process's own code, and a range inside a
    // region this process placed for the guest cannot be that. A range covered by nothing is
    // still refused.
    if region_covering(addr, len).is_none() {
        return vendor(orbistoun_core::errno::INVALID);
    }
    match orbistoun_mem::platform::protect(addr, len, protection) {
        Ok(()) => OK,
        // The region is the guest's, and the host would not change it. Reported as the console
        // reports an invalid mapping rather than as success: the guest is about to act on the
        // answer, and the whole of this function's value is that the answer is true.
        Err(_) => vendor(orbistoun_core::errno::INVALID),
    }
}

/// `sceKernelAvailableFlexibleMemorySize(out)`.
///
/// Flexible memory is the share an application may map without reserving physical pages first, and it is
/// a **separate budget** from the direct pool (D444) - obSCEne maps it clear of the pool, and the two are
/// distinct on hardware. Answered as the measured launch figure minus what the guest has mapped, so it
/// falls as flexible memory is taken and a guest that maps then re-queries is not told it still has what
/// it just used.
///
/// **Now the measured figure (D444).** obSCEne's `020-memory/flexible-available` answered `0x1b40_0000`
/// on hardware; running obSCEne under orbistoun showed this call answering `~0x1_3f01_0000` (the direct
/// pool) instead, an order of magnitude high. It reads [`direct::flexible_available`] now, which is the
/// system default (no title overrides it, D442) minus the tracked flexible mappings.
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
/// The configured flexible-memory total, distinct from *available* (configured minus mapped): the
/// ceiling, which does not move as the guest maps. Was unimplemented - obSCEne under orbistoun answered
/// the placeholder and its `020-memory/flexible-configured` check failed - and the value it must answer is
/// now measured, `0x1c00_0000` ([`direct::flexible_configured`]), the system default since no title
/// overrides it (D442, D444).
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

/// `sceKernelReleaseFlexibleMemory(address, len)` - returns a flexible mapping to the budget.
///
/// The counterpart to [`map_flexible_memory`], and without it the round trip has no way
/// back: the probe maps, uses and then releases, and an unimplemented release makes the
/// whole sequence report failure at the last step (D273).
fn release_flexible_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (address, len) = (args[0], args[1]);
    if address == 0 || len == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // The span itself is left mapped - orbistoun hands out memory once and does not tear a hole in a
    // guest's contiguous space - but the flexible budget is credited back, so a map/release loop does not
    // drive `available` to zero over a budget the guest never actually exhausted (D444).
    direct::record_flexible_release(len);
    OK
}

/// `sceKernelMapFlexibleMemory(out, len, prot, flags)`.
///
/// Maps pages and hands back their address in one step - the caller never sees a physical address, which
/// is what makes it *flexible*. Drawn against the separate flexible budget ([`direct::flexible_available`]),
/// not the direct pool, and refused when the budget cannot cover it, so `available` stays honest (D444).
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
    // Mapped through the same path a direct mapping takes, so one implementation decides
    // where guest memory appears and the two cannot disagree.
    let mut mapping = [0_u64; GUEST_ARG_REGISTERS];
    mapping[0] = out;
    mapping[1] = len;
    mapping[2] = prot;
    mapping[3] = flags;
    mapping[4] = physical;
    mapping[5] = direct::DIRECT_ALIGN;
    let result = map_named_direct_memory(&mapping);
    // Charge the budget only once the mapping actually succeeded, so a failed map does not shrink
    // `available` for memory the guest never received.
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

/// `scePthreadAttrInit(attr)` - allocates a thread attribute object.
///
/// The same pointer-to-pointer shape as the mutex attributes: the guest declares one as
/// null and passes its address, so this allocates and hands the address back (D272).
fn pthread_attr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // Sixteen words rather than eight: the attribute set grew past the original six fields
    // and this crate owns the layout (see the field constants below), so widening it costs
    // nothing and leaves room for the ones still unwritten.
    let handle = orbistoun_mem::blocks::block(16);
    // **A fresh attribute set is not entirely zero, and that is measured.** obSCEne's
    // `031-stackattr/fresh-attr-names-no-stack` ran on the console and read a stack *address*
    // of `0x0` and a stack *size* of `0x10000` off a set nothing had touched. Zeroing both
    // made a guest that sizes an allocation from the default get nothing, and a guest that
    // checks the size before creating a thread take a failure path the console never gives it.
    //
    // The address stays zero, which the same run also measured: the two fields have different
    // defaults and answering one for both is how a plausible value gets invented (D585).
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
/// Sixty-four kibibytes, read off the console by `031-stackattr/fresh-attr-names-no-stack`.
/// **Not a guess and not FreeBSD's**, whose default is a great deal larger - this is what the
/// target answered, which is the only thing that makes a guest reading it behave the same here
/// as there (D585).
const DEFAULT_ATTR_STACK_SIZE: u64 = 0x1_0000;

/// `pthread_attr_getguardsize(attr, out)` - POSIX.1-2008.
fn pthread_attr_getguardsize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_GUARD_SIZE)
}

/// `pthread_attr_getinheritsched(attr, out)` - POSIX.1-2008.
fn pthread_attr_getinheritsched(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_INHERIT_SCHED)
}

/// `pthread_attr_getschedpolicy(attr, out)` - POSIX.1-2008.
fn pthread_attr_getschedpolicy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_SCHED_POLICY)
}

/// `pthread_attr_getscope(attr, out)` - POSIX.1-2008.
fn pthread_attr_getscope(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, ATTR_SCOPE)
}

/// `pthread_attr_setscope(attr, scope)` - POSIX.1-2008.
fn pthread_attr_setscope(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_SCOPE)
}

/// `pthread_mutexattr_getpshared(attr, out)` - POSIX.1-2008.
fn pthread_mutexattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, MUTEXATTR_PSHARED)
}

/// `pthread_mutexattr_setpshared(attr, pshared)` - POSIX.1-2008.
fn pthread_mutexattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, MUTEXATTR_PSHARED)
}

/// `pthread_mutexattr_getprioceiling(attr, out)` - POSIX.1-2008.
fn pthread_mutexattr_getprioceiling(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, MUTEXATTR_PRIOCEILING)
}

/// `pthread_mutexattr_setprioceiling(attr, ceiling)` - POSIX.1-2008.
fn pthread_mutexattr_setprioceiling(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, MUTEXATTR_PRIOCEILING)
}

/// `pthread_condattr_getclock(attr, out)` - POSIX.1-2008.
fn pthread_condattr_getclock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, CONDATTR_CLOCK)
}

/// `pthread_condattr_setclock(attr, clock)` - POSIX.1-2008.
fn pthread_condattr_setclock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, CONDATTR_CLOCK)
}

/// `pthread_condattr_getpshared(attr, out)` - POSIX.1-2008.
fn pthread_condattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, CONDATTR_PSHARED)
}

/// `pthread_condattr_setpshared(attr, pshared)` - POSIX.1-2008.
fn pthread_condattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, CONDATTR_PSHARED)
}

/// `pthread_condattr_destroy(attr)` - POSIX.1-2008.
///
/// The object is leaked rather than freed, exactly as the thread attribute's destroy does:
/// a guest may destroy an attribute while a condition variable built from it is still live,
/// and reclaiming here would leave that one reading freed memory. The storage is small and
/// bounded by how many a guest initialises.
fn pthread_condattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if attr_at(args[0]).is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_equal(a, b)` - whether two thread identifiers name the same thread.
///
/// POSIX.1-2008. **Non-zero for equal, zero for not** - the opposite sense to the `_np`
/// comparison functions and to `strcmp`, which is the mistake worth naming: a caller reading
/// this as a difference gets every answer backwards.
fn pthread_equal(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(args[0] == args[1])
}

// --- barrier and read-write lock attributes, plus the scheduling odds and ends -----------------
//
// The attribute objects follow the shape `pthread_attr_init` established: this crate allocates
// one, hands the guest a handle to it, and every accessor reads or writes a field by offset.
// Storing a value and answering it later is the *whole* contract of an attribute object (D272);
// whether a lock built from one acts on the value belongs to that lock's own account.

/// Whether a barrier or read-write lock is shared between processes.
///
/// Stored and answered, not acted on - there is one process here, so process-shared has nothing
/// to mean. See [`MUTEXATTR_PSHARED`] for the same reasoning at length.
const LOCKATTR_PSHARED: u64 = 0;

/// A read-write lock's kind, from the non-portable `gettype_np`/`settype_np` pair.
const LOCKATTR_TYPE: u64 = 8;

/// Allocates an attribute object and hands the guest a handle to it.
///
/// Shared by the barrier and read-write-lock attribute initialisers, which differ in nothing
/// else - four words is the same size the condition-variable and mutex attributes use.
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

/// Accepts a destroy without reclaiming the object.
///
/// The same choice the other attribute destroys make: a guest may destroy an attribute while
/// something built from it is still live, and reclaiming here would leave that reading freed
/// memory. The storage is small and bounded by how many a guest initialises.
fn lockattr_destroy(pointer: u64) -> u64 {
    if attr_at(pointer).is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `pthread_barrierattr_init(attr)` - POSIX.1-2008.
fn pthread_barrierattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_init(args[0])
}

/// `pthread_barrierattr_destroy(attr)` - POSIX.1-2008.
fn pthread_barrierattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_destroy(args[0])
}

/// `pthread_barrierattr_getpshared(attr, out)` - POSIX.1-2008.
fn pthread_barrierattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_PSHARED)
}

/// `pthread_barrierattr_setpshared(attr, pshared)` - POSIX.1-2008.
fn pthread_barrierattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_init(attr)` - POSIX.1-2008.
fn pthread_rwlockattr_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_init(args[0])
}

/// `pthread_rwlockattr_destroy(attr)` - POSIX.1-2008.
fn pthread_rwlockattr_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    lockattr_destroy(args[0])
}

/// `pthread_rwlockattr_getpshared(attr, out)` - POSIX.1-2008.
fn pthread_rwlockattr_getpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_setpshared(attr, pshared)` - POSIX.1-2008.
fn pthread_rwlockattr_setpshared(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_PSHARED)
}

/// `pthread_rwlockattr_gettype_np(attr, out)` - the non-portable kind accessor.
///
/// Reference: FreeBSD `pthread_rwlockattr_settype_np(3)`. Stored and answered like the rest;
/// the values it carries are the caller's own and are not interpreted here.
fn pthread_rwlockattr_gettype_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_get(args, LOCKATTR_TYPE)
}

/// `pthread_rwlockattr_settype_np(attr, kind)` - the non-portable kind setter.
fn pthread_rwlockattr_settype_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, LOCKATTR_TYPE)
}

/// `pthread_yield()` - offer the processor to another thread.
///
/// Reference: FreeBSD `pthread_yield(3)`; `sched_yield(2)` is the POSIX spelling of the same
/// thing. A hint by definition - the scheduler is free to run this thread again immediately -
/// so handing it to the host scheduler is the whole of it rather than an approximation.
fn pthread_yield(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::yield_now();
    OK
}

/// `sched_yield()` - POSIX.1-2008, the same as [`pthread_yield`].
fn sched_yield(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    std::thread::yield_now();
    OK
}

/// `pthread_getconcurrency()` - the concurrency level a guest asked for.
///
/// Reference: POSIX.1-2008. **Zero unless the guest has set one**, which the standard states
/// outright: it is a hint to an implementation that multiplexes threads, this one does not,
/// and answering zero says "the system decides" rather than inventing a level.
fn pthread_getconcurrency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    concurrency().load(std::sync::atomic::Ordering::Relaxed)
}

/// `pthread_setconcurrency(level)` - records the hint so the getter can answer it.
///
/// Reference: POSIX.1-2008, which permits an implementation to ignore the value entirely. It
/// is kept only so the pair agree with each other; nothing here reads it.
fn pthread_setconcurrency(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    concurrency().store(args[0], std::sync::atomic::Ordering::Relaxed);
    OK
}

/// The concurrency hint, which only its own two accessors read.
fn concurrency() -> &'static std::sync::atomic::AtomicU64 {
    static LEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    &LEVEL
}

// --- the POSIX timed wait and once-only initialiser ----------------------------------------

/// `pthread_cond_timedwait(cond, mutex, abstime)` - POSIX.1-2008.
///
/// **`abstime` is an absolute deadline, not a duration**, which is the half a caller's own code
/// depends on: it computes "now plus a second" once and re-passes the same deadline around a
/// spurious-wakeup loop, so treating it as a relative timeout restarts the clock every turn and
/// the loop never ends. `duration_until` does the conversion, and a deadline already past
/// becomes a zero wait rather than a negative one.
///
/// Carries the same non-atomicity the untimed [`pthread_cond_wait`] records: the condition
/// variable and the mutex are independent objects here, so a signal landing between the unlock
/// and the wait is lost where the platform would hold it.
fn pthread_cond_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let deadline = read_xtime(args[2]);
    let timeout = deadline.map_or(std::time::Duration::ZERO, |d| {
        duration_until(d).unwrap_or(std::time::Duration::ZERO)
    });
    cond_timedwait(args[0], args[1], timeout)
}

/// The body both timed condition waits share, once the timeout is a plain span.
///
/// **Absolute against relative is resolved before this point**, by whichever spelling the
/// guest called. Everything after it is identical, so it is written once.
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

/// `pthread_once(control, routine)` - runs an initialiser exactly once.
///
/// Reference: POSIX.1-2008 `pthread_once(3)`. **The routine takes no arguments and answers
/// nothing**, unlike the C++ runtime's `_Execute_once` next door, whose callback is
/// `InitOnce`-shaped and reports success - so this cannot simply forward to it. The flag is
/// marked done *after* the routine returns, which is what makes a routine that never returns
/// leave the flag unset rather than recorded complete.
///
/// **Not yet serialised across threads**, exactly as `_Execute_once` records of itself: two
/// threads racing the same fresh flag could both run the initialiser. Nothing measured does,
/// and a per-flag guard is the fix when something does.
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

// --- the timed acquisitions ---------------------------------------------------------
//
// Two families with one difference that matters: **the POSIX calls take an absolute
// deadline and FreeBSD's `_np` spellings take a relative span.** A caller computes "now
// plus a second" once and re-passes the same deadline around its retry loop, so reading an
// absolute time as relative restarts the clock every turn and the loop never ends; reading
// a relative span as absolute makes every wait expire instantly, because a span of one
// second is a moment in 1970. Neither misreading fails loudly, which is why they get
// separate readers rather than a flag.
//
// Reference: POSIX.1-2008 for `pthread_mutex_timedlock`, `pthread_rwlock_timedrdlock`,
// `pthread_rwlock_timedwrlock`, `sem_timedwait` and `sem_getvalue`; Solaris
// `sem_timedwait(3C)` and `pthread_cond_timedwait(3C)`, which document the
// `sem_reltimedwait_np` and `pthread_cond_reltimedwait_np` spellings and their arities.

/// Turns an **absolute** `timespec` into the moment a wait should give up.
///
/// `None` for a pointer that cannot be read, which the caller reports rather than treating
/// as "no timeout" - a null deadline waiting forever is the one outcome a caller that asked
/// for a bounded wait cannot recover from.
fn deadline_at(pointer: u64) -> Option<sync::Blocking> {
    let remaining = duration_until(read_xtime(pointer)?)?;
    Some(patience_for(remaining))
}

/// Turns a **relative** `timespec` into the same, for FreeBSD's `_np` spellings.
fn deadline_after(pointer: u64) -> Option<sync::Blocking> {
    Some(patience_for(read_xtime(pointer)?))
}

/// A span from now, as a deadline.
///
/// A span so large that the host clock cannot represent the moment becomes "wait forever",
/// which is what a guest asking for it meant. Answering an instant timeout instead would
/// turn the most patient possible request into the least patient one.
fn patience_for(span: std::time::Duration) -> sync::Blocking {
    std::time::Instant::now()
        .checked_add(span)
        .map_or(sync::Blocking::Forever, sync::Blocking::Until)
}

/// Answers an acquisition that was given a deadline.
///
/// **`Some(false)` is a timeout here and busy elsewhere**, and only the caller knows which,
/// because only the caller knows what patience it asked for. See [`acquired`] for the
/// untimed form.
fn timed_out(outcome: Option<bool>) -> u64 {
    match outcome {
        Some(true) => OK,
        Some(false) => u64::from(GuestError::vendor(orbistoun_core::errno::TIMED_OUT).as_raw()),
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `pthread_mutex_timedlock(mutex, abstime)` - POSIX.1-2008.
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
        // The owner re-taking an error-checking lock, which no amount of waiting can
        // resolve and which the console answers with the invalid-argument errno rather
        // than a busy - measured in 015-sync/mutex-recursion (D416).
        Some(sync::Acquisition::Deadlock) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw())
        }
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `pthread_rwlock_timedrdlock(lock, abstime)` - POSIX.1-2008.
fn pthread_rwlock_timedrdlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, until)))
}

/// `pthread_rwlock_timedwrlock(lock, abstime)` - POSIX.1-2008.
fn pthread_rwlock_timedwrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, until)))
}

/// `sem_timedwait(sem, abstime)` - POSIX.1-2008, an absolute deadline.
///
/// **Answers the code directly rather than `-1` with `errno` set**, which is the convention
/// the rest of the `sem_*` family here already follows: this project does not maintain a
/// guest `errno`, so a caller reading one would find whatever was there before. A guest
/// testing the result against zero branches correctly either way.
fn sem_timedwait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_at(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, until)))
}

/// `sem_reltimedwait_np(sem, reltime)` - the same wait, given a span instead.
fn sem_reltimedwait_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_after(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, 1, until)))
}

/// `sem_getvalue(sem, sval)` - how many the semaphore has free.
///
/// Reference: POSIX.1-2008. **Four bytes**, because `sval` is an `int *` - a whole word
/// would put the top half in whatever the guest keeps next door, which has bitten this
/// crate twice (D210, D272).
///
/// The standard permits a negative answer whose magnitude counts the waiters, and permits
/// zero instead; this counts no waiters, so it answers the count and stops. It also states
/// the value may already be stale by the time the caller sees it, so a snapshot is the
/// contract rather than an approximation of it.
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

/// `pthread_cond_reltimedwait_np(cond, mutex, reltime)` - a **relative** timed wait.
///
/// The same body as [`pthread_cond_timedwait`] with the other time reader, which is the
/// only difference between the two calls and the whole reason both exist.
fn pthread_cond_reltimedwait_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let timeout = read_xtime(args[2]).unwrap_or(std::time::Duration::ZERO);
    cond_timedwait(args[0], args[1], timeout)
}

/// Fields a thread attribute object holds, by offset.
///
/// **This crate defines the layout**, defensible only because nothing else reads it: every
/// call that touches one is here, and the real layout is not known from any lawful source.
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
/// The lowest address of the stack, one further - pointer-wide, like the size (D575).
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
    // Four bytes: the out-parameter is an `int`, and eight would take the caller's
    // neighbouring variable with it (D272).
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
/// **A size, so the out-parameter is pointer-width rather than an `int`.**
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

/// `scePthreadAttrGet(thread, attr)` - fills an attribute object with a running thread's
/// attributes.
///
/// FreeBSD's `pthread_attr_get_np(3)` is the lawful reference for the shape: the object must
/// already be initialised, and it comes back describing the thread as it *is* - stack, size,
/// priority, policy, affinity - rather than as it was asked for. The guest that established it
/// asks about **itself**: `scePthreadSelf`, then this, then the stack address and size, which is
/// the sequence a garbage collector runs to find the bottom of the stack it must scan.
/// Unimplemented, three titles computed that bottom from a placeholder and scanned upward off
/// the top of the real stack, into the first unmapped page above it (D575).
///
/// The stack comes from the thread's own record, and for the thread the guest was entered on -
/// which reserved nothing and runs on the span the worker placed - from the span the worker told
/// this crate about. A thread with neither is refused rather than described: an attribute object
/// holding a made-up stack is exactly the wrong answer this exists to stop.
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

/// `scePthreadAttrGetstackaddr(attr, out)` - the lowest address of the stack an attribute names.
///
/// FreeBSD `pthread_attr_getstackaddr(3)`: the value set on the object, which is null on one
/// nothing has set - so a fresh object answers zero and success, as the reference does. Written
/// pointer-wide, as [`pthread_attr_getstacksize`] is: an address, not an `int`.
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

/// `scePthreadAttrSetschedpolicy(attr, policy)` - stored, so a later get reads it back.
fn pthread_attr_setschedpolicy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_SCHED_POLICY)
}

/// `scePthreadAttrSetinheritsched(attr, inherit)` - stored, so a later get reads it back.
fn pthread_attr_setinheritsched(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_INHERIT_SCHED)
}

/// `scePthreadAttrSetaffinity(attr, mask)`.
///
/// **The mask is stored, not applied.** Which host core a guest thread runs on is the host
/// scheduler's to decide, and orbistoun does not pin guest threads; a guest reading the
/// attribute back still gets what it set, which is all the setter's own contract promises.
fn pthread_attr_setaffinity(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_AFFINITY)
}

/// `scePthreadSetaffinity(thread, mask)` - the running-thread form.
///
/// # Accepted and not applied, which is the attribute form's own bargain
///
/// [`pthread_attr_setaffinity`] already says it: which host core a guest thread runs on is the
/// host scheduler's to decide, and orbistoun does not pin guest threads. The attribute form can
/// at least hand back what a caller set, because it owns a block to store it in. This one has
/// nowhere to put it - orbistoun keeps no per-thread scheduling record - so the mask is
/// accepted and dropped.
///
/// **That is a real difference from the attribute form and it is stated rather than glossed:**
/// a guest that sets an affinity here and reads it back through `scePthreadGetaffinity` would
/// not get what it set. Nothing observed does; the 62 calls PPSA02664 makes never read one back.
/// The moment something does, this needs the per-thread record rather than a wider `Ok` (D523).
///
/// # Why `Ok` and not the placeholder
///
/// A stub answered `0x7fff_0001`, and a caller testing a scheduling call against zero reads
/// that as a failure to set affinity - which is a *lie in the other direction*, because the
/// call is one orbistoun can honestly accept. Nothing here reports success at applying it; the
/// contract a setter promises is that the request was taken.
///
/// The thread handle is not checked. Orbistoun has no registry to check it against, and
/// refusing a handle that cannot be verified would turn an accepted call into a refused one on
/// no evidence.
fn pthread_setaffinity(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `scePthreadGetaffinity(thread, mask)` - reads back the affinity recorded for a thread.
///
/// **The read [`pthread_setaffinity`]'s own note (D523) said would need the per-thread record the
/// moment a title asked for one - and PPSA04263 does.** It answers from the record orbistoun already
/// keeps: the `requested_affinity` captured when the thread was created (the attribute form stores it,
/// D523). A mask of zero is the guest's own convention for "anywhere" ([`thread::Affinity::is_unset`]),
/// so a thread that pinned nothing, or one orbistoun holds no record for - the main thread, or one made
/// before this ran - reads back zero, which is that "anywhere", not an error.
///
/// **It returns what was asked, not what the host scheduler did** - the effective mask under the default
/// `Observe` policy is always zero (D150), and handing that back would tell a guest its own request was
/// discarded. The running-thread setter still drops (D523), so a set-then-get on a live thread reads the
/// creation-time value; that is the documented limit, not a new one, and no title observed reads one back
/// after setting it on a running thread.
fn pthread_getaffinity(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let mask = thread::record(args[0]).map_or(0, |record| record.requested_affinity.0);
    // SAFETY: an address the guest passed for this call, valid by its contract.
    if args[1] == 0 || !unsafe { guest::write_u64(args[1], mask) } {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadAttrSetguardsize(attr, size)` - stored, so a later get reads it back.
fn pthread_attr_setguardsize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    attr_set(args, ATTR_GUARD_SIZE)
}

/// `scePthreadAttrDestroy(attr)`.
///
/// The block is leaked rather than freed, as every handle here is: a guest that destroys
/// an attribute and then uses it gets a stale object rather than a fault into freed
/// memory, which is the safer of two wrong behaviours while nothing tracks lifetimes.
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
/// # Why a nominal value rather than the host's own
///
/// The counter and its frequency have to agree, and until a console had been asked, the
/// host's real rate was neither stable across machines nor knowable. A nominal nanosecond
/// tick was chosen then because it made the arithmetic exact.
///
/// **The hardware trip happened, and this is what it answered.** A conformance run on a
/// target console read the frequency back as `0x5f25_9b8e`, and the same run cross-checked
/// it without meaning to: a twenty-millisecond sleep advanced the counter by `0x1f12cd9`
/// ticks while the microsecond clock advanced by `0x4fbb`, which works out at 1.5963 GHz -
/// the same number to four significant figures, arrived at two independent ways.
///
/// So the assumption D275 recorded for the trip is retired: this is no longer a convenient
/// round figure but the rate the machine runs at, and a title deriving a frame budget from
/// it now gets the budget the console would have given it (D398).
///
/// **A second run, on a different day, answered `0x5f25_9b93`** - five ticks per second
/// higher, which is 0.0000003% and is the measurement's own jitter rather than a different
/// machine. It cross-checked itself the same way: `0x4e8e` microseconds against `0x1e9d6e6`
/// ticks across a sleep, giving 1.593 GHz.
///
/// The constant is left at the first reading deliberately. Two measurements four ticks apart
/// do not tell you which is nearer the truth, and moving it would change every derived frame
/// budget for no reason anyone could state (D605).
const TSC_HZ: u64 = 0x5f25_9b8e;

/// Nanoseconds in a second, for converting the host's clock to the target's rate.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// `sceKernelReadTsc()` - the time stamp counter.
///
/// **Must actually advance.** A stub answering a constant makes every elapsed measurement
/// zero, which reads as a sleep that returned instantly - and that is precisely what the
/// conformance probe reported before this existed (D275).
fn read_tsc(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ticks_since()
}

/// Host time since `origin`, expressed in the target's own ticks.
///
/// **Scaled rather than passed through.** The counter used to be nanoseconds and the
/// frequency a matching billion, so the two agreed by construction; now the frequency is a
/// measured number, and a counter still ticking in nanoseconds would report a rate the
/// paired `GetTscFrequency` call denies. A guest converting ticks to seconds divides by what
/// that call told it, so the two must agree or the guest mistimes everything - which is the
/// same trap the nominal rate was originally chosen to avoid, one step along.
///
/// Done in `u128` because nanoseconds times the frequency overflows sixty-four bits in about
/// eleven seconds, and saturating at the end so a run long enough to overflow reports a stuck
/// clock rather than a wrapped one.
fn ticks_since() -> u64 {
    // The same source every other clock here reads, so a guest converting between them lands
    // where it expects - and so one setting decides whether all of them repeat (D582).
    let nanos = orbistoun_hle::clocks::since_start_nanos();
    u64::try_from(nanos * u128::from(TSC_HZ) / NANOS_PER_SECOND).unwrap_or(u64::MAX)
}

/// `sceKernelGetTscFrequency()` - ticks per second, matching [`read_tsc`].
fn get_tsc_frequency(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    TSC_HZ
}

/// The guest stack, as the worker mapped it.
static STACK_SPAN: OnceLock<(u64, u64)> = OnceLock::new();

/// Records where the guest's stack is, so `sceKernelIsStack` can answer.
///
/// Told rather than derived: this crate does not place the stack and re-deriving it from
/// the constants it was built with is how the readable window ended up a page too low
/// (D217).
pub fn note_stack_span(base: u64, len: u64) {
    let _ = STACK_SPAN.set((base, len));
}

/// `sceKernelIsStack(address, low, high)` - where the calling thread's stack is.
///
/// # It is not the predicate its name suggests, and this answered as though it were
///
/// This took one argument and answered `1` for an address in the stack and `0` for one
/// outside. Every part of that is contradicted by the console (D612):
///
/// - **It takes three.** The probe declares `int sceKernelIsStack(void *, void **, void **)`
///   and calls it that way, so the two words after the address are where the bounds go.
/// - **It returns a status, not a verdict.** Twenty-three runs of `010-kernel/is-stack` all
///   fail with *a stack address and a static one were reported alike*, value `0x0` - the
///   console answers `0` to a local **and** to a static. A check written for a predicate
///   reads that as a broken function; it is a successful call whose answer is elsewhere.
/// - **The answer is elsewhere.** `031-stackattr/address-is-the-base` records `low` and
///   `high` two mebibytes apart, which is exactly the stack size the same run read off the
///   thread's own attribute. The bounds are the output.
///
/// So a guest asking this where its stack is got an inverted flag and two words of its own
/// uninitialised memory back.
///
/// # The calling thread's, which used to mean the first thread's
///
/// A guest thread gets its own stack at its own address, and only the main one was ever
/// recorded - so a thread asking about a local of its own was told **no**. That is the
/// wrong answer to the only question this function is ever asked, and it is the same blind
/// spot the argument dumps had: a span that comes into existence after the run starts, and
/// a table that was filled before it (D387, D391).
///
/// So a thread records its own span when it gets one, and that is consulted first. The main
/// stack remains the answer for the thread the guest was entered on.
///
/// # What is not measured
///
/// Whether the console writes the bounds when the address is **not** in a stack. It answers
/// `0` either way, and no capture records the two words for the static case. The bounds are
/// written here regardless, because they describe the calling thread's stack rather than the
/// address - a caller that asked about a static still learns where its own stack is, and
/// refusing to say would be inventing a distinction nothing has measured.
fn is_stack(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let span = thread::this_stack().or_else(|| STACK_SPAN.get().copied());
    if let Some((base, len)) = span {
        // Null is how a caller says it does not want one, which the probe's own second
        // witness relies on - it passes both and reads neither when the call is absent.
        if args[1] != 0 {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::write_u64(args[1], base) };
        }
        if args[2] != 0 {
            // SAFETY: an address the guest passed for this call, valid by its contract.
            unsafe { guest::write_u64(args[2], base.saturating_add(len)) };
        }
    }
    // **Zero whether or not the address is in it**, which is what twenty-three runs measured
    // and is the whole of what the return value says.
    OK
}

/// The modules this process has loaded, as the guest should see them.
///
/// One today: the executable the loader placed. A title that loaded a library at runtime
/// would add to this, and nothing here can do that yet - see [`load_start_module`].
static LOADED_MODULES: OnceLock<Vec<(u64, String)>> = OnceLock::new();

/// Records which modules are loaded, for the guest to enumerate.
///
/// Told rather than derived, for the same reason as the stack span: this crate does not do
/// the loading and re-deriving the list from constants is how two copies drift (D275).
pub fn note_loaded_modules(modules: Vec<(u64, String)>) {
    let _ = LOADED_MODULES.set(modules);
}

/// `sceKernelGetModuleList(handles, max, written)`.
///
/// **Reports the module the loader actually placed**, not a plausible-looking list. A guest
/// enumerating modules and finding names nothing loaded would be reading an invention, and
/// the enumeration is exactly the sort of thing a title uses to decide what it may call.
fn get_module_list(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (out, max, written) = (args[0], args[1], args[2]);
    if out == 0 || written == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let loaded = LOADED_MODULES.get().map_or(&[][..], Vec::as_slice);
    let count = usize::try_from(max).unwrap_or(0).min(loaded.len());
    for (index, (handle, _)) in loaded.iter().take(count).enumerate() {
        // Handles are written as `int`, four bytes - the array the guest passed is an
        // `SceKernelModule[]` and a whole word each would run off the end of it (D272).
        let Ok(at) = usize::try_from(out.saturating_add((index * 4) as u64)) else {
            return u64::from(GuestError::InvalidArgument.as_raw());
        };
        // SAFETY: a guest-supplied array under the identity mapping (D014), written within
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

/// `sceKernelGetModuleInfo(handle, info)` - refused, because the structure is not derivable.
///
/// # Why this is a refusal rather than a best effort
///
/// The answer is a `SceKernelModuleInfo`: a vendor structure whose field offsets are not
/// described by anything lawful in this repository. Filling it means choosing where the name
/// goes, where the segment table goes and how long each is - and a guest reading a name out of
/// the wrong offset gets whatever was next to it, printed as though the platform had said it.
/// The same reasoning that leaves a notification's message undecoded (D271).
///
/// **What changes is that it stops answering a placeholder.** Unimplemented, it returned this
/// project's `Unimplemented` code - `0x7fff0001`, deliberately positive so it can never be
/// mistaken for a firmware value, which for a status makes it look like *success with a small
/// non-zero code* and for a count or a handle looks like data (D125, D273). The conformance
/// probe reported exactly that number back.
///
/// **Refused with the code hardware refuses it with.** obSCEne's `110-modules` records this call
/// failing with `0x8002_0016` on the target - `INVALID`, the platform declining to describe a
/// module by name in this mode - across both the module and payload runs. An earlier cut returned
/// a bare `-1`, which was the POSIX shape assumed rather than the vendor code measured (the same
/// assumption-over-measurement slip as the software version, D420); this answers what the console
/// answered, so a guest and a diff both see the refusal the hardware gives.
///
/// # What would let this be implemented
///
/// One byte dump of the structure from real hardware. `sceKernelGetModuleList` already answers
/// honestly and `LOADED_MODULES` already holds the handle and the name, so the only missing
/// thing is the shape - and it is the kind of thing a conformance probe records rather than
/// the kind of thing anybody should reason out (D395).
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
/// **The caller's own size field would be better and is not trusted.** Every vendor structure
/// of this family is documented elsewhere as beginning with its own length, and "elsewhere" is
/// exactly the kind of source this project does not take - so a fixed, generous span is filled
/// instead and the run says so. A guest that declared less gets more written than it asked
/// for, which is why this is a diagnostic and not a default.
const DESCRIBED_WORDS: usize = 64;

/// Whether this run was asked to describe what it cannot describe.
fn describing_module_info() -> bool {
    orbistoun_env::DESCRIBE.get().as_deref() == Some("module-info")
}

/// Fills a structure with markers that name their own offset, and reports success.
///
/// # Why this is the loop rather than a guess
///
/// A layout that cannot be derived can still be **measured**, and the guest is the thing that
/// knows it. Each word says which offset it came from, so a guest that reads a field and uses
/// it - as a pointer, a length, a handle - stops on an address that decodes back to the offset
/// it was read from. One run per question, and the question is *which field does a title
/// actually want* rather than *what is the whole structure* (D390, D395).
///
/// Emphatically a diagnostic: it writes memory the guest owns, answers success for something
/// that did not happen, and is recorded as intervening. What it produces is a work list, not a
/// layout - a field a title reads is a field worth learning the meaning of, and a field nothing
/// ever touches is one nobody needs to.
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
        // SAFETY: a guest-supplied structure under the identity mapping (D014), which the
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
/// Negative, which is how this FreeBSD-derived kernel reports failure to a caller that tests
/// for it - and unlike a placeholder it can never be read as a small successful value.
const FAILED_STATUS: u64 = -1_i64 as u64;

/// `sceKernelLoadStartModule(path, argc, argv, flags, opt, result)`.
///
/// # What each kind of path answers, and where that came from
///
/// A conformance run asked for eight paths and recorded the code for each
/// (`110-modules/load`), so these are measured rather than chosen:
///
/// - **libkernel** is always resident and answers its well-known handle (D264, D400).
/// - **`/system`** modules are the firmware's own copy; the platform does not load a second
///   and answers the not-found errno.
/// - **`/app0`** is a title's own module, which loads and gets a fresh handle.
/// - Anything else is refused. `060-module/load-rejects-missing` loads a bogus path, and a
///   stub answering success reported a nonexistent module as loaded - which is the
///   honest-failure mistake exactly.
///
/// **The `/app0` handle value is opaque and deliberately does not match the console's.** Its
/// handles reflect however many modules its own loader had already placed, so the number is a
/// fact about that machine rather than about this interface. What a guest keys on is that each
/// load gets a distinct non-negative handle.
///
/// A refusal is negative rather than a small positive placeholder, because the success answer
/// is a handle and a placeholder is exactly what a handle looks like (D273).
///
/// # What this still does not do
///
/// **Answering a handle is not loading a module.** Nothing places a second image, and nothing
/// registers its exports, so a guest that loads its own module and then calls into it finds
/// nothing there. That is the remaining work, and it is what stands between PPSA02664 and
/// `il2cpp_init`.
fn load_start_module(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let path = unsafe { read_name(args[0]) };

    // libkernel is always resident, at the handle every guest reaches it by (D264, D400).
    if path.contains("libkernel") {
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u32(args[5], 0) };
        return LIBKERNEL_MODULE_HANDLE;
    }

    // A firmware module is the platform's own copy; it does not load a second and answers the
    // not-found errno instead - measured, which returns `0x8002_0002` for both `/system` paths
    // the probe asked for (110-modules/load).
    //
    // **Three directories, not one**, and this used to know about one of them. The probe only
    // ever asked about `/system/common/lib/`, and a prefix generalised from it silently
    // excluded `/system_ex/common_ex/lib/` - which holds 234 of the platform's 537 modules,
    // more than the 274 in the directory that was covered. A `/system_ex` path fell through to
    // the unrecognised-path refusal below and got the same errno by luck, for a reason that
    // was not the true one. `/system/priv/lib/` was covered only because it happens to share
    // the `/system/` prefix.
    //
    // Reference: `docs/PLATFORM_LIBRARIES.md` and `data/hardware/ps5-sprx-manifest.tsv` in the
    // sibling conformance-probe repository, which enumerate the three directories and what
    // each tier reaches. **The two `/system` answers are measured; the extension to
    // `/system_ex` is not** - no probe has asked for one - so it is the same rule applied to a
    // directory the same reasoning covers, and a run that asks would settle it.
    if FIRMWARE_MODULE_DIRECTORIES
        .iter()
        .any(|dir| path.starts_with(dir))
    {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    }

    // A title's own module, under `/app0`, loads and is handed a fresh handle. The value is
    // opaque and need not match the console's (its handles reflect however many modules its
    // loader had already placed); what matters is that each load gets a distinct non-negative
    // handle, which is what a guest keys its later calls on.
    if path.starts_with("/app0/") {
        let handle = NEXT_MODULE_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: an address the guest passed for this call, valid by its contract.
        unsafe { guest::write_u32(args[5], 0) };
        // **And now the `Start` half.** The module is already placed, relocated and
        // protected - `place_title_modules` does that before the guest runs - so starting it
        // is running its `DT_INIT` and `DT_INIT_ARRAY`, which is how a C++ module's static
        // constructors are reached (D514, D515).
        //
        // A module the loader never told this crate about is still recorded as unstarted,
        // because a handle and a silence are indistinguishable from a module that started -
        // principle 3 exactly.
        match initialisers_for(&path) {
            Some(initialisers) => started(&path, handle, run_initialisers(initialisers)),
            None => started_nothing(&path, handle),
        }
        return handle;
    }

    // Anything else is not a module location this kernel recognises. Returning a handle for it
    // would be the honest-failure mistake exactly: `060-module/load-rejects-missing` loads a
    // bogus path and a stub that answered success reported a nonexistent module as loaded. A
    // path that is neither libkernel, a `/system` module, nor a `/app0` module is refused.
    u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw())
}

/// Where the platform keeps its own modules, so a request for one is refused rather than loaded.
///
/// A table rather than a prefix test, because the three are not one prefix: `/system_ex` is not
/// under `/system`, and a rule that assumed it was covered 274 modules while missing 234.
///
/// Reference: the platform library survey in the sibling conformance-probe repository.
const FIRMWARE_MODULE_DIRECTORIES: &[&str] = &[
    // The application tier, mapped into every game sandbox - 274 modules.
    "/system/common/lib/",
    // The system-application tier: WebKit, the shell, media - 234 modules. **Not** under
    // `/system/`, which is the whole reason this is a list.
    "/system_ex/common_ex/lib/",
    // Privileged services - 29 modules.
    "/system/priv/lib/",
];

/// libkernel's module handle - the one well-known value in the space, confirmed on hardware.
const LIBKERNEL_MODULE_HANDLE: u64 = 0x2001;

/// The next handle handed to a freshly loaded `/app0` module. Starts clear of the low handles
/// the loader's own placed modules use and of `LIBKERNEL_MODULE_HANDLE`.
static NEXT_MODULE_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x40);

/// `sceKernelDlsym(module, name, address)` - the address of a function, by name, at run
/// time.
///
/// # Why this one function decides whether the payloads run at all
///
/// An import table is a list of names resolved before a program starts. The open-toolchain
/// payloads barely use one: their runtime asks the platform for its C library **a name at a
/// time**, storing each answer in a global of its own. `klogsrv` carries `vsnprintf`,
/// `snprintf` and `sprintf` as eight-byte objects in `.bss` for exactly that reason, and
/// the first thing its startup code does is call through the structure it was handed with
/// the string `sceKernelDlsym` - bootstrapping the resolver before resolving anything else
/// (D365).
///
/// So a payload whose every import resolves still reaches `main` with a table of nulls, and
/// dies calling one. That is the wall three sessions of diagnostics kept arriving at from
/// different directions, and it is this.
///
/// # The answer is a stub that already existed
///
/// **Nothing new is manufactured here.** A name is looked up in the same table of stubs the
/// linker resolves imports into, so a function reached this way and the same function
/// reached by an import are the *same address*, with the same counter, the same trace entry
/// and the same implementation. A resolver that built its own answers would have created a
/// second way for a call to behave, and the first divergence would have been unattributable.
///
/// # What a name nobody implements gets
///
/// A failure, and no write. Answering an address for a name this cannot serve hands the
/// guest something to call that is not what it asked for - the D125 class, one layer up:
/// the caller checks the return value, and inventing success is what stops it checking.
///
/// **The module handle is ignored.** A name is library-independent here for the same reason
/// a NID is: the hash is of the name alone. A guest asking two modules for one name gets one
/// answer, which is what this emulator has to give and is stated rather than hidden.
/// Whether this is the first time a name has been looked up this run.
///
/// A runtime resolves the same name once and keeps it, but a guest with several threads or
/// a second initialisation pass does not - and a line per call would bury the list under
/// repeats of its own first entry.
fn first_time_asked(name: &str) -> bool {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    static ASKED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let Ok(mut guard) = ASKED.lock() else {
        // A poisoned lock means another thread panicked while holding it. Reporting again
        // is harmless; going quiet would lose the work list.
        return true;
    };
    guard
        .get_or_insert_with(Default::default)
        .insert(name.to_owned())
}

/// What the guest's own binaries export, by NID, at the address they were placed.
///
/// # Why the kernel holds this at all
///
/// `sceKernelDlsym` is handed a **name**; every export table on this platform is keyed by a
/// **hash of that name**. So the lookup cannot be precomputed by the loader - it does not know
/// which names a guest will ask for, and a hash cannot be reversed. The loader registers what
/// it placed, the kernel hashes at the call, and the two meet in the middle (D517).
static GUEST_EXPORTS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

/// The hash suffix the loader is using, so a name can be turned into the NID it exports under.
static NID_SUFFIX: OnceLock<Vec<u8>> = OnceLock::new();

/// Tells this crate what a guest binary exports, so `sceKernelDlsym` can answer for it.
///
/// `exports` are `(nid, address)` with the address already offset by wherever the module was
/// placed. Called once per placed binary, and once for the executable - which is the case that
/// matters: PPSA02664's eboot has exactly **one** export, `scriptingGetMem`, and asks for it by
/// name through `dlsym` (D517).
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

/// Whether libkernel is where this run says a name lives - or whether it does not say.
///
/// **Three answers collapsed into two, deliberately.** A name libkernel declares is `true`; a
/// name declared somewhere else is `false`; and a name nothing has published a verdict on is
/// also `true`, because "nobody said" must never become "this module does not export it". The
/// refusal built on this is only ever allowed to correct a measured wrong success, never to
/// invent a failure (D629).
fn libkernel_exports(name: &str) -> bool {
    orbistoun_thunk::name_is_libkernel(name).unwrap_or(true)
}

fn dlsym(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (module, name, out) = (args[0], args[1], args[2]);
    // A module handle is checked before the name. It is a 32-bit `SceKernelModule`, so only its low
    // word is meaningful (the high half of the register is undefined for an `int` argument). Every
    // handle this kernel hands out is non-negative (`sceKernelLoadStartModule` answers `0x2001` for
    // libkernel and counts up from `0x40`), so a negative one read as a signed 32-bit value - `-1`,
    // the invalid handle obSCEne's `060-module/dlsym-rejects-bad-handle` passes - names no module and
    // earns `ESRCH` (`0x80020003`), rather than being ignored while the name is resolved globally (D366).
    if (module as u32 as i32) < 0 {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    if name == 0 || out == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: an address the guest passed for this call, valid by its contract.
    let name = unsafe { read_name(name) };
    // **The thunk table first, then the guest's own exports.** Purely additive: every name that
    // resolved before resolves to the same address, and a name that did not now reaches the
    // guest's own code instead of an error.
    //
    // Which of the two *should* win when both answer is not settled here, because nothing has
    // been seen where both do. A guest asking for a symbol its own binary exports plainly wants
    // its own code; a guest asking for a platform function wants orbistoun's. Ordering it the
    // other way would change what already-working names answer, on no evidence (D517).
    // **Resolved from the stub table first, and separately**, because the narrowing below applies
    // only to those. A name the *guest's own binary* exports is a different question: PPSA02664
    // asks for its own `scriptingGetMem` through this call (D517), and refusing that because
    // libkernel does not export it would break a working path to fix a different one.
    //
    // **And on a title's route the stub table is not consulted at all.** The console does not
    // resolve platform names by name from a launched title: obSCEne's
    // `060-module/dlsym-resolves-known-symbol` fails `0x8002_0003` for a known symbol from a
    // valid handle, and every `dlsym` measurement in that leg reads `0x0`. A payload's does
    // resolve and has to (D365), so the refusal is the route's rather than the function's
    // (D669).
    //
    // Scoped to the stub table on purpose. What was measured is the platform refusing to hand
    // out *its own* functions; nothing has asked a title for a symbol its own binary exports,
    // so that path is left alone rather than refused on the strength of an adjacent finding.
    let resolves_by_name =
        orbistoun_core::route::resolves_by_name(orbistoun_core::route::presented());
    let from_stubs = if resolves_by_name {
        orbistoun_thunk::name_thunk(&name)
    } else {
        None
    };

    // **A module handle now narrows the answer, for the one handle that has been measured.**
    //
    // obSCEne's `110-modules/symbol` asks libkernel - handle `0x2001`, three measurements
    // agreeing - for `memcpy`, and the console answers `0x80020003`: libkernel does not export
    // it. This table is flat, so `dlsym` answered an address and success, which is plausible
    // output in exactly the sense principle 3 means it: a guest asking *whether* a symbol
    // exists is told yes, and cannot tell that nothing consulted the module it named (D629).
    //
    // **Narrowed only downward.** It fires where a name is declared here *and* declared
    // somewhere that is not libkernel. A name nothing has a library for still resolves, a name
    // only the guest exports still resolves, and every handle but libkernel's still resolves
    // anything - so this can turn a wrong success into the measured refusal and cannot turn a
    // working resolution into a failure on a guess.
    if module == LIBKERNEL_MODULE_HANDLE && from_stubs.is_some() && !libkernel_exports(&name) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    }
    // **And, only when a run asked for it, a stub for a name this project declares and does not
    // implement.** Empty in an ordinary run, so this line changes nothing unless
    // `ORBISTOUN_DLSYM_STUBS` installed a table - which is what makes the two runs comparable
    // (D632). Last, so it can never shadow an implementation or the guest's own export.
    let address = from_stubs
        .or_else(|| guest_export(&name))
        .or_else(|| orbistoun_thunk::declared_thunk(&name));

    // **Every distinct name, once, answered or not.** A payload's resolution pass is the
    // clearest statement it ever makes of what it needs, and it makes it before doing
    // anything else - so this is the work list, and printing only the failures would throw
    // away the half that says what the runtime is built out of. The same shape as the
    // `sysctl` report, for the same reason (D366).
    if first_time_asked(&name) {
        // **Three verdicts, not two.** A title's refusal is not "nothing here implements it" -
        // the function is implemented and reachable by import, and the platform simply does not
        // hand it out by name. Reporting the two the same way would put every platform name a
        // title asks for onto a work list that has already been done (D669).
        let verdict = match address {
            Some(at) => format!("answered {at:#x}"),
            None if !resolves_by_name => {
                "which a title's route does not resolve by name, as the console does not".to_owned()
            }
            None => "which nothing here implements".to_owned(),
        };
        let line = format!("orbistoun: the guest asked for the address of {name} - {verdict}");
        eprintln!("{line}");
        // **And to the kernel log**, which is what `klogsrv` forwards. A name the guest could
        // not resolve is the kernel talking about the process, which is exactly what belongs
        // there (D389).
        orbistoun_core::klog::note(&line);
    }

    let Some(address) = address else {
        // **The measured refusal where the route explains it, the placeholder where it does
        // not.** `0x7FFF_0001` says "nothing here implements this", which is orbistoun talking
        // about itself and is deliberately a value no firmware answers. A title asking for a
        // platform name is a different case: the console answers `0x8002_0003`, the function
        // exists, and handing back a placeholder would be reporting an emulator gap where the
        // platform has a rule (D669).
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

/// `sceKernelSendNotificationRequest(device, request, size, blocking)` - the toast a payload
/// puts on screen to say it started.
///
/// # The most wanted vendor name in the payload library
///
/// Twenty-one of the twenty-five open-toolchain payloads import it, second only to
/// `vsnprintf`. It is how they announce themselves, and several call it before they do
/// anything else - so a payload whose notification fails may report and stop.
///
/// # What is answered, and what is deliberately not decoded
///
/// Success, because the alternative sends a working payload down an error path over a
/// cosmetic call. **The message is not read.** The request buffer's layout is not derivable
/// from anything lawful in this repository - it is a vendor structure, and no header in the
/// FreeBSD checkout describes it - so decoding a message out of it would be inventing a field
/// offset and then printing whatever was there as though it were the guest's own words.
///
/// What *is* reported is what can be seen without a layout: that a notification was asked
/// for, and how many bytes it was given. A run that wants the bytes themselves can point the
/// argument-dump machinery at this call, which is what that machinery is for.
///
/// Once per run, because a payload that notifies on a timer would otherwise fill the log
/// with the same line.
fn send_notification_request(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "orbistoun: the guest asked to show a notification ({} bytes at {:#x}) - accepted, and the message is not decoded because the structure is not published",
            args[2], args[1]
        );
    }
    OK
}

/// Implementations this crate provides, by symbol name.
///
/// Names rather than hashes: the hash is derived, and a table written in hashes could
/// not be read by a person or checked against the declarations above.
/// The three POSIX thread calls whose vendor twin takes one argument more.
///
/// # The bug this exists to stop, which was silent and is not rare
///
/// `libScePosix` mostly delegates a POSIX name straight to its vendor-named twin, and the
/// arity is taken from the twin "so the two cannot disagree". They disagree. Three of the
/// vendor calls end in a **name** the POSIX ones do not have:
///
/// | POSIX | vendor |
/// |---|---|
/// | `pthread_create(thread, attr, start, arg)` | `scePthreadCreate(..., name)` |
/// | `pthread_cond_init(cond, attr)` | `scePthreadCondInit(..., name)` |
/// | `pthread_mutex_init(mutex, attr)` | `scePthreadMutexInit(..., name)` |
///
/// So a guest calling the POSIX spelling had its *uninitialised* third or fifth argument
/// register read as a string pointer. `zftpd` had bound its socket, listened on it and was
/// initialising its client table when `pthread_mutex_init` read `rdx`, which held `0x18`
/// left over from the loop above, and faulted on it (D385).
///
/// Nothing detects this by inspection: the delegation resolves, the test that every
/// delegation names a real implementation passes, and the call works for every guest that
/// happens to leave a readable address in that register.
///
/// # Why a wrapper rather than a defensive read
///
/// `read_name` following a wild pointer is correct behaviour - a guest that passes one gets
/// the fault it would have got. The defect is upstream: **the argument was never passed**,
/// and the honest fix is to not read it. Each of these supplies no name, which is exactly
/// what the caller said.
fn posix_pthread_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_create(&[args[0], args[1], args[2], args[3], 0, 0])
}

/// `pthread_cond_init(cond, attr)` - two arguments, and no name. See [`posix_pthread_create`].
fn posix_pthread_cond_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_cond_init(&[args[0], args[1], 0, 0, 0, 0])
}

/// `pthread_mutex_init(mutex, attr)` - two arguments, and no name. See [`posix_pthread_create`].
fn posix_pthread_mutex_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    pthread_mutex_init(&[args[0], args[1], 0, 0, 0, 0])
}

/// `pthread_detach(thread)` - says nobody will join this thread.
///
/// **Recorded rather than acted on.** Detaching is a promise about who cleans up, and every
/// guest thread here is a host thread the runtime already cleans up when its body returns -
/// so the promise is kept by construction and there is nothing for this to do but agree.
///
/// A handle nobody issued is still refused: an arbitrary guest value arriving here must never
/// be treated as a thread.
///
/// Reference: POSIX.1-2008 `pthread_detach(3)`.
fn pthread_detach(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if thread::is_issued(args[0]) {
        OK
    } else {
        u64::from(GuestError::InvalidHandle.as_raw())
    }
}

/// `pthread_exit(value)` - ends the calling thread, and never comes back.
///
/// # The two cases, and the one thing that tells them apart
///
/// On the **main** thread this ends the program, and that is reported as the guest exiting
/// deliberately - a different outcome from a fault, and the distinction that matters most in
/// a report (D177).
///
/// On any **other** thread it ends only that thread. Nothing here can unwind guest frames, so
/// the thread is parked instead of returned from: it stops executing guest code, which is
/// what `pthread_exit` promises, and its stack is not reclaimed, which is what this cannot
/// do. A run has a time limit, so a parked thread costs the run nothing it was not already
/// spending.
///
/// The two are told apart by the thread registry: the process's first thread runs guest code
/// without ever having been created, so it is *adopted* rather than spawned.
///
/// Reference: POSIX.1-2008 `pthread_exit(3)`.
fn pthread_exit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if thread::current() == thread::adopt("main") {
        orbistoun_core::stop(orbistoun_core::StopReason::Exited, args[0])
    } else {
        eprintln!(
            "orbistoun: a guest thread ended itself with pthread_exit - parked rather than unwound, because nothing here can unwind guest frames"
        );
        loop {
            std::thread::park();
        }
    }
}

/// Handlers a guest has installed, by signal number.
///
/// A `BTreeMap` behind a mutex rather than an array indexed by signal, because nothing
/// measured says what the valid range is - see [`raise_exception`] - and an array would have to
/// pick one.
fn installed_handlers() -> &'static Mutex<std::collections::BTreeMap<u64, u64>> {
    static HANDLERS: OnceLock<Mutex<std::collections::BTreeMap<u64, u64>>> = OnceLock::new();
    HANDLERS.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// `sceKernelInstallExceptionHandler(signum, handler)` - register a function to run on a signal.
///
/// # What was measured, and where
///
/// obSCEne's `030-thread/exception-handler`, sweep 20260909-140114, payload leg, against the
/// console's own `libkernel` export at `0x800028660`:
///
/// | call | return |
/// |---|---|
/// | `install(30, handler)` | `0x0` |
/// | `install(30, other)` a second time | `0x80020023` - errno 35 |
/// | `install(30, NULL)` while 30 is installed | `0x80020023` - errno 35 |
///
/// The third row is the one worth having: passing null is **not** an uninstall, because the
/// duplicate check happens first. `sceKernelRemoveExceptionHandler` is the way back out.
///
/// **This is PPSA25872's wall, and the placeholder was the wrong shape.** The title installs a
/// handler for signal 30 and then raises it on its own main thread; orbistoun answered
/// `0x7fff0001`, which is not zero, so as far as the guest could tell the registration failed.
///
/// Argument order is measured rather than assumed: an inverted call answers `EINVAL`, and the
/// guest's own handler begins `cmp edi, 0x1e` - it expects to be handed 30 (D645, D648).
fn install_exception_handler(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // Installed here rather than during setup: delivery matters only once a guest has a handler,
    // and a `OnceLock` makes the repeat calls free. It has to be wired before the first raise,
    // and nothing can raise before something installs.
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

/// `sceKernelRemoveExceptionHandler(signum)` - undo an install.
///
/// Measured as `0x0` for a signal that has a handler. **The other case is not measured**: nothing
/// was asked to remove a handler that was never installed, so that branch answers the loud
/// placeholder rather than guessing between `EINVAL`, `ESRCH` and success.
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
/// Installed into [`sync`] as the delivery half of its hook pair, so a thread asleep in a wait
/// can run a handler raised on it from elsewhere. A plain `fn` because that is how it is stored -
/// the same reason the call-budget callback is one.
///
/// Silent when nothing is pending, which is the ordinary case: every wait consults this, and
/// almost none of them find anything.
fn deliver_pending_signal() {
    let Some(signum) = thread::take_pending() else {
        return;
    };
    let installed = match installed_handlers().lock() {
        Ok(handlers) => handlers.get(&signum).copied(),
        Err(_) => None,
    };
    let Some(handler) = installed else {
        // The handler was removed between the raise and the wake. Hardware would take the
        // signal's default action here; this thread is mid-wait inside an emulator, and ending
        // the process for a race is worse than dropping a signal nothing was listening for.
        return;
    };
    // **The same value in both, as measured.** `rsi` and `rdx` carried the identical pointer on
    // hardware, and a handler that compares them would notice if they did not here.
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

/// How far into the interrupted thread's stack the context is placed.
///
/// **Its own stack, because that is where the console puts it**, and because an invented region
/// is unbounded to whatever walks it. Pointed at a reserved block of orbistoun's own, PPSA25872
/// walks forward from the context until it leaves the region - `0x368` past the end, at 4 KiB and
/// again at 2 MiB. It is scanning, not reading a struct, and a scan is bounded by the allocation
/// it is in. On the thread's own stack it is bounded by memory the guest already owns (D656).
///
/// The low end, because a guest stack grows down: the frames in use are at the top, and the
/// bottom is the furthest thing from them this crate can choose without knowing the parked
/// thread's guest stack pointer - which it does not record.
const CONTEXT_INTO_STACK: u64 = 0x1000;

/// Offset of the signal number within the context.
///
/// Measured: `1e00000000000000` at +0x48 while delivering signal 30
/// (obSCEne `030-thread/exception-handler`, sweep 20260909-175052).
const CONTEXT_SIGNAL: u64 = 0x48;

/// Offset of the pointer PPSA25872's handler reads.
///
/// Measured as an address on the interrupted thread's own 2 MiB stack, `0x7e8` past the context
/// itself, confirmed mapped by `sceKernelVirtualQuery`. What it *holds* is whatever that thread
/// had there; only the pointer's existence and reachability are facts.
const CONTEXT_INNER: u64 = 0xf8;

/// How far past the context the measured inner pointer pointed.
const CONTEXT_INNER_DELTA: u64 = 0x7e8;

/// How much of the context is written.
///
/// obSCEne dumped `0x180` bytes from the pointer a real handler was given. The structure may be
/// larger; nothing measured says so, and the stack beyond it is already zero.
const CONTEXT_WRITTEN: u64 = 0x180;

/// Builds the block a handler is handed in `rsi` and `rdx`, and answers its address.
///
/// # What is measured, and what is deliberately zero
///
/// The console hands a handler a pointer to a structure on the interrupted thread's own stack.
/// Four things about it were measured and are reproduced: it lives on that thread's stack, the
/// signal number sits at `+0x48`, a pointer at `+0xf8` refers to another address on the same
/// stack, and `rsi` and `rdx` carry the identical value.
///
/// **Everything else is zero, including what the inner pointer points at.** The register frame at
/// `+0x100..+0x170` is real state on the console and orbistoun has none to put there - the
/// handler ran because a *different* thread raised the signal, and the interrupted thread's
/// registers are not this process's to read. Plausible values would let a guest resume onto them;
/// zero makes it read a null and check, the same trade the zeroed data blocks make (D323, D656).
///
/// [`None`] when the calling thread has no recorded guest stack, in which case the handler is
/// handed zero and faults at a named address rather than reading somewhere arbitrary.
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

/// Whether the calling thread has a signal waiting - the cheap half of the hook pair.
fn signal_is_pending() -> bool {
    thread::signal_pending()
}

/// `sceKernelRaiseException(thread, signum)` - run a thread's handler for a signal.
///
/// # The whole contract, measured
///
/// obSCEne's `030-thread/exception-handler`, sweep 20260909-151910, calling with the leg's own
/// `scePthreadSelf()` handle - which is what the previous sweep could not do, and why this was
/// left unimplemented until now (D648):
///
/// | call | return |
/// |---|---|
/// | `raise(self, 30)`, handler installed | `0x0` |
/// | `raise(self, 31)` | `0x8002_0016` - `EINVAL` |
/// | `raise(1, 30)` | `0x8002_0003` - `ESRCH` |
/// | `raise(self, 30)`, no handler | no return - the process takes the signal and dies |
///
/// **Delivery is synchronous on the calling thread.** The probe's handler sets a flag, and the
/// flag reads `1` on the line after `raise` returns. That single bit decided the implementation:
/// the handler is called here and now, nested on this thread, and `raise` answers `0` afterwards.
/// An asynchronous model - queue it, deliver at some later point - was the other candidate and is
/// ruled out rather than merely unchosen.
///
/// The handler receives the signal number in `rdi` (measured as `0x1e`, matching the `cmp edi,
/// 0x1e` PPSA25872's own handler opens with) and a context pointer in both `rsi` and `rdx`.
///
/// # What is passed for the context, and why it is null
///
/// On the console those two registers carry a pointer to the interrupted thread's exception
/// context - a structure whose layout is not published and which orbistoun does not build. Null is
/// passed rather than a plausible block: a handler that only reads its signal number is unaffected,
/// and one that walks the context faults immediately at a named address instead of reading
/// invented fields and continuing. That is the same trade the zeroed data blocks make (D323).
///
/// # The signal number
///
/// 30 is accepted and 31 is refused, both measured with a valid handle. Nothing says where the
/// boundary is, so nothing generalises: an unmeasured number answers the loud placeholder rather
/// than being folded into either case.
fn raise_exception(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// The signal PPSA25872 raises, and the only one measured as accepted.
    const DELIVERABLE: u64 = 30;
    /// Measured as refused, with a valid thread handle, in the same sweep.
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
        // Measured: the process takes the signal and dies. Stopping the guest is the honest
        // emulation of that - and saying which signal did it is the half a raw exit could not.
        orbistoun_core::stop(orbistoun_core::StopReason::Signalled, signum);
    };
    // **Cross-thread, which is what PPSA25872 actually does**: it raises on `main` from a
    // collector thread. Hardware delivers that whatever the target is doing. Orbistoun can only
    // reach a target parked in a wait it owns, so `raise_pending` answers whether this one is -
    // and a target anywhere else is refused rather than told a handler ran (D652).
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

/// `sceKernelMapperGetParam(out)` - fills a size-prefixed structure and answers `0`.
///
/// Measured on FW 12.40: obSCEne `137-kernelcall/mapper-param` and `166-agc/mapper-after-init`
/// answer `0` and fill the 48 bytes after the 56-byte structure's leading size quadword with
/// [`MAPPER_PARAM`], leaving the size word as the caller wrote it. Earthion aborts on any non-zero
/// answer here (D677). Only the bytes the caller's size declares are written, so a smaller
/// structure is never overrun.
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

/// The 48 bytes `sceKernelMapperGetParam` writes after the size quadword, as the console wrote them:
/// `0x80000000000`, then `0x88000000000` three times, then `0x40` and `0x2663`.
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

/// Every implementation, as one table.
///
/// A constant rather than a literal inside the function above, because a list is not a
/// hundred lines of *logic* and a line limit that says otherwise is measuring the wrong
/// thing. The function stays as the interface every other crate calls.
const TABLE: &[(&str, GuestFn)] = &[
    ("sceKernelDirectMemoryQuery", direct_memory_query),
    ("sceKernelGetSystemSwVersion", get_system_sw_version),
    ("sceKernelDlsym", dlsym),
    ("pthread_detach", pthread_detach),
    ("pthread_setcancelstate", posix_pthread_setcancelstate),
    ("pthread_exit", pthread_exit),
    // POSIX thread-specific-data keys - no vendor twin, written here beside the thread
    // registry and served under their POSIX names via `orbistoun-posix` (D453).
    ("pthread_key_create", pthread_key_create),
    ("pthread_setspecific", pthread_setspecific),
    ("pthread_getspecific", pthread_getspecific),
    ("pthread_key_delete", pthread_key_delete),
    // The three whose POSIX spelling is one argument shorter than the vendor one, so the
    // POSIX spelling cannot simply delegate to it.
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
    // The barrier and read-write lock attribute accessors, registered (worklog 313).
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
    // The POSIX timed wait and once-only initialiser, registered (worklog 314).
    ("pthread_cond_timedwait", pthread_cond_timedwait),
    ("pthread_once", pthread_once),
    // The timed acquisitions, registered (worklog 315).
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
    // A condattr holds a clock and a pshared flag, neither of which anything observed sets, so
    // destroying one is accepting the call - the same shape as the mutexattr form beside it.
    // The block itself is never freed, by this family's own convention.
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
    // POSIX unnamed semaphores - no vendor twin, served under their POSIX names via
    // `orbistoun-posix` (D455).
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
    // The C-runtime threading family the C++ standard library lowers onto - declared in
    // `orbistoun-libc`, implemented here beside the thread registry and `sync` they rest on.
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
    // libSceUlt mutexes - declared in the `ult` module, implemented here beside `sync`.
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
    // Reported rather than answered - see each handler for what is established and what is not.
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

/// How much of a command buffer's storage to scan for content.
///
/// Eight kibibytes of words. The header claims twenty bytes of command, so anything at all
/// should be near the front - and every reading before this one looked at thirty-two bytes and
/// concluded the storage was empty, which is the same shape of mistake as reading a struct
/// field by assumption (D594).
const STORAGE_SCAN_WORDS: u64 = 1024;

/// How many non-zero words to name.
///
/// Enough to see a twenty-byte command whole, and few enough that a buffer full of asset data
/// reports a sample rather than eight kibibytes on the guest's own stack (principle 9).
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
/// Each entry is written at its full width, since a guest reads a size slot it did not initialise.
///
/// A path the title's own index names (D591) resolves to the index's identifier and size. Any
/// other path gets the unresolved answer: there is no source for the id a console assigns a file
/// outside an index.
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
        // Written per entry: arrays of `count` elements at their measured widths.
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u32(ids + entry * 4, id) };
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u64(sizes + entry * 8, size) };
        // SAFETY: an address the guest passed for this call, valid by its contract.
        let _ = unsafe { guest::write_u32(statuses + entry * 4, 0) };
        if entry < MOST_PATHS_REPORTED {
            match answer {
                Some((id, size)) => {
                    eprintln!("orbistoun:   the index has {path} as entry {id}, {size} byte(s)");
                }
                None => {
                    eprintln!("orbistoun:   the index does not name {path}; answered unresolved");
                }
            }
            paths.push(path);
        }
        // The first unresolved path ends the call and later slots keep what the caller put there -
        // measured: the `sceKernelAprResolveFilepathsToIdsAndFileSizes` record in `libkernel.toml`.
        if answer.is_none() {
            break;
        }
    }
    apr::note_resolved(paths);
    if all_resolved {
        OK
    } else {
        // `-1`, as the console answered for a path it could not resolve.
        u64::from(u32::MAX)
    }
}

/// How many paths one call reports.
///
/// A bound rather than none: a title resolving a manifest could pass thousands, and a diagnostic
/// that prints all of them on the guest's own stack is an observation heavy enough to change
/// what it observes (principle 9).
const MOST_PATHS_REPORTED: u64 = 16;

/// `sceKernelAprSubmitCommandBufferAndGetResult(buffer, ...)` - reported, not answered.
///
/// Prints the command buffer's header, which `libSceAmpr`'s own accessors name: the console
/// exports `sceAmprCommandBufferGetSize`, `GetNumCommands` and `GetCurrentOffset`, so the three
/// words at the front are a size, a count and an offset in some order (obSCEne's hardware
/// census). **Which order is not established**, so they are printed as three numbers rather
/// than labelled - a label is a claim, and this has measured none of them (D587).
fn apr_submit_command_buffer(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let buffer = args[0];
    let words: Vec<String> = (0..8)
        // SAFETY: an address the guest passed for this call, valid by its contract.
        .filter_map(|i| unsafe { guest::read_u64(buffer + i * 8) })
        .map(|w| format!("{w:#x}"))
        .collect();
    eprintln!(
        "orbistoun: the guest submitted an asynchronous file command buffer at {buffer:#x} - first words {}",
        words.join(" ")
    );
    // **Where the non-zero bytes actually are.** The header claims one command of twenty bytes
    // and the storage it names is empty, so the commands are somewhere this has not looked.
    // Scanning reports offsets rather than assuming a layout, which is what every reading of
    // these fields has done so far - and each of those readings has had to be corrected (D591).
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
        eprintln!(
            "orbistoun:   non-zero words around it: {}",
            nonzero.join(" ")
        );
    }
    // **The pointer at `+0x10`, checked in the run that produced it.** It was read as unmapped
    // once, from an address typed in out of a *previous* run - and the arena moves between runs
    // (D582), so that pairing was never sound. Asked here, where the two cannot disagree.
    //
    // Guarded by the same test the argument dump uses, because an unpublished address
    // dereferenced from a guest call faults inside the emulator and reports as the guest's
    // fault (D580).
    // **The experiment, off unless asked for.** What the buffer means is not established, so
    // delivering the resolved file into it is a guess the guest grades - and one that changes
    // the program, which is why it is declared as intervening (D589).
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
            eprintln!(
                "orbistoun:   what it points at, {inner:#x}: {}",
                head.join(" ")
            );
            // **The whole storage, not its first eight words.** Every reading so far has looked
            // at the front and concluded it was empty; a command written at an offset would
            // have been invisible to all of them. Scanning says where the bytes are rather
            // than assuming where they should be, which is the lesson D591 already paid for
            // once (D594).
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
                eprintln!(
                    "orbistoun:   and the first {STORAGE_SCAN_WORDS} words of it are all zero"
                );
            } else {
                eprintln!("orbistoun:   non-zero in it: {}", found.join(" "));
            }
        } else if let Some((start, end)) = region_containing(inner) {
            // **Mapped, and merely not published to the dump.** Two different findings, and
            // this crate is the one that can tell them apart: `region_containing` consults the
            // live map rather than the list something published for diagnostics (D580).
            eprintln!(
                "orbistoun:   it points at {inner:#x}, inside a mapping of {start:#x}..{end:#x} that nothing published for reading"
            );
        } else {
            eprintln!(
                "orbistoun:   it points at {inner:#x}, which this run never mapped - the guest is holding a buffer it was not given"
            );
        }
    }
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceKernelAprWaitCommandBuffer(...)` - reported, not answered.
///
/// The guest's own wrapper prints `waitCommandBufferCompletion error=%d` with whatever this
/// returns, which is how the name was found: forcing three unnamed imports to distinct values
/// in one run made the printed number name which of them the wrapper was reporting (D587).
fn apr_wait_command_buffer(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    eprintln!("orbistoun: the guest waited on an asynchronous file command buffer");
    u64::from(GuestError::Unimplemented.as_raw())
}
/// Reads the last resolved file into the buffer a command header names.
///
/// # What is guessed here, stated once
///
/// The header's third and fourth words are a length and an address matching a guest mapping
/// exactly, so they are taken as a buffer and its size (D587). **Which file** is taken from the
/// last resolve call, because the buffer's own command storage is empty and says nothing. Both
/// are guesses; the run report carries the caveat.
///
/// Split out of the submit handler because that function reports and this one changes the
/// program, and mixing the two would put an intervention inside something named for observing.
fn deliver_resolved_file(buffer: u64) {
    let Some(path) = apr::last_resolved() else {
        eprintln!("orbistoun: asked to deliver a file, and no resolve named one");
        return;
    };
    // SAFETY: two words of the command header the guest submitted, valid by the call's contract.
    let most = unsafe { guest::read_u64(buffer + 0x0c) };
    // SAFETY: as above.
    let into = unsafe { guest::read_u64(buffer + 0x10) };
    let (Some(most), Some(into)) = (most, into) else {
        eprintln!("orbistoun: asked to deliver {path}, and the command header could not be read");
        return;
    };
    // The length shares a word with the count above it, so only the low half is the size.
    let most = most & 0xFFFF_FFFF;
    match apr::deliver(&path, into, most) {
        Some(got) => eprintln!(
            "orbistoun: delivered {got} byte(s) of {path} into {into:#x} (up to {most:#x})"
        ),
        None => {
            eprintln!("orbistoun: asked to deliver {path}, and nothing installed a reader for it");
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
    /// ([`super::note_region`]/[`super::clear_noted_regions`]). cargo runs a crate's tests in
    /// parallel over one process, so without this one test's `clear` wipes another's `note`
    /// between that test's `note` and its assertion - which is exactly how
    /// `a_noted_region_is_found_and_a_gap_is_not` saw its base go missing. Only the two writers
    /// take it, because they are the only callers that clear. The lock guards the sequence, not
    /// a value; poison is recovered so a panicking holder does not cascade into the other test.
    fn noted_regions_serial() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `scePthreadGetaffinity` reads back the mask a thread was recorded with; an unknown handle
    /// is "anywhere" (zero), not an error; a null out-parameter is refused.
    ///
    /// Watched failing on the middle case: a getter that answered from an empty lookup with a wrong
    /// error code would read as a scheduling failure to a guest that tests the return (D523's hazard,
    /// one call over).
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

    /// **A set stack size and affinity are honoured; a fresh or zero size is not.**
    ///
    /// The decode `pthread_create` had been missing (REQ-...c2e9, obSCEne `031-stackattr`): it
    /// ignored the attribute block, so every thread ran on the 8 MiB default with default affinity.
    /// A console thread was measured running on exactly the `0x181000` its block asked for, so a set
    /// size passes through; a fresh attribute's 64 KiB default and POSIX's `0` both fall back to the
    /// working 8 MiB stack rather than shrink a thread to a size no measurement says bounds a real
    /// one. The affinity mask is carried through so the thread record - and `scePthreadAttrGet` after
    /// it - reads back what the block set. The fresh-attribute case is the negative that keeps the
    /// honouring real: without it a passthrough of every value would pass this test too.
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

    /// **A work-area size is something a caller can actually allocate.**
    ///
    /// The bug this closes, stated as the property that was violated. Unimplemented, both sizing
    /// calls answered the placeholder `0x7fff_0001` - and PPSA28061 passed it straight to
    /// `malloc`, **twice**, asking for 2 GiB a piece, and got it. A size is a number the caller
    /// spends, so a placeholder here is not a loud failure; it is a very quiet 4 GiB (D564).
    ///
    /// # What this cannot assert
    ///
    /// That the size is *right*. Nothing measured says what the console needs, and orbistoun
    /// stores nothing in the block - it is a number chosen to be spendable and proportional, and
    /// the test pins those two properties and no more.
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
            // Proportional, so a caller sanity-checking "more threads, more memory" is not
            // surprised - and asking for nothing still yields a real block.
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

    /// **A constructed Ult object leaves a handle this crate issued in its first word.**
    ///
    /// The convention `_sceUltMutexCreate` already set in this library, and the guest reads it
    /// back from there. A handle nothing issued would let a later call be told a pool it invented
    /// is fine - the reason D524 gave the event queues a table rather than a bare `Ok`.
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

    /// **A null object is refused rather than written through.**
    ///
    /// # A break that did not fire, and why
    ///
    /// Removing the `out == 0` check alone does **not** fail this: `write_word` refuses a null
    /// destination too, so the property survives on a second check downstream. A break removing
    /// both does fire. Recorded because the two are not the same guarantee - the explicit check
    /// says *this call refuses null*, and `write_word`'s says *this process will not write
    /// through null*, which is a different promise made for a different reason (D564).
    #[test]
    fn constructing_an_ult_object_into_nothing_is_refused() {
        assert_ne!(
            super::ult_runtime_create(&args([0, 0, 16, 3])),
            super::OK,
            "a null destination was accepted, which is a write through null"
        );
    }

    /// **The two out-parameters are four bytes apart, and neither may take the other with it.**
    ///
    /// This is D272's lesson in the one place it is unmissable. A `sched_param` is a single
    /// `int`, so a caller putting a policy and a param on its stack together puts them
    /// **adjacent** - PPSA02664 passes `0x…c86c` and `0x…c868`. An eight-byte write to either
    /// destroys the other, and the guest would read a policy it never set with nothing in any
    /// trace to say why.
    ///
    /// # What this cannot assert
    ///
    /// That the structure *is* one `int`. It is POSIX's `sched_param` and the guest's own
    /// spacing agrees, but a vendor field beyond it would sit past what this writes and read as
    /// whatever the caller left there.
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

    /// **A get hands back what a set was given, policy and priority both.**
    ///
    /// The property D523 said would be needed the moment anything read one back, and
    /// `scePthreadGetschedparam` is called 34 times by PPSA02664 - so it is read back.
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

    /// **Setting a priority alone does not reset the policy.**
    ///
    /// `scePthreadSetprio` names one value, so touching the other would change something the
    /// caller never mentioned - and a later get would report it as though the guest had asked.
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

    /// **A handle this crate never issued is refused by every one of them.**
    ///
    /// Handles are addresses (D272's family), so an arbitrary guest value must never be treated
    /// as one - accepting it would make a bad pointer from the guest into a write through it
    /// here. Asserted across the whole set rather than one of them, because it is the kind of
    /// check that gets added to the function being worked on and forgotten on its neighbours.
    ///
    /// # A break that did not fire, and why
    ///
    /// Removing the `is_issued` preamble from `pthread_rename` does **not** fail this - the
    /// property survives on `thread::rename`'s own refusal, which is a second check downstream.
    /// The two only both disappear together, and a break that removes both does fire. Recorded
    /// because a guard that cannot tell which of two checks is holding it up is a guard that
    /// would not notice one of them rotting: `is_issued` covers a handle handed out whose record
    /// never landed, and nothing else here would see that.
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

    /// **Readability is answered live, and only for a range one region wholly covers.** A region
    /// noted after the question was first asked is found the next time - the property the submit
    /// path needs for a command buffer mapped after entry (worklog 815) - and a range that starts
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

    /// Hex to bytes, for the console's own records.
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

    /// An unresolved path fills each slot at the width obSCEne `040-file/apr-resolve-filepaths` measured.
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

    /// **The mapper answers what the console wrote** (obSCEne `166-agc/mapper-after-init`): `rc 0`,
    /// the size quadword untouched, the 48 bytes after it byte for byte - and a smaller declared
    /// size is never overrun.
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

    /// **An address in no region is `EACCES`**, the code the console answered for
    /// `0x720000240000` (obSCEne `020-memory/virtual-query-unmapped`).
    #[test]
    fn a_query_of_nothing_is_refused_as_the_console_refuses_it() {
        let mut info = [0_u8; super::VIRTUAL_QUERY_INFO_BYTES];
        let at = std::ptr::from_mut(&mut info[0]) as usize as u64;
        assert_eq!(
            super::virtual_query(&args([0x0000_0000_0001_0000, 0, at, 72])),
            u64::from(super::GuestError::vendor(orbistoun_core::errno::DENIED).as_raw())
        );
    }

    /// A region the worker notes - the image, or a stack - is found by [`super::region_containing`],
    /// so `sceKernelVirtualQuery` answers for the guest's own code and stack rather than refusing
    /// them the way a lookup against the runtime map alone did (D446).
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

    /// **The resolver refuses rather than inventing an address** (D366).
    ///
    /// A name nothing implements has no stub, and answering one anyway hands the guest
    /// something to call that is not what it asked for - the D125 class, one layer up.
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
    ///
    /// A runtime resolves a name once and keeps it, but a second initialisation pass does
    /// not - and a line per call would bury the work list under repeats of its own first
    /// entry.
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
        // The call that was two steps before a `memset` through null, twice. Refused, the
        // guest cleared a buffer it never got.
        //
        // Note what is *not* asserted: that the address is non-zero. Physical offset zero
        // is a real place in this pool and the first allocation legitimately lands there.
        // Asserting otherwise was this test's first version, and it failed for the right
        // reason.
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
        // The address is the entire answer. Succeeding without delivering it leaves the
        // guest believing it owns memory it cannot name.
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
        // A guest asks for a stronger alignment because its hardware needs it. An address
        // that ignores the request works everywhere except where it matters.
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
        // Rounding a non-power-of-two into shape answers a question nobody asked.
        //
        // This caught a real bug: the alignment was widened to the pool's minimum before
        // being checked, so every nonsense value became a power of two and the check
        // could never fire.
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
        // Zero is what a caller with no alignment requirement passes, which is most of
        // them. Refusing it would refuse the ordinary case.
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
        // The wall PPSA28061 died at: allocate physical memory, ask for somewhere to
        // reach it, get nothing, write through null.
        let mut addr = 0_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(addr) as usize as u64;
        args[1] = 0x10000;
        args[2] = 3; // read | write

        assert_eq!(super::map_named_direct_memory(&args), 0, "should map");
        assert_ne!(addr, 0, "and the guest must be told where");

        // Mapped for real, not just recorded: the whole point is that the guest can use
        // it, and a bookkeeping entry that is not backed by memory faults on first touch.
        // SAFETY: the address was just reserved read-write by this call, and the length
        // written is well inside the region asked for.
        unsafe { std::ptr::write_volatile(addr as usize as *mut u64, 0x1234) };
        // SAFETY: as above - reading back what was just written to a live reservation.
        let read_back = unsafe { std::ptr::read_volatile(addr as usize as *const u64) };
        assert_eq!(read_back, 0x1234);
    }

    /// **Mappings that touch are one range; a gap or a read-only piece refuses a write across them.**
    ///
    /// The SDK maps its scanout surface as sixteen 2 MiB pieces end to end, and a fill of a 1080p
    /// target crosses four; a single-region rule refused it (worklog 816). Two writable 64 KiB pieces
    /// placed back to back, then a read-only one: a write across the first two is allowed, across into
    /// the third is not (though a read is), and past the third - a gap - nothing is.
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
        // Reusing an address a guest still holds a pointer into produces corruption that
        // looks like anything except a mapping bug.
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
        // **The aliasing property**, and this test replaced one asserting the opposite.
        // A guest allocates a range, maps it, loads a file into the address it was given,
        // and maps that range again expecting its data still to be there. Handing back
        // fresh memory is silent, total data loss faulting nowhere near the cause (D174).
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

        // Write through the address the guest was given, then map the same physical range
        // again - the whole point is that what was written is still there.
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
        // A mapping the guest cannot touch is indistinguishable from a failed one, and
        // it faults somewhere with no relation to the cause.
        let none = super::protection_from_guest(0);
        assert!(none.read, "no request must not mean no access");
        assert!(!none.write);
    }

    #[test]
    fn the_posix_protection_bits_are_translated_in_the_right_order() {
        // Read 1, write 2, execute 4 - the published POSIX values. Transcribing them in
        // the intuitive-but-wrong order maps data as executable and text as writable.
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
        // **Nothing reachable from a guest call may panic.** The frame was entered across
        // a `sysv64` boundary and unwinding through it is undefined - it does not surface
        // as a panic message, it surfaces as an unattributable fault in host code, which
        // is exactly how this was found (D156).
        //
        // The all-ones word is not hypothetical: it is what a caller passes to mean "no
        // preference", and rounding it up to an alignment overflows.
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
        // Same rule, same reason: rounding a length up to the pool alignment overflows.
        let mut physical = u64::MAX;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = u64::MAX;
        args[3] = std::ptr::addr_of_mut!(physical) as usize as u64;
        assert_ne!(super::allocate_main_direct_memory(&args), 0);
        assert_eq!(physical, u64::MAX, "and nothing was written back");
    }

    #[test]
    fn asking_who_you_are_never_answers_no_thread() {
        // The process's first thread runs guest code without the guest ever having
        // created it, and the guest still asks. Answering zero would make every
        // unadopted thread compare equal to every other one.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        let me = super::pthread_self(&args);
        assert_ne!(me, super::thread::NO_THREAD);
        assert_eq!(super::pthread_self(&args), me, "and the answer is stable");
    }

    #[test]
    fn creating_a_thread_with_nowhere_to_put_the_handle_is_refused() {
        // Writing to address zero is the alternative, and it faults inside the emulator
        // rather than naming the guest's mistake.
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
        // The whole path the guest uses: a handle written into its memory, then read
        // back out of it on every later call.
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
        // A guest that filled the location at compile time never called init, so the
        // value there names nothing we made. Returning success would let every thread
        // through the critical section at once and the corruption would be blamed on
        // whatever the lock was protecting (principle 3).
        let mut slot = 0x1234_5678_u64;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = std::ptr::addr_of_mut!(slot) as usize as u64;
        assert_ne!(super::pthread_mutex_lock(&args), 0, "not a lock we made");
    }

    #[test]
    fn joining_a_thread_that_does_not_exist_is_refused() {
        // Blocking forever is the alternative, and it looks identical to a guest
        // deadlock from the outside.
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
        // Every other test here could pass against a thread model that never runs
        // anything. This one generates real machine code, hands it to the guest
        // interface, and only passes if that code executed on a thread of its own.
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
    /// wait-on-address library declared beside it under the name the guest imports it by (D572).
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
        /// **Where a symbol is declared is a claim about the target; where its code lives is
        /// a claim about this repository** (D367). These two are POSIX thread calls with no
        /// vendor-named twin, and a title was measured importing them from `libScePosix` - so
        /// that is where they are declared, and the code is here because this is where the
        /// thread registry is.
        const DECLARED_ELSEWHERE: &[&str] = &[
            "pthread_detach",
            "pthread_exit",
            "mmap",
            // `std::call_once`'s engine: a libc symbol, declared in `orbistoun-libc`, implemented
            // here because it runs a guest callback through the thread registry's reentrant call.
            "_ZSt13_Execute_onceRSt9once_flagPFiPvS1_PS1_ES1_",
            // The C-runtime threading family, for the same reason: libc symbols declared in
            // `orbistoun-libc`, implemented here where the thread registry and `sync` primitives are.
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
            // libSceUlt mutexes: declared in the `ult` module, implemented here for `sync`.
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
            // The runtime and pool setup, same reason: declared in the `ult` module,
            // implemented here beside the table that holds them (D564).
            "sceUltInitialize",
            "sceUltWaitingQueueResourcePoolGetWorkAreaSize",
            "sceUltUlthreadRuntimeGetWorkAreaSize",
            "_sceUltWaitingQueueResourcePoolCreate",
            "_sceUltUlthreadRuntimeCreate",
            // And three more, for a second reason: these are the POSIX spellings of calls
            // this library *does* declare under vendor names, and they exist separately
            // because the POSIX signature is one argument shorter (D385).
            "pthread_create",
            "pthread_cond_init",
            "pthread_mutex_init",
            // Thread-specific-data keys and POSIX unnamed semaphores: declared in the POSIX
            // module under their POSIX names and implemented here, beside the thread registry
            // and the vendor semaphore calls whose primitives they share (D453, D455).
            "pthread_key_create",
            "pthread_setspecific",
            "pthread_getspecific",
            "pthread_key_delete",
            "sem_init",
            "sem_wait",
            "sem_trywait",
            "sem_post",
            "sem_destroy",
            // The timed acquisitions, added in bulk (worklog 315):
            // declared in the POSIX module, implemented here beside the primitives
            // whose deadlines they carry. There are no vendor twins for these.
            "pthread_mutex_timedlock",
            "pthread_rwlock_timedrdlock",
            "pthread_rwlock_timedwrlock",
            "sem_timedwait",
            "sem_reltimedwait_np",
            "sem_getvalue",
            "pthread_cond_reltimedwait_np",
            // The POSIX timed wait and once-only initialiser, added in bulk (worklog 314):
            // declared in the POSIX module, implemented here beside the condition
            // variables and the C++ runtime once-flag they sit next to.
            "pthread_cond_timedwait",
            "pthread_once",
            // The barrier and read-write lock attribute accessors, added in bulk (worklog 313):
            // declared in the POSIX module, implemented here beside the locks they
            // configure. There are no vendor twins for these to be declared as.
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
            // The attribute accessors and `pthread_equal`, added in bulk (worklog 310).
            // Same argument as the semaphores above: declared in the POSIX module under
            // their POSIX names, implemented here beside the attribute object they read
            // and write. There are no vendor twins for these to be declared as.
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

        // An implementation nobody declared can never be reached: resolution goes
        // through the declared symbol list, so the two drifting apart would leave code
        // that looks written and never runs.
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

    /// **The software-version body is the configured display string then its packed integer, and
    /// it does not touch the size word.**
    ///
    /// `sceKernelGetSystemSwVersion` and syscall 649 return *different* numbers on one console:
    /// the firmware is 12.40 (syscall 649, from `machine.firmware`), and this call answers whatever
    /// the profile's `software_version` says. The reference profile carries `13.090.001` /
    /// `0x1309_0001`; an earlier cut hardcoded 12.40 and hardware refuted it, so this pins the
    /// encoding: string from offset 8, packed int at struct offset 0x24, size word (0..8) untouched.
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

    /// **A configured string longer than the field is truncated, not overrun into the integer.**
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

    /// **An unset software version refuses the call rather than inventing one**, and a null
    /// destination refuses too - the honest defaults `firmware` and `kernel_release` also keep.
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

    /// **A structure smaller than the whole one is accepted, and nothing past it is touched.**
    ///
    /// This test used to assert the opposite, on this project's own reasoning that a caller
    /// passing less wanted a different layout. A conformance run swept the declared size from 1
    /// to 256 on a target console and every one succeeded, so the refusal was invented here.
    ///
    /// The guard byte is the half worth keeping: accepting a short buffer is only safe if the
    /// write stops where the caller said it does, and a test that checked the return code alone
    /// would pass just as happily while scribbling past the end (D398).
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

    /// **A flag the console refuses is refused here, with the code it used.**
    ///
    /// It answered 0 and 1 and rejected 2 and 4, which is a measured boundary rather than a
    /// guess about which bits carry meaning (D398).
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
        // Two answers about the same machine. If they disagree, a guest sizes its heaps
        // against memory the walk will never show it. Both read the same `pool_bytes`
        // setting now, so the identity is by construction rather than by two constants
        // that happen to agree.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(
            super::direct_memory_size(&args),
            direct::configured().pool_bytes
        );
    }

    /// **The default pool is the retail figure, and the homebrew figure is a different measured
    /// value - not a rounding of it.**
    ///
    /// Both are hardware measurements of `sceKernelGetDirectMemorySize`: twelve gibibytes on a
    /// retail eboot leg (`REQ-...5d1c`), five on a homebrew payload (D398). The corpus is retail
    /// and every memory wall in it is retail, so twelve is the default; five is what a homebrew
    /// leg configures. This pins that the default did not quietly drift back to five - which would
    /// re-wall PPSA04263 - and that the two are genuinely different values rather than one.
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
        // The sequence a garbage collector runs to find the bottom of the stack it scans. A
        // placeholder here became a scan off the top of the real stack in three titles (D575).
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

        // A handle nobody registered is refused, not described - the attribute object would
        // otherwise carry a stack that exists nowhere.
        args = [0xdead_0000, attr_at, 0, 0, 0, 0];
        assert_eq!(
            super::pthread_attr_get(&args),
            u64::from(super::GuestError::InvalidHandle.as_raw())
        );
    }

    /// One static for this whole module, per D399: an instance declared inside a test would
    /// start its own cursor at zero and hand a neighbour the same address.
    static RANGE: orbistoun_mem::test_bases::Range =
        orbistoun_mem::test_bases::Range::nth(orbistoun_mem::test_bases::crates::KERNEL);

    #[test]
    fn a_range_is_covered_only_when_one_region_holds_all_of_it() {
        // Pure, so the rule is checked without any mapping existing. Both edges, because an
        // off-by-one either way is the difference between refusing a guest's own module and
        // letting a range run off the end of one.
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

    /// A guest may re-protect a region somebody else placed for it - its own module's pages -
    /// and a range covered by nothing is still refused.
    ///
    /// The wall this fixes: PPSA03416 asks for write access across its own module, is refused
    /// because `mappings()` never mapped it, checks the answer, and takes a failure path that
    /// ends on a trap (D577).
    #[test]
    fn a_noted_region_can_be_re_protected_and_a_gap_cannot() {
        let _serial = noted_regions_serial();
        let base = RANGE.take();
        let len = orbistoun_core::GUEST_PAGE_SIZE * 4;

        // Reserved through an address space of this test's own, so the global `mappings()` -
        // the one `mprotect` consults first - genuinely does not own it. That is the case
        // being tested: the region exists, and this crate did not make it. Held for the whole
        // test, because dropping it releases the pages.
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

    /// **A mapping the guest was given at its own address is not the arena.**
    ///
    /// The failure this catches is the one that would make the name a lie: a guest that asks
    /// for `0x5000_0000_0000` is honoured there (D459), and folding that into the arena's
    /// extent would stretch it across nineteen terabytes nothing placed anything in - so
    /// every stray pointer between the two would be named as a guest mapping.
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

    /// An arena nothing has used answers nothing, rather than a span of length zero.
    ///
    /// Watched failing: written as `end < MAPPING_BASE` the second assertion returns
    /// `Some((base, 0))`, which registers the region and leaves every address in it unnamed
    /// anyway - the reporter spells an unused slot with a zero length too, so the two states
    /// would have become indistinguishable at the one place they differ.
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
