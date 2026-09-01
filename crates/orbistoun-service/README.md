# orbistoun-service

The shared logic layer every shim calls.

**Models:** assembling the module registry, inspecting a container, surveying imports,
placing and relocating an image, building thunks, applying page protection, resolving
import labels, discovering titles, and emitting the default stub policy - every operation
a shim needs, and no shim holds any of it.

**Deliberately fakes:** nothing. It refuses honestly when a container cannot be
parsed rather than returning an empty result.

**Design note.** `orbistoun-cli`, the GUI, and worker mode are interaction shims -
none holds behaviour. This is what they all call, so an operation exists exactly once
and the shims cannot drift.

Everything crossing the boundary is **serialisable and owned**, taken from
`orbistoun-proto` rather than defined here. That is what lets the same operation be
invoked in-process by the CLI and across a process boundary by the worker. It is a
constraint, deliberately.

`modules()` in `symbols.rs` is the one place that knows the full module set, so a new
subsystem is one line there plus its own declaration. A test asserts no symbol is declared
twice and that every module contributes at least one, because a subsystem added to the
workspace but not wired in would be invisible in every shim at once - which has happened
once already (D123).

**Status:** complete for what the shims ask of it, up to and including running a guest.
