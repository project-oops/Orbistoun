# D122 - Knowledge is a file, and it is the loop's output

**decided** · 2026-08-20 · at the user's direction

There are exactly two kinds of fact about a guest function, and conflating them was
costing us the expensive one.

**Derived** - which pattern produced a name, at which index, on which day. A search makes
this, `symbols/generated.json` holds it, and that file is **overwritten on every search**.
Anything irreplaceable stored there is destroyed on the next run.

**Known** - arity, what the arguments mean, what the function is for, what it does at its
edges. No tool can produce it. Only observation can, and each item costs an experiment.

The second kind had no home. `sceKernelDirectMemoryQuery` had its argument layout, its
ignored return value and its buffer-clearing requirement all established by measurement,
and every one of those facts lived in decision-log prose recoverable only by grepping.

### Not a third place - one fewer

The objection was duplication, and it was the right question. `guest_module!` already held
knowledge: an arity, compiled into a macro invocation. That is precisely what principle 5
says should not happen - "if answering what this function does requires a rebuild, it is
in the wrong place".

So the count goes from two knowledge locations to one, with a test asserting the macro and
the file never *contradict*. They may each be incomplete; they may not disagree.

`guest_module!` was left alone rather than rewritten, because the graphics crates use it
and another session is working in them.

### Why it cannot merge with the generated database

That file is regenerated several times a session. A curated fact stored there would be
silently destroyed on the next search - the volatile-results failure, arriving by a route
nobody would look at twice. One file is disposable, the other is not; they share a key,
which is not duplication.

### Written by tooling, not only by hand

"Hand-maintained" was the wrong framing. This is what the loop *produces*: run a title,
watch a function, learn something. `orbistoun-cli learn` appends a finding as a command,
merging rather than replacing - recording one edge case must not erase a purpose
established three sessions earlier. `orbistoun-cli knows` reads it back.

### Small decisions inside it

- **Unknown arity is `None`, not zero.** Zero is a real answer a trace renders very
  differently, and collapsing them makes an unmeasured function look measured.
- **A bare entry counts as recorded but not as understood.** The gap between those two
  numbers is the size of the job, and one count would flatter both.
- **The NID is not stored.** Derived from the name, so a file cannot hold a pair that
  disagrees with itself - the same rule `docs/SYMBOLS.md` sets for symbol databases.
- **Title identifiers, never paths.** Enough to repeat a measurement; nothing that
  implies a tracked module.

