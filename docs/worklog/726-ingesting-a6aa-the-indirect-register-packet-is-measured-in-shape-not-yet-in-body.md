# 726. Ingesting a6aa: the indirect-register packet is measured in shape, not yet in body

**2026-09-20** — folded obSCEne request `a6aa`'s measurement into orbistoun's knowledge for
`sceAgcDcbSetCxRegistersIndirect`, the builder at the head of PPSA02664 (Alex Kidd in Miracle World)'s
`CreateWorkload` wall. The entry had been telling a false story, and a6aa is the hardware reading that
corrects it.

## What the entry claimed, and why it was wrong

The stored note said, in as many words, "NOT IMPLEMENTABLE YET, its encoding is unmeasured: obSCEne
measured `sceAgcDcbSetCxRegistersIndirectGetSize` but never the packet." It was `known_by =
"guest-observed"`, dated 2026-09-14. That was true when written and stale by 2026-09-15: a6aa (a
request orbistoun itself filed) wired and probed all 35 present-but-uncalled libSceAgc builders, and
sweep `20260915-125124` captured this one — `pass 20 B`, header `0xc0039f00`, GetSize 20. The "never
the packet" claim was simply out of date, and a stale "not implementable" note is worse than no note:
it tells the next session to stop.

## What a6aa actually pins, and what it does not

The dump reads `009f03c0…00000080`. Decoding the header settles the apparent 16-vs-20 mismatch between
the dumped bytes and the reported size: `0xc0039f00` is a PM4 type-3 packet, opcode `0x9f`, header
count field = four body dwords, so 20 bytes total — which is the GetSize a6aa read independently. The
Sh and Uc siblings measured the same shape (`0xc0036300`, `0xc0036400`, both 20 bytes).

So the **extent and header are measured, not guessed** — a real advance over "unmeasured". But a6aa
probed with *no register arguments*, so the body dumped as zeros carrying a single `0x80000000`. The
one thing still unmeasured is how a register offset and its value land in the four body dwords — the
arg→body mapping — and that is the whole distance between orbistoun and a correct encoder here.
Deriving it from the PM4 `SET_CONTEXT_REG` indirect layout alone would be the format-plus-a-guess that
principle 3 forbids and that this project files obSCEne requests specifically to avoid.

The knowledge entry now says exactly that: `known_by = "measured"` for the structure, an edge_case
spelling out the measured header/extent and the open body mapping, and a cite to a6aa's sweep and log.

## Two other corrections folded into the same entry

- **The phantom, re-attributed.** The old note credited *this named builder* with being "the producer
  whose return is passed to the patch family 32×." Worklog 724–725 showed that role belongs to the
  unnamed `libSceAgc::0x7d86501b8094ef57` (a distinct NID, a GetSize), and that the old attribution
  came from a static file-offset disassembly `ORBISTOUN_PEEK` proved wrong. The entry now records the
  correction rather than propagating the conflation.
- **No duplicate request.** The obvious follow-up — reproduce the build with representative register
  args and dump the populated body — was already on file when I went to write it: a concurrent
  Orbistoun process had filed `REQ-20260920T1056Z-7c22` minutes earlier (it surfaced as a
  "file modified since read" the instant I tried to append). 7c22 is stronger than the request I had
  drafted: it walks the full producer→`PatchAddRegisters`→`PatchSetAddress` chain with representative
  arguments, dumps each 20-byte packet, and checks whether the workload buffer itself is written —
  which is the actual mechanism of the PPSA02664 wall. So I filed nothing, and instead pointed the
  knowledge entry forward at 7c22 as the probe that closes the body gap. Filing a near-duplicate would
  have been noise; the discipline is to let the existing open request stand.

## Why this is the honest shape of the work

The wall does not move this tick — no packet is emitted, because the body is still unmeasured and
guessing it is exactly the thing D708 and principle 3 rule out. What moves is the *accounting*: a
title's builder that the tree called "not implementable / unmeasured" is now correctly recorded as
"measured in shape, one hardware probe from measurable in full," with that probe named and open. That
is the difference between a wall attributed to the title and a wall attributed to orbistoun's missing
measurement — which is the whole point of D708.

## Gate state

One file changed in orbistoun: `crates/orbistoun-hle/data/knowledge/libSceAgc.toml` (a data file, no
Rust). `status --write` regenerated README.md and docs/PROJECT_STATUS.md (recorded behaviours 815→817,
measured 82→85 across this session's knowledge additions). `./bin/orbistoun check` green; identity scan
clean. No commit.
