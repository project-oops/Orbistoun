# `call` and `read` are live, and the fixture for one of them is broken


obSCEne reports both implemented - the serving build announces `call,read,report` where it
announced `report`. Consumers built.

`Client::call(address, args)` and `Client::read(address, length)` send hexadecimal and
handle the outcomes the feedback was explicit about:

- **A fatal address is called, not refused.** Malformed arguments are refused; a well-formed
  address that happens to be fatal is executed, and the probe dies doing it - which arrives
  as an acknowledgement with no result. Null is called rather than rejected, because "what
  does this platform do when you call null" is a real question with a real answer.
- **A bad read address has two legitimate answers.** A platform that can test before
  touching answers `unmapped`; one that cannot faults, and that is a death. Today's serving
  build does not pre-validate. Both are handled and both are tested, because which build is
  on the other end is not knowable from here.

`bytes` records are parsed and reassembled in offset order.

### Surprises

**The captured `06-read.txt` carries an odd number of hexadecimal digits.** It requests
`0x20` bytes - thirty-two, so sixty-four digits - and the record holds **sixty-five**. One
character spare, so the run cannot be a whole number of bytes.

The decoder refuses it and counts it. Rounding would put a value in a buffer that nothing
observed, and a buffer is exactly where an invented byte is least visible; silently dropping
the run would be worse, because a caller would receive a shorter buffer that looks complete.
Pinned as a test asserting the defect, so that correcting the capture upstream fails here
and whoever fixes it is told to replace the test with one asserting the bytes decode.

Found by writing a decoder that refuses the impossible rather than one that copes.

**The thing that made both consumers cheap was already there.** The client is generic over
any stream, so `call` returning a value, `call 0x0` dying, `read` returning ELF magic and
`read` of a bad address dying are four tests over in-memory buffers - none needing a socket,
a console, or the emulator path that currently cannot reach its own listen call.

