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

A mod may also patch the title's code and data; that half is decided with its format, below the
files.

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
