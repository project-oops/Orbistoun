# D297 - The learned file becomes a record of measurements, so it can be sent to somebody


**decided** · 2026-08-26 · a patch is only worth submitting if the receiver can check it

`learned.toml` began as a local cache: policy the loop worked out, folded in under a person's.
That is enough to run, and not enough to **send**. Someone running orbistoun as a binary, with
no repository and no source, produces exactly the same measurements - and those are worth more
to this project than almost anything else it can generate, because they scale the one oracle
that otherwise cannot.

The project's third oracle is *"the guest itself - a 1-bit oracle per call site. Expensive per
query (a boot)."* Expensive **per person**. A hundred people each turning the loop on titles
nobody here owns is the same oracle at a hundred times the rate, and it costs this repository
nothing and holds no title data.

**What makes it acceptable to receive is that it is checkable.** Accepting contributions to an
emulator is usually a provenance problem - nobody can tell whether a change came from reading
something. A measurement produced by `turn --apply` cannot have: it is derived from running a
binary the submitter owns, it is reproducible by anyone with the same title, and the claim it
makes is **falsifiable by a command**. That is precisely the standard CLAUDE.md sets - *"writing
a fact down means committing to a checkable claim about where it came from"* - and it is a
stronger contribution model than "trust the diff".

So the file stops being settings and becomes evidence:

```toml
[[measurement]]
function = "sceKernelReserveVirtualRange"
measured = "PPSA02664"     # which guest, so a reader knows what it is true of
on       = "2026-08-26"
by       = "orbistoun 0.1.0"
known    = "guest-observed"
evidence = "conformance-check"
answers  = "ok"
assumes  = ["0x200000 is a guess: the sweep measured where the guest faulted, not what it asked for"]
writes   = { slot = 0, region_bytes = 0x200000 }
```

Every field is something the loop already produced and **threw away** - `measured`, `assumes`
and `evidence` were printed to a terminal and lost. The policy the emulator runs under is
*derived* from these, which keeps the distinction the project cares about: a measurement is a
claim about a guest, a setting is a decision about a machine, and one is submittable.

**`measured` is load-bearing.** `region_bytes` was established against one title; another may
index further. An entry that did not say which guest it came from would read as a fact about
the platform, which it is not.

**`--verify` is what makes receiving one safe.** Re-derive locally, compare, report agreement -
the same shape `audit --repair` already has for names. A maintainer without the title cannot
check an entry, and does not have to: it is accepted as `assumed` and promoted to
`guest-observed` when somebody who owns the title confirms it. That ladder already exists.

**This property lasts exactly as long as the fixes are data.** A tier-3 contribution is Rust,
and Rust arrives as a diff that a person reviews - back to ordinary code review, with none of
this. Which is one more reason to keep tiers one and two as wide as they will go (D296).

