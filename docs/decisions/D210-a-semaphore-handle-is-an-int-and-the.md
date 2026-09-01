# D210 - A semaphore handle is an int, and the type says so now


**decided** · 2026-08-24

obSCEne relayed the `sceKernelCreateSema` signature from public interface documentation:

```c
sceKernelCreateSema(int *out, const char *name, uint32_t attr, int init, int max, const void *opt)
```

Two facts, one of which was already right here and one of which was a live bug.

### The argument order was right, as a guess

`args[1]` was read as a name and `args[3]`/`args[4]` as the counts, which matches. But the
code said so in the words of somebody who did not know: *"clamped, not trusted - if the
argument order is not what is assumed, these are some other value entirely"*. It is now a
citation rather than a hedge, and the knowledge entry moved from `guest-observed` to
`published`.

### The handle width was wrong, and nothing here could have caught it

The out-parameter is an `int`. This crate typed the handle `MutexHandle = u64` and wrote it
with `write_unaligned::<u64>`, so **every `sceKernelCreateSema` put four bytes of handle into
whatever the guest kept next to its semaphore**.

The write succeeds. The handle round-trips through our own table. The damage lands wherever
that neighbour is read, arbitrarily far away. It is the D171 out-parameter class arriving
from the other direction - not failing to write, but writing too much - and it is worse in
one respect: a missing write leaves a poison pattern somebody can recognise, and an overrun
leaves plausible bytes.

### Narrowing the cast would have been the wrong fix

Mutex handles are leaked host addresses, which is fine for a `void *`. Truncated to four
bytes, a 48-bit address collides with every other semaphore sharing its low half - silently,
which is the same failure again in a new place.

So `SemaphoreHandle` is its own type (`i32`) with its own **counter** allocator starting at
one, and obSCEne's "they are not the same shape" is enforced by the compiler rather than
remembered. A test that passed a mutex handle to a semaphore call and asserted it found
nothing no longer compiles, which is a better answer than the one it was checking for.

### It did not move the wall

`PPSA02664` faults at `image+0xafc959` exactly as before, with the same last calls. The
overrun was real and was not this. Recorded because the pull to file a good fix under "and it
fixed the thing" is strongest when the fix is genuinely good.

### What the error-code finding settled, and what it did not

obSCEne's probe measured `0x8002_0000 | errno` and unheld-unlock-returns-EPERM **on
PS5PCEM**, not on hardware - their caveat, carried into the knowledge entry verbatim rather
than summarised. Both are recorded as assumptions and are now in the probe worklist.

One thing it settles on its own: **it validates a rule made blind.** `GuestError` placeholders
avoid the high bit so they can never be mistaken for firmware values, and there was no
evidence real codes used it. `0x80020001` says they do.

### A gap in the grading vocabulary, raised rather than filled

`published` / `measured` / `guest-observed` / `assumed` has no slot for *measured somewhere
that is not the target*. `measured` means hardware and claiming it would be false; `assumed`
means nobody knows and undersells an observation somebody actually made. It went to
`assumed`, which flattens the distinction obSCEne's own caveat was careful to draw.

Not filled unilaterally: the vocabulary is obSCEne's contract and mirrored here, so a value
added on one side only is the drift the shared grading exists to prevent. Raised on the
bridge. If one arrives it needs the property the others have - naming something that could
contradict it, which a hardware run would.


