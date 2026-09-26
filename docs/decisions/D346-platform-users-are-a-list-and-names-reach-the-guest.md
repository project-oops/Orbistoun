# D346 - Platform users are a list, and names reach the guest

**Status:** decided
**Date:** 2026-08-27

`Settings` holds a list of users with identifiers from a stored high-water mark, and the worker
reads `shell.toml` itself. The user-name call answers the configured name, trusting the size
argument only up to 64 bytes and refusing otherwise, and truncating on a character boundary.
Other user-service calls stay declared and unimplemented.

**Why:** titles ask for a name by identifier, enumerate signed-in users and key save data on
them, which one name field cannot answer. A name is a string with no encoding to guess; age
bands and flags are measured encodings. The size position is assumed, and a refusal is
recoverable where a smashed stack is not. Reusing a deleted user's identifier would hand a new
person someone else's saves.

**Rejected:**
- A single `user_name`: cannot answer by identifier.
- Next identifier from the highest live one: reuses identifiers.
- Settings forwarded by message: can arrive half-applied.
