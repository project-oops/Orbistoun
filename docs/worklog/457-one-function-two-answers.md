# 457. One function, two answers

**2026-09-08** - directed, continuing 456

Bus idle a sixth pass. Local work.

## The inconsistency four differential entries were sitting on

Four entries said the same thing in the probe's words - *"libSceKeyboard loaded but no symbols
resolved"*, and the same for libSceMouse, libSceVideodec2 and libSceAjm - for libraries orbistoun
**declares and stubs**.

The cause is one line of table-building. `sceKernelDlsym`'s by-name table is `symbols::resolvable()`,
the *implemented* set, while imports get a stub for every *declared* name. **949 declared, 680
implemented.** So 269 names answer differently depending on how the guest asked, and
`sceKernelDlsym`'s own doc comment claims the opposite - *"a function reached this way and the same
function reached by an import are the same address"* - which is true of two-thirds of the table.

One payload run asks by name for **266** of them.

## Measured, not argued

The case against is real: a guest handed a stub calls it and gets a placeholder, where a guest
handed null may take a fallback it would have preferred. That is not settled by argument, so it
got a flag - `ORBISTOUN_DLSYM_STUBS`, `Effect::Intervenes`, and the table it installs is empty
unless a run asks, which is what makes two runs comparable (D632).

The payload: **223 -> 226 imports, FURTHER, still exits cleanly.** And because an intervention that
moves a wall is not a diagnosis, the second observation - the probe's own verdicts against the
console:

| | baseline | under the flag |
|---|---|---|
| concluding differently | 255 | **240** |
| passed there, failed here | 115 | **106** |
| distinct findings | 17 | **13** |

All four *"no symbols resolved"* entries gone; the absent-library census 99 -> 94.

**Three titles byte-identical.** PPSA03416, PPSA02664 and PPSA04263: same imports, same calls, same
standing, same fault address, flag or no flag. They resolve by import, so this cannot reach them -
which was the expectation and is now the measurement.

## Left as a flag, deliberately

Nothing got worse and one thing got better. That is evidence, not proof: four guests is a small
population, and the guest the case against describes - one that prefers a null it can branch on -
is exactly the kind that would not be in a corpus built from guests that already run.

Changing what `dlsym` answers by default is a user-visible behaviour change. The flag, the numbers
and the recommendation are recorded; the default stands until somebody decides.

## Surprises

- **The doc comment was the bug report.** It states the invariant this violates, in the same
  function, and has done since the resolver was written. Nobody read it against the table it
  describes.
- **The titles could not have been affected and nobody had checked.** Running them was three
  minutes and turned "this should be safe" into "these three are identical".

## Next

- Whether `ORBISTOUN_DLSYM_STUBS` should be the default - a decision, not a measurement.
- The six title-module imports (D630), still the largest wall, still waiting on scope.
- The declined-syscall casualties, waiting on `REQ-20260908T1620Z-4c1e`.
- `sceAgcCreateShader`, waiting on `REQ-20260908T1621Z-7a5d`.
