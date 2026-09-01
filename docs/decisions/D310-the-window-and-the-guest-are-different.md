# D310 - The window and the guest are different processes, so the shell button had nowhere to go


**decided** · 2026-08-27 · found by reading the protocol before writing the shell

The shell was scoped as four things: browse the library, boot straight into it, press the
shell button in-game, and quit back to it. Two of those are a front-end and two are an ABI
contract, and only the first two were buildable - for a reason that had nothing to do with
graphics.

Guest code executes in a child process (D032). `Request` was send-once-then-listen: the
worker read a request, ran the guest to completion, and streamed events back. The only
in-flight control was `Stopper`, which is `TerminateProcess`. **So a shell action arriving
while a title ran sat unread in the pipe until the run it was meant to interrupt had already
finished.** Nothing was broken; there was simply no path.

Reading happens on its own thread now, and `Request::Shell` is applied *there* rather than
forwarded - it needs no reply, so the output stream keeps exactly one writer even while
`run_guest` is mid-sentence. That property is asserted as *no event*, because a reply would
pass a test that only checked the handshake and would corrupt the stream solely under the
timing this whole arrangement exists to support.

`StdinLock` turned out not to be `Send` - it holds a `MutexGuard` - so worker mode wraps the
unlocked handle. Nothing is given up; this process is its only reader.

**The general point is about where a feature's cost actually sits.** The scoping conversation
put controller, GPU and audio support ahead of the in-game shell. The real prerequisite was a
control channel, and it was invisible because it is not a subsystem - it is the absence of one.

