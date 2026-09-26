# Memory

Guest code runs natively, so guest virtual addresses are host addresses: orbistoun reserves
the ranges a guest expects in its own address space and maps the guest's segments, stacks,
heap and direct memory into them. Guest memory is reached only through `orbistoun-mem`, which
every other crate uses through checked accessors.

## The address map

Every fixed base orbistoun places something at, and its owner, is listed in
[ADDRESS_MAP.md](../ADDRESS_MAP.md). The map is checked against the source, so it is the place
to look before choosing an address (D513).

## Preferences

The memory pane of the preferences window has one switch:

| Setting | Does |
|---|---|
| map direct memory for real | on: a guest's direct-memory reservations are mapped and return addresses. Off: the calls answer unimplemented and the guest gets no address. |

It is saved in `config.toml` and applies to the next run.

## On the command line

```bash
orbistoun-cli load <module> --base <address>     # reserve the address space a module demands, without executing it
OOPS_LOG=orbistoun_mem=debug orbistoun-cli run <title>/eboot.bin   # log memory decisions during a run
```

`--base` places a module; modules link at zero and need one, and executables carry absolute
addresses and take the default `0`.

## Memory diagnostics

Each of these asks one question of a run by changing what the guest's memory holds. They are
off unless set; `orbistoun-cli env` lists them with their current values.

| Variable | Question it asks |
|---|---|
| `ORBISTOUN_STACK_FILL=<byte>` | does the run depend on stack memory nobody wrote? |
| `ORBISTOUN_HEAP_FILL=<byte>` | the same for every heap allocation |
| `ORBISTOUN_BSS_FILL=<byte>` | the same for `.bss` globals |
| `ORBISTOUN_DIRECT_FILL=<byte>` | the same for direct-memory mappings |
| `ORBISTOUN_HEAP_BASE=default\|<addr>` | does the run depend on where the host put the heap? |
| `ORBISTOUN_MAP=<addr>[+len]` | reserve a range before entry: does a fault there become a region the guest wanted? |
| `ORBISTOUN_MAP_SHAPE=whole\|reserved-low\|fragmented` | which physical map shape the guest is shown |
| `ORBISTOUN_POKE=<addr>:<value>` | write a value before entry: does the fault follow it? |
| `ORBISTOUN_PEEK=caller\|<addr>[+len]` | hex-dump guest memory at a fault, at the faulting call site or an address |
