# 802. The oops-mesa/oops-gl cube apps now render 3D on hardware and are orbistoun's own source end to end — recorded as the fully-owned baseline the tool-bound retail frontier lacks (backlog 037)

**2026-09-22** — a direction from the operator, recorded so a session reading `WORKLOG.md` finds it, with
the detail in `docs/backlog/037-oops-mesa-gl-cube-apps-as-a-fully-owned-baseline.md`.

oops-mesa and oops-gl have reached rendering 3D on native PS5 hardware, and their cube demos are built
(`oops-apps/src/oops-mesa/mesa-cube`, `oops-apps/src/oops-gl/gl1-cube`, `.../gl2-cube`, each with a
`dist/`). Unlike every retail title - where the six-title survey (776/796) ends at walls reached by
computed dispatch that static RE cannot follow because orbistoun does not have the source - these are
**orbistoun's own source, guest program through GL through Mesa through the command stream**. The
clean-room boundary is about other people's source; knowing ours completely is the point.

Why it matters, in one line each (backlog 037 has the argument):

- **The tracer's first target.** `REQ-...7a91` asks for an execution/branch tracer; a fully-owned program
  is the one you can *check* the trace against from source, which no retail title allows.
- **A correctness signal above the loader that is not `assumed`.** Framebuffer diffing (the GPU oracle,
  CLAUDE.md) has been unreachable because retail titles stop before any surface exists; a cube that
  renders on hardware is a known-good image to diff against, every layer owned.
- **A path to the first honest pixel** - the smallest whole program whose entire expected behaviour is
  knowable, so drawing it is checkable rather than plausible.

Not started - this is the standing baseline for the tracer and framebuffer-diff work when either is taken
up. No code changed; a backlog item and this pointer. `./bin/orbistoun check` green, indexes regenerated,
identity scan clean. No commit.
