# 2026-09-04 - (/loop) The relation needed a mount, not a guest

```
all 7 measured relations now asserted (was 6 + 1 "unreachable")
the encoder-tier group checks out: 7 reasons accurate as written
suites 131   clippy/fmt/identity clean on both repos
```

Thirty-third cron tick, both remaining plan items.

## (a) The encoder group checks out

Six `related-libs` entries say the console reports an encoder library absent, completion "model
the tiers the manifest records"; the `tier-probe` entry says "orbistoun models no tiers".

Nothing models them. Every "tier" in the source is the **provenance** tier the audit sorts symbol
derivations into - a different word for a different thing. The nearest thing is
`FIRMWARE_MODULE_DIRECTORIES`, which refuses all three directories identically.

Seven reasons accurate as written. Worth a paragraph, not a manufactured finding: four ticks of
re-deriving stated blockers has found two wrong and thirteen right, which is about the ratio to
expect before calling the exercise finished.

## (b) The one that was wrong was mine, from two ticks ago

D545 recorded `file-position-tracks-reads` as unreachable because nothing opens in a bare service
test. Every clause true; the conclusion wrong. **The relation needed a mount, not a running
guest.** `orbistoun_fs::mount::mount` is public, `descriptor.rs`'s own tests stand a title up
with it in four lines, and `orbistoun-service` already depends on the crate.

What I had established was "the filesystem answers nothing when nothing is mounted" - a fact
about the default state - and I read it as "a service test cannot mount anything". The skipped
check is the cheapest one there is: **ask whether the tool has a seam before concluding the tool
cannot be used.**

`file_position.rs` asserts two four-byte reads of a sixteen-byte file give `0123` then `4567`,
and a third after seeking back gives `0123`. **Bytes, not counts** - a descriptor re-reading the
start returns the right length twice and the wrong content, which is the failure the relation is
about. The console's `0x20` is compared to nothing; it is likely a byte count but that is a
reading, not a citation.

Broken twice, in the two mechanisms it spans: a `read` that seeks to zero first fails the
advancing half; a `seek` that reports success without moving fails the seeking half.

The sentence in `measured_relations.rs` calling it uncovered is rewritten rather than left -
second time in a week that check 13 has caught my own prose.

Decision: [D547](../decisions/D547-the-relation-needed-a-mount-not-a-guest.md).
