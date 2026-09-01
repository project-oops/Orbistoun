# D294 - The first function implemented from a contract the loop measured


**decided** · 2026-08-26 · `sceKernelReserveVirtualRange`, written to the rule rather than to a memory

The loop measured this one end to end (D283, D284) and printed the entry recording it (D291).
Implementing it is step 18 - a person's - and it is the first chance to write that step **to
the provenance rule** rather than merely proposing one: the code may use what the sweep
established, and anything else has to be labelled.

**Measured, and the implementation rests on these:**

- `arg0` is an out-parameter holding a **sixty-four-bit** base. Not four: a `void **` takes a
  word, and the four-byte lesson (D210, D272) is about `int *`, which this is not.
- It must **answer zero** or the guest never reads `arg0` at all - crossed sentinels proved
  the read is gated on the return (D283).
- The region has to be **writable**, because the guest wrote at `base + 0xfffe0` and carried
  on when it was given one. A reservation with no commit behind it would fault exactly where
  the placeholder did.

**Assumed, and recorded as such rather than written as though known:**

- `arg1` is the length. It equals `rdx` at the fault and is the obvious reading, and the sweep
  measured *where the guest faulted*, not how much it asked for.
- `arg3` is an alignment. It is a power of two and sits where alignments sit in the sibling
  call; nothing observed depends on it.
- `arg2` is flags, and is ignored. Nothing measured says what it selects.

**What the rule refused.** The name says "reserve", and a reserve in most virtual-memory APIs
takes address space without committing pages - which would be a natural thing to write, and
would fault at exactly the address the placeholder did. The measurement says the guest writes
there. Where the name and the measurement disagree, the measurement wins, because the name is
a label on a hash and the measurement is a run somebody can repeat.

That is the whole discipline in one function: recalled semantics are a hypothesis, and this
one was **wrong**.

Declared arity four, for the four arguments the implementation reads. The dump shows six
registers because that is how many System V passes, not because the function takes six.

