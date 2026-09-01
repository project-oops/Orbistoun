# D342 - A shape can be disabled, and the two names that cost


**decided** · 2026-08-27 · asked whether a shape was a waste; the data said something stronger

`prefix-module-verb-learned-learned` had produced 0 of the 323 generated records and its
mirror 2. That is weak evidence on its own - a shape produces nothing when the *vocabulary*
lacks the words, which says nothing about the shape.

`tests/shapes.rs`, run against the corpus, says what the constraint actually is:

| of 8,417 vendor-shaped names found without the grammar | |
|---|---|
| reachable under the current patterns | 426 |
| blocked on a missing **shape** | 1,025 |
| **not splittable into known words at all** | **6,966** |

Vocabulary is roughly seven times the constraint shapes are. And `learned` appearing twice
makes a round quadratic in it, so at the 2.6-billion budget (D330) those two shapes are what
caps the list:

| | `learned` ceiling |
|---|---|
| keeping both | **483 words** |
| dropping them | **16,042 words** |

**So they are not a waste, they are self-defeating**: about 145 forecast names, paid for with
a vocabulary cap that leaves 6,966 names unsplittable. D262 shrank the vocabulary to make
these affordable and that was sound *given the shapes*; what nobody asked was whether the
shapes were worth the shrink.

### Disabled, not deleted, and the reason is mandatory

`PatternSpec::disabled` is an `Option<String>` and **presence is the disabling**, so a shape
cannot be switched off without saying what it cost and what would bring it back - the rule
`CompatEntry::reason` already carries. A bare `false` is what a file of unexplained exceptions
is made of.

Kept in the file because the forecast is real: this is a shape that is *early*, not wrong. It
comes back when the vocabulary is large enough that these stop being the binding constraint.

**Validated before it is filtered.** The first version skipped disabled shapes before
resolving their parts, which would let one carry a vocabulary name that does not exist - and
the error would surface only when somebody re-enabled it, at the moment they have least
context for it.

### The price, and it is exactly what was predicted

The audit stranded two names immediately:

```
sceAudioPropagationPortalDestroy
sceAudioPropagationSystemDestroy
```

`sce` + `Audio` + `Propagation` + `Portal`/`System` + `Destroy` - the 2 of 323 the record
count named, confirming the analysis rather than surprising it. They are on the ceiling with
that reason, and they return the moment the shape does.

### The consequence expected, and what actually happened

The prediction was that the next `names` run would grow `learned` to about 5,592 words, now
that 906 million candidates sits under the 2.6-billion ceiling. **It grew by nothing**, and
the reason is worth more than the prediction was.

The vocabulary is fed the parts of names a run **newly confirms**, not the candidates it
tries. That run tried 613,445 candidates across 54 modules and named **0** - the corpus is
already fully named - so nothing was offered and `learn_words` was never called.

The 11,842-word list came from an era when a large batch of static names was first confirmed:
`parts_of` splits on capitals, so a mangled symbol like `_ZN8Document9terminateEv` yields
`Document9terminate`, and that is where the fragments came from. Re-running against a corpus
that is already named repeats none of it.

So the ceiling is **not currently binding**, and bites on a fresh corpus or a large new naming
run rather than on the next command. Worth having, not worth worrying about now - which is the
opposite of what this entry first said, and the correction is the useful half.

