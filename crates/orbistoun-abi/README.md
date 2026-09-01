# orbistoun-abi

The guest-to-host call boundary. Roadmap phase 0b, kept because it answered its
question and the answer is load-bearing.

**Models:** emitting real x86-64 machine code that calls a host function, and the
executable memory to run it in.

**Deliberately fakes:** nothing. It executes actual instructions.

## The question, and why the answer is not obvious

The guest is FreeBSD-derived, so it calls with **System V**: integer arguments in
`rdi, rsi, rdx, rcx, r8, r9`.

On Windows the host's native convention is different - `rcx, rdx, r8, r9` plus a
32-byte shadow space. A guest calling an ordinary `extern "C"` Rust function on Windows
would pass arguments in the wrong registers, **and the failure would be silent**: the
callee simply reads whatever was in `rcx`.

The answer is that host functions the guest can reach are declared
**`extern "sysv64"`**, so the compiler emits a callee using the guest's convention on
every host. Proved end to end by executing hand-assembled code and asserting all six
arguments arrive.

**W^X is respected**: bytes are written while the page is writable, then flipped to
read-execute. Never both at once - some platforms enforce that, and code assuming RWX
fails there in a way that looks like a corrupt instruction stream.

**Status:** answered, on Windows and Linux, and it grew into
[orbistoun-thunk](../orbistoun-thunk/) exactly as intended - the per-import thunks a
guest lands on are this spike, generalised.

The finding that justified doing it early: host functions must be `extern "sysv64"`,
because on Windows the native convention differs and a mismatch would be **silent** - the
callee reads whatever was in `rcx`. Discovering that at phase 4 would have been expensive.
