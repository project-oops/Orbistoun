# D187 - The stub policy reached only declared imports, and a conclusion rested on it


**decided** · 2026-08-21

Three findings, and the order they arrived in is the point.

### The vacuous experiment

Investigating why two titles abort at 53 calls, the first move was the cheapest oracle
available: set `default_return = "ok"`, run, and see whether the guest proceeds. It did not -
identical imports, identical calls, identical abort. **"Return values are not the cause"**
was concluded and recorded.

### `printf` showed the experiment had not run

With `printf` implemented (D186), the guest's own diagnostics appear - and under
`default_return = "ok"` they *still* reported `returned 0x7fff0001`. The functions under
test had never seen the setting.

In `orbistoun-service`, an import the registry could not resolve was skipped:

```rust
let Some(resolved) = self.registry.resolve(Nid::from_raw(import.nid)) else {
    continue;
};
```

So the policy applied only to **declared** symbols - and an undeclared import is precisely
the one worth asking "does the guest proceed if this succeeds?" about, because a declared
one is usually already implemented. The majority of what a guest calls was exempt from the
one knob for asking.

This is D082 and D166 a third time: a setting consulted nowhere, surviving in the branch
nobody re-read. What is new is the *shape of the damage*. It did not produce a wrong answer;
it produced a **confident negative result**, which is worse, because a wrong answer invites
checking and a clean negative closes the question.

### With the policy actually applied

53 calls and an abort became **215 calls** and an ordinary fault far downstream. Return
values were the cause the whole time.

### Overrides by hash, so one function can be asked about

A blanket sweep proves that *something* was answering wrongly and cannot say which. Overrides
were keyed by symbol name, which an unnamed function does not have - so the only available
experiment was the one that changes every answer at once.

Keyed by hash as well, the abort bisects to a single import in one run:
`0x48a758b2e731cfd7` answering success takes both titles to **23 imports and 220 calls, 95%
of them on real implementations**, with the abort replaced by an ordinary null dereference.

That one function's error return is the entire cause of abort-at-53 in two titles.

### And the guest named four more functions

`scePthreadMutexattrInit`, `Settype`, `Setprotocol`, `Destroy` - printed by the title, with
file and line, then **confirmed by hash** rather than believed. Nothing was consulted: the
program under test named them, in this process, and the NID algorithm checked its claim.

Implementing them honestly moved the call count *down*, from 53 to 45, because eight of
those calls were the guest complaining. Removing the complaint removes the calls. The
clearest demonstration yet that a call count is not a measure of progress (D181, D183).

### The next wall, also self-reported

```
tlsf_create: Memory must be aligned to 8 bytes.
```

The D171 shape again - a stub reporting success without writing its out-parameter, so the
guest builds an allocator on whatever the stack held.

