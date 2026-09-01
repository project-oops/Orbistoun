# Two modelled states made real, after being told they were only modelled


Pushed back on for parking suspend/resume and the input transport as "downstream of
measurements". The pushback was right on both counts and the reasons differ.

**Suspend/resume was never blocked by anything external.** The worker owns its guest threads.
`Execution::Suspended` had been derived, documented and inert for a day - a value describing
a behaviour rather than causing one, which is the exact failure this log keeps recording
about other people's code.

The obvious implementation is a trap worth writing down: suspending a thread at an arbitrary
instruction can catch it holding the host C runtime's heap lock, and then the next allocation
anywhere in the worker blocks forever - including on the thread that would issue the resume.
Not a stopped worker; an unrecoverable one, and only sometimes.

So threads park cooperatively at the trampoline, the single place every guest call passes
through, where a thread holds no guest lock. The cost is real and stated: a thread that stops
calling imports never parks. The difference from a silent hole is that this one is counted -
`backgrounded: 2 of 3 live guest thread(s) parked`.

**The input transport was an inconsistency, not a judgement.** The event queue was built with
no deliverable events - raise, withhold, count, wait for a measurement - and then the input
transport was refused because nothing could consume it. Same shape, opposite answer. Either
both were premature or neither was.

What building it bought before any measurement: `Focus` stopped being a tested function with
no observable effect. What a title may see is now decided in the window and nothing downstream
can widen it - the shell's button always stripped, a neutral pad when the shell has focus
rather than the last state held forever.

### The same mistake twice in one day

Two process-wide statics, two pairs of tests touching them, two races under the parallel
harness - and both times the failure read as the mechanism being broken rather than the tests
being wrong. Merging each pair into one test fixed both. Worth watching for: a `static` plus
`#[test]` is a race unless the tests are one test.

### What the corpus says to scaffold next

Import lists across four titles, by library: `libSceAgc` 268, `libc` 222,
`libSceNpCppWebApi` 143, `libkernel` 119, `libSceAmpr` 106, `libScePosix` 61. The
shell-adjacent ones are much smaller - `libSceSystemService` 19, `libSceUserService` 16,
`libSceSaveData_native` 16 - which is worth knowing before deciding where "scaffolding the
firmware" should start.

The most interesting single name in it is `sceUserServiceGetUserName`: a confirmed import
whose answer is a string somebody typed into the shell's own settings pane. The connection
this project argued for in the morning, available to build in the afternoon.

