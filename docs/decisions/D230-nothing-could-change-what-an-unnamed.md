# D230 - Nothing could change what an unnamed function answers, so an elimination had never been measured


**decided** · 2026-08-25 · found by re-testing something already recorded as eliminated

`StubPolicy` is keyed by symbol name and carries a `u32`. Both are right for what it is - a
human-editable file of established error codes. Both are fatal for the question at a wall:

- The function on the biggest wall **has no name**, so it cannot be keyed at all. An
  override written for it matched nothing and fell back to the default.
- A region base is **64-bit**, which a `u32` cannot express.

So the run that established "the return value is not where the base comes back" was a run in
which nothing was overridden. It reported no change, and no change was the only thing it
could report. That went into `PROJECT_STATUS.md` as one of seven eliminations "by
measurement rather than by argument" - the exact claim it could not support.

`ORBISTOUN_RETURN=<import>:<value>` forces a 64-bit answer, matched by name where there is
one and **by hash where there is not**, on its own layer consulted before the policy's. A
separate layer rather than a second `install_stub_returns`, because that is a `OnceLock` the
service has already set - a second install is a silent no-op, which is this same bug again.

**The count is the point.** The first run under it reproduced the original result exactly:
fault unmoved, `rax=0`. Identical output to the broken experiment. What separates them is
`(1 answered)` in the conditions - the diagnostic proving it ran. Without that line the two
runs are indistinguishable, and one of them is a measurement while the other is nothing.

The return value is now **genuinely** eliminated.

### The four registers

The fault printed `rax`, `rcx`, `rdx`, `rdi` - "the four that carry an address or a size in
almost every fault worth reading". All sixteen were captured, recorded in the trace, and
serialised to disk; only the last step threw them away. The question at the wall was *which
register held the base that should not have been zero*, and the four cannot answer it.

Printing all sixteen took one function. It immediately showed `rbx=0x20`, `r14=0x20`,
`r15=0x10` - the `0x20` header offset sitting in a register, where it had been every run for
weeks - and narrowed the missing base to `rax`, `rsi`, `r8`, `r11` or `r12`.

A run had to be repeated to learn something already in its own trace. Where a value is
already recorded, the cost of showing it is nothing and the cost of hiding it is a run.

### Two traps found on the way out of it

Both are the same defect wearing different clothes - the window knowing something and
declining to say it.

**A settings file that fails to parse falls back to defaults**, deliberately, so nothing
half-written gets overwritten (D153). But the fallback includes the library folder, so the
library panel then reports a folder nobody chose while the explanation sits in a status
line inside the preferences window. It now says *settings not loaded* above the scan
result, which is where the confusion actually happens.

**"rescan library" does not re-read `config.toml`.** It re-scans the folder named by
settings already in memory - correct, and it reads as though it should have picked up a
hand edit. Hand-editing that file is a supported way to work, since it is how every
setting in that window was reached before the window existed, so *reload settings file* is
its own item. It discards unsaved form state rather than merging a file with a form:
anything else leaves nobody able to say what the settings now are.

