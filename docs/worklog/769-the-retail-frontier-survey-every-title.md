# 769. The retail frontier survey: every title is at a hard wall; PPSA28061 needs sceAgcGetRegisterDefaults2 measured, so an obSCEne request is filed

**2026-09-21** — worklog 768 banked PPSA04263 (Grand Theft Auto V)'s wall as a deep startup trace and
turned the loop to broaden. This surveys the whole retail frontier to pick where a wall can move, and
records the honest finding: every retail title now stands at a hard wall, and the one with a named
missing function needs a hardware measurement, which is now requested rather than guessed.

## The frontier, as it stands

From `docs/compat-frontier.txt` and a fresh run of each:

| title | reach | wall | shape |
|---|---|---|---|
| PPSA03416 | flipped, 222 imports | `VCRUNTIME140.dll+0x1dc8d` | execution fault inside the title's own C runtime |
| PPSA02664 | flipped, 222 imports | `VCRUNTIME140.dll+0x1dc8d` | **same** wall as PPSA03416 |
| PPSA25872 | flipped, 192 imports | `image+0x3b383b` | execution fault in title code |
| PPSA04263 | entered, 75 imports | `image+0x19676d7` | null-vtable, banked (768) as a startup trace |
| PPSA21564 | entered, 57 imports | `the title's modules+0x7af792` | execution fault in a title module |
| PPSA28061 | entered, ~47 imports | `image+0x10b9e9` | **a named missing import**, dereferenced |

Five of six are execution faults inside the title's own code with **100 % of imports answered** - no
missing HLE call to implement, the same class as GTA's null-vtable: they need disassembly of the title's
path, not a symbol. PPSA28061 is the exception, and the only one whose wall is a function orbistoun can
name.

## PPSA28061's wall is a real missing function

A fresh run faults at `image+0x10b9e9`, `read of 0xf7ff0039`, with the cause named directly:

```text
libSceAgc::sceAgcGetRegisterDefaults2(0xd) -> 0xf7ff0001
  the guest used sceAgcGetRegisterDefaults2's answer as a pointer without checking it
```

`sceAgcGetRegisterDefaults2` is **declared** in `orbistoun-gpu`'s AGC module (arity 6) but has **no
handler**, so it returns the placeholder-error range (D670). The guest treats the return as a **pointer
to a register-defaults table** and reads its `+0x38` field - `0xf7ff0001 + 0x38 = 0xf7ff0039`, the fault.
So the function returns a pointer to a fixed table of GPU register reset values, and the title reads
through it immediately.

This run also reached fewer calls than the title's best-ever (326 against 933 on 2026-08-23, "below the
best ever recorded"), i.e. it is at or behind its high-water mark; whether that is a regression or a
path difference is a separate question, noted for later and not chased here.

## Why this is a request, not an implementation

The table's contents are a hardware fact: the register reset defaults for AGC register-set `0xd`, and the
layout the title reads them back through. Inventing either would be exactly the plausible-but-wrong output
D708 forbids - a zeroed buffer would clear this one fault and hand the GPU wrong defaults that surface far
downstream. `sceAgcInit` next door is implemented **from a measurement** (FW 12.40, REQ-...a3f0,
`166-agc/init`), which is the standard this must meet. obSCEne measures AGC through that same `166-agc`
section, so the value is obtainable.

Per the standing rule (needing hardware data means filing, never a question), an obSCEne request is filed
on the bus for `sceAgcGetRegisterDefaults2`: the pointer it returns for set `0xd` (and the other set ids a
title passes), and enough of the pointed-to table to cover the `+0x38` field the title reads. When it
comes back, the handler is a table return recorded against that measurement, the way the rest of the AGC
cluster is.

## Gate state

No code changed - a survey that fixes where the loop spends next (PPSA28061 on the AGC measurement;
GTA and the four execution-fault titles on title-path disassembly) and files the one request that
unblocks a nameable wall. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean.
No commit.
