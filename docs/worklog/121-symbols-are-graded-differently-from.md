# Symbols are graded differently from values, and four record kinds were left unparsed


`sym` and `section` are read now. `orbistoun probe` reports which symbols resolved
separately from what functions returned, because **they are not the same kind of fact**.

A return value depends on arguments, on state, and on the part. Existence does not: a name
that resolves resolves, so a `present` from a stand-in still establishes that the name is
spelled correctly and lives in that library - even where nothing it *returns* there can be
trusted for the target. Grading both the same way would either throw away a usable fact or
promote an unusable one, so `SymbolFact` is a separate type and only `Finding` is demoted by
part.

### Surprises

**Four documented record kinds appear in no real output at all.** `call`, `responsive`,
`measure` and `progress` are in the format's own table and are absent from every captured
exchange and from the example report. Writing parsers for them would be transcribing a
document rather than reading evidence, which is what every other table in this project is
built to avoid - so they are deliberately unparsed, and a test pins that an unrecognised
kind is kept verbatim rather than dropped. Nothing is lost by waiting, and the gap is
recorded as a choice rather than left looking like an oversight.

That also retires an idea from the previous entry: filling `arity` and `returns` from `call`
records is not available, because there are no `call` records.

**`presence` is read as an exact word.** Anything that is not `present` is not a claim that
it is - a target answering something this version has never seen is saying something it does
not understand, and reading an unrecognised answer as present would invent the single fact
the record was consulted for.

**The gate grew a check for the mistake I kept making.** `no line-continued string literals`
now fails a file that adds one, against a ceiling that can only shrink - because `cargo fmt`
collapses them and bakes the source indentation into the rendered text. That is exactly the
padded output this session produced four times. The one unlisted offender is
`orbistoun-cli/src/main.rs`, from lines this thread did not write, and it is left rather
than added to the ceiling: putting somebody else's new offence on a shrinking list would
grant the permission the check exists to withhold.

**Splitting `cmd_probe` uncovered a doc comment I had cut in half earlier.** Relocating that
function had anchored on the *last* `///` line of `cmd_shaders`'s doc block rather than its
first, so half of one comment sat above the wrong function for several edits. Restored. The
same anchoring mistake as the `buffer_memory` split this morning, which makes it a pattern
rather than an accident: prose anchors repeat, and the fix is to anchor on the `fn` line.

