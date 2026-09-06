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

pub mod direct;
pub mod sync;
pub mod thread;

use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

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
        // Four, for the four arguments the implementation reads. A dump shows six registers
        // because that is how many System V passes, not because the function takes six (D294).
        "sceKernelReserveVirtualRange" => 4,
        "sceKernelVirtualQuery" => 4,
        "sceKernelMprotect" => 3,
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
        "scePthreadAttrSetdetachstate" => 2, "scePthreadAttrGetdetachstate" => 2,
        "scePthreadAttrSetschedparam" => 2, "scePthreadAttrGetschedparam" => 2,
        // The **thread** form, not the attribute form. Arity 2 for the same reason the
        // attribute form has it - a subject and a mask - and PPSA02664 passes a small
        // bitmask in the second register across 62 calls, with leftovers after it (D523).
        "scePthreadSetaffinity" => 2,
        "scePthreadAttrSetschedpolicy" => 2, "scePthreadAttrSetinheritsched" => 2,
        "scePthreadAttrSetaffinity" => 2, "scePthreadAttrSetguardsize" => 2,
        "sceKernelReadTsc" => 0,
        "sceKernelGetTscFrequency" => 0,
        "sceKernelIsStack" => 1,
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
/// How much physical memory exists. Answered from the same model the query walks, so the
/// two cannot describe different machines.
fn direct_memory_size(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    direct::DIRECT_MEMORY_SIZE
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
    if read_word(flag) == Some(DONE) {
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
            write_word(flag, DONE);
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
    let inner = read_word(arg)?;
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
    if !write_word(mtx, handle) {
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
    if !write_word(args[0], handle) {
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
    let sec = read_word(pointer)?;
    let nsec = read_word(pointer + 8)?;
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
    let handle = sync::create_ult_object(&read_name(name), args[2], args[3]);
    if !write_word(out, handle) {
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
    let handle = sync::create(sync::Recursion::Allowed, &read_name(name));
    if !write_word(out, handle) {
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
        write_word(args[0], 0);
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
    let handle = sync::create_cond(&read_name(name));
    if !write_word(out, handle) {
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
        write_word(args[0], 0);
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
    if !write_word(out, next_ult_thread()) {
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
        return u64::from(GuestError::NoMemory.as_raw());
    };
    drop(guard);

    if !write_word(out, address) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

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

/// `sceKernelMapNamedDirectMemory(addr, len, prot, flags, physical, alignment)`.
///
/// Gives the guest a virtual address for physical memory it has already reserved. This is
/// the other half of `sceKernelAllocateMainDirectMemory`: the allocation answers *which*
/// physical memory, and this answers *where the guest can reach it*.
///
/// # How this was found
///
/// The ordered call tail (D154) ended with three calls and then a null write:
///
/// ```text
/// sceKernelAllocateMainDirectMemory(0x100000)
/// libc::0xa75420e43cad1cdc(...)
/// libkernel::0x8434cc175396c635(0x6000007ffcd8)
/// -> write to 0x0
/// ```
///
/// The hash was unnamed. Proposing candidate names and letting the hash confirm - the
/// ordinary clean-room method, nothing consulted - matched `sceKernelMapNamedDirectMemory`
/// exactly. The first argument being a guest stack address agrees: it is where the guest
/// wants to be told the answer (D155).
///
/// # What is honoured and what is not
///
/// The requested protection is applied. The name is the **seventh** argument and this
/// trampoline spills six, so it is not readable here at all - which costs a label in a
/// trace and nothing else.
///
/// Physical memory is not aliased. Two mappings of the same physical range get two
/// separate pieces of host memory, so a guest that writes through one and reads through
/// the other sees stale data. Nothing observed does that yet, and doing it properly needs
/// a shared-memory object rather than a reservation - recorded rather than pretended.
/// Virtual addresses already handed out, by the physical offset they were mapped from.
///
/// **Because physical memory has to alias itself.** A guest allocates a physical range,
/// maps it, loads a file into the address it was given, and later maps the same range
/// again expecting its data to still be there. Handing out fresh zeroed memory the second
/// time is a silent, total data loss - the guest reads zeroes from a buffer it filled, and
/// the fault lands wherever it first trusts the contents (D174).
///
/// This is not full aliasing: two *simultaneous* mappings of one physical range still get
/// one address rather than two, which would need a shared memory object rather than a
/// reservation. It is the case that actually occurs.
fn physical_mappings() -> &'static Mutex<std::collections::BTreeMap<u64, u64>> {
    static MAPPED: OnceLock<Mutex<std::collections::BTreeMap<u64, u64>>> = OnceLock::new();
    MAPPED.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

fn map_named_direct_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // On by default. The switch remains because turning a subsystem off is a useful thing
    // to be able to do while bisecting - it was off for one afternoon while a fault inside
    // this function went unexplained, and the cause turned out to be elsewhere (D178).
    if !direct::configured().map_direct_memory {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let (out, len, prot, physical, alignment) = (args[0], args[1], args[2], args[4], args[5]);

    // Already mapped? Then the guest gets the address it had, and its data with it. The
    // physical offset is the identity of the memory; the virtual address is just where it
    // is currently reachable.
    if let Ok(mapped) = physical_mappings().lock() {
        if let Some(existing) = mapped.get(&physical) {
            let existing = *existing;
            drop(mapped);
            return if write_word(out, existing) {
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
    let requested = read_word(out).unwrap_or(0);
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
    let placed = if space.owns(base, len) {
        space.protect(base, len, protection)
    } else {
        space.reserve(base, len, protection).map(|_| ())
    };
    if placed.is_err() {
        return u64::from(GuestError::NoMemory.as_raw());
    }
    drop(space);
    fill_mapping(base, len, protection);

    if !write_word(out, base) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    if let Ok(mut mapped) = physical_mappings().lock() {
        mapped.insert(physical, base);
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
        let Some(entry) = read_word(at) else {
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

/// One line for a run report: the event queues a guest made, and what it registered on each.
///
/// [`None`] when none was created, so a quiet run stays quiet.
///
/// **A queue with nothing registered is worth saying out loud.** `sceKernelAddUserEventEdge`
/// refuses a handle no queue answers, and a run where every registration was refused looks
/// exactly like a run that made none - which is the ambiguity that makes a check worse than no
/// check (D325, D524).
#[must_use]
pub fn equeue_summary() -> Option<String> {
    let queues = sync::equeue_summary();
    if queues.is_empty() {
        return None;
    }
    let named = queues
        .iter()
        .map(|(name, count)| format!("{name:?} ({count} event(s))"))
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
        read: read || !(write || execute),
        write,
        execute,
    }
}

/// Reads a machine word out of guest memory.
///
/// The mapping is identity, so a guest address is a host address (D014). An address the
/// guest never mapped faults here exactly as it would have faulted in the guest, and the
/// worker's fault reporter names it - which is more useful than a check that would turn
/// a guest bug into a quiet zero.
fn read_word(address: u64) -> Option<u64> {
    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    // SAFETY: the guest supplied this address as somewhere it keeps a word, which is the
    // same contract the real call has. Read unaligned because nothing guarantees the
    // guest aligned it, and an unaligned read through a `*const u64` is undefined
    // behaviour where the instruction itself is fine.
    Some(unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at)) })
}

/// Writes a 32-bit value where a guest expects an `int`.
///
/// **Four bytes, not eight, and the difference has bitten this crate twice.** A semaphore
/// handle is an `int` and writing a whole word through it put the top half in whatever the
/// guest kept next door (D210). The mutex attribute `Gettype` out-parameter is the same
/// shape - and there the neighbour was the caller's loop counter, so an eight-byte write
/// reset it every iteration and the check ran until the call budget stopped it (D272).
fn write_int(address: u64, value: u32) -> bool {
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    if at == 0 {
        return false;
    }
    // SAFETY: as `write_word`, but four bytes - a guest-supplied `int *` under an identity
    // mapping, written unaligned because the guest's alignment is its own business.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u32>(at), value);
    }
    true
}

/// Reads a guest `int` - four bytes, not eight.
///
/// The counterpart to [`write_int`], and it exists for the same reason (D272): a `sched_param`
/// is a four-byte structure and the guest packs it right against its neighbour. Reading eight
/// would take the neighbour with it.
fn read_int(address: u64) -> Option<u32> {
    let at = usize::try_from(address).ok()?;
    if at == 0 {
        return None;
    }
    // SAFETY: as `read_word`, but four bytes - a guest-supplied `int *` under an identity
    // mapping, read unaligned because the guest's alignment is its own business.
    Some(unsafe { std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u32>(at)) })
}

/// Writes a machine word into guest memory.
fn write_word(address: u64, value: u64) -> bool {
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    if at == 0 {
        return false;
    }
    // SAFETY: as `read_word` - a guest-supplied destination under an identity mapping,
    // written unaligned because the guest's alignment is its own business.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value);
    }
    true
}

/// Writes a block of bytes into guest memory.
///
/// For the structures a guest hands a buffer for and expects filled - a delivered event, here.
/// Refuses a null destination for the same reason [`write_word`] does.
fn write_block(address: u64, bytes: &[u8]) -> bool {
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    if at == 0 {
        return false;
    }
    // SAFETY: as `write_word` - a guest-supplied destination under an identity mapping, and
    // `bytes.len()` bytes of it, which is the size the guest asked to be filled.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::with_exposed_provenance_mut::<u8>(at),
            bytes.len(),
        );
    }
    true
}

/// Reads a NUL-terminated name the guest passed.
///
/// Bounded, because an unterminated string would otherwise walk until it hit an unmapped
/// page - and a name is cosmetic, so the trade is obvious. A truncated name in a trace is
/// a small annoyance; a fault raised while fetching one is a fault attributed to the
/// wrong thing entirely.
fn read_name(address: u64) -> String {
    /// Longer than any thread name observed, and short enough to stay within one page
    /// from almost any starting point.
    const MAX_NAME: usize = 64;

    let Ok(at) = usize::try_from(address) else {
        return String::new();
    };
    if at == 0 {
        return String::new();
    }
    let mut bytes = Vec::new();
    for offset in 0..MAX_NAME {
        // SAFETY: a guest-supplied string under the identity mapping, read one byte at a
        // time so the scan cannot straddle the end of a mapping by more than it reads.
        let byte = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u8>(at + offset)) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
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
    if !write_int(args[0], key) {
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
    let (out, entry, argument, name) = (args[0], args[2], args[3], args[4]);
    if out == 0 || entry == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let name = read_name(name);
    let start = thread::Start { entry, argument };

    // SAFETY: `entry` is a guest address the guest itself is asking to have called, in a
    // fully relocated image - the same contract the real call has. The thread body runs
    // guest instructions, which is the entire purpose of this emulator, and it runs on a
    // stack of its own so an overrun hits a guard page rather than host frames.
    let spawned = unsafe { thread::spawn(start, &name, thread::Affinity::default(), 0) };

    match spawned {
        Ok(handle) => {
            if !write_word(out, handle) {
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
        write_word(value, thread::exit_value(handle));
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
    let name = read_name(args[1]);

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
    let name = read_name(name);
    let handle = sync::create(mutex_recursion_from_attr(attr), &name);
    if !write_word(out, handle) {
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
    match read_word(object + ATTR_TYPE) {
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
    let handle = read_word(pointer)?;
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
    write_word(args[0], sync::NO_MUTEX);
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
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    // Microseconds: the unit every `GetProcessTime` in this family reports, and the one the
    // probe's own check compares two readings in. Saturating, so a run long enough to
    // overflow reports a stuck clock rather than a wrapped one.
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
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
    ticks_since(tsc_origin())
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
        let mut bytes = Box::new([SYSTEM_VERSION_FILL; SYSTEM_VERSION_BYTES]);
        bytes[SYSTEM_VERSION_AT..SYSTEM_VERSION_AT + 2].copy_from_slice(&version.to_le_bytes());
        std::ptr::from_mut(Box::leak(bytes)).cast::<u8>() as usize as u64
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
    std::thread::sleep(std::time::Duration::from_micros(args[0]));
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
        if !write_word(args[0].saturating_add(word * 8), 0) {
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
            if !write_word(args[2].saturating_add(index as u64 * 8), *word) {
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
        let Some(read) = read_word(args[1].saturating_add(index as u64 * 8)) else {
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
    if !write_block(args[0], &bytes) {
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
        if !write_word(args[0].saturating_add(word * 8), u64::MAX) {
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
    let Some(current) = read_word(at) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if !write_word(at, current | bit) {
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
    let Some(current) = read_word(at) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if !write_word(at, current & !bit) {
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
    let Some(current) = read_word(args[0].saturating_add(word * 8)) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    u64::from(current & bit != 0)
}

/// Resolves the condition variable a guest pointer refers to.
fn cond_at(pointer: u64) -> Option<sync::CondHandle> {
    let handle = read_word(pointer)?;
    (handle != 0).then_some(handle)
}

/// `scePthreadCondInit(cond, attr, name)`.
fn pthread_cond_init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let handle = sync::create_cond(&read_name(args[2]));
    if !write_word(args[0], handle) {
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
    write_word(args[0], 0);
    OK
}

/// Resolves the read/write lock a guest pointer refers to.
fn rwlock_at(pointer: u64) -> Option<sync::RwlockHandle> {
    let handle = read_word(pointer)?;
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
    let handle = sync::create_rwlock(&read_name(name));
    if !write_word(lock, handle) {
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
    write_word(args[0], 0);
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
    let handle = sync::create_barrier(needed, &read_name(name));
    if !write_word(barrier, handle) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadBarrierWait(barrier)`.
fn pthread_barrier_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = read_word(args[0]).filter(|h| *h != 0) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    match sync::barrier_wait(handle) {
        Some(_) => OK,
        None => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `scePthreadBarrierDestroy(barrier)`.
fn pthread_barrier_destroy(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(handle) = read_word(args[0]).filter(|h| *h != 0) else {
        return u64::from(GuestError::InvalidHandle.as_raw());
    };
    if !sync::barrier_destroy(handle) {
        return u64::from(GuestError::InvalidHandle.as_raw());
    }
    write_word(args[0], 0);
    OK
}

/// `sceKernelCreateEventFlag(out, name, attr, initial, param)`.
fn kernel_create_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let handle = sync::create_event_flag(args[3], &read_name(args[1]));
    if !write_word(args[0], handle) {
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
    if args[1] == 0 || !write_word(args[1], seconds) {
        return u64::from(GuestError::vendor(orbistoun_core::errno::INVALID).as_raw());
    }
    // The second field of the structure the caller described, eight bytes on.
    if !write_word(args[1].saturating_add(8), nanos) {
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
    let handle = sync::create_equeue(&read_name(args[1]));
    if !write_word(args[0], handle) {
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
    if !write_int(args[1], record.requested_policy as u32)
        || !write_int(args[2], record.requested_priority as u32)
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
    let Some(priority) = read_int(args[2]) else {
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
    if thread::rename(args[0], &read_name(args[1])) {
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
    if args[1] != 0 && !write_int(args[1], previous as u32) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

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

    let events = sync::take_events(args[0], wanted);
    for (index, event) in events.iter().enumerate() {
        let at = args[1].saturating_add((index * sync::EVENT_BYTES) as u64);
        if !write_block(at, &event.to_bytes()) {
            return u64::from(GuestError::InvalidArgument.as_raw());
        }
    }
    // The count is written through `arg3` and the return is a status - the shape the argument
    // roles establish. Zero delivered is written as zero rather than skipped: a caller reading
    // a stale count would act on an event it was never given.
    if args[3] != 0 && !write_block(args[3], &(events.len() as u32).to_le_bytes()) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceKernelPollEventFlag(flag, pattern, mode, result, timeout)`.
///
/// **A miss is not an error.** Polling asks whether the pattern is set right now, and
/// answering an argument error when it is not would make a guest read an ordinary poll as
/// a broken handle.
fn kernel_poll_event_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// Bit in the mode meaning every bit of the pattern must be present.
    const WAIT_AND: u64 = 0x01;

    let Some(outcome) = sync::event_flag_poll(args[0], args[1], args[2] & WAIT_AND != 0) else {
        // ESRCH, the vendor code obSCEne's `015-sync/event-flag-rejects-bad-handle` measured
        // (`0x80020003`), not the `0x7fff…` placeholder a guest would fail to recognise (D125).
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_SUCH).as_raw());
    };
    match outcome {
        Some(bits) => {
            if args[3] != 0 {
                write_word(args[3], bits);
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
    /// Mode bit: every bit of the pattern must be present (AND); its absence means any (OR).
    const WAIT_AND: u64 = 0x01;
    /// Mode bit: clear every bit of the flag on a successful wait.
    const WAIT_CLEAR_ALL: u64 = 0x10;
    /// Mode bit: clear just the matched pattern on a successful wait.
    const WAIT_CLEAR_PAT: u64 = 0x20;
    /// FreeBSD `ETIMEDOUT`, under the measured `0x8002_0000` vendor mapping. The observed guest
    /// waits indefinitely, so this path is unexercised; the value is the published errno, not a
    /// measured one.
    const ETIMEDOUT: u32 = 60;

    let (flag, pattern, mode, result, timeout_ptr) = (args[0], args[1], args[2], args[3], args[4]);
    // NULL waits forever; otherwise the pointer holds a microsecond count.
    let timeout = if timeout_ptr == 0 {
        None
    } else {
        read_word(timeout_ptr).map(std::time::Duration::from_micros)
    };
    let Some(outcome) = sync::event_flag_wait(
        flag,
        pattern,
        mode & WAIT_AND != 0,
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
                write_word(result, bits);
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

/// `sceKernelPollSema(semaphore, need)` - takes without waiting.
fn kernel_poll_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, sync::Blocking::Never)) {
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
fn kernel_wait_sema(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, sync::Blocking::Forever)) {
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
    sema_at(read_word(sem)?)
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
    if !write_word(sem, u64::try_from(handle).unwrap_or(0)) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sem_wait(sem)` - take one, waiting for it.
fn sem_wait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, sync::Blocking::Forever)) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_trywait(sem)` - take one only if it is available.
fn sem_trywait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, sync::Blocking::Never)) {
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
    let block: Box<[u64; 4]> = Box::new([0; 4]);
    let handle = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    if !write_word(args[0], handle) {
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
    let block: Box<[u64; 4]> = Box::new([0; 4]);
    let handle = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    // **Not zero, which is what an empty block would have said.** A conformance run read the
    // type back out of a freshly initialised attribute on a target console and got 1, so a
    // guest that initialises an attribute and asks what it holds was being told the wrong
    // thing here - and a guest that *acts* on the answer builds a different kind of lock
    // (D398).
    write_word(handle + ATTR_TYPE, DEFAULT_MUTEX_TYPE);
    if !write_word(args[0], handle) {
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
    let handle = read_word(pointer)?;
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
    if !write_word(object + ATTR_TYPE, args[1]) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrGettype(attr, out)`.
fn pthread_mutexattr_gettype(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(value) = read_word(object + ATTR_TYPE) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if args[1] == 0 || !write_int(args[1], value as u32) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrSetprotocol(attr, protocol)`.
fn pthread_mutexattr_setprotocol(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if !write_word(object + ATTR_PROTOCOL, args[1]) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `scePthreadMutexattrGetprotocol(attr, out)`.
fn pthread_mutexattr_getprotocol(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(value) = read_word(object + ATTR_PROTOCOL) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if args[1] == 0 || !write_int(args[1], value as u32) {
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
    if !write_word(out, address) {
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
    // The reservation itself is not torn down here. orbistoun maps the guest's whole span
    // once at load and hands out pieces of it; releasing a piece back to the host would
    // put a hole in an address space the guest still believes is contiguous. Recorded as
    // an assumption rather than implied by this answering success (D273).
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
    fill_mapping(base, len, protection);
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
    let hint = read_word(addr_out).unwrap_or(0);
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
    for _ in 0..16 {
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
    fill_mapping(base, len, protection);
    // The reserved base, written back through the `void **` the guest passed - the documented shape,
    // status returned separately as success.
    if write_word(addr_out, base) {
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
    let (addr, info) = (args[0], args[2]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if info == 0 {
        return vendor(orbistoun_core::errno::INVALID);
    }
    // Both the runtime mappings and the regions the loader/worker noted (the image, the stack,
    // the TLS block), so a guest querying its own code or stack is answered rather than refused.
    let Some((start, end)) = region_containing(addr) else {
        return vendor(orbistoun_core::errno::NO_ENTRY);
    };
    if write_word(info, start) && write_word(info + 8, end) {
        OK
    } else {
        vendor(orbistoun_core::errno::INVALID)
    }
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
/// The range is re-protected against the same address space `mmap` and the reservation use, so
/// a typo cannot re-protect this process's own code (`AddressSpace::protect` refuses a range it
/// does not own). Two honest simplifications, both from orbistoun's identity-mapped model rather
/// than guessed:
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
    let Ok(mut space) = mappings().lock() else {
        return vendor(orbistoun_core::errno::INVALID);
    };
    let outcome = space.protect(addr, len, protection);
    drop(space);
    match outcome {
        Ok(()) => OK,
        // The range is not one orbistoun reserved. The console answers `EINVAL` for an address
        // that is not a valid mapping; this reports the same rather than inventing a code.
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
    if !write_word(args[0], direct::flexible_available()) {
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
    if !write_word(args[0], direct::flexible_configured()) {
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
    }
    result
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
    let block: Box<[u64; 16]> = Box::new([0; 16]);
    let handle = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    if !write_word(args[0], handle) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

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
    let block: Box<[u64; 4]> = Box::new([0; 4]);
    let handle = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    if !write_word(pointer, handle) {
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
    if read_word(control) == Some(DONE) {
        return OK;
    }
    // SAFETY: `routine` is a guest function pointer the caller handed over, and `call_guest`
    // runs it on a fresh stack through the thread registry's reentrant call - the same contract
    // `_Execute_once` relies on. It takes no arguments, so the registers are left zero.
    let ran = unsafe { thread::call_guest(routine, [0, 0, 0]) };
    if ran.is_none() {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    write_word(control, DONE);
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
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, until)))
}

/// `sem_reltimedwait_np(sem, reltime)` - the same wait, given a span instead.
fn sem_reltimedwait_np(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some(until) = deadline_after(args[1]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    timed_out(posix_sema_at(args[0]).and_then(|h| sync::semaphore_wait(h, until)))
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
    if write_int(out, value) {
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

/// Stores one field of a thread attribute object.
fn attr_set(args: &[u64; GUEST_ARG_REGISTERS], field: u64) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if !write_word(object + field, args[1]) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// Reads one field of a thread attribute object into a guest `int`.
fn attr_get(args: &[u64; GUEST_ARG_REGISTERS], field: u64) -> u64 {
    let Some(object) = attr_at(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(value) = read_word(object + field) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Four bytes: the out-parameter is an `int`, and eight would take the caller's
    // neighbouring variable with it (D272).
    if args[1] == 0 || !write_int(args[1], value as u32) {
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
    let Some(value) = read_word(object + ATTR_STACK_SIZE) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if args[1] == 0 || !write_word(args[1], value) {
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
    write_word(args[0], 0);
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
const TSC_HZ: u64 = 0x5f25_9b8e;

/// Nanoseconds in a second, for converting the host's clock to the target's rate.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// When the counter started, so a guest sees it advance from a small value.
fn tsc_origin() -> std::time::Instant {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

/// `sceKernelReadTsc()` - the time stamp counter.
///
/// **Must actually advance.** A stub answering a constant makes every elapsed measurement
/// zero, which reads as a sleep that returned instantly - and that is precisely what the
/// conformance probe reported before this existed (D275).
fn read_tsc(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    ticks_since(tsc_origin())
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
fn ticks_since(origin: std::time::Instant) -> u64 {
    let nanos = origin.elapsed().as_nanos();
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

/// `sceKernelIsStack(address)` - whether an address is in the calling thread's stack.
///
/// **Answers false when nothing told it where the stack is**, rather than guessing. A
/// wrong yes and a wrong no are both wrong, and only one of them is silent about it: a
/// guest told a static is stack memory may free it.
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
fn is_stack(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let address = args[0];
    let within = |(base, len): (u64, u64)| address >= base && address < base.saturating_add(len);
    if thread::this_stack().is_some_and(within) {
        return 1;
    }
    let Some(&span) = STACK_SPAN.get() else {
        return 0;
    };
    u64::from(within(span))
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
    if !write_int(written, count as u32) {
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
    let path = read_name(args[0]);

    // libkernel is always resident, at the handle every guest reaches it by (D264, D400).
    if path.contains("libkernel") {
        write_int(args[5], 0);
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
        write_int(args[5], 0);
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
    let name = read_name(name);
    // **The thunk table first, then the guest's own exports.** Purely additive: every name that
    // resolved before resolves to the same address, and a name that did not now reaches the
    // guest's own code instead of an error.
    //
    // Which of the two *should* win when both answer is not settled here, because nothing has
    // been seen where both do. A guest asking for a symbol its own binary exports plainly wants
    // its own code; a guest asking for a platform function wants orbistoun's. Ordering it the
    // other way would change what already-working names answer, on no evidence (D517).
    let address = orbistoun_thunk::name_thunk(&name).or_else(|| guest_export(&name));

    // **Every distinct name, once, answered or not.** A payload's resolution pass is the
    // clearest statement it ever makes of what it needs, and it makes it before doing
    // anything else - so this is the work list, and printing only the failures would throw
    // away the half that says what the runtime is built out of. The same shape as the
    // `sysctl` report, for the same reason (D366).
    if first_time_asked(&name) {
        let verdict = address.map_or_else(
            || "which nothing here implements".to_owned(),
            |at| format!("answered {at:#x}"),
        );
        let line = format!("orbistoun: the guest asked for the address of {name} - {verdict}");
        eprintln!("{line}");
        // **And to the kernel log**, which is what `klogsrv` forwards. A name the guest could
        // not resolve is the kernel talking about the process, which is exactly what belongs
        // there (D389).
        orbistoun_core::klog::note(&line);
    }

    let Some(address) = address else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if !write_word(out, address) {
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
    ("scePthreadAttrSetdetachstate", pthread_attr_setdetachstate),
    ("scePthreadAttrGetdetachstate", pthread_attr_getdetachstate),
    ("scePthreadAttrSetschedparam", pthread_attr_setschedparam),
    ("scePthreadAttrGetschedparam", pthread_attr_getschedparam),
    ("scePthreadSetaffinity", pthread_setaffinity),
    ("scePthreadAttrSetschedpolicy", pthread_attr_setschedpolicy),
    (
        "scePthreadAttrSetinheritsched",
        pthread_attr_setinheritsched,
    ),
    ("scePthreadAttrSetaffinity", pthread_attr_setaffinity),
    ("scePthreadAttrSetguardsize", pthread_attr_setguardsize),
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
        let name = b"waiting queue ";
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

    /// A region the worker notes - the image, or a stack - is found by [`super::region_containing`],
    /// so `sceKernelVirtualQuery` answers for the guest's own code and stack rather than refusing
    /// them the way a lookup against the runtime map alone did (D446).
    #[test]
    fn a_noted_region_is_found_and_a_gap_is_not() {
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
        let declared: Vec<&str> = super::MODULE.imports.iter().map(|i| i.name).collect();
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
        // against memory the walk will never show it.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::direct_memory_size(&args), direct::DIRECT_MEMORY_SIZE);
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
}
