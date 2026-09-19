# 694. Seven stale facts corrected, and the flip builder's measurement recorded

**2026-09-19** — inbox `-49a1`: seven lines read as true beside code or a record that says otherwise,
and one obSCEne measurement nothing in the tree carried. Two of the seven would each send a later
session to wait on or re-probe a measurement that has already arrived - the worst kind of stale fact.

## The two that cost a session

- **D705 named a measurement that will never come.** The decision I wrote (worklog 689) cited obSCEne
  `-3423` as *"unresolved"* and as a way D705 retires. It was resolved **not-possible** - the symbol
  is not exported on the retail eboot - *before* D705 was written; I did not check. Corrected in both
  places in `D705` and in the `sceAgcDriverAddEqEvent` knowledge note: execution (36c0) is the only
  retirement left, and no `3423` measurement is coming. (The "compare values before acting" lesson,
  again, on my own decision.)
- **A flip builder's measurement, now on record.** New `sceAgcDcbSetFlip` entry in
  `libSceAgc.toml`, `known_by = "measured"`, recording obSCEne `-7443` (`166-agc/dcb-set-flip`, sweep
  `20260917-124503`): empty encoding, rc `0x0` under bare and prepared writers, GetSize symbol absent -
  a library-level no-op like `sceAgcDcbWaitUntilSafeForRendering`. Unwired: no guest reaches it
  (principle 6), recorded so nobody re-probes it.

## The other five

- `libSceVideoOut.toml` said *"The addresses are not read"*; `video_out_register_buffers` reads and
  stores them since worklog 678. Restated.
- `PROJECT_STATUS.md`'s "three walls": PPSA02664/PPSA03416 no longer die because
  `sceAgcDcbSetCxRegistersIndirect` *"is unimplemented"* (it is, worklog 616) with *"220 imports"*
  (222) *"blocked on a hardware probe"* - they pass that wall and die at the `0xa8` null object
  (worklogs 672/684). Both the section intro and the bullet restated.
- `CLAUDE.md` said placeholder codes *"deliberately avoid the high bit"*; D670 set it
  (`0xF7FF_0000`). Restated as "a reserved range no real firmware value occupies".
- `libSceAgc.toml`'s `sceAgcInit` entry said all three titles *"stop inside this library's
  initialisation"*; the first two pass init and fault in command-buffer construction.
- `HANDOVER-OBSCENE.md` called `0x7fff0001` orbistoun's placeholder; it is `0xf7ff0001` since D670.
- `primitive_draw.rs` said the render-and-compare *"needs a backend on the run path"*;
  `a_full_frame_composes_through_the_driver` (`orbistoun-gpu-vulkan`) already drives a `Submission`
  through `VulkanBackend` in a test, so the comparison can be a test (`-f50b`); only a live *run*
  still lacks a backend on the run path.

## Gate state

knowledge-audit 27 pass (new entry parses and cites); `orbistoun-gpu` builds; `status --check` exit 0
(the new measured entry bumped the behaviours count 814 → 815, regenerated); `./bin/orbistoun prose`
exit 0; `cargo fmt --check` clean; identity scan clean. No commit.
