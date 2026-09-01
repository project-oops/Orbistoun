# D257 - A name that does not say how it was found cannot be used


**decided** · 2026-08-25 · found by not being able to answer the question

`orbistoun-cli names` tries four sources - module strings, runtime dumps, published word
lists, the generated grammar - and printed one sorted list of hits. A name harvested from a
module's own bytes and one the grammar produced were spelled identically.

That was tolerable while every module was a commercial title. Pointed at another emulator's
binary it stops being tolerable, because *which source* decides whether the name may be used
at all (D242), and the output could not answer it. Confronted with 29,403 hits there was no
way to tell the 299 the grammar earned from the 29,336 read out of somebody else's census.

Each hit now prints how it was found, and the run ends with a count per source - so "the
sweep named 29,403" can never be read as "this repository can account for 29,403".

`Solved` carried the derivation the whole time, recorded at the moment of discovery (D073).
Only the last step threw it away, which is the same defect as the sixteen registers a fault
already held and did not print (D230).

