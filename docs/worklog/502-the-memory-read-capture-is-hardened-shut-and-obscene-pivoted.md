# 502. The memory-read capture is hardened shut, and obSCEne pivoted to calling the builders

**2026-09-10** - loop, woken by a new obSCEne sweep (`swp20260910-141829`)

The Monitor armed in 501 fired within the half hour: obSCEne ran a fresh hardware sweep, and it is
the first one built to answer `8ef4`. The payload leg landed first and it changes the GPU picture -
not with a capture, but with a wall of its own.

## The lucky read is gone: kernel hardening closed both paths

`170-gpu-capture` this run tried to read `AgcCompositor.elf` (pid 0x39) again and reported, in the
open:

- `krw-rodata-read` -> `blocked-dmap-unmapped`
- `krw-xom-read` -> `blocked-xom`
- `ptrace-attach` -> `attached` (0x0) but `ptrace-rodata-read` -> `failed`, `ptrace-xom-read` ->
  `blocked-xom`
- `access-verdict` -> **`both-paths-blocked-by-kernel-hardening`**

The `command-stream` sub-check "passed" but the sample it read is sixteen zero bytes
(`raw-dwords 0`, `dv-buffer-vaddr 0x4040200000`); `shader-blob` failed outright -
`sl00 ... or s_endpgm not read from compositor rodata/code`.

So the 09:46 capture that gave worklog 499 its real header `0xc0599328` and its shader window was a
**window that is no longer open**: external KRW now finds the DMAP unmapped for user memory and
ptrace cannot read the compositor's rodata/XOM. This is the `not-possible naming the limit` branch
of `8ef4`, and it is a harder limit than the "bounded window per call" I guessed at - it is *no*
window by that route. The header and shader agreements already committed (worklogs 499) still stand
as evidence; they just cannot be extended by reading another process.

## obSCEne's answer is to stop reading memory and call the builders itself

The same sweep added two sections that get command bytes a lawful way - by calling the real
command-building functions in obSCEne's own process and recording what they encode:

- `165-gnm` - "Calling the confirmed libSceGnmDriver command-builders and recording the PM4 they
  encode." **Skipped**: `libSceGnmDriver is previous-generation; excluded from native PS5 target`.
  Gnm is the PS4 path; a native PS5 title does not use it.
- `166-agc` - "Calling confirmed libSceAgc command builders and shader creation, recording packet
  encodings and shader structure offsets." Every probe (`cb-nop`, `create-shader`, `dcb-dma-data`,
  `dcb-wait-reg-mem`, `dcb-reset-queue`, the census) **skipped** with
  `the loader did not resolve this symbol for this build` / `libSceAgc is not loaded`.

This is the approach that matters. `166-agc` recording *packet encodings and shader structure
offsets* from the real `sceAgcCreateShader` is exactly what the orbistoun GPU wall needs, and it is
robust to the hardening because obSCEne calls the function rather than spying on a process that did.

## Why the payload leg could not run any of it

Every graphics section skipped for the same reason: **the payload leg has no graphics libraries
loaded**. `085-videobuf/payload-screen-reading` shows `sceVideoOutOpen` at `dlsym 0x0`,
`kexport 0x0`, `dynlib 0x0` - libSceVideoOut is not resolvable in an injected payload, and neither
is libSceAgc. The injected `.bin` gets a minimal environment, not a title's full dynamic link.

So `080-video/*`, `085-videobuf/{buffer-shape,flip-alternates,framebuffer-refusal}`, `166-agc/*` and
the `170` command-builder path can only produce data on a leg that launches as a real title with the
graphics libraries linked - the **pkg / eboot legs**, which were still running when the payload
`.obs.log` landed. The payload leg's own passes (`scanout`, `payload-screen-reading`, `reduction`)
are all `derived` - structural, not exercised.

## What I am waiting for, precisely

The pkg and eboot legs of `swp20260910-141829`. On those, if libSceAgc resolves,
`166-agc/create-shader` and `166-agc/dcb-*` will carry **real packet encodings and shader-structure
offsets from the hardware command builders** - the first structured, lawful, reproducible GPU data
in the whole corpus, and better than a raw memory dump because obSCEne names which builder produced
each encoding. That flows straight into `orbistoun-gpu`'s `packet::walk`/`registers` and
`orbistoun-shader`, which worklog 499 already anchored on the one header and one shader that the
now-closed read gave up.

## Next

- Read the pkg/eboot `166-agc` output the moment it lands (Monitor is armed).
- If it carries packet encodings: cross-check them against orbistoun's `data/packets.toml` the way
  the header was cross-checked - a second independent producer, this time of whole encodings.
- If the pkg/eboot legs *also* skip `166-agc` (obSCEne's own title does not link libSceAgc either),
  then the builder-call approach needs obSCEne to link the library deliberately, and that is the
  next request - a smaller, more tractable ask than "defeat the hardening".
- The memory-read `8ef4` ask is now answered by the hardening verdict; reframe it rather than leave
  it open expecting bytes that route cannot deliver.
