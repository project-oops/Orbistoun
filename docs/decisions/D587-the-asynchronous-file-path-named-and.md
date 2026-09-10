# D587 - The asynchronous file path, named and reported

**Status:** measured
**Date:** 2026-09-08

## Three functions the wall is behind, and none was declared

PPSA03416 dies behind the platform's asynchronous file path (worklog 426): Unity's
`LocalFileSystemPS5` resolves paths to identifiers and sizes, builds a command buffer of reads,
submits it and waits. Nothing in orbistoun **declared** any of it, so all three landed on the
generic stub and the run could say only that something unimplemented had been called.

They are declared now, with handlers that **report and refuse**. Declaring is not implementing:
each answers the placeholder, and what it adds is that the run says what it was asked.

## The first thing it said

```text
orbistoun: the guest asked the asynchronous file path to resolve 1 path(s):
           /app0/Media/globalgamemanagers
```

**The loose-layout entry point**, which D578 established this title opens and reads nothing from.
So the shape is settled: the guest opens the file by name, hands the *path* to the asynchronous
path to be turned into an identifier, and reads through that - which is why five opens produce
one read of zero bytes, and why the four missing archive-layout probes were red herrings.

## A bare hash, named from the guest's own vocabulary

`libkernel::0x23020f8e2805acae` was one of 8,329 on `symbols/wanted.txt`. The guest's wrapper
prints `waitCommandBufferCompletion`; its sibling wrapper `submitCommandBufferAndGetResult` is
the platform name with `sceKernelApr` prefixed and nothing else changed. Same transformation,
plus the obvious truncation:

```text
0xce925bf6d363cd2e  sceKernelAprWaitCommandBufferCompletion
0x23020f8e2805acae  sceKernelAprWaitCommandBuffer     <-- the import
```

**Confirmed by the hash agreeing and nothing else**, which is the only confirmation there is
(`docs/PROVENANCE.md`). Which of the three unnamed imports the wrapper reports was itself
measured, by forcing each to a distinct value in one run and reading the number the guest printed.

## Reported rather than answered, and the reason is specific

Each handler prints its inputs and returns the placeholder. That is not caution for its own sake:

- **Resolve** takes an array of path pointers and a count - established - and three adjacent
  four-byte stack slots that are out-parameters of unknown order. Filling one would hand the
  guest a size where it expects an identifier. Planting distinct markers in all three *and*
  forcing success changed nothing the run could see: the guest submits its command buffer either
  way, so the resolve is not what gates it.
- **Submit** takes a header whose third and fourth words are a length and an address matching a
  guest mapping exactly. `libSceAmpr` exports `GetSize`, `GetNumCommands` and
  `GetCurrentOffset`, so three such fields exist; which is which is not established, and a label
  is a claim.
- **Wait** answers what the guest prints. Nothing is established about its arguments.

## The contradiction it exposed, recorded as one

The header reports one command of twenty bytes. The buffer it points at is **entirely zero** at
the moment of submission (readable at last - D588). And `sceAmprAprCommandBufferReadFile` is
imported by this title and **never called**, along with the constructor.

So the guest submits a buffer whose header claims a command that is not in it, having never used
the library function that would put one there. That is written down as an open contradiction
rather than resolved by picking whichever half is more convenient.

## What this does not establish

**Anything about implementing the path.** Three functions are declared, three report, and the
wall has not moved - 193 imports, the same fault. What changed is that the run now names its
inputs instead of naming only that something failed.

**Nor that arity six is right.** It is the trampoline's full capture, which is what
`orbistoun-gpu`'s `agc` module does for the same reason (D504): a wrong arity degrades a trace,
a wrong name is a shim nothing can reach.
