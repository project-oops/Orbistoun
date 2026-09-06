# 2026-09-03 - (/loop) Skeletons for the gaps: 61 graphics names and 49 recorded imports

```
declared functions      674  ->  735
unresolved imports      342  ->  286
knowledge entries       655  ->  682 recorded, 654 understood beyond a name
```

Directive was to stop self-blocking and build skeletons. Two gaps had one, and the honest
skeleton in both cases was **names without claims**.

## The current generation's graphics API was not declared at all

D500 found orbistoun modelling `libSceGnmDriver` - the *previous* generation - while guests call
`Agc`. Reading PPSA02664's import table gave the size of it: **fifty-one `libSceAgc` names and
five `libSceAgcDriver`**, plus five more `libSceAgcDriver` names the corpus saw called from other
modules. Sixty-one confirmed names, none declared, six of them being called and reported as
`unknown::`.

Declared, nothing implemented. **Unresolved imports for that executable: 342 -> 286.** D504.

The arity question is settled by precedent rather than by invention: `orbistoun-input` already
documents that *"the names are confirmed, the arities are not, and that asymmetry is
deliberate"*. Checked that `arity` never reaches the call path - it goes to `ImportDesc`, the
CLI listing and the knowledge generator, and nowhere else. So every arity is **6**, the
trampoline's full capture, on the reasoning that with nothing established recording every
argument register loses no information where guessing low discards it.

**Two guards caught me, and both were right.**

- `declared_arity_and_recorded_arity_never_disagree`: *"sceAgcCreateShader is declared with
  arity 6 and recorded with 4"*. The knowledge base already held an arity from a dump (D194).
  The declaration now defers to it - the record is the older claim.
- `every_declared_library_either_serves_something_or_says_why_not`: sixty-one declared,
  none served. `SERVES_NOTHING` existed for exactly this and **was empty until now**; both
  libraries are in it with the reason (Phase 6 has not begun; implementing one to shorten the
  list would be writing the abstraction before its caller).

**And it changed nothing about the run.** Four samples after: 2077/44 and 2080/46, the same
bimodal pair, the same fault at the same address. The guest dies long before graphics, which is
what principle 6 predicts. Worth recording either way - a declaration that *had* moved the wall
would have been a surprise worth chasing.

## Forty-nine called imports had no knowledge entry at all

Computed rather than guessed: of the 433 distinct imports the corpus has seen called, **49
named ones had no entry** (the 53 unnamed hashes excluded - those are the harvester plateau).
Led by `scePadReadState` at 519 calls and `sceKeyboardReadState` at 519.

All 49 recorded, with **name, library, where it was seen and how often, and nothing else**. No
arity, no purpose, no `known_by` - because the field is required only when something beyond a
name is recorded, and nothing beyond a name is. `knows` counts them honestly: **682 recorded,
654 understood beyond a name.**

I got the provenance wrong on the first one and caught it: I recorded
`sceAgcDriverGetDefaultOwner` as `--known measured --cites "obSCEne 106-encoder/related-libs"`,
and no probe measured that symbol - the `related-libs` check measured *libraries* absent. Its
knowledge comes from a guest's import table, which is `found_by = static` and no behavioural
claim at all. Deleted and re-recorded.

## Eleven libraries guests import from are still undeclared

`libSceKeyboard`, `libSceNetCtl`, `libSceVideoRecording`, `libSceJson2`, `libSceCommonDialog`,
`libSceCoredump`, `libSceErrorDialog`, `libSceSaveData_native` and the two now done. That is why
`unknown::sceKeyboardReadState` shows 327 calls beside `libSceKeyboard::sceKeyboardReadState`'s
192 - the same function, split by whether the library resolved.

Not done here: each wants its names read out of an import table the way `Agc`'s were, and
`libSceKeyboard` belongs in `orbistoun-input` beside the pad rather than in a new crate. Next.

## State

`cargo test --workspace` green - 117 suites, **1993 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-353 and D466-D504.
