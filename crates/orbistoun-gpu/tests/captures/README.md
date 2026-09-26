# Command-stream captures

Command streams recorded from a running guest - the only material in this crate that the crate
did not generate.

## Purpose

`../../data/packets.toml` says which packet opcodes write hardware registers, what their base
offsets are, and which registers hold a shader address. It is transcribed from published
documentation, and a mistake in it is silent: a wrong base attributes every register write in its
class to the wrong register, consistently, and a wrong shader-address row sends shader lookups to
the wrong place. Neither produces an error. The captures are the external check on that table.

## A capture is a pair

The guest's graphics library is a command-buffer builder: library calls append packets to a
buffer the guest owns, and a separate call submits it. A capture records both sides of one call:

- what the guest asked for, as the arguments the shim observed
- the bytes that call appended

A command buffer on its own would have to be read through the table under test, so agreement
would prove nothing. In the pair, the call states the answer and the bytes are the question.

## Format

Files share a stem:

    <name>.toml          what the call said (the claims below)
    <name>.hex           the bytes it appended
    <name>.payload.hex   optional: a guest-memory image the stream refers to, such as a
                         shader payload, for tests past the register table
    <name>.vertex.hex    optional: the vertex shader image a draw test binds
    <name>.pixel.hex     optional: the pixel shader image a draw test binds
    <name>.target.hex    optional: the colour target read back after the draw on the hardware

Every `.hex` file is text: little-endian dwords, each exactly eight hex digits, separated by
whitespace, with `#` starting a comment that runs to the end of the line. Text rather than a
`.bin`, because the provenance guard refuses `.bin` files and because a capture is the one thing
here worth reading in a diff.

`tests/vocabulary.rs` checks every `<name>.toml` against its `<name>.hex`; a `.toml` without its
`.hex` is an error. It ignores the optional files, and a `.hex` with no `.toml` beside it. The
optional files, and stream-only `.hex` captures, are read by the device tests in
`crates/orbistoun-gpu-vulkan/tests/` by name.

The `.toml` holds:

```toml
# Which library call this came from, and which queue the buffer belongs to.
call  = "sceAgcDcbSetCxRegisterDirect"
queue = "draw"                     # "draw" or "compute"

# Optional free text.
note = "one builder call: SET_CONTEXT_REG offset 0x200 = 0x12345678, 12 bytes"

# A register the call is known to have written, and the value. Checked through the packet
# walk and the register bases alone.
[[register]]
register = 0xA200
value    = 0x12345678

# A shader the call is known to have set up, and its address. Checked additionally through
# the shader-address rows and the pairing of the address halves.
[[shader]]
stage   = "vertex"                 # a stage named in data/packets.toml: vertex, fragment, compute
address = 0x2_008F_0000
```

`call` and `queue` are required. `[[register]]` and `[[shader]]` are both optional lists; a
capture holds either or both, and the suite fails if no capture holds any expectation. An
expectation that is not known is left out, never guessed: a fixture asserting what the capturer
inferred rather than observed is not an oracle.

## Adding a capture

Put the files in this directory. `tests/vocabulary.rs` picks up every `.toml` with no
registration step and reports any disagreement, naming the register or shader stage and the two
values. With no captures present it prints that it checked nothing rather than passing.

## Provenance

Captures are observations of behaviour - bytes a guest produced and arguments it passed - which
cross the provenance boundary freely. Nothing here may be derived from reading another
implementation's source. See [PROVENANCE.md](../../../../docs/PROVENANCE.md).
