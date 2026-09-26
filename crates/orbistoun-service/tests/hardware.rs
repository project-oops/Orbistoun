//! Orbistoun checked against what the hardware was measured to do.
//!
//! `crates/orbistoun-hle/data/hardware.toml` is generated from conformance captures and holds every
//! measurement. A test here names a measurement by id and asserts orbistoun answers the same. The
//! gate is coverage: every constant measurement is claimed by a test or listed in [`OUTSTANDING`]
//! with the reason, so a new capture arrives as work. A known divergence is written into
//! [`OUTSTANDING`] rather than left as a permanently red test; fixing it moves the id into a test.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::hardware::{Measurement, Measurements};

/// Measurements this file asserts orbistoun against.
const CLAIMED: &[&str] = &[
    // Return codes the hardware gave and orbistoun answers, asserted below.
    "031-stackattr/address-is-the-base:sceKernelIsStack:is-stack",
    "031-stackattr/fresh-attr-names-no-stack:scePthreadAttrGetstackaddr:stack-address",
    "031-stackattr/fresh-attr-names-no-stack:scePthreadAttrGetstacksize:stack-size",
    "032-syncaddr/wake-releases-a-waiter:sceKernelSyncOnAddressWake:returned",
    "032-syncaddr/wake-releases-a-waiter:sceKernelSyncOnAddressWake:retry-all",
    "032-syncaddr/wake-with-no-waiter:sceKernelSyncOnAddressWake:returned",
    "130-layout/net-interfaces:getifaddrs:return_code",
    "130-layout/user-service:sceUserServiceGetInitialUser:return_code",
    // The sync bounds the hardware measured: the semaphore count and the event-flag wait mode,
    // asserted below.
    "016-syncbounds/sema-count:sceKernelPollSema:need-0-of-empty",
    "016-syncbounds/sema-count:sceKernelPollSema:need-1-of-1-left",
    "016-syncbounds/sema-count:sceKernelPollSema:need-2-of-1-left",
    "016-syncbounds/sema-count:sceKernelPollSema:need-2-of-3",
    "016-syncbounds/event-flag-waitmode:sceKernelPollEventFlag:mode-0x00-both-of-one",
    "016-syncbounds/event-flag-waitmode:sceKernelPollEventFlag:mode-0x01-both-of-one",
    "016-syncbounds/event-flag-waitmode:sceKernelPollEventFlag:mode-0x02-both-of-one",
    "016-syncbounds/event-flag-waitmode:sceKernelPollEventFlag:mode-0x11-both-of-one",
    // Every platform module path the encoder probe tried, refused with the same code.
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
    // The float environment a title is entered under, asserted against what orbistoun installs
    // (D486).
    "035-libc/fpu-environment:mxcsr:daz_denormals_are_zero",
    "035-libc/fpu-environment:mxcsr:ftz_flush_to_zero",
    "035-libc/fpu-environment:mxcsr:rounding_mode",
    "035-libc/fpu-environment:mxcsr:exception_masks",
    // The raw `MXCSR` value is not claimed here: runs saw `0x9fe0` and `0x9fc0`, differing in the
    // sticky precision flag the hardware's startup may set, so it varies. The surviving assertion
    // is `the_consoles_float_configuration_is_the_raw_value_without_its_status_flags`.
    //
    // One attribute object, set then get, for each of 0..4.
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:default-type",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-0-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-1-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-2-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-3-read-back",
    "015-sync/mutexattr-round-trip:scePthreadMutexattrGettype:type-4-read-back",
    // The hardware writes eight bytes where it was given four, measured by a guard word.
    "018-relational/handle-fits-its-out-parameter:sceKernelCreateSema:guard-after-handle",
    // The character-classification tables, confirmed by a second capture from a different build
    // form, each entry read through the function as a guest reads it.
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
    "130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-1",
    "110-modules/load:sceKernelLoadStartModule:libkernel",
    "110-modules/load:sceKernelLoadStartModule:system-libc",
    "110-modules/load:sceKernelLoadStartModule:system-fios2",
    "135-sysctl/names:kern.ostype:length",
    "135-sysctl/names:hw.ncpu:length",
    "135-sysctl/names:hw.pagesize:length",
    "135-sysctl/names:machdep.tsc_freq:length",
    "135-sysctl/names:kern.hostname:length",
    // The third query field is the memory type, measured by allocating one page of each type and
    // reading the field back.
    "130-layout/memory-type:wb-onion:third-field",
    "130-layout/memory-type:wc-garlic:third-field",
    "130-layout/memory-type:wb-garlic:third-field",
    // The two encoder system modules the hardware loads; the seven it refuses stay in the queue,
    // because that refusal may belong to the capture's application category.
    "106-encoder/sysmodule-load:VIDEOREC:rc",
    // The other four paths obSCEne loaded, from its own quantity table.
    "110-modules/load:sceKernelLoadStartModule:system-libc-internal",
    "110-modules/load:sceKernelLoadStartModule:system-sysmodule",
    "110-modules/load:sceKernelLoadStartModule:libkernel-prx",
    "110-modules/load:sceKernelLoadStartModule:libkernel-sprx",
];

/// Measurements whose value is not a property of the interface, so nothing can assert it.
///
/// Separate from [`OUTSTANDING`], which means "not done yet", so that queue has no permanent
/// residents. A module handle is the typical case: the hardware answered `0x15` and `0x14` for two
/// `/app0` modules, a count of what that loader had already placed. The record still says the call
/// answers a small non-negative handle for a title's own module, and that shape is asserted.
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
        "120-measure/cpuid:cpuid:feature_edx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid:family",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:cpuid_ext:feature_edx",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    (
        "120-measure/cpuid:rdtscp:numa_node_id",
        "guest instructions run natively on the host CPU, so `cpuid` answers the host's - presenting the console's would need the instruction trapped, and interception here is linking rather than hooking (principle 7)",
    ),
    // The hardware's own address space: values that belong to that machine and that run, each entry
    // carrying its own reason.
    (
        "100-input/dualsense-symbols:scePadDeviceClassGetExtendedInformation:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadDeviceClassParseData:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadGetControllerInformation:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadGetTriggerEffectState:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadSetTriggerEffect:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadSetVibrationForce:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "100-input/dualsense-symbols:scePadSetVibrationMode:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardClose:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardInit:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardOpen:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardReadState:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmBatchStartBuffer:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmBatchWait:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmFinalize:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmInitialize:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmInstanceCreate:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmInstanceDestroy:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "108-audiodec/ajm:sceAjmModuleRegister:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "130-layout/net-interfaces:eth0:flags",
        "the interface flags of one machine's network stack - which interfaces exist and what they are configured as is that console's setup, not the platform's. `getifaddrs` answering at all is claimable and is listed apart",
    ),
    (
        "130-layout/net-interfaces:lo0:flags",
        "the interface flags of one machine's network stack - which interfaces exist and what they are configured as is that console's setup, not the platform's. `getifaddrs` answering at all is claimable and is listed apart",
    ),
    (
        "130-layout/net-interfaces:wlan0:flags",
        "the interface flags of one machine's network stack - which interfaces exist and what they are configured as is that console's setup, not the platform's. `getifaddrs` answering at all is claimable and is listed apart",
    ),
    (
        "130-layout/net-interfaces:wlan1:flags",
        "the interface flags of one machine's network stack - which interfaces exist and what they are configured as is that console's setup, not the platform's. `getifaddrs` answering at all is claimable and is listed apart",
    ),
    (
        "130-layout/user-service:sceUserServiceGetInitialUser:user_id",
        "one console's user identifier. A number that machine assigned to an account is not a property of the platform, and orbistoun answering it would be reproducing somebody's bookkeeping",
    ),
    (
        "136-kernel/handoff:allproc_addr:val",
        "the kernel's handoff to an exploit payload, which is below the user-space boundary this project works inside - orbistoun runs guest code, it does not become a payload, so there is nothing here for it to answer",
    ),
    (
        "136-kernel/handoff:rwpair_0:val",
        "the kernel's handoff to an exploit payload, which is below the user-space boundary this project works inside - orbistoun runs guest code, it does not become a payload, so there is nothing here for it to answer",
    ),
    (
        "136-kernel/handoff:rwpair_1:val",
        "the kernel's handoff to an exploit payload, which is below the user-space boundary this project works inside - orbistoun runs guest code, it does not become a payload, so there is nothing here for it to answer",
    ),
    (
        "136-kernel/handoff:rwpipe_0:val",
        "the kernel's handoff to an exploit payload, which is below the user-space boundary this project works inside - orbistoun runs guest code, it does not become a payload, so there is nothing here for it to answer",
    ),
    (
        "136-kernel/handoff:rwpipe_1:val",
        "the kernel's handoff to an exploit payload, which is below the user-space boundary this project works inside - orbistoun runs guest code, it does not become a payload, so there is nothing here for it to answer",
    ),
    (
        "138-layout/addresses:getpid:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:malloc:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelAllocateDirectMemory:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelClose:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelDirectMemoryQuery:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelDlsym:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelGetDirectMemorySize:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelGetModuleInfo:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelGetModuleList:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelGetProcessTime:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelLoadStartModule:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelMapDirectMemory:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelOpen:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelRead:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelReadTsc:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelUsleep:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceKernelWrite:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:scePthreadCreate:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:scePthreadJoin:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:sceSysmoduleLoadModule:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "138-layout/addresses:strlen:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    // Sockets, keyboard and mouse resolving on the hardware: where each landed is that machine's,
    // and how long the probe waited for an operator is that run's.
    (
        "101-input-ext/keyboard-held:sceKeyboardReadState:hold-shift-and-a-letter-now",
        "how long the probe waited for an operator, or for a socket that was never going to answer. A duration measured on that machine and on that day, not a property of the platform",
    ),
    (
        "101-input-ext/mouse-moving:sceMouseRead:move-and-hold-a-button-now",
        "how long the probe waited for an operator, or for a socket that was never going to answer. A duration measured on that machine and on that day, not a property of the platform",
    ),
    (
        "101-input-ext/reachability:sceKeyboardReadState:resolved",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetAccept:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetAccept:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetAccept:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetBind:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetBind:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetBind:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetConnect:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetConnect:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetConnect:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetListen:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetListen:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetListen:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetRecv:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetRecv:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetRecv:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSend:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSend:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSend:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSetsockopt:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSetsockopt:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSetsockopt:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocket:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocket:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocket:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocketClose:dlsym",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocketClose:dlsym-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "102-net/resolve:sceNetSocketClose:import-null",
        "an address or handle on the console, which orbistoun cannot answer - it places its own, and every base it uses is in docs/ADDRESS_MAP.md. That the symbol resolves at all is the fact worth having and is recorded beside it",
    ),
    (
        "101-input-ext/kbd-read:sceKeyboardReadState:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. A census of where a symbol landed says the symbol exists, which is recorded beside it; where it landed is a property of that machine's loader",
    ),
    (
        "101-input-ext/mouse-read:sceMouseInit:unlinked-stub",
        "the address of a stub the loader left unlinked in that build - a fact about that build's relocation, not about the platform, and orbistoun places its own stubs at bases listed in docs/ADDRESS_MAP.md",
    ),
    (
        "102-net/resolve:__error:kexport",
        "an address on the console, found by walking the kernel export table - the payload environment's own route, and something this project models nothing of, deliberately. Orbistoun places its own addresses; every base it uses is in docs/ADDRESS_MAP.md and none of them is this",
    ),
    (
        "102-net/resolve:__sys_socketex:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:_sendto:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:_setsockopt:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:accept:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:bind:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:close:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:connect:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:fcntl:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:listen:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "102-net/resolve:recv:kexport",
        "as `__error:kexport` above: an address found by the kernel export-table walk",
    ),
    (
        "106-encoder/sysmodules:sceSysmoduleIsLoaded:table-vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol is in the table is the fact worth having, and it is recorded beside this",
    ),
    (
        "106-encoder/sysmodules:sceSysmoduleLoadModule:table-vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol is in the table is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:__sys_socketex:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:bind:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:listen:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:accept:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:recv:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:_sendto:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:close:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:_setsockopt:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:connect:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:fcntl:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/posix-symbols:__error:vaddr",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
    (
        "102-net/socket-resolve:__sys_socketex:address",
        "an address on the console, and orbistoun places its own - every base it uses is in docs/ADDRESS_MAP.md and none of them is this. That the symbol resolves is the fact worth having, and it is recorded beside this",
    ),
];

/// Constant measurements nothing asserts yet, and why: the work queue.
///
/// Each entry is complete when orbistoun answers what the hardware answered and the id moves into
/// [`CLAIMED`] with a test.
const OUTSTANDING: &[(&str, &str)] = &[
    (
        "000-hw/sw-version:sceKernelGetSystemSwVersion:rc",
        "the console answers `0` because a console has a software version. Orbistoun refuses with `0x80020002` when none is configured, which is deliberate - answering a made-up version is the thing D420 declined to do - so the call is right and the claim needs a machine presented before it. **Not written**: `machine::present` is a process-wide `OnceLock`, so a test that set it would decide what every other test in that binary sees, according to which ran first. It needs a harness that owns the process, not a line in this one (D611)",
    ),
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
    // Codes recorded as `0xffffffff8002_xxxx`. The probe reports `(uint64_t)(int64_t)` of a C
    // `int`, so the leading `ffffffff` is obSCEne widening the value, and the upper half of `rax`
    // was never observed. At 32 bits each is a value orbistoun already produces (`0x80020001` is
    // `errno::NOT_OWNER`, `0x80020010` `BUSY`, `0x80020016` `INVALID`, `0x80020002` `NO_ENTRY`,
    // `0x80020003` `NO_SUCH`); what keeps them here is reproducing each check's condition.
    (
        "135-sysctl/names:kern.version:length",
        "the console answers a 0x2c-byte build banner; orbistoun refuses the knob rather than inventing one (D397)",
    ),
    // Conditions of functions orbistoun implements that nothing asserts yet, and symbols the
    // hardware resolves that this project does not declare, across the controller, keyboard, mouse,
    // audio decoder and AJM libraries.
    (
        "000-hw/tsc-frequency:sceKernelGetTscFrequency:hz",
        "`sceKernelGetTscFrequency` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "031-stackattr/address-is-the-base:sceKernelIsStack:high",
        "`sceKernelIsStack` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "031-stackattr/address-is-the-base:sceKernelIsStack:low",
        "`sceKernelIsStack` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "031-stackattr/address-is-the-base:scePthreadAttrGetstackaddr:placement",
        "`scePthreadAttrGetstackaddr` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "031-stackattr/self-describes:scePthreadAttrGetstackaddr:stack-address",
        "`scePthreadAttrGetstackaddr` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "031-stackattr/self-describes:scePthreadAttrGetstacksize:stack-size",
        "`scePthreadAttrGetstacksize` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "085-videobuf/buffer-shape:sceVideoOutRegisterBuffers:onion,0x4000,1",
        "`sceVideoOutRegisterBuffers` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "085-videobuf/framebuffer-refusal:sceVideoOutRegisterBuffers:baseline",
        "`sceVideoOutRegisterBuffers` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "100-input/dualsense-symbols:scePadDeviceClassGetExtendedInformation:unresolved",
        "the console resolves `scePadDeviceClassGetExtendedInformation` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "100-input/dualsense-symbols:scePadDeviceClassParseData:unresolved",
        "the console resolves `scePadDeviceClassParseData` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "100-input/dualsense-symbols:scePadGetControllerInformation:unresolved",
        "the console resolves `scePadGetControllerInformation` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "100-input/dualsense-symbols:scePadGetTriggerEffectState:unresolved",
        "the console resolves `scePadGetTriggerEffectState` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "100-input/dualsense-symbols:scePadSetTriggerEffect:unresolved",
        "the console resolves `scePadSetTriggerEffect` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "100-input/dualsense-symbols:scePadSetVibrationForce:unresolved",
        "`scePadSetVibrationForce` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "100-input/dualsense-symbols:scePadSetVibrationMode:unresolved",
        "the console resolves `scePadSetVibrationMode` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardClose:unresolved",
        "the console resolves `sceKeyboardClose` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardInit:unresolved",
        "the console resolves `sceKeyboardInit` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardOpen:unresolved",
        "the console resolves `sceKeyboardOpen` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardReadState:unresolved",
        "the console resolves `sceKeyboardReadState` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseClose:unresolved",
        "the console resolves `sceMouseClose` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseInit:unresolved",
        "the console resolves `sceMouseInit` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseOpen:unresolved",
        "the console resolves `sceMouseOpen` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseRead:unresolved",
        "the console resolves `sceMouseRead` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/related-libs:libSceAjm:absent",
        "`libSceAjm` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "108-audiodec/related-libs:libSceAudioIn:absent",
        "`libSceAudioIn` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "108-audiodec/related-libs:libSceAudiodec:absent",
        "`libSceAudiodec` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "108-audiodec/related-libs:libSceOpusCeltDec:absent",
        "`libSceOpusCeltDec` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "108-audiodec/related-libs:libSceOpusDec:absent",
        "`libSceOpusDec` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    (
        "108-audiodec/symbols:sceAudiodecClearContext:unresolved",
        "the console resolves `sceAudiodecClearContext` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecCreateDecoder:unresolved",
        "the console resolves `sceAudiodecCreateDecoder` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecCreateDecoderEx:unresolved",
        "the console resolves `sceAudiodecCreateDecoderEx` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecDecode:unresolved",
        "the console resolves `sceAudiodecDecode` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecDecodeEx:unresolved",
        "the console resolves `sceAudiodecDecodeEx` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecDeleteDecoder:unresolved",
        "the console resolves `sceAudiodecDeleteDecoder` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecInitialize:unresolved",
        "the console resolves `sceAudiodecInitialize` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "108-audiodec/symbols:sceAudiodecTerminate:unresolved",
        "the console resolves `sceAudiodecTerminate` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "130-layout/pad-controller:scePadGetHandle:handle",
        "the console resolves `scePadGetHandle` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "130-layout/user-service:sceUserServiceGetLoginUserIdList:return_code",
        "the console resolves `sceUserServiceGetLoginUserIdList` and orbistoun does not declare it, so there is no implementation for a claim to check. **This is the finding, not the obstacle**: a symbol a console exports and this project has never heard of is a named gap rather than an unknown one, and the subsystem it belongs to is the unit of work (principle 6)",
    ),
    (
        "130-layout/user-service:sceUserServiceGetUserName:return_code",
        "`sceUserServiceGetUserName` is implemented and this condition is not asserted yet - reproducing the probe's setup is the work, not the call",
    ),
    // What the network calls answered, recorded before anything implements them so no code is
    // invented: `sceNetRecv` answers `0x80410123` when it would block.
    (
        "101-input-ext/mouse-moving:sceMouseRead:extent-four",
        "orbistoun does not declare `sceMouseRead` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x0` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "101-input-ext/mouse-moving:sceMouseRead:extent-one",
        "orbistoun does not declare `sceMouseRead` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x0` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/listener:sceNetBind:bound",
        "orbistoun does not declare `sceNetBind` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x0` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/listener:sceNetListen:listening",
        "orbistoun does not declare `sceNetListen` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x0` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/nonblocking-option:sceNetSetsockopt:0x1200-accepted",
        "orbistoun does not declare `sceNetSetsockopt` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x0` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/recv-would-block:sceNetRecv:connected-return",
        "orbistoun does not declare `sceNetRecv` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x80410123` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/recv-would-block:sceNetRecv:listener-return",
        "orbistoun does not declare `sceNetRecv` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x80410139` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/sockaddr-bind:sceNetBind:sin_len-0-refused",
        "orbistoun does not declare `sceNetBind` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x80410130` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/sockaddr-bind:sceNetBind:sin_len-16-refused",
        "orbistoun does not declare `sceNetBind` at all, so there is no implementation for a claim to check. **The value is the point of recording it**: `0x80410130` is what the console answered, and whoever writes this function will otherwise invent a code (D617)",
    ),
    (
        "102-net/sockaddr-bind:sceNetBind:sin_len-16-bound",
        "the console binds a `sockaddr_in` whose `sin_len` says 16 and answers `0x0`. Orbistoun refuses both lengths, which is what `102-net/sockaddr-bind` reports as *bind refused both sockaddr lengths* in the differential. Done when this crate's bind accepts a 16-byte sockaddr and answers `0x0`",
    ),
    (
        "102-net/sockaddr-bind:sceNetBind:sin_len-0-bound",
        "and the same address bound with `sin_len` left at zero, also `0x0`: the field is not validated. The two in-tree sockaddr shapes reconcile because neither length is refused. Done alongside the entry above - one of them passing without the other would mean the length is being read",
    ),
    (
        "102-net/accept-inherits:sceNetSend:sent",
        "a send on an accepted socket returned `0xbf88` - a partial count, not a would-block - so an accepted socket does not inherit the listener's non-blocking flag in the way the count alone would suggest. Done when this crate's accept path is written far enough to have an answer of its own to compare",
    ),
    (
        "102-net/resolve:sceNetSocket:walk-null",
        "the kernel export-table walk answers null for `sceNetSocket` in the payload leg: libSceNet is not reachable that way at all, which is a finding rather than a failure to look (REQ-20260908T1400Z-0001). Nine of these, one per socket call, and they stand or fall together. Done when this project models which modules a leg can reach, which it does not today",
    ),
    (
        "102-net/resolve:sceNetBind:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetListen:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetAccept:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetRecv:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetSend:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetSocketClose:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetSetsockopt:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "102-net/resolve:sceNetConnect:walk-null",
        "as `sceNetSocket:walk-null` above: null from the export-table walk in the payload leg",
    ),
    (
        "090-audio/open-shapes:sceAudioOutOpen:rejected-rc",
        "`0x80260008` is what the console answers an audio-out open it will not accept - a real vendor code in libSceAudioOut's own space, which is worth having written down before anything here answers that call. Done when this project opens audio outputs and refuses a bad shape with this code rather than a placeholder",
    ),
    (
        "090-audio/volume-flag:sceAudioOutSetVolume:accepted",
        "no volume flag returned success on the console either - the value is `0x0` accepted flags. Done when the audio crate has a volume path with an answer of its own",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseInit:xotext",
        "**library text is execute-only on the console.** The flag is zero: the probe could call the symbol and could not read the bytes at it. Orbistoun maps its stub pages readable, so a guest that reads a prologue to work out a layout - which obSCEne itself does - sees something here and nothing there. Ten of these, four libraries. Done when stub pages are mapped execute-only and a read of one faults the way the console's does",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseOpen:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseClose:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/mouse-symbols:sceMouseRead:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/mouse-read:sceMouseRead:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardInit:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardOpen:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardClose:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/keyboard-symbols:sceKeyboardReadState:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/kbd-read:sceKeyboardReadState:xotext",
        "as `sceMouseInit:xotext` above: the text is execute-only on the console and readable here",
    ),
    (
        "101-input-ext/mouse-read:sceMouseRead:callable",
        "the console's `sceMouseRead` address is callable, which is the half of the pair `xotext` above does not say: execute-only means callable *and* unreadable, and a flag for only one of the two would read as the symbol being absent. Done alongside the execute-only mapping",
    ),
    (
        "101-input-ext/kbd-read:sceKeyboardReadState:callable",
        "as `sceMouseRead:callable` above: callable on the console, and the readable half is `xotext`",
    ),
    (
        "102-net/recv-would-block:errno:connected-errno",
        "**`35` - plain BSD `EAGAIN` - is what `__error()` holds after a would-block recv on a connected socket, while `sceNetRecv` reports `0x80410123` for the same condition.** Two spellings of one state, and `0x123` is not `35`, so libSceNet's space is not `0x8041_0000 | errno` the way libkernel's is `0x8002_0000 | errno`. Worth having written down before anything here answers either. Done when this crate has a recv that can be asked",
    ),
    (
        "102-net/recv-would-block:errno:listener-errno",
        "`57`, `ENOTCONN`, from a recv on a *listener* - the red herring the resolved REQ-20260908T1400Z-0001 warns about, since it looks like a would-block answer and is not. Done alongside the connected case",
    ),
    (
        "102-net/recv-would-block:recv:connected-return",
        "`-1`, with the errno above carrying the reason: the POSIX layer reports the classic pair rather than a vendor code. Done alongside the errno entries",
    ),
    (
        "102-net/recv-would-block:recv:listener-return",
        "`-1` from a recv on a listener, paired with `ENOTCONN`. Done alongside the errno entries",
    ),
    (
        "102-net/nonblocking-option:_setsockopt:0x1200-accepted",
        "`0x0`: `SO_NBIO` is `0x1200`, accepted. Done when this crate sets non-blocking on a socket and accepts that option value",
    ),
    (
        "102-net/nonblocking-option:fcntl:O_NONBLOCK-return",
        "`-1`: `fcntl` `F_SETFL` **fails** where the `0x1200` socket option succeeds, so the two routes to non-blocking are not equivalent on this platform. That asymmetry is the finding; done when this crate refuses the same one",
    ),
    (
        "102-net/sockaddr-bind:bind:sin_len-16-bound",
        "`0x0`: the POSIX `bind` accepts a `sockaddr_in` whose `sin_len` says 16. Done when this crate's bind accepts it",
    ),
    (
        "102-net/sockaddr-bind:bind:sin_len-0-bound",
        "`0x0`: and the same address with `sin_len` left at zero, so the field is not validated. Done alongside the entry above - one passing without the other would mean the length is being read",
    ),
    (
        "102-net/listener:bind:bound",
        "`0x0`: a listener binds. Done when this crate opens and binds a socket",
    ),
    (
        "102-net/listener:listen:listening",
        "`0x0`: and listens. Done alongside the bind entry",
    ),
    (
        "102-net/listener:SYS_socket:descriptor",
        "`0xc` - the descriptor a raw `SYS_socket` returned. A descriptor number is not a platform constant in the way a return code is, but it is what the payload leg got, and the same `0xc` comes back through three routes below, which is what says they are one socket. Done when this crate hands out descriptors of its own to compare",
    ),
    (
        "102-net/listener:__sys_socketex:named",
        "`0xc` again, through `__sys_socketex` with a name argument: the same descriptor as the raw syscall, so the named form is the same call. Done alongside the descriptor entry",
    ),
    (
        "102-net/listener:__sys_socketex:null-name",
        "`0xc` again with a null name, so the name argument is accepted either way. Done alongside the descriptor entry",
    ),
    (
        "102-net/resolve:SYS_socket:syscall-fd",
        "`0xc`: the payload reaches a socket **by raw syscall** where libSceNet resolves by no route at all (REQ-20260908T1400Z-0001). That is the route this leg actually has, and it is the one orbistoun does not offer a payload today (D626). Done when a payload under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/accept-inherits:send:total-sent",
        "`0xbf88` bytes went out before the socket saturated. Done when this crate has a send with a buffer of its own",
    ),
    (
        "102-net/accept-inherits:send:sends-until-saturated",
        "`2`: two sends filled it, which is what makes the byte count above a buffer size rather than a coincidence",
    ),
    (
        "102-net/accept-inherits:send:would-block-code",
        "`-1` from the POSIX send once saturated, with the errno below",
    ),
    (
        "102-net/accept-inherits:errno:saturation-errno",
        "`35`, `EAGAIN`, the same POSIX spelling the connected recv gives - so saturation and no-data-yet are one errno, and only the direction distinguishes them",
    ),
    (
        "102-net/accept-inherits:sceNetSend:would-block-code",
        "`0x80410123` - the vendor spelling of the `35` above, through `sceNetSend` rather than `send`. The pair is the evidence that the two layers report one condition two ways",
    ),
    (
        "101-input-ext/mouse-read:sceMouseRead:rc",
        "`0x0`: the console's `sceMouseRead` succeeds. Orbistoun answers a placeholder - it is the third most-called unimplemented import in the payload run, 63 calls. Done when the input crate implements it and answers `0x0` for a valid handle",
    ),
    (
        "101-input-ext/kbd-read:sceKeyboardReadState:rc",
        "`0x0`: the console's `sceKeyboardReadState` succeeds, and it is the second most-called unimplemented import in the payload run at 82 calls. Done when the input crate implements it and answers `0x0` for a valid handle",
    ),
    (
        "102-net/socket-bind:sceNetBind:bound",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-listen:sceNetListen:listening",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/posix-symbols:__sys_socketex:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:bind:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:listen:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:accept:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:recv:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:_sendto:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:close:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:_setsockopt:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:connect:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:fcntl:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/posix-symbols:__error:callable",
        "the POSIX socket layer's symbol census, answered by the console for another agent's request. Orbistoun declares libSceNet and implements none of it (D617, D627). Done when the net crate has a socket path and this census can be compared",
    ),
    (
        "102-net/socket-syscall:SYS_socket:syscall-fd",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-create:SYS_socket:descriptor",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-syscall-97:bind-port-9899:bound",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-syscall-97:listen-port-9899:listening",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-create:__sys_socketex:named",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-create:__sys_socketex:null-name",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-bind:bind:bound",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/socket-listen:listen:listening",
        "the console obtains a socket by raw syscall and binds and listens on it - the route the payload actually has, and the one orbistoun does not offer (D626). Done when a guest under orbistoun can obtain a descriptor the same way",
    ),
    (
        "102-net/posix-fcntl:fcntl:F_SETFL",
        "measured on the console and nothing here implements the call it describes. Done when the declaring crate has an implementation to compare against",
    ),
    (
        "102-net/posix-would-block:__error:errno",
        "measured on the console and nothing here implements the call it describes. Done when the declaring crate has an implementation to compare against",
    ),
    (
        "102-net/posix-accept:fcntl:F_GETFL",
        "measured on the console and nothing here implements the call it describes. Done when the declaring crate has an implementation to compare against",
    ),
    (
        "107-videodec/symbols:sceVideodec2QueryComputeMemoryInfo:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2AllocateComputeQueue:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2ReleaseComputeQueue:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2CreateDecoder:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2DeleteDecoder:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2Decode:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2Flush:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2Reset:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "107-videodec/symbols:sceVideodec2GetPictureInfo:unresolved",
        "the video-decoder entry points the console reports unresolved even on hardware. Recorded so that a future run showing them resolved is visibly a change rather than a surprise; orbistoun declares libSceVideodec2 and implements none of it",
    ),
    (
        "108-audiodec/ajm:sceAjmInitialize:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmFinalize:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmModuleRegister:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmInstanceCreate:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmInstanceDestroy:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmBatchStartBuffer:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "108-audiodec/ajm:sceAjmBatchWait:unresolved",
        "the AJM audio-decoder census on the console. Orbistoun declares libSceAjm and implements none of it, so there is nothing here to disagree with yet. Done when the audio crate has a decode path",
    ),
    (
        "166-agc/create-shader:sceAgcCreateShader:rc-wellformed",
        "**the wall, measured at last.** sceAgcCreateShader answers 0x8a6c002f and writes nothing, for a well-formed header, for the 0xd8 and 0x118 payload shapes, and for PPSA03416's own 0x108 shape supplied by this project - so the header is not the variable. A null argument answers 0xb. Orbistoun answers its Unimplemented placeholder; forcing the measured code instead was tried and moved nothing, so the out-parameter is what the guest acts on (D621, D641). Done when something here fills that structure",
    ),
    (
        "166-agc/create-shader:sceAgcCreateShader:rc-payload-118",
        "**the wall, measured at last.** sceAgcCreateShader answers 0x8a6c002f and writes nothing, for a well-formed header, for the 0xd8 and 0x118 payload shapes, and for PPSA03416's own 0x108 shape supplied by this project - so the header is not the variable. A null argument answers 0xb. Orbistoun answers its Unimplemented placeholder; forcing the measured code instead was tried and moved nothing, so the out-parameter is what the guest acts on (D621, D641). Done when something here fills that structure",
    ),
    (
        "166-agc/create-shader:sceAgcCreateShader:rc-ppsa03416",
        "**the wall, measured at last.** sceAgcCreateShader answers 0x8a6c002f and writes nothing, for a well-formed header, for the 0xd8 and 0x118 payload shapes, and for PPSA03416's own 0x108 shape supplied by this project - so the header is not the variable. A null argument answers 0xb. Orbistoun answers its Unimplemented placeholder; forcing the measured code instead was tried and moved nothing, so the out-parameter is what the guest acts on (D621, D641). Done when something here fills that structure",
    ),
    (
        "166-agc/create-shader:sceAgcCreateShader:rc-null",
        "**the wall, measured at last.** sceAgcCreateShader answers 0x8a6c002f and writes nothing, for a well-formed header, for the 0xd8 and 0x118 payload shapes, and for PPSA03416's own 0x108 shape supplied by this project - so the header is not the variable. A null argument answers 0xb. Orbistoun answers its Unimplemented placeholder; forcing the measured code instead was tried and moved nothing, so the out-parameter is what the guest acts on (D621, D641). Done when something here fills that structure",
    ),
    (
        "100-input/sysmodule-callable:sceSysmoduleLoadModule:callable",
        "whether the input sysmodule loads and is callable on the console. Orbistoun refuses every firmware module path with ENOENT, which is measured and correct, so this cannot match until sysmodule loading means something here. Done alongside the input read functions",
    ),
    (
        "100-input/sysmodule-load:libScePad:module_id",
        "whether the input sysmodule loads and is callable on the console. Orbistoun refuses every firmware module path with ENOENT, which is measured and correct, so this cannot match until sysmodule loading means something here. Done alongside the input read functions",
    ),
    (
        "100-input/sysmodule-load:sceSysmoduleLoadModule:rc",
        "whether the input sysmodule loads and is callable on the console. Orbistoun refuses every firmware module path with ENOENT, which is measured and correct, so this cannot match until sysmodule loading means something here. Done alongside the input read functions",
    ),
    (
        "100-input/resolve:scePadInit:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadInit:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadInit:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadOpen:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadOpen:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadOpen:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadClose:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadClose:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadClose:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadReadState:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadReadState:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadReadState:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceAddDevice:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceAddDevice:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceAddDevice:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceDeleteDevice:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceDeleteDevice:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceDeleteDevice:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceInsertData:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceInsertData:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadVirtualDeviceInsertData:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadSetParticularMode:dlsym",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadSetParticularMode:kexport",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "100-input/resolve:scePadSetParticularMode:dynlib",
        "the input SDK's symbol census: whether each pad, keyboard and mouse entry point resolves on the console. Orbistoun declares libScePad and libSceKeyboard and implements neither read path, so nothing here has an answer of its own. Done when the input crate implements the read functions and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/sysmodule-callable:sceSysmoduleLoadModule:callable",
        "whether the encoder sysmodule loads and is callable on the console. Orbistoun refuses firmware module paths, so this cannot match yet. Done when sysmodule loading is modelled",
    ),
    (
        "106-encoder/sysmodules:sceSysmoduleUnloadModule:table-vaddr",
        "whether the encoder sysmodule loads and is callable on the console. Orbistoun refuses firmware module paths, so this cannot match yet. Done when sysmodule loading is modelled",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-1",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-2",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-3",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-4",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-5",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-6",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-7",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "080-video/visual-flip:sceVideoOutSubmitFlip:rc-8",
        "flip pacing measured by return codes and flip status, answered for another agent's request. Orbistoun's video-out accepts flips and does not pace them, so it has no timing of its own to compare. Done when the video crate paces submission",
    ),
    (
        "100-input/batched-read:scePadRead:returned",
        "how far a pad read writes, and the stride of a batched one, with no controller attached. Orbistoun implements no pad read (scePadReadState is the most-called unimplemented import in the corpus), so there is nothing here to disagree with. Done when the input crate implements it and writes the measured extent",
    ),
    (
        "100-input/button-bits:scePadReadState:press-create-ps-touchpad-mic-now",
        "the pad record's stick range and button bit layout, measured on the console. The layout is what an implementation must fill in; recorded before anything here fills anything. Done alongside the pad read",
    ),
    (
        "100-input/stick-trigger-range:scePadReadState:sweep-sticks-and-pull-triggers-now",
        "the pad record's stick range and button bit layout, measured on the console. The layout is what an implementation must fill in; recorded before anything here fills anything. Done alongside the pad read",
    ),
    (
        "106-encoder/resolve:sceVencCoreQueryMemorySize:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreQueryMemorySize:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreQueryMemorySize:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreCreateEncoder:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreCreateEncoder:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreCreateEncoder:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreGetAuData:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreGetAuData:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreGetAuData:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreSetInputFrame:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreSetInputFrame:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreSetInputFrame:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStartSequence:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStartSequence:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStartSequence:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStopSequence:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStopSequence:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreStopSequence:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreDeleteEncoder:dlsym",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreDeleteEncoder:kexport",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "106-encoder/resolve:sceVencCoreDeleteEncoder:dynlib",
        "the video-encoder symbol census on the console, answered for another agent's request. Orbistoun declares no encoder library and implements none of it. Done when an encoder crate exists and this census can be compared rather than only recorded",
    ),
    (
        "100-input/read-extent:scePadReadState:extent",
        "how far a pad read writes, and the stride of a batched one, with no controller attached. Orbistoun implements no pad read (scePadReadState is the most-called unimplemented import in the corpus), so there is nothing here to disagree with. Done when the input crate implements it and writes the measured extent",
    ),
    (
        "100-input/read-extent:scePadReadState:rc",
        "how far a pad read writes, and the stride of a batched one, with no controller attached. Orbistoun implements no pad read (scePadReadState is the most-called unimplemented import in the corpus), so there is nothing here to disagree with. Done when the input crate implements it and writes the measured extent",
    ),
    (
        "100-input/batched-read:scePadRead:extent",
        "how far a pad read writes, and the stride of a batched one, with no controller attached. Orbistoun implements no pad read (scePadReadState is the most-called unimplemented import in the corpus), so there is nothing here to disagree with. Done when the input crate implements it and writes the measured extent",
    ),
];

/// Calls an implementation by the name a guest would import it under.
///
/// Guest arguments are plain words and guest memory is the host's, so a test hands over the address
/// of its own local and the implementation writes through it, as it would for a guest.
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

/// Every constant measurement is claimed by a test or declared outstanding.
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

    // The gate's printed counts are asserted, since a count is a claim like any other.
    assert_eq!(
        CLAIMED.len() + OUTSTANDING.len() + OPAQUE.len(),
        table.constants().count(),
        "every constant is accounted for exactly once, so the four counts must agree"
    );
}

/// What the hardware reported, without the check that took the reading: subject, condition,
/// observation. Two checks producing one of these produced one fact.
type Reading = (String, String, String);

/// One measured fact is not filed in two different lists.
///
/// Where two checks measured the same subject, condition and value, both land in the same list:
/// [`CLAIMED`] is asserted, [`OUTSTANDING`] is a queue, and [`OPAQUE`] never moves up, so splitting
/// one fact across two means one is wrong. It checks only that duplicates agree, not that a single
/// classification is right.
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

    // Keyed on what the hardware reported, not the id, which carries the check that took the
    // reading.
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

/// Nothing claims or defers a measurement that is not there, or is not constant: a stale id reads
/// as coverage while asserting nothing, and a varying measurement is never asserted.
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

/// The counter frequency is one the hardware reported, and the two names agree.
///
/// The frequency is calibrated per boot (runs measured `0x5f259b8e` and `0x5f259bb6`, stable within
/// a session), so the measurement is not constant. Checked instead: orbistoun answers a frequency
/// some run reported, and the timestamp counter and the process-time counter agree, as they did in
/// every run.
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

/// The character-classification tables carry what the hardware's C library carries.
///
/// Read as a guest reads them: `_Getpctype()` answers the address of the entry for index zero and
/// the caller indexes from there, so this checks installation and the margin below zero as well as
/// the values.
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
            // `mask_tab_9` and `entry_eof_neg1`: the character is the last field, and a `neg`
            // prefix is the minus sign the id cannot carry.
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
            // SAFETY: `base` is the address the implementation answered for the entry at index
            // zero, and the measured tables span -8 to 263, so every index checked lands inside the
            // implementation's allocation.
            let at = unsafe {
                std::ptr::with_exposed_provenance::<u16>(
                    usize::try_from(base).expect("a guest address fits the host"),
                )
                .offset(isize::try_from(index).expect("a table index is small"))
            };
            // SAFETY: `at` is inside the same allocation, and the table holds `u16` entries, so it
            // is aligned and initialised.
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

/// Releasing a lock nobody holds answers the code the hardware answered.
///
/// Compared at 32 bits, the width the measurement was taken at: the record's `0xffffffff80020001`
/// is the probe widening a C `int`.
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

/// Taking a mutex twice answers what the hardware answered, for every type it was asked about.
///
/// The hardware was swept across five attribute type values for what a second `Trylock` says,
/// giving the mapping orbistoun uses: `2` is recursive, `4` is error-checking, anything else is a
/// plain lock. Compared at 32 bits, since the probe widened a C `int`.
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

/// The memory query accepts exactly the flag values the hardware accepted.
///
/// The probe calls `sceKernelDirectMemoryQuery(0, flag, buffer, 256)` and allocates nothing; 0 and
/// 1 are accepted, 2 and 4 refused (D398). Compared at 32 bits.
#[test]
fn the_memory_query_accepts_the_flags_the_console_accepted() {
    for flag in [0_u64, 1, 2, 4] {
        let id =
            format!("130-layout/direct-memory-query-flags:sceKernelDirectMemoryQuery:flags-{flag}");
        // Every code any run answered: whether a query with no allocation finds something depends
        // on the machine's state, not the flag, so the claim is membership of what was seen.
        let seen: Vec<u32> = measurement(&id)
            .values()
            .into_iter()
            .map(|value| value as u32)
            .collect();
        assert!(!seen.is_empty(), "flag {flag}: no code was ever measured");

        // The buffer size the probe declared, so the conditions match.
        let mut info = [0_u8; 256];
        let answered = call(
            "sceKernelDirectMemoryQuery",
            [0, flag, info.as_mut_ptr() as u64, info.len() as u64, 0, 0],
        );
        assert!(
            seen.contains(&(answered as u32)),
            "flag {flag}: orbistoun answered {:#x}, which no console run reported - they said {seen:#x?}",
            answered as u32
        );
    }
}

/// Loading a module answers what the hardware answered, for the paths whose answer is fixed.
///
/// The probe's exact paths and call form, `(path, 0, 0, 0, 0, &started)`. libkernel is resident and
/// answers its well-known handle; a `/system` module is the platform's own copy and is refused with
/// the not-found errno. Both are properties of the call, unlike the `/app0` handles in [`OPAQUE`].
/// Compared at 32 bits.
#[test]
fn loading_a_module_answers_the_measured_code() {
    // The paths are obSCEne's own, in the order its `obs_module_quantity` table names them.
    //
    // For the `/system/` entries this pins the answer and not the route: a misspelled path falls
    // through to the same `0x8002_0002`. The `libkernel` entries answer `0x2001`, so a wrong path
    // there fails.
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

/// The third field of the direct-memory query structure is the memory type.
///
/// obSCEne allocated one 16 KiB page each of `WB_ONION` (0), `WC_GARLIC` (3) and `WB_GARLIC` (10)
/// and read back `0x0`, `0x3` and `0xa`: the type asked for, not a state (D398). This takes the
/// same path, allocate then query, so it does not pass on the model alone.
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

/// A mutex attribute round-trips the types the hardware round-trips, and refuses the one it
/// refuses.
///
/// `Settype` then `Gettype` on one attribute object for each of 0..4: the hardware refused 0 and
/// round-tripped 1 to 4, with a default of 1. `type-0-read-back` reads all ones, which is the
/// probe's marker for a refusal, so this asserts that the round trip after `Settype(0)` does not
/// succeed rather than asserting the marker. The refusal code itself is not measured.
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
    // The address is taken once, so a closure below does not hold a borrow of `attr`.
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

    // Zero: refused, so the round trip must not come back with zero.
    set(0);
    assert_eq!(get(&mut read), 0, "the attribute is still readable");
    assert_ne!(
        u64::from(read),
        0,
        "type 0 must not round-trip - the console refuses it"
    );

    // One through four: each reads back as itself.
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

/// The hardware writes eight bytes where it was given four, and so does orbistoun (D272).
///
/// obSCEne plants `0xA5A5A5A5` in the word after an `int handle`, calls `sceKernelCreateSema` on
/// the `int`, and reads the guard back as `0x0`. A measured layout outranks a published one, and a
/// guest is built against the hardware. The guard is asserted, not the handle, which is orbistoun's
/// own number.
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

/// The two encoder system modules the hardware loads, orbistoun answers the same way.
///
/// obSCEne asked for nine and was answered `0` for `VENC` and `VIDEOREC` and `0x805a1000` for the
/// other seven. Only the two successes are claimed: orbistoun answers `0` to every identifier
/// (D428), and the refusals may depend on the capture's application category rather than the
/// modules.
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
    // Asserted rather than counted on: a loop that compared fewer would pass.
    assert_eq!(compared, 2, "both measured successes are compared");
}

/// Every platform module directory is refused, including the one not under `/system`.
///
/// The platform keeps its modules in three directories, and `/system_ex/common_ex/lib/` is not
/// under `/system/`. An unmatched path falls through to the unrecognised-path refusal, which
/// answers the same `0x8002_0002` as a missing path, so this pins the answer and not the rule; the
/// two diverge once `/app0` loads and a platform path must not. The two `/system` paths are
/// measured (`110-modules/load`); `/system_ex` applies the same rule unmeasured.
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

/// A title's own module gets a distinct, non-negative handle: two loads never share one, and
/// neither looks like a failure.
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

/// The sysctl knobs orbistoun can source answer the widths the hardware answered.
///
/// The width is the claim, not the value: a caller reading four bytes of an eight-byte answer reads
/// a different number (D272).
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

/// A guest starts in the float environment the hardware starts a title in (D486).
///
/// Of the measured `MXCSR`, four fields are configuration (flush to zero, denormals are zero, round
/// to nearest, all six exceptions masked) and are asserted here. The low six bits are sticky
/// status, which orbistoun leaves clear so a guest does not see an inexact result it never
/// produced. Read back from the register, so a skipped install fails.
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

    // The status bits are clear: installing the raw measurement verbatim would pass everything
    // above and fail here.
    assert_eq!(
        live & 0x3f,
        0,
        "a status flag was installed as though it were configuration"
    );
}

/// The hardware's raw `MXCSR` is orbistoun's, once the sticky flags are taken off.
///
/// Bits 0-5 of `MXCSR` are the exception flags, sticky status, and every other bit is
/// configuration, so `0x9fe0 & !0x3f == 0x9fc0 == GUEST_MXCSR`. The equality is asserted rather
/// than the constant (D486).
#[test]
fn the_consoles_float_configuration_is_the_raw_value_without_its_status_flags() {
    /// Bits 0-5: the six exception flags IE, DE, ZE, OE, UE and PE (Intel SDM Vol. 1, `MXCSR`).
    /// Sticky: set by arithmetic and cleared only by writing the register, so they are status, not
    /// configuration.
    const STATUS_FLAGS: u64 = 0x3f;

    let raw = measurement("035-libc/fpu-environment:mxcsr:raw");
    let seen = raw.values();
    assert!(
        !seen.is_empty(),
        "the raw MXCSR must have been measured at least once"
    );

    orbistoun_abi::enter::adopt_guest_float_environment();
    let live = u64::from(orbistoun_abi::enter::float_environment());

    // Every value any run reported: the measurement is not constant, so the claim is membership.
    for value in &seen {
        assert_eq!(
            value & !STATUS_FLAGS,
            live,
            concat!(
                "the console reported {:#x}; without status flags that is {:#x} ",
                "and orbistoun installs {:#x}"
            ),
            value,
            value & !STATUS_FLAGS,
            live
        );
    }

    // Every bit the runs differ in is status; masking alone would hide a configuration bit
    // orbistoun set and the hardware clears.
    let differing = seen.iter().fold(0, |acc, value| acc | (value ^ seen[0]));
    assert_eq!(
        differing & !STATUS_FLAGS,
        0,
        "the runs disagree outside the status bits, so the configuration is not constant"
    );
}

/// Every platform module path the encoder probe tried is refused, with the code the hardware gave.
///
/// Twenty-four paths across `/system/common/lib`, `/system/priv/lib`, `/system/sys/lib` and
/// `/system/lib`, each answered `0x80020002`, the vendor encoding of `ENOENT`, compared at 32 bits.
/// The first two directories are refused as known platform module directories; the other two reach
/// the same code by falling through to the unrecognised-path refusal. The guest sees the same
/// answer either way.
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

/// Polling a semaphore honours the count asked for, at every boundary the hardware measured.
///
/// With `need-2-of-1-left` the hardware answers busy; taking the one that is there instead would
/// let a caller release two it never held. Setup is the probe's: a semaphore created with the named
/// initial count, then one poll.
#[test]
fn polling_a_semaphore_answers_the_measured_code_for_each_count() {
    for (condition, initial, need) in [
        ("need-0-of-empty", 0_u64, 0_u64),
        ("need-1-of-1-left", 1, 1),
        ("need-2-of-1-left", 1, 2),
        ("need-2-of-3", 3, 2),
    ] {
        let expected = measurement(&format!(
            "016-syncbounds/sema-count:sceKernelPollSema:{condition}"
        ))
        .value()
        .expect("a code is a number") as u32;

        // A word, not an `int`: `sceKernelCreateEventFlag` writes eight bytes through this pointer.
        let mut handle = 0_u64;
        let name = std::ffi::CString::new(format!("orbistoun-{condition}")).expect("text");
        assert_eq!(
            call(
                "sceKernelCreateSema",
                [
                    std::ptr::from_mut(&mut handle) as u64,
                    name.as_ptr() as u64,
                    0,
                    initial,
                    // A ceiling above every initial count used, so the create never fails for an
                    // unrelated reason.
                    8,
                    0,
                ],
            ),
            0,
            "{condition}: the semaphore must exist before it can be polled"
        );

        let answered = call("sceKernelPollSema", [handle, need, 0, 0, 0, 0]);
        assert_eq!(
            answered as u32, expected,
            "{condition}: asking for {need} of {initial}, the console answered {expected:#x}"
        );
        call("sceKernelDeleteSema", [handle, 0, 0, 0, 0, 0]);
    }
}

/// A wait mode naming neither `and` nor `or` is an argument error.
///
/// `0x02` is `or`, but `0x00` is invalid: the hardware answers `0x80020016` for it. Setup is the
/// probe's: a two-bit pattern with one bit set, polled under four modes.
#[test]
fn an_event_flag_answers_the_measured_code_for_each_wait_mode() {
    /// The two bits the pattern names.
    const PATTERN: u64 = 0b11;
    /// The one of them the flag is created holding.
    const PRESENT: u64 = 0b01;

    for mode in [0x00_u64, 0x01, 0x02, 0x11] {
        let expected = measurement(&format!(
            "016-syncbounds/event-flag-waitmode:sceKernelPollEventFlag:mode-{mode:#04x}-both-of-one"
        ))
        .value()
        .expect("a code is a number") as u32;

        let mut handle = 0_u64;
        let name = std::ffi::CString::new(format!("orbistoun-mode-{mode:#04x}")).expect("text");
        assert_eq!(
            call(
                "sceKernelCreateEventFlag",
                [
                    std::ptr::from_mut(&mut handle) as u64,
                    name.as_ptr() as u64,
                    0,
                    PRESENT,
                    0,
                    0,
                ],
            ),
            0,
            "mode {mode:#04x}: the flag must exist before it can be polled"
        );

        let answered = call("sceKernelPollEventFlag", [handle, PATTERN, mode, 0, 0, 0]);
        assert_eq!(
            answered as u32, expected,
            "mode {mode:#04x}: one bit of two set, the console answered {expected:#x}"
        );
        call("sceKernelDeleteEventFlag", [handle, 0, 0, 0, 0, 0]);
    }
}

/// Waking an address nobody is waiting on succeeds.
///
/// The hardware answers `0`: the count of waiters released is not the return value, and "none were
/// waiting" is not an error.
#[test]
fn waking_an_address_with_no_waiter_succeeds() {
    let expected =
        measurement("032-syncaddr/wake-with-no-waiter:sceKernelSyncOnAddressWake:returned")
            .value()
            .expect("a code is a number") as u32;

    // An address in this test's own frame that nothing has waited on; guest memory is the host's.
    let mut nobody_waits_here = 0_u32;
    let answered = call(
        "sceKernelSyncOnAddressWake",
        [
            std::ptr::from_mut(&mut nobody_waits_here) as u64,
            1,
            0,
            0,
            0,
            0,
        ],
    );
    assert_eq!(
        answered as u32, expected,
        "the console answered {expected:#x} to a wake with nothing waiting"
    );

    // The same code where a waiter was released: three conditions record one fact, that this call
    // answers `0`. Whether a waiter woke is a second fact these measurements do not carry.
    for also in [
        "032-syncaddr/wake-releases-a-waiter:sceKernelSyncOnAddressWake:returned",
        "032-syncaddr/wake-releases-a-waiter:sceKernelSyncOnAddressWake:retry-all",
    ] {
        assert_eq!(
            measurement(also).value().expect("a code is a number") as u32,
            expected,
            "{also}: the same call answering the same code, so one claim covers it"
        );
    }
}

/// The system software version query answers, and the enumeration of users does too.
///
/// Four return codes from three calls, each `0` on the hardware: orbistoun answers rather than
/// refusing, which is what a guest branches on first.
#[test]
fn the_queries_that_answer_zero_on_the_console_answer_zero_here() {
    // Room well past what any of these calls writes, so an overrun lands in the buffer rather than
    // the stack.
    let mut out = [0_u8; 512];

    for (id, symbol) in [
        (
            "130-layout/net-interfaces:getifaddrs:return_code",
            "getifaddrs",
        ),
        (
            "130-layout/user-service:sceUserServiceGetInitialUser:return_code",
            "sceUserServiceGetInitialUser",
        ),
    ] {
        let expected = measurement(id).value().expect("a code is a number") as u32;
        out.fill(0);
        let answered = call(symbol, [out.as_mut_ptr() as u64, 0, 0, 0, 0, 0]);
        assert_eq!(
            answered as u32, expected,
            "{symbol}: the console answered {expected:#x}"
        );
    }
}

/// A freshly initialised thread attribute names no stack and a default size: an address before one
/// was set would hand out someone else's stack, and a zero size would fail every `scePthreadCreate`
/// from a default attribute.
#[test]
fn a_fresh_thread_attribute_names_no_stack_and_the_measured_default_size() {
    let expected_address = measurement(
        "031-stackattr/fresh-attr-names-no-stack:scePthreadAttrGetstackaddr:stack-address",
    )
    .value()
    .expect("an address is a number");
    let expected_size = measurement(
        "031-stackattr/fresh-attr-names-no-stack:scePthreadAttrGetstacksize:stack-size",
    )
    .value()
    .expect("a size is a number");

    // The storage, not the handle: every call in this family takes a `ScePthreadAttr *` and reads
    // the handle out of it.
    let mut attr = 0_u64;
    let attr_at = std::ptr::from_mut(&mut attr) as u64;
    assert_eq!(
        call("scePthreadAttrInit", [attr_at, 0, 0, 0, 0, 0]),
        0,
        "the attribute must initialise before it can be read"
    );

    let mut address = u64::MAX;
    assert_eq!(
        call(
            "scePthreadAttrGetstackaddr",
            [attr_at, std::ptr::from_mut(&mut address) as u64, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        address, expected_address,
        "a fresh attribute names no stack, and the console answers {expected_address:#x}"
    );

    let mut size = u64::MAX;
    assert_eq!(
        call(
            "scePthreadAttrGetstacksize",
            [attr_at, std::ptr::from_mut(&mut size) as u64, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        size, expected_size,
        "the console's default stack size is {expected_size:#x}"
    );

    call("scePthreadAttrDestroy", [attr_at, 0, 0, 0, 0, 0]);
}

/// `sceKernelIsStack` answers zero and reports the span through its out-parameters.
///
/// The call takes three arguments, returns a status, and outputs the bounds. The return code is
/// asserted; the hardware's bounds are that machine's addresses and are opaque, and orbistoun's
/// span is asserted in `orbistoun-kernel`'s own tests.
#[test]
fn asking_where_the_stack_is_answers_the_measured_status() {
    let expected = measurement("031-stackattr/address-is-the-base:sceKernelIsStack:is-stack")
        .value()
        .expect("a code is a number") as u32;

    let mut low = 0_u64;
    let mut high = 0_u64;
    let local = 0_u64;
    let answered = call(
        "sceKernelIsStack",
        [
            std::ptr::from_ref(&local) as u64,
            std::ptr::from_mut(&mut low) as u64,
            std::ptr::from_mut(&mut high) as u64,
            0,
            0,
            0,
        ],
    );
    assert_eq!(
        answered as u32, expected,
        "the console answered {expected:#x} for an address in its own stack"
    );
}
