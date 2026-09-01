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
        "sceKernelAllocateMainDirectMemory" => 4,
        "sceKernelGetDirectMemorySize" => 0,
        "sceKernelDirectMemoryQuery" => 4,
        "sceKernelGetSystemSwVersion" => 1,
        "sysctlbyname" => 5,
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
        "sceKernelCreateEventFlag" => 5, "sceKernelPollEventFlag" => 5,
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
    // the previous meaning was provably not the platform's. What `3` denotes - a type, or some
    // state - is still open, and one run distinguishes them: allocate with several types and
    // query each back. Sweeping it 0..10 changed nothing a guest reacted to, so this is a
    // conformance difference rather than one a title is waiting on (D083, D398).
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

/// The bytes a named `sysctl` knob answers with, or `None` for one orbistoun does not carry.
///
/// The names are FreeBSD's, because the target kernel is FreeBSD-derived (see `docs/REFERENCES.md`):
///
/// - `kern.ostype` answers `"FreeBSD"`. That is not a guess but the one fact this whole project
///   rests on stated back to the guest - a citable constant, the same way the C library is treated
///   as POSIX with vendor naming.
/// - `kern.osrelease` answers the **configured** machine's release ([`machine`]`().kernel_release`),
///   which is empty until a machine sets one - orbistoun does not invent a kernel version (the
///   reason `Machine::default().kernel_release` is empty). An unset release answers an empty,
///   NUL-terminated string: a knob that exists with no value, rather than an invented one. A
///   console measured `"0.0-prototype"` here (obSCEne `135-sysctl/osrelease`), which a machine
///   profile may carry, but the default does not pretend to know it.
///
/// Every other name is refused rather than answered with something plausible: a payload doing
/// firmware detection off a value nobody measured would take a path chosen by an invention
/// (principle 3), which is the exact failure a sibling emulator hit and obSCEne exists to avoid.
fn sysctl_value(name: &str, kernel_release: &str) -> Option<Vec<u8>> {
    let c_string = |text: &str| {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    };
    match name {
        "kern.ostype" => Some(c_string("FreeBSD")),
        "kern.osrelease" => Some(c_string(kernel_release)),
        _ => None,
    }
}

/// `sysctlbyname(name, oldp, oldlenp, newp, newlen)` - read a named kernel knob.
///
/// Left to the default stub the call refused, and obSCEne's `135-sysctl/osrelease` failed - a
/// refusal is what turns firmware detection off in a title that asks. This answers the knobs it can
/// source honestly (see [`sysctl_value`]) and follows the POSIX/FreeBSD contract: with a destination
/// it copies what fits and updates the length; with none it answers the size alone; and it never
/// writes past the length it was given, which is the overrun obSCEne guards for.
fn sysctlbyname(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (name_ptr, oldp, oldlenp) = (args[0], args[1], args[2]);
    let vendor = |errno| u64::from(GuestError::vendor(errno).as_raw());
    if name_ptr == 0 || oldlenp == 0 {
        return vendor(orbistoun_core::errno::FAULT);
    }
    let name = read_name(name_ptr);
    let Some(value) = sysctl_value(&name, &machine().kernel_release) else {
        return vendor(orbistoun_core::errno::NO_ENTRY);
    };
    let capacity = read_word(oldlenp).unwrap_or(0);
    if oldp != 0 {
        if (value.len() as u64) > capacity {
            // Too small: report the size the value needs and refuse, rather than truncate into a
            // buffer the caller sized itself. FreeBSD answers `ENOMEM` here.
            let _ = write_word(oldlenp, value.len() as u64);
            return vendor(orbistoun_core::errno::NO_MEMORY);
        }
        let Ok(dest) = usize::try_from(oldp) else {
            return vendor(orbistoun_core::errno::FAULT);
        };
        // SAFETY: `oldp` is a guest out-pointer under the identity mapping, and `value.len()` is
        // no more than `capacity`, the length the caller declared its buffer holds, so the write
        // stays inside it. An address the guest never mapped faults here as it would have in the guest.
        unsafe {
            std::ptr::copy_nonoverlapping(
                value.as_ptr(),
                std::ptr::with_exposed_provenance_mut::<u8>(dest),
                value.len(),
            );
        }
    }
    if !write_word(oldlenp, value.len() as u64) {
        return vendor(orbistoun_core::errno::FAULT);
    }
    0
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
    match sync::lock(handle, by) {
        Some(true) => thrd::SUCCESS,
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
    match sync::try_lock(handle, by) {
        Some(sync::TryLock::Locked) => thrd::SUCCESS,
        Some(sync::TryLock::Busy | sync::TryLock::Deadlock) => thrd::BUSY,
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
        sync::lock(handle, by);
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
    match sync::lock(handle, thread::adopt("main")) {
        Some(true) => OK,
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
    match sync::try_lock(handle, thread::adopt("main")) {
        Some(sync::TryLock::Locked) => OK,
        Some(sync::TryLock::Busy | sync::TryLock::Deadlock) => {
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
        sync::lock(handle, by);
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
/// Clear of the image, the stacks and the thunk table, so a stray pointer into any of
/// them is still recognisable by its address alone.
pub const MAPPING_BASE: u64 = 0x0000_7200_0000_0000;

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
/// [`mappings`] never sees them; [`virtual_query`] consults this alongside it, because a guest
/// asking about its own code or stack expects a mapping, not "nothing here" (D446).
fn noted_regions() -> &'static Mutex<Vec<(u64, u64)>> {
    static NOTED: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    NOTED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Records a `[base, base + len)` region the guest can read but this crate did not map, so
/// [`virtual_query`] answers for it. Stored as `(start, end)`; a region already noted is not
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

/// Writes a machine word into guest memory.
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
    // **Four bytes, not eight.** The destination is an `int *`, not a `void **` - a
    // semaphore handle and a mutex handle are different shapes, which obSCEne established
    // from the public interface documentation (D210). This wrote a full word until then,
    // so every `sceKernelCreateSema` put four bytes of handle into whatever the guest kept
    // next to its semaphore. Nothing here could have noticed: the write succeeds, the
    // handle round-trips, and the damage surfaces wherever that neighbour is read.
    //
    // SAFETY: the guest supplied this destination, which is the same contract the real call
    // has. Written unaligned because the guest's alignment is its own business, and an
    // address it has not mapped faults here exactly as it would have in the guest.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<i32>(out as usize),
            handle,
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
    match sync::lock(handle, by) {
        Some(true) => OK,
        // Two different failures, deliberately given two different codes. A refusal is
        // the guest deadlocking against itself on a non-recursive lock; a miss is a
        // handle naming nothing. Collapsing them would make the trace unable to tell a
        // guest bug from a gap in this crate.
        Some(false) => u64::from(GuestError::InvalidArgument.as_raw()),
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
    match sync::try_lock(handle, by) {
        Some(sync::TryLock::Locked) => OK,
        // Held by somebody else, or the owner re-taking a normal lock, which is the ordinary
        // outcome of this call rather than a misuse of it - the reason D256 gave it a code of
        // its own. **The console returned the busy errno** when asked to take a lock already
        // held, so the distinction is made with the value the machine uses (D398).
        Some(sync::TryLock::Busy) => {
            u64::from(GuestError::vendor(orbistoun_core::errno::BUSY).as_raw())
        }
        // The owner re-taking an error-checking lock, which the console reports with the
        // invalid-argument errno (`0x8002_0016`) rather than the busy a normal lock gives -
        // a distinct code for a distinct condition, measured in 015-sync/mutex-recursion (D416).
        Some(sync::TryLock::Deadlock) => {
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
        sync::lock(handle, by);
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
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, true)))
}

/// `scePthreadRwlockTryrdlock(lock)`.
fn pthread_rwlock_tryrdlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_read(h, false)))
}

/// `scePthreadRwlockWrlock(lock)`.
fn pthread_rwlock_wrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, true)))
}

/// `scePthreadRwlockTrywrlock(lock)`.
fn pthread_rwlock_trywrlock(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    acquired(rwlock_at(args[0]).and_then(|h| sync::rwlock_write(h, false)))
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
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let needed = u32::try_from(args[2]).unwrap_or(1);
    let handle = sync::create_barrier(needed, &read_name(args[3]));
    if !write_word(args[0], handle) {
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
    match sema_at(args[0]).and_then(sync::semaphore_try_wait) {
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
    match sema_at(args[0]).and_then(sync::semaphore_wait) {
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
    match posix_sema_at(args[0]).and_then(sync::semaphore_wait) {
        Some(true) => OK,
        _ => u64::from(GuestError::InvalidHandle.as_raw()),
    }
}

/// `sem_trywait(sem)` - take one only if it is available.
fn sem_trywait(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match posix_sema_at(args[0]).and_then(sync::semaphore_try_wait) {
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
    let block: Box<[u64; 8]> = Box::new([0; 8]);
    let handle = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    if !write_word(args[0], handle) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
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
/// **Refused, and that is the honest answer.** orbistoun places one executable at load and
/// has no way to bring another in afterwards, so every request fails - including one for a
/// module that really exists. Answering a handle would tell a guest a library it is about
/// to call is present.
///
/// Negative, because the success answer is a module handle and a small positive placeholder
/// is exactly what a handle looks like (D273).
fn load_start_module(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let path = read_name(args[0]);

    // libkernel is always resident, at the handle every guest reaches it by (D264, D400).
    if path.contains("libkernel") {
        write_int(args[5], 0);
        return LIBKERNEL_MODULE_HANDLE;
    }

    // A `/system` module is the firmware's own copy; the platform does not load a second and
    // answers the not-found errno instead - measured on hardware, which returns `0x8002_0002`
    // for both `/system` paths in the probe (110-modules/load).
    if path.starts_with("/system/") {
        return u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw());
    }

    // A title's own module, under `/app0`, loads and is handed a fresh handle. The value is
    // opaque and need not match the console's (its handles reflect however many modules its
    // loader had already placed); what matters is that each load gets a distinct non-negative
    // handle, which is what a guest keys its later calls on.
    if path.starts_with("/app0/") {
        let handle = NEXT_MODULE_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        write_int(args[5], 0);
        return handle;
    }

    // Anything else is not a module location this kernel recognises. Returning a handle for it
    // would be the honest-failure mistake exactly: `060-module/load-rejects-missing` loads a
    // bogus path and a stub that answered success reported a nonexistent module as loaded. A
    // path that is neither libkernel, a `/system` module, nor a `/app0` module is refused.
    u64::from(GuestError::vendor(orbistoun_core::errno::NO_ENTRY).as_raw())
}

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
    let address = orbistoun_thunk::name_thunk(&name);

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
    ("sysctlbyname", sysctlbyname),
    ("sceKernelDlsym", dlsym),
    ("pthread_detach", pthread_detach),
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
    ("scePthreadBarrierInit", pthread_barrier_init),
    ("scePthreadBarrierWait", pthread_barrier_wait),
    ("scePthreadBarrierDestroy", pthread_barrier_destroy),
    ("sceKernelCreateEventFlag", kernel_create_event_flag),
    ("sceKernelPollEventFlag", kernel_poll_event_flag),
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
    ("posix_sigemptyset", sigemptyset),
    ("posix_sigfillset", sigfillset),
    ("posix_sigaddset", sigaddset),
    ("posix_sigdelset", sigdelset),
    ("posix_sigismember", sigismember),
];

#[cfg(test)]
mod tests {

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

    /// `sysctl_value` answers the knobs orbistoun can source and refuses the rest (D447).
    #[test]
    fn sysctl_answers_ostype_and_the_configured_release_and_refuses_the_rest() {
        // ostype is the FreeBSD fact, NUL-terminated; osrelease is the configured release, also
        // NUL-terminated, so its reported length matches the console's (13 chars + NUL = 14 for
        // "0.0-prototype"). An unset release is an empty knob, not an invented one. Anything else
        // is refused rather than answered plausibly.
        assert_eq!(
            super::sysctl_value("kern.ostype", "anything"),
            Some(b"FreeBSD\0".to_vec())
        );
        assert_eq!(
            super::sysctl_value("kern.osrelease", "0.0-prototype"),
            Some(b"0.0-prototype\0".to_vec())
        );
        assert_eq!(
            super::sysctl_value("kern.osrelease", ""),
            Some(vec![0]),
            "an unset release is an empty NUL-terminated string, not a refusal"
        );
        assert_eq!(super::sysctl_value("kern.version", ""), None);
        assert_eq!(super::sysctl_value("hw.ncpu", ""), None);
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
