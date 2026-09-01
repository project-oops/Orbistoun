# 2026-08-31 (later) - C succeeded: the full current obSCEne runs as the payload


The payload shape (obscene.elf, the 9.3MB artifact the corpus asset actually names, and the exact
bytes hardware's payload run streamed) runs the *current* obSCEne in orbistoun: meta 1|38|543, 189
imports, 29/38 sections, 158 unique tests captured through the now-implemented sceKernelDebugOutText
(+ fd1). No handoff needed - it carries a real import table, so the plain entry runs it. This is the
valid-diff unblock the stale corpus (Aug-25, 27 sections) had been hiding.

And it settles the earlier "missing tests" question outright: 130-layout/memory-type now runs and is
pass 0x3, query-size-ladder partial, short-buffer-overrun pass 0x4 - byte-for-byte the hardware
verdicts. They were never an orbistoun gap, only the stale binary. B (the eboot's PT_SCE_DYNLIBDATA
vendor-segment fault) is therefore optional: the payload, not the eboot, is the corpus artifact.

New blocker, narrower: the run crashes ~29% in, in the video section. sceVideoOutGetFlipStatus,
sceVideoOutSubmitFlip and sceVideoOutSetFlipRate are declared but not in video::implementations(),
so a flip is submitted, its completion polled, and the placeholder read into a fault. That is a
video-flip model to build (a new subsystem piece), flagged rather than assumed. Capture note: obSCEne
emits each record to both DebugOutText and fd1 and orbistoun forwards both to stderr, so the raw
stream double-counts - dedupe on read.

