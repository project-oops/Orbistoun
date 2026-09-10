# 441. The other title

**2026-09-08** - directed, continuing 440

## What was done

Two of the four queued items became decisions rather than tasks, so the session's tooling went
somewhere it had never been: **PPSA02664**, which reaches furthest of any title here and had not
been looked at once in nine iterations.

The first thing it said was new (D598):

```text
path /app0/Media/globalgamemanagers is not considered suitable for apr reads flags:0x0
```

**It asks whether a path suits the asynchronous file path, is told nothing, and declines.**
PPSA03416 never says this and goes ahead.

## They are two different kinds of dump

| | PPSA03416 | PPSA02664 |
|---|---|---|
| `ampr_emu.index` | 16,248 bytes | absent |
| `fakelib/libSceAmpr.sprx` | 218,678 bytes | absent |

One is a **modified** dump carrying a shim that implements the Ampr API in guest code; the other
is **clean**. Nine iterations of work on the shim applies to one title.

**And that settles the scope question left open in D594.** Serving the shim is compatibility with
somebody else's emulation layer. Serving what the clean title needs is implementing the platform,
and no scope question arises - so the clean title is the better target and was never being looked
at.

## The clean title names the code it wanted

Its fault is a general-protection fault the report already explains (D384), and the registers say
more than the address:

```text
rbx=0x7fff0001            our own Unimplemented placeholder, live at the fault
81 fb 3d 00 6c 8a         cmp ebx, 0x8a6c003d
75 0c                     jne
```

**The guest compares an error against `0x8a6c003d` and branches.** The placeholder does not match,
so it takes the branch for a code it does not recognise, and that branch is what dies. D125's
shape with the answer attached: not just *a placeholder was used*, but *here is the value the
guest was looking for*.

## Surprises

- **Nine iterations on one title while the furthest-reaching one went unexamined.** Every tool
  built in those iterations worked on it immediately and the first output was a message no run
  had ever printed.
- **The two titles take opposite branches of the same decision** and both fail - one because it
  uses the asynchronous path, the other because it refuses to.
- **The report corrected me mid-investigation.** I read `0xffffffffffffffff` as `MAP_FAILED`; the
  fault note said it is usually a general-protection fault and cited D384. Both readings are
  compatible, and the tool's was the one with a measurement behind it.

## Next

- Force a candidate to answer `0x8a6c003d` and let the guest grade it. `ORBISTOUN_RETURN` does
  this and the oracle is the fault moving.
- Which import sets `ebx` before that comparison - the call immediately preceding it, unnamed so
  far.
- What `flags:0x0` should be, which is a separate defect from the fallback failing.
