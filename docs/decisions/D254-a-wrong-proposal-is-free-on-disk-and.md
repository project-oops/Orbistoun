# D254 - A wrong proposal is free on disk and expensive in the loop


**decided** · 2026-08-25 · measured over thirty-six rounds

`round()` grows a *local clone* of the grammar with the words it is about to try, sweeps,
and discards it. Only words a confirmed name was built from reach the bank. The comment
beside it called this "what makes a wrong proposal genuinely free".

It is free on disk. It is not free in the loop. Nothing remembered a failure, so the model
re-proposed the same words every round and the sweep re-ran them: **`Group` was accepted and
swept against thirty-five million candidates twelve separate times**, and `Node` nearly as
often. The measured yield of a thirty-six round run - three names - sat entirely in the
first round of each position, and the remaining thirty-three rounds largely re-tried what
had already failed.

`Vocabulary::tried_before` holds every word swept this run. It is deliberately **not** the
bank: the bank is evidence and the hash is the only thing that puts a word in it, so a
failure must be remembered for the run and forgotten by the file. Two tests pin both halves,
and the first was confirmed to fail against the old code.

`Refusal::AlreadyTried` is separate from `AlreadyKnown` because they state different facts -
one says the vocabulary has the word, the other says it does not and it was tried anyway.

