# D296 - Three tiers of automatic fix, and why the third is the smallest


**decided** · 2026-08-26 · scaffolding all of it before building any of it

The loop measures a contract, satisfies it, proves it helped, prints it - and stops, because
the thing it would write next is code. The remaining question was how to close that, and the
answer that survived is **not "have a model write Rust"**.

| tier | who proposes | what it emits | oracle | rebuild |
|---|---|---|---|---|
| **1** | a rule | a policy entry | the finding that produced it | no |
| **2** | a model | a policy entry | `FURTHER`, then the probe | no |
| **3** | a person, or a model | Rust | the conformance probe | yes |

**The output being *data* is what removes the person, not the model being good.** A policy
entry is one line in a file that is already a runtime input: blast radius is one line, undo is
deleting it, and the loop re-runs in seconds. That is why it can run unattended. Rust is none
of those things, and no amount of model quality changes any of them.

**Tier 3 is therefore the smallest, not the largest.** Anything expressible as an effect
belongs in tier 1 or 2, where it costs no rebuild and reverts cleanly. What is left for Rust
is real logic - a pseudo-random sequence, a symbol lookup, formatted output - and those are a
minority, which was measured rather than assumed: of the six gaps the probe still reports, one
is effect-shaped and five are logic.

**The acceptance test is where this goes wrong if it goes wrong.** `FURTHER` means the guest
executed code it could not reach before; it does **not** mean the behaviour is right, and
principle 3's opening sentence is exactly this failure - *"a stub that returns success is
indistinguishable from working code until forty thousand frames later"*. A `copy n bytes` with
the wrong `n` very often produces `FURTHER` and corrupts state that surfaces somewhere
unrelated. So:

- a trial that only changes a **return value** may be accepted on `FURTHER`;
- a trial that **writes memory** may not, and needs a conformance check covering it.

**Where the learned entries live.** A separate `learned.toml` beside `config.toml`, merged at
load, losing to anything the config states explicitly. Three properties, each of which is the
reason:

- deleting the file is a **complete undo**;
- a diff shows the loop's guesses and a person's decisions **separately**;
- a person's entry **wins**, so nothing the loop writes can quietly override a deliberate one.

**What tier 2's vocabulary would be, when it is built.** A closed set of effect shapes - return
a constant, return an argument, write a base, zero *n* bytes, copy *n* bytes, advance a counter
- and not a language. Each shape is something a rule can propose *and* a probe can check.
An open grammar is a program in a data file, which is what principle 5 avoids rather than
achieves (D295).

Its value is front-loaded and mostly already spent: it would have covered `memset`, `memcpy`,
`strlen` and much of the string library, all of which were written by hand this session. Worth
sizing against a title nobody has worked on before building it.

