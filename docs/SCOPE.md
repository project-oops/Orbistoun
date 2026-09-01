# Scope

What orbistoun deliberately is not. Recorded so that the answer to "should we add
X?" is already written down.

## Not a firmware-loading emulator

orbistoun reimplements the vendor's system libraries; it never loads them. Another project in this space takes the
other path - it loads genuine `.sprx` modules and emulates beneath them - and gets
correct OS behaviour for free. That is a real, defensible design, and it is not
this one.

The trade is deliberate. Requiring a firmware dump means the build cannot be
packaged cleanly, cannot be distributed without dragging provenance questions into
every release, and cannot accept contributions from anyone unwilling to obtain one.
Guessing at semantics is the price of a project that can be shared.

## Not a hypervisor

Running the real target kernel under virtualisation is architecturally cleaner
than reimplementing it, and it is a dead end here. Two projects have tried it,
and neither runs a title.

The reasons are structural, not effort:

- The GPU is not a PCIe device. It is on-die, sharing one coherent memory pool with
  the CPU, so there is nothing for passthrough to hand over. Passthrough is
  delegation, not translation - it needs the exact silicon, at which point you have
  the hardware and no need for an emulator.
- Custom blocks with no PC analogue (hardware decompressor and IO complex, the audio
  engine, cache scrubbers) get probed during init.
- It makes a firmware dump a hard prerequisite, and there is no partial credit: HLE
  boots a triangle with 200 stubs and 3,000 missing, while a hypervisor boots
  nothing until the entire chain including the crypto is satisfied.

## Not a circumvention tool

No key extraction, no decryption, no DRM circumvention, and no assistance with any
of it. orbistoun is an OS reimplementation; obtaining anything to run on it is
outside the project entirely.

## Not derived from the vendor's code

No disassembly in the tree, no code written while reading the vendor's binaries. See
[CLAUDE.md](../CLAUDE.md) principle 1 - this is enforced in CI, not just intended.

## Not a previous-generation emulator

The container loader will accept the previous generation binaries, because homebrew for the hardware is scarce and a
loader with nothing to load cannot be debugged. That is a test-material decision,
not a product direction. Other projects in this space already target that hardware, are
years ahead, and there is no reason to compete with them.

## Not chasing a flagship title

Compatibility is measured by the unresolved-import count and the stub-to-real call
ratio, not by which famous game nearly boots. A big open-world title is the *worst*
early target: widest OS surface, longest sessions, most accumulated state. The
useful early targets are small, contained, and offline.
