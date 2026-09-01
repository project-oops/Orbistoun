# D414 - The test corpus, made a verb: a manifest of sources, fetched and recorded on demand


**assumed** - 2026-08-31

D042 specced a manifest-driven corpus under gitignored `titles/`; the recording half (`compat/`,
automatic on `run`) was built, but the fetch half was not. This is the fetch half, and the concrete
manifest format D042 left open.

**`orbistoun-corpus`** (new crate, D034: the crate holds the logic, the CLI is a shim) reads
`corpus/sources.toml` - metadata only, tracked - and fetches each source's assets into gitignored
`titles/<source>/<stem>/<file>`, one directory per guest so `run`'s automatic recording keys
`compat/<stem>.toml` without collision. Two source kinds:

- **`github-release`** - `repo` + `tag` (a tag, never a branch - D042); each asset is downloaded
  from the release, its sha256 pinned into the manifest on first fetch, and **verified** against
  that pin every fetch after. A changed pin under a fixed tag stops the sync loudly.
- **`local`** - a `path` into a sibling checkout, for a project of ours with no published release
  yet; re-hashed each sync rather than verified, and carrying a `todo` to migrate it. obSCEne is
  the first (D043), sourced from `../obscene/build` until it publishes a release.

The verb: `corpus list` / `corpus sync` / `corpus run`. `run` syncs then turns the ordinary `run`
loop over every guest. Seeded with `ps5-payloads-mirror` (itsPLK's mirror, 25 payloads, pinned) and
`obscene` (local). The guest bytes are never tracked; the provenance guard enforces it. Licence is
recorded per source, because downloading is not redistributing but the field should exist (D042).

**What the records say today, and the one change that makes them differentiate.** `corpus run`
records the *non-intervened* baseline, because an intervened run is a fact about the intervention,
not the title (D227) - so it does not set the handoff entry argument, which is still a diagnostic.
The elfldr-style payloads therefore all record `outcome = "0x1"`: entered with argc, the way any
program is, when an elfldr payload wants a handoff block. The follow-up is to make the handoff the
**default** entry for a payload-shaped guest under a console profile - not a diagnostic, because on
hardware elfldr always hands the block over - at which point these records differentiate on their
own (klog into its server loop, the sysctl-phase crashers at their walls) without any change to the
tooling. That is the D042 breadth signal working: does anything real get further this week.

Recorded `assumed`: a tooling and structure choice, extending a decision already made.

