# 577. An image instruction is not always eight bytes, and the decoder believed it was

**2026-09-15** - orbistoun-shader and orbistoun-translate, after worklog 576

**Every instruction in the shader fixture corpus now translates: 184 of 184, no blockers.** The
last one was `image_sample_l`, and going to measure the one fact it needed turned up a decoder
defect that had nothing to do with it.

## 1. The fact it needed

Worklog 576 established that the level is one **extra address register** - the assembler refuses
an address operand whose register count does not match the instruction's dimensionality, so a
levelled two-dimensional sample takes three registers where a level-zero one takes two. What was
left was where among the three the level sits.

Compiled rather than looked up. A program calling the levelled sampling intrinsic with three
arguments that arrive in known registers:

```
image_sample_l v0, v[0:2], s[4:11], s[12:15] dmask:0x1 dim:SQ_RSRC_IMG_2D
```

No moves at all - the arguments go straight in, in the order the intrinsic names them. So the
level is the **last** address element, after the coordinates.

## 2. The control, which is where the defect came from

"No moves" only means something if the compiler would have emitted moves for a different order.
So the arguments were reversed, and the compiler did something else entirely:

```
image_sample_l v0, [v2, v1, v0], s[4:11], s[12:15] dmask:0x1 dim:SQ_RSRC_IMG_2D
```

It named the address registers **individually** rather than as a range, in preference to moving
anything. That form exists on this generation, and a reference compiler reaches for it on the
first program written to ask.

## 3. The instruction is then longer than the table said

The extra register numbers go in dwords appended to the instruction. Measured, by assembling the
same instruction both ways:

| form | first byte | field at 2:1 | bytes |
|---|---|---|---|
| `v[0:2]` | `0x08` | 0 | 8 |
| `[v2, v1, v0]` | `0x0a` | 1 | 12 |
| six addresses | `0x0c` | 2 | 16 |
| nine addresses, 3D | `0x14` | 2 | 16 |

Length is eight plus four times that field. The encoding table said `width_bytes = 8`, full stop.

**Reading a twelve-byte instruction as eight starts the next decode four bytes inside this one**,
and the rest of the shader decodes as garbage from there. That is precisely the failure
`literal_operands` exists to prevent for the other variable-length case, and the encoding row
carried no equivalent for this one.

An encoding now carries an optional `extra_dwords` field - a count, where the literal case is a
flag - and the image family names it. The two tests that pin it were watched failing with the
field commented out: the twelve-byte instruction decoded as two, and the four-instruction stream
came apart.

## 4. Why nothing had caught it

Every image instruction this project has ever decoded came from a probe file or a captured
shader, and all of them use the consecutive form. The corpus is not wrong; it simply never
contained the other form, and nothing asked whether it could.

It took writing a control for an unrelated measurement to produce one. That is worth noticing:
the control existed because worklog 565 recorded a probe that lied, and the habit it left behind
- run the known-good shape through the same probe - is what found this.

## 5. Where the corpus stands

```
shaders      11 of 14 translate
instructions 184 of 184 translatable
no blockers - every instruction seen is supported
```

**Three shaders still do not translate, and no instruction explains it.** The census tries the
compute, fragment and mesh stages and takes any success, so it is not a stage mismatch either.
That is the next question and it is a well-formed one: which three, and what refuses them.

## 6. Files

- `crates/orbistoun-shader/src/encoding.rs` - `extra_dwords` and the length it contributes.
- `crates/orbistoun-shader/data/encodings.toml` - the field on the image family, with the four
  measured encodings that establish it.
- `crates/orbistoun-shader/tests/image_length.rs` - both halves, watched failing.
- `crates/orbistoun-translate/src/model.rs` - `image_sample_l`: three address registers, the
  level read from the last.

## Next

1. The three shaders that translate at no stage with every instruction supported.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
3. A translated `image_sample_l` on a device. The instruction translates; nothing has drawn with
   it, and the level being the last address element is measured from a compiler rather than from
   a picture.
