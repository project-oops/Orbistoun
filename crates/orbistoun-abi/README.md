# orbistoun-abi

The guest-to-host call boundary: emitting x86-64 machine code that calls a host function, and
the executable memory it runs in. It executes real instructions and fakes nothing.
[orbistoun-thunk](../orbistoun-thunk/) builds the per-import thunks on top of it.

## Calling convention

The guest is FreeBSD-derived and calls with System V: integer arguments in
`rdi, rsi, rdx, rcx, r8, r9`. The Windows host convention is `rcx, rdx, r8, r9` plus a 32-byte
shadow space, so a guest calling an ordinary `extern "C"` Rust function on Windows passes
arguments in the wrong registers and the callee silently reads whatever was in `rcx`.

**Rule:** every host function the guest can reach is declared `extern "sysv64"`, so the
compiler emits a callee using the guest's convention on every host. A test executes
hand-assembled code and asserts that all six arguments arrive.

## W^X

Code bytes are written while the page is writable, then the page is flipped to read-execute.
A page is never writable and executable at once: some platforms enforce that, and code that
assumes RWX fails there in a way that looks like a corrupt instruction stream.
