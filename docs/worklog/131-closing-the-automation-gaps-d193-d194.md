# 2026-08-22 - Closing the automation gaps (D193, D194, D195)


Three capabilities, in the order they unblocked each other.

**Names out of the titles' own bytes (D193).** `sceKernelCreateSema` blocked two Unity
titles and the generator could not spell it - the vocabulary held `Semaphore` and the vendor
wrote `Sema`, so 2.58 *billion* candidates were tried in seventy-five seconds and the real
name was never among them. The string was in a *third* title's data. Scanning module bytes
for identifier-shaped runs names 22 imports of one title in a single pass, against zero from
the full generated sweep. Implemented, both titles go from 45 calls and an abort to **222
calls at 96% standing**, with no override.

**Argument dumps (D194).** For any import nothing implements, the first two calls, arguments
pointing into mapped memory only: 32 bytes captured at call time. `sceAgcCreateShader`
immediately showed an out-parameter, a header carrying magic `"1234"` and a size, and shader
bytecode - the shader submission calling convention, with nobody choosing what to look at.

**The grammar widens itself (D195).** Confirmed names' parts are written into `vendor.toml`,
so the gap that blocked one title cannot block the next. Seeded with 175 words the grammar
could not previously spell.

### What is now automatic, and what is not

Scanning, hash-confirming, merging into the tracked database, auditing provenance, dumping
arguments, ranking findings, comparing runs, and widening the grammar all happen with nobody
present. Writing an implementation does not, and neither does interpreting a dump - `magic
"1234", then a size field` was a person reading hex.

Two beliefs corrected along the way: the symbol database already merged rather than
overwrote (a hand-merge was wasted), and auto-declaring a confirmed name buys nothing, since
declaration only gates attaching an implementation and whoever writes one edits the same
file twenty lines away.

### And a process failure worth recording

A background `names` job was left running for a long time, holding the release binary's
lock, so a verification of the new feature ran against a stale binary and reported `466 ->
466`. That would have read as "the feature does not work". It was caught only because a line
that should have been in the output was missing.

The same shape as every other failure this session: **a broken run producing a confident
result**. The tell was an absence, which is the hardest thing to notice.


