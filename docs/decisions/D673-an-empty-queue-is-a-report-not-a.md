# D673 - An empty queue is a report, not a refusal

**Status:** decided
**Date:** 2026-09-10

## What the console does with no mouse plugged in

obSCEne's `101-input-ext/mouse-read` ran on a machine with nothing attached:

```
sceMouseOpen   handle 0xc70700
sceMouseRead   rc 0x0,  extent 0,  changed 0
```

It **opens**, and the read **succeeds having written nothing**. Zero is the number of events
taken, not an error code. A guest polling in a loop gets "nothing happened" and loops.

orbistoun refused the open and answered a placeholder to the read, so the check read `partial`.

## The shape, and what is not claimed

Four functions, all measured or inferred narrowly:

- `sceMouseOpen` opens with no device. Refusing would look stricter and be wrong.
- `sceMouseRead` answers `0` and touches **nothing** - not even zeroing the destination, because
  the console's measured extent was zero and a guest holding a previous sample there would find
  it wiped.
- `sceMouseInit` and `sceMouseClose` answer success, and that is **assumed**: obSCEne calls both
  and discards the return, so it is inferred from the calls around them working.

The arities stop being `6`. They were the trampoline's full capture, which is not a claim (D504);
obSCEne types each one and the console answered, so the first N arguments are established - with
the usual caveat that a call which works cannot see a trailing argument nothing passes.

**A bad handle earns `GuestError::InvalidHandle`, a placeholder.** Every other input subsystem
here carries a measured base - the pad's `0x8092_0000`, audio's `0x8026_0000` - because obSCEne
provoked a failure and read one back. Nothing has provoked one from `libSceMouse`, and inventing
a `0x80xx_xxxx` to look like its neighbours is the single thing this must not do.

## Registered beside the pad rather than folded into it

`orbistoun_input::implementations()` answers a `&'static` slice, so gathering a second module
there means allocating. One explicit line in `symbols.rs` is cheaper and easier to notice when a
third arrives.

## The guard from D668 earned itself

Writing `libSceMouse.toml` and not registering it in `EMBEDDED` failed
`every_knowledge_file_on_disk_is_embedded` on the first run, by name. That test was written two
turns ago because twelve files had accumulated that no build loaded; this is the thirteenth
being caught before it joined them.

## What it bought

`101-input-ext/mouse-read` passes. `101-input-ext/mouse-moving` moved from *"no mouse attached"*
to *"mouse open, but no record was read: move it and re-run"* - the same shape the pad's
`button-bits` took, and the same limitation: orbistoun has no field offsets to place live input
into, so the queue is permanently empty rather than sometimes empty.

Against the matching hardware leg the divergences went from five to **four**, and orbistoun
passes 175 against hardware's 167.
