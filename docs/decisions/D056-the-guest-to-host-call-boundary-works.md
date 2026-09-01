# D056 - The guest-to-host call boundary works, and needs `extern "sysv64"`

**decided** · 2026-08-19 · phase 0b, answered on both platforms

The guest calls with **System V**: integer arguments in `rdi, rsi, rdx, rcx, r8, r9`,
return in `rax`.

On Windows the host's native convention is different - `rcx, rdx, r8, r9` plus a
32-byte shadow space. **A guest calling an ordinary `extern "C"` Rust function on
Windows would pass arguments in the wrong registers, and the failure would be silent**:
the callee reads whatever happened to be in `rcx`. That is the worst possible shape for
a bug at this layer, and it is why the spike was worth doing before phase 4 rather
than during it.

Host functions the guest can reach are therefore declared **`extern "sysv64"`**. Proved
by executing hand-assembled machine code that loads six argument registers, calls
through `rax`, and returns - with every argument asserted on arrival. Passes on Windows
and Linux from the same source.

`orbistoun-abi` is kept rather than thrown away: it grows into phase 4's thunk
mechanism, and the tests are the regression guard for a convention mismatch.

**W^X is respected** in the executable buffer - written while writable, then flipped to
read-execute, never both at once. Some platforms enforce that, and code assuming RWX
fails there in a way that looks like a corrupt instruction stream rather than a
permissions problem.

