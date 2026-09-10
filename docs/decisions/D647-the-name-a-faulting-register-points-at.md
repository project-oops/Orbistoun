# D647 - The name a faulting register points at

**Status:** measured
**Date:** 2026-09-09

## What the report said, and what it meant

PPSA21564 faults reading `0x38` - a null plus a field offset - and its report printed:

```text
r12 -> data f7uOxY9mM1U#r#n+0x0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
r13 -> data f7uOxY9mM1U#r#n+0x0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

D639 got the region named, which was the hard half. The label is the module's own spelling of the
symbol - a vendor module encodes every import as base64 of the hash plus a library and module id -
and a reader cannot tell from it whether the guest is holding a C++ vtable, a stdio object or the
stack canary. Those three want completely different answers, and the run report's whole job is to
be the thing that answers rather than the thing that has to be decoded by hand.

It is `libkernel::__stack_chk_guard`. Nothing about the printed label says so.

## Two faults, and the second only appeared once the first was fixed

**The database was the wrong one.** The service resolves a hash through `self.symbols`, and a
**worker process builds its `Service` with none** - the symbol database is loaded per run, because
names belong to the run, which is exactly what `import_labels_with` already documents. Reaching
for the service's copy compiled, ran, and renamed nothing. A lookup that cannot fail always fails
that way, and this is the second time in two days (the first was `Knowledge::library_of` answering
about documented functions when the question was about declared ones).

**The by-name map is lossy, and resolving the names is what made it lossy.** `DataBlocks` kept one
`BTreeMap<String, u64>` serving two questions: *where does `optarg` live* (an implementation
writing a guest global) and *what lives at this address* (a fault reporter). Encoded spellings
carry a per-module suffix, so three modules importing `__stack_chk_guard` produced three distinct
keys and the map happened to hold all three. Resolving them to one name collapsed the three to one
entry - and the run then printed a **bare number**, worse than the encoded label it replaced.

That regression was visible in one run, which is the only reason it was caught rather than shipped
as an improvement.

## One map per question

`DataBlocks` now keeps `labels: BTreeMap<u64, String>` beside `named: BTreeMap<String, u64>`, and
`data_symbol_at` asks the address-keyed one. A repeated name labels every page it was given; a name
lookup still answers the one address it can. Neither map is asked a question it cannot answer.

```text
r12 -> data __stack_chk_guard+0x0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

## What this does not claim

The canary being zero is **not** the cause of this fault. A zero canary compares equal to itself
and protects nothing, which is a real gap and is worth its own entry; the fault is a read through a
null base at `+0x38`, and the two registers merely hold the guard's *address*, which is ordinary
stack-protector code. Naming the region says what the guest was holding. It does not say what
killed it, and the report does not claim it does.

## The guard was made to fail

The test asserts on the **second** page of a repeated name, because the first was already working -
and it asserts that `named()` really does hold one entry for two pages, so the reason the second
map exists is stated in the test rather than in a comment nobody reads next to it.
