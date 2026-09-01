# Two routes to a shader, because the loader thread named the entry points


The loader thread identified the guest's graphics entry points: the two command-buffer
submit calls, and a vocabulary of the calls that build those buffers. The submit calls
answer where to hook. The *builder* calls changed a design assumption, which is the more
useful half.

The names show a command-buffer builder library - calls that append packets to a buffer
the guest owns, and a separate call that submits it. So a shader can be learned about
twice: once because the guest registered it, and once because a register write in the
submitted packets points at it. `Pipeline::submit` now takes both, believes registration
where they overlap, and **counts every overlap either way**.

That counting is the point. The register vocabulary is the least verified thing in this
crate - its own data file says so - and nothing in this subsystem could produce evidence
about it. Agreement between the two routes is evidence it is right; disagreement names
both addresses and says it is wrong. Neither is obtainable without running both routes on
the same submission, which is why the inferred path keeps running after registration has
already answered.

A submission now also knows which queue it came from, and a stage that queue cannot run
is reported rather than filtered - a vertex shader in a compute submission is a decode
that found the wrong bits, not an unusual frame.

**Surprises.**

- **The most valuable thing in the handoff was not what I asked for.** The ask was a NID
  to hook and one dumped command buffer. What arrived alongside it was a list of function
  names, and the names carry the *architecture*: builder-then-submit rather than direct
  submission. That is what made a second route to a shader address exist, and it would
  not have come from the dump.

- **This is inference from names and is built to survive being wrong.** Whether the
  registration call carries an address has not been confirmed. If it does not, the
  registry stays empty, the inferred route answers exactly as before, and nothing here is
  load-bearing.

**What this makes possible next.** The same shape extends to the builder calls generally:
hook one, record its arguments, decode the packets it just appended, and check the two
agree. That would turn the packet vocabulary from transcribed-and-unverified into
something differentially tested - the same move that found a wrong row in the shader
encoding table. It needs captures, which is the other thread's side.

