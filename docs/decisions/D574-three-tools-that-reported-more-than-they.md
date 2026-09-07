# D574 - Three tools that reported more than they measured, on one afternoon

**Status:** measured
**Date:** 2026-09-07

Principle 3 says a tool is as capable of plausible output as a stub. Three did today, and each
had the D220 shape - one fact, held at one site, consulted at another that did not know.

## The launcher ran nothing and reported it

`./bin/orbistoun run PPSA25872-app0` printed `running  (limit 20s)` - the module blank - then the
tool's usage error, which `|| true` swallowed, then *what this guest asked for* over **zero
traces**. `resolve_module` fell off the end of a `for` loop whose exit status is success, so a
title it could not find resolved to an empty string and the verb carried on.

The title was not under `titles/` because it is not: the tool keeps its library in the platform
data directory, and `orbistoun-cli paths` says so, but the launcher only ever looked at the
repository's own convenience directory. Fixed twice: `resolve_module` returns failure when nothing
matches, and one `titles_root` rule serves `run`, `names` and `sweep`, falling back to wherever
the built tool says its library is. `run` builds before it resolves, because resolving may ask.

**The first argument the guest sees is still a host path.** `orbistoun-worker` builds it as
`/app0/{module}` with the module exactly as the caller spelled it, under a comment that says it
must not be the host path. A guest printed the whole of one today. Measured not to be the cause
of PPSA25872's crash (D573) and left alone here: it is the process description, and it deserves
its own entry with the argument for what a console actually puts there.

## The tail line hid twelve of thirteen addresses

`print_wall` collapses a run of identical labels into one line and shows the first call's first
argument. D566 read `(0x740000754a38) -> 0x7fff0001 x48` as "one address, eleven million times".
The run was forty-eight calls over **thirteen** addresses. The line now says
`x48 on 13 distinct first arguments` whenever the calls in a run did not share one, and still
shows the first.

## The decision counter counted a generated index

`./bin/orbistoun decide` reserved the next number by grepping `## D` headings out of
`docs/DECISIONS.md` - which has been a generated index with no such headings since the log was
split into files. It would have found nothing, added one, and reserved D1 by appending a heading
to a file the next index regeneration overwrites. It now counts the files under `docs/decisions/`
and writes the reservation as a file with the same slug rule as the splitter.

## What this does not establish

**That these were the only three.** Each was found by being run today, on a machine whose cache
directories had been emptied. A verb nobody ran this week may have the same shape, and nothing
here audits them.
