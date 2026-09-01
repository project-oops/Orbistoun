# D001 - Rust, not C++ or C#

**decided** · 2026-08-19

Crate-per-subsystem is Cargo's ergonomic sweet spot for this shape. FFI is explicit
rather than ambient, which is what you want when implementing someone else's ABI.
No runtime and no GC, which matters because guest threads call into host code from
threads the runtime never created - the specific thing that makes C# a poor fit here
despite managed emulators being viable in general. Host-side bookkeeping is most of
the code by volume and is where Rust removes whole bug classes.

Secondary benefit: reference material in this space is C++, so reading C++ and
writing Rust forces a translation through understanding rather than a copy, which
helps D015.

