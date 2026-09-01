# D200 - The submission entry point takes an address, because a guest has one


**Status:** decided

`Pipeline::submit` takes bytes, which is what a test has. A guest has a pointer and a
length: it builds a command buffer in its own memory and calls the vendor's submit
function with an address. `submit_at` is that shape.

**Nothing calls it**, and that is a statement about the loader rather than about this
crate - no guest has yet reached a submission. It exists so that when one does, the work is
wiring a shim to a function rather than designing an interface under time pressure, and so
the address arrives measured (D149) rather than assumed.

An unreadable command buffer answers `None` rather than an empty submission. The pointer
came from the guest's own CPU-side code, so a failure there is the shim's arguments rather
than any GPU-address assumption - a different thing to suspect - and an empty submission
would read as "there was nothing to do".

**One progress vocabulary.** A submission converts into the same summary the shader corpus
reports (D148), because it is the same question asked of shaders that arrived from a guest
rather than from a directory. Given its own format it would drift immediately, and a reader
would have to learn which numbers meant what depending on where the shaders came from.

