# D034 - CLI, GUI, and worker are interaction shims; logic lives in the crates

**decided** · 2026-08-19

No shim contains logic. The crates are the emulator. A shim calls the relevant crate
directly, or `orbistoun-service` where shared orchestration is warranted.

With three shims, a shared service stops being optional - it is what all three call,
and the protocol (D035) becomes a serialisable projection of its operations rather
than a parallel API free to drift. The same principle another project of mine
settled on, applied here.

Consequence for the current code: `orbistoun-cli` today owns real logic -
`build_registry` knows the full module set and the survey flow is assembled in
`main.rs`. That extraction happens **before** the GUI and before the CLI grows any
further, since it gets more expensive with every command added.

