# D836 - A title's mods are user content: files layered over its own, and patches

**Status:** assumed
**Date:** 2026-10-14

A title's mods live in its directory's `mods`, one directory each. A mod applies unless its
`mod.toml` says `enabled = false`; one whose `mod.toml` does not parse is left off with the reason
logged. They apply in the order of their directories' names.

A mod's `app0` tree is laid over the title's `/app0`, read-only: over the title's own files, so a
mod's file answers first, and a later mod's before an earlier one's; under a staged title's
writable layer (D722), so a guest's write is copied up into its overlay and never lands in a mod or
the library. The `mods` directory, like the rest of what orbistoun keeps beside a title, is hidden
from the guest (D756).

A mod's `patches.toml` replaces bytes in the title's executable once it is linked and before its
segments take their protection: each patch an offset from the executable's base, the bytes it
expects there and as many bytes it writes. A mod's patches apply all or none - one outside the
executable's segments, finding other bytes than it expects (a mod made for another build), or
naming another of the title's modules leaves every one of that mod's patches unwritten, and the
log says which and what it found. Only the executable is patched; the title's other modules are
linked and protected inside the link (D724), and patching them is a further step.

A mod's `cheats.toml` writes values into the running title at its flips: each from an offset from
the executable's base, through a pointer chain where it names one, written at every flip (a frozen
value) or once. Every step is checked against the executable's segments and the guest's own
mappings before it is touched, and a value is written only where the title may write; a chain that
does not reach yet is tried again at the next flip.

**Why:** a player changing a title - a fan translation, replaced textures, a fix the publisher never
shipped - wants the title's own files left alone and the change undone by turning it off, and the
layered filesystem already answers each path from the first layer holding it (D251). A mod is the
player's, put beside the titles they choose: orbistoun ships none and changes nothing by a title's
id, which is what keeps "no title-specific code" true when titles are changed. D005's rule that
orbistoun does not patch guest code is about orbistoun's own behaviour; a player's mod is input, as
a pad script is.

**Rejected:**
- Copying a mod's files into the title's directory: the title's files would be lost to the change,
  and turning the mod off would mean restoring them.
- A mod above a staged title's writable layer: the guest's writes would land in the mod's
  directory, mixing what the guest wrote with what the player installed.
- Listing enabled mods in the title's settings: the directory is the list, and a person installing
  one by hand should need nothing else.
