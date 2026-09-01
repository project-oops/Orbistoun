# D092 - Report rendering lives in the library

**assumed** · 2026-08-19

`orbistoun-shader::report` renders the coverage summary and the ranked worklist. The
CLI will call it; so will the run report.

Principle 13. What a coverage report *says* is a property of the analysis, not of
whichever surface asked for it, and two surfaces that render it independently will
eventually disagree about it.

Two constraints the module holds:

- **Byte-identical output for an unchanged corpus.** No timestamps, no paths, total
  ordering everywhere. These reports are diffed between runs, and anything that
  changes for no reason teaches a reader to ignore the diff.
- **Truncation is announced.** A list capped at ten that stops silently reads as the
  whole list, and the reader concludes there is less work than there is.

The headline is *complete shaders*, not an instruction percentage. Partial support for
a shader renders nothing, so an instruction-level number flatters progress to anyone
tracking it.

