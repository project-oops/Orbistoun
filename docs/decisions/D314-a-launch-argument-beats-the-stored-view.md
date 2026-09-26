# D314 - A launch argument beats the stored view

**Status:** decided
**Date:** 2026-08-27

The view the window opens into is decided by a pure function in `orbistoun-shell` over the
arguments: `--shell`, `--list` or `--title <name>` beats the default in `config.toml`,
contradictory arguments are refused with a named cause and a non-zero exit, and unrecognised
arguments are ignored.

**Why:** an argument is what someone wants this time and a setting what they want usually, so
a launcher entry must not be overridden by a preference. Silently choosing between
contradictory flags makes a typed flag do something else. The window is re-executed with a
worker flag and sits on frameworks with flags of their own, so it cannot refuse what it does
not recognise.

**Rejected:**
- The setting beating an argument: launcher entries stop meaning anything.
- Resolving contradictions by picking one: the typed flag is silently ignored.
- Deciding in `main`: untestable.
