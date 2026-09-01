# String literals already collapsed by a formatter


The `prose` guard catches a **new** line-continued string literal, which is the cause. It
cannot see one that a formatter already collapsed, which is the consequence - the literal
is by then a single long line with runs of spaces baked through the middle of it, and the
rendered message reads garbled (D184).

Three were found and fixed while working nearby. A grep for a run of six or more spaces
inside a string literal finds six more, in `orbistoun-gpu`, `orbistoun-shader`,
`orbistoun-spirv` and `orbistoun-translate`. It also finds two deliberate column
alignments, so it is not a gate as written - the shape that works is the same
grep plus a shrink-only ceiling file, exactly like the one the cause already has.

Left rather than fixed in passing because those files are actively being worked in another
session, and a formatting sweep across them would collide.

