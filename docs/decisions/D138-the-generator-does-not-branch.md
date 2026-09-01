# D138 - The generator does not branch


**Status:** assumed

`tests/agreement.rs` emits no control flow.

A generated backward branch is a generated infinite loop, and an infinite loop in a
compute dispatch is a hung GPU rather than a failing test. Forward-only branching would
be safe but needs the target patched in after the body exists, because the instructions
are variable length.

Control flow is covered by `execute.rs` against hand-written programs where the target is
known. The risk of getting this wrong is out of proportion to what it would add, which is
a different judgement from the usual one here - normally the answer is to build the
careful version rather than skip it, and this is the case where the failure mode is a
machine that needs rebooting rather than a red test.

