# D206 - Vendor documentation is consumed freely; the LLVM restriction is about oracles, not licences


**Status:** decided (2026-08-21) - prompted by another thread raising it as a provenance concern

Three categories, and only one is excluded for licensing reasons. The file that records
them named two, which left room to read the emulator rule as covering everything.

| | |
|---|---|
| The platform vendor's binaries and firmware | **Never.** Not public, not licensed, not ambiguous. |
| Another implementation's source | **Not read.** Not a licensing matter - much of it is open source. It is *derivative*: someone else's reading of the hardware, inheriting their mistakes with none of their working, and reimplementation-from-source converges on the original in a way that is evidence. |
| Public vendor documentation and vendor-contributed open source | **Used freely.** AMD publishes its instruction set guides openly and contributes the AMDGPU backend to LLVM under a permissive licence. This is a silicon interface shipped in retail parts and documented for anyone to program against. It is published for this. |

The middle row is the one that carries the project's actual value: this derives its own
tables and keeps the proof. That is a reason to avoid other people's *conclusions*, and it
says nothing about the vendor's own material.

### The concrete misreading, which is why this is written down

A thread reviewing this work concluded the instruction set material came from PS4-era
emulator decode tables and recommended LLVM's TableGen files instead. Both halves were
wrong in opposite directions: the material came from AMD document 70648, cited with a
retrieval date since 2026-08-20, and the reason we do not parse `.td` was never a
provenance one.

Two distinct failures worth naming, because they will recur:

- **Assuming a restriction is broader than it is.** The emulator rule got applied to
  vendor material, which would mean refusing documentation published so that developers
  will use it. That is a cost with no benefit.
- **Assuming a refusal is a provenance refusal.** Not parsing `.td` is an *engineering*
  constraint - see below - and defending it on licensing grounds would be defending it
  badly, and would eventually lose the argument for the wrong reason.

### The LLVM rule, stated once

**LLVM may check a table and may cross-check a fact; it may not be the thing the table is
generated from.**

The AMD document supplies values; LLVM detects errors through its behaviour as a black
box. Generating the table from LLVM's tables collapses two sources into one and the
differential test can only confirm that LLVM agrees with itself.

The LDS opcode field is the standing proof: it is at `[25:18]`, the document's field table
says `[24:17]`, the generator disagreed from assembled bytes, and the document's own
opcode table settled it. Generated from `.td`, both sides of that comparison would have
been LLVM.

Reading `.td` as a *third* source is welcome and additive in principle. It would want an
acknowledgement entry and a `published` attribution, like anything else.

**It has no consumer today.** This entry first justified it by saying the hidden
condition-code side effects and the division thresholds were `BLOCKED` in `model.rs` for
want of exactly that kind of fact. Both are implemented - the condition-code behaviour
since D129 and the division sequence since the published reference supplied its thresholds.
The claim was copied from a stale paragraph in REFERENCES.md within an hour of writing it,
which is the documentation drift D202 and the sweep before it were both about, arriving one
more time.

The single remaining `BLOCKED` entry is `exp`, and no table can settle it: it needs a
render target to export to, which is a decision about this project rather than a fact about
the hardware. So the third-source route is available and correct, and there is currently
nothing to point it at.

