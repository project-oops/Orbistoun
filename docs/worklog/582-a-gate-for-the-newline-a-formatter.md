# 582. A gate for the newline a formatter never had to collapse

**2026-09-15** - bin/orbistoun and tools, after worklog 581

Three variants of one defect turned up this session, and only the first had a guard.

| variant | what it is | who caught it |
|---|---|---|
| a line-continued literal | `\` at end of line; `cargo fmt` collapses it and bakes the indentation in | the `prose` step (D184) |
| one already collapsed | the damage, sitting in the tree | a search for runs of spaces, written by hand (worklog 567) |
| a literal containing a newline | no backslash at all; the break and the indentation are in the text as written | nothing |

The third has a gate now, and the tree gate is green.

## 1. Why the existing guard cannot see it

`prose` refuses a `\` at the end of a line inside a literal - the *cause*, before a formatter has
done anything with it. A literal that simply contains a newline has no backslash to find, and the
search for runs of spaces cannot find it either, because the run is not on one line: it is a
newline followed by the next line's indentation.

Six such messages were found this session and every one had shipped. A refusal a reader sees with
twenty-nine spaces through the middle of it, three assertions, a print, and a lint's own `reason`
attribute - that last one written deliberately, as an escaped newline followed by fourteen spaces.

## 2. What the gate is careful about

A newline inside a literal is often exactly what is meant. A generated block, a report with a
leading blank line, a multi-line usage message - all of those want the newline, and a gate that
refused them would be refused itself.

So the test is narrow: the break happens **inside a literal**, and the next line is deep
indentation followed by a lowercase word. That is a sentence carrying on, not a new line of
output. Everything else passes.

Deciding "inside a literal" needs no parser, and that is only true because the *other* guard
exists: with no line-continued literals left in the tree, a line carrying an odd number of
unescaped quotes opens or closes exactly one, so a scan that toggles is exact. The two guards
hold each other up, which is worth knowing before either is relaxed.

Character literals are removed before counting, or `'"'` reads as an opening quote and every line
after it is inside a string. A lifetime cannot match that pattern, so it is left alone. Raw
strings are skipped by name rather than half-handled: a miss there is a decision instead of an
accident.

## 3. Watched failing

On a scratch file carrying all three shapes - an ordinary literal, a deliberate multi-line block,
a character literal holding a quote, and one damaged sentence - it reports the damaged one and
nothing else, and exits non-zero.

There is no ceiling file for it. The count started at zero because the repairs came first, and a
ceiling exists to be walked down rather than to be established.

## 4. The tree gate is green

`./bin/orbistoun check` passes end to end. The last thing standing in its way was a
line-continued literal in another session's uncommitted work on direct memory allocation -
converted here rather than left, because this session is what raised the bar the line tripped
over.

`cargo-deny` still reports an advisory against a transitive TLS dependency, which the check
prints as a warning and does not fail on.

## 5. Files

- `tools/prose-newlines.awk` - the detector.
- `bin/orbistoun` - a second step in `prose`.
- `crates/orbistoun-kernel/src/lib.rs` - the literal that was holding the gate red.

## Next

1. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
2. The record's pixel hash, which needs the vertex buffer and the texture the records did not
   capture.
