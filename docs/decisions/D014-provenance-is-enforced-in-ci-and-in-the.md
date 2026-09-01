# D014 - Provenance is enforced in CI and in the pre-push hook

**decided** · 2026-08-19

No firmware, keys, decrypted titles, disassembly, or code written while reading
vendor binaries. The `provenance` job fails the build; `.gitignore` is a convenience,
not the control. The hook runs it unconditionally, even for a docs-only change.

Reimplementation-from-disassembly converges on the original's structure, and that
convergence is evidence. The cost of contamination is not a courtroom - it is that
the work can never be shared.

