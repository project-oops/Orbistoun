# D423 - The sandbox is one entry point in orbistoun-fs, not orchestration in a consumer


**decided** - 2026-08-31 (user-directed)

D422 added the sandbox but left its *assembly* in the worker: `install_filesystem` emptied the
overlay for an ephemeral run, called `filesystem::install`, and then - separately, a few lines
later - called `mount::mount_title`. Three steps that must happen in one order, in one consumer,
with the retain/ephemeral decision a loose `== "ephemeral"` string check. The moment a second
consumer established a title's filesystem (a replay tool, the GUI, a test), it would re-derive that
order - and the order is exactly what, split apart, once cost a title its textures (D269).

So the assembly moves into `orbistoun_fs::sandbox`: one `establish(base, overlay, title_module,
retention)` that empties-if-ephemeral, installs the base tree with its writable device overlays, and
layers the title over `/app0`, in the order `mount`/`layer` require. `Retention` is a typed enum
with a `Retain` default, not a string, so the policy has one meaning and many callers. The fs crate
reads no environment - the consumer maps `ORBISTOUN_SANDBOX` to a `Retention` and passes it, keeping
the mechanism configurable-by-its-caller rather than self-configuring (principle 5, 13).

The worker's `install_filesystem` is now the thin thing a shim should be: resolve paths, read the
policy, call `establish`. Tested in the fs crate where it belongs - retain keeps a prior run's file
and ephemeral empties it; a device path is writable after establishing - rather than through a
whole-guest run. The engine (`mount`, `filesystem`) was already shared; this makes the *use* of it
shared too, which is what "centralised" has to mean to be worth the word.

