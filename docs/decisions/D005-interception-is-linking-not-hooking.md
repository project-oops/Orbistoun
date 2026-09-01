# D005 - Interception is linking, not hooking

**decided** · 2026-08-19

Guests import by NID hash; the loader resolves each against the registry and writes
the address into the guest's relocation slot. No instrumentation pass, ever.

The consequence is the project's main early asset: the complete list of what a title
needs is available **statically**, before a single guest instruction executes.

