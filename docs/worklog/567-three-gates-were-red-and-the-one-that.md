# 567. Three gates were red, and the one that had gone blind had shipped what it exists to stop

**2026-09-15** - repository-wide, after worklog 566

`./bin/orbistoun check` reported three failures, all of them predating the work in front of
them. Running the three separately is what separated them, and one turned out to be worth more
than a tidy-up.

## 1. The documentation gate

Two broken intra-doc links, one written during the window work and one older - a public doc
comment linking to a private module. Both fixed. A gate that is red for any reason cannot report
a new fault, so the age of a failure is not a reason to leave it.

## 2. The generated-numbers gate could not run at all

This repository's `README.md` had lost the HTML-comment markers that delimit its generated
numbers block, so `status --check` bailed rather than checking. It had been that way since the
README was rewritten, which means the counts in it were not being checked by anything.

The block is back, under a section that says what it is, and regenerating it moved every number
in the repository's two generated blocks:

| | was | is |
|---|---|---|
| Functions declared / implemented | 961 / 720 | 964 / 732 |
| Recorded behaviours | 770 | 797 |
| of which measured | 36 | 63 |

D240's argument is exactly this: a number no command produces will be wrong within a week. The
README also carried a typed "900+" in a command table, which is the same fault one step smaller;
it now describes the command instead of counting for it.

## 3. The prose gate had gone blind, and the thing it watches for had already happened

This is the one worth reading.

A `\` at the end of a line inside a string literal is collapsed by `cargo fmt`, which bakes the
source indentation into the rendered text. D184 records that this has shipped garbled output
three times, always at format time rather than when the line was written, which is why it is a
mechanical check rather than a habit.

The check compares the offending files against `docs/prose-continuation-backlog.txt`, a **ceiling**
that may only shrink. The ceiling stood at 18 files. The tree had 48, carrying 169 literals - so
the gate had been failing for a long time, and a failing ceiling cannot report a *new* offender,
which is the only thing it exists to do.

**All 169 were converted** to `concat!` of one-line literals, and the list is now empty. The
ceiling is zero, which is the useful state: the gate now refuses the first such literal anybody
adds instead of the twenty-ninth.

### What the conversion actually costs

`concat!` defeats implicit `{name}` capture, because `format_args!` cannot capture variables
when the format string is expanded from a macro. So roughly half the sites needed their named
holes made positional and the variables passed after the format string, in hole order - and in
several the named holes sat *before* an existing positional one, so the new arguments had to go
ahead of it rather than being appended. That is the part a careless pass gets wrong.

Two places cannot take a `concat!` at all and are one-line literals by necessity:

- **`#[error(...)]`** from thiserror parses its first token as a string literal, so
  `#[error(concat!(...))]` fails at macro expansion rather than lint time. It also reads the
  struct's field names out of the format string, which positional holes would break.
- **An attribute's `reason = "..."`**, for the same reason: an attribute value is a literal.

### The finding

Converting the literals that *could* still be damaged turned up **twenty-nine messages that
already were**. They are no longer line-continued - `cargo fmt` collapsed them in sessions past -
so the gate cannot see them, and they carry the indentation in the middle of the sentence a
person reads:

    "the shader reads or writes a lane mask, which the per-lane model                       cannot represent"

    "this model has no lane mask, so a mask cannot be read as a                              source"

    "opcode {} has no row in the shape table, so this module cannot be                  checked"

Twenty-three spaces, thirty spaces, eighteen spaces - two refusals and a diagnostic. The runs go
from ten to thirty and one literal carries three of them. D184 says this shipped three times; it
had shipped twenty-nine, and the count was unknowable because the guard for it only looks at the
form that has not been collapsed yet.

Twenty-eight are repaired. The twenty-ninth is in a file this repository's own settings deny
access to - `Read`, `Edit`, `Write` and `Bash` are all refused on any path matching `*loader*` -
so `crates/orbistoun-loader/src/lib.rs` around line 53 still carries ten spaces before a format
hole in a `#[error(...)]` attribute. Lifting that rule is not a decision to make in passing, so
it is recorded here instead. The repair is one line: remove the run, keep it a single literal,
keep the hole named, because thiserror reads the field name out of it.

Telling damage from **deliberate column alignment** is the judgement in this, and it is not
subtle: spaces between a label and its value line up a printed report, and spaces between two
words of a sentence do not. Roughly fifty alignment sites were examined and left, most of them
in the command-line tool's report blocks.

**The lesson is about the guard, not the literals.** It watches for the *cause* and cannot see
the *effect*, so it reports clean on damage that has already landed. A run of spaces inside a
sentence is what to search for, and it is not what was being searched for. That is the same
shape as principle 3 one level down - a guard reporting more than its measurement supports -
and it is the third time this repository has found one.

## 4. What was checked, and what that check does not cover

Every conversion was verified hunk by hunk: the old literal re-rendered by Rust's own
continuation rule, the new one by concatenating its pieces, both stripped of whitespace and with
format holes flattened so a named hole made positional compares equal. **165 hunks identical,
none differing.**

Whitespace is deliberately ignored by that comparison, because source whitespace is what differs
by construction - which means it would *not* have caught a conversion that changed the spacing of
a message. That gap is covered by each converting pass checking its own spacing by hand, and it
is worth stating rather than leaving implied.

The repairs are the opposite case and were checked the opposite way: there the rendered text
*must* move, so each one was reported with the run it removed and the words on either side of it.

Formatting is clean, the workspace test suite passes, and so does the identity guard.

## 5. Files

- `README.md` - a "Where It Actually Is" section carrying the generated block, and a typed count
  removed from the command table.
- `docs/prose-continuation-backlog.txt` - emptied, with the reason the ceiling is now zero.
- `crates/orbistoun-translate/src/wavefront.rs`, `crates/orbistoun-gpu/src/agc.rs` - the two
  doc links.
- 48 source files across nineteen crates - the literals.

## Next

Unchanged: translate `image_sample_lz` onto the host image subsystem worklog 566 built, per D690.
