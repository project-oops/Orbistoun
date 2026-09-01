# D102 - The SPIR-V builder holds the section layout, not its callers


**Status:** decided (2026-08-21) - and the same fault was found one level down

`Builder` had one `preamble` section for capabilities, decorations, types and variables
alike. The format requires them in a specific order - every decoration before every type
- so correctness was a property of the order calls happened to be written in.

It was got wrong the first time a second buffer was declared: the memory buffer's
decorations were emitted after the register file's types, and `spirv-val` answered
"Decorate is in an invalid layout section", which names the symptom and not the cause.

There are now four sections - header, annotations, declarations, functions - and
`finish` concatenates them in the order the format demands. Declaring something new
cannot put a decoration in the wrong place, because the method chosen determines the
section rather than the position in the source.

### The same fault, inside a section

Sections made ordering independent of call order *between* them and left it dependent
*within* them. The header is where that matters, because it has an order of its own: every
capability, then extensions, then the memory model, then the entry point, then the
execution modes.

It went wrong the same way and for the same reason. A module needing two extra
capabilities declared them from the code that needed them - which runs after the entry
point is written - so they were emitted after it. **The driver accepted the module**, which
is worse than a rejection: the layout was wrong, nothing said so, and it would have kept
working until it met a stricter implementation.

The header is five ordered slots now and the opcode chooses one, so a capability declared
last is still emitted first. That is this entry's own principle - the method decides the
position, not the source - applied one level down, and the test pins the case that caught
it.

Found by re-reading this entry rather than by anything failing. Its own summary asked for
a second look "before anything is built on them", and by then a good deal had been.

