# D024 - Other projects are reference-only, and get credited

**decided** · 2026-08-19

Never lift code wholesale from another project. Where prior work makes something
understandable, credit it in [ACKNOWLEDGEMENTS.md](../../ACKNOWLEDGEMENTS.md) in the
same change and write the implementation independently.

The distinction that matters: reading another project's prose, design notes, or
public documentation to understand a format is ordinary engineering. Reading its
source and reproducing the structure is not - that is the convergence problem in
D014 arriving by a different route.

Recording what was consulted is *better* provenance hygiene than silence, because it
makes the question answerable later. An uncredited influence is what makes it
unanswerable.

