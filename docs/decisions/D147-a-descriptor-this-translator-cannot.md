# D147 - A descriptor this translator cannot address is forced out of bounds


**Status:** decided

The untyped buffer accesses are translated. A buffer resource constant lives in four
scalar registers, so every field of it - base, stride, record count, bounds mode - is a
*runtime* value, and the addressing is emitted as arithmetic rather than evaluated here.

Two of its modes ask for addressing this does not do: **swizzled** buffers interleave
records by an element size not modelled, and **add-thread-id** folds the lane number into
the index. Both change *where* an access lands.

A translated shader cannot refuse at run time. So the refusal is expressed the only way it
can be: those descriptors are forced **out of bounds**, which the reference already defines
as reading zero and dropping writes.

That is a deliberate choice between two wrong answers. Producing the unswizzled address
would read real-looking data from the wrong offset - indistinguishable from correct data,
and exactly the failure principle 3 exists to prevent. Reading zero is visibly and
consistently wrong: a buffer that is entirely zero is a symptom anyone can spot, and it
cannot be mistaken for a working shader.

**One place where the reference is loose**, recorded because it is the only inference here.
Bounds mode three checks `offset + payload > NumRecords`, and defines payload as "the
number of dwords the instruction transfers" while every other term in that comparison is a
byte count. Read as dwords the comparison mixes units and is wrong by a factor of four at
the boundary; read as bytes it is the ordinary range check, and a raw buffer of N bytes
accepts a four-byte read at `off` exactly when `off + 4 <= N`. Read as bytes.

**The address is thirty-two bits.** The base is documented as 48, but guest memory here is
a small window indexed directly (D101), so the upper bits have nowhere to go - the same
simplification the flat accesses already make. It is the thing that changes when the
address space is real.

