# D662 - Category is a permission, not a label

**Status:** published
**Date:** 2026-09-10

## Orbistoun granted every title everything

Execution on the target is governed by two orthogonal axes: a **privilege tier**, carried as the
authority id in the container header, and an **application category**, declared by a title in its
own `param.json` as `applicationCategoryType`. The category decides three things a guest can
observe - how much direct memory the kernel grants it, whether it may own the video scanout, and
whether it runs alongside other titles or excludes them.

Orbistoun modelled neither. Searching the tree for the concept finds nothing but the GUI's shell
categories. Every title ran as though it were the most privileged kind.

**That is wrong in the direction that hides bugs.** A guest denied direct memory on the console
gets it here, proceeds past the point where the real machine stopped it, and fails somewhere else
entirely - and the report names the second place. The same asymmetry the conformance differential
turned up six times over: orbistoun passing where the console fails is more dangerous than the
reverse, because the divergence is silent.

## The table, and where it comes from

SELFish's `docs/features/writing.md`, which documents what each category costs because it stamps
them into packages:

| declared | name | direct memory | scanout |
|---|---|---|---|
| `0` | big app | full budget | exclusive primary |
| `3` | daemon | system pool | none |
| `65536` | system app | **zero** | denied, `0x8029_0001` |
| `131072` | mini app | restricted | compositor layer |
| `262144` | media app | media budget | protected path |

Reading a sibling project's own notes is ordinary engineering; the provenance boundary is about
other people's *source*.

**Every value is `published`, not `measured`.** No probe has asked the console to confirm that a
system app is granted zero bytes. It is recorded at that grade so it can be promoted by a
measurement rather than believed because it is written down.

## An unrecognised category is refused, not defaulted

`Category::Unknown(n)` is its own variant, and it is granted neither the scanout nor direct
memory. Folding it into the default would grant everything on the strength of a number nobody
recognises - which is the permissive direction again, one level further down. The negative test
pins exactly that, because it is the branch that would never announce itself.

## What is modelled and what is enforced

The type, the mapping and the three questions are modelled. **Nothing is enforced yet**, and the
distinction is deliberate: the corpus declares `0` for all three titles that have a `param.json`,
so no enforcement path would be exercised, and wiring a refusal nothing runs is how a guard gets
written wrong and stays wrong.

The requirement that a big app ships `sce_module/libc.prx` is reported rather than enforced for a
stronger reason: nothing measured says what the console does when it is absent, and refusing to
run a title over a documented requirement would be inventing a verdict.
