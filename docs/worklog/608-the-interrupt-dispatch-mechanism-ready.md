# 608. The interrupt-dispatch mechanism, ready for the `int 0x41` handler the moment it lands

**2026-09-15** - orbistoun had no interrupt handling at all; now it has the mechanism, inert until a
vector is measured

## The gap this fills

orbistoun intercepts the guest at the library boundary (a relocation puts a stub where a name was)
and at the syscall-gadget path (D376). A commercial title reached the kernel a **third** way -
`int 0x41` - and there was no interrupt handling of any kind, so the vectored exception handler
caught the trap, reported it, and ended the process. PPSA04263 runs on retail hardware, so that is
orbistoun's gap (worklog 603, 605).

Filling it splits into a mechanism and the handlers, and only the handlers are blocked:

- **The mechanism** - intercept the `int`, look up a handler for the vector, run it against the
  guest's registers, resume past the instruction - depends on nothing external. Built here.
- **The handlers** - what `int 0x41` *does* - are a device measurement (obSCEne `REQ-...b3c2`). Until
  that lands, no handler is registered and every `int` still falls through to the honest
  "unimplemented kernel entry" fault.

So the table is deliberately empty, and the run is unchanged: PPSA04263 still stops at `int 0x41`,
70 imports, with the finding worklog 605 gives. The value is that servicing it becomes a one-line
`interrupt::install(0x41, handler)` once the measurement arrives, over plumbing that is **tested
now** rather than debugged the first time a real handler is added.

## The mechanism

`orbistoun_kernel::interrupt`, a new module:

- `InterruptFrame` - the guest's sixteen registers plus `rip`, which a handler reads its arguments
  from and answers into (a value in `rax`, a jump by setting `rip`), the way a syscall gate does.
- `install(vector, handler)` / `clear(vector)` / `has_handler(vector)` / `service(vector, &mut
  frame)` - a registry over a fixed `[AtomicUsize; 256]`, one slot per vector, a handler stored as
  its bare `fn`-pointer address. **Lock-free and allocation-free**, because `service` runs inside
  the fault handler where a lock or an allocation can deadlock the process it is resuming (principle
  9). Zero is the "no handler" sentinel, which is honest because no function lives at address zero.

## The glue

The vectored exception handler, on an access violation or illegal instruction, now reads the
faulting instruction, and if it is an `int n` (via the shared `classify_trap`) with a registered
handler, builds the frame, services it, writes the result back to the OS context, and resumes -
`return CONTINUE_EXECUTION` - instead of reporting. The `cd NN` encoding is two bytes, so the resume
point is set past it before the handler runs; a handler that touches nothing returns to the
instruction after the trap, and one that needs to jump sets `rip` itself.

The decision - is this an `int`, which vector, where does it resume, did a handler run - is factored
into a **pure** `serviced_interrupt`, so it is tested without raising a real trap; only the context
read and the single write-back stay in the unsafe handler. The write-back updates a **copy** of the
context and stores the whole record in one operation, keeping the single-unsafe-op discipline
(principle 4) rather than seventeen writes through the raw pointer.

## Made to fail

- `a_registered_handler_services_the_interrupt_and_answers_in_the_frame` and
  `an_unregistered_vector_is_not_serviced` (kernel): the dispatch mechanism, both directions - a
  handler runs and answers, an empty slot answers "no" and leaves the frame untouched.
- `int_0x41_has_no_handler_until_it_is_measured` (kernel): pinned as its own case, because
  registering a handler for it before the measurement would be inventing behaviour. The day it fails
  is the day a real handler was added - intended, and then replaced by a test that exercises it.
- `a_software_interrupt_with_a_handler_is_serviced_and_resumes_past_it` (worker): the glue - an `int`
  with a handler resumes two bytes past the trap with `rax` answered; an `int` with no handler, a
  plain `mov`, and `int 0x41` specifically all decline, so they reach the fault report rather than
  being swallowed.

## What it deliberately does not do

Register a handler for any real vector. `int 0x41`'s semantics are unmeasured, and a handler that
guessed what it reads and returns would be exactly the plausible-output the project forbids - and
worse here, because a wrong resume corrupts the guest silently rather than failing loudly. The
mechanism waits, empty, for the measurement.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,351 pass / 0 fail, worklogs unique, identity scan clean. Verified inert
on the live PPSA04263 run: `int 0x41` still reports the unimplemented kernel entry and stops at 70
imports, unchanged.
