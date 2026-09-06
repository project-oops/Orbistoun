//! Orbistoun checked against what a console was measured to do.
//!
//! # What this file is for
//!
//! `crates/orbistoun-hle/data/hardware.toml` is generated from conformance captures and holds
//! every measurement the runs took. This is where those become claims about *this* emulator:
//! a test names a measurement by id and asserts orbistoun answers the same thing.
//!
//! **The gate is coverage, not the assertions.** Every constant measurement must be either
//! claimed by a test here or listed in [`OUTSTANDING`] with the reason - so
//! [`OUTSTANDING`] is a work queue with a completion condition per item, which is the thing a
//! function-shaped task list cannot give. A measurement that is neither fails the gate, and a
//! new capture therefore arrives as work rather than as silence.
//!
//! # Why divergences are listed rather than left failing
//!
//! A test that is red on purpose forever is not a queue, it is a broken build that people
//! learn to ignore. Where orbistoun is known to disagree with the console, the measured value
//! and the disagreement are written into [`OUTSTANDING`]. Fixing one moves it up into a test,
//! and the gate notices immediately if somebody deletes the test instead.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::hardware::{Measurement, Measurements};

/// Measurements this file asserts orbistoun against.
const CLAIMED: &[&str] = &[
    // Every firmware path the encoder probe tried, refused with the same code (D497).
    "106-encoder/path-probe:/system/common/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system/priv/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system/sys/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_ex/common/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_ex/priv/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_ex/sys/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_ex/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_data/priv/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_data/sys/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system_data/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/RuC3TlgXmY/common/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/RuC3TlgXmY/priv/lib/libSceVencCore.sprx:handle",
    "106-encoder/path-probe:/system/common/lib/libSceVideoRecording.sprx:handle",
    "106-encoder/path-probe:/system/priv/lib/libSceVideoRecording.sprx:handle",
    "106-encoder/path-probe:/system_ex/common/lib/libSceVideoRecording.sprx:handle",
    "106-encoder/path-probe:/system_ex/priv/lib/libSceVideoRecording.sprx:handle",
    "106-encoder/path-probe:/RuC3TlgXmY/common/lib/libSceVideoRecording.sprx:handle",
    "106-encoder/path-probe:/system/common/lib/libSceAvcEnc.sprx:handle",
    "106-encoder/path-probe:/system/priv/lib/libSceAvcEnc.sprx:handle",
    "106-encoder/path-probe:/system/common/lib/libSceHevcEnc.sprx:handle",
    "106-encoder/path-probe:/system/priv/lib/libSceHevcEnc.sprx:handle",
    "106-encoder/path-probe:/system/common/lib/libSceVideodec.sprx:handle",
    "106-encoder/path-probe:/system/priv/lib/libSceVideodec.sprx:handle",
    // The float environment a title is entered under, asserted against what orbistoun
    // installs rather than against a constant written down twice (D486).
    "035-libc/fpu-environment:mxcsr:daz_denormals_are_zero",
    "035-libc/fpu-environment:mxcsr:ftz_flush_to_zero",
    "035-libc/fpu-environment:mxcsr:rounding_mode",
    "035-libc/fpu-environment:mxcsr:exception_masks",
    // The raw value too: it is the four fields above plus the sticky precision flag, and
    // the split between configuration and status is published rather than guessed.
    "035-libc/fpu-environment:mxcsr:raw",
    // One attribute object, set then get, for each of 0..4 - the run the queue asked for.
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:default-type",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-0-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-1-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-2-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-3-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-4-read-back",
    // The console writes eight bytes where it was given four; D210 said four, from
    // documentation, and a guard word measured otherwise.
    "018-relational/handle-fits-its-out-parameter:sceKernelCreateSema:guard-after-handle",
    // The character-classification tables, confirmed by a second capture from a different
    // build form. Every entry read the way a guest reads it: through the function (D468).
    "035-libc/getpctype:_Getpctype:mask_eof_neg1",
    "035-libc/getpctype:_Getpctype:mask_nul_0",
    "035-libc/getpctype:_Getpctype:mask_tab_9",
    "035-libc/getpctype:_Getpctype:mask_space_32",
    "035-libc/getpctype:_Getpctype:mask_0_48",
    "035-libc/getpctype:_Getpctype:mask_9_57",
    "035-libc/getpctype:_Getpctype:mask_A_65",
    "035-libc/getpctype:_Getpctype:mask_F_70",
    "035-libc/getpctype:_Getpctype:mask_Z_90",
    "035-libc/getpctype:_Getpctype:mask_a_97",
    "035-libc/getpctype:_Getpctype:mask_f_102",
    "035-libc/getpctype:_Getpctype:mask_z_122",
    "035-libc/getpctype:_Getptolower:entry_eof_neg1",
    "035-libc/getpctype:_Getptolower:entry_A_65",
    "035-libc/getpctype:_Getptolower:entry_Z_90",
    "035-libc/getpctype:_Getptolower:entry_a_97",
    "035-libc/getpctype:_Getptolower:entry_0_48",
    "035-libc/getpctype:_Getptoupper:entry_eof_neg1",
    "035-libc/getpctype:_Getptoupper:entry_a_97",
    "035-libc/getpctype:_Getptoupper:entry_z_122",
    "035-libc/getpctype:_Getptoupper:entry_A_65",
    "035-libc/getpctype:_Getptoupper:entry_0_48",
    "015-sync/mutex-unlock-unheld:scePthreadMutexUnlock:unheld-unlock",
    "015-sync/mutex-recursion:scePthreadMutexTrylock:type-1-second-acquisition",
    "015-sync/mutex-recursion:scePthreadMutexTrylock:type-2-second-acquisition",
    "015-sync/mutex-recursion:scePthreadMutexTrylock:type-3-second-acquisition",
    "015-sync/mutex-recursion:scePthreadMutexTrylock:type-4-second-acquisition",
    "130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-0",
    "130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-1",
    "130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-2",
    "130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-4",
    "110-modules/load:sceKernelLoadStartModule:libkernel",
    "110-modules/load:sceKernelLoadStartModule:system-libc",
    "110-modules/load:sceKernelLoadStartModule:system-fios2",
    "135-sysctl/names:kern.ostype:length",
    "135-sysctl/names:hw.ncpu:length",
    "135-sysctl/names:hw.pagesize:length",
    "135-sysctl/names:machdep.tsc_freq:length",
    "135-sysctl/names:kern.hostname:length",
    // The third query field is the memory type - the question D398 left open, settled by a
    // capture that allocated one page of each type and read the field back.
    "130-layout/memory-type:wb-onion:third-field",
    "130-layout/memory-type:wc-garlic:third-field",
    "130-layout/memory-type:wb-garlic:third-field",
    // The two encoder system modules the console loads; the seven it refuses stay in the
    // queue, because that refusal may belong to the capture's application category.
    "106-encoder/sysmodule-load:VENC:rc",
    "106-encoder/sysmodule-load:VIDEOREC:rc",
    // The other four paths obSCEne loaded, recovered from its own quantity table.
    "110-modules/load:sceKernelLoadStartModule:system-libc-internal",
    "110-modules/load:sceKernelLoadStartModule:system-sysmodule",
    "110-modules/load:sceKernelLoadStartModule:libkernel-prx",
    "110-modules/load:sceKernelLoadStartModule:libkernel-sprx",
];

/// Measurements whose value is **not a property of the interface**, so nothing can assert it.
///
/// # Why this is a separate list from the work queue
///
/// [`OUTSTANDING`] means "not done yet", and every entry in it should one day move up into a
/// test. These never will, and keeping them there would leave the queue with permanent
/// residents - at which point it stops being read as a queue.
///
/// A module handle is the case that forced it. The console answered `0x15` and `0x14` for two
/// `/app0` modules, and both runs agree, but the number reflects how many modules *that*
/// loader had already placed. It is a fact about that machine at that moment, not about
/// `sceKernelLoadStartModule`. Asserting it would pin orbistoun to somebody else's bookkeeping.
///
/// **The record is still worth having** - it says the call answers a small non-negative handle
/// for a title's own module, which is the shape a guest keys on, and that much *is* asserted
/// where the shape can be checked.
const OPAQUE: &[(&str, &str)] = &[
    (
        "106-encoder/sysmodule-load:VENC:id",
        "the probe passes `venc_modules[i].id` and reports it back unchanged, so this records the identifier obSCEne chose and nothing the console decided - D497's family. The identifier is worth having beside the return code; it is not a claim about orbistoun",
    ),
    (
        "106-encoder/sysmodule-load:VIDEOREC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:AVC_DEC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:AVC_ENC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:HEVC_DEC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:HEVC_ENC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:VIDEODEC:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:CAMERA:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/sysmodule-load:SCREEN_SHOT:id",
        "as VENC above: the probe's own constant, reported back",
    ),
    (
        "106-encoder/module-handle:loaded:handle",
        "a handle from the console's own numbering - **the case the list above cites as forcing this table to exist**, and it had been left in the queue anyway. It reflects how many modules that loader had already placed, so asserting it pins orbistoun to somebody else's bookkeeping",
    ),
    (
        "106-encoder/module-list:total:count",
        "how many modules that process had loaded, which is a fact about that machine at that moment rather than about any call. orbistoun loads one, and matching the number would mean loading modules it has no reason to load",
    ),
    (
        "120-measure/identify-clocks:sceKernelUsleep:requested",
        "**the check's own input, read back.** The record is the sleep the probe asked for, not anything the console chose, so a platform that ignores the request entirely produces the same reading - D497's family, one layer up from an out-parameter",
    ),
    (
        "135-sysctl/osrelease:kern.osrelease:length",
        "the length of the measured `0.0-prototype`, which is a per-machine setting and empty by default. Orbistoun matching it would be matching that console's configuration, not the platform",
    ),
    (
        "135-sysctl/names:kern.osrelease:length",
        "the same per-machine value as the entry above, taken by a second check. It was in the work queue, where it could never be completed: matching `0.0-prototype` means matching one console's configuration, which is the definition of opaque rather than a thing to do. Its reason said `as above` and pointed at the *other* neighbour - a knob orbistoun refuses (D397) - which is how a permanent resident of the queue read as work for as long as it did (D546)",
    ),
    (
        "035-libc/getpctype:_Getpctype:pointer",
        "the console's own load address for its C library; orbistoun's table is wherever the guest allocator put it, and a guest reaches it by calling the function rather than by address",
    ),
    (
        "035-libc/getpctype:_Getptolower:pointer",
        "the console's own load address for its C library; orbistoun's table is wherever the guest allocator put it, and a guest reaches it by calling the function rather than by address",
    ),
    (
        "035-libc/getpctype:_Getptoupper:pointer",
        "the console's own load address for its C library; orbistoun's table is wherever the guest allocator put it, and a guest reaches it by calling the function rather than by address",
    ),
    (
        "120-measure/cpuid:cpuid:max_basic_leaf",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:signature_eax",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:feature_ecx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:feature_edx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:stepping",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:model",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:family",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid_ext:feature_ecx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid_ext:feature_edx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:rdtscp:tsc_aux_raw",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:rdtscp:core_id",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:rdtscp:numa_node_id",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
];

/// Constant measurements nothing asserts yet, and why. **This is the work queue.**
///
/// Each entry is one unit of work with an unambiguous completion condition: make orbistoun
/// answer what the console answered, then move the id into [`CLAIMED`] with a test.
const OUTSTANDING: &[(&str, &str)] = &[
    (
        "106-encoder/sysmodule-load:AVC_DEC:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:AVC_ENC:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:HEVC_DEC:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:HEVC_ENC:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:VIDEODEC:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:CAMERA:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/sysmodule-load:SCREEN_SHOT:rc",
        "the console answers `0x805a1000` for this module and `0` for VENC and VIDEOREC, which orbistoun claims. **Orbistoun's shim does not refuse anything** - it answers `0` to every identifier, on the reasoning that every library a title imports is already resolved before the guest runs (D125). Matching the refusal would mean refusing seven identifiers because one capture, taken at one application category, was refused them - and obSCEne's D301 records that category deciding an unrelated call. Needs a capture at a different category to say whether this is a property of the module or of the asker",
    ),
    (
        "106-encoder/symbols:sceVencCoreCreateEncoder:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreDeleteEncoder:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreGetAuData:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreGetPicParams:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreMapTargetMemory:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreMapTargetMemoryByPid:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreQueryHeader:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreQueryMemorySize:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreQueryMemorySizeEx:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreQueryPreset:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreQueryPresetEx:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetBitRate:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetInputFrame:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetInputFrameByPid:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetInvalidFrame:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetPasteImage:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetPicParams:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetPictureType:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSetPrivacyGuard:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreStartSequence:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreStopSequence:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreSyncEncode:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreUnmapTargetMemory:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/symbols:sceVencCoreUnmapTargetMemoryByPid:unresolved",
        "**the symbol did not resolve on the console.** The probe emits this record in the branch where its lookup returned null - kernel export table, every loaded module handle, then 0x2001 - and the `0` beside it is filler for a status field, not an address. There is no `vaddr` or `handle` measurement anywhere in this group, and obSCEne's own `related-libs` check separately records the library as *absent* in that process. So this records a non-resolution under one capture's application category, the same shape as the sysmodule refusals, and matching it would pin orbistoun to that process's loaded set. The category-0 capture in obSCEne's backlog 022 is what would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingOpen:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingClose:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingGetStatus:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingQueryMemorySize:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingSetStatus:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingStart:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingStop:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingGetAuData:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/rec-symbols:sceVideoRecordingSetInputFrame:unresolved",
        "**the symbol did not resolve on the console**, as the `symbols` group above records for the encoder library: the probe's lookup returned null and the `0` beside it is filler for a status field, not an address. Matching it would pin orbistoun to one process's loaded set. Completion: the category-0 capture in obSCEne's backlog 022, which would say whether the library is absent to every title or only to that one (D506)",
    ),
    (
        "106-encoder/related-libs:libSceVideoRecording:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "106-encoder/related-libs:libSceMediaFrameworkInterface:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "106-encoder/related-libs:libSceVideoCoreServerInterface:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "106-encoder/related-libs:libSceAvcEnc:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "106-encoder/related-libs:libSceHevcEnc:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "106-encoder/related-libs:libSceVideodec:absent",
        "the console reports this encoder library *absent* - a fact about which tier a title may reach, not about a call. Completion: model the tiers the manifest records",
    ),
    (
        "110-modules/tier-probe:obs_module_open_tier:resolved_tiers",
        "how many privilege tiers a title-level process could open a module from; orbistoun models no tiers",
    ),
    (
        "120-measure/cache-topology:cache:line_size_bytes",
        "0x40 - the console's L1 line size, read through `cpuid`, which answers the host's here (see the cpuid entries in OPAQUE)",
    ),
    (
        "130-layout/memory-type:wb-onion:alloc-refused",
        "**the refusal is not about the memory type.** The capture ran as application category 65536, and the console's resource arbitrator grants such a process zero bytes of direct memory whatever type it asks for - obSCEne's own D301 measured the same call succeeding under category 0 at the same privilege. So this records the run's category, not a property of onion/garlic, and a capture taken as a big app is needed before orbistoun has anything to match",
    ),
    (
        "130-layout/memory-type:wc-garlic:alloc-refused",
        "**the refusal is not about the memory type.** The capture ran as application category 65536, and the console's resource arbitrator grants such a process zero bytes of direct memory whatever type it asks for - obSCEne's own D301 measured the same call succeeding under category 0 at the same privilege. So this records the run's category, not a property of onion/garlic, and a capture taken as a big app is needed before orbistoun has anything to match",
    ),
    (
        "130-layout/memory-type:wb-garlic:alloc-refused",
        "**the refusal is not about the memory type.** The capture ran as application category 65536, and the console's resource arbitrator grants such a process zero bytes of direct memory whatever type it asks for - obSCEne's own D301 measured the same call succeeding under category 0 at the same privilege. So this records the run's category, not a property of onion/garlic, and a capture taken as a big app is needed before orbistoun has anything to match",
    ),
    (
        "135-sysctl/names:kern.osrevision:length",
        "0x4 bytes - a four-byte revision number orbistoun does not source",
    ),
    (
        "135-sysctl/names:kern.sdk_version:length",
        "0x4 bytes - the SDK the title was built against; orbistoun has no value for it and refuses rather than inventing one (D397)",
    ),
    (
        "135-sysctl/names:hw.model:length",
        "the console's CPU model string; orbistoun refuses the knob today",
    ),
    (
        "135-sysctl/names:hw.machine:length",
        "the architecture string; `amd64` is the obvious answer and is unconfirmed, so it is not written",
    ),
    (
        "135-sysctl/names:hw.availpages:length",
        "a page count that depends on the console's memory split, which orbistoun does not model",
    ),
    (
        "136-kernel/handoff:payload_args:null",
        "whether the payload-argument pointer is null on a native title entry; orbistoun does not present a payload-argument block",
    ),
    // --- the widened codes, and a claim that was withdrawn (D480) ------------------------
    //
    // These were listed as divergences on the reading that the console answers
    // `0xffffffff8002_xxxx` sign-extended while orbistoun zero-extends. **That reading was
    // wrong**, and the probe source says so: the check writes
    // `int second = scePthreadMutexTrylock(...)` and reports `(uint64_t)(int64_t)second`, so
    // the leading `ffffffff` is obSCEne widening a C `int`, not the console setting the top
    // half of `rax`. A prototype returning `int` reads `eax` and nothing else, so these
    // records cannot say what the other thirty-two bits held.
    //
    // Read at the width they were actually taken, **every one is a value orbistoun already
    // produces**: `0x80020001` is `errno::NOT_OWNER`, `0x80020010` is `BUSY`, `0x80020016` is
    // `INVALID`, `0x80020002` is `NO_ENTRY`, `0x80020003` is `NO_SUCH`. What keeps them here
    // is the *condition*, not the value - each needs its check's setup reproduced.
    (
        "110-modules/symbol:sceKernelDlsym:memcpy",
        "**orbistoun does not fail here, it succeeds - which is worse.** The probe loads libkernel (handle `0x2001`, agreed by three measurements) and asks it for `memcpy`; the console answers ESRCH, so libkernel does not export it. Orbistoun publishes every implementation in one flat by-name table and `dlsym` looks a name up there **without consulting the module handle at all**, so it answers `0` and writes an address for a symbol the named module does not have - measured in `tests/dlsym_divergence.rs`, which also records why a claim written with an invalid handle would pass while checking nothing (orbistoun already answers this exact code on that branch, D366). Completion: a per-module export list for the platform's own libraries, which nothing lawful here provides - the earlier note that the loader could not resolve anything stopped being true when guest exports were added (D517) and was never the reason",
    ),
    // --- the module loader, which loads nothing ---------------------------------------------
    // --- knobs orbistoun cannot source ------------------------------------------------------
    (
        "135-sysctl/names:kern.version:length",
        "the console answers a 0x2c-byte build banner; orbistoun refuses the knob rather than inventing one (D397)",
    ),
    // --- needs a harness rather than a call --------------------------------------------------
    // --- behaviour not modelled --------------------------------------------------------------
];

/// Calls an implementation by the name a guest would import it under.
///
/// Guest arguments are plain words and guest memory is the host's under an identity mapping,
/// so a test hands over the address of its own local and the implementation writes through it -
/// which is the same thing a guest does.
fn call(name: &str, args: [u64; GUEST_ARG_REGISTERS]) -> u64 {
    let found = orbistoun_service::implementation_named(name)
        .unwrap_or_else(|| panic!("{name} is not implemented, so the claim cannot be checked"));
    found(&args)
}

/// The measurement with this id, which the gate below proves exists.
fn measurement(id: &str) -> Measurement {
    Measurements::builtin()
        .get(id)
        .unwrap_or_else(|| panic!("no measurement called {id}"))
        .clone()
}

/// **Every constant measurement is claimed by a test or declared outstanding.**
///
/// The gate the rest of this file exists to satisfy. A new capture bringing new measurements
/// fails it, which is how a hardware run becomes work rather than a file nobody reads.
#[test]
fn every_constant_measurement_is_claimed_or_declared_outstanding() {
    let table = Measurements::builtin();
    let claimed: std::collections::BTreeSet<&str> = CLAIMED.iter().copied().collect();
    let outstanding: std::collections::BTreeSet<&str> =
        OUTSTANDING.iter().map(|(id, _)| *id).collect();

    let opaque: std::collections::BTreeSet<&str> = OPAQUE.iter().map(|(id, _)| *id).collect();
    let unaccounted: Vec<&str> = table
        .constants()
        .map(|m| m.id.as_str())
        .filter(|id| !claimed.contains(id) && !outstanding.contains(id) && !opaque.contains(id))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "measured on hardware and neither asserted nor declared outstanding: {unaccounted:#?}"
    );

    // **The arithmetic, asserted rather than reported.** The first version of the gate that
    // runs this counted the outstanding list by grepping the Rust source and got 26 for a list
    // of 21, because five of the reasons happen to begin with a digit. A number a gate prints
    // is a claim like any other, and this is the one place that can check it.
    assert_eq!(
        CLAIMED.len() + OUTSTANDING.len() + OPAQUE.len(),
        table.constants().count(),
        "every constant is accounted for exactly once, so the four counts must agree"
    );
}

/// What the console reported, without the check that took the reading: subject, condition,
/// observation. Two checks producing one of these produced one fact.
type Reading = (String, String, String);

/// **One measured fact is not both a thing to do and a thing that cannot be done.**
///
/// # What this asserts
///
/// That where two checks measured the same subject, condition and value, both land in the same
/// list. The three lists are not opinions - [`CLAIMED`] is asserted, [`OUTSTANDING`] is a queue
/// with a completion condition, and [`OPAQUE`] is what can never move up - so splitting one
/// fact across two of them means one of the two is wrong.
///
/// It caught `kern.osrelease`. Two checks measured its length as `0xe`; one sat in [`OPAQUE`]
/// with the right reason - a per-machine setting, and matching it would match one console's
/// configuration - and the other sat in the work queue, where it could never be completed. That
/// is the permanent resident [`OPAQUE`] exists to keep out, and the queue stops being read as a
/// queue once it has one (D546).
///
/// # What it cannot assert
///
/// That any single classification is right - only that duplicates agree. And it can only see
/// duplicates: **one group qualifies today**, which is honest about its reach rather than
/// impressive. Its value is on the next capture, where a re-measured id arrives beside one
/// already filed and the two are decided by different people at different times.
#[test]
fn two_checks_of_one_fact_do_not_disagree_about_which_list_it_is_in() {
    let claimed: std::collections::BTreeSet<&str> = CLAIMED.iter().copied().collect();
    let outstanding: std::collections::BTreeSet<&str> =
        OUTSTANDING.iter().map(|(id, _)| *id).collect();
    let opaque: std::collections::BTreeSet<&str> = OPAQUE.iter().map(|(id, _)| *id).collect();
    let list_of = |id: &str| {
        if claimed.contains(id) {
            "CLAIMED"
        } else if outstanding.contains(id) {
            "OUTSTANDING"
        } else if opaque.contains(id) {
            "OPAQUE"
        } else {
            "unlisted"
        }
    };

    // Keyed on what the console reported, not on the id - the id carries the check that took
    // the reading, and two checks taking one reading is exactly the case being looked for.
    let mut seen: std::collections::BTreeMap<Reading, Vec<(&str, &str)>> =
        std::collections::BTreeMap::new();
    for measurement in Measurements::builtin().constants() {
        let key = (
            measurement.subject.clone(),
            measurement.condition.clone(),
            measurement.observation.clone(),
        );
        let id: &str = Box::leak(measurement.id.clone().into_boxed_str());
        seen.entry(key).or_default().push((list_of(id), id));
    }

    let mut duplicated = 0usize;
    let mut disagreeing = Vec::new();
    for (key, entries) in &seen {
        if entries.len() < 2 {
            continue;
        }
        duplicated += 1;
        let lists: std::collections::BTreeSet<&str> = entries.iter().map(|(l, _)| *l).collect();
        if lists.len() > 1 {
            disagreeing.push(format!("{key:?} -> {entries:?}"));
        }
    }
    assert!(
        disagreeing.is_empty(),
        "one measured fact is filed two ways, so one of the two is wrong: {disagreeing:#?}"
    );
    assert!(
        duplicated > 0,
        "nothing is measured by two checks any more, so this checks nothing; if a capture removed the overlap, say so here rather than leaving a green test that reads as cover"
    );
}

/// Nothing claims or defers a measurement that is not there, or is not constant.
///
/// The other half of the gate, and the half that catches a stale list: an id that no longer
/// exists reads as coverage while asserting nothing, and a varying measurement must never be
/// asserted at all.
#[test]
fn nothing_claims_a_measurement_that_cannot_be_claimed() {
    let table = Measurements::builtin();
    for id in CLAIMED
        .iter()
        .chain(OUTSTANDING.iter().map(|(id, _)| id))
        .chain(OPAQUE.iter().map(|(id, _)| id))
    {
        let found = table
            .get(id)
            .unwrap_or_else(|| panic!("{id} is named here but is in no capture"));
        assert!(
            found.constant,
            "{id} varies between runs, so it is not a property of the platform to claim"
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in CLAIMED
        .iter()
        .chain(OUTSTANDING.iter().map(|(id, _)| id))
        .chain(OPAQUE.iter().map(|(id, _)| id))
    {
        assert!(seen.insert(*id), "{id} is listed twice");
    }
}

/// The counter frequency is one a console actually reported, and the two names agree.
///
/// # Why this is not an equality against one number
///
/// It was, and a third capture falsified it. Two runs measured `0x5f259b8e` and a third
/// measured `0x5f259bb6` - **1,596,300,174 against 1,596,300,214**, forty hertz apart in one
/// and a half gigahertz. The two earlier runs agreed with each other *exactly*, so this is not
/// jitter in the reading: the counter is calibrated per boot, and is stable within a session.
///
/// So there is no single value to assert, and the measurement is now `constant = false`. What
/// is still checkable, and worth checking:
///
/// - **orbistoun answers a frequency some console actually reported**, not an invented one.
///   That is the defect this catches, and the per-boot variation does not excuse it.
/// - **the two names agree**, in every run and in orbistoun. The timestamp counter and the
///   process-time counter were measured separately and answered identically each time, which
///   is why orbistoun answers one constant for both.
#[test]
fn the_counter_frequency_matches_the_console() {
    let measured = measurement("120-measure/frequencies:sceKernelGetTscFrequency:frequency");
    let reported = measured.values();
    assert!(
        reported.len() > 1,
        "this test exists because the runs disagreed; if they no longer do, assert the value"
    );
    let answered = call("sceKernelGetTscFrequency", [0; GUEST_ARG_REGISTERS]);
    assert!(
        reported.contains(&answered),
        "orbistoun answers {answered}, which no run measured - the runs saw {reported:?}"
    );

    let twin =
        measurement("120-measure/frequencies:sceKernelGetProcessTimeCounterFrequency:frequency");
    assert_eq!(
        twin.values(),
        reported,
        "the console reported one frequency under two names, in every run"
    );
}

/// The character-classification tables carry what the console's C library carries.
///
/// **Read the way a guest reads them.** `_Getpctype()` answers the address of the entry for
/// index zero and a caller indexes from there, so this calls the function and indexes the
/// result rather than reading the data file - which checks installation and the margin below
/// zero as well as the values (D468).
///
/// Confirmed independently: the committed tables came from one capture, and a second run from
/// a different build form reproduced all twenty-two of these.
#[test]
fn the_ctype_tables_are_the_ones_the_console_carries() {
    for (symbol, prefix) in [
        ("_Getpctype", "mask_"),
        ("_Getptolower", "entry_"),
        ("_Getptoupper", "entry_"),
    ] {
        let base = call(symbol, [0; GUEST_ARG_REGISTERS]);
        assert_ne!(base, 0, "{symbol} answered a null table");
        let table = Measurements::builtin();
        let mut checked = 0_usize;
        for m in table.constants() {
            let Some(condition) = m.condition.strip_prefix(prefix) else {
                continue;
            };
            if m.subject != symbol {
                continue;
            }
            // `mask_tab_9` and `entry_eof_neg1` - the character is the last field, and a
            // `neg` prefix on it is the minus sign the id cannot carry.
            let last = condition
                .rsplit('_')
                .next()
                .expect("a condition has fields");
            let index: i64 = match last.strip_prefix("neg") {
                Some(digits) => -digits.parse::<i64>().expect("a negative index"),
                None => last.parse().expect("an index"),
            };
            let expected = u16::try_from(m.value().expect("a table entry is a number"))
                .expect("a table entry fits in an entry");
            // SAFETY: `base` is the address the implementation just answered for the entry
            // at index zero, and the measured tables span -8 to 263, so every index checked
            // here lands inside the allocation the implementation made for them.
            let at = unsafe {
                std::ptr::with_exposed_provenance::<u16>(
                    usize::try_from(base).expect("a guest address fits the host"),
                )
                .offset(isize::try_from(index).expect("a table index is small"))
            };
            // SAFETY: `at` was just computed inside the same allocation, and the table was
            // written as `u16` entries, so it is aligned and initialised.
            let found = unsafe { *at };
            assert_eq!(
                found, expected,
                "{symbol}[{index}] is {found:#x}, the console carries {expected:#x}"
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "{symbol}: no measurement was compared, so this asserted nothing"
        );
    }
}

/// Releasing a lock nobody holds answers the code the console answered.
///
/// **Compared at thirty-two bits, which is the width the measurement was taken at.** The
/// record reads `0xffffffff80020001` because the probe widened a C `int` through `int64_t`;
/// a prototype returning `int` never saw the other half of the register, so comparing all
/// sixty-four would be comparing against obSCEne's cast rather than against the console
/// (D480).
#[test]
fn releasing_an_unheld_lock_answers_the_measured_code() {
    let measured = measurement("015-sync/mutex-unlock-unheld:scePthreadMutexUnlock:unheld-unlock");
    let expected = measured.value().expect("a code is a number") as u32;

    let name = std::ffi::CString::new("unheld").expect("a lock name is text");
    let mut handle = 0_u64;
    let created = call(
        "scePthreadMutexInit",
        [
            std::ptr::from_mut(&mut handle) as u64,
            0,
            name.as_ptr() as u64,
            0,
            0,
            0,
        ],
    );
    assert_eq!(
        created, 0,
        "the lock has to exist for the question to mean anything"
    );

    let answered = call(
        "scePthreadMutexUnlock",
        [std::ptr::from_mut(&mut handle) as u64, 0, 0, 0, 0, 0],
    );
    assert_eq!(
        answered as u32, expected,
        "the console answered {expected:#x} for an unlock by a thread holding nothing"
    );
}

/// Taking a mutex twice answers what the console answered, for every type it was asked about.
///
/// # What this actually pins
///
/// The console was swept across five attribute type values and asked what a *second*
/// `Trylock` says. That produced the mapping orbistoun already uses - `2` is recursive, `4` is
/// error-checking, anything else is a plain lock - which until now lived in a comment citing
/// the check by name. This makes it a test: change the mapping and four measurements start
/// disagreeing with the machine they came from.
///
/// **Compared at thirty-two bits** (D480): the record reads `0xffffffff80020010` because the
/// probe widened a C `int`, and the other half of the register was never observed.
#[test]
fn a_second_acquisition_answers_the_measured_code_for_each_mutex_type() {
    for kind in [1_u64, 2, 3, 4] {
        let id = format!(
            "015-sync/mutex-recursion:scePthreadMutexTrylock:type-{kind}-second-acquisition"
        );
        let expected = measurement(&id).value().expect("a code is a number") as u32;

        let mut attr = 0_u64;
        let attr_at = std::ptr::from_mut(&mut attr) as u64;
        assert_eq!(
            call("scePthreadMutexattrInit", [attr_at, 0, 0, 0, 0, 0]),
            0,
            "type {kind}: the attribute object has to exist"
        );
        assert_eq!(
            call("scePthreadMutexattrSettype", [attr_at, kind, 0, 0, 0, 0]),
            0,
            "type {kind}: the type has to be accepted"
        );

        let name = std::ffi::CString::new(format!("type-{kind}")).expect("a lock name is text");
        let mut handle = 0_u64;
        let handle_at = std::ptr::from_mut(&mut handle) as u64;
        assert_eq!(
            call(
                "scePthreadMutexInit",
                [handle_at, attr_at, name.as_ptr() as u64, 0, 0, 0]
            ),
            0,
            "type {kind}: the lock has to be created"
        );

        assert_eq!(
            call("scePthreadMutexTrylock", [handle_at, 0, 0, 0, 0, 0]),
            0,
            "type {kind}: a fresh lock must be takeable"
        );
        let second = call("scePthreadMutexTrylock", [handle_at, 0, 0, 0, 0, 0]);
        assert_eq!(
            second as u32, expected,
            "type {kind}: the console answered {expected:#x} to a second acquisition"
        );
    }
}

/// The memory query accepts exactly the flag values the console accepted.
///
/// # It needed no allocation, which is worth writing down
///
/// This was carried as "needs a direct-memory allocation to query, which is a harness rather
/// than one call" - twice, on two entries. Reading the probe's own check settled it in a line:
/// it calls `sceKernelDirectMemoryQuery(0, flag, buffer, 256)` and allocates nothing. **The
/// difficulty was in my note, not in the call.**
///
/// The boundary itself - 0 and 1 accepted, 2 and 4 refused - is already in orbistoun, in a
/// comment citing this measurement (D398). A comment citing a measurement is checked by
/// nothing; this is the same claim as a test.
///
/// Compared at thirty-two bits (D480).
#[test]
fn the_memory_query_accepts_the_flags_the_console_accepted() {
    for flag in [0_u64, 1, 2, 4] {
        let id =
            format!("130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-{flag}");
        let expected = measurement(&id).value().expect("a code is a number") as u32;

        // The same buffer size the probe declared, so the conditions match rather than
        // resemble each other.
        let mut info = [0_u8; 256];
        let answered = call(
            "sceKernelDirectMemoryQuery",
            [0, flag, info.as_mut_ptr() as u64, info.len() as u64, 0, 0],
        );
        assert_eq!(
            answered as u32, expected,
            "flag {flag}: the console answered {expected:#x}"
        );
    }
}

/// Loading a module answers what the console answered, for the paths whose answer is fixed.
///
/// The exact paths the probe asked for, in the same call form - `(path, 0, 0, 0, 0, &started)`.
/// libkernel is resident and answers its well-known handle; a `/system` module is the
/// firmware's own copy and is refused with the not-found errno. Both are properties of the
/// call rather than of one machine's bookkeeping, which is what separates them from the
/// `/app0` handles in [`OPAQUE`].
///
/// Compared at thirty-two bits (D480).
#[test]
fn loading_a_module_answers_the_measured_code() {
    // The paths are obSCEne's own, in the order its `obs_module_quantity` table names them,
    // recovered from that table rather than guessed.
    //
    // **Two of these cannot detect a wrong path, and that was checked rather than assumed.**
    // Misspelling `libSceSysmodule.sprx` leaves this test passing: an unrecognised path falls
    // through to the same `0x8002_0002` the firmware directories answer, so for the `/system/`
    // entries this pins the answer and not the route to it - the same caveat the firmware
    // directory test below records. The `libkernel` entries do not share it: they answer
    // `0x2001`, so a wrong path there fails, and that is the break this was watched failing on.
    for (quantity, path) in [
        ("libkernel", "libkernel.prx"),
        ("libkernel-prx", "libkernel.prx"),
        ("libkernel-sprx", "libkernel.sprx"),
        ("system-libc", "/system/common/lib/libc.prx"),
        ("system-fios2", "/system/common/lib/libSceFios2.prx"),
        (
            "system-libc-internal",
            "/system/common/lib/libSceLibcInternal.sprx",
        ),
        (
            "system-sysmodule",
            "/system/common/lib/libSceSysmodule.sprx",
        ),
    ] {
        let id = format!("110-modules/load:sceKernelLoadStartModule:{quantity}");
        let expected = measurement(&id).value().expect("a code is a number") as u32;

        let asked = std::ffi::CString::new(path).expect("a module path is text");
        let mut started = 0_u32;
        let answered = call(
            "sceKernelLoadStartModule",
            [
                asked.as_ptr() as u64,
                0,
                0,
                0,
                0,
                std::ptr::from_mut(&mut started) as u64,
            ],
        );
        assert_eq!(
            answered as u32, expected,
            "{path}: the console answered {expected:#x}"
        );
    }
}

/// **The third field of the direct-memory query structure is the memory type.**
///
/// # The run that settled a question this project asked
///
/// `orbistoun-kernel` has carried this comment beside the field: *"What `3` denotes - a type,
/// or some state - is still open, and one run distinguishes them: allocate with several types
/// and query each back."* D398 could only say the field was not a boolean, because the console
/// answered `3` for the region at the bottom of the map and no boolean is three.
///
/// That run has now been taken. obSCEne allocated one 16 KiB page with each of `WB_ONION` (0),
/// `WC_GARLIC` (3) and `WB_GARLIC` (10) and read the field back for each:
///
/// ```text
/// wb-onion  -> 0x0        wc-garlic -> 0x3        wb-garlic -> 0xa
/// ```
///
/// Every one is the type it asked for. Three distinct answers rule out the other reading the
/// probe named - *"if it is the same value for every type, it is state"* - so the field is the
/// type, and orbistoun's model of it was right before it could be checked.
///
/// # Why this asserts through allocate rather than reading a constant
///
/// A test that queried a region orbistoun had already built would pass on the model alone. This
/// takes the same path the probe took: ask for a type, then ask what is there.
#[test]
fn the_third_query_field_is_the_memory_type_the_allocation_asked_for() {
    let mut compared = 0_usize;
    for (name, requested) in [("wb-onion", 0_u64), ("wc-garlic", 3), ("wb-garlic", 10)] {
        let measured = measurement(&format!("130-layout/memory-type:{name}:third-field"))
            .value()
            .expect("a memory type is a number");

        let mut physical = 0_u64;
        let allocated = call(
            "sceKernelAllocateDirectMemory",
            [
                0,
                0x3_0000_0000,
                0x4000,
                0x4000,
                requested,
                std::ptr::from_mut(&mut physical) as u64,
            ],
        );
        assert_eq!(allocated, 0, "{name}: the allocation must succeed");

        let mut info = [0_u64; 3];
        let queried = call(
            "sceKernelDirectMemoryQuery",
            [
                physical,
                0,
                info.as_mut_ptr() as u64,
                (info.len() * 8) as u64,
                0,
                0,
            ],
        );
        assert_eq!(queried, 0, "{name}: the query must succeed");
        assert_eq!(
            info[2], measured,
            "{name}: the console answered {measured:#x} for the type it was asked for"
        );
        compared += 1;
    }
    assert_eq!(compared, 3, "all three measured types are compared");
}

/// **A mutex attribute round-trips the types the console round-trips, and refuses the one it
/// refuses.**
///
/// One attribute object, `Settype` then `Gettype`, for each of 0..4 - which is the run the
/// queue said this needed and a capture has now taken. The console's answers:
///
/// ```text
/// default 1     0 -> refused     1 -> 1     2 -> 2     3 -> 3     4 -> 4
/// ```
///
/// # The refusal is asserted as behaviour, not as its recorded value
///
/// `type-0-read-back` reads `0xffff_ffff_ffff_ffff`, and that is **the probe's marker** - its
/// own comment says each entry records what `Gettype` read back *"or -1 where `Settype` refused
/// the type or `Gettype` failed"*. Asserting orbistoun answers `-1` would be asserting against
/// the instrument, which is D497's mistake. So this asserts what the marker encodes: after
/// `Settype(0)` the round trip must not succeed.
///
/// **What the console answers for that refusal is not measured**, so orbistoun answers its own
/// placeholder and this test does not look at the code - only that zero does not come back.
#[test]
fn a_mutex_attribute_round_trips_the_types_the_console_does() {
    let want = |id: &str| {
        measurement(&format!(
            "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:{id}"
        ))
        .value()
        .expect("a mutex type is a number")
    };

    let mut attr = 0_u64;
    // The address is taken once, because a closure below would otherwise hold a borrow of
    // `attr` across every later use of it. A guest passes an address; so does this.
    let attr_ptr = std::ptr::from_mut(&mut attr) as u64;
    assert_eq!(
        call("scePthreadMutexattrInit", [attr_ptr, 0, 0, 0, 0, 0]),
        0,
        "the attribute must initialise"
    );

    let get = |read: &mut u32| {
        call(
            "scePthreadMutexattrGettype",
            [attr_ptr, std::ptr::from_mut(read) as u64, 0, 0, 0, 0],
        )
    };
    let set = |requested: u64| {
        call(
            "scePthreadMutexattrSettype",
            [attr_ptr, requested, 0, 0, 0, 0],
        )
    };

    let mut read = 0_u32;
    assert_eq!(get(&mut read), 0, "a fresh attribute answers its type");
    assert_eq!(
        u64::from(read),
        want("default-type"),
        "the type a freshly initialised attribute carries"
    );

    // Zero: the console refuses it, so the round trip must not come back with zero.
    set(0);
    assert_eq!(get(&mut read), 0, "the attribute is still readable");
    assert_ne!(
        u64::from(read),
        0,
        "type 0 must not round-trip - the console refuses it"
    );

    // One through four: a clean round trip, each read back as itself.
    let mut compared = 0_usize;
    for requested in 1..=4_u64 {
        let expected = want(&format!("type-{requested}-read-back"));
        assert_eq!(set(requested), 0, "type {requested} must be accepted");
        assert_eq!(get(&mut read), 0, "type {requested} must read back");
        assert_eq!(
            u64::from(read),
            expected,
            "the console read {expected} back after setting {requested}"
        );
        compared += 1;
    }
    assert_eq!(compared, 4, "all four round-tripping types are compared");
}

/// **The console writes eight bytes where it was given four, and so does orbistoun.**
///
/// obSCEne plants `0xA5A5A5A5` in the word after an `int handle`, calls `sceKernelCreateSema`
/// on the `int`, and reads the guard back. It read **`0x0`**: the call wrote past the end of
/// what it was given, which obSCEne reports as a failure of the platform rather than of the
/// check.
///
/// # This reverses a decision, on purpose
///
/// D210 narrowed orbistoun's write to four bytes, reasoning from public interface documentation
/// that the destination is an `int *`. The measurement says otherwise, and `Measured` outranks
/// `Published` here for exactly this reason - a documented layout and a real one have diverged
/// once before (D468). A guest is built against the console, so writing four bytes leaves a
/// neighbour holding a value the console would have cleared.
///
/// The guard is asserted, not the handle: the handle is orbistoun's own number and says
/// nothing about the platform, where the word beyond it is the whole finding.
#[test]
fn creating_a_semaphore_writes_past_the_int_it_was_given() {
    /// The same shape the probe used: an `int` for the handle, a guard immediately after it.
    #[repr(C)]
    struct Slot {
        handle: i32,
        guard: u32,
    }

    let expected = measurement(
        "018-relational/handle-fits-its-out-parameter:sceKernelCreateSema:guard-after-handle",
    )
    .value()
    .expect("a guard word is a number");
    let mut slot = Slot {
        handle: 0,
        guard: 0xA5A5_A5A5,
    };
    let name = std::ffi::CString::new("orbistoun-width").expect("a name is text");
    let answered = call(
        "sceKernelCreateSema",
        [
            std::ptr::from_mut(&mut slot.handle) as u64,
            name.as_ptr() as u64,
            0,
            0,
            4,
            0,
        ],
    );

    assert_eq!(answered, 0, "the semaphore must be created");
    assert_eq!(
        u64::from(slot.guard),
        expected,
        "the console left {expected:#x} in the word after the handle"
    );
}

/// The two encoder system modules the console loads, orbistoun answers the same way.
///
/// # Why only two of the nine
///
/// obSCEne asked for nine and was answered `0` for `VENC` and `VIDEOREC` and `0x805a1000` for
/// the other seven. **Only the two successes are claimed**, because orbistoun answers `0` to
/// every identifier by design (D125) and the seven refusals may be a property of the capture's
/// application category rather than of the modules - obSCEne's own D301 records that category
/// deciding an unrelated call. Claiming the refusals would pin orbistoun to one process's
/// privileges, which is the mistake `OPAQUE` exists to prevent.
///
/// So this is a real agreement on two, and the other seven stay in the queue with the question
/// that would settle them written down.
#[test]
fn the_encoder_system_modules_the_console_loads_are_answered_the_same() {
    let mut compared = 0_usize;
    for (module, id) in [("VENC", 0x00a0_u64), ("VIDEOREC", 0x0081_u64)] {
        let measured = measurement(&format!("106-encoder/sysmodule-load:{module}:rc"))
            .value()
            .expect("a return code is a number") as u32;
        let answered = call("sceSysmoduleLoadModule", [id, 0, 0, 0, 0, 0]);
        assert_eq!(
            answered as u32, measured,
            "{module} ({id:#x}): the console answered {measured:#x}"
        );
        compared += 1;
    }
    // Asserted rather than counted on: a loop that quietly compared fewer would pass.
    assert_eq!(compared, 2, "both measured successes are compared");
}

/// Every firmware directory is refused, including the one that is not under `/system`.
///
/// # What this test can and cannot tell you
///
/// **It cannot detect the bug that prompted it, and that was checked rather than assumed.**
/// The platform keeps its modules in three directories, and `/system_ex/common_ex/lib/` is not
/// under `/system/` - so a rule written as one prefix covered 274 modules and missed 234. But
/// an unmatched path falls through to the unrecognised-path refusal, and the console answers
/// `0x8002_0002` for a missing path too (`060-module/load-rejects-missing`). Both routes give
/// the same code. Reverting to the single prefix leaves this test passing.
///
/// So this pins **the answer**, not the reasoning. The reasoning is worth keeping anyway, and
/// the table it lives in earns its place the moment the loader stops refusing: `/app0` will
/// load and a firmware path must not, and at that point the two branches stop agreeing. A
/// distinction that is invisible today is the one that breaks silently tomorrow.
///
/// The two `/system` paths are measured (`110-modules/load`); `/system_ex` is the same rule
/// applied to a directory the same reasoning covers, and **no probe has asked for one**.
///
/// Reference: the platform library survey in the sibling conformance-probe repository.
#[test]
fn a_module_in_any_firmware_directory_is_refused() {
    let refused = measurement("110-modules/load:sceKernelLoadStartModule:system-libc")
        .value()
        .expect("a code is a number") as u32;

    for path in [
        "/system/common/lib/libc.prx",
        "/system_ex/common_ex/lib/libSceWebKit2.sprx",
        "/system/priv/lib/libSceSblAuthMgr.sprx",
    ] {
        let asked = std::ffi::CString::new(path).expect("a module path is text");
        let mut started = 0_u32;
        let answered = call(
            "sceKernelLoadStartModule",
            [
                asked.as_ptr() as u64,
                0,
                0,
                0,
                0,
                std::ptr::from_mut(&mut started) as u64,
            ],
        );
        assert_eq!(
            answered as u32, refused,
            "{path}: a firmware module is the platform's own copy and is not loaded again"
        );
    }
}

/// A title's own module gets a distinct, non-negative handle - the part a guest actually uses.
///
/// The console's numbers cannot be asserted (see [`OPAQUE`]), but the *shape* can, and it is
/// the half a guest keys on: two loads must not answer the same handle, and neither may look
/// like a failure.
#[test]
fn a_title_module_gets_its_own_non_negative_handle() {
    let mut seen = Vec::new();
    for path in [
        "/app0/sce_module/libc.prx",
        "/app0/sce_module/libSceFios2.prx",
    ] {
        let asked = std::ffi::CString::new(path).expect("a module path is text");
        let mut started = 0_u32;
        let handle = call(
            "sceKernelLoadStartModule",
            [
                asked.as_ptr() as u64,
                0,
                0,
                0,
                0,
                std::ptr::from_mut(&mut started) as u64,
            ],
        );
        assert!(
            (handle as i64) >= 0,
            "{path}: a title's own module must load, not be refused ({handle:#x})"
        );
        assert!(
            !seen.contains(&handle),
            "{path}: two loads answered the same handle {handle:#x}"
        );
        seen.push(handle);
    }
}

/// The sysctl knobs orbistoun can source answer the widths the console answered.
///
/// **The width is the claim, not the value.** A caller reading four bytes of an eight-byte
/// answer reads a different number than was written, which is the failure D210 and D272 both
/// record; the console's byte count is the thing that pins it.
#[test]
fn the_sourceable_sysctl_knobs_answer_the_measured_widths() {
    for id in [
        "135-sysctl/names:kern.ostype:length",
        "135-sysctl/names:hw.ncpu:length",
        "135-sysctl/names:hw.pagesize:length",
        "135-sysctl/names:machdep.tsc_freq:length",
        "135-sysctl/names:kern.hostname:length",
    ] {
        let measured = measurement(id);
        let expected = measured.value().expect("a length is a number");

        // The size half of the size/value pair: a null destination asks how big the answer is.
        let name = std::ffi::CString::new(measured.subject.clone()).expect("a knob name is text");
        let mut length = 0_u64;
        let rc = call(
            "sysctlbyname",
            [
                name.as_ptr() as u64,
                0,
                std::ptr::from_mut(&mut length) as u64,
                0,
                0,
                0,
            ],
        );
        assert_eq!(
            rc, 0,
            "{}: orbistoun refused a knob the console answered",
            measured.subject
        );
        assert_eq!(
            length, expected,
            "{}: the console answered {expected} bytes",
            measured.subject
        );
    }
}

/// A guest starts in the float environment the console starts a title in.
///
/// # Why the four fields and not the raw value
///
/// The run measured `MXCSR` as `0x9fe0`. Four of its fields are **configuration** - flush to
/// zero, denormals are zero, round to nearest, all six exceptions masked - and those are
/// asserted here. The low six bits are sticky **status**, and `0x9fe0` carries the precision
/// flag set by float work the console had already done. Reproducing that would tell a guest an
/// inexact result had occurred before it executed an instruction, so orbistoun installs the
/// configuration and leaves status clear (D486).
///
/// Read back from the register rather than compared against the constant, so this fails if the
/// install silently does not happen - which comparing two constants could never catch.
#[test]
fn the_guest_starts_in_the_float_environment_the_console_uses() {
    orbistoun_abi::enter::adopt_guest_float_environment();
    let live = orbistoun_abi::enter::float_environment();

    let field = |id: &str| {
        measurement(id)
            .value()
            .expect("a measured flag is a number")
    };
    assert_eq!(
        u64::from((live >> 6) & 1),
        field("035-libc/fpu-environment:mxcsr:daz_denormals_are_zero"),
        "denormals-are-zero"
    );
    assert_eq!(
        u64::from((live >> 15) & 1),
        field("035-libc/fpu-environment:mxcsr:ftz_flush_to_zero"),
        "flush-to-zero"
    );
    assert_eq!(
        u64::from((live >> 13) & 3),
        field("035-libc/fpu-environment:mxcsr:rounding_mode"),
        "rounding mode"
    );
    assert_eq!(
        u64::from((live >> 7) & 0x3f),
        field("035-libc/fpu-environment:mxcsr:exception_masks"),
        "exception masks"
    );

    // **And the status bits are clear**, which is the half the raw measurement must not be
    // copied for. Asserted rather than assumed: installing `0x9fe0` verbatim would pass every
    // check above and still be wrong here.
    assert_eq!(
        live & 0x3f,
        0,
        "a status flag was installed as though it were configuration"
    );
}

/// **The console's raw `MXCSR` is orbistoun's, once the sticky flags are taken off.**
///
/// The raw value was outstanding because it mixes two things: `0x9fe0` is the four
/// configuration fields *plus* bit 5, the precision flag, which the console's own startup
/// arithmetic had already set before the title got control. Orbistoun installs `0x9fc0` and
/// deliberately does not reproduce that bit - writing it would tell a guest an inexact result
/// had occurred before it executed an instruction (D486).
///
/// That made it look unclaimable, and it is not. **The split is published, not guessed**: bits
/// 0-5 of `MXCSR` are the exception *flags* - sticky status a program accumulates - and every
/// other bit is configuration. So the whole raw value is claimable against the one thing it
/// says about the platform:
///
/// ```text
/// 0x9fe0 & !0x3f == 0x9fc0 == GUEST_MXCSR
/// ```
///
/// This asserts the equality rather than the constant, so it stays true if the console's
/// configuration is ever measured differently: the mask is the claim, not the number.
#[test]
fn the_consoles_float_configuration_is_the_raw_value_without_its_status_flags() {
    /// Bits 0-5: the six exception flags, which are status and not configuration.
    ///
    /// Intel SDM Vol. 1, the `MXCSR` register: IE, DE, ZE, OE, UE, PE. Sticky - set by
    /// arithmetic and cleared only by writing the register - so they describe what a program
    /// has done, never how it was set up.
    const STATUS_FLAGS: u64 = 0x3f;

    let raw = measurement("035-libc/fpu-environment:mxcsr:raw")
        .value()
        .expect("the raw MXCSR is a number");

    orbistoun_abi::enter::adopt_guest_float_environment();
    let live = u64::from(orbistoun_abi::enter::float_environment());

    assert_eq!(
        raw & !STATUS_FLAGS,
        live,
        "the console's {raw:#x} without its status flags is {:#x}, and orbistoun installs          {live:#x}",
        raw & !STATUS_FLAGS
    );
    // **And the difference is only status.** Without this the assertion above would also pass
    // if orbistoun had set a configuration bit the console clears, because masking hides it.
    assert_eq!(
        raw & STATUS_FLAGS,
        0x20,
        "the only bit the console carries and orbistoun does not is the precision flag"
    );
}

/// Every firmware path the encoder probe tried is refused, with the code the console gave.
///
/// # What is being claimed
///
/// Twenty-four paths across four directories - `/system/common/lib`, `/system/priv/lib`,
/// `/system/sys/lib` and `/system/lib` - each asked for by `sceKernelLoadStartModule`. The
/// console answered `0x80020002` to every one, which is the vendor encoding of `ENOENT`.
///
/// **Compared at thirty-two bits**, which is the width the probe took it at: it reports
/// `(uint64_t)(uint32_t)h` from a prototype returning `int`, so the upper half is obSCEne's
/// cast rather than the console's answer (D480).
///
/// # Two of the four directories are right for the wrong reason
///
/// `/system/common/lib/` and `/system/priv/lib/` are in `FIRMWARE_MODULE_DIRECTORIES` and are
/// refused because this project knows they hold the platform's own modules. `/system/sys/lib/`
/// and `/system/lib/` are in no table and reach the same code by falling through to the
/// unrecognised-path refusal at the end.
///
/// Same value, different reason - and the constant's own comment already records that hazard
/// biting once, when `/system_ex` was refused by luck for two hundred and thirty-four modules.
/// Asserted anyway, because the guest cannot tell the two apart and the answer is what it sees.
#[test]
fn a_firmware_encoder_path_is_refused() {
    let mut checked = 0_usize;
    for measurement in Measurements::builtin().constants() {
        if measurement.check != "106-encoder/path-probe" || measurement.condition != "handle" {
            continue;
        }
        let expected = measurement.value().expect("a refusal is a number");
        let path = std::ffi::CString::new(measurement.subject.clone()).expect("a path has no NUL");
        let mut res: u64 = 0;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = path.as_ptr() as u64;
        args[5] = std::ptr::from_mut(&mut res) as u64;
        let answered = call("sceKernelLoadStartModule", args);
        assert_eq!(
            answered & 0xffff_ffff,
            expected & 0xffff_ffff,
            "{} answered {answered:#x}, the console answered {expected:#x}",
            measurement.subject
        );
        checked += 1;
    }
    assert_eq!(
        checked, 24,
        "twenty-four paths were measured; a loop that compared fewer would pass by doing less"
    );
}
