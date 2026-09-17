# 579. A flag written nowhere was refused, and `null` means less rather than unsupported

**2026-09-15** - orbistoun-translate, after worklog 578

Worklog 578 named the one real gap among the three shaders that translated at no stage: the
`arith` fixture's division pre-scale wrote its flag "to a destination this translator does not
know as a lane mask".

The destination was `null`.

## 1. What the fixture actually says

```
0x000018 12 v_div_scale_f32 v1, null, 0x40400000, 0x40400000, v0
0x000034 12 v_div_scale_f32 v3, vcc,  v0,         0x40400000, v0
```

Two of the same instruction: one writing its flag to the condition mask, and one **declining to
write it at all**. The second translated and the first did not, and the message said the
translator did not recognise the destination - which was true and was the wrong thing to say
about it.

`null` is the architecture's "nothing here". It is the same name a flat access uses to say it has
no base register, and this repository already knows it: `FLAT_NO_BASE` is that string, corrected
to it from a wrong guess two worklogs ago (worklog 565). The two are now one constant, because
they are one thing - a field saying it holds nothing.

## 2. Asking for less is not asking for something unsupported

A shader writes its flag to `null` when it wants the scaled operand and not the flag. The
division pre-scale produces both; a shader doing an ordinary divide needs one.

Refusing that is refusing a shader for being *simpler* than the translator can handle, which is
backwards. The translation now drops the write and keeps everything else - the flag is still
computed, because it falls out of the same selects as the value and there is nothing to gain by
skipping it. **Only the write is conditional.**

The same applies to carry arithmetic, which had the same shape and the same refusal, so both
sites take it.

It is worth being precise about what is *not* dropped. An ordinary register pair as the flag
destination is still refused: that names a place this translator cannot write, where `null` names
no place at all. One declines the result and the other asks for something unimplemented, and a
carry silently going nowhere is an address that is wrong only sometimes.

## 3. Where the corpus stands

```
shaders      12 of 14 translate
instructions 184 of 184 translatable
  FURTHER  12 of 14 shaders complete (+1)
```

**The two that remain are both decisions.** The sampling fixture uses more than one texture,
which D690 refuses so a frame is not drawn from whichever was bound; `unreached` reads one
attribute both interpolated and flat, which a host input cannot be. Neither is a gap, and the
corpus is now as complete as the project's own choices allow.

## 4. The test, and the trap in writing it

The obvious test - "it translates now" - proves nothing about whether the value survived. So the
test uses the **wide-spread branch**, one of the two that sets the flag, and asserts three
things: the scaled value matches the same instruction writing its flag to the condition mask, the
comparison fixture really does set the flag, and the condition mask is untouched.

The middle assertion is the one that keeps the third honest. Without it a fixture that never
flagged anything would leave the mask at zero for a reason that has nothing to do with the
change, and the test would pass while checking nothing.

## 5. Files

- `crates/orbistoun-translate/src/model.rs` - `NO_DESTINATION`, `discards`, and the two
  conditional writes.
- `crates/orbistoun-translate/tests/execute.rs` - the flag dropped, the value kept.

## Next

1. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
2. A translated levelled sample on a device: it translates, and nothing has drawn with it.
3. The two remaining fixtures are refusals by decision, so the corpus has nothing further to say
   about instruction breadth. What it cannot say anything about is whether a *real* shader hits
   either refusal - and the oracle records are two shaders, which is a small sample.
