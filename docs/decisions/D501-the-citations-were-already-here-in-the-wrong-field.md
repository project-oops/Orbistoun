# D501 - The citations were already here, in the wrong field

**measured** - 2026-09-03 (a controlled comparison inside one comment block)

Fourteen entries moved from `assumed` to `published`. **No new source was consulted for any of
them** - the citation was already written in the implementation's own comment, and the knowledge
generator could not see it.

```text
348 published, 265 assumed  ->  362 published, 251 assumed
```

## The comparison that names the cause

`crates/orbistoun-libc/src/math.rs` adds ten float functions in one block, from one standard, in
one style. The first line read:

```rust
// `acosf(x)` - ISO/IEC 9899 7.12.4.1.
```

and every line after it read:

```rust
// `asinf(x)` - 7.12.4.2.
```

leaning on the block header, which names the standard once. In the knowledge file:

| | known_by | cites | purpose |
|---|---|---|---|
| `acosf` | **published** | `ISO/IEC 9899 7.12.4.1.` | - |
| `atanf` | assumed | - | `7.12.4.3.` |

**One block, one generator, and the only difference is six characters.** `orbistoun-gen
knowledge` takes a citation from the line describing the function, tests it against a list of
standard names, and a bare clause number matches none of them - so the clause number landed in
`purpose` and the entry was recorded `assumed` with the note *"Derived from the implementation's
own documentation, which cites no published specification."*

Which was false. The documentation cited a specification; it cited it one line up.

## Fixed in the comments, not in the guard

The generator's rule exists to stop it manufacturing provenance, and its test
`documentation_that_cites_nothing_is_assumed_rather_than_published` is the guard that keeps it
honest. **Teaching it to accept a bare `7.12.4.3.` would have been exactly the manufacturing it
forbids** - the number means nothing without the standard's name, and a later reader of one line
would have no way to check it.

So the twelve comments now name the standard, which is better documentation independently: a
reader of one line no longer has to scroll to a header to know what `7.12.4.3.` is a clause of.

Two more, `strtoimax` and `strtoumax`, said `C99 7.8.2.3` - the same standard under a name the
list does not carry. Spelled as `ISO/IEC 9899`, which is what it is called, they cite their
exact clause too.

**Verified end to end rather than asserted**: the fourteen entries were deleted and the
generator rewrote them. All fourteen came back `published`, thirteen with the exact sub-clause.
`hypotf` came back with the bare standard name because prose followed the clause on the same
line, so that line was split in two and it regenerated exact.

## What is left is correctly assumed, and that is the more useful half

251 remain, and the expectation that a large share are citation work is **wrong**. Ninety-nine
of them are `libScePosix`, and their assumption is not a missing reference:

> That the POSIX name and `scePthreadMutexLock` are the same behaviour rather than merely
> similar. Unmeasured - it is inferred from the names and from both being exported by one
> platform.

**No standard settles that.** POSIX says what `pthread_mutex_lock` does; it says nothing about
what the vendor's export of that name does. Citing POSIX here would record agreement with a
document that was never in question and hide the claim that is. This is D468's lesson - the
ctype tables were written from FreeBSD's documented layout and hardware differed - and
relabelling ninety-nine entries would have been that mistake at scale.

The rest are the same shape or genuinely unspecified: Dinkumware C-runtime internals
(`_Cnd_*`, `_Mtx_*`, `_Getpctype`), Itanium-ABI mangled throw helpers (`_ZSt*`), and seven
vendor `libkernel` functions with no published analogue at all.

**So the assumed count is close to honest, and the fourteen were bookkeeping.** That is worth
knowing before anyone budgets a week to "cite the assumptions".
