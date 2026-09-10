# 472. The canary had a name

**2026-09-09** - directed, continuing 471

PPSA21564's fault report named the region a faulting register pointed at - D639's work - and named
it `f7uOxY9mM1U#r#n`, which is the module's own base64 spelling of the hash. A reader cannot tell
from that whether the guest is holding a C++ vtable, a stdio object or the stack canary.

It is `libkernel::__stack_chk_guard`, and the report says so now.

## Two faults behind each other, again

**The wrong database.** The service resolves hashes through `self.symbols`, and a worker process
builds its `Service` with none - the database is loaded per run. The first attempt compiled, ran,
and renamed nothing.

**The right names broke the map.** `DataBlocks` kept one name-keyed map answering two questions.
Encoded spellings carry a per-module suffix, so three modules importing `__stack_chk_guard` were
three distinct keys; resolving them to one name collapsed three entries to one, and the register
then printed as a **bare number** - worse than the encoded label. `labels: BTreeMap<u64, String>`
now sits beside `named`, and `data_symbol_at` asks the address-keyed one (D647).

```text
before:  r12 -> data f7uOxY9mM1U#r#n+0x0 = 00 00 00 00 00 00 00 00 …
after:   r12 -> data __stack_chk_guard+0x0 = 00 00 00 00 00 00 00 00 …
```

## Surprises

- **The improvement was briefly a regression, and one run showed it.** Resolving the names made
  the label worse before it made it better. Nothing about the change looked risky; the map's
  keying was the risk, and it was invisible until real names collided in it.
- **A lookup that cannot fail fails silently.** `self.symbols` in a worker is always `None`, so the
  fallback fired every time and the output looked like "the database does not know this name". The
  second time in two days - `Knowledge::library_of` did the same thing on a different question.
- **`__stack_chk_guard` is zero.** Not this fault's cause: a zero canary compares equal to itself
  and simply protects nothing. Worth its own entry, and deliberately not claimed here.

## Next

- The zero canary, on its own terms.
- PPSA21564's actual wall: a null base read at `+0x38`, inside a C++ function-local static
  initialiser (`__cxa_guard_acquire` two calls earlier) that is building a thread with an affinity
  and a scheduling policy.
- The three filed mesh requests.
