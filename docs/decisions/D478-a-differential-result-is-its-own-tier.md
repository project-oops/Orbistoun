# D478 - A differential result gets its own tier, because it is not a measurement

**assumed** - 2026-09-02 (user-directed plan, before R7)

R7 stands a FreeBSD box beside orbistoun and diffs return value, errno and out-parameter bytes
for the same call. With no console available it is the only live oracle this project has, and
it is unboundedly parallel where the console is one machine.

Before it writes a single record it needs somewhere honest to write it, and `known_by` has four
values - `published`, `measured`, `guest-observed`, `assumed` - **none of which mean what a
differential result means.**

## What a differential pass actually establishes

That orbistoun and a real FreeBSD answer the same thing for those inputs. Two claims are
tangled in that, and only one of them is settled:

1. **orbistoun implements the FreeBSD-analogue behaviour correctly** - verified, mechanically.
2. **the console behaves like FreeBSD** - not verified, and still an assumption.

The second is not a hypothetical worry. **D468 is this project watching it fail**: the ctype
tables were written from FreeBSD's documented bit layout, hardware was measured, and the layout
was different. A differential can agree perfectly and still describe behaviour the target does
not have.

## Why none of the four fits

**`measured` is wrong and would be the worst mistake.** Its own documentation says "measured on
real hardware by a conformance probe", and the tier counts are what this project uses to know
how much of itself is guessed. A FreeBSD box is not the console; filing agreement with it as a
console measurement inflates exactly the number that is supposed to be trustworthy.

**`published` is wrong by being too coarse.** It already covers the FreeBSD tree, so a
differential result is about the same source - but it is a different *kind* of knowing. "Somebody
read the manual page and implemented it" and "a machine ran both and they agreed" would become
the same label, and the whole point of the vocabulary is that they are not.

**`assumed` throws away real evidence**, and `guest-observed` is about a guest running on
orbistoun, which is a different question entirely.

## The decision

A fifth value, `differential`, with the properties chosen deliberately:

- **`needs_citation` is true.** It claims support from outside this repository, so it must name
  the run that produced it - which FreeBSD, which release, which harness invocation. An
  uncheckable claim of external support is worth less than an honest `assumed`, which is the
  rule that already governs `published` and `measured`.
- **`is_guess` is false.** It rests on something that was actually executed.
- **`is_probeable` is true**, and this is the one that matters. A differential result is *not*
  the end of the line: a conformance probe on real hardware can still contradict it, exactly as
  it contradicted the documented ctype layout. So it stays in the worklist that ranks what
  hardware could settle, rather than being counted as finished.

It sits immediately after `published` in report order, because it is a published claim that has
been verified rather than a stronger claim about the target.

## What this costs, stated

Every `match` on `Oracle` has to grow an arm - which is the point of using an enum, and the
compiler finds them all. Two report orderings in the CLI list the tiers explicitly and need the
new one inserted rather than appended, or it prints last and reads as the weakest.

## Status

`assumed`, because nothing has been recorded at this tier yet. It becomes settled the first
time R7 writes one and the tier survives contact with a real run - and the first differential
result that hardware later contradicts is the entry that proves the tier was worth having.
