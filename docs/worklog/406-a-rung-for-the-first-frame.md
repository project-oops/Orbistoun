# 406. A rung for the first frame, and a prop that was never counted

**2026-09-04** - (/loop)

## What was done

Two questions that had been sitting unanswered got answered, and both turned into a mechanism
rather than a number.

**A measured answer is no longer a prop (D557).** The compatibility record decided "was this run
helped along?" by asking whether any function was answered by name at all - which counts a
hardware measurement and a wild guess as the same act. The provenance to tell them apart already
existed and was being **thrown away at one line**: `Learned::policy()` read a measurement carrying
`known = "guest-observed"` and produced a policy that carried nothing. It now travels, and
`propped_up` asks whether an entry rests on nothing measured.

**A region was never counted at all.** The count came from `overrides.len()`, and a region lives
in a different map - so a policy that wrote a base into guest memory behind a guessed byte count,
and answered nothing, reported zero and read as an unassisted run. It counts symbols now, answers
and regions together.

**A rung above `Entered` (D558).** `Flipped`: the guest submitted a flip and a real port took it.
D182 refused a rung for surviving to the time limit because *not dying is an outcome, not a
distance* - this is the opposite shape, reachable only after an output has been opened, its
attributes set, its buffers registered and its mode configured, each against real code. Nothing
can spin into it.

**PPSA02664 records `flipped`, 197 imports, 1 frame, honestly measured.**

## The thing worth being careful about

**The guest did not get further.** It dies at `image+0xf56e09`, the same address, the same
instruction. The flip was already happening - D516 made `sceVideoOutIsFlipPending` honest and the
guest submitted its one frame long before today. The ladder simply could not express the furthest
thing its furthest title did. Recording a rung is not the same as reaching one, and this worklog
would be wrong to read as progress at the wall.

## Where the frames may not rank

Below imports, above calls. A guest can sit in its present loop handing the same buffer over for
ever; ranked above imports, that run sorts above one that presented three times and got twice as
far into the engine - D182's failure exactly, one rung higher. Guarded, and the guard was watched
failing with the order swapped.

## The count is read from the port, not from the call list

A submission with a handle this process never issued is refused - and is still a call to something
`implemented`, so counting labels in the trace would have credited a frame the guest never got.
`frames_presented` asks the port table, which is the only thing that knows which submissions were
taken.

## Guards

Eleven, each watched failing, varying a different parameter: provenance dropped in the conversion;
regions excluded from the count; the unlabelled default flipped to `Measured`; `is_evidence`
widened to admit `guest-observed`; `propped_up` reverted to the old rule; `propped_up` disabled
entirely; the rung ordered below `Entered`; frames ranked above imports; frames dropped from the
ranking; promotion driven by the word instead of the count; promotion ignoring the floor.

The D312 guard **failed** when the semantics changed, which is the gate working. It was widened
rather than replaced: it now asserts the old case is still caught, and a second test covers the
new one.

## The stale data this invalidated

Thirty-three compatibility records carried `overrides` and no `propping`, so they deserialised as
*honest* and became incomparable with new runs. Every override written before today rested on
nothing measured - the only learned entry is guest-observed - so `propping = overrides` is a
faithful transcription rather than a guess, and they were rewritten. The generated
`COMPATIBILITY.md` preamble described a ranking and a From column that no longer matched.

## Surprises

**Inserting before a `pub fn` anchor landed between its doc comment and its signature**, silently
handing `implementations`' documentation to the new function above it. This is written in the
machine notes as a known trap and it still happened; clippy's `missing_docs` caught it, which is
the only reason it is not in the tree.

**The workspace test count I reported earlier today was wrong.** A pipeline summing `test result`
lines gave 683; a single clean run captured to a file gives **2056 across 138 binaries**, exit 0.
The failure count was right both times.
