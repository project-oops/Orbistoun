# 584. A captured command stream draws a frame, which is the last join in the GPU path

**2026-09-15** - orbistoun-gpu-vulkan, after worklog 583

```
packets 357, shaders found 2, translated 2, failures []
```

and then a frame.

Nothing had ever gone from *a guest asked for this* to pixels. Both halves existed and had never
met: `orbistoun-gpu` walks a captured stream, finds the shader addresses its register writes
name, reads those shaders out of guest memory and translates them - and stops, because that crate
deliberately knows no graphics API (principle 12, enforced by the absence of a dependency). This
crate draws translated modules, and every test that did so reached into the payload image at an
offset written in the test.

So the submission pipeline's output had never been drawn, which is its whole purpose.

## 1. What makes it a claim rather than a rearrangement

**Which module is the geometry and which is the shading comes from the stage the pipeline
attributed**, not from an offset this test knows. The test reads the `BindShader` commands the
submission emitted and takes the modules they name.

That matters because the register vocabulary those addresses come from is, by its own comment,
the least certain table in the GPU crate. It is transcribed. A frame drawn from what it found is
the first evidence that what it found were **shaders** rather than plausible numbers - the oracle
test could say the addresses resolved and the shaders translated, and neither of those would
distinguish a correct table from one that happened to land on something decodable.

## 2. What it needed that did not exist a day ago

The window. A submission could not say where guest memory sits until `Pipeline::with_window`
(worklog 571), so every shader it translated got a window at address zero and refused every
access. A vertex program that fetches its vertices would have drawn nothing, and the report would
have said it translated.

That is worth restating because it is the shape this whole sequence keeps producing: a component
that reports success for something nothing has run.

## 3. What it is not

**Not the console's frame.** The stream names its shaders and its draw; it carries neither the
vertex buffer nor the texture, so both are this test's. The record's pixel hash needs both and
remains a different claim.

And it says nothing about the *draw* the stream describes - the vertex count, the topology, the
render target - because it draws through the harness rather than through anything that reads
those. The harness's own mesh draw is one workgroup and its attachment is eight by eight.

## 4. Files

- `crates/orbistoun-gpu-vulkan/tests/submitted_frame.rs` - the stream, the submission, the frame.

## Next

1. `REQ-20260914T2348Z-4e71` - which register carries a buffer address. With it the window stops
   being the caller's and the frame above needs nothing told to it.
2. `REQ-20260914T1720Z-9c4a` - the payload split D688 assumes.
3. The draw the stream describes, which is the next thing the harness cannot express: a vertex
   count, a topology and a render target read out of the submission rather than fixed here.
