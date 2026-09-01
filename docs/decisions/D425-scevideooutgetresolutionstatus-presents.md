# D425 - sceVideoOutGetResolutionStatus presents 1080p; the skip is a held output, not a headless one


**assumed** - 2026-08-31 (value corroborated, layout assumed)

With the flip model in (D424), the one clean partial left was `130-layout/resolution-status`. The
call reports the display's resolution; a title reads it to size a render target. Implemented it to
write `1920x1080` - `width` at offset 0, `height` at offset 4, the documented leading two `uint32`s
of `SceVideoOutResolutionStatus` in the open homebrew SDKs. The rest of the structure is left as the
caller prepared it, exactly as `GetFlipStatus` writes only the count: no lawful source here gives the
other offsets. Recorded `assumed` for the layout; the *value* is stronger than that, because obSCEne
renders at 1080p ("the resolution every output supports") and the hardware runs bring a
`1920x1080` framebuffer up and present through it - so it is a size the console demonstrably drives.

The finding worth keeping is why hardware skips this test, because it is not what it looks like. The
skip text is "no video output to query", which reads as *headless*. It is not: the same reports carry
`OBS|display|ready|1920x1080` and `OBS|display|presenting`. The display is up. The test skips because
obSCEne's own display path already opened and holds the main output, so the test's *second*
`sceVideoOutOpen` is refused (`0x8029_0009`) - the handle-still-held trap obSCEne documents elsewhere.
So orbistoun and hardware diverge here for a real reason: orbistoun lets the main output be opened
twice, where hardware refuses the second. That single-output-ownership is a separate fidelity gap,
now noted; modelling it would make orbistoun *skip* this test as hardware does, rather than pass it.

obSCEne's report already records display state richly (`opening`/`ready`/`presenting`/`absent`/
`failed`), so "record the run's display state" was already done; the gap was the resolution-status
skip *message* naming a cause it had not determined. Fixed in obSCEne's `layout.c` to distinguish
"the display path already holds the main output" from a genuine absence - but that rebuild is blocked
on the parallel injector workstream's untracked `src/common/` + Makefile (missing `-Isrc`), so the
source change stands and the built binary lags until their build is sound. orbistoun side: video
tests + clippy clean, resolution-status now `pass 0x8`.

