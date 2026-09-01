# 2026-08-24 - The loop, written down; and a documentation audit (D197, D198, D199)


**Done.** `docs/THE_LOOP.md` - what one turn of the work actually does, start to finish,
in one-sentence steps, with three mermaid diagrams and a table saying which steps are
automatic and which still need a person. Then a sweep of every markdown file and every doc
comment in the tree for claims that had stopped being true.

### Why the loop needed its own document

`WORKFLOW.md` was the closest thing and it is a command reference: what to type, in what
order, how often. It never says *why the sequence is that sequence*, so somebody reading it
learns the tool without learning the method. The two are now split - `THE_LOOP.md` owns the
method, `WORKFLOW.md` owns the commands, and each links the other.

Three edges are worth having drawn rather than described:

- **Naming feeds back into itself.** A confirmed name is split into words and widens the
  grammar, so the search gets cheaper every time it succeeds. That is the one part of this
  project that does not scale linearly with titles, and it was invisible in prose.
- **The findings replace the diagnosis step, not the fixing step.** Worth stating plainly,
  because "the tool tells you what to do" invites the reading that it does the work.
- **Steps 17 and 18 are a person.** Marked in red on the diagram and repeated in the table.
  There is no plan to generate implementations unverified, and the document says so.

### The audit found more than staleness

Some of it was ordinary drift - the root README opened with "runs nothing" while six titles
executed guest code, the crate table listed 14 of 31, `orbistoun-cli/README.md` said "two of
three commands work" against twenty commands. But three findings were structural:

**`orbistoun-trace` has no dependents.** The whole crate. When execution landed, the
recording that was actually needed got built where the calls are - a fixed-size atomic ring
in `orbistoun-thunk`'s dispatch path, drained into `orbistoun-report`'s `CallTrace`. Its
README claimed it was "wired at phase 4, alongside execution", which never happened. Two of
its ideas are genuinely still needed - per-thread sequence numbers, and a binary file sink -
so it is not simply deletable, and that decision is now a backlog entry rather than a
silent assumption that the spine has a tracing layer in it.

**`Container::imports` was dead and its comment was a lie.** It returned
`VendorUnsupported` under a doc comment reading "Not implemented. This is the single piece
of work that turns this crate into a usable import dumper" - while the crate's `dynamic`
module parsed imports perfectly well and had done for weeks. Nothing called it but its own
test. Removed, along with the error variant only it used.

**`orbistoun_kernel::register` was a second copy of the registration path.** Nothing called
it; `modules()` in `orbistoun-service` is the single list. That is precisely the drift D123
was written about, sitting in the tree with the decision that forbids it already recorded.
Removed, and both READMEs and the `guest_module!` doc now point at the real list.

Six documents also carried mangled text from a blanket trademark scrub - "shadthe
previous-generation console", "vendor command-stream command streams", "Audio output - the
audio-output library". Those had been readable-looking enough to survive several passes.

Nine crates had no README at all, including the three largest test suites in the workspace.
Written.

### Numbers now come from the tool

Every figure in `PROJECT_STATUS.md` was hand-counted and several had drifted: 137 observed
names against a real 154, 219 unaccounted against 202, 677 unnamed imports against 565.
The implemented-function count was 68 in three documents and 69 in the tree - miscounted
because one entry in `implementations()` spans two lines and does not look like the others.

`orbistoun-cli symbols` now marks implemented functions with `*` and prints
`95 declared, 69 implemented (*), 26 on stubs`. The document cites the command. A number no
command produces is a number that will be wrong within a week, and this one proved it.

### The guard that reported success while seeing nothing

The line-continuation check was blind twice over, and both failures printed reassurance.
First it used `git grep`, which searches the index, in a repository with no commits. Fixing
that revealed the second: the output was piped through a malformed `sed` expression which
aborted, emptying the offender list - so nothing could be reported as unlisted, and every
file on the ceiling was reported as having stopped offending. The visible symptom was the
second one, which reads as good news.

The ceiling went 8 to 21. That is the direction its own header forbids, and the header now
says why. D184 - which recorded the eight, and which congratulates itself for testing the
guard by making it fire - has a correction note under it.

**The general form is worth keeping:** a check whose failure mode is a false pass is worse
than no check, because no check leaves the question open and a false pass closes it.

### Also

- `cargo clippy --workspace --all-targets -- -D warnings` had been failing on an
  undocumented `unsafe` block in the dispatch path - the `// SAFETY:` comment sat above the
  `if` rather than above the `unsafe`. `./orbistoun.sh check` now passes end to end, 806
  tests, zero failures.
- Five doc comments citing D197 for the argument-dump work were moved to D198, which is
  where that work is actually recorded.
- Every relative markdown link in the repository resolves: 145 checked.
- `__pycache__` and `*.pyc` are gitignored. Four `.pyc` files are already in the index and
  need removing from it - not done here.

