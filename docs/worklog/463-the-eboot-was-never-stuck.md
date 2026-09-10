# 463. The eboot was never stuck

**2026-09-09** - directed, continuing 462

Bus idle a twelfth pass. Picked up the question 462 set out to answer and never reached.

## It finishes

`obscene` is filed in the frontier as *"ran to the time limit"*, which reads as a hang. Its own
record stream says otherwise:

```text
OBS|tally|521|9|6|19
OBS|end|sceKernelWrite
```

**521 passed, 9 failed, 6 partial, 19 skipped, suite ended.** It then runs to the limit because
`obs_screen_present` does what its comment says - *"With a display, stay and show it"* - and
orbistoun gives it one. The payload exits because it has none.

The most complete run in the corpus was filed under an outcome that reads as a failure.

## The eboot leg had never been diffed against the eboot leg

Every differential so far: local **payload** against console **pkg**. The eboot has its own
hardware leg (D638):

| local | against | passed there, failed here |
|---|---|---|
| payload | pkg | **106** |
| eboot | eboot | **1** |

The one is `900-surface/control`, the weak symbol that should be null and is bound (D633).

**370 checks go the other way**, fail-to-pass - orbistoun passing what the console's sandboxed
eboot could not attempt. That is reach, not correctness, and D622 already drew the right conclusion
for the payload in the opposite direction. What like-for-like buys is the *other* direction: among
checks both legs ran and concluded differently, exactly one is orbistoun failing where the console
passed.

## Four of the six failures are the console's too

| check | orbistoun | console |
|---|---|---|
| `010-kernel/is-stack` | fail | **fail** |
| `018-relational/handle-fits-its-out-parameter` | fail | **fail** |
| `110-modules/info-size` | fail | **fail** |
| `110-modules/names` | fail | **fail** |
| `137-kernelcall/system-version` | fail | skip |

The last is honest: the console could not build a syscall gadget and never asked; orbistoun has
one, asked, and its dispatcher refused the number with `-ENOSYS`.

## Surprises

- **I nearly "fixed" `is_stack` to pass a check the console also fails.** It ignores its first
  argument, the check requires two answers to differ, and the fix looked obvious. Its own doc
  comment records that the console answers `0` to a local *and* to a static across twenty-three
  runs, and that the bounds are the answer. Making them differ would have invented a distinction
  the platform does not make, to turn a check green - D227 exactly. What stopped it was reading the
  thirty lines above the function.
- **Two passes running, the thing worth finding was in an ordinary run of a guest nobody was
  investigating.** 462 found a live regression that way; this pass found the corpus's best result
  mislabelled as a hang.

## Next

- The blocked list from 461, unchanged.
- `900-surface/control` is now the *only* behavioural defect on the like-for-like comparison, which
  raises the value of the relocation scope question (D633).
