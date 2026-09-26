# orbistoun-loader

The ELF loader: loads a guest module in six steps - parse, reserve, resolve, relocate, set up
TLS, hand over the entry point.

| Entry point | Step |
|---|---|
| `survey` | what a module needs, without executing it |
| `image::place` | map the segments |
| `relocate::apply` | write resolved imports into the guest's slots |
| `tls::layout_of` | read the thread-local layout |
| `protect::apply` | set final page permissions |
| `process::build` | assemble the process image |

It builds on [orbistoun-elf](../orbistoun-elf/), [orbistoun-mem](../orbistoun-mem/),
[orbistoun-hle](../orbistoun-hle/) and [orbistoun-thunk](../orbistoun-thunk/), and
[orbistoun-service](../orbistoun-service/) drives it.

## Rules

- **Interception is linking.** The resolve step is where interception happens: each imported
  NID is resolved against the registry and the result is written into the guest's relocation
  slots. There is no instrumentation pass.
- `Survey::unresolved` is the headline number for a compatibility report: what a module asks
  for that nothing answers.
