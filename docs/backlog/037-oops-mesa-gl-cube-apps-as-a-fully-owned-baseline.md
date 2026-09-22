# Run the oops-mesa / oops-gl cube apps as a fully-owned baseline and tracer target

oops-mesa and oops-gl have come far enough to **render 3D on native PS5 hardware**, and their cube
demos are built and shipping a `dist/`:

- `oops-apps/src/oops-mesa/mesa-cube`
- `oops-apps/src/oops-gl/gl1-cube`
- `oops-apps/src/oops-gl/gl2-cube`

**These are orbistoun's own source, end to end**, and that is the whole point. Every retail title is
reverse engineering - the six-title survey (worklog 776/796) ends at walls reached by computed dispatch
that static analysis cannot follow, because orbistoun does not have the source and cannot know what the
guest *meant* to do. The cube apps are the opposite: the guest program, the GL implementation, the Mesa
translation and the command stream are all in this collection, so at every instruction there is a known
answer to "what should happen next". The clean-room boundary (principle 1) is about *other people's*
source; these are ours, so knowing them completely is allowed and expected.

That makes them the baseline the tool-bound frontier has been missing, in three concrete ways:

1. **The tracer's first target.** `REQ-20260922T0746Z-7a91` asks for an execution/branch tracer from
   entry - the one capability behind five of the six retail walls. A fully-owned program is the right
   thing to build and validate it against: the expected control flow is known from source, so the
   trace can be *checked* rather than merely produced, which no retail title allows.

2. **A correctness signal above the loader that is not `assumed`.** Framebuffer diffing is the GPU
   oracle this project names (CLAUDE.md, "where the oracle comes from" #2), and it has been unreachable
   because the retail titles stop inside `libSceAgc` init before any surface exists. A cube that renders
   on hardware gives a known-good image to diff orbistoun's output against, end to end - guest → GL →
   Mesa → GPU packets → orbistoun's Vulkan translation - with every layer owned.

3. **A path to the first honest pixel.** Orbistoun's no-pixel state is deliberate (correctness-first),
   but the cube is the smallest whole program whose *entire* expected behaviour is knowable, so getting
   it to run and draw is a milestone that is checkable rather than plausible.

**Next, when taken up:** get a cube eboot into orbistoun's title corpus, run it, and see where it stops -
and unlike a retail title, walk the divergence against the known source rather than against a disassembly.
This is the standing baseline for the tracer work and for framebuffer diffing, not a one-off.
