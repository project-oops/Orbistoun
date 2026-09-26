# orbistoun-hle

The HLE boundary: module declarations, the import registry, and stub policy.

`guest_module!` declares a system library, `Registry` resolves imports by NID, and
`StubPolicy` says what an unimplemented function returns. This crate is the machinery that
makes a missing implementation honest and configurable. Every subsystem crate depends on it,
and [orbistoun-loader](../orbistoun-loader/) resolves against its registry.

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

That expands to a `pub const MODULE`, which is what the service's list names. A crate that
also implements some of what it declares exposes them the same way:

```rust
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("exampleInit", example_init)]
}
```

Declaration and implementation are two lists, checked against each other: a function
implemented but never declared is unreachable, and the test that catches it lives beside the
declaration.

A declaration carries no NID. The NID is derived from the name at registration, so a
declaration can never carry a hash that disagrees with its own symbol.

## Rules

- **Interception is linking.** The loader resolves a NID against this registry and writes the
  address into the guest's relocation slot, so the full import list is available statically,
  before any guest instruction executes. A hook or trampoline means this path is being worked
  around rather than used.
- **Stub policy defaults to `Unimplemented`, never `Ok`.** The policy is a runtime TOML file
  keyed by human-readable symbol name. A silent success is how a wrong shim becomes a hang
  thousands of frames later.
- Editing the policy file and relaunching is the bisection workflow, and for most functions
  the only oracle (see [docs/TESTING.md](../../docs/TESTING.md)), so per-symbol isolation of
  overrides is a tested property.
