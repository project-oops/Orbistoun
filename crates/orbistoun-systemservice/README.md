# orbistoun-systemservice

The settings and status a title asks the system about.

**Models:** `sceSystemServiceParamGetInt`, plus declarations for the rest.

**Deliberately fakes:** the values. What is written is a stated placeholder, never a guess
dressed as knowledge, and every one of them is an open question in
`orbistoun-cli questions`.

**Design note.** A title asks what language the hardware is set to, which button confirms,
what the display looks like. None of that is emulation in any interesting sense - it is a
question with an answer, and the answer is a setting of the hardware rather than a fact
about the guest.

Which is exactly the problem. **We do not know the values**, and the interface hands them
back through an out-pointer rather than a return value - so an unimplemented stub does not
merely answer wrongly, it answers *nothing*, and the guest reads whatever the stack
happened to hold. That is worse than a bad return, because a bad return is at least the
same wrong answer every run (D171).

So the out-pointer is always written. This is the crate where a hardware probe would pay
off fastest: every value here is one measurement away from being known.

**Status:** one function implemented, the rest declared.
