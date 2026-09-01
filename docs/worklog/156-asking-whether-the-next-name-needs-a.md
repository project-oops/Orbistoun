# Asking whether the next name needs a word or a shape


`tests/shapes.rs` answers it. The trick is measuring against names the grammar did **not**
find - harvested from module strings or seen in a trace - because the ones it did find are
necessarily spellable and measuring against them measures the search.

Of 183 vendor-shaped independently-found names: **28 reachable, 121 needing a shape, 34
needing vocabulary**. Shapes are the binding constraint by more than three to one, which
confirms with numbers what the naming side predicted from the other direction (D261).

**The first cut was wrong.** Counting all 464 independently-found names gave a 68%
vocabulary gap - but 276 of them are POSIX or libc names that every vendor pattern, all of
which start with the `sce` prefix, structurally cannot spell. Scoped properly the ratio
inverts. Worth remembering: a measurement over the wrong population is not a small error, it
points the other way.

**Three shapes cost almost nothing and spell fourteen names today**: `prefix-learned-verb`
at +0%, `prefix-module-learned-learned-verb` at +4%, `prefix-learned-verb-learned-learned`
at +11%. The rest of the ranked list runs to +1862% and beyond, and those are names the
generator should not be asked for.

**And a consequence nobody had costed** (D262): repeated `learned` positions were ruled out
by D195 because squaring a big list is unaffordable. `learned` fell from 12,255 entries to
177 today for unrelated reasons, and that same shape went from 12.2 trillion candidates to
2.5 billion - 4,800 times cheaper. Shrinking a vocabulary list bought shapes that growing it
never could, and nothing in the tooling would have said so.

