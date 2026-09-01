# D149 - An address that resolves is evidence, not just a precondition


**Status:** decided

Every address in a command stream is a **GPU** virtual address; guest memory is indexed by
a guest one. This crate reads the first as the second and nobody has confirmed they are the
same number - the open half of D101.

That assumption was tested on every submission and the result thrown away. An address that
resolved was a precondition met; one that did not was a shader failure. Neither was
recorded as what it actually is: a data point about the address space.

`SubmissionReport` now counts both, **apart from the shader outcome**. The two are
different questions with different answers - an address can resolve perfectly and its
shader still be refused - and folding them together loses the only measurement available
for the assumption. Weak evidence per address, strong in aggregate: a run where every
address resolves is a run where the assumption held every time it was tested.

The failure path had to be split to do it. `prepare` used to return a string; it returns
which *kind* of failure now, because "guest memory has nothing there" and "the shader
behind it is not translatable" are evidence about different things.

