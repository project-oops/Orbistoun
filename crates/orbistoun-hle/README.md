# orbistoun-hle

The HLE boundary: module declarations, the import registry, and stub policy.

**Models:** `guest_module!` for declaring a system library, `Registry` for NID-keyed
resolution, and `StubPolicy` for what an unimplemented function returns.

**Deliberately fakes:** everything - by definition. This crate is the machinery that
makes faking honest and configurable.

## Adding a system library

One block, plus one line in `modules()` in `crates/orbistoun-service/src/symbols.rs`:

```rust
use orbistoun_hle::guest_module;

guest_module! {
    "libExample" {
        "exampleInit" => 0,
        "exampleOpen" => 4,
    }
}
```

That expands to a `pub const MODULE`, which is what the service's list names. A crate
that also implements some of what it declares exposes them the same way:

```rust
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("exampleInit", example_init)]
}
```

Declaration and implementation are two lists in two places, checked against each other:
a function implemented but never declared is unreachable, and the test that catches it
lives beside the declaration (D123).

The NID is absent on purpose - it is derived from the name at registration time, so
a declaration can never carry a hash that disagrees with its own symbol.

**Design note.** Interception is linking, not hooking: the loader resolves a NID
against this registry and writes the address into the guest's relocation slot. That
is why the full import list is available statically, before any guest instruction
executes. If you find yourself adding a hook or trampoline, this path is being
worked around rather than used.

Stub policy is a runtime TOML file keyed by human-readable symbol name, and defaults
to `Unimplemented` rather than `Ok`. A silent success is how a wrong shim becomes a
hang forty thousand frames later. Editing that file and relaunching *is* the
bisection workflow - and per `docs/TESTING.md` it is the only oracle most functions
have, so the per-symbol isolation of overrides is a tested property.

**Status:** complete for the current design.
