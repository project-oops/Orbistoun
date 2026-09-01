# D333 - The model that costs nothing to set up, and the two things not copied with it


**decided** · 2026-08-27 · ported from a sibling project, and measured before trusting it

An installed coding assistant is already signed in. Running it as a subprocess borrows that
session, so a machine with one reaches a capable model with **no API key, no download and no
accelerator** - which is the entire setup cost of every other option in the ladder.

`Kind::Cli` and `crates/orbistoun-llm/src/cli.rs`. The Windows binary-discovery order is
carried across from another project of mine along with its reasoning: the launcher under
`LOCALAPPDATA` hands off to a running desktop application and the caller never sees the
reply, so the versioned command under `APPDATA` wins and the launcher is a last resort.
Somebody had to be caught by that to find it.

**Two things were deliberately not copied.**

The prompt goes on **standard input**, not in the argument list. A prompt here carries
decomposed examples and a vocabulary sample, and a Windows command line stops near
thirty-two thousand characters - which works until one day it does not, silently. Measured
first: the command reads a prompt from stdin, prints the reply to stdout, and keeps its own
diagnostics on stderr, so a bare `stdout.trim()` is safe.

A signed-out command is **reported, not signed in**. The sibling shells an interactive login
on the first authentication failure, which is right for a desktop application and wrong for
a tool that has to be able to run unattended: a thing that seizes the terminal to open a
browser cannot be left going. Pinned by a test, because it is an omission somebody could
reasonably read as missing.

**What it cannot do, said out loud rather than hidden.** The command exposes no seed and no
temperature, and the proposal loop is built on both - the seed advances per round so
successive rounds ask different questions, and the temperature is 0.9 because greedy
sampling repeated fourteen of twenty suggestions inside one round. So this engine ignores
two fields of every request it is handed, and `describe()` says so. An engine that dropped
them quietly would be indistinguishable from one that honoured them, which is principle 3
one level up from a stub returning success.

**Ranked with the hosted providers, not the local ones.** The seeding comment a few lines
above it states the rule: local outranks hosted because a trace, a fault address and a
guest's own strings are this project's material and the default should not be to post them
to somebody else. A command that answers over the network is not exempt from that for being
convenient to install. It goes *first* among the hosted entries, because it needs no key and
an entry that cannot answer should not sit above one that can.

I proposed placing it above the local CPU engine and was wrong; the file said so.

**And there is no elaborate framing.** The sibling prefixes an instruction telling the
assistant to ignore its own role, because the command runs with its own system prompt that
cannot be replaced. Measured here before copying it: asked plainly for twelve nouns as a
JSON array, the command returned exactly that and nothing else. So the system text is simply
prepended, `describe()` records that it is not isolated, and the workaround waits for
evidence that it is needed.

