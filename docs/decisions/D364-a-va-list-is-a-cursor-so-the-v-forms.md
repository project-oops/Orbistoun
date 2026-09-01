# D364 - A va_list is a cursor, so the v-forms render what the register forms cannot


**decided** - 2026-08-29

`vsnprintf` is the most imported name nothing here implemented: twenty-two of the
twenty-five open-toolchain payloads in the local set ask for it, ahead of every other
missing name. It is not decoration - every one of their logging helpers is built out of it,
so a payload that cannot render a message cannot say why it stopped.

### What a `va_list` is on this architecture

Not a pointer walking a stack. The System V AMD64 psABI defines a four-field structure, and
the arguments live in two places at once: the first six integer arguments were spilled by
the *caller's own prologue* into a register save area, and everything past them is on the
stack.

```text
offset  0  gp_offset          u32
offset  4  fp_offset          u32
offset  8  overflow_arg_area  ptr
offset 16  reg_save_area      ptr
```

### The capability this adds

The register-based forms see only what the trampoline caught in registers, so a format with
more conversions than that **cannot be rendered at all** and is refused. Refusing is right
for them - they genuinely cannot see the seventh argument. A `va_list` can, so the `v` forms
are not a convenience wrapper over the others: they are the only ones that can render a long
format correctly. The test asserts exactly that, by rendering the same seven-conversion
format both ways.

### One renderer, two sources

Rather than a second formatter, `render_format` grew an argument *source* - a trait with two
implementations, one over the captured registers and one over a guest's list. Two formatters
would have drifted, and the first divergence would have been a string that differed depending
on which spelling of `printf` the guest happened to call.

### Nothing is written back

`va_arg` advances the caller's list, and a real `vsnprintf` does too. The standard says `ap`
is indeterminate after the call for exactly that reason, so a conforming caller may not read
it again - which makes advancing it unobservable, and makes *not* advancing it the safer of
two permitted behaviours. The cursor lives on our side and no write reaches guest memory.

