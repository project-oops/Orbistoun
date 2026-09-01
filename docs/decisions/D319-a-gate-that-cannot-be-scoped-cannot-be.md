# D319 - A gate that cannot be scoped cannot be run in a shared tree


**decided** · 2026-08-27 · an hour of finished work sat unverified behind somebody else's crate

`check` compiles, lints and tests the whole workspace. That is right for the question it
usually answers - *is this tree sound* - and it makes the gate unrunnable the moment more than
one session works in the tree, which is now ordinary here. A crate half-written by another
session does not compile, so `cargo check --workspace` fails, so **nothing can be verified**,
including work that never touched it.

`check --only "<crates>"` narrows the cargo steps. The static gates - provenance, decision
numbers, prose, generated numbers, the symbol audit, the tables - always run whole-tree,
because none of them compiles anything and all of them are about the repository rather than
a crate.

**The verdict is the part that had to be got right.** A scoped run never prints
`all checks passed`. Green is what a person scrolls to and reads as permission, and letting a
subset borrow it would build this log's recurring failure - reporting more than the
measurement supports - into the gate itself. It says which crates passed and states plainly
that the rest was not compiled, linted or tested.

### The first draft exempted formatting, and was wrong within a minute

The reasoning was that `cargo fmt` needs no compilation, so another session's half-written
crate cannot stop it. False: it reads every file in the tree and fails on an unformatted one
whoever wrote it. The very first scoped run failed on exactly that, having asserted in a
comment that it could not.

**A step that cannot pass for reasons outside the scope makes the scope useless**, which is
the general rule and the one the exemption missed. `cargo doc` had the same problem and the
same fix.

Worth recording rather than quietly correcting: the wrong reasoning was written down as a
justification, in a comment, in the same change that the run then contradicted. The comment
was the more confident of the two.

### And `-p` turned out not to be a scope at all

Found the same way, one day later: a scoped run failed on `crates/orbistoun-libc`, a crate the
scope excluded.

**clippy lints every workspace crate it compiles from source.** `-p orbistoun-cli` therefore
lints most of the tree, because the shim depends on nearly all of it - so the scoped gate was
reporting another session's finding as the scoped crates'. Worse than not scoping: it attaches
somebody else's failure to your name. `--no-deps` confines it, and is used only when a scope
is given.

**What is still whole-tree, deliberately.** The static gates - provenance, decision numbers,
prose, generated numbers, the symbol audit, the tables - never scope, because each is a fact
about the *repository* rather than a crate, and a scoped run that skipped them would claim
something it had not checked. A scoped run can therefore still be blocked by a file nobody in
the scope touched. That is a limitation of the design and not a bug in it, and it is written
here so the next person meets it as a known edge rather than a puzzle.

So the honest description of `--only` is: **the cargo steps scope, the repository checks do
not.** The first draft of this entry implied more than that.


