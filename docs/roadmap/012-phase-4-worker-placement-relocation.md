# Phase 4 - Worker, placement, relocation, protection, stubs, entry *(DONE)*; thread pointer, trace sink


The largest phase, and the one the whole design turns on.

- **Worker mode** - **done** (D057). A shim re-invokes itself with a hidden flag,
  hosts the crates, and speaks `orbistoun-proto` over stdio. `orbistoun-cli run`
  drives a real child process today; execution is what it reports as outstanding.
- **Placement** - **done** (D058). The span is reserved and every loadable segment
  copied into it with `.bss` zeroed. `orbistoun-cli run` places a 96 MB commercial
  executable in a worker process today.
- **Relocation** - **done** (D059). The commercial executable applies **174,172 of
  174,172** relocations and reaches `Linked`.
- **Protection** - **done** (D060). Page-granular, unioned across segments that share
  a page. Zero write-plus-execute segments in any executable examined.
- **Thread-local storage layout** - **done** (D061). Variant II, block below the
  pointer. Installing an `fs` base is a separate platform problem and has blocked
  nothing: no executable examined declares a TLS relocation.
- **Per-import stubs** - **done** (D062). One 32-byte stub per dynamic symbol, each
  recording which import the guest wanted, in order, without allocating.
- **Entry jump** - **done** (D063). A dedicated guest stack with a guard page, a stack
  switch, and full System V register discipline. Guest code executes.

**Next, and now measurable:** all four commercial executables fault immediately with an
access violation. The next step is to say *where* - a fault handler reporting the
faulting address and instruction pointer turns one bit of information into a work list.
The two prime suspects are the absent thread pointer and the absent process stack
image.
- **Trace sink** wired here and not earlier - it records guest calls, and no guest
  call exists before now.

**Observable result:** guest code executes, immediately hits a stub, and the trace
records its first real event with a real call site. This is where "interception is
linking" is proven or disproven - and where the stub-policy bisection loop becomes
usable, which is the main working tool from here on.

Also the point the accuracy suite - `obSCEne`, a separate repo (D043) - becomes
usable in its first mode: it calls
each interface in a known order, and the **trace is the report** - no I/O
implemented, nothing to print to. And the point regression assertions become
possible: once corpus items reach known states, CI can assert they still do.

