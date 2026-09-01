# D076 - Names are ours; selection is the module's

**decided** · 2026-08-19 · a correction, prompted by the user

The provenance document said every name is "re-derivable from this repository alone",
which is true, and next to framing that implied the whole database could be rebuilt with
no guest module. **It could not**, and letting that stand would have been exactly the
overstatement the document exists to prevent.

| | Comes from |
|---|---|
| The names | This repository - the grammar and the word list, nothing else |
| Knowing *which* are real | A module's import table |

The generator produces 251 million candidates and cannot know which name a function that
exists. That is what an import table supplies, and a candidate is accepted only when its
hash equals one the module declares it needs.

The audit proves the first half - the half a provenance question is about. It shows every
name in the tree came out of inputs visible in the tree, rather than out of somebody
else's database.

**Reading an import table is a different kind of act** from reading a binary. It is a
list of hashes of *system library* names - the same values appear in everything built for
the platform, because they identify the operating system's interface rather than anything
belonging to a title. No code is copied and nothing about how the module works is
examined. What is read is "this program requires these 1,380 functions", and nothing else.

The dependence should shrink: a conformance probe built in this project imports platform
functions from binaries we wrote, and its import table serves the same purpose with no
third-party module involved.

