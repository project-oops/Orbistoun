# 747. The faulting descriptor is a "1234" shader header with two unrelocated fields

**2026-09-21** — the indirect peek from 743, aimed by a frame calculation, finally reads the faulting
descriptor itself. It is a shader/register header with ASCII magic `"1234"`, and its data-pointer array
has **two raw, unrelocated values where absolute pointers belong** — one of which is the `0xa8` the
`memcpy` dies reading. The relocated-and-not split is exact, and it names a concrete suspect: the
relocation set orbistoun (and the guest) apply is missing two entries.

## Finding the descriptor across ASLR

The descriptor is `0x37ea0`'s `r14`, callee-saved, so the copier `0x42d90` pushes it in its prologue
(`push rbp; push r15; push r14`). Its saved slot is therefore `[copier_rbp - 0x10]`, and the copier's
return into the caller (`0x37f53`, group 0's call site) fixes `copier_rbp = 0x6000007fc1a0` — so the
descriptor pointer lives at the **stable** stack slot `0x6000007fc190`. The indirect peek
`[0x6000007fc190]+0x80` follows it into this run's heap and dumps the object, ASLR notwithstanding
(743's tool doing exactly its job).

## What the descriptor holds

```
0x…600  "1234" 0x18…                 ; magic + header
+0x18   0x00000000000000a8            ; group 0 data pointer  -> RAW VALUE 0xa8    (faults)
+0x20   0x…670  (= base + 0x70)       ; group 1  -> relocated, valid
+0x28   0x…638  (= base + 0x38)       ; group 2  -> relocated, valid
+0x30   0x…660  (= base + 0x60)       ; group 3  -> relocated, valid
+0x38   0x0000000000000058            ; group 4 data pointer  -> RAW VALUE 0x58    (would also fault)
+0x5b   0x0a  +0x5c 0x06 …            ; per-group register counts (group 0 = 10 -> n = 0x50)
```

So the fault is not a null pointer and not a stray value: `+0x18` holds the **relative value `0xa8`
that was never relocated to `base + 0xa8`**. The fields that were relocated (`+0x20/+0x28/+0x30`)
became valid pointers into the header's own buffer; the two that were not (`+0x18`, `+0x38`) still hold
their raw values, and the copier reads `0xa8`/`0x58` as addresses and dies on the first. Group 0's
count `0x0a` (ten registers, `n = 0x50`) is correct — only its pointer is unrelocated.

## The suspect: an incomplete relocation set

The relocated set — `+0x20`, `+0x28`, `+0x30`, and not `+0x18`/`+0x38` — is **exactly**
`create_shader`'s hardcoded list `for &offset in &[0x20, 0x28, 0x30]` (`agc.rs:253`). That list, and
the "44 bytes changed" it derives from, come from obSCEne `166-agc/create-shader`. The match is too
precise to be coincidence: the relocation logic that ran on this header — whoever ran it — uses the
same three-entry set orbistoun does, and PPSA02664's shader has **five** relative fields
(`+0x18,+0x20,+0x28,+0x30,+0x38`) needing relocation. The measured shader behind the current list did
not exercise `+0x18`/`+0x38`, so those were never learned, and a header that uses them relocates three
of five and leaves two raw.

Two facts keep this honest rather than a fix waiting to be typed:

- **`sceAgcCreateShader` is not called on this run** (the full `libSceAgc` census confirms it), so
  orbistoun's `create_shader` is not the code that relocated this header — the guest's own inlined
  loader did. Adding `0x18`/`0x38` to `create_shader`'s list would not touch this path. The value of
  the match is that it points at *which* fields a header of this shape needs relocated, not at a line
  to edit.
- Whether hardware relocates `+0x18`/`+0x38` is a **measured** question, and the measurement that set
  the current list is the one that was silent on them.

## Next step

Two threads, both concrete now that the descriptor is in hand. First, find the code that relocates this
header on PPSA02664's path — the guest's inlined loader — by disassembling the construction (the header
base is knowable each run through the same slot chain, so the write of `base + 0x70` into `+0x20` can be
located). Second, and independently, file an obSCEne request to measure a shader header that populates
`+0x18` and `+0x38`, to establish whether hardware relocates them and to correct the shared relocation
set if so. The second is the accurate-emulation answer: the current three-entry list is a measurement
that never saw these fields, and the fix is to measure them, not to guess.

## Gate state

No code changed — an indirect peek, a frame calculation, and reading `agc.rs`. `./bin/orbistoun check`
is unchanged from 746 (green but for the same three generated-doc drifts from a prior session's
uncommitted `compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No commit.
