# A controller subsystem, and a name I talked myself out of


Asked for controllers - configurable count, real pads, keyboard mapping - and for those
inputs to reach the emulator so the shell could be tested by pressing its own button.

The host side is built and works: buttons named by position, four configurable ports, a
per-port key mapping held as text so no window toolkit leaks into the input contract, and a
pure tap-versus-hold type that takes elapsed milliseconds rather than reading a clock. That
last one made every edge an assertion instead of something found by holding a button until
it looked wrong - a hold fires once, fires while still held, and a release after one is not
also a tap.

Verified by pressing it rather than by reasoning about it: holding for 1.1 seconds opened
the power menu, with "quit the title" correctly absent because nothing was running.

### The mistake worth keeping

`orbistoun-input` declared six pad functions and implemented none. Before implementing, the
names were checked against the symbol database and **none of the six was in it**, while
ninety-one other names from the same library were.

From that I concluded they were previous-generation names - recalled rather than derived -
and therefore hashed to NIDs no import would ever carry. I rewrote the declarations to the
confirmed `...Ext` siblings and wrote a confident paragraph about shims that could never be
reached.

Then:

```
orbistoun-cli imports titles/obscene/eboot.bin
```

Ninety-seven pad imports, including **both** halves of every pair. `scePadOpen` and
`scePadOpenExt`. `scePadReadState` and `scePadReadStateExt`. The library exports both and a
real module asks for both.

What the error was made of: absence from a *generated* database was read as evidence about
the platform. That database holds what the naming loop has derived - a name missing from it
means nobody here has named it yet, which is a fact about this project. The direct evidence
was one command away, and it is the command `CLAUDE.md` opens with.

### Two guards earned their keep

Registering the implementations tripped `every_implemented_function_is_written_down`, which
wanted a knowledge file - so `libScePad.toml` now records provenance for all nine. And
`orbistoun_input::implementations()` had never been added to the service's list at all: the
module was registered, the functions were not, and the comment directly above that list warns
about exactly this - *"code that looks written and never runs."*

### What it is worth

obSCEne already had a `100-input` probe section. It now reports three passes where the
recorded table had one pass and one failure, and the best of the three is
`close-rejects-bad-handle` - a negative test, asserting a bad handle is refused.

### Review from a parallel implementation

Prosperous grew a controller model for its own reasons and read this one alongside it. Three
findings, all real: a keyboard could not move a stick at all (the shipped configuration is
one keyboard port, so out of the box it could press seventeen buttons and drive nothing
analogue), cross-port key conflicts were invisible because the seen-map was built inside the
per-port loop, and consequently there was no safe way to add a second keyboard player.

Their cancellation rule is the good one and is now here: opposite pushes sum and clamp, so
left and right held together mean centre - a position a stick can actually be in. Letting the
first win would make the pair mean something no pad can express.

### The prediction was wrong, and the reason is the finding

Expected the next `names` run to grow `learned` to about 5,592 words, now that the quadratic
shapes are off and 906 million candidates fits under the ceiling. **It grew by nothing.**

613,445 candidates across 54 modules, **0 named**. The vocabulary is fed the parts of names a
run *newly confirms*, not the candidates it tries - and the corpus is already fully named, so
nothing was offered and `learn_words` was never called.

The 11,842-word list came from the era when a large batch of static names was first confirmed.
`parts_of` splits on capitals, so `_ZN8Document9terminateEv` yields `Document9terminate` -
which is exactly where the mangling fragments came from, and it only happens on names being
confirmed for the first time.

**So the ceiling is not currently binding at all.** It bites on a fresh corpus or a large new
naming run, not on the next command - and the worry about 5,592 junk words landing was
misplaced. D342 said the opposite and is corrected in place rather than left standing.

The instrument was right and my model of it was not, which is the reverse of the day's usual
failure and worth the same amount.

