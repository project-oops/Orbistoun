# The dispatcher becomes its own crate, and the loop becomes a command


`orbistoun-propose` was two things sharing a name: one proposes **words** and needs a model
runtime, the other turns the **loop** and needs boots. Measured before splitting - each
cluster referenced only its own modules, and the four dispatcher files referenced
`orbistoun_llm` zero times.

`orbistoun-turn` now holds `turn`, `experiment`, `axis`, `trial`. `cargo tree -p
orbistoun-turn` shows six direct dependencies and no path to candle, reqwest or tokenizers -
which is the boundary doing the work rather than a feature flag nobody re-checks, the same
argument `orbistoun-gpu` makes for having no path to `ash` (D293).

`Error` split with it: the dispatcher used exactly one of the five variants.

And the point of the exercise:

```
$ orbistoun-cli turn titles/PPSA02664-app0/eboot.bin --record
8 finding(s), 4 step(s), 3 of them mechanical
  swept: OutParameter { slot: 0, offset: 1048544, answer: Some(0) }
  *** gave it a region at 0x50000000: reached 25 against 23, faulting at 0x0
  stopped: implementing a function is a person writing code

  orbistoun-cli learn sceKernelReserveVirtualRange --library libkernel --known guest-observed     --edge "arg0 is an out-parameter: the guest reads a value back from it and indexes ..."     --assumes "what this function is for is not measured; the name is a label on a hash ..."
```

**Printed, not written.** What a sweep measures is admissible; changing a tracked file stays a
deliberate act with a diff.

### Three of my own mistakes, all the same shape

- **`cargo build --workspace` passed with the new crate not in it.** My check for the
  workspace member was `'"crates/orbistoun-turn"' in s`, which matched the *dependency* line
  further down the same file. A check that could not fail, so a build that proved nothing.
- **`bad()` never existed.** The D280 helper - the whole point of which was that a failing
  gate step reports instead of exiting - was added by a `replace` I did not assert on, and it
  silently did not apply. Every gate failure since printed `bad: command not found` in place
  of its message. Found only because `prose` failed for a different reason.
- **`prose` failed for that different reason: I wrote line-continued string literals**, which
  is D184, a guard that exists because this has shipped garbled output three times. `cargo fmt`
  had already collapsed the ones in `turn.rs`, baking the source indentation into the text -
  so the promoted entry read `indexes          +0xfffe0` and the guard could no longer see a
  backslash to complain about.

Three edits that looked applied and were not, in one unit of work. The lesson is the assert:
every one of them was a `replace` whose result nothing checked.

