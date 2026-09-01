# 2026-08-27 - The tags outlived the instruction that removed the thinking


`/no_think` and `--reasoning off` were both working. The reasoning *content* was suppressed
and the model emitted `<think> </think>` regardless, so every reader downstream saw a tag
where an array should have been and scored the engine zero (D336).

`engine::without_reasoning` strips them, applied by the two engines that cannot configure
the model - in-process and command-line. An unclosed block returns nothing rather than the
narration, because a reply cut off mid-thought is working rather than conclusion.

Found only because the failure message started quoting the reply. "Answered, but not with a
list of words" was true and useless; one line of what it said held the whole diagnosis.

**And it changed no score.** 4 of 12 before and after - the loop's reader already fell back
to bare tokens and was coping with the markup. Recorded because a fix that fixes nothing
measurable is exactly the kind that gets remembered as having worked.

Four runs now, and the ladder is stable: Claude Code 9, 10, 7, 8; the in-process 1.7B model
4 and 4; the 4B on the accelerator 2 every time.

