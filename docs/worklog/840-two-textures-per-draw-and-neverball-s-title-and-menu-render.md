# 840. Two textures per draw, and Neverball's title and menu render

**2026-09-24**. A pixel shader may sample two textures (D690 had refused a second).

- **Translator:** each distinct image descriptor gets a slot (bindings 2 and 4). It resolves to the
  descriptor-table offset its `s_load_dwordx8 …, s[0:1], off` loaded it from (`descriptor_table_loads`).
  `Translated.textures` reports `(slot, table_offset)`. A second texture whose descriptors are not
  both from the table is still refused, as is a descriptor loaded from two offsets or a third
  texture.
- **Pipeline:** keeps each module's sources. `bind_textures` emits one `BindTexture { slot, … }` per
  source before a draw whenever the fragment module or its table changes.
- **Backend:** binds a second sampled image at binding 4.
- **Source:** oops-sdk `tools/shader/tex-prolog2.s`, where the second unit is at `+0x40` and the first
  at `+0x00`.

Also fixed worklog 839's clippy failures: two over-long functions, now sharing
`view_and_framebuffer`, and one SAFETY comment.

**Result:** Neverball, 150 s, **101 of 101 submissions complete** (from stopping at 31). The title
scene renders: 3D "Neverball" letters, start pad, goal. So does the **main menu** (Play / Replay / Help
/ Options / Exit).

**Next:** the menu draws on black rather than over the scene; depth and cull (`REQ-...2ea9`).
