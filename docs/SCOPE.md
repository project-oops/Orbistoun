# Scope

What orbistoun deliberately is not, so that the answer to "should we add X?" is already
written down.

## Not a firmware-loading emulator

orbistoun reimplements the vendor's system libraries and never loads them. Another project in
this space takes the other path: it loads genuine `.sprx` modules and emulates beneath them,
which gives it correct operating-system behaviour without reimplementing it. That is a sound
design, and it is not this one.

Requiring a firmware dump means the build cannot be packaged cleanly, cannot be distributed
without provenance questions, and cannot accept contributions from anyone unwilling to obtain
one. Reconstructing semantics is the cost of a project that can be shared.

## Not a hypervisor

Running the real target kernel under virtualisation is architecturally cleaner than
reimplementing it, and it does not fit this hardware. The reasons are structural:

- The GPU is not a PCIe device. It is on-die and shares one coherent memory pool with the
  CPU, so there is nothing for passthrough to hand over. Passthrough is delegation, not
  translation: it needs the exact silicon.
- Custom blocks with no PC analogue (the hardware decompressor and IO complex, the audio
  engine, the cache scrubbers) are probed during initialisation.
- It makes a firmware dump a hard prerequisite with no partial credit. High-level emulation
  runs a guest with most of its imports stubbed; a hypervisor runs nothing until the entire
  boot chain, including its cryptography, is satisfied.

## Not a circumvention tool

No key extraction, no decryption, no DRM circumvention, and no assistance with any of it.
orbistoun is an operating-system reimplementation; obtaining anything to run on it is outside
the project.

## Not derived from the vendor's code

No disassembly in the tree and no code written while reading the vendor's binaries. See
[CLAUDE.md](../CLAUDE.md) principle 1; CI enforces it. No firmware, keys, dumps or guest
modules are tracked either: `./bin/orbistoun provenance` refuses them, so anything that
needs a guest module to reproduce stays outside the repository.

## Not an online-services implementation

The vendor's online services are out of scope. Their network calls are declared so that a
title asking for them shows up in a report, and a guest that tolerates a refused connection
carries on. BSD sockets are a different layer: a guest's own socket calls map onto host
sockets ([PAYLOADS.md](PAYLOADS.md)).

## Not a previous-generation emulator

The container loader accepts previous-generation binaries, because homebrew for the hardware
is scarce and a loader with nothing to load cannot be debugged. That is a source of test
material, not a product direction. Other projects in this space target the previous
generation.

## Not chasing a flagship title

Compatibility is measured by the unresolved-import count and the stub-to-real call ratio,
not by which well-known title nearly starts. A large open-world title is the worst early
target: the widest operating-system surface, the longest sessions, and the most accumulated
state. Useful early targets are small, contained and offline.
