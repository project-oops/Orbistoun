# D333 - The command-line engine reads its prompt from standard input

**Status:** decided
**Date:** 2026-08-27

The engine that borrows an installed coding assistant's session (`Kind::Cli`) passes the prompt
on standard input and reads the reply from standard output. A signed-out command is reported,
never signed in, and `describe()` states that seed and temperature are ignored and the system
text is not isolated.

**Why:** it needs no key, download or accelerator. A prompt carrying examples and a vocabulary
sample can exceed the platform's command-line length limit. A tool that must run unattended
cannot seize the terminal to open a browser. An engine that dropped request fields quietly
would be indistinguishable from one that honoured them.

**Rejected:**
- The prompt as an argument: fails once it grows past the length limit.
- Interactive login on failure: blocks an unattended run.
