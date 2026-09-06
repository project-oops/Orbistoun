//! The current generation's graphics API - libSceAgc and libSceAgcDriver.
//!
//! # Why this exists as names and nothing else
//!
//! This crate declared `libSceGnmDriver` - the **previous** generation's interface - and
//! nothing at all for the one titles on this generation actually call. A survey of what
//! guests ask for found six `Agc` functions being called with no declaration anywhere in
//! the project, so they were reported as `unknown::` and could not be counted, named or
//! stubbed (D500).
//!
//! # Where these names come from
//!
//! **A real module's own import table.** PPSA02664's executable imports fifty-one names
//! from `libSceAgc` and five from `libSceAgcDriver`; five further `libSceAgcDriver` names
//! appear in the recorded corpus from other modules. Every name below is one of those.
//! None is derived from a pattern and hoped to match - which is the same provenance
//! `orbistoun-input` documents for `libScePad`, and the strongest available without a
//! console.
//!
//! # Status: names confirmed, arities not
//!
//! The asymmetry is the one `orbistoun-input` states and it holds for the same reason: a
//! wrong arity degrades a call trace and cannot break a call - it is carried only into
//! reports and trace shape, never into the call path - while a wrong *name* is a NID that
//! matches no import and a shim nothing can reach.
//!
//! **Every arity here is 6, and that is not a claim that these take six arguments.** Six is
//! the trampoline's full capture. With nothing established, recording every argument
//! register loses no information, where guessing low silently discards the arguments - and
//! for a command-buffer interface the arguments are buffer addresses and sizes, which is
//! precisely what a trace of it is for.
//!
//! Nothing here is implemented. That is deliberate: `docs/ROADMAP.md` puts the first frame
//! at Phase 6, which has not begun, and principle 6 puts a subsystem after the address
//! space and threads. What this buys now is that a guest reaching the graphics interface is
//! **named and counted** instead of vanishing into `unknown::`, and that the loud stub
//! policy answers it rather than a placeholder a caller might read as a handle (D125).

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAgc" {
        "sceAgcAcbAcquireMem" => 6,
        "sceAgcAcbDispatchIndirect" => 6,
        "sceAgcAcbDmaData" => 6,
        "sceAgcAcbEventWrite" => 6,
        "sceAgcAcbResetQueue" => 6,
        "sceAgcAcbWaitRegMem" => 6,
        "sceAgcAcbWriteData" => 6,
        "sceAgcCbDispatch" => 6,
        "sceAgcCbNop" => 6,
        "sceAgcCbReleaseMem" => 6,
        "sceAgcCbSetShRegisterRangeDirect" => 6,
        "sceAgcCbSetShRegistersDirect" => 6,
        "sceAgcCreatePrimState" => 6,
        // **Four, not six: the knowledge base already recorded an arity for this one** and
        // the declaration defers to it. `declared_arity_and_recorded_arity_never_disagree`
        // is the guard that said so, and it was right to - the record is the older claim.
        "sceAgcCreateShader" => 4,
        "sceAgcDcbAcquireMem" => 6,
        "sceAgcDcbDispatchIndirect" => 6,
        "sceAgcDcbDmaData" => 6,
        "sceAgcDcbDrawIndex" => 6,
        "sceAgcDcbDrawIndexAuto" => 6,
        "sceAgcDcbDrawIndexIndirect" => 6,
        "sceAgcDcbDrawIndirect" => 6,
        "sceAgcDcbEventWrite" => 6,
        "sceAgcDcbPopMarker" => 6,
        "sceAgcDcbPushMarker" => 6,
        "sceAgcDcbResetQueue" => 6,
        "sceAgcDcbSetBaseIndirectArgs" => 6,
        "sceAgcDcbSetCxRegistersIndirect" => 6,
        "sceAgcDcbSetFlip" => 6,
        "sceAgcDcbSetIndexBuffer" => 6,
        "sceAgcDcbSetIndexCount" => 6,
        "sceAgcDcbSetIndexSize" => 6,
        "sceAgcDcbSetNumInstances" => 6,
        "sceAgcDcbSetShRegistersIndirect" => 6,
        "sceAgcDcbSetUcRegistersIndirect" => 6,
        "sceAgcDcbStallCommandBufferParser" => 6,
        "sceAgcDcbWaitRegMem" => 6,
        "sceAgcDcbWaitUntilSafeForRendering" => 6,
        "sceAgcDcbWriteData" => 6,
        "sceAgcDmaDataPatchSetDstAddressOrOffset" => 6,
        "sceAgcDmaDataPatchSetSrcAddressOrOffsetOrImmediate" => 6,
        "sceAgcGetRegisterDefaults2" => 6,
        "sceAgcGetRegisterDefaults2Internal" => 6,
        "sceAgcQueueEndOfPipeActionPatchAddress" => 6,
        "sceAgcSetCxRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetCxRegIndirectPatchSetAddress" => 6,
        "sceAgcSetShRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetShRegIndirectPatchSetAddress" => 6,
        "sceAgcSetUcRegIndirectPatchAddRegisters" => 6,
        "sceAgcSetUcRegIndirectPatchSetAddress" => 6,
        "sceAgcSuspendPoint" => 6,
        "sceAgcWaitRegMemPatchAddress" => 6,
    }
}
