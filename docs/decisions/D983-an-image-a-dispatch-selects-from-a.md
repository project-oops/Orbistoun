# D983 - An image a dispatch selects from a table at run time is read from an array of every image the table names

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A dispatch's image access whose eight descriptor registers hold a record the program selected from
a table at run time - `s_buffer_load_dwordx8` through the table's traced descriptor at an offset
register the program computes, the words then read in order - binds every image the table names.
The tracer follows the eight registers from the load through moves and across blocks, as it follows
a selected buffer record's four (D966). Every access selecting from one table through one sampler,
in one shape, shares a texture slot. The slot's own binding holds its sampler alone. Its images lie
in two arrays of sampled images, one of flat images and one of cubes, each
`MOST_SELECTED_IMAGES` (4096) long, every image at the same place in both.

**The host.** The command processor resolves the table's descriptor from the user data and reads
every 32-byte record its range holds, at most 65536. A record of zeros, or with a base of zero,
names nothing. Each distinct descriptor of a flat image or a cube is read as a fetched image is, by
its own words, its blocks decoded where it is compressed. A record whose image cannot be bound - an
integer format, a layout not modelled, another type - is left out and counted. The images go into
the arrays in order, and a directory of their descriptors goes beside them: a power-of-two table of
slots holding a descriptor's eight words and its place plus one, the cube bit set for a cube, each
descriptor within four slots of its hash's. More distinct images than the arrays hold, or more than
a gibibyte of texels, refuse the dispatch with what its tables hold. The slot's sampler is read from
the words the tracer found for it - constants, user data or loads at fixed places. A sample through a
sampler the program computes from what the host cannot know is refused by name.

**The module.** At the access, the module reads the eight registers, hashes them as the host did,
and compares each probed slot word by word. A hit in the access's shape gives the place. The image
is loaded from the array at that place and paired with the slot's sampler (`OpSampledImage`), the
index and every value read through it decorated `NonUniform`
(`SampledImageArrayNonUniformIndexing`). Words the directory does not hold, or holds in the other
shape, are an escape (D950): the access reads the first place, and the dispatch's result is refused.
A compare is made in the module (D976) by the function and filter read from the slot's sampler.

**Samplers built in place.** The tracer runs a scalar ALU instruction whose every register source
holds a constant through the translator's own semantics (`evaluate::fold_scalar`, as D753 runs a
prefix), and its destination holds the constant the run leaves. A program builds a sampler so, with
`s_bfm_b32`, `s_brev_b32`, `s_bitreplicate_b64_b32` and 64-bit moves of literals, and the slot's
sampler is then read from those words. No rule is written per instruction. A buffer load of an image
descriptor at an offset register is no longer taken for the fixed load at its immediate (D934).

**Why:** Grand Theft Auto V's ray-traversal passes select the material texture a hit uses in a
waterfall: the program at `0x74005b854a00` loads `s[8:15]` with `s_buffer_load_dwordx8 s[8:15],
s[40:43], vcc_lo` at `0x163c`, the offset a lane's material shifted by five, and samples its alpha at
`0x1648`. The program at `0x74005b858800` does the same through `s[24:27]` at six places, and selects
a light's shadow map, flat and cube, at `0x28a0` and `0x2948`. Both were prepared device-addressed
(D964, D982) and refused at the image: running the program up to the access (D960) cannot follow
the waterfall's branch on the execution mask, and cannot know the lane's value it does reach.

**Measured** on Grand Theft Auto V, a 480 s run. Both programs translate device-addressed from their
bytes: `0x74005b854a00` with one selecting slot, `0x74005b858800` with seven, every one reading the
table at `+0x10` of the table its first two user-data words name. The table of `0x74005b854a00`
holds 65536 records. 329 name an image, 288 of them distinct, in twelve formats, 153 of them in the
two most common (formats 169 and 173). Decoded to texels on the host, as every dispatch image is, the
first 268 took more than a gibibyte, and each of its 14 dispatches spent 2.2 to 2.4 s in preparation
before the bound refused it. The bound is now checked against the descriptors' base levels before
any image is read, so such a dispatch is refused without the cost. `0x74005b858800` was refused at
its shadow compares (`fetched image 12`). Their samplers are not read from the light record the
program selects per lane: it builds both before its loop over lights, from constants, in `s[44:47]`
(`s_movk_i32`, `s_bfm_b32`, `s_mov_b32`, `s_brev_b32` at `0x2670` to `0x269c`) and `s[48:51]`
(`s_bitreplicate_b64_b32`, `s_mov_b64`), and the loop writes neither. The trace had stopped at the
bit reversal and the replication. Folded, both are traced: compare function 3, filtered, for the flat
map at `0x299c`, and the cube's at `0x291c`. Every one of the program's seven selecting slots now
has its sampler. RUN2

**The tests.** `selected_images.rs` in the backend runs the waterfall over four records on the
device, each lane's record its id's low two bits, and checks every lane's texel, fetched and sampled.
A directory missing one record makes those lanes escape, and a directory with its places reordered
still reads each lane's own image. `every_image_a_table_names_is_bound_and_found_by_its_descriptor`
reads a table of six records - four images, a null record and a repeat - and finds each image's
texels at the place its descriptor's directory slot names. A depth-compare sample of a selected image
compares in the module by each of the eight functions, point and filtered, every lane's answer
checked (`each_lane_compares_the_image_it_selected_by_every_function`), and the host reads a
selected compared slot's function and filter from its traced sampler. Both of the shadow compares'
sampler forms trace to their words (`a_sampler_built_from_constants_is_traced`). The guest programs at `0x74005b854a00`
and `0x74005b858800` translate device-addressed from their bytes with their selecting accesses in
slots of their own (`ORBISTOUN_DEVICE_PROGRAMS`).

**Known limits.** The images are read, decoded and uploaded for every dispatch; nothing keeps them
on the device between dispatches. The measurement says this is the next wall: a table naming
hundreds of block-compressed textures needs them kept on the device in their own formats, and
read again only where the write-watch says their bytes changed. A sampled image binds its base level, as every dispatch image does
(D833), so a sample at a coarser level reads the base level's texels. A capture holds no selected
images, so a dispatch reading them is not replayed. A sampler the program computes from what the
host cannot know is refused by name; no program met so far does.

**Rejected:**
- Fetching texels from guest memory in the module, detiling as it reads: every swizzle mode, every
  block-compressed format, compression keys, filtering and level selection written again in SPIR-V,
  beside the host decoders that already read them.
- Specialising a module per group: the record is chosen per lane by a waterfall, not per group, and
  a module per combination of records is a module per frame.
- Indexing the arrays by the record's position, from the load's offset: the offset would have to be
  carried from the load to every access through register copies and branches. The descriptor's own
  words are at the access, and comparing all eight checks that the bound image is the one the
  registers name, whatever reached them.
- Runtime-sized, partially bound descriptor arrays: fixed arrays whose other places hold a filler
  need neither feature, and every module reading selected images binds alike.
- Binding the images as combined image samplers in one array per slot: a slot's sampler is the
  slot's, and the images would be bound once per slot.
- A sampler array with a directory of its own, the samplers a table can name all bound: the shadow
  compares it was meant for build their samplers from constants, which the trace now reads exactly.
  A sampler table has no program to read its records from or to be tested against. It waits for a
  program that selects its sampler at run time, which then says where the candidates lie.
- A rule per instruction for the forms a sampler is built with (`s_bfm`, `s_brev`, ...): the
  translator's semantics already run them, and untraced forms keep arriving (D960).
