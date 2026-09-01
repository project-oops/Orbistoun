# D188 - The shipped symbol database is loaded unless told otherwise


**decided** · 2026-08-21

`symbols/generated.json` holds 407 names, is committed, and is re-derived by CI on every
push. Nothing loaded it. `--symbols-db` was opt-in and defaulted to *no database at all*, so
every run reported hashes the tree could already name.

`printf` and `memalign` were both in that file while the corpus reports listed them as
`has no name` and advised:

> extend the candidate vocabulary and re-run the name search

Work already done, in a file already committed, that nothing read.

### Why this is worse than a missing feature

The findings are the output this project exists to produce (D179), and they were
**confidently recommending the wrong next action**. That is the same failure as a stub
reporting success, one layer up: not an absence a reader can notice, but a positive claim
that happens to be false.

It cost real time in the session that found it. A twenty-minute name search was run to
discover `printf` - which was sitting in the committed database the whole time - and the
search then dutifully reported it as a new name.

### Where the default belongs

The first fix was in the CLI's own resolution, which was the wrong place and did nothing:
the *worker* loads names from a path, and the CLI passes the path rather than the loaded
database. A shim resolving a default the layer below ignores is principle 13 again.

The default now lives where the naming happens, so both shims get it, and a supplied path
still wins - a database under construction has to be testable before it is committed.

Embedded via the file at the workspace root rather than a copy inside the crate. One file,
so the audited database and the loaded one cannot diverge; the cost is that
`orbistoun-nid` no longer builds outside its workspace, which nothing here needs it to.

