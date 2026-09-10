# 482. The guest was talking all along

**2026-09-09** - directed, continuing 481

Asked whether an engine's own logs could be read *generically* rather than per-engine. They can,
because the capture point is not the engine: it is the platform ABI every guest shares - the C
library's format family, `sceKernelDebugOutText`, and writes to the standard descriptors. Six call
sites, no knowledge of Unity, and a homebrew `printf` lands in the same ring as a commercial
engine's logger (D658).

## What it said, first time it ran

PPSA25872, whose wall the last four decisions have been about:

```text
[libil2cpp] sceAppContentInitialize returned 0x7fff0001
```

**The engine names the function and quotes back orbistoun's own placeholder.** Then it enters the
diagnostic dump that `image+0x17554a3` dies inside. `REQ-20260909T2145Z-9e52` had already asked
about that function on a ranked guess; the guest says it outright.

PPSA02664, never read before:

```text
outbuffer 0_8640KB  1_8640KB  2_8640KB  3_2560KB  4_192KB
todo: sceVideoOutInitializeOutputOptions will be available in a future SDK
path /app0/Media/globalgamemanagers is not considered suitable for apr reads
TODO: virtual bool LocalFileSystemPS5::Enumerate(...)
todo: void GfxDevicePS5SharedData::CreateWorkload()
```

Three 8.6 MB display buffers, video-out options, Unity's boot asset, and the graphics device
creating its workload. **That title is closer to a first frame than the one a week of walls has
been about**, and no fault address or import count said so.

## Surprises

- **121 formatted messages collapsed into 5 lines** on the first run, because a `printf` without a
  trailing newline is still a separate utterance and I was concatenating them. `Argument Count =
  1Arg 0 = ...` was the tell.
- **The capability existed and nobody looked.** `sceKernelDebugOutText` has forwarded to stderr all
  along; it scrolled past under the build output every single run. What was missing was not the
  bytes but a place in the report that says *this is a thing to read*.
- **It reorders the corpus.** Ranked findings answer what to implement; this answers where the
  guest thinks it is, and the two disagree about which title is furthest.

## Next

- PPSA02664 as the menu candidate: it is inside `GfxDevicePS5SharedData::CreateWorkload` when it
  dies at `image+0x39f7c`.
- `9e52` is now confirmed rather than suspected - `sceAppContentInitialize` is PPSA25872's gate.
