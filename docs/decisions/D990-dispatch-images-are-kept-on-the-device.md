# D990 - Dispatch images are kept on the device between dispatches

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A compute dispatch's images stay on the device after it runs. Each is kept under a key: where its
bytes lie, and how they are read - the surface's layout, the texel format and channel order, the
layers from the first read, and whether it is seen as a cube. The descriptor's other words are not
part of the key, so an image one dispatch stores to through a storage descriptor is the image the
next fetches through a sampled one. A later dispatch that binds the same image, its bytes
unwritten since, takes it as it was left: nothing is read from guest memory, detiled or uploaded.
This holds for fetched images, stored images, block-compressed images and the images a program
selects from tables at run time (D983), which reference kept images like any other.

**Coherence** is the host's write-watch (D824, D984), as a texture's is:

- *Kept.* An image read for a dispatch is kept from a write-watch mark taken before its bytes
  were read, once the dispatch has run. An image a dispatch stored to is kept from a mark taken
  after the command processor wrote it back, when guest memory holds exactly what the device
  holds. Write-back still happens at every dispatch: kept images are written through.
- *Taken.* A preparation first settles an image's bytes as any read does: a frame pending over
  them is written back and a deferred copy is carried out (D714, D719). Each of those is a write
  the mark sees. It then takes the image only if nothing has written its bytes since the mark.
  The device's own stores into guest memory are noted written where they land (D964, D974).
- *Written while it ran.* A stored image whose bytes something else wrote while its dispatch ran
  is not kept, because the write-back would hide that write from the mark taken after it.
- *Not yet run.* An image read for a dispatch that has not run is not taken. A preparation made
  twice reads it twice. A dispatch that fails lets every image it bound go.
- *Let go.* The command processor's registry decides what is kept, not the device. Past a budget
  of 5 GiB the least recently bound images go first, never one bound by the dispatch being
  prepared (3 GiB, after 5 GiB took the device to 16.08 GiB of a 16 GB card; see below). The
  device is told which keys to drop before the next dispatch. A device asked for an
  image it does not hold refuses the dispatch. The command processor then lets every image go and
  prepares the dispatch again with its images read afresh.
- *Diagnostics.* A capture (`ORBISTOUN_CAPTURE_DISPATCH`, `ORBISTOUN_CAPTURE_RECENT`) and an image
  dump (`ORBISTOUN_DUMP_IMAGE`) copy each dispatch's texels, which a taken image does not carry, so
  images are not kept while one is set.

**Blocks stay blocks.** A fetched block-compressed image whose format the host holds as it is
(`BC1` to `BC7`) is bound as its blocks, detiled from the guest's layout into rows of blocks and
sampled by the device, rather than decoded to `8_8_8_8_UNORM` texels on the host. That is a
quarter to an eighth of the bytes. A kept block image is held once, on the device: the buffer its
blocks travelled through is let go, and a refill makes one again. A format the host does not hold,
or one whose selects move channels, is decoded as before. The selected images' size limit counts
a block image by its blocks and allows 4 GiB for one table: the images persist across dispatches
and frames, so the limit is a share of the kept budget, not a per-dispatch upload.

An image kept is made for every role it may be bound in (stored, fetched, uploaded, read back)
where its format can be stored to on the device. A block image is only ever fetched. Taken in
another role than it was left in, it is moved to the new layout with its contents preserved.

**Why:** Grand Theft Auto V's in-world frames bound about 200 dispatch images a flip, and nearly
all were the memory they had been at the previous dispatch: a pass's output is the next pass's
input, and a texture table serves every frame. One build, run twice, its images kept and not
(an image dump at an address no image has turns the keeping off). Measured with the command
processor sampled, from the first in-world window, per flip:

| | not kept | kept |
|---|---|---|
| seconds a flip | 12.47 (0.080 a second) | 11.24 (0.089 a second) |
| dispatches, all told | 6.85 s | 5.48 s |
| dispatch preparation | 2.91 s | 2.06 s |
| dispatch on the device | 1.65 s | 1.20 s |
| device setup / release | 0.46 / 0.35 s | 0.20 / 0.21 s |
| images made afresh | 106 | 34 |
| images taken / read | - | 184.5 / 16.2 (92% taken) |

The run without kept images may have shared the device with another session's run for part of its
time.

**Blocks**, measured in the next build (same method). Grand Theft Auto V's ray-tracing programs
`0x74005b854a00` and `0x74005b858800` select their images from one table of 65,536 records. Its
3,000-odd images took 5.8-6.0 GiB as decoded texels, past the selected images' limit, so both
programs were refused after 2.0-2.4 s of preparation each frame. As blocks the table takes
0.63-0.76 GiB:

- `0x74005b854a00` now runs. Its preparation takes 0.89 s on average (112 dispatches) and its
  device time is under 0.5 s, apart from the first uploads of the table (0.84-1.86 s).
- `0x74005b858800` still stops, after 0.83 s, at a buffer descriptor computed from what the host
  cannot know (D753). Its refusal is not this decision's.
- Per flip, 12,218 image places were taken from the device and 254 read; the dispatch-image
  share of device memory reached 4.98 GiB under the 5 GiB budget then in force.

A flip took 12.44 s against 11.24 s without the blocks, because a ray-traced pass now runs every
frame that was refused before. The table's per-dispatch preparation is now the cost to cut: about
3,000 placements a dispatch, each a settle and a write-watch question.

**Rejected:**
- Keying a kept image by its whole descriptor: a storage view and a sampled view of one image
  differ in words that do not change its bytes, so the producer-consumer chain the keeping exists
  for would never match.
- Letting the device evict on its own: the command processor would take an image the device had
  dropped. The registry decides, and the device refuses only after a loss.
- Guarding kept images' pages read-only (D824): the command processor writes stored images back at
  every dispatch, so each write would fault and churn the guard. The write-watch alone answers.
- Deferring stored images' write-back until a reader asks: it removes the per-dispatch readback and
  tiling as well. It needs deferred-copy entries (D719) for device images and a residency check
  that does not settle the image it would take. It is left for a decision of its own.
