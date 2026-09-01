# 2026-08-27 - A model that needs no key, and the parts not worth copying


An installed coding assistant is already authenticated, so running it as a subprocess
reaches a capable model with no key, no download and no accelerator. `Kind::Cli` plus
`crates/orbistoun-llm/src/cli.rs`, ported from an earlier project of mine, including its Windows
discovery order and the reason for it (D333).

**Three things were measured before being trusted, and two changed the design.** The command
reads a prompt from **stdin** - so the prompt does not go in the argument list, where a
Windows command line would have capped it near thirty-two thousand characters and failed
silently one day. Its diagnostics go to **stderr**, so a bare `stdout.trim()` is safe; my
first check merged the streams and briefly suggested otherwise. And asked plainly for twelve
nouns as a JSON array it returned exactly that, so the sibling's "ignore your default role"
framing is not carried over until something shows it is needed.

**What it cannot do is declared, not hidden.** No seed, no temperature - and the proposal
loop is built on both. `describe()` says so, because an engine that quietly drops a field it
was handed cannot be told apart from one that honoured it.

**It ranks with the hosted providers, not the local ones.** I proposed putting it above the
local CPU engine; the seeding code's own comment says local outranks hosted because this
project's material should not be posted elsewhere by default, and a subprocess that answers
over the network is not exempt for being easy to install. First among the hosted entries,
since it needs no key.

`orbistoun-suggest` now prints which backend and model answered each round. There is more
than one kind of engine now and they differ in where the prompt goes, so a person running it
should not have to read a configuration file to find out which one it was.

