# 585. The stream says three vertices and the shader says three, from opposite ends

**2026-09-15** - orbistoun-gpu, after worklog 584

Worklog 584 drew a frame from a captured command stream and said what it could not do: read the
draw the stream describes. It reads it now.

```
twelve draws, three vertices each, one instance
```

A cube's twelve triangles, issued one at a time.

## 1. Two numbers that met

**Three is also what the guest's own primitive shader declares it will emit.** That number comes
from `MSG_GS_ALLOC_REQ` in the shader, decoded from its instruction words and translated into a
mesh module's output declaration (D688, worklog 558). This one comes from a packet body, found by
opcode in a stream captured from the same frame.

Nothing made them agree. They were arrived at from opposite ends of the capture by machinery that
shares no code, and each was built without reference to the other. That is the first independent
corroboration either has had.

It is also, quietly, evidence for the packet vocabulary - the table whose own comment calls it the
least certain thing in the crate. An opcode table that had `DRAW_INDEX_AUTO` in the wrong slot
would not produce a body word matching what the shader says.

## 2. What is read, and what is deliberately not

The auto-indexed draw's **first** body word is its vertex count, and the instance-count packet's
is the number of instances. Both agree with obSCEne's own measurement of the builders that emit
them: `DcbDrawIndexAuto` is twelve bytes with header `0xc0012d00` - opcode `0x2d` and two body
words - and `DcbSetNumInstances` is eight with `0xc0002f00`.

The draw's second body word is its initiator, which says *how* a draw is issued rather than what
it draws. Nothing reads it, because nothing here would know what to do with it, and reading a
field into a value nobody uses is how a wrong reading survives unnoticed.

The **indexed** draw is not extracted at all. It takes its count from separate state and an index
buffer in guest memory, and inventing either is what this crate refuses to do. A stream of
indexed draws therefore reports none, which is the honest answer rather than a guess at how many.

## 3. What this does not make true

A submission now *says* what the guest asked to draw. Nothing acts on it: the frame in worklog
584 still draws through the harness, which issues one mesh workgroup into an eight-by-eight
attachment. Twelve triangles into a real target needs a render target read out of the stream,
which is the capture-shaped half of G11 and still open.

So this is the draw being **reported** rather than performed, which is the same distinction the
shader failures already draw and worth keeping in the same words.

## 4. Files

- `crates/orbistoun-gpu/src/registers.rs` - `DrawCall` and `draw_calls`.
- `crates/orbistoun-gpu/src/pipeline.rs` - the draws emitted as commands, and counted in the
  report.
- `crates/orbistoun-gpu/tests/oracle_gl_cube.rs` - twelve of three, asserted.

## Next

1. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
2. The render target the stream names, which is the last thing between a reported draw and a
   performed one - and the one D104 refuses to invent.
