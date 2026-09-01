# 2026-08-30 - Seven placeholders, retired by one file


A complete conformance suite ran on a target console and the capture landed in the sibling
repository. 521 checks, 28 sections, and - this is the part that mattered - value-bearing
records rather than pass and fail: `measure`, `bytes`, `size`, `region`, `err`, `sysinfo`.

This project keeps its guesses in a field with a controlled vocabulary specifically so they can
be counted and retired. That is only worth the ceremony if it actually happens when evidence
arrives, so this was the test of the whole arrangement. Six entries now say `measured`, where
none did before. D398 lists what moved.

**The encoding was the one worth having.** Seven distinct failures provoked across five
unrelated call families, every one coming back as the errno under `0x8002_0000`. It had been
resting on a single value seen on an emulator that could itself have been inferring the same
rule - evidence of nothing, and correctly recorded as a hypothesis. `GuestError::Busy` is gone
because of it: the reasoning that gave `trylock` its own code was right and the number was a
placeholder, and the distinction is now made with what the machine uses.

**Two things were wrong here in ways no amount of reasoning would have found.** Direct memory is
five gibibytes and not the eight assumed - not a power of two, so unreachable by guessing. And a
buffer smaller than the query structure was being *refused*, on this project's own argument that
a caller passing less wanted a different layout; the console accepts every size from 1 to 256.
That refusal was invented here and had been sitting behind a confident comment.

### The surprise

`sceKernelGetModuleInfo` **failed on hardware too**, with the invalid-argument code - and that is
better news than it sounds. D395 refused to invent the structure and said the layout had to be
measured. A refusal of the *call* rather than the *request* points at a size field the caller
fills in first, which is a specific thing to try instead of a layout to guess.

And the run answered a question nobody asked it. `sceKernelLoadStartModule` returned `0x2001`
for a system module - the exact constant `elfldr` and `pldmgr` have been dying on, which had
been established as invariant to every handoff variation and never identified. It is a module
handle. Those two payloads are not failing with an error; they are succeeding and doing
something with the result.

### What it exposed about the accounting itself

`known_by` is one value per entry and evidence arrives per *claim*. `GetProcessTime` now has a
measured unit and an unmeasured origin, and the guard refuses `measured` alongside an open
question - correctly, since the queue would otherwise re-ask something settled.

The temptation was to weaken the guard so the new data fit. That is the failure principle 3
warns about one level up, so instead the entry holds the weakest link and the measurement lives
in a cited edge case. It is honest and it undersells the evidence, and whether provenance should
attach to a claim rather than a function is raised in D398 rather than decided.

`docs/HANDOVER-OBSCENE.md` is new: the residue, aimed. Seven asks, each saying what to call and
what it unblocks here, because a probe that costs a hardware run deserves to be pointed at
something.

