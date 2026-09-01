# Resolved since this list was written


Kept briefly so the same three get re-raised less often: `rustix` and `windows-sys` are
used by `orbistoun-mem::platform`; `docs/design/` and `tools/` are populated and
`docs/features/` is deleted; `.gitattributes` no longer anchors its fixture patterns at a
root `tests/` that does not exist.

## Deliberately deferred

- **Audio, input, filesystem implementations.** Declared, unscheduled. Unreachable
  until threading works; writing them earlier means code that cannot be exercised.
- **Vendor-specific haptics and adaptive triggers.** No general PC analogue. Out of
  scope until something asks for them.
- **Anything about performance.** There is nothing to profile. Revisit when a frame
  is being produced.
