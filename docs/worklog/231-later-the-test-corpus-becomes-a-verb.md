# 2026-08-31 (later) - the test corpus becomes a verb (D414)


Built the fetch half of D042's manifest-driven corpus, which was specced but never written; the
recording half (compat/, automatic on run) already existed. New `orbistoun-corpus` crate holds a
manifest (`corpus/sources.toml`, metadata only, tracked) and the fetch/pin logic; a new `corpus`
CLI verb (list/sync/run) drives it. Two source kinds: `github-release` (pinned by sha256, verified
every fetch) and `local` (a sibling checkout, re-snapshotted, with a todo). Seeded with
`ps5-payloads-mirror` (itsPLK's mirror, all 25 payloads) and `obscene` (local, from ../obscene/build
until it has a release). Guest bytes land in gitignored `titles/<source>/<stem>/<file>` - one dir
per guest so compat records key by name and do not collide; only the manifest and the compat
reports are tracked, and `git check-ignore` confirms the bytes are not.

End to end it works: `corpus sync` fetched all 25 from the release, pinned every hash into the
manifest, and snapshotted obscene.elf; a re-sync showed `cached` (idempotent); `corpus run
--profile ps5-cex-12.40` ran all 26 guests and wrote 25 new `compat/*.toml` reports. Crate is
clippy-clean at `-D warnings` and its four tests pass.

Two surprises. First, `toml::to_string_pretty` on the pin-writing save drops the manifest's header
comment - the durable prose moved to `corpus/README.md`, and the migration note survives because it
is a `todo` field, not a comment. Second, and worth carrying forward: `corpus run` records the
honest non-diagnostic baseline (no handoff, because an intervened run is not recorded - D227), so
every elfldr payload currently records `outcome = "0x1"` - entered with argc when it wants a
handoff block. The reports differentiate the moment the handoff becomes the default entry for a
payload-shaped guest under a console profile (D414's follow-up); the tooling needs no change for
that.

Note the CLI cannot be clippy'd whole right now: `orbistoun-fs/src/socket.rs` (mid-edit, `AM`) trips
`multiple_unsafe_ops_per_block`, and the workspace still does not build on Linux (`orbistoun-mem`'s
`rustix::param` under the missing `param` feature). Both are pre-existing and unrelated; this work
was built and run natively on Windows, where neither bites.

