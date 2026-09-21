# 740. The fault source is a null-base offset, not a count — and the trace now shows copy operands

**2026-09-20** — a question about consulting reference emulators turned into a tooling change that
corrected an eleven-worklog misreading of this wall in one run. The `0xa8` everyone (me included,
719 onward) took for the *count* of a copy from a null container is the copy's **source pointer**;
the count is `0x50`. The correction came from orbistoun's own trace once it was made to print what
it had recorded all along.

## The oracle was consulted, generated a hypothesis, and measurement discarded it

The prompt was whether to read a reference emulator for hints on the stuck descriptor trace. The
answer honoured the standing rule ([[emulator-is-oracle-not-source]]): a reference predicts what a
correct measurement returns; it is never a fact orbistoun transcribes. prosper is already that kind
of oracle here — credited in `ACKNOWLEDGEMENTS.md` as an independent PS5 emulator and cited ~33
times in `libSceAgc.toml`, always to corroborate or refute a number measured independently, never
as a source.

prosper's AGC notes (`docs/AGC_IMPL_PLAN.md`) do carry a real hit: a shader header with ASCII magic
`"1234"` and size fields `0x18`, `0x108`, `0xa8`, which is `sceAgcCreateShader`'s `arg1`. That was a
clean hypothesis — that `0xa8` is a shader register-default block. **Native evidence refuted it for
this title.** PPSA02664's call census on the faulting run contains **no `sceAgcCreateShader`** at
all; the `0xa8` here is not a shader-header size. orbistoun took nothing from prosper, and the
hypothesis it suggested did not survive contact with the measurement. That is the discipline working
as intended, and it is the honest answer to the question: consult the oracle for hypotheses, then
let the machine that is actually running the title decide — here it said no.

## The tooling that decided it, and why it was missing

The call trace printed each recorded call as `label(arg0) -> ret from site`. For a byte copy `arg0`
is the destination, which is valid by the time the copy runs; the source and length live in `arg1`
and `arg2` and were **recorded but never shown** — the same blind spot D570 fixed for a placeholder
handed on as a *size*, left open for the copy family. So a copy *from* a null source — this
project's commonest graphics wall — was invisible in the trace, and eleven worklogs read the copy's
count off the mid-copy register dump (`rdx=0xa8`) instead, which is memcpy's *remaining* count, not
its argument.

`traced_line` now appends the operands for the copy family: `src <arg1> n <arg2>` for
`memcpy`/`memmove`, `n <arg2>` for `memset` (its `arg1` is a fill byte, not a pointer). Values only,
never a cause (principle 3). A test pins both directions — a copy grows the operands, an ordinary
call does not. It is in `orbistoun-report`, the shared formatter, so the finding evidence the loop
reads carries it; the CLI and GUI tail printers still collapse runs in their own code and are a
principle-13 follow-up.

## What it showed, in the first run after the change

```
just before: libc::memcpy(0x…f070) src 0xa8 n 0x50   from 0x…42ebd   <- faults
just before: libc::memcpy(0x…f068) src 0x6000007fc2f8 n 0x8 -> …     <- the one before, fine
```

So the faulting copy is `memcpy(dst, src=0xa8, n=0x50)`. **The source is `0xa8`** — a near-null
pointer, `0 + 0xa8` — and `read of 0xa8` is reading *from* it. The count is `0x50`, not `0xa8`. The
successful groups just before copy 8 bytes from valid **stack** sources (`0x6000007fc2f8`); the
faulting group copies `0x50` bytes from a null-based object at offset `0xa8`. Group nine is a
different, larger kind of register group, backed by a heap object whose base is null.

## Ruling out the obvious next guess, natively

The value `0xa8` is also what the phantom GetSize (`0x7d86501b8094ef57`) writes as the register-block
size, so the tempting reading is "the size is being used as a pointer". Tested with the one-bit
oracle (principle 5): change the phantom to write `0xb8`, re-run. The faulting copy stayed
`src 0xa8 n 0x50` — `src` did **not** track the phantom's value. So `0xa8` is a fixed structural
offset from a null base, not the phantom's size mis-used. The phantom is correct and stays; the
experiment was reverted.

## Where this points next

The target is now sharply defined and different from what 719–739 chased: a heap object whose base
is **null**, at least `0xf8` bytes, whose `+0xa8` field is the source of a `0x50`-byte register
group — where groups zero through eight source from the stack and succeed. The question is which
creation or lookup returned that null object, not "which field of a descriptor nobody filled". The
copy-operands line makes any future instance of this class — a copy whose `src` is null or near-null
— legible in the trace on the first run, without the register-dump misreading that cost this wall
eleven ticks.

## Gate state

Changed `crates/orbistoun-report/src/diagnose.rs` (copy operands in `traced_line`, with a test);
`agc.rs` experiment reverted to `0xa8`. The change is green in isolation — `cargo test -p
orbistoun-report` is 101/101, including the new operands test. `./bin/orbistoun check` is red, but
on three generated-doc drifts (`status --check` → `PROJECT_STATUS.md`; `compat markdown --check` →
`PPSA02664-app0.md`, `COMPATIBILITY.md`; the `frontier` test → `compat-frontier.txt`) that all
render from `compat/PPSA02664-app0.toml`, which a concurrent session has edited (+10/−4) without
regenerating the derived files. Measured, not assumed (the diff is in that record, not in this
change); regenerating another session's live compat edit would race it, so it is left for that
session. Identity scan clean. No commit.
