# D114 - A shader read from memory ends where the program ends


**Status:** assumed

`decode_program` stops at the instruction that ends a program and reports whether it
found one; `decode` still decodes the slice it is given.

Two different situations. A fixture or a dumped binary *is* the shader and its extent is
known. A shader at a guest address has no extent - it begins where a register pointed and
ends at the terminating instruction, and what follows is whatever the guest put there.

Decoding past the end is not merely wasteful: the bytes after a shader are usually not
instructions, so they desynchronise the decode, and a perfectly good shader would then
report as untrustworthy because of data that is not part of it.

`terminated` being false means the address was wrong or the window was too small. Both
are errors, and neither is "a shader that happens to run to the end of memory".

