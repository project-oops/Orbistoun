# D099 - Translated shaders are executed, not merely validated

**decided** · 2026-08-19

`orbistoun-gpu-vulkan::compute` dispatches a compute shader over a storage buffer and
reads back what it wrote. `spirv-val` answers whether a module is well-formed; this
answers whether it computes the right thing, which is the question that matters.

Without it the translator would be the first component in this project built before its
oracle. Everything else got the oracle first - the differential harness before the
operand layouts, `spirv-val` before the emitter, `RecordingBackend` before any backend -
and each time it caught something that looked entirely reasonable.

**A missing device skips, and the skip is loud.** `probe` reports absence separately
from failure, because a machine may legitimately have no Vulkan. The trap is what
"skip" means in a Rust test: there is no first-class skip, so the obvious shape is to
return early - and a test harness captures the output of a *passing* test, which makes
the skip message invisible. **The first version of this had exactly that hole.**

Fixed in the gate rather than the test: `orbistoun.sh check` re-runs the device tests
with output shown and prints either `executed against a real device` or a warning that
they did not run. A suite reporting green while its most important test was skipped is
the precise failure these tests exist to catch, arriving in the tests themselves.

**`ash` is loaded, not linked.** Linking would make the *build* depend on a Vulkan SDK
being installed; loading defers it to runtime, so a machine with no Vulkan still
compiles and says so clearly rather than failing to link.

**Software rendering is the better oracle, with a caveat worth writing down.** A
software implementation is deterministic, where real drivers differ in floating-point
behaviour and optimisation - so a regression test that passes on one machine and fails
on another says nothing. The trade: this verifies *the translator*, not compatibility
with any hardware. A green suite here is not hardware validation.

In practice both are available - the development machine has a real device and the
build VM has a software one, so the same tests run against each.

**Resources are released only on the successful path.** An error abandons them, because
the alternative is a guard type per Vulkan object for a process that exits moments
later. Stated in the module rather than hidden, because it stops being acceptable the
moment this runs inside anything long-lived.

