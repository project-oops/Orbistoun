//! `libSceAgcDriver` - the submission side of the current generation's graphics API.
//!
//! Separate from [`super::agc`] because the platform separates them: a guest builds command
//! buffers with `libSceAgc` and hands them over with `libSceAgcDriver`, and the two are
//! distinct libraries in an import table. Declaring them as one would make every trace
//! entry name the wrong library.
//!
//! Names, provenance and the arity caveat are as [`super::agc`] states them - read out of
//! real import tables, with arities deliberately unestablished.

use crate::pipeline::{GuestMemory, Pipeline, Queue, SubmissionReport};
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_translate::{Fidelity, Strategy, Width};
use std::sync::{Mutex, OnceLock};

guest_module! {
    "libSceAgcDriver" {
        "sceAgcDriverAddEqEvent" => 6,
        "sceAgcDriverCreateQueue" => 3,
        "sceAgcDriverGetDefaultOwner" => 6,
        "sceAgcDriverGetResourceRegistrationMaxNameLength" => 6,
        "sceAgcDriverInitResourceRegistration" => 6,
        "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements" => 6,
        "sceAgcDriverRegisterDefaultOwner" => 6,
        "sceAgcDriverRegisterOwner" => 6,
        "sceAgcDriverRegisterResource" => 6,
        "sceAgcDriverSetHsOffchipParam" => 6,
        "sceAgcDriverSetTFRing" => 6,
        "sceAgcDriverSubmitAcb" => 6,
        "sceAgcDriverSubmitDcb" => 6,
    }
}

/// `sceAgcDriverCreateQueue(type, out_queue, flags)`.
///
/// Accepts the queue type and returns `0` - the measured success code for both the compute queue
/// (`type` 3) and the graphics Universal Graphics Queue (`type` 0): obSCEne's
/// `166-agc/driver-create-queue` and `166-agc/primitive-draw` both record `rc-create 0x0` (sweeps
/// `20260911-*` and `20260912-003916`, the latter answering 9a41).
///
/// **What it deliberately does not do:** write the queue object into `*out_queue`. obSCEne measured
/// the object's header (`38 00 00 00 03 00 00 00 00 00 02 00 ...`) but not *where* the call places
/// it relative to the arguments - the `3c5e`-style pointer-distance measurement create-shader has,
/// this call does not yet. Fabricating a pointer into `*out_queue` is the plausible-output failure
/// principle 3 forbids, so the return is honest and the out-parameter waits on that measurement. No
/// guest reaches this call today (the corpus stalls earlier, at `sceAgcDriverRegisterOwner`), so the
/// accept-and-return is exercised by tests and by obSCEne's rc check, not by a guest dereference yet.
fn create_queue(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `SCE_AGC_ERROR_RESOURCE_REGISTRATION_NOT_SUPPORTED`.
///
/// **The whole resource-registration subsystem is a stub on retail.** obSCEne disassembled
/// `libSceAgcDriver.sprx` (REQ-...0925Z-7b3c) and found `sceAgcDriverRegisterOwner`,
/// `RegisterResource`, `InitResourceRegistration` are each `mov $0x8a6c9018, %eax; ret` - pure
/// stubs - and confirmed it on hardware: they return `0x8a6c9018` and mutate zero bytes of their
/// caller buffers. So this is the measured value, and orbistoun returns it for fidelity (principle 1): the
/// guest gets exactly the "not supported" the console gives it.
///
/// It is **not** what gates PPSA28061's startup abort, and returning it does not clear that abort.
/// With this in effect alongside `sceAgcCreateShader -> 0x0` and
/// `sceKernelMapperGetParam -> 0x80020006`, PPSA28061 still aborts at the same point (worklog 515).
/// The abort is gated on the *mapper's* return, not this constant (D643); that the console ships with
/// these measured codes and the guest here does not survive them is an open divergence, not a reason
/// to invent a success.
const RESOURCE_REGISTRATION_NOT_SUPPORTED: u64 = 0x8a6c_9018;

/// `sceAgcDriverRegisterOwner(owner_buf)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_owner(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverRegisterResource(res, ...)` - stub, returns `0x8a6c9018`, writes nothing.
fn register_resource(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverInitResourceRegistration(...)` - stub, returns `0x8a6c9018`.
fn init_resource_registration(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// `sceAgcDriverQueryResourceRegistrationUserMemoryRequirements(...)` - stub, returns `0x8a6c9018`.
///
/// Hardware left the caller's size sentinel at `0`; being a "not supported" stub it does not write a
/// meaningful requirement, and a guest that gets the error does not read the size, so nothing is
/// written back rather than an invented figure.
fn query_resource_registration_user_memory_requirements(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    RESOURCE_REGISTRATION_NOT_SUPPORTED
}

/// The measured `rc-submit`: `0x0` on every `166-agc/driver-submit-*` check obSCEne ran (sweep
/// `20260916-223136`), and on the `primitive-draw` submission itself.
const SUBMIT_OK: u64 = 0x0;

/// A ceiling on how many dwords a submit will read out of guest memory, so a wild `size` field in the
/// descriptor cannot walk the read off into unmapped memory. obSCEne's own draw buffers are ~2 KB;
/// this is generous (16 MiB) and exists only to refuse an absurd descriptor, not to model a real
/// buffer limit.
const MAX_DCB_DWORDS: u32 = 1 << 22;

/// The guest's readable memory regions (`start`, `end` half-open), set by the worker before a run.
fn guest_regions() -> &'static Mutex<Vec<(u64, u64)>> {
    static REGIONS: OnceLock<Mutex<Vec<(u64, u64)>>> = OnceLock::new();
    REGIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Record the regions a submit may read from - the allocated regions of the guest's memory map.
///
/// The worker calls this before entering the guest, from the same map it records as `memory_map` in
/// the run conditions. A submit then serves the pipeline from these: a shader address inside one is
/// read (and counted resolved), one outside reads `None` (counted unresolved) - the count that
/// settles D101's first route - and neither faults the host. Set empty (the default) nothing resolves.
pub fn set_guest_regions(regions: Vec<(u64, u64)>) {
    if let Ok(mut slot) = guest_regions().lock() {
        *slot = regions;
    }
}

/// Guest memory served from the regions the guest was given.
///
/// A read whose whole range lies inside a region is answered from host memory (identity mapping,
/// D014); one outside every region is `None`, so a shader **GPU** address that does not resolve is
/// reported rather than dereferenced (the open half of D101). This is the whole of what a submit
/// needs, and the only reads it makes are ones a region vouches for.
struct MappedRegions {
    regions: Vec<(u64, u64)>,
}

impl MappedRegions {
    /// The regions in force for this submit, copied so the lock is not held across a walk.
    fn current() -> Self {
        Self {
            regions: guest_regions()
                .lock()
                .map(|r| r.clone())
                .unwrap_or_default(),
        }
    }

    /// Whether `[address, address + length)` lies wholly inside one region.
    fn contains(&self, address: u64, length: usize) -> bool {
        let Some(end) = address.checked_add(length as u64) else {
            return false;
        };
        self.regions
            .iter()
            .any(|&(start, region_end)| address >= start && end <= region_end)
    }
}

impl GuestMemory for MappedRegions {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        if length == 0 || !self.contains(address, length) {
            return None;
        }
        let ptr = std::ptr::with_exposed_provenance::<u8>(usize::try_from(address).ok()?);
        // SAFETY: `[address, address + length)` lies wholly inside a region the guest was given
        // (checked above); under the identity mapping (D014) it is `length` bytes of readable guest
        // memory, which outlives this borrow because the guest's mappings live for the process.
        Some(unsafe { std::slice::from_raw_parts(ptr, length) })
    }
}

/// The last submission a guest handed to `sceAgcDriverSubmitDcb`, for the run report to read.
fn last_submission() -> &'static Mutex<Option<SubmissionReport>> {
    static LAST: OnceLock<Mutex<Option<SubmissionReport>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

/// The report of the most recent DCB a guest submitted, or `None` if none has.
///
/// Read by the run report (`orbistoun-worker`): a submission is the first real graphics measurement a
/// title produces, and it belongs beside the reach and import counts rather than only in a trace.
#[must_use]
pub fn last_submission_report() -> Option<SubmissionReport> {
    last_submission().lock().ok().and_then(|slot| slot.clone())
}

/// Reads the 16-byte submit descriptor (`{gpu_addr: u64, size_dwords: u32, flags: u8, pad}`, obSCEne
/// `-1c97`, sweep `20260917-124503`) into an address and a byte length, refusing a null or absurd one.
fn submit_descriptor(descriptor: u64) -> Option<(u64, usize)> {
    if descriptor == 0 {
        return None;
    }
    // SAFETY: `descriptor` is the guest's own submit descriptor - a CPU-side 16-byte struct it fills
    // and passes by pointer; under the identity mapping (D014) the guest address is a host pointer.
    // Sixteen bytes are read, the width obSCEne measured the driver reads (1c97).
    let desc = unsafe {
        std::slice::from_raw_parts(
            std::ptr::with_exposed_provenance::<u8>(usize::try_from(descriptor).ok()?),
            16,
        )
    };
    let gpu_addr = u64::from_le_bytes(desc[0..8].try_into().ok()?);
    let size_dwords = u32::from_le_bytes(desc[8..12].try_into().ok()?);
    if gpu_addr == 0 || size_dwords == 0 || size_dwords > MAX_DCB_DWORDS {
        return None;
    }
    Some((gpu_addr, size_dwords as usize * 4))
}

/// `sceAgcDriverSubmitDcb(dcb)` - **the handover.** A guest builds a command buffer with `libSceAgc`
/// and hands it over here; this reads the descriptor it passed, walks the command buffer through the
/// translator, and records a `SubmissionReport`.
///
/// It is the only point at which a real guest command stream enters `walk` and `pipeline` - until now
/// both were reached only from tests and `orbistoun-cli`. No backend is attached, so this **reports
/// rather than renders**: packets, register writes, draws and shader candidates, the measurement that
/// says where translation effort goes (3861). It never fails on the guest's account - an empty or
/// out-of-bounds descriptor records nothing and still returns success, the way the console's does
/// (`rc-submit 0x0` throughout obSCEne's driver-submit checks). The command buffer and the shader
/// addresses the stream names are both served from the regions the guest was given, so a descriptor
/// pointing outside every region is refused rather than dereferenced, and a shader address that does
/// not fall in a region is counted unresolved (D101's first route) rather than faulted (5bff).
fn submit_dcb(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Some((gpu_addr, length)) = submit_descriptor(args[0]) else {
        return SUBMIT_OK;
    };
    let memory = MappedRegions::current();
    // Refuse a descriptor whose command buffer lies outside every region the guest was given, rather
    // than dereferencing an address that would fault the host. Recorded as an empty report so the run
    // report shows the submit happened and read nothing (5bff).
    let Some(bytes) = memory.read(gpu_addr, length).map(<[u8]>::to_vec) else {
        if let Ok(mut slot) = last_submission().lock() {
            *slot = Some(SubmissionReport::default());
        }
        return SUBMIT_OK;
    };
    let report = Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Lane,
        width: Width::default(),
    })
    .map(|mut pipeline| pipeline.submit(&bytes, Queue::Draw, &[], &memory).report)
    .unwrap_or_default();
    if let Ok(mut slot) = last_submission().lock() {
        *slot = Some(report);
    }
    SUBMIT_OK
}

/// Implementations this crate provides for `libSceAgcDriver`.
///
/// `sceAgcDriverCreateQueue` accepts the queue (9a41); the resource-registration family are the
/// retail stubs 7b3c disassembled, each returning the measured `0x8a6c9018`. This is fidelity, not an
/// unblock: PPSA28061 still aborts at its startup wall with these in effect (the abort is gated on the
/// mapper, D643/worklog 515).
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAgcDriverCreateQueue", create_queue),
        ("sceAgcDriverRegisterOwner", register_owner),
        ("sceAgcDriverRegisterResource", register_resource),
        (
            "sceAgcDriverInitResourceRegistration",
            init_resource_registration,
        ),
        (
            "sceAgcDriverQueryResourceRegistrationUserMemoryRequirements",
            query_resource_registration_user_memory_requirements,
        ),
        ("sceAgcDriverSubmitDcb", submit_dcb),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        GUEST_ARG_REGISTERS, SUBMIT_OK, last_submission_report, set_guest_regions, submit_dcb,
    };
    use std::sync::{Mutex, PoisonError};

    /// These tests share the process-global region and last-submission stores, so they run one at a
    /// time. A poisoned lock is recovered rather than cascading a panic across the others.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The `(start, end)` region a heap buffer occupies, its address exposed for the handler's read.
    fn region_of(buffer: &[u8]) -> (u64, u64) {
        let base = buffer.as_ptr() as usize as u64;
        (base, base + buffer.len() as u64)
    }

    /// Two `SET_CONTEXT_REG` packets (`0xc0016900`, offset, value), as the guest would have built.
    fn command_buffer() -> Vec<u8> {
        [
            0xc001_6900u32,
            0x0000_0318,
            0x4001_4000,
            0xc001_6900,
            0x0000_03b0,
            0x000f_c03f,
        ]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
    }

    /// A minimal shader that translates: `v_mov_b32 v0, 9 ; s_endpgm`.
    fn shader_bytes() -> Vec<u8> {
        [0x7E00_0000u32 | (1 << 9) | (128 + 9), 0xBF81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    /// A heap buffer holding the shader at a **256-byte-aligned** address (a shader-address register
    /// stores the address in 256-byte units, so its low bits must be zero to reconstruct), and that
    /// address. The whole buffer is the region to register.
    fn aligned_shader() -> (Vec<u8>, u64) {
        let shader = shader_bytes();
        let mut buffer = vec![0u8; shader.len() + 512];
        let base = buffer.as_ptr() as usize;
        let aligned = (base + 255) & !255usize;
        let offset = aligned - base;
        buffer[offset..offset + shader.len()].copy_from_slice(&shader);
        (buffer, aligned as u64)
    }

    /// A command stream that writes a graphics shader's address registers to `address`, as a guest
    /// would - the register numbers coming from the crate's own vocabulary, not invented here.
    fn command_naming_a_shader(address: u64) -> Vec<u8> {
        let vocabulary = crate::registers::Vocabulary::builtin().expect("vocabulary");
        // The first non-compute stage's address register pair - a submit uses the graphics queue.
        let mut stage_wanted: Option<String> = None;
        let (mut low, mut high) = (None, None);
        for (register, (stage, is_high)) in vocabulary.shader_registers() {
            if stage == "compute" {
                continue;
            }
            let stage = stage.clone();
            if stage_wanted.get_or_insert(stage.clone()) != &stage {
                continue;
            }
            if *is_high {
                high = Some(*register);
            } else {
                low = Some(*register);
            }
        }
        let (low, high) = (
            low.expect("a graphics shader address register"),
            high.expect("a graphics shader address register"),
        );
        let (opcode, base) = vocabulary
            .opcode_for_register(low)
            .expect("an opcode reaching the shader address registers");
        let mut words: Vec<u32> = Vec::new();
        for (register, value) in [
            (
                low,
                u32::try_from((address >> 8) & 0xFFFF_FFFF).expect("low half"),
            ),
            (high, u32::try_from(address >> 40).expect("high half")),
        ] {
            words.push((3 << 30) | ((2 - 1) << 16) | (u32::from(opcode) << 8));
            words.push(register - base);
            words.push(value);
        }
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// A 16-byte submit descriptor pointing at `buffer` (the layout obSCEne `-1c97` measured). The
    /// pointer is cast through `usize`, which exposes its provenance for the handler's read.
    fn descriptor(buffer: &[u8]) -> [u8; 16] {
        let mut d = [0u8; 16];
        d[0..8].copy_from_slice(&(buffer.as_ptr() as usize as u64).to_le_bytes());
        d[8..12].copy_from_slice(&u32::try_from(buffer.len() / 4).unwrap().to_le_bytes());
        d
    }

    fn args(descriptor_ptr: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut a = [0u64; GUEST_ARG_REGISTERS];
        a[0] = descriptor_ptr;
        a
    }

    /// **A submitted command buffer inside a region is walked into a report.**
    ///
    /// The handover, exercised: the descriptor's buffer (now served from the region it lies in) walks
    /// into a report whose packet count is what the walk recognised. A null descriptor returns
    /// success without faulting - the property `pipeline`'s walker already guarantees, carried up.
    #[test]
    fn a_submitted_command_buffer_in_a_region_is_reported() {
        let _guard = serial();
        let buffer = command_buffer();
        set_guest_regions(vec![region_of(&buffer)]);
        let desc = descriptor(&buffer);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let report = last_submission_report().expect("a report was recorded");
        assert_eq!(report.packets, 2, "two SET_CONTEXT_REG packets: {report:?}");
        assert!(
            report.register_writes >= 1,
            "registers extracted: {report:?}"
        );

        // A null descriptor: success, no fault, nothing to read.
        assert_eq!(submit_dcb(&args(0)), SUBMIT_OK);
    }

    /// **A shader named at an address inside a region resolves; one outside every region does not.**
    ///
    /// D101's first route, both ways (5bff). When the shader's region is registered its address falls
    /// in a region, so it is counted resolved and - the bytes being a real shader - translated, which
    /// is what pushes a `BindShader`. When it is not registered the same address falls in no region,
    /// so it is counted unresolved and nothing is prepared. The command buffer's own region is
    /// registered in both, so the difference is the shader's alone.
    #[test]
    fn a_shader_address_resolves_in_a_region_and_not_outside_one() {
        let _guard = serial();
        let (shader, shader_addr) = aligned_shader();
        let stream = command_naming_a_shader(shader_addr);

        // Resolved: both the command buffer and the shader are in registered regions.
        set_guest_regions(vec![region_of(&stream), region_of(&shader)]);
        let desc = descriptor(&stream);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let resolved = last_submission_report().expect("a report");
        assert!(
            resolved.shaders_found >= 1,
            "an address was named: {resolved:?}"
        );
        assert_eq!(resolved.addresses_resolved, 1, "it resolved: {resolved:?}");
        assert_eq!(resolved.addresses_unresolved, 0, "{resolved:?}");
        assert!(
            resolved.shaders_translated >= 1,
            "the resolved shader translated, yielding a BindShader: {resolved:?}"
        );

        // Unresolved: only the command buffer's region is registered, so the shader is out of bounds.
        set_guest_regions(vec![region_of(&stream)]);
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let unresolved = last_submission_report().expect("a report");
        assert!(unresolved.shaders_found >= 1, "still named: {unresolved:?}");
        assert_eq!(unresolved.addresses_resolved, 0, "{unresolved:?}");
        assert_eq!(
            unresolved.addresses_unresolved, 1,
            "unresolved: {unresolved:?}"
        );
        assert_eq!(unresolved.shaders_translated, 0, "{unresolved:?}");
    }

    /// **A descriptor whose command buffer is outside every region is refused, not dereferenced.**
    ///
    /// The `gpu_addr` is a wild address in no region; the submit returns success and records an empty
    /// report without reading it, rather than faulting the host on a raw dereference (5bff).
    #[test]
    fn a_descriptor_outside_every_region_returns_ok_without_reading() {
        let _guard = serial();
        set_guest_regions(vec![(0x1000, 0x2000)]);
        let mut desc = [0u8; 16];
        desc[0..8].copy_from_slice(&0xdead_0000_u64.to_le_bytes()); // gpu_addr in no region
        desc[8..12].copy_from_slice(&4u32.to_le_bytes()); // 16 bytes
        assert_eq!(submit_dcb(&args(desc.as_ptr() as usize as u64)), SUBMIT_OK);
        let report = last_submission_report().expect("an empty report was recorded");
        assert_eq!(report.packets, 0, "nothing was read: {report:?}");
    }
}
