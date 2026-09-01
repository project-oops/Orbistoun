# 2026-08-27 - The payloads started working


The one-bit experiment off the backlog, run (D343). `[entry] at = <image-relative>` enters a
guest past its runtime start, and `main` is a real sized symbol in three of the five
payloads, so it needed no derivation - just an address.

```
klogsrv    image+0x0      signal(0xd) -> getopt(...) -> printf(...)
ftpsrv     image+0x5ba0   getopt(...)
shsrv      image+0x0      jumps straight to null
```

**The first library calls any payload has made in orbistoun**, after a day where the answer
was uniformly zero. `0xd` is `SIGPIPE`: a network server installing a SIGPIPE handler, then
parsing its options, then printing. That the calls are *the right ones* matters more than
the count - it says the code reached is the code meant to run, not a plausible accident.
`printf` is one orbistoun implements, so klogsrv is now executing against real code.

So `PAYLOADS.md` Stage 1 has a measured front of the queue instead of a guessed one:
**`signal`, `getopt`, `optarg`** ahead of the thirteen-function universal set. `optarg` is a
data object and already has storage (D323).

### What it does not mean

`__crt_start` is skipped, not solved. Something runs it on hardware and presumably matters;
all this establishes is that whatever it does is not required for `main` to begin working.
elfldr and pldmgr are stripped, have no `main` to enter at, and still need the structure.

Two runs of klogsrv gave 2 imports and then 3. The refactor between them cannot have changed
behaviour - `starting_address` returns the same value - so that is run-to-run variation under
an eight-second limit, and the call *sequence* is the finding rather than the count.

### Shape

`Option<u64>`, not a sentinel zero: an image's first byte is a legitimate address and two of
these put `main` exactly there, so the round-trip test carries `Some(0)` deliberately. The
executable-segment refusal that guards the declared entry (D010) guards this too - a
diagnostic that jumps into data produces a fault about itself, which is the third time this
session an instrument needed guarding before its results could be trusted (D308, D325).


