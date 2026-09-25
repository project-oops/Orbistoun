# 874. sceAgcCreateShader patches the program address; the APR resolve stops at its first miss

**2026-09-25**. obSCEne answered both follow-ups.

**REQ-20260925T2056Z-5d19, the two bytes.** The full diff of the probe's header shows 40 bytes of
self-relative relocation (as worklog 872), the payload pointer, and `+0x94..+0x95`. The register
descriptor at `+0x90` (the `+0x20` target) is `{reg, value, reg + 1, value}`, and the console wrote
`payload >> 8` into its second dword. For stages 4 and 5, whose first dword is 0, it wrote nothing.

The extent is `0x105`; `0x130` was a comparison artefact. The count at `sub_table + 0x28` is
untouched: it is the asset's own input.

`create_shader` now patches the descriptor: `sh[1] = payload >> 8`, and `sh[3] = payload >> 40`
for the pair's high register. The high half is Mesa's `address32_hi >> 8` (`ac_cmdbuf.c:318-324`)
and orbistoun's own decode (`registers.rs`); it read 0 on the probe only because its payload sat
below 2^40. Test: `create_shader_matches_the_consoles_byte_diff`, which covers the descriptor, all
five sub-table targets (`+0x118`, then `+0x130` four times, confirming the entries are
self-relative) and the unregistered case. It was watched failing with the patch removed.

**REQ-20260925T2130Z-e61a, a successful resolve.**

- Only `/app0/...` paths are valid; anything else is `EINVAL` in the kernel's log.
- On a title without a mounted package filesystem, every path answers unresolved, so no probe
  can show a real id.
- A multi-path call stops at its first unresolved entry and leaves the later slots as the caller
  prepared them.

`apr_resolve_filepaths` now stops there too. Test:
`an_unresolved_path_leaves_the_later_slots_untouched`, watched failing without the stop.

**Where that leaves the titles.**

- PPSA25872 resolves paths a retail package filesystem would answer with kernel ids, and nothing
  can measure those ids from a probe. It stays at its Il2Cpp null (`the title's own modules+0xe3b20`).
- PPSA02664 and PPSA03416 still reach index `0x4e` in a table whose extended-buffer count the
  hardware confirms is the asset's own 0. How a console avoids that path is the open question, and
  a dump of their slot tables is the next step.

No wall moved this unit; both changes are accuracy.
