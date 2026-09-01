# D328 - A promotion generated from a measurement, and the one field it invented


**decided** · 2026-08-27 · the generator gap closed, and caught inventing within a minute

`learned.toml` is one machine's cache; a knowledge file is what the emulator **ships**. So the
change a measurement is asking for is an entry in one, and the loop can now write it:
`submit export` emits a unified diff per unpromoted measurement, into `patches/`, described in
`patches.toml` beside it.

**Every field comes from the measurement, and that is what keeps a generated change clear of
principle 1.** Nothing is recalled, so nothing can be recall dressed as reasoning - which is
the objection that actually applies to generated code, rather than the verification one
(D322). The entry is deliberately partial: no `purpose`, no `arity`, no argument list, because
a sweep measured none of those and filling them in from the name is the exact move the
provenance rules exist to stop.

### The field it invented anyway

The first draft wrote `found_by = "generated"`. Applied, it failed
`the_shipped_files_account_for_everything_they_claim` inside a minute:

```
sceKernelReserveVirtualRange: found_by = generated but symbols/generated.json re-derives it as static
```

`found_by` says how the **name** was found - harvested, generated, supplied. A measurement
establishes how the **behaviour** was. They are different questions and the generator answered
one with the other, in the same function whose doc comment says it invents nothing.

It is omitted now. What that costs is nothing: the field is optional, and a measurement that
does not know something should not say it.

**Worth keeping because of how it was caught.** Not by review - the wrong line had a
confident comment above it - but by a guard somebody wrote long ago that re-derives every
name and compares. The generator is only safe to the extent the guards around it are real,
and this is the first evidence that they are.

### A measurement now records its library

`from_finding` takes `libkernel::sceKernelReserveVirtualRange` and keeps only the second half,
so nothing downstream could say which knowledge file an entry belonged in. The obvious lookup
cannot help: `library_of` is built *from* the knowledge files, so it places a function that
already has an entry and never a new measurement - exactly the case that needs placing.

The loop knew and was dropping it. It is a field now.


