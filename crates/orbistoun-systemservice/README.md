# orbistoun-systemservice

The settings and status a title asks the system about.

A title asks what language the hardware is set to, which button confirms, and what the display
looks like. Each is a question with an answer, and the answer is a setting of the hardware
rather than a fact about the guest. The crate serves `sceSystemServiceParamGetInt` and related
calls from [orbistoun-shell](../orbistoun-shell/)'s settings, and declares the rest.

## Rule

**The out-pointer is always written** (D171). The interface hands values back through an
out-pointer rather than a return value, so an unimplemented stub does not answer wrongly - it
answers nothing, and the guest reads whatever the stack held, a different wrong answer on each
run. Where a value is unmeasured, what is written is a stated placeholder, never a guess dressed
as knowledge, and each one is an open question in `orbistoun-cli questions`. Every value here
is one hardware measurement away from being known.
