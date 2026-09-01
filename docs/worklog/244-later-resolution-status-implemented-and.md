# 2026-08-31 (later) - resolution-status: implemented, and the "headless" skip debunked


Implemented sceVideoOutGetResolutionStatus (1920x1080; width@0, height@4, the citable fields), which
flipped 130-layout/resolution-status partial->pass (tally 516 pass / 9 partial / 4 fail). The four
fails are all correct-to-match now (two 110-modules refusals at the hardware code, the parallel
kernelcall, the all-mode census control).

Surprise, and it corrects my own earlier claim: hardware does not skip resolution-status because it
is headless. The reports show OBS|display|ready|1920x1080 and |presenting - the display is up. It
skips because obSCEne's display path already holds the main output, so the test's second
sceVideoOutOpen is refused (0x80290009). That means (a) 1920x1080 is corroborated, not just
public-doc assumed, and (b) orbistoun has a separate fidelity gap: it lets the main output be opened
twice where hardware refuses - which is why orbistoun passes this test and hardware skips it.

obSCEne already reports display state richly, so "add a display-state header" was already done; the
real gap was the skip message. Fixed layout.c to say "the display path already holds the main output"
rather than "no video output to query" (+ the display.h include it needed). That obSCEne rebuild is
blocked, though: the parallel injector workstream's untracked src/common/ (freestd.c + injector/*)
and its Makefile lack -Isrc, so the host build fails on common/freestd.h - not our change, not ours
to fix. The layout.c source stands; the corpus obscene.elf (built before that landed) still runs.

