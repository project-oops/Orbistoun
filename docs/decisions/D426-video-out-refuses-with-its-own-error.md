# D426 - Video-out refuses with its own error family, and refuses a second open of a held output


**measured** - 2026-09-01

Two video divergences off the complete run, both measured. First, `080-video/flip-rate-rejects-bad-
handle` records the console refusing a bad handle with `0x8029_000b` - a `libSceVideoOut` error, base
`0x8029_0000`, a family distinct from the kernel's `0x8002_00xx`. orbistoun answered the generic
`GuestError::InvalidHandle` placeholder `0x7fff_0003`, which is both the wrong code and the D125
shape (a value a caller tests reading as a handle). Every video call now refuses with
`video_error::INVALID_HANDLE` = `0x8029_000b` (measured for flip-rate, assumed uniform across the
family until each is measured).

Second, `130-layout/resolution-status` passed here where hardware skips it, and the reason was a real
gap: orbistoun let the main output be opened twice. A console refuses a second `sceVideoOutOpen` of an
output already open (`0x8029_0009`, the handle-still-held trap D169 documents), which is exactly why
obSCEne's resolution probe - which opens the main output while the display path still holds it - skips
on hardware. A port now records its `(bus, index)` and its open flag, and `open` refuses a second open
of a live output; `close` clears the flag so the output can be opened again. The probe now skips as
hardware does, and the `GetResolutionStatus` implementation (D425) still stands for a guest that opens
the output once.

Net after both: 515 pass / 9 partial / 4 fail / 15 skip, and the video section matches hardware
test-for-test. Video tests (4) + clippy clean.

