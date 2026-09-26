# D038 - Portable mode and the data root

**Status:** decided
**Date:** 2026-09-26

orbistoun writes only beneath one resolved root. Portable mode wins if any trigger fires - the
`ORBISTOUN_PORTABLE_MODE` variable, `portable` in the binary's file name, or a `.portable`
directory beside the binary - and roots everything in that directory, traces included. Otherwise
`ORBISTOUN_DATA_DIR`, then the collection's shared data directory. A relative library folder
resolves against the data root, never the working directory.

**Why:** "does not touch outside its own directory" is false the moment one exception exists.
The sentinel is a directory because it is also the data root, and a sentinel file blocks
creating it. The working directory is a property of how the program was launched, so a path
resolved against it means something different for each launcher.

**Rejected:**
- A `.portable` file as the sentinel: collides with the directory it marks.
- Looking for the sentinel in the working directory: `cd` relocates the data root.
- Resolving the library against the working directory: three launchers, three libraries.
- An environment override outranking portable: containment becomes a suggestion.
