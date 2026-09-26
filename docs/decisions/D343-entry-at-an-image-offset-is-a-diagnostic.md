# D343 - Entry at an image offset is a diagnostic

**Status:** decided
**Date:** 2026-08-27

`[entry] at = <image-relative address>` starts a guest somewhere other than its declared entry,
typically at a sized `main` symbol. The field is `Option<u64>` so offset zero is a real request,
the executable-segment check guards it as it guards the declared entry, and the run says on
stderr that it did not start where its container says.

**Why:** it steps past startup code whose handoff structure cannot be derived and reaches
program code directly. It is not a claim about how the platform starts a program, so every
later number must carry that caveat. A jump into data would produce a fault about the
diagnostic rather than the guest.

**Rejected:**
- Zero as "not set": an image's first byte is a legitimate entry.
- An unguarded jump: a fault about the instrument.
