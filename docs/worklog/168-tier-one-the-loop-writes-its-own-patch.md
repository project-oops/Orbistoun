# Tier one: the loop writes its own patch


Three tiers scaffolded (D296), first one built. From a clean machine - no policy anywhere:

```
$ orbistoun-cli turn titles/PPSA02664-app0/eboot.bin --apply
8 finding(s), 4 step(s), 3 of them mechanical
  swept: OutParameter { slot: 0, offset: 1048544, answer: Some(0) }
  *** gave it a region at 0x50000000: reached 25 against 23, faulting at 0x0
  proposing sceKernelReserveVirtualRange: needs ConformanceCheck
    assumes: 0x200000 bytes is a guess: the sweep measured where the guest
             faulted, not how much it asked for
  wrote 1 entr(y/ies) to ...\learned.toml

$ orbistoun-cli run titles/PPSA02664-app0/eboot.bin
  fault    image+0xafcc08   (was image+0xafc959)
  verdict  FURTHER  executed code it could not reach before
```

**No human input, no code, no rebuild.** The hand-written implementation from earlier is gone;
the entry the loop wrote does the same job.

### The three properties that make it safe to run unattended

- `learned.toml` is a **separate file**, so deleting it is a complete undo.
- It is **absorbed** into the policy, never merged over it - `StubPolicy::absorb` keeps every
  entry a person wrote, so the worst a wrong guess costs is a run rather than a decision.
- `default_return` is **never** taken from it. That applies to every function the loop did not
  measure, and changing it would be the loop altering behaviour it knows nothing about.

### What tier one deliberately will not do

`Evidence` is a field on the patch rather than a judgement at the point of reading. A patch
that changes only a **return value** may be kept on `FURTHER`; one that **writes memory** may
not, and needs a conformance check. `FURTHER` says the guest went further, not that the
behaviour is right - and a `copy n` with the wrong `n` produces `FURTHER` and corrupts state
that surfaces somewhere unrelated. That is principle 3's opening sentence, and it is the one
thing that would make an autopatcher worse than nothing.

### Two caught by lints, one of them real

- `u32::try_from(answer)` failing folded to `StubReturn::Ok` - a value in a policy file that
  **nothing measured**. Clippy flagged it as identical match arms; the fix is to refuse the
  answer and keep the write, which is still earned.
- The first `--apply` wrote `region_bytes = 0x1fffc0` - the un-page-aligned figure that had
  already caused a guest to fault *inside the region it was given* (D289). The service rounds,
  so it worked; the number in the file still lied. Rounded at the source, with a test.

